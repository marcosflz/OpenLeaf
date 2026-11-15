"""Graba una fixture de eventos del agente real con un modelo guionizado.

    cd python && uv run python tools/record_fixture.py ../fixtures/agent/events/agent-fix-edit.jsonl

Escenario: modo ``latex-fix`` → ``read`` → ``edit`` con permiso (aprobado
``once``) → respuesta final en streaming. El formato de línea es el de
``SseRecord`` (``{"event", "data"}``), el mismo que reproduce ``cargo xtask
chat-replay`` y que consumen los tests de ``lsx-chat``.
"""

from __future__ import annotations

import asyncio
import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT), str(ROOT / "tests")]

from conftest import Harness, call, say  # noqa: E402


async def record(out: Path) -> int:
    with tempfile.TemporaryDirectory() as tmp:
        h = Harness(Path(tmp))
        (h.project / "main.tex").write_text(
            "\\documentclass{article}\n\\begin{document}\n\\textbf{Hola\n\\end{document}\n",
            encoding="utf-8",
        )
        sid = await h.session()
        h.model.script = [
            call("read", "call_read", path="main.tex"),
            call(
                "edit",
                "call_edit",
                path="main.tex",
                old_text="\\textbf{Hola\n",
                new_text="\\textbf{Hola}\n",
            ),
            say("Cerré la llave de `\\textbf` en `main.tex:3`; ya compila."),
        ]
        await h.prompt(sid, "Arregla los errores de compilación", agent="latex-fix")
        asked = await h.transport.wait_event("permission.asked")
        await h.rpc("permission.reply", sessionID=sid, requestID=asked["id"], reply="once")
        await h.finish(sid)
        await h.server.close()
        # Sin rutas personales en el repo: el proyecto temporal pasa a C:\proyecto.
        real = json.dumps(str(h.project.resolve()))[1:-1]
        fake = json.dumps("C:\\proyecto")[1:-1]
        lines = [
            json.dumps({"event": m["type"], "data": m}, ensure_ascii=False).replace(real, fake)
            for m in h.transport.sent
            if "type" in m
        ]
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write("\n".join(lines) + "\n")
    return len(lines)


if __name__ == "__main__":
    target = Path(sys.argv[1] if len(sys.argv) > 1 else "agent-fix-edit.jsonl")
    count = asyncio.run(record(target))
    print(f"{count} eventos -> {target}")
