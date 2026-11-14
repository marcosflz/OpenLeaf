from __future__ import annotations

from pathlib import Path

import pytest

from lsx_agent import latex
from lsx_agent.tools import Workspace, build_tools


@pytest.fixture
def ws(tmp_path: Path) -> Workspace:
    (tmp_path / "main.tex").write_bytes(b"\\documentclass{article}\r\nA\r\n")
    (tmp_path / "caps").mkdir()
    (tmp_path / "caps" / "uno.tex").write_text("uno\n", encoding="utf-8")
    (tmp_path / ".env").write_text("SECRET=1\n", encoding="utf-8")
    return Workspace(tmp_path)


@pytest.mark.parametrize(
    "path", ["../fuera.tex", "C:/Windows/win.ini", "/etc/passwd", ".env", ".git/config"]
)
def test_resolve_rejects_escapes_and_secrets(ws: Workspace, path: str) -> None:
    with pytest.raises(PermissionError):
        ws.resolve(path)


def test_resolve_accepts_backslashes(ws: Workspace) -> None:
    assert ws.resolve("caps\\uno.tex") == ws.root / "caps" / "uno.tex"


async def test_list_and_read_and_search(ws: Workspace) -> None:
    tools = {t.name: t for t in build_tools(ws)}
    listing = await tools["list"].run_async()
    assert "main.tex" in listing and "caps/uno.tex" in listing and ".env" not in listing
    assert (await tools["read"].run_async(path="caps/uno.tex")) == "1: uno"
    assert "caps/uno.tex:1: uno" in await tools["search"].run_async(text="uno")


async def test_edit_preserves_crlf(ws: Workspace) -> None:
    tools = {t.name: t for t in build_tools(ws)}
    result = await tools["edit"].run_async(path="main.tex", old_text="A", new_text="B")
    assert result.startswith("Editado main.tex")
    assert (ws.root / "main.tex").read_bytes() == b"\\documentclass{article}\r\nB\r\n"


def test_edit_rejects_ambiguous_match(ws: Workspace) -> None:
    (ws.root / "dup.tex").write_text("x x\n", encoding="utf-8")
    with pytest.raises(ValueError, match="2 veces"):
        ws.preview("edit", {"path": "dup.tex", "old_text": "x", "new_text": "y"})


def test_read_only_tools(ws: Workspace) -> None:
    names = {t.name for t in build_tools(ws, read_only=True)}
    assert names == {"list", "read", "search", "log", "kpsewhich", "skill"}


LOG = r"""
This is pdfTeX, Version 3.141592653
./main.tex:12: Undefined control sequence.
l.12 \foo
./main.tex:12: Undefined control sequence.
! LaTeX Error: File `tikzcd.sty' not found.

Type X to quit or <RETURN> to proceed,
l.5 \usepackage
LaTeX Warning: Reference `fig:a' on page 1 undefined on input line 3.
"""


def test_parse_log_extracts_errors_and_missing_packages() -> None:
    summary = latex.parse_log(LOG)
    assert summary["errors"][0] == {
        "file": "./main.tex",
        "line": 12,
        "message": "Undefined control sequence.",
    }
    assert summary["error_count"] == 2  # duplicado colapsado + error `!`
    assert summary["missing_files"] == ["tikzcd.sty"]
    assert summary["undefined_references"] == 1


def test_find_main_prefers_main_tex(tmp_path: Path) -> None:
    (tmp_path / "a.tex").write_text("\\documentclass{book}", encoding="utf-8")
    assert latex.find_main(tmp_path) == tmp_path / "a.tex"
    (tmp_path / "main.tex").write_text("x", encoding="utf-8")
    assert latex.find_main(tmp_path) == tmp_path / "main.tex"


async def test_edit_with_lf_text_matches_crlf_file(ws: Workspace) -> None:
    (ws.root / "doc.tex").write_bytes(b"\textbf{Hola\r\nfin\r\n")
    tools = {t.name: t for t in build_tools(ws)}
    await tools["edit"].run_async(
        path="doc.tex", old_text="\textbf{Hola\nfin", new_text="\textbf{Hola}\nfin"
    )
    assert (ws.root / "doc.tex").read_bytes() == b"\textbf{Hola}\r\nfin\r\n"


async def test_write_keeps_crlf_of_existing_file(ws: Workspace) -> None:
    tools = {t.name: t for t in build_tools(ws)}
    await tools["write"].run_async(path="main.tex", content="uno\ndos\n")
    assert (ws.root / "main.tex").read_bytes() == b"uno\r\ndos\r\n"
