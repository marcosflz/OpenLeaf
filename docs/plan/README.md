# LaTeX Studio — Plan de ejecución (agent swarm)

> **Estado: LISTO PARA EJECUCIÓN, pendiente de la orden del usuario.**
> Repo destino: `C:\Users\marco\Documents\.vsc\latex-ide\` (la distro actual se reemplaza; lo viejo pasa a `legacy\`).
> Fecha: 2026-10-17.

## Qué es

Clon open-source de Overleaf, portable y nativo en Rust: **4 paneles** (árbol de archivos → editor → PDF → chat IA), compilación al guardar, SyncTeX bidireccional y un chat basado en el **opencode portable** existente (`opencode serve`), todo dentro de una carpeta portable sin instalación.

## Decisiones fijadas por el usuario

| Tema | Decisión |
|---|---|
| Framework UI | **GPUI** vía `gpui-kit` 0.7.x (pinnea el snapshot `gpui-pre 0.3.9`; editor tree-sitter + LSP, dock, 75+ componentes) |
| Motor LaTeX | **TeX Live portátil actual** (`tex\`): latexmk + synctex + biber + tlmgr, tras un trait para añadir Tectonic en M4 |
| Plataformas | **Windows primero**, código portable |
| Editor | **tree-sitter desde el día 1** (`tree-sitter-latex`, MIT) |
| Ubicación | Proyecto nuevo en la raíz actual; `app\ data\ extensions\ ...` → `legacy\` (**nunca** se tocan `tex\`, `agent\`, `workspace\`) |

## Documentos (leer en este orden)

| Doc | Para qué |
|---|---|
| `PLAN-MAESTRO.md` | Visión, UX de los 4 paneles, arquitectura (crates, runtime, integraciones opencode/LaTeX/PDF/editor), presupuestos de rendimiento, riesgos R1–R10, licencias, hitos M0–M3 |
| `INTERFACES.md` | Contratos congelables: traits/firmas Rust, tabla de endpoints y eventos de opencode, esquema de `settings.json`, convenciones de unidades/coordenadas, fixtures, decisiones I-1..I-7 |
| `TAREAS.md` | Board completo: P0 (preflight + mover legacy con manifiesto), M0 (6 spikes + gate G0), M1–M3 por streams con criterios de aceptación, guion E2E de 12 pasos, M4 backlog |
| `SWARM.md` | Topología de agentes, worktrees/git, DoD y gates, **skills** oficiales y del proyecto, oleadas O0–O4, escalado/fallbacks, checklist de lanzamiento y plantillas de prompt |

## Arquitectura en una vista

```
latex-studio.exe (GPUI)
 ├─ lsx-ui      shell dock 4 paneles · temas · ajustes · palette
 ├─ lsx-editor  tree-sitter-latex · pestañas · diagnósticos · (M4: texlab)
 ├─ lsx-pdf     hayro (pure Rust) · caché LRU · SyncTeX coords
 ├─ lsx-chat    supervisor opencode serve · REST/SSE · permisos · reducer
 ├─ lsx-latex   LatexEngine (TeX Live) · CompileQueue · LogParser · tlmgr
 └─ lsx-core    raíz portable · config · proyectos · watcher · eventos
 Servicios en tokio (hilo propio) ← async_channel → entidades GPUI
```

## Hitos

| Hito | Resultado | Gate |
|---|---|---|
| **P0** | Git + legacy + esqueleto que compila + CI + skills + fixtures | CI verde |
| **M0** | 6 spikes (shell, editor TS, PDF hayro, chat opencode, engine+synctex, fs) | **G0**: INTERFACES congelado |
| **M1** | App usable: editar → compilar → PDF + syncTeX + chat con permisos | **G1**: E2E 1–8 + perf |
| **M2** | IA completa + pulido (outline, R/E, thumbs, tema claro, paleta) | **G2**: E2E 9–12 + QA adversarial |
| **M3** | Distribución portable (zips + instalador opcional + docs) | **G3**: instalación limpia |

## Cómo arrancar (cuando el usuario dé la orden)

1. Cambiar a un agente de ejecución (salir de Plan mode) y abrir sesión nueva de **orchestrator**.
2. Seguir `SWARM.md` §8 (checklist) — prompt de arranque incluido.
3. O0 (P0) → confirmación del usuario → O1 (M0) → gate G0 → olas O2–O4.

## Supuestos y limitaciones conocidas

- `gpui-kit`/GPUI es **pre-1.0**: versión pinneada exacta; upgrades de snapshot solo como tarea aislada (§R1).
- Selección/búsqueda de texto dentro del PDF queda para M4 (limitación de hayro; el visor v1 prioriza render + SyncTeX).
- `gpui-pdf`/`ratex-gpui` (repo zorite) son **GPL-3.0**: solo se leen como referencia de patrones; el código propio es MIT.
- texlab (LSP) es GPL-3.0: binario separado, M4, con avisos de terceros.
- Riesgo abierto R6 (instrucciones AGENTS.md del proyecto vs repo de desarrollo) tiene test explícito en G1 y plan B documentado.
- Todo bound a `git` + Windows/pwsh; Linux/macOS en M4.

## Fuentes principales (verificadas 2026-10-17)

- GPUI Kit: `github.com/longbridge/gpui-kit` (skills oficiales incluidas) · gpui-pre 0.3.9 = snapshot zed@93d7456
- OpenCode server: `opencode.ai/docs/server` (API v1; binario embebido 1.18.30) · skills: `opencode.ai/docs/skills`
- hayro 0.8 (`github.com/LaurenzV/hayro`) · tree-sitter-latex (MIT) · texlab v5.26 · TinyTeX/TeX Live 2026 ya presente en `tex\`
- Referencia de patrones (GPL, solo lectura): `github.com/packetThrower/zorite`
