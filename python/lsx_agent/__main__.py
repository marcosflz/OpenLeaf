"""Punto de entrada del agente.

    python -P -s -m lsx_agent --data-dir <dir> [--secrets <json>] [--skills <dir>] [--log <file>]
    python -P -s -m lsx_agent --probe <proveedor/modelo> --data-dir <dir> [--secrets <json>]

Sin ``--probe`` sirve el protocolo NDJSON por stdio. Con ``--probe`` hace una
petición mínima al modelo e imprime ``{"ok": true, "text": ...}`` o
``{"ok": false, "error": ...}`` (botón «Probar» de Ajustes › IA).
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys
from pathlib import Path

from .protocol import Transport
from .providers import Secrets, build_model
from .server import AgentServer, configure_logging


async def probe(target: str, secrets: Secrets) -> dict[str, object]:
    from nas.adk.models.base_llm import LlmRequest

    provider, _, model_id = target.partition("/")
    try:
        model = build_model(provider, model_id, secrets)
    except Exception as error:  # noqa: BLE001 — se informa a la UI
        return {"ok": False, "error": str(error)}
    try:
        response = await asyncio.wait_for(
            model.generate_content_async(
                LlmRequest(
                    model=model.model,
                    messages=[{"role": "user", "content": "Responde solo con la palabra: OK"}],
                    max_tokens=16,
                )
            ),
            timeout=80,
        )
        return {"ok": True, "text": (response.content or "").strip()[:200]}
    except Exception as error:  # noqa: BLE001
        return {"ok": False, "error": f"{type(error).__name__}: {error}"[:400]}
    finally:
        await model.aclose()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="lsx_agent", description="Agente LaTeX de LaTeX Studio")
    parser.add_argument("--data-dir", type=Path, required=True, help="sesiones y checkpoints")
    parser.add_argument("--secrets", type=Path, default=None, help="config/secrets/ai.json")
    parser.add_argument("--skills", type=Path, default=None, help="assets/skills")
    parser.add_argument("--log", type=Path, default=None, help="archivo de log (o stderr)")
    parser.add_argument("--probe", default=None, help="prueba proveedor/modelo y termina")
    args = parser.parse_args(argv)

    if args.probe:
        result = asyncio.run(probe(args.probe, Secrets(args.secrets)))
        sys.stdout.write(json.dumps(result, ensure_ascii=False) + "\n")
        return 0  # el resultado viaja en el JSON; un código ≠ 0 sería un fallo del proceso

    configure_logging(args.log)
    if sys.platform == "win32":
        # stdout binario sin traducción de saltos: el protocolo es NDJSON con \n.
        import msvcrt
        import os

        msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
    server = AgentServer(
        Transport(),
        data_dir=args.data_dir,
        secrets=Secrets(args.secrets),
        skills_dir=args.skills,
    )
    try:
        asyncio.run(server.serve())
    except KeyboardInterrupt:
        return 130
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
