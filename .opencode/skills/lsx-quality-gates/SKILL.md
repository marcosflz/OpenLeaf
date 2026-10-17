---
name: lsx-quality-gates
description: Usar antes de cerrar cualquier tarea o PR de LaTeX Studio y al preparar un gate de hito — comandos exactos de fmt, clippy, nextest, deny, taplo y typos, tests de integración con LSX_TEST_TEXLIVE, cómo correr tests UI con TestAppContext, cuándo actualizar snapshots de insta, presupuestos de rendimiento de docs/plan/PLAN-MAESTRO.md §6.3 y plantilla de evidencia e informes de gate en docs/qa/reports.
---

# Gates y evidencia

Fuente: `docs/plan/SWARM.md` §3 (DoD) y §5 (gates); `docs/plan/PLAN-MAESTRO.md` §6; CI real en `.github/workflows/ci.yml`. Aplica a todos los streams, QA y orchestrator.

## Comandos obligatorios (local, Windows/pwsh)

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace
cargo deny check licenses bans sources
taplo fmt --check **/Cargo.toml
typos
```

- Sin `nextest`: fallback permitido `cargo test --workspace` (AGENTS.md).
- Integración real del motor (requiere `tex\`): `$env:LSX_TEST_TEXLIVE="1"; cargo nextest run -p lsx-latex`.
- `deny` usa el allowlist de `deny.toml` del repo; no añadir licencias nuevas sin ADR.
- CI (`.github/workflows/ci.yml`, windows-latest): fmt → `cargo clippy --workspace --all-targets -- -D warnings` → nextest → cargo-deny. Ojo: el CI aún no pasa `--all-features` ni corre `taplo`/`typos` — `TODO(P0/CI): verificar` cuándo los añade el orchestrator; localmente correr los 6 comandos completos.

## Tests por capa (PLAN §6.1)

| Nivel | Herramientas | Dónde |
|---|---|---|
| Unit core (logparse, synctex, settings, LRU, SSE, reducer, filtros) | `cargo test` + `insta` | cada crate |
| Integración engine | nextest con `LSX_TEST_TEXLIVE=1` | `lsx-latex` |
| Integración chat | `wiremock` | `lsx-chat` |
| PDF golden | `cargo run -p xtask -- pdf-golden` (stub hoy) | fixtures PDF |
| UI GPUI | `TestAppContext` / `#[gpui_kit::test]` | `lsx-ui`, `lsx-editor` |

- UI: `cargo nextest run -p lsx-ui`; patrones en el skill `lsx-gpui-patterns`; accesibilidad según `ACCESSIBILITY-UI-TESTING.md` del kit (M3-5).
- `insta`: revisar cada diff con `cargo insta review`; aceptar SOLO cambios intencionales; jamás actualizar snapshots "para que pase". El DoD exige tests que cubren los criterios de aceptación.

## DoD por tarea (SWARM §3)

1. Código + tests (unit; integración si aplica) que cubren los criterios de aceptación.
2. Gates verdes local y en CI.
3. Sin `todo!()`/`unwrap()` en rutas de producción; errores tipados con mensaje de usuario.
4. Docs afectadas actualizadas (INTERFACES / skill / README del crate).
5. Evidencia para cambios visibles: captura o GIF + pasos de reproducción.
6. `docs/board.md` actualizado (estado + notas + enlaces).
7. Perf dentro del presupuesto si la tarea toca render/IO.

## Presupuestos de rendimiento (PLAN §6.3) — medir en cada gate

| Métrica | Objetivo |
|---|---|
| Arranque en frío hasta ventana pintada | < 1.2 s |
| RAM idle (1 .tex + PDF 1 página, sin chat) | < 250 MB |
| RAM con chat + PDF 50 páginas | < 450 MB |
| Latencia tecla → pintado (10k líneas) | < 16 ms |
| Render página PDF típica 100 % (150 dpi) | < 120 ms (caché miss) |
| Ciclo guardar → PDF actualizado (CV 1 pág) | < 2.5 s |
| Forward/inverse SyncTeX (respuesta UI) | < 300 ms |

- Medición: overlay dev `F12` (M2-11) y `cargo xtask bench` (SWARM §5) — `TODO(M0): verificar`, el stub de `xtask` aún no tiene tarea `bench`; mientras, criterion manual. Publicar en `docs/perf.md` (se crea en el primer gate).

## Informe de gate (SWARM §3)

- Obligatorio `docs/qa/reports/G<N>-<fecha>.md` con: E2E ejecutado + evidencia, gates técnicos, presupuestos medidos, riesgos nuevos y checklist de `docs/gates.md`.
- E2E: `docs/qa/e2e.md` (12 pasos). G1 = pasos 1–8; G2 = 9–12 + QA adversarial; G3 = completo.
- Lo firma `qa-verifier` (nunca quien implementó); el veredicto se refleja en `docs/board.md` y `docs/gates.md` vía el orchestrator.

## Plantilla de evidencia (PR / comentario)

```
Tarea: <ID> · Worktree: <feat/...>
Qué: <1–3 líneas>
Cómo probar: <comandos exactos>
Evidencia: <captura/GIF/log>
Gates: fmt ✅ clippy ✅ nextest ✅ deny ✅ (extras)
Riesgos/notas: <...>
```

## Evidencia visual (UI)

```powershell
pwsh scripts/capture-window.ps1 -ProcessName spike-<x> -Out shots/<nombre>.png -DelaySeconds 1
```

- Captura la ventana PRINCIPAL (mayor área visible) aunque esté tapada: flota la ventana con `HWND_TOPMOST` (no roba el foco) y la restaura al terminar. DPI-aware.
- Con bins que muestran consola, elige la ventana GPUI (EnumWindows), no `MainWindowHandle`.
- **Lee siempre el PNG** (la herramienta de lectura acepta imágenes) antes de adjuntarlo como evidencia; itera estilo → captura → lectura.
- `shots/` está en `.gitignore`; adjunta las capturas en los informes de gate (`docs/qa/reports/`).
