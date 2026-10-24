---
id: kb/product/que-es
title: LaTeX Studio es un IDE LaTeX portable de cuatro paneles
status: stable
updated: 2026-10-24
source:
  - README.md
  - docs/plan/PLAN-MAESTRO.md
  - docs/plan/README.md
tags: [producto, vision]
related: [kb/product/promesas-ux, kb/architecture/crates]
verified: true
---

# LaTeX Studio es un IDE LaTeX portable de cuatro paneles

## Qué

**LaTeX Studio** (repo **OpenLeaf**) es un IDE LaTeX nativo para Windows escrito en Rust
con GPUI. Su superficie principal son cuatro paneles: **árbol de archivos · editor · PDF ·
chat IA**. El PDF se renderiza dentro de la app, la compilación se dispara al guardar y
SyncTeX conecta el cursor del `.tex` con la página del PDF en ambos sentidos.

El producto se distribuye como carpeta portable (`latex-ide/`) con el ejecutable, el agente
`opencode`, las skills y un TeX Live reducido. También se publica un instalador por usuario
(sin UAC) en `%LOCALAPPDATA%\LaTeX Studio`.

## Lo que no es

- No es Overleaf en el navegador: no hay servidor remoto ni colaboración en tiempo real.
- No es un editor genérico: el dominio es LaTeX y el flujo principal es compilar un PDF.
- No es un wrapper de VS Code: la UI es propia (gpui-kit 0.7.3) y el agente es opencode.

## Restricciones fijadas

| Tema | Decisión |
|---|---|
| UI | GPUI vía `gpui-kit =0.7.3` (snapshot `gpui-pre 0.3.9`) |
| Motor | TeX Live portátil (latexmk + synctex + biber + tlmgr) tras el trait `LatexEngine` |
| Plataforma | Windows primero; código portable |
| Editor | tree-sitter LaTeX (`codebook-tree-sitter-latex 0.6.1`) |
| Chat | opencode portable (`agent/opencode.exe`), server local + SSE |
| Licencia | MIT; binarios GPL solo como referencia/extra separado |

## Fuente de verdad

- Visión y presupuestos: `docs/plan/PLAN-MAESTRO.md`.
- Contratos: `docs/plan/INTERFACES.md`.
- Estado: `docs/board.md`.
- Conocimiento durable: este OKF.
