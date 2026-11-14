from __future__ import annotations

import asyncio
import json
from pathlib import Path

import pytest
from conftest import Harness, call, say

from lsx_agent.providers import Secrets, catalog


async def test_hello_and_agents(harness: Harness) -> None:
    hello = await harness.rpc("hello")
    assert hello["healthy"] is True and hello["version"]
    agents = await harness.rpc("agent.list")
    assert {a["name"] for a in agents} == {"build", "latex-fix", "ask"}


def test_catalog_lists_only_configured_providers(tmp_path: Path, monkeypatch) -> None:
    for var in ("OPENCODE_API_KEY", "OPENROUTER_API_KEY", "ANTHROPIC_API_KEY"):
        monkeypatch.delenv(var, raising=False)
    monkeypatch.setenv("CLAUDE_CODE_BIN", "claude-que-no-existe")
    assert catalog(Secrets(tmp_path / "nada.json"))["providers"] == []
    secrets = tmp_path / "ai.json"
    secrets.write_text(json.dumps({"opencode": {"apiKey": "oc_test"}}), encoding="utf-8")
    data = catalog(Secrets(secrets))
    assert [p["id"] for p in data["providers"]] == ["opencode"]
    assert data["default"] == {}
    assert "deepseek-v4.1-flash" in data["providers"][0]["models"]


async def test_prompt_streams_text_and_persists_history(harness: Harness) -> None:
    sid = await harness.session()
    harness.model.script = [say("El documento compila con pdflatex.")]
    await harness.prompt(sid, "¿Qué motor uso?")
    await harness.finish(sid)

    deltas = harness.transport.events("message.part.delta")
    assert "".join(d["delta"] for d in deltas) == "El documento compila con pdflatex."
    assert harness.transport.events("session.idle")[-1]["sessionID"] == sid
    roles = [m["info"]["role"] for m in harness.transport.events("message.updated")]
    assert roles[0] == "user" and "assistant" in roles

    history = await harness.rpc("session.messages", id=sid)
    assert [entry["info"]["role"] for entry in history] == ["user", "assistant"]
    text = next(p for p in history[1]["parts"] if p["type"] == "text")
    assert text["text"] == "El documento compila con pdflatex." and text["time"]["end"]
    # El primer prompt da título a la sesión.
    sessions = await harness.rpc("session.list", directory=str(harness.project.resolve()))
    assert sessions[0]["title"] == "¿Qué motor uso?"


async def test_edit_asks_permission_with_diff_and_applies_once(harness: Harness) -> None:
    sid = await harness.session()
    harness.model.script = [
        call("edit", path="main.tex", old_text="Hola mundo", new_text="Hola LaTeX"),
        say("Hecho."),
    ]
    await harness.prompt(sid, "Cambia el saludo")
    asked = await harness.transport.wait_event("permission.asked")
    assert asked["permission"] == "edit"
    assert asked["patterns"] == ["main.tex"]
    assert "-Hola mundo" in asked["metadata"]["diff"] and "+Hola LaTeX" in asked["metadata"]["diff"]
    # Aún no se ha escrito nada.
    assert "Hola mundo" in (harness.project / "main.tex").read_text(encoding="utf-8")

    await harness.rpc("permission.reply", sessionID=sid, requestID=asked["id"], reply="once")
    await harness.finish(sid)

    assert "Hola LaTeX" in (harness.project / "main.tex").read_text(encoding="utf-8")
    tool_parts = [
        e["part"]
        for e in harness.transport.events("message.part.updated")
        if e["part"]["type"] == "tool"
    ]
    assert tool_parts[-1]["state"]["status"] == "completed"
    assert "+Hola LaTeX" in tool_parts[-1]["state"]["metadata"]["diff"]
    assert harness.transport.events("permission.replied")[-1]["reply"] == "once"


async def test_rejected_edit_leaves_file_untouched(harness: Harness) -> None:
    sid = await harness.session()
    harness.model.script = [
        call("edit", path="main.tex", old_text="Hola mundo", new_text="Adiós"),
        say("Entendido, no lo cambio."),
    ]
    await harness.prompt(sid, "Cambia el saludo")
    asked = await harness.transport.wait_event("permission.asked")
    await harness.rpc("permission.reply", sessionID=sid, requestID=asked["id"], reply="reject")
    await harness.finish(sid)
    assert "Hola mundo" in (harness.project / "main.tex").read_text(encoding="utf-8")
    tool = [
        e["part"]
        for e in harness.transport.events("message.part.updated")
        if e["part"]["type"] == "tool"
    ][-1]
    assert tool["state"]["status"] == "error"


