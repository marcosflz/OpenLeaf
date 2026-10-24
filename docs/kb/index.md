# Índice del OKF — LaTeX Studio / OpenLeaf

> **Empieza aquí.** Busca por la pregunta que tienes, no por el nombre del archivo.
> Estado vivo: `docs/board.md` · Plan incremental: `changes/PLAN-0.2.md` · Historia: `changes/CHANGELOG.md`.

## Tabla de recuperación

| Si necesitas… | Lee |
|---|---|
| Saber qué es el producto y qué promete | `product/01-que-es.md`, `product/02-promesas-ux.md` |
| Entender el primer arranque y qué debe haber instalado | `product/03-primer-arranque.md`, `operations/04-latex-bootstrap.md` |
| Saber qué crate posee un cambio | `architecture/01-crates.md`, `docs/plan/SWARM.md` §2 |
| Seguir el flujo abrir → editar → compilar → PDF → chat | `architecture/02-flujos.md` |
| Entender `config/settings.json` y qué campos están cableados | `architecture/03-configuracion.md` |
| Trabajar con opencode (server, SSE, modelos, auth) | `architecture/04-chat-opencode.md`, skill `lsx-opencode-api` |
| Tocar el instalador o la distribución | `architecture/05-instalador.md`, `operations/03-release.md`, skill `lsx-dist-portable` |
| Tocar dock, paneles, toolbar, ajustes o temas | `architecture/06-ui-shell.md`, skills `gpui-kit` y `gpui-kit-design-guides` |
| Preparar el entorno de desarrollo | `operations/01-desarrollo.md` |
| Ejecutar gates y recoger evidencia | `operations/02-gates.md`, skill `lsx-quality-gates` |
| Instalar TeX Live/TinyTeX desde cero | `operations/04-latex-bootstrap.md` |
| Ver los problemas abiertos y su severidad | `quality/01-auditoria-2026-10-24.md` |
| Revisar una pantalla antes de darla por buena | `quality/03-checklist-ui.md` |
| Saber qué se está ejecutando ahora | `changes/PLAN-0.2.md` |
| Ver qué cambió y con qué evidencia | `changes/CHANGELOG.md` |

## Notas

### Producto

| ID | Nota | Estado | Actualizado |
|---|---|---|---|
| `kb/product/que-es` | [Qué es LaTeX Studio](product/01-que-es.md) | stable | 2026-10-24 |
| `kb/product/promesas-ux` | [Promesas de experiencia](product/02-promesas-ux.md) | stable | 2026-10-24 |
| `kb/product/primer-arranque` | [Primer arranque](product/03-primer-arranque.md) | draft | 2026-10-24 |

### Arquitectura

| ID | Nota | Estado | Actualizado |
|---|---|---|---|
| `kb/architecture/crates` | [Crates y ley de dependencias](architecture/01-crates.md) | stable | 2026-10-24 |
| `kb/architecture/flujos` | [Flujos de extremo a extremo](architecture/02-flujos.md) | stable | 2026-10-24 |
| `kb/architecture/configuracion` | [Configuración y estado de usuario](architecture/03-configuracion.md) | stable | 2026-10-24 |
| `kb/architecture/chat-opencode` | [Chat y opencode](architecture/04-chat-opencode.md) | stable | 2026-10-24 |
| `kb/architecture/instalador` | [Instalador y distribución](architecture/05-instalador.md) | draft | 2026-10-24 |
| `kb/architecture/ui-shell` | [Shell, paneles y temas](architecture/06-ui-shell.md) | stable | 2026-10-24 |

### Operaciones

| ID | Nota | Estado | Actualizado |
|---|---|---|---|
| `kb/operations/desarrollo` | [Desarrollo local](operations/01-desarrollo.md) | stable | 2026-10-24 |
| `kb/operations/gates` | [Gates y evidencia](operations/02-gates.md) | stable | 2026-10-24 |
| `kb/operations/release` | [Release y publicación](operations/03-release.md) | stable | 2026-10-24 |
| `kb/operations/latex-bootstrap` | [Bootstrap de LaTeX](operations/04-latex-bootstrap.md) | draft | 2026-10-24 |

### Calidad

| ID | Nota | Estado | Actualizado |
|---|---|---|---|
| `kb/quality/auditoria-2026-10-24` | [Auditoría de producto y UI](quality/01-auditoria-2026-10-24.md) | stable | 2026-10-24 |
| `kb/quality/deuda-tecnica` | [Deuda técnica priorizada](quality/02-deuda-tecnica.md) | stable | 2026-10-24 |
| `kb/quality/checklist-ui` | [Checklist de revisión de UI](quality/03-checklist-ui.md) | stable | 2026-10-24 |

### Cambios

| ID | Documento | Estado | Actualizado |
|---|---|---|---|
| `kb/changes/plan-0.2` | [Plan incremental 0.2](changes/PLAN-0.2.md) | **DOING** | 2026-10-24 |
| `kb/changes/changelog` | [Changelog](changes/CHANGELOG.md) | viva | 2026-10-24 |

## Notas por crear (backlog de conocimiento)

- `kb/quality/perf` — presupuestos medidos (`docs/perf.md` aún no existe).
- `kb/architecture/editor` — tree-sitter, sesión, autosave y find/replace en detalle.
- `kb/architecture/pdf` — caché LRU, coordenadas y SyncTeX en detalle.
- `kb/operations/telemetry` — logs, diagnóstico `--diagnose` y soporte.
