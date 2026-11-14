"""Herramientas del agente, limitadas al proyecto LaTeX activo.

No hay shell genérico. El agente lee, busca, edita, compila y consulta la
distribución TeX solo con estas funciones. Las rutas se validan siempre
contra la raíz del proyecto.

Las ediciones se calculan dos veces. La primera, en ``preview()``, la usa el
callback de permisos para enseñar el diff antes de escribir. La segunda, en la
propia tool, escribe y guarda un checkpoint para poder revertir.
"""

from __future__ import annotations

import difflib
import os
import tempfile
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path, PureWindowsPath
from typing import Any

from nas.adk.tools import FunctionTool

from . import latex
from .store import Store

MAX_FILE_BYTES = 512 * 1024
MAX_OUTPUT_CHARS = 12000
MAX_SCAN_ENTRIES = 10000
TEXT_SUFFIXES = {
    ".tex",
    ".sty",
    ".cls",
    ".bib",
    ".bst",
    ".ltx",
    ".dtx",
    ".ins",
    ".cfg",
    ".def",
    ".txt",
    ".md",
    ".csv",
    ".tsv",
    ".dat",
    ".lua",
    ".py",
    ".json",
    ".yaml",
    ".yml",
    ".log",
    ".blg",
    ".latexmkrc",
    ".tikz",
    ".pgf",
    ".gp",
    ".r",
    ".m",
}
EXCLUDED_DIRS = {".git", ".hg", ".svn", "node_modules", "__pycache__", ".venv", "venv"}
SECRET_NAMES = {".env", ".netrc", ".npmrc", ".pypirc", "id_rsa", "id_ed25519", "credentials.json"}
SECRET_SUFFIXES = {".pem", ".key", ".pfx", ".p12", ".jks"}

# Tools que escriben en el proyecto: siempre pasan por el callback de permisos.
WRITE_TOOLS = {"edit", "write"}
READ_ONLY_TOOLS = {"list", "read", "search", "log", "kpsewhich", "skill"}
TOOL_NAMES = {"list_files": "list"}


def _is_reserved(part: str) -> bool:
    return os.name == "nt" and PureWindowsPath(part).is_reserved()


def _excluded(name: str) -> bool:
    lowered = name.casefold()
    return (
        lowered in EXCLUDED_DIRS
        or lowered in SECRET_NAMES
        or lowered.startswith(".env")
        or Path(lowered).suffix in SECRET_SUFFIXES
    )


def _bounded(text: str) -> str:
    if len(text) > MAX_OUTPUT_CHARS:
        return text[:MAX_OUTPUT_CHARS] + "\n[Salida truncada; acota la ruta o el rango]"
    return text


def _uses_crlf(text: str) -> bool:
    return "\r\n" in text


def _to_crlf(text: str) -> str:
    return text.replace("\r\n", "\n").replace("\n", "\r\n")


def unified_diff(rel: str, before: str | None, after: str) -> str:
    diff = difflib.unified_diff(
        (before or "").splitlines(keepends=True),
        after.splitlines(keepends=True),
        fromfile=f"a/{rel}" if before is not None else "/dev/null",
        tofile=f"b/{rel}",
        n=3,
    )
    return "".join(line if line.endswith("\n") else line + "\n" for line in diff)


