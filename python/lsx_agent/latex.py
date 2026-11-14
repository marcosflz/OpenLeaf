"""Compilación con ``latexmk`` y lectura del log, pensada para el modelo.

La salida está resumida a propósito. El modelo recibe errores con
archivo:línea, los paquetes que faltan y la cola del log, no 3000 líneas
de ruido de TeX.
"""

from __future__ import annotations

import asyncio
import os
import re
import shutil
from pathlib import Path
from typing import Any

ENGINES = {"pdflatex": "-pdf", "xelatex": "-xelatex", "lualatex": "-lualatex"}
COMPILE_TIMEOUT = 180.0
MAX_ERRORS = 30
LOG_TAIL_LINES = 60

_FILE_LINE_ERROR = re.compile(
    r"^(?P<file>[^:\n]+?\.(?:tex|sty|cls|bib|ltx|dtx)):(?P<line>\d+): (?P<msg>.+)$"
)
_BANG_ERROR = re.compile(r"^! (?P<msg>.+)$")
_MISSING_FILE = re.compile(r"File [`'](?P<name>[^`']+?)' not found")
_REQUESTED_PACKAGE = re.compile(r"You have requested package [`'](?P<name>[^`']+)'")
_UNDEFINED_REF = re.compile(r"LaTeX Warning: (Reference|Citation) [`'][^`']+' .*undefined")
_LINE_HINT = re.compile(r"^l\.(?P<line>\d+)")


def find_main(root: Path) -> Path | None:
    """``main.tex`` si existe; si no, el primer ``.tex`` de la raíz con ``\\documentclass``."""
    main = root / "main.tex"
    if main.is_file():
        return main
    for candidate in sorted(root.glob("*.tex")):
        try:
            head = candidate.read_text(encoding="utf-8", errors="replace")[:4000]
        except OSError:
            continue
        if "\\documentclass" in head:
            return candidate
    return None


def parse_log(text: str) -> dict[str, Any]:
    """Errores, paquetes que faltan y avisos relevantes de un ``.log`` de TeX."""
    errors: list[dict[str, Any]] = []
    seen: set[tuple[str, int, str]] = set()
    lines = text.splitlines()
    for index, line in enumerate(lines):
        match = _FILE_LINE_ERROR.match(line.strip())
        if match:
            key = (match["file"], int(match["line"]), match["msg"].strip())
            if key not in seen:
                seen.add(key)
                errors.append({"file": key[0], "line": key[1], "message": key[2]})
            continue
        match = _BANG_ERROR.match(line)
        if match:
            hint = next(
                (
                    _LINE_HINT.match(follow)
                    for follow in lines[index + 1 : index + 12]
                    if _LINE_HINT.match(follow)
                ),
                None,
            )
            key = ("", int(hint["line"]) if hint else 0, match["msg"].strip())
            if not any(error["message"] == key[2] for error in errors) and key not in seen:
                seen.add(key)
                errors.append({"file": None, "line": key[1] or None, "message": key[2]})
    missing = sorted(
        {m["name"] for m in _MISSING_FILE.finditer(text)}
        | {f"{m['name']}.sty" for m in _REQUESTED_PACKAGE.finditer(text)}
    )
    return {
        "errors": errors[:MAX_ERRORS],
        "error_count": len(errors),
        "missing_files": missing,
        "undefined_references": len(_UNDEFINED_REF.findall(text)),
    }


def log_tail(text: str, lines: int = LOG_TAIL_LINES) -> str:
    return "\n".join(text.splitlines()[-lines:])


def read_log_text(root: Path, main: Path | None) -> str | None:
    """Log del archivo principal o, si no existe, el ``.log`` más reciente."""
    candidates: list[Path] = []
    if main is not None:
        candidates.append(main.with_suffix(".log"))
    logs = sorted(root.glob("*.log"), key=lambda p: p.stat().st_mtime, reverse=True)
    candidates.extend(logs)
    for log in candidates:
        if log.is_file():
            return log.read_text(encoding="utf-8", errors="replace")
    return None


async def run_process(
    args: list[str], cwd: Path, timeout: float = COMPILE_TIMEOUT
) -> tuple[int, str]:
    """Ejecuta sin shell; devuelve (exit_code, stdout+stderr acotado)."""
    executable = shutil.which(args[0])
    if executable is None:
        raise FileNotFoundError(
            f"no se encontró `{args[0]}` en el PATH (¿falta la distribución TeX portable?)"
        )
    kwargs: dict[str, Any] = {}
    if os.name == "nt":
        kwargs["creationflags"] = 0x08000000  # CREATE_NO_WINDOW
    process = await asyncio.create_subprocess_exec(
        executable,
        *args[1:],
        cwd=str(cwd),
        stdin=asyncio.subprocess.DEVNULL,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
        **kwargs,
    )
    try:
        output, _ = await asyncio.wait_for(process.communicate(), timeout)
    except (TimeoutError, asyncio.CancelledError):
        process.kill()
        await process.wait()
        raise
    return process.returncode or 0, output.decode("utf-8", errors="replace")[-8000:]


async def compile_project(
    root: Path, file: str | None = None, engine: str = "pdflatex"
) -> dict[str, Any]:
    if engine not in ENGINES:
        raise ValueError(f"motor no soportado: {engine}; usa {', '.join(ENGINES)}")
    main = (root / file) if file else find_main(root)
    if main is None or not main.is_file():
        raise FileNotFoundError("no hay archivo principal: indica `file` (p. ej. main.tex)")
    args = [
        "latexmk",
        ENGINES[engine],
        "-synctex=1",
        "-interaction=nonstopmode",
        "-file-line-error",
        main.name,
    ]
    code, output = await run_process(args, main.parent)
    log = read_log_text(main.parent, main) or output
    summary = parse_log(log)
    pdf = main.with_suffix(".pdf")
    return {
        "ok": code == 0 and summary["error_count"] == 0,
        "exit_code": code,
        "main": main.relative_to(root).as_posix(),
        "engine": engine,
        "pdf": pdf.relative_to(root).as_posix() if pdf.is_file() else None,
        **summary,
        "log_tail": log_tail(log) if summary["error_count"] or code else "",
    }
