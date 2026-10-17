# Spike M0-S5 — Engine (latexmk) + LogParser + SyncTeX + auto-install

- Fecha: 2026-10-17 · Sesión: `ses_eb6356f59ffeS9HGXlbA83Atu2`
- **ESTADO: OK** (implementación real + tests gated ejecutados de verdad; gates verdes; temporales limpios; estado de TeX Live restaurado)

## Entregable

| Archivo | Contenido |
|---|---|
| `crates/latex/src/engine.rs` | `LatexEngine`, `TexLiveEngine::{probe,compile}`, `CompileRequest/Outcome/Event`, `CancelToken`, auto-install (bucle tlmgr) |
| `crates/latex/src/logparse.rs` | `parse_log`, `parse_stdout_stream`, `Diagnostic*`, `LogHint` (sin regex) |
| `crates/latex/src/synctex.rs` | `Synctex::{forward,forward_all,inverse}` |
| `crates/latex/src/tlmgr.rs` | `Tlmgr::{version,is_installed,install,remove}` (fallback `--force`) |
| `crates/latex/src/{root,proc,pathutil}.rs` | `TexLivePaths::detect()`, ejecución con timeout/cancelación (kill de árbol), normalización de rutas |
| `crates/latex/tests/{logparse,texlive_gated}.rs` | 6 tests de fixtures + 6 gated con TeX Live real |
| `crates/latex/examples/spike.rs` | demo CLI |

Sin deps nuevas. Los archivos `fixtures/**` nunca se ensuciaron (copias a `%TEMP%\lsx-s5\`; verificado).

## Evidencia (salida real)

```
$env:LSX_TEST_TEXLIVE="1"; cargo test -p lsx-latex -- --nocapture
  [probe] latexmk 4.88 available=true
  [cv] status=Success dur=3.38s pdf=out\main.pdf diags=1 exit=Some(0)
  [undefined] Failed exit=12 → LatexError main.tex:4
  [missingpkg] Failed exit=12 → MissingPackage{package:"estepaquete-quenoexiste"}
  [synctex] forward 78 → page=1 x=46.799553 y=311.051819 w=523.80 h=11.955
  [synctex] inverse (1, 46.799553, 311.051819) → src\main.tex:79      # ±1 ✓
  test result: ok. 6 passed
$env:LSX_TEST_TLMGR_INSTALL="1" (2ª gate)
  [tlmgr] adjmulticol: sin auto-install Failed → con auto-install Success (installs=[adjmulticol])
  test result: ok. 6 passed      # paquete reinstalado; estado final = inicial
```
Gates: `cargo fmt -p lsx-latex --check` ✓ · `clippy -p lsx-latex --all-targets -- -D warnings` ✓ · tests unit+fixtures ✓.

## Respuestas de interfaz

### SyncTeX CLI (1.5) — salida real

```
$ synctex view -i 78:0:<abs>\main.tex -o <abs>\out\main.pdf
SyncTeX result begin … Page:1  x:46.799553 y:311.051819  h:46.799553 v:314.638397  W:523.800598 H:11.955168 …
$ synctex edit -o "1:46.799553:311.051819:<abs>\out\main.pdf"
… Input:C:/…/src/./main.tex  Line:79  Column:-1 …
```

- Unidades **bp (1/72")**, origen **arriba-izquierda** (round-trip exacto lo confirma).
- `Line:` **1-based**; `Column:-1` = desconocida; entrada `-i` = `línea:columna:[página:]archivo`.
- `view` da **N registros** (10 para esa línea; el 1º más preciso) + caja `(h, v−H)..(h+W, v)` (v = línea base).
- El CLI **siempre exit 0**: sin PDF/sidecar imprime nada → pre-validar `<pdf>.synctex.gz|.synctex` → `NotAvailable`; lista vacía → `NoMatch`.
- `Input:` con `/./` → **normalizar** la ruta.
- Latencia real ~50 ms. **CLI suficiente para M1-C4**; parser nativo `.synctex.gz` = optimización M4-9.

### latexmk `-outdir`

- Todos los artefactos al outdir (pdf/log/synctex/aux/fls/fdb); dir fuente limpio; incremental OK.
- Exit `0` ok; **`12`** en error TeX (undefined aun genera PDF; missingpkg no).
- **Hallazgo crítico**: tras un fallo, rerun con mismas fuentes es "up-to-date" → el rerun post-auto-install necesita **`latexmk -g`** (implementado). Sin eso el auto-install "instala pero no repara".
- stdout sin progreso porcentual (solo `Latexmk: …` y `Output written on … N páginas`) → M1-D2 usa elapsed.

### tlmgr

- `tlmgr.bat` **siempre exit 0** → decidir éxito parseando salida.
- Distro desactualizada vs tlnet → `install` puede morir con *"tlmgr itself needs to be updated"*; `Tlmgr::install` lo detecta y **reintenta con `--force`** (implementado). M3: `tlmgr update --self` o pin de repo.
- Prueba real con **`adjmulticol`** (reinstalado; estado final = inicial). Reversible.

## API descubierta / ajustes al contrato §3 (freeze G0)

```rust
// cambios respecto al borrador §3
SourcePoint.column -> Option<u32>                      // CLI emite -1
+ SyncRecord { page, x, y, h, v, width, height } + Synctex::forward_all(..)
                                                        // M1-C4 resalta con la CAJA (h,v), no con el punto (~3 bp de diferencia)
+ SynctexError::Spawn(String);  Synctex::new(exe)       // el exe va al constructor
CompileRequest += out_dir: Option<Utf8PathBuf>, auto_install: bool
CompileOutcome += log: Option<Utf8PathBuf>, exit_code: Option<i32>
EngineInfo { id, available, version:Some("4.88"), latexmk, texbin, synctex:Option, tlmgr:Option }
TexLivePaths::detect()                                 // sustituto spike de PortableRoot (env LSX_TEX_BIN -> LATESTUDIO_ROOT)
CompileQueue                                           // M1-D2 (generation=0 placeholder)
LogHint { Progress{line}, Error{message}, Warning{message}, Output{pages,bytes} }
```

## Riesgos / TODO

1. Kill de árbol con `taskkill /T` (spike) → M1-D1 migra a **job objects** + `CREATE_NO_WINDOW`; falta test de timeout/cancelación con un compile realmente colgado.
2. `CompileQueue` (single-flight/coalescing/generaciones) → M1-D2.
3. Parser: sin `insta` (no está en deps); `BibError` mínimo; sin `Underfull`/`Info`; bloques multi-línea heurísticos → M1-D3.
4. SyncTeX sin `-d` (sidecar junto al PDF — nuestro layout siempre lo cumple); `forward` con `src` absoluto recomendado.
5. Auto-install requiere red; test doble-gated.
6. `tex/` modificado legítimamente por tlmgr (reinstalaciones + ls-R); estado final = inicial.
7. Volcar hallazgos a `INTERFACES.md §3` y skill `lsx-latex-engine` (hecho en consolidación).
