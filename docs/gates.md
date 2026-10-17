# Gates vivos — riesgos y decisiones de interfaz

> Checklist viva. Los gates (G0–G3) firman aquí + en `docs/qa/reports/`.
> Fuentes: `docs/plan/PLAN-MAESTRO.md` §7 (riesgos R*) e `docs/plan/INTERFACES.md` §11 (decisiones I*).

## Riesgos

| ID | Riesgo | Se verifica en | Estado | Notas |
|----|--------|----------------|--------|-------|
| R1 | Breaking changes de snapshots GPUI | bump M3-8 / mensual | abierto | Pin `=0.7.3` (gpui-pre 0.3.9) |
| R2 | Editor del kit no cubre necesidades (grammar LaTeX, diagnósticos) | M0-S2 → G0 | abierto | Fallback: highlighter propio sobre Input/Textarea |
| R3 | Rendimiento PDF (hayro) | M0-S3 → G0/G1 | abierto | Fallback: pdfium-render tras trait |
| R4 | Formato de `synctex.exe` entre versiones | M0-S5 → G1 | abierto | Parser tolerante + fixtures de la versión embebida |
| R5 | Deriva API opencode v1→v2 | continuo | abierto | Aislamiento en `protocol.rs` + fixtures |
| R6 | AGENTS.md del repo contaminando al agente del producto | M1 → G1 | abierto | Proyectos viven en `workspace/`; test explícito + plan B |
| R7 | Compilaciones concurrentes/procesos pegados | M1-D → G1 | abierto | Single-flight + timeout + job objects |
| R8 | Windows: rutas largas/acentos/antivirus | M1-A/F → G1 | abierto | Retry rename, tests con acentos/espacios |
| R9 | Deriva de dependencias (tamaño/duplicados) | mensual | abierto | `cargo tree -d`, features mínimas |
| R10 | Parálisis por detalles UI | continuo | abierto | Backlog M4 sin discusión |

## Decisiones de interfaz

| ID | Pregunta | Default del plan | Se decide en | Estado |
|----|----------|------------------|--------------|--------|
| I-1 | ¿Editor del kit expone modelo de archivo + rangos + registry de lenguajes? | Sí (spike S2) | G0 | abierto |
| I-2 | Render propio `lsx-pdf` + `RenderImage` (no adoptar gpui-pdf GPL) | Render propio | G0 | abierto |
| I-3 | ¿Buffer propio ropey o del componente? | El del componente | G0 | abierto |
| I-4 | Nombres exactos de eventos SSE y campos de `parts` | Tabla congelada con fixtures | G0 (S4) | abierto |
| I-5 | Puerto/instancia opencode por proyecto o global | Global, cwd = proyecto activo | G0 | abierto |
| I-6 | ¿`gpui-fast`? | No en v1 | G0 | abierto |
| I-7 | ¿Dock fijo 4 hojas o dockable libre? | Fijo 4 hojas (sin drag entre paneles) | G0 | abierto |

## Registro de gates

| Gate | Fecha | Verdicto | Informe | Pendientes |
|------|-------|----------|---------|-----------|
| P0 | — | — | — | — |
| G0 | — | — | — | — |
| G1 | — | — | — | — |
| G2 | — | — | — | — |
| G3 | — | — | — | — |
