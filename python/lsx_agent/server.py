"""Servidor NDJSON: sesiones, turnos del agente, permisos y revert.

Cada método RPC tiene el mismo nombre conceptual que el endpoint de opencode
al que sustituye (``session.create`` ↔ ``POST /session``, etc.). Los turnos
corren como tareas asyncio y publican eventos ``message.*``, ``permission.*``
y ``session.*`` mientras avanzan.
"""

from __future__ import annotations

import asyncio
import json
import logging
import sys
import uuid
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from nas.adk import Agent, Runner
from nas.adk.core.callbacks import BaseCallback
from nas.adk.core.events.event import Event
from nas.adk.core.interrupts import ResumeEntry, ToolDecision
from nas.adk.models.base_llm import BaseLlm
from nas.adk.sessions.sqlite_session_service import SQLiteSessionService
from nas.adk.version import __version__ as nas_version

from . import __version__, prompts, providers
from .protocol import Transport
from .store import Store, now_ms
from .tools import WRITE_TOOLS, Workspace, build_tools, unified_diff

log = logging.getLogger("lsx_agent")

APP_NAME = "latex-studio"
USER_ID = "local"
TOOL_OUTPUT_CHARS = 4000

ModelFactory = Callable[[str, str], BaseLlm]


def new_id(prefix: str) -> str:
    return f"{prefix}_{uuid.uuid4().hex[:20]}"


@dataclass
class Turn:
    """Estado de un turno en curso (de un prompt hasta ``session.idle``)."""

    session_id: str
    message_id: str
    workspace: Workspace
    provider_id: str
    model_id: str
    text_part: dict[str, Any] | None = None
    reasoning_part: dict[str, Any] | None = None
    tools: dict[str, dict[str, Any]] = field(default_factory=dict)
    diffs: dict[str, str] = field(default_factory=dict)
    pending: dict[str, asyncio.Future[str]] = field(default_factory=dict)
    task: asyncio.Task[None] | None = None


class PermissionPolicy(BaseCallback):
    """Pide permiso antes de cada escritura salvo que la sesión lo concediera «siempre»."""

    def __init__(self, server: AgentServer, turn: Turn) -> None:
        self.server = server
        self.turn = turn

    async def before_tool_call(
        self, tool_name: str, args: dict[str, Any], tool_call_id: str = ""
    ) -> ToolDecision | None:
        if tool_name not in WRITE_TOOLS:
            return None
        ws = self.turn.workspace
        try:
            target, before, after = ws.preview(tool_name, args)
        except Exception:  # noqa: BLE001 — la tool repetirá la validación
            return None  # la tool fallará con el mismo error y el modelo lo verá
        rel = ws.rel(target)
        diff = unified_diff(rel, before, after)
        self.turn.diffs[tool_call_id] = diff
        if "edit" in self.server.always.get(self.turn.session_id, set()):
            return None
        return ToolDecision.interrupt(
            "approval",
            {"permission": "edit", "path": rel, "filepath": str(target), "diff": diff},
        )