async def test_always_skips_later_prompts_in_session(harness: Harness) -> None:
    sid = await harness.session()
    harness.model.script = [
        call("edit", path="main.tex", old_text="Hola mundo", new_text="Uno"),
        say("ok"),
        call("edit", "call_2", path="main.tex", old_text="Uno", new_text="Dos"),
        say("ok"),
    ]
    await harness.prompt(sid, "primero")
    asked = await harness.transport.wait_event("permission.asked")
    await harness.rpc("permission.reply", sessionID=sid, requestID=asked["id"], reply="always")
    await harness.finish(sid)
    await harness.prompt(sid, "segundo")
    await harness.finish(sid)
    assert len(harness.transport.events("permission.asked")) == 1
    assert "Dos" in (harness.project / "main.tex").read_text(encoding="utf-8")


async def test_revert_and_unrevert_restore_files_and_history(harness: Harness) -> None:
    sid = await harness.session()
    harness.model.script = [
        call("write", path="cap1.tex", content="\\section{Uno}\n"),
        say("Creado."),
    ]
    await harness.prompt(sid, "Crea cap1")
    asked = await harness.transport.wait_event("permission.asked")
    await harness.rpc("permission.reply", sessionID=sid, requestID=asked["id"], reply="once")
    await harness.finish(sid)
    assert (harness.project / "cap1.tex").is_file()

    history = await harness.rpc("session.messages", id=sid)
    user_id = history[0]["info"]["id"]
    info = await harness.rpc("session.revert", id=sid, messageID=user_id)
    assert info["revert"]["messageID"] == user_id
    assert not (harness.project / "cap1.tex").exists()
    assert await harness.rpc("session.messages", id=sid) == []

    await harness.rpc("session.unrevert", id=sid)
    assert (harness.project / "cap1.tex").is_file()
    assert len(await harness.rpc("session.messages", id=sid)) == 2

    # Revert + prompt nuevo: lo revertido se descarta también para el modelo.
    await harness.rpc("session.revert", id=sid, messageID=user_id)
    harness.model.script = [say("Empezamos de cero.")]
    await harness.prompt(sid, "Otra cosa")
    await harness.finish(sid)
    history = await harness.rpc("session.messages", id=sid)
    assert [e["info"]["role"] for e in history] == ["user", "assistant"]
    last_request = harness.model.requests[-1]
    assert not any("Crea cap1" in str(m.get("content")) for m in last_request.messages)


async def test_ask_mode_has_no_write_tools(harness: Harness) -> None:
    sid = await harness.session()
    harness.model.script = [say("Solo leo.")]
    await harness.prompt(sid, "Explica", agent="ask")
    await harness.finish(sid)
    tools = {t["function"]["name"] for t in harness.model.requests[-1].tools or []}
    assert "read" in tools and "edit" not in tools and "compile" not in tools


async def test_abort_cancels_turn_and_closes_session(harness: Harness) -> None:
    sid = await harness.session()

    async def slow(request):
        await asyncio.sleep(30)

    original = harness.model.generate_content_async

    async def blocking(request):
        await asyncio.sleep(30)
        return await original(request)

    object.__setattr__(harness.model, "generate_content_async", blocking)
    await harness.prompt(sid, "tarda")
    await asyncio.sleep(0.05)
    assert await harness.rpc("session.abort", id=sid) is True
    await harness.finish(sid)
    errors = harness.transport.events("session.error")
    assert errors and "cancelado" in errors[-1]["error"]["data"]["message"]
    assert harness.transport.events("session.idle")


async def test_prompt_without_model_is_rejected(harness: Harness) -> None:
    sid = await harness.session()
    with pytest.raises(RuntimeError, match="proveedor"):
        await harness.rpc("session.prompt", id=sid, text="hola", model=None)


async def test_unknown_method_returns_error(harness: Harness) -> None:
    with pytest.raises(RuntimeError, match="desconocido"):
        await harness.rpc("no.existe")


async def test_probe_reports_unconfigured_provider(tmp_path, monkeypatch) -> None:
    from lsx_agent.__main__ import probe

    monkeypatch.delenv("OPENCODE_API_KEY", raising=False)
    result = await probe("opencode/glm-5.3-flash", Secrets(tmp_path / "nada.json"))
    assert result["ok"] is False and "no está configurado" in result["error"]