@dataclass
class Workspace:
    """Raíz del proyecto y estado del turno en curso (para checkpoints)."""

    root: Path
    store: Store | None = None
    session_id: str = ""
    message_id: str = ""
    skills_dir: Path | None = None
    edited: list[str] = field(default_factory=list)

    def __post_init__(self) -> None:
        self.root = self.root.expanduser().resolve(strict=True)

    # --- rutas -------------------------------------------------------------------------

    def resolve(self, path: str) -> Path:
        """Ruta relativa → absoluta dentro del proyecto. Rechaza escapes y secretos."""
        cleaned = path.strip().replace("\\", "/")
        if cleaned in ("", "."):
            return self.root
        requested = Path(cleaned)
        if requested.is_absolute() or requested.drive or ":" in cleaned:
            raise PermissionError("usa una ruta relativa al proyecto (sin unidad ni ':')")
        if any(_is_reserved(part) for part in requested.parts):
            raise PermissionError("nombre de dispositivo de Windows no permitido")
        target = (self.root / requested).resolve()
        try:
            relative = target.relative_to(self.root)
        except ValueError as error:
            raise PermissionError("la ruta sale del proyecto") from error
        if any(_excluded(part) for part in relative.parts):
            raise PermissionError("ruta excluida (metadatos o credenciales)")
        return target

    def rel(self, path: Path) -> str:
        return path.relative_to(self.root).as_posix()

    def read_text(self, path: Path) -> str:
        with path.open("rb") as handle:
            data = handle.read(MAX_FILE_BYTES + 1)
        if len(data) > MAX_FILE_BYTES or b"\x00" in data:
            raise ValueError("solo archivos de texto de hasta 512 KiB")
        return data.decode("utf-8", errors="replace")

    def walk(self, start: Path):
        if start.is_file():
            yield start
            return
        if not start.is_dir():
            raise FileNotFoundError("no existe el archivo o la carpeta")
        scanned = 0
        for folder, dirs, files in os.walk(start, followlinks=False):
            scanned += len(dirs) + len(files)
            if scanned > MAX_SCAN_ENTRIES:
                raise ValueError("demasiadas entradas; acota la carpeta")
            dirs[:] = sorted(
                d for d in dirs if not _excluded(d) and not (Path(folder) / d).is_symlink()
            )
            for name in sorted(files):
                entry = Path(folder) / name
                if not _excluded(name) and not entry.is_symlink():
                    yield entry

    # --- vista previa de escrituras (callback de permisos) ------------------------------

    def preview(self, tool: str, args: dict[str, Any]) -> tuple[Path, str | None, str]:
        """(ruta, antes, después) de una escritura sin aplicarla. Lanza si no es válida."""
        target = self.resolve(str(args.get("path", "")))
        if tool == "write":
            content = str(args.get("content", ""))
            before = self.read_text(target) if target.is_file() else None
            if before is not None and _uses_crlf(before):
                content = _to_crlf(content)
            return target, before, content
        if tool == "edit":
            before = self.read_text(target)
            old = str(args.get("old_text", ""))
            new = str(args.get("new_text", ""))
            if not old:
                raise ValueError("old_text no puede estar vacío")
            count = before.count(old)
            if count == 0 and _uses_crlf(before) and "\n" in old:
                # Los modelos escriben `\n`; el archivo (Windows) usa CRLF.
                old, new = _to_crlf(old), _to_crlf(new)
                count = before.count(old)
            if count == 0:
                raise ValueError("old_text no aparece en el archivo; vuelve a leerlo")
            if count > 1 and not args.get("replace_all"):
                raise ValueError(
                    f"old_text aparece {count} veces; amplía el contexto o usa replace_all"
                )
            after = (
                before.replace(old, new) if args.get("replace_all") else before.replace(old, new, 1)
            )
            return target, before, after
        raise ValueError(f"{tool} no es una tool de escritura")

    def apply(self, target: Path, before: str | None, after: str) -> None:
        if len(after.encode("utf-8")) > MAX_FILE_BYTES or "\x00" in after:
            raise ValueError("contenido no válido (binario o mayor de 512 KiB)")
        current = self.read_text(target) if target.is_file() else None
        if current != before:
            raise ValueError("el archivo cambió mientras se pedía permiso; vuelve a leerlo")
        target.parent.mkdir(parents=True, exist_ok=True)
        fd, temporary = tempfile.mkstemp(
            dir=target.parent, prefix=f".{target.name}.", suffix=".tmp"
        )
        try:
            with os.fdopen(fd, "w", encoding="utf-8", newline="") as handle:
                handle.write(after)
            os.replace(temporary, target)
        finally:
            Path(temporary).unlink(missing_ok=True)
        if self.store is not None and self.session_id:
            self.store.checkpoint(self.session_id, self.message_id, target, before, after)
        self.edited.append(self.rel(target))