class AgentServer:
    def __init__(
        self,
        transport: Transport,
        data_dir: Path,
        secrets: providers.Secrets,
        skills_dir: Path | None = None,
        model_factory: ModelFactory | None = None,
    ) -> None:
        self.transport = transport
        data_dir.mkdir(parents=True, exist_ok=True)
        self.store = Store(data_dir / "transcript.db")
        self.history = SQLiteSessionService(str(data_dir / "history.db"))
        self.secrets = secrets
        self.skills_dir = skills_dir
        self.model_factory = model_factory or (
            lambda provider, model: providers.build_model(provider, model, secrets)
        )
        self.turns: dict[str, Turn] = {}
        self.always: dict[str, set[str]] = {}
        self._models: dict[tuple[str, str], BaseLlm] = {}

    # --- bucle principal --------------------------------------------------------------

    async def serve(self) -> None:
        self.transport.start(asyncio.get_running_loop())
        self.transport.event("server.connected", {})
        while True:
            message = await self.transport.receive()
            if message is None:
                break
            await self.dispatch(message)
        await self.close()

    async def close(self) -> None:
        for turn in list(self.turns.values()):
            if turn.task is not None:
                turn.task.cancel()
        for turn in list(self.turns.values()):
            if turn.task is not None:
                await asyncio.gather(turn.task, return_exceptions=True)
        for model in self._models.values():
            await model.aclose()
        self.history.close()
        self.store.close()

    async def dispatch(self, message: dict[str, Any]) -> None:
        request_id = message.get("id")
        method = str(message.get("method", ""))
        params = message.get("params") or {}
        handler = getattr(self, "rpc_" + method.replace(".", "_"), None)
        if handler is None:
            self.transport.error(request_id, f"método desconocido: {method}")
            return
        try:
            result = await handler(**params)
        except Exception as error:
            log.exception("rpc %s", method)
            self.transport.error(request_id, str(error) or type(error).__name__)
            return
        self.transport.result(request_id, result)

    # --- RPC: servidor y catálogos ---------------------------------------------------------

    async def rpc_hello(self) -> dict[str, Any]:
        return {"healthy": True, "version": __version__, "nasAdk": nas_version}

    async def rpc_config_providers(self) -> dict[str, Any]:
        return providers.catalog(self.secrets)

    async def rpc_provider_status(self) -> list[dict[str, Any]]:
        return providers.status(self.secrets)

    async def rpc_agent_list(self) -> list[dict[str, Any]]:
        return prompts.AGENTS

    # --- RPC: sesiones ----------------------------------------------------------------------

    async def rpc_session_list(self, directory: str | None = None) -> list[dict[str, Any]]:
        return [row.info() for row in self.store.sessions(directory)]

    async def rpc_session_get(self, id: str) -> dict[str, Any]:
        return self._session(id).info()

    async def rpc_session_create(self, title: str = "", directory: str = ".") -> dict[str, Any]:
        root = Path(directory).expanduser().resolve(strict=True)
        session_id = new_id("ses")
        row = self.store.create_session(session_id, title or "Sesión nueva", str(root))
        await asyncio.to_thread(self.history.create_session, APP_NAME, USER_ID, session_id)
        self.transport.event("session.created", {"sessionID": session_id, "info": row.info()})
        return row.info()

    async def rpc_session_delete(self, id: str) -> bool:
        turn = self.turns.get(id)
        if turn is not None and turn.task is not None:
            turn.task.cancel()
            await asyncio.gather(turn.task, return_exceptions=True)
        deleted = self.store.delete_session(id)
        try:
            await asyncio.to_thread(
                self.history.delete_session, id, app_name=APP_NAME, user_id=USER_ID
            )
        except (KeyError, PermissionError, RuntimeError):
            pass
        self.always.pop(id, None)
        self.transport.event("session.deleted", {"sessionID": id})
        return deleted

    async def rpc_session_messages(self, id: str, limit: int = 200) -> list[dict[str, Any]]:
        self._session(id)
        return self.store.history(id, limit)

    async def rpc_session_revert(self, id: str, messageID: str) -> dict[str, Any]:
        self._ensure_idle(id)
        touched = self.store.revert(id, messageID)
        for path in touched:
            self.transport.event("file.edited", {"file": path})
        return self._session(id).info()

    async def rpc_session_unrevert(self, id: str) -> dict[str, Any]:
        self._ensure_idle(id)
        touched = self.store.unrevert(id)
        for path in touched:
            self.transport.event("file.edited", {"file": path})
        return self._session(id).info()

    # --- RPC: turnos ------------------------------------------------------------------------

    async def rpc_session_prompt(
        self,
        id: str,
        text: str,
        model: dict[str, str] | None = None,
        agent: str = "build",
    ) -> bool:
        session = self._session(id)
        self._ensure_idle(id)
        if not model or not model.get("providerID") or not model.get("modelID"):
            raise ValueError("no hay modelo seleccionado; configura un proveedor en Ajustes › IA")
        workspace = Workspace(
            Path(session.directory),
            store=self.store,
            session_id=id,
            message_id=new_id("msg"),
            skills_dir=self.skills_dir,
        )
        turn = Turn(
            session_id=id,
            message_id=workspace.message_id,
            workspace=workspace,
            provider_id=model["providerID"],
            model_id=model["modelID"],
        )
        self.turns[id] = turn
        turn.task = asyncio.create_task(self._run_turn(turn, text, agent))
        return True

    async def rpc_session_abort(self, id: str) -> bool:
        turn = self.turns.get(id)
        if turn is None or turn.task is None:
            return False
        turn.task.cancel()
        return True

    async def rpc_permission_reply(self, sessionID: str, requestID: str, reply: str) -> bool:
        turn = self.turns.get(sessionID)
        future = turn.pending.get(requestID) if turn else None
        if future is None or future.done():
            raise ValueError("no hay un permiso pendiente con ese id")
        if reply not in ("once", "always", "reject"):
            raise ValueError("reply debe ser once, always o reject")
        future.set_result(reply)
        return True

    # --- ejecución de un turno -----------------------------------------------------------------

    async def _run_turn(self, turn: Turn, text: str, agent_name: str) -> None:
        session_id = turn.session_id
        self._status(session_id, "busy")
        cut = self.store.commit_revert(session_id)
        if cut and cut.get("nasTimestamp"):
            await asyncio.to_thread(self._truncate_history, session_id, cut["nasTimestamp"])
        user_id = new_id("msg")
        user_info = {
            "id": user_id,
            "sessionID": session_id,
            "role": "user",
            "time": {"created": now_ms(), "completed": now_ms()},
        }
        self._message(session_id, user_info)
        self._part(
            session_id,
            {
                "id": new_id("prt"),
                "messageID": user_id,
                "sessionID": session_id,
                "type": "text",
                "text": text,
                "time": {"start": now_ms(), "end": now_ms()},
            },
        )
        assistant = {
            "id": turn.message_id,
            "sessionID": session_id,
            "role": "assistant",
            "providerID": turn.provider_id,
            "modelID": turn.model_id,
            "time": {"created": now_ms()},
        }
        self._message(session_id, assistant)
        self._maybe_title(session_id, text)
        error: str | None = None
        try:
            model = self._model(turn.provider_id, turn.model_id)
            project_notes = turn.workspace.root / "AGENTS.md"
            agent = Agent(
                name="latex",
                model=model,
                instruction=prompts.instruction(agent_name, project_notes),
                tools=build_tools(turn.workspace, read_only=agent_name == "ask"),
            )
            runner = Runner(
                app_name=APP_NAME,
                agent=agent,
                session_service=self.history,
                callbacks=[PermissionPolicy(self, turn)],
                max_context_tokens=120_000,
                max_tool_result_chars=16_000,
                max_model_calls=40,
                max_turns=30,
                timeout=900,
            )
            resume: list[ResumeEntry] | None = None
            message: str | None = text
            while True:
                interrupts: list[dict[str, Any]] = []
                async for event in runner.run_async(
                    USER_ID, session_id, message=message, stream=True, resume=resume
                ):
                    if event.role == "user" and message is not None:
                        user_info["nasTimestamp"] = event.timestamp.isoformat()
                        self.store.put_message(session_id, user_info)
                    elif event.type == "status":
                        if event.status == "interrupted":
                            interrupts = list(event.metrics.get("interrupts", []))
                        elif event.status in ("error", "budget_exhausted"):
                            error = event.content or event.status
                    else:
                        self._on_event(turn, event)
                if not interrupts:
                    break
                resume = await self._ask_permissions(turn, interrupts)
                message = None
        except asyncio.CancelledError:
            error = "Turno cancelado"
        except Exception as exc:
            log.exception("turno")
            error = str(exc) or type(exc).__name__
        finally:
            self._close_parts(turn, error)
            assistant["time"]["completed"] = now_ms()
            if error:
                assistant["error"] = {"name": "AgentError", "data": {"message": error}}
                self.transport.event(
                    "session.error",
                    {"sessionID": session_id, "error": {"data": {"message": error}}},
                )
            self._message(session_id, assistant)
            if turn.workspace.edited:
                self.transport.event(
                    "session.diff",
                    {"sessionID": session_id, "diff": sorted(set(turn.workspace.edited))},
                )
            self.store.touch(session_id)
            self.turns.pop(session_id, None)
            self._status(session_id, "idle")
            self.transport.event("session.idle", {"sessionID": session_id})

    def _on_event(self, turn: Turn, event: Event) -> None:
        sid = turn.session_id
        if event.type in ("stream", "reasoning"):
            kind = "reasoning" if event.type == "reasoning" else "text"
            part = turn.reasoning_part if kind == "reasoning" else turn.text_part
            if part is None:
                part = {
                    "id": new_id("prt"),
                    "messageID": turn.message_id,
                    "sessionID": sid,
                    "type": kind,
                    "text": "",
                    "time": {"start": now_ms()},
                }
                if kind == "reasoning":
                    turn.reasoning_part = part
                else:
                    if turn.reasoning_part is not None:
                        self._finish(sid, turn.reasoning_part)
                        turn.reasoning_part = None
                    turn.text_part = part
                self._part(sid, part)
            delta = event.content or ""
            part["text"] += delta
            self.transport.event(
                "message.part.delta",
                {
                    "sessionID": sid,
                    "messageID": turn.message_id,
                    "partID": part["id"],
                    "field": "text",
                    "delta": delta,
                },
            )
            return
        if event.role == "assistant":
            if turn.reasoning_part is not None:
                self._finish(sid, turn.reasoning_part)
                turn.reasoning_part = None
            content = event.content or ""
            if turn.text_part is None and content.strip():
                turn.text_part = {
                    "id": new_id("prt"),
                    "messageID": turn.message_id,
                    "sessionID": sid,
                    "type": "text",
                    "text": content,
                    "time": {"start": now_ms()},
                }
            if turn.text_part is not None:
                if content:
                    turn.text_part["text"] = content
                self._finish(sid, turn.text_part)
                turn.text_part = None
            for call in event.tool_calls:
                function = call.get("function", {})
                raw = function.get("arguments", "{}")
                try:
                    args = json.loads(raw) if isinstance(raw, str) else raw
                except json.JSONDecodeError:
                    args = {"raw": raw}
                name = function.get("name", "tool")
                part = {
                    "id": "prt_" + call["id"],
                    "messageID": turn.message_id,
                    "sessionID": sid,
                    "type": "tool",
                    "tool": name,
                    "callID": call["id"],
                    "state": {
                        "status": "running",
                        "input": args,
                        "title": _tool_title(name, args),
                        "time": {"start": now_ms()},
                    },
                }
                turn.tools[call["id"]] = part
                self._part(sid, part)
            return
        if event.role == "tool":
            part = turn.tools.get(event.tool_call_id)
            if part is None:
                return
            content = event.content or ""
            failure = _tool_error(content)
            state = part["state"]
            state["time"]["end"] = now_ms()
            if failure is not None:
                state["status"] = "error"
                state["error"] = failure
            else:
                state["status"] = "completed"
                state["output"] = content[:TOOL_OUTPUT_CHARS]
                diff = turn.diffs.get(event.tool_call_id)
                if diff:
                    state["metadata"] = {"diff": diff}
                if part["tool"] in WRITE_TOOLS:
                    path = state["input"].get("path", "")
                    self.transport.event("file.edited", {"file": str(turn.workspace.root / path)})
            self._part(sid, part)

    async def _ask_permissions(
        self, turn: Turn, interrupts: list[dict[str, Any]]
    ) -> list[ResumeEntry]:
        entries: list[ResumeEntry] = []
        loop = asyncio.get_running_loop()
        for item in interrupts:
            call_id = item["toolCallId"]
            payload = item.get("payload") or {}
            if not payload:
                # Llamada que quedó pendiente detrás de otra interrupción: se reintenta.
                entries.append(ResumeEntry(call_id, "approved"))
                continue
            if "edit" in self.always.get(turn.session_id, set()):
                entries.append(ResumeEntry(call_id, "approved"))
                continue
            future: asyncio.Future[str] = loop.create_future()
            turn.pending[call_id] = future
            self.transport.event(
                "permission.asked",
                {
                    "id": call_id,
                    "sessionID": turn.session_id,
                    "permission": payload.get("permission", "edit"),
                    "patterns": [payload.get("path", "")],
                    "always": [payload.get("path", "")],
                    "metadata": {"filepath": payload.get("filepath"), "diff": payload.get("diff")},
                    "tool": {"messageID": turn.message_id, "callID": call_id},
                },
            )
            try:
                reply = await future
            finally:
                turn.pending.pop(call_id, None)
            self.transport.event(
                "permission.replied",
                {
                    "sessionID": turn.session_id,
                    "requestID": call_id,
                    "reply": reply,
                },
            )
            if reply == "always":
                self.always.setdefault(turn.session_id, set()).add("edit")
            if reply == "reject":
                entries.append(
                    ResumeEntry(call_id, "denied", reason="La persona rechazó el cambio")
                )
            else:
                entries.append(ResumeEntry(call_id, "approved"))
        return entries

    # --- utilidades -------------------------------------------------------------------------

    def _session(self, session_id: str):
        row = self.store.session(session_id)
        if row is None:
            raise KeyError(f"sesión desconocida: {session_id}")
        return row

    def _ensure_idle(self, session_id: str) -> None:
        if session_id in self.turns:
            raise RuntimeError("hay un turno en curso; espera o pulsa Stop")

    def _model(self, provider: str, model: str) -> BaseLlm:
        key = (provider, model)
        if key not in self._models:
            self._models[key] = self.model_factory(provider, model)
        return self._models[key]

    def _truncate_history(self, session_id: str, timestamp: str) -> None:
        # nas-adk no expone truncado; se borra con su propia conexión y lock.
        with self.history._lock, self.history._conn:
            self.history._conn.execute(
                "DELETE FROM events WHERE session_id = ? AND timestamp >= ?",
                (session_id, timestamp),
            )

    def _status(self, session_id: str, status: str) -> None:
        self.transport.event(
            "session.status", {"sessionID": session_id, "status": {"type": status}}
        )

    def _message(self, session_id: str, info: dict[str, Any]) -> None:
        self.store.put_message(session_id, info)
        public = {k: v for k, v in info.items() if k != "nasTimestamp"}
        self.transport.event("message.updated", {"sessionID": session_id, "info": public})

    def _part(self, session_id: str, part: dict[str, Any]) -> None:
        self.store.put_part(session_id, part)
        self.transport.event("message.part.updated", {"sessionID": session_id, "part": part})

    def _finish(self, session_id: str, part: dict[str, Any]) -> None:
        part["time"]["end"] = now_ms()
        self._part(session_id, part)

    def _close_parts(self, turn: Turn, error: str | None) -> None:
        for part in (turn.reasoning_part, turn.text_part):
            if part is not None:
                self._finish(turn.session_id, part)
        turn.reasoning_part = turn.text_part = None
        for part in turn.tools.values():
            state = part["state"]
            if state["status"] in ("pending", "running"):
                state["status"] = "error"
                state["error"] = error or "No se ejecutó"
                state["time"]["end"] = now_ms()
                self._part(turn.session_id, part)

    def _maybe_title(self, session_id: str, text: str) -> None:
        """La sesión toma como título su primer prompt (como hacía opencode)."""
        entries = self.store.history(session_id)
        if sum(1 for entry in entries if entry["info"].get("role") == "user") != 1:
            return
        title = " ".join(text.split())[:60]
        if not title:
            return
        self.store.touch(session_id, title)
        updated = self._session(session_id)
        self.transport.event("session.updated", {"sessionID": session_id, "info": updated.info()})


def _tool_title(name: str, args: dict[str, Any]) -> str:
    if isinstance(args, dict):
        for key in ("path", "file", "text", "name", "package"):
            value = args.get(key)
            if isinstance(value, str) and value:
                return value if name != "compile" else f"{value} ({args.get('engine', 'pdflatex')})"
    return "proyecto" if name in ("compile", "list") else ""


def _tool_error(content: str) -> str | None:
    if not content.startswith("{"):
        return None
    try:
        data = json.loads(content)
    except json.JSONDecodeError:
        return None
    if isinstance(data, dict) and set(data) == {"error"}:
        return str(data["error"])
    return None


def configure_logging(log_file: Path | None) -> None:
    handler: logging.Handler
    if log_file is not None:
        log_file.parent.mkdir(parents=True, exist_ok=True)
        handler = logging.FileHandler(log_file, encoding="utf-8")
    else:
        handler = logging.StreamHandler(sys.stderr)
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s"))
    root = logging.getLogger()
    root.addHandler(handler)
    root.setLevel(logging.INFO)
