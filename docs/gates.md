# Gates vivos — riesgos y decisiones de interfaz

> Checklist viva. Los gates (G0–G3) firman aquí + en `docs/qa/reports/`.
> Fuentes: `docs/plan/PLAN-MAESTRO.md` §7 (riesgos R*) e `docs/plan/INTERFACES.md` §11/§12 (decisiones I*).

## Riesgos

| ID | Riesgo | Se verifica en | Estado | Notas |
|----|--------|----------------|--------|-------|
| R1 | Breaking changes de snapshots GPUI | bump M3-8 / mensual | abierto | Pin `=0.7.3` (gpui-pre 0.3.9) compilando en Windows (stable 1.98) ✓ |
| R2 | Editor del kit no cubre necesidades | M0-S2 → G0 | ✅ **resuelto (M0-S2)** | Registro de lenguajes + diagnósticos inline + buffer del kit; pestañas app-side; fallback innecesario |
| R3 | Rendimiento PDF (hayro) | M0-S3 → G0/G1 | ✅ **resuelto (M0-S3)** | p50 release 4–8 ms a 1.0–2.0× (budget <120 ms); visor propio confirmado |
| R4 | Formato de `synctex.exe` entre versiones | M0-S5 → G1 | ✅ **resuelto (M0-S5)** | Formato real fijado + round-trip 78↔79 (±1) con tests; fixtures `fixtures/synctex/` |
| R5 | Deriva API opencode v1→v2 | continuo | mitigado (M0-S4) | Tabla I-4 congelada + fixtures + aislamiento `protocol.rs`; vigilar upgrades |
| R6 | AGENTS.md del repo contaminando al agente del producto | M1 → G1 | abierto | Proyectos viven en `workspace/`; test explícito + plan B |
| R7 | Compilaciones concurrentes/procesos pegados | M1-D1 → G1 | abierto | Job objects pendientes (spike usa `taskkill /T`) |
| R8 | Windows: rutas largas/acentos/antivirus | M1-A/F → G1 | abierto | Falta test con acentos/espacios en gates |
| R9 | Deriva de dependencias (tamaño/duplicados) | mensual | abierto | `cargo tree -d`, features mínimas |
| R10 | Parálisis por detalles UI | continuo | abierto | Backlog M4 sin discusión |

## Decisiones de interfaz — todas resueltas en M0 (evidencia en INTERFACES §11/§12)

| ID | Resolución | Estado |
|----|-----------|--------|
| I-1 | Editor del kit: lenguajes custom + diagnósticos ✓; pestañas app-side | ✅ M0-S2 |
| I-2 | Visor propio con `RenderImage` (BGRA, w/dpr); gpui-pdf descartado (GPL) | ✅ M0-S3 |
| I-3 | Buffer del componente (`Rope`); ropey fuera del editor | ✅ M0-S2 |
| I-4 | Tabla SSE congelada con fixtures reales; sin Last-Event-ID → refetch | ✅ M0-S4 |
| I-5 | Instancia global + `?directory=` por sesión | ✅ M0-S4 |
| I-6 | `gpui-fast` no en v1 | ✅ decisión firme |
| I-7 | Dock del kit, 4 hojas fijas, colapso vía `Panel::visible`, persistencia dump/load | ✅ M0-S1 |

## Registro de gates

| Gate | Fecha | Verdicto | Informe | Pendientes |
|------|-------|----------|---------|-----------|
| P0 | 2026-10-17 | ✅ PASS | `docs/qa/reports/P0-2026-10-17.md` | P0.4b remoto+push; `xtask fixtures` |
| G0 | pendiente | — | informes `docs/qa/reports/M0-S{1..6}-*.md` + demos ejecutables | **revisión con el usuario** (demos + freeze); tras G0 → O2 (M1) |
| G1 | — | — | — | — |
| G2 | — | — | — | — |
| G3 | — | — | — | — |
