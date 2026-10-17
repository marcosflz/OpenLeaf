---
name: lsx-latex-engine
description: Usar cuando se trabaje en el motor LaTeX (stream D, crate lsx-latex) — rutas y comandos de latexmk, synctex, biber y tlmgr en tex/bin/windows, args por defecto con -synctex=1, contrato y fixtures del LogParser, flujo de auto-instalación de paquetes, cola single-flight con generaciones, timeout, cancelación y job objects. Enlaza a docs/plan/INTERFACES.md §3 y docs/plan/PLAN-MAESTRO.md §5.6.
---

# Motor LaTeX (TeX Live portátil)

Datos verificados en el repo: existen `tex\bin\windows\latexmk.exe`, `synctex.exe`, `biber.exe` y `tlmgr.bat` (TeX Live 2026 embebido). Nunca editar `tex\` a mano; solo invocarlo.
Contrato: `docs/plan/INTERFACES.md` §3. Flujo del plan: `docs/plan/PLAN-MAESTRO.md` §5.6.

## Rutas (vía `PortableRoot`)

| Helper | Ruta |
|---|---|
| `latexmk()` | `tex\bin\windows\latexmk.exe` |
| `synctex_exe()` | `tex\bin\windows\synctex.exe` |
| `tex_dir()` | `tex\` (raíz del árbol TeX Live) |
| tlmgr | `tex\bin\windows\tlmgr.bat` — `TODO(M0-S5): verificar` invocación portátil exacta y si necesita env vars (TEXMFCONFIG/TEXMFHOME) |

## Compilación

- Args por defecto (`CompileRequest.args`): `-synctex=1 -interaction=nonstopmode -file-line-error -pdf -outdir=<proyecto> <main.tex>` (PLAN §5.6 / INTERFACES §3).
- Limpiar auxiliares (F5): `latexmk -c` con confirmación (M1-D5); `-C` solo con confirmación explícita (borra el PDF).
- Detección de `main.tex`: settings `latex.mainFile` → archivo abierto con `\documentclass` → `\input` raíz → único `.tex` con `\documentclass` (PLAN §5.6; heurística en M1-D5).
- `probe()`: versión + disponibilidad del motor para el badge del toolbar (`EngineInfo`).
- Integración real: `$env:LSX_TEST_TEXLIVE="1"; cargo nextest run -p lsx-latex` (requiere `tex\`).

## Synctex CLI

- Forward (cursor → PDF): `synctex view -i línea:col:archivo -o main.pdf` (PLAN §4.3).
- Inverse (PDF → origen): `synctex edit -o página:x:y:main.pdf` (PLAN §4.3).
- Unidades: bp (1/72"), origen arriba-izquierda; parseo tolerante entre `SyncTeX result begin…end`; timeout 2 s; ejecutar en `spawn_blocking`; cachear el último resultado por (pdf, generation) (INTERFACES §3).
- `TODO(M0-S5): verificar` el formato de salida real de `synctex.exe` embebido y fijarlo con fixtures (`fixtures/synctex/` aún no existe en el repo; se crea en M0).

## LogParser

- API: `parse_log(&str) -> Vec<Diagnostic>` (tolerante, cap 8 MB, analiza el último run desde `Job started:`) y `parse_stdout_stream(line) -> Option<LogHint>` para progreso en vivo.
- `DiagnosticKind`: `MissingPackage{package}`, `LatexError`, `BibError`, `UndefinedRef`, `Overfull`, `RerunNeeded`, `Unknown`; severidad `Error`/`Warning`/`Info`; con `file`/`line`/`column` gracias a `-file-line-error`; conservar `raw_block`.
- Corpus de tests: `fixtures/logs/*.log` (4 tipos de error reales) con snapshots `insta` — `TODO(P0.6/M0): verificar` que la carpeta `fixtures/logs/` aún no está en el repo; no romper los tests si falta.

## Auto-install tlmgr

- Si hay `MissingPackage` y `latex.autoInstallPackages=true` → `tex\bin\windows\tlmgr.bat install <paquete>` → rerun; tope 3 intentos por sesión; registrar `PackageInstall[]` en `CompileOutcome`; banner de progreso (M1-D4).
- Fixtures: `fixtures/projects/broken-missingpkg` (falta paquete) y `broken-undefined` (error de código) ya existen.
- No instalar a ciegas: verificar el nombre del paquete (`kpsewhich`/`tlmgr`) antes de reintentar.

## Cola, timeout y procesos

- `CompileQueue`: single-flight + coalescing (si llegan cambios mientras compila → 1 rerun); `submit(CompileRequest) -> generation`; `cancel(generation)`; estados `Idle → Queued → Running → Success/Failed/Cancelled`.
- `CompileEvent`: `Queued`, `Started{generation}`, `Progress{line}`, `Finished{generation, outcome}`, `Cancelled{generation}`.
- Timeout default 600 s (settings `latex.timeoutSecs`); cancelación con `CancelToken` (AtomicBool + broadcast); kill-on-drop + **job object kill-on-close** para latexmk/biber/tlmgr (PLAN §5.3; criterio G1: sin procesos huérfanos).
- `TODO(M0-S5): verificar` el formato real de stdout de `latexmk` para progreso (%, páginas) — si no hay, usar elapsed.

## Salida y errores

- `CompileOutcome { status, duration, pdf, log_tail (~200 líneas), diagnostics, package_installs }`.
- Mensajes de usuario en español; detalles técnicos en `tracing` (targets `lsx::latex`).
