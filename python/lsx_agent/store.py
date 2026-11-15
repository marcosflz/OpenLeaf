"""Metadatos de sesiones, transcript visible y checkpoints de archivos.

nas-adk guarda el historial *del modelo* (``SQLiteSessionService``). Aquí se
guarda lo que ve la persona:

- **sesiones**: título, proyecto y marca de revert;
- **mensajes y partes**: el último snapshot de cada uno, con la forma de
  opencode (``{info, parts}``), para reconstruir el historial al reconectar;
- **checkpoints**: contenido antes/después de cada edición del agente, para
  revertir y deshacer el revert.
"""

from __future__ import annotations

import json
import sqlite3
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


def now_ms() -> int:
    return int(time.time() * 1000)


@dataclass
class SessionRow:
    id: str
    title: str
    directory: str
    created: int
    updated: int
    revert_message: str | None

    def info(self) -> dict[str, Any]:
        """Forma de ``SessionInfo`` de opencode (``protocol.rs``)."""
        data: dict[str, Any] = {
            "id": self.id,
            "title": self.title,
            "directory": self.directory,
            "time": {"created": self.created, "updated": self.updated},
        }
        if self.revert_message:
            data["revert"] = {"messageID": self.revert_message}
        return data


class Store:
    def __init__(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()
        self._conn = sqlite3.connect(str(path), check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        self._conn.executescript(
            """
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, directory TEXT NOT NULL,
                created INTEGER NOT NULL, updated INTEGER NOT NULL, revert_message TEXT);
            CREATE TABLE IF NOT EXISTS messages (
                seq INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
                id TEXT NOT NULL, info TEXT NOT NULL, UNIQUE(session_id, id));
            CREATE TABLE IF NOT EXISTS parts (
                seq INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
                message_id TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL,
                UNIQUE(session_id, id));
            CREATE TABLE IF NOT EXISTS checkpoints (
                seq INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
                message_id TEXT NOT NULL, path TEXT NOT NULL, before TEXT, after TEXT);
            """
        )
        self._conn.commit()

    def close(self) -> None:
        with self._lock:
            self._conn.close()

    # --- sesiones -------------------------------------------------------------------

    def create_session(self, session_id: str, title: str, directory: str) -> SessionRow:
        stamp = now_ms()
        with self._lock, self._conn:
            self._conn.execute(
                "INSERT INTO sessions(id, title, directory, created, updated) VALUES(?,?,?,?,?)",
                (session_id, title, directory, stamp, stamp),
            )
        return SessionRow(session_id, title, directory, stamp, stamp, None)

    def session(self, session_id: str) -> SessionRow | None:
        with self._lock:
            row = self._conn.execute(
                "SELECT * FROM sessions WHERE id = ?", (session_id,)
            ).fetchone()
        return _session(row) if row else None

    def sessions(self, directory: str | None = None) -> list[SessionRow]:
        with self._lock:
            if directory is None:
                rows = self._conn.execute("SELECT * FROM sessions ORDER BY updated DESC")
            else:
                rows = self._conn.execute(
                    "SELECT * FROM sessions WHERE directory = ? ORDER BY updated DESC",
                    (directory,),
                )
            return [_session(row) for row in rows.fetchall()]

    def touch(self, session_id: str, title: str | None = None) -> None:
        with self._lock, self._conn:
            if title is None:
                self._conn.execute(
                    "UPDATE sessions SET updated = ? WHERE id = ?", (now_ms(), session_id)
                )
            else:
                self._conn.execute(
                    "UPDATE sessions SET updated = ?, title = ? WHERE id = ?",
                    (now_ms(), title, session_id),
                )

    def delete_session(self, session_id: str) -> bool:
        with self._lock, self._conn:
            for table in ("parts", "messages", "checkpoints"):
                self._conn.execute(f"DELETE FROM {table} WHERE session_id = ?", (session_id,))
            cursor = self._conn.execute("DELETE FROM sessions WHERE id = ?", (session_id,))
            return cursor.rowcount > 0

    # --- transcript -----------------------------------------------------------------

    def put_message(self, session_id: str, info: dict[str, Any]) -> None:
        data = json.dumps(info, ensure_ascii=False)
        with self._lock, self._conn:
            self._conn.execute(
                "INSERT INTO messages(session_id, id, info) VALUES(?,?,?) "
                "ON CONFLICT(session_id, id) DO UPDATE SET info = excluded.info",
                (session_id, info["id"], data),
            )

    def put_part(self, session_id: str, part: dict[str, Any]) -> None:
        data = json.dumps(part, ensure_ascii=False)
        with self._lock, self._conn:
            self._conn.execute(
                "INSERT INTO parts(session_id, message_id, id, data) VALUES(?,?,?,?) "
                "ON CONFLICT(session_id, id) DO UPDATE SET data = excluded.data",
                (session_id, part["messageID"], part["id"], data),
            )

    def message_info(self, session_id: str, message_id: str) -> dict[str, Any] | None:
        with self._lock:
            row = self._conn.execute(
                "SELECT info FROM messages WHERE session_id = ? AND id = ?",
                (session_id, message_id),
            ).fetchone()
        return json.loads(row["info"]) if row else None

    def history(self, session_id: str, limit: int | None = None) -> list[dict[str, Any]]:
        """``[{info, parts}]`` visibles (sin lo revertido), del más antiguo al más nuevo."""
        cutoff = self._revert_seq(session_id)
        with self._lock:
            rows = self._conn.execute(
                "SELECT seq, id, info FROM messages WHERE session_id = ? ORDER BY seq",
                (session_id,),
            ).fetchall()
            entries = []
            for row in rows:
                if cutoff is not None and row["seq"] >= cutoff:
                    break
                parts = self._conn.execute(
                    "SELECT data FROM parts WHERE session_id = ? AND message_id = ? ORDER BY seq",
                    (session_id, row["id"]),
                ).fetchall()
                entries.append(
                    {
                        "info": json.loads(row["info"]),
                        "parts": [json.loads(part["data"]) for part in parts],
                    }
                )
        if limit is not None and len(entries) > limit:
            entries = entries[-limit:]
        return entries

    # --- checkpoints y revert ------------------------------------------------------------

    def checkpoint(
        self, session_id: str, message_id: str, path: Path, before: str | None, after: str | None
    ) -> None:
        with self._lock, self._conn:
            self._conn.execute(
                "INSERT INTO checkpoints(session_id, message_id, path, before, after) "
                "VALUES(?,?,?,?,?)",
                (session_id, message_id, str(path), before, after),
            )

    def _message_seq(self, session_id: str, message_id: str) -> int | None:
        with self._lock:
            row = self._conn.execute(
                "SELECT seq FROM messages WHERE session_id = ? AND id = ?",
                (session_id, message_id),
            ).fetchone()
        return int(row["seq"]) if row else None

    def _revert_seq(self, session_id: str) -> int | None:
        session = self.session(session_id)
        if session is None or not session.revert_message:
            return None
        return self._message_seq(session_id, session.revert_message)

    def _checkpoints_from(self, session_id: str, seq: int) -> list[sqlite3.Row]:
        with self._lock:
            return self._conn.execute(
                "SELECT c.* FROM checkpoints c JOIN messages m "
                "ON m.session_id = c.session_id AND m.id = c.message_id "
                "WHERE c.session_id = ? AND m.seq >= ? ORDER BY c.seq",
                (session_id, seq),
            ).fetchall()

    def revert(self, session_id: str, message_id: str) -> list[str]:
        """Restaura los archivos al estado previo a ``message_id``; devuelve las rutas."""
        seq = self._message_seq(session_id, message_id)
        if seq is None:
            raise ValueError(f"mensaje desconocido: {message_id}")
        current = self._revert_seq(session_id)
        if current is not None and current <= seq:
            raise ValueError("ese mensaje ya está revertido")
        if current is not None:
            # Revert sobre revert: primero se vuelve al estado completo.
            self.unrevert(session_id)
        touched: list[str] = []
        for row in reversed(self._checkpoints_from(session_id, seq)):
            _write(Path(row["path"]), row["before"])
            touched.append(row["path"])
        with self._lock, self._conn:
            self._conn.execute(
                "UPDATE sessions SET revert_message = ?, updated = ? WHERE id = ?",
                (message_id, now_ms(), session_id),
            )
        return touched

    def unrevert(self, session_id: str) -> list[str]:
        seq = self._revert_seq(session_id)
        if seq is None:
            return []
        touched: list[str] = []
        for row in self._checkpoints_from(session_id, seq):
            _write(Path(row["path"]), row["after"])
            touched.append(row["path"])
        with self._lock, self._conn:
            self._conn.execute(
                "UPDATE sessions SET revert_message = NULL, updated = ? WHERE id = ?",
                (now_ms(), session_id),
            )
        return touched

    def commit_revert(self, session_id: str) -> dict[str, Any] | None:
        """Antes de un prompt nuevo: borra lo revertido. Devuelve el ``info`` del mensaje
        desde el que se cortó (para truncar también el historial del modelo)."""
        session = self.session(session_id)
        if session is None or not session.revert_message:
            return None
        seq = self._message_seq(session_id, session.revert_message)
        info = self.message_info(session_id, session.revert_message)
        with self._lock, self._conn:
            if seq is not None:
                ids = [
                    row["id"]
                    for row in self._conn.execute(
                        "SELECT id FROM messages WHERE session_id = ? AND seq >= ?",
                        (session_id, seq),
                    ).fetchall()
                ]
                for message_id in ids:
                    self._conn.execute(
                        "DELETE FROM parts WHERE session_id = ? AND message_id = ?",
                        (session_id, message_id),
                    )
                    self._conn.execute(
                        "DELETE FROM checkpoints WHERE session_id = ? AND message_id = ?",
                        (session_id, message_id),
                    )
                self._conn.execute(
                    "DELETE FROM messages WHERE session_id = ? AND seq >= ?", (session_id, seq)
                )
            self._conn.execute(
                "UPDATE sessions SET revert_message = NULL WHERE id = ?", (session_id,)
            )
        return info


def _session(row: sqlite3.Row) -> SessionRow:
    return SessionRow(
        id=row["id"],
        title=row["title"],
        directory=row["directory"],
        created=row["created"],
        updated=row["updated"],
        revert_message=row["revert_message"],
    )


def _write(path: Path, content: str | None) -> None:
    if content is None:
        path.unlink(missing_ok=True)
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        handle.write(content)
