from __future__ import annotations

import asyncio
import json
from collections.abc import AsyncGenerator, Callable
from pathlib import Path
from typing import Any, ClassVar

import pytest
from nas.adk.models.base_llm import BaseLlm, LlmRequest, LlmResponse, ToolCall

from lsx_agent.protocol import Transport
from lsx_agent.providers import Secrets
from lsx_agent.server import AgentServer


class ScriptedLlm(BaseLlm):
    """Modelo fake: devuelve respuestas guionizadas y registra las peticiones."""

    supports_streaming: ClassVar[bool] = True
    script: list[Any] = []  # noqa: RUF012
    requests: list[LlmRequest] = []  # noqa: RUF012

    async def generate_content_async(self, request: LlmRequest) -> LlmResponse:
        self.requests.append(request)
        if not self.script:
            return LlmResponse(content="(sin guion)")
        step = self.script.pop(0)
        return step(request) if callable(step) else step

    async def generate_stream_async(self, request: LlmRequest) -> AsyncGenerator[LlmResponse, None]:
        response = await self.generate_content_async(request)
        if response.content and not response.tool_calls:
            for chunk in (
                response.content[: len(response.content) // 2],
                response.content[len(response.content) // 2 :],
            ):
                yield LlmResponse(type="delta", content=chunk)
        yield response


def say(text: str) -> LlmResponse:
    return LlmResponse(content=text)


def call(name: str, call_id: str = "call_1", **arguments: Any) -> LlmResponse:
    return LlmResponse(
        tool_calls=[
            ToolCall(id=call_id, function={"name": name, "arguments": json.dumps(arguments)})
        ],
        finish_reason="tool_calls",
    )


class MemoryTransport(Transport):
    def __init__(self) -> None:
        super().__init__(reader=None, writer=None)  # type: ignore[arg-type]
        self.sent: list[dict[str, Any]] = []
        self.waiters: list[
            tuple[Callable[[dict[str, Any]], bool], asyncio.Future[dict[str, Any]]]
        ] = []

    def send(self, message: dict[str, Any]) -> None:
        self.sent.append(json.loads(json.dumps(message, default=str)))
        for predicate, future in list(self.waiters):
            if not future.done() and predicate(message):
                future.set_result(message)

    def events(self, event_type: str) -> list[dict[str, Any]]:
        return [m["properties"] for m in self.sent if m.get("type") == event_type]

    def results(self) -> list[dict[str, Any]]:
        return [m for m in self.sent if "id" in m and ("result" in m or "error" in m)]

    async def wait_event(self, event_type: str, timeout: float = 5.0) -> dict[str, Any]:
        for message in self.sent:
            if message.get("type") == event_type:
                return message["properties"]
        future: asyncio.Future[dict[str, Any]] = asyncio.get_running_loop().create_future()
        self.waiters.append((lambda m: m.get("type") == event_type, future))
        message = await asyncio.wait_for(future, timeout)
        return message["properties"]


class Harness:
    def __init__(self, tmp_path: Path) -> None:
        self.project = tmp_path / "proyecto"
        self.project.mkdir()
        (self.project / "main.tex").write_text(
            "\\documentclass{article}\n\\begin{document}\nHola mundo\n\\end{document}\n",
            encoding="utf-8",
        )
        self.transport = MemoryTransport()
        self.model = ScriptedLlm(model="fake")
        self.model.script = []
        self.model.requests = []
        self.server = AgentServer(
            self.transport,
            data_dir=tmp_path / "data",
            secrets=Secrets(None),
            skills_dir=None,
            model_factory=lambda provider, model: self.model,
        )
        self._next = 0

    async def rpc(self, method: str, **params: Any) -> Any:
        self._next += 1
        await self.server.dispatch({"id": self._next, "method": method, "params": params})
        reply = next(m for m in reversed(self.transport.sent) if m.get("id") == self._next)
        if "error" in reply:
            raise RuntimeError(reply["error"]["message"])
        return reply["result"]

    async def session(self) -> str:
        info = await self.rpc("session.create", title="Prueba", directory=str(self.project))
        return info["id"]

    async def prompt(self, session_id: str, text: str, agent: str = "build") -> None:
        await self.rpc(
            "session.prompt",
            id=session_id,
            text=text,
            agent=agent,
            model={"providerID": "fake", "modelID": "fake"},
        )

    async def finish(self, session_id: str) -> None:
        turn = self.server.turns.get(session_id)
        if turn is not None and turn.task is not None:
            await asyncio.wait_for(asyncio.shield(turn.task), 10)


@pytest.fixture
async def harness(tmp_path: Path) -> AsyncGenerator[Harness, None]:
    h = Harness(tmp_path)
    yield h
    await h.server.close()