def build_tools(ws: Workspace, read_only: bool = False) -> list[FunctionTool]:
    def list_files(path: str = ".", offset: int = 0, limit: int = 200) -> str:
        """Lista archivos del proyecto (recursivo, paginado). Omite .git y credenciales."""
        if offset < 0 or not 1 <= limit <= 500:
            raise ValueError("offset >= 0 y limit entre 1 y 500")
        out: list[str] = []
        for index, file in enumerate(ws.walk(ws.resolve(path))):
            if index < offset:
                continue
            if len(out) == limit:
                out.append(f"[Hay más archivos; usa offset={offset + limit}]")
                break
            out.append(ws.rel(file))
        return _bounded("\n".join(out) or "Sin archivos")

    def read(path: str, start_line: int = 1, limit: int = 400) -> str:
        """Lee líneas numeradas de un archivo de texto del proyecto (máx. 1000 por llamada)."""
        if start_line < 1 or not 1 <= limit <= 1000:
            raise ValueError("start_line >= 1 y limit entre 1 y 1000")
        lines = ws.read_text(ws.resolve(path)).splitlines()
        end = start_line - 1 + limit
        body = "\n".join(
            f"{number}: {line}"
            for number, line in enumerate(lines[start_line - 1 : end], start_line)
        )
        if end < len(lines):
            body += f"\n[{len(lines) - end} líneas más; usa start_line={end + 1}]"
        return _bounded(body or "Sin líneas en este rango")

    def search(text: str, path: str = ".", limit: int = 80) -> str:
        """Busca texto literal (sensible a mayúsculas) en los archivos de texto del proyecto."""
        if not text or "\n" in text or not 1 <= limit <= 300:
            raise ValueError("texto de una línea no vacío y limit entre 1 y 300")
        results: list[str] = []
        for file in ws.walk(ws.resolve(path)):
            if file.suffix.lower() not in TEXT_SUFFIXES:
                continue
            try:
                content = ws.read_text(file)
            except (OSError, ValueError):
                continue
            for number, line in enumerate(content.splitlines(), 1):
                if text in line:
                    results.append(f"{ws.rel(file)}:{number}: {line.strip()}")
                    if len(results) >= limit:
                        return _bounded("\n".join(results) + "\n[Más coincidencias; acota]")
        return _bounded("\n".join(results) or "Sin coincidencias")

    def edit(path: str, old_text: str, new_text: str, replace_all: bool = False) -> str:
        """Reemplaza texto exacto en un archivo (una vez salvo replace_all). Cambios mínimos."""
        target, before, after = ws.preview(
            "edit",
            {"path": path, "old_text": old_text, "new_text": new_text, "replace_all": replace_all},
        )
        ws.apply(target, before, after)
        return f"Editado {ws.rel(target)}\n" + unified_diff(ws.rel(target), before, after)

    def write(path: str, content: str) -> str:
        """Crea un archivo nuevo o reescribe uno entero. Prefiere `edit` para cambios pequeños."""
        target, before, after = ws.preview("write", {"path": path, "content": content})
        ws.apply(target, before, after)
        verb = "Creado" if before is None else "Reescrito"
        return f"{verb} {ws.rel(target)}"

    async def compile(file: str = "", engine: str = "pdflatex") -> dict[str, Any]:
        """Compila con latexmk (pdflatex|xelatex|lualatex). Devuelve errores con archivo:línea,
        archivos/paquetes que faltan y la cola del log. Sin `file` usa main.tex."""
        return await latex.compile_project(ws.root, file or None, engine)

    def log(lines: int = 120) -> str:
        """Últimas líneas del .log de la última compilación."""
        if not 10 <= lines <= 600:
            raise ValueError("lines entre 10 y 600")
        text = latex.read_log_text(ws.root, latex.find_main(ws.root))
        return _bounded(latex.log_tail(text, lines) if text else "No hay .log; compila primero")

    async def kpsewhich(name: str) -> str:
        """Busca un archivo (p. ej. `tikz.sty`) en la distribución TeX. Vacío = no instalado."""
        if not name or "/" in name or "\\" in name or len(name) > 120:
            raise ValueError("nombre de archivo simple, p. ej. geometry.sty")
        code, output = await latex.run_process(["kpsewhich", name], ws.root, timeout=30)
        return output.strip() or f"{name}: no encontrado (exit {code})"

    async def tlmgr_install(package: str) -> str:
        """Instala un paquete de CTAN con tlmgr (nombre del paquete, no del .sty)."""
        if not package or not all(c.isalnum() or c in "-_." for c in package):
            raise ValueError("nombre de paquete no válido")
        code, output = await latex.run_process(["tlmgr", "install", package], ws.root, timeout=600)
        return _bounded(f"exit {code}\n{output[-4000:]}")

    def skill(name: str = "") -> str:
        """Guía experta de LaTeX. Sin `name` lista las disponibles; con `name` la devuelve."""
        base = ws.skills_dir
        if base is None or not base.is_dir():
            return "No hay skills instaladas"
        available = sorted(p.name for p in base.iterdir() if (p / "SKILL.md").is_file())
        if not name:
            return "Skills: " + ", ".join(available)
        if name not in available:
            raise ValueError(f"skill desconocida; disponibles: {', '.join(available)}")
        return _bounded((base / name / "SKILL.md").read_text(encoding="utf-8"))

    readers: list[Callable[..., Any]] = [list_files, read, search, log, kpsewhich, skill]
    writers: list[Callable[..., Any]] = [edit, write, compile, tlmgr_install]
    selected = readers if read_only else readers + writers
    # Lecturas en paralelo; escrituras y compilación siempre en serie.
    return [
        FunctionTool(fn, name=TOOL_NAMES.get(fn.__name__, fn.__name__), parallel_safe=fn in readers)
        for fn in selected
    ]
