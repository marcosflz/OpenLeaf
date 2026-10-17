---
name: lsx-latex-engine
description: Usar cuando se trabaje en el motor LaTeX (stream D, crate lsx-latex) — rutas y comandos verificados de latexmk, synctex, biber y tlmgr en tex/bin/windows, args con -outdir y el rerun -g tras auto-install, semántica REAL de la salida de synctex (bp, Line 1-based, always-exit-0), contrato y fixtures del LogParser, flujo de auto-instalación de paquetes y pendientes de M1 (job objects, CompileQueue). Enlaza a docs/plan/INTERFACES.md §3 y §12.
---

# Motor LaTeX (TeX Live portátil)

Datos verificados en el repo (M0-S5): `tex\bin\windows\{latexmk.exe, synctex.exe, biber.exe, tlmgr.bat}` (TeX Live 2026 embebido). Nunca editar `tex\` a mano; solo invocarlo.
Contrato: `docs/plan/INTERFACES.md` §3 + hallazgos §12 (M0-S5). Flujo del plan: `docs/plan/PLAN-MAESTRO.md` §5.6.

## Rutas

| Helper | Ruta |
|---|---|
| latexmk | `tex\bin\windows\latexmk.exe` (v4.88 verificado) |
| synctex | `tex\bin\windows\synctex.exe` (CLI 1.5) |
| tlmgr | `tex\bin\windows\tlmgr.bat` — **siempre exit 0**: decidir éxito/FALLO parseando la salida |
| root | `TexLivePaths::detect()` (spike) — env `LSX_TEX_BIN` → `LATESTUDIO_ROOT` → ancestros; M1 lo sustituye `PortableRoot` |

## Compilación (verificado)

- Args: `-pdf -synctex=1 -interaction=nonstopmode -file-line-error -outdir=<abs> <main>`. **Todo** cae al outdir (pdf/log/synctex/aux); cwd = proyecto; incremental reutiliza `.fdb_latexmk`.
- Exit codes: `0` ok; **`12`** error TeX (undefined genera PDF igualmente; missingpkg no).
- **CRÍTICO — rerun post-auto-install: usar `latexmk -g`** (sin `-g` da "up-to-date" tras un fallo y el fix "no repara").
- stdout sin % de progreso: solo `Latexmk: …` y `Output written on … (N páginas, M bytes)` → `LogHint::Output{pages,bytes}`; el progreso de UI va por elapsed (M1-D2).
- Limpiar auxiliares: `latexmk -c` con confirmación; `-C` solo explícito (borra PDF) (M1-D5).
- Tests reales: `$env:LSX_TEST_TEXLIVE="1"; cargo test -p lsx-latex -- --nocapture` (cv ≈ 3.4 s).

## Synctex CLI (semántica REAL — no re-descubrir)

- Forward: `synctex view -i línea:col:[página:]archivo -o main.pdf` → bloques `SyncTeX result begin/end` con `Page`, `x`,`y` (punto), `h`,`v` (borde izq. y línea base), `W`,`H` (caja). **La línea de entrada y `Line:` de la salida son 1-based**; columna de entrada `0` = no relevante; salida `Column:-1` = desconocida.
- Inverse: `synctex edit -o "página:x:y:main.pdf"` → `Input:` (**normalizar**: viene con `/./`), `Line:`.
- Unidades **bp, origen arriba-izquierda** (round-trip exacto ±1 línea verificado: 78 ↔ 79). Para **resaltar** usar la CAJA `(h, v−H)..(h+W, v)` — difiere ~3 bp del punto.
- **El CLI siempre exit 0**: pre-validar sidecar `<pdf>.synctex.gz|.synctex` junto al PDF → si no, `NotAvailable`; resultado vacío → `NoMatch`; `src` inexistente → warning + lista vacía.
- `view` devuelve N registros (el 1º es el más preciso). Latencia ~50 ms real. **CLI suficiente para M1-C4**; parser nativo `.synctex.gz` = optimización M4-9. El sidecar debe ir junto al PDF (sin `-d` en CLI): nuestro `-outdir` lo garantiza.
- API: `Synctex::new(exe)` + `forward`/`forward_all`/`inverse`; `SyncRecord{page,x,y,h,v,width,height}`; `SourcePoint.column: Option<u32>`.

## LogParser

- API: `parse_log(&str) -> Vec<Diagnostic>` (tolerante, cap 8 MB, último run desde `Job started:`, dedupe, `raw_block`) y `parse_stdout_stream(line) -> Option<LogHint>`.
- `DiagnosticKind`: `MissingPackage{package}`, `LatexError`, `BibError`, `UndefinedRef`, `Overfull`, `RerunNeeded`, `Unknown`; severidades Error/Warning/Info; `file`/`line`/`column` gracias a `-file-line-error` (verificado: `main.tex:4`).
- Corpus: `fixtures/logs/{cv-ok, cv-sidebar-ok, missing-package, undefined-command}.log` (existen; tests en `crates/latex/tests/logparse.rs`). **Sin `insta` en deps** (asserts directos); snapshots = mejora opcional M1-D3.
- Pendientes M1-D3: `BibError` mínimo, `Underfull`, severidad `Info`, bloques multi-línea heurísticos.

## Auto-install tlmgr

- Flujo: `MissingPackage` + `latex.autoInstallPackages=true` → `tlmgr install <pkg>` (parsear salida; fallback automático `--force` si "tlmgr itself needs to be updated") → **rerun con `-g`** → tope 3 intentos; registrar `PackageInstall[]`.
- Verificado con **`adjmulticol`** (test doble-gated `LSX_TEST_TEXLIVE=1` + `LSX_TEST_TLMGR_INSTALL=1`; reinstalado, estado final = inicial).
- Distro vs tlnet desalineada en esta máquina → M3 considera `tlmgr update --self` o pin de repo.

## Cola, timeout y procesos (pendientes M1)

- `CompileQueue` (single-flight, coalescing, generaciones `submit/cancel`, estados `Idle→Queued→Running→Finished/Cancelled`) → **M1-D2** (el spike emite `generation=0`).
- Timeout 600 s; `CancelToken`; el spike mata con `taskkill /T` → **M1-D1 migra a job objects + `CREATE_NO_WINDOW`** (criterio G1: sin huérfanos). Falta test de cancelación con compile realmente colgado.

## Salida y errores

- `CompileOutcome { status, duration, pdf, log: Option<Path>, exit_code: Option<i32>, log_tail, diagnostics, package_installs }`.
- Mensajes de usuario en español; detalles técnicos en `tracing` (target `lsx::latex`).
