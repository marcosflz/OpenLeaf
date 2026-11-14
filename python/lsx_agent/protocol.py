"""Transporte NDJSON por stdio entre la app (Rust) y el agente.

Cada línea es un objeto JSON UTF-8. Hay tres formas:

- **Petición** (app → agente): ``{"id": 7, "method": "session.create", "params": {...}}``.
- **Respuesta** (agente → app): ``{"id": 7, "result": ...}`` o
  ``{"id": 7, "error": {"message": "..."}}``.
- **Evento** (agente → app): ``{"type": "message.part.updated", "properties": {...}}``,
  con la forma de la API v1 de opencode (tabla I-4 de ``lsx-chat``).

stdout queda reservado al protocolo: los logs van a stderr.
"""

from __future__ import annotations

import asyncio
import json
import sys
import threading
from typing import Any, BinaryIO


class Transport:
    """Lee peticiones de stdin y escribe respuestas/eventos en stdout."""

    def __init__(self, reader: BinaryIO | None = None, writer: BinaryIO | None = None) -> None:
        self._reader = reader if reader is not None else sys.stdin.buffer
        self._writer = writer if writer is not None else sys.stdout.buffer
        self._lock = threading.Lock()
        self._queue: asyncio.Queue[dict[str, Any] | None] | None = None

    def start(self, loop: asyncio.AbstractEventLoop) -> None:
        """Lanza el hilo lector (las pipes de Windows no admiten lectura async)."""
        self._queue = asyncio.Queue()
        queue = self._queue

        def pump() -> None:
            for raw in self._reader:
                line = raw.decode("utf-8", errors="replace").strip()
                if not line:
                    continue
                try:
                    message = json.loads(line)
                except json.JSONDecodeError:
                    self.send({"type": "protocol.error", "properties": {"line": line[:200]}})
                    continue
                if isinstance(message, dict):
                    loop.call_soon_threadsafe(queue.put_nowait, message)
            loop.call_soon_threadsafe(queue.put_nowait, None)

        threading.Thread(target=pump, name="lsx-agent-stdin", daemon=True).start()

    async def receive(self) -> dict[str, Any] | None:
        """Siguiente petición; ``None`` cuando la app cierra stdin."""
        if self._queue is None:
            raise RuntimeError("Transport.start() no se ha llamado")
        return await self._queue.get()

    def send(self, message: dict[str, Any]) -> None:
        data = json.dumps(message, ensure_ascii=False, separators=(",", ":"), default=str)
        with self._lock:
            self._writer.write(data.encode("utf-8") + b"\n")
            self._writer.flush()

    def event(self, event_type: str, properties: dict[str, Any]) -> None:
        self.send({"type": event_type, "properties": properties})

    def result(self, request_id: Any, result: Any) -> None:
        self.send({"id": request_id, "result": result})

    def error(self, request_id: Any, message: str) -> None:
        self.send({"id": request_id, "error": {"message": message}})
