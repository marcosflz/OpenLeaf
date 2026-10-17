# SWARM — Topología de agentes, skills y protocolo de ejecución

> Este documento define **cómo se ejecuta el plan** (PLAN-MAESTRO + TAREAS + INTERFACES) con OpenCode.
> Se activa **solo cuando el usuario dé la orden**. Hasta entonces, todo queda en modo plan.

---

## 1. Topología

```
┌────────────────────────────────────────────────────────────────┐
│  ORCHESTRATOR (sesión primaria, modelo top)                    │
│  · dueño del board, INTERFACES.md, Cargo.toml raíz, .opencode  │
│  · ejecuta spikes críticos, decide gates, mergea streams       │
│  · único que habla con el usuario en nombre del swarm          │
└──────┬──────────────┬──────────────┬──────────────┬────────────┘
       │              │              │              │
 ┌─────▼────┐   ┌─────▼────┐   ┌─────▼────┐   ┌─────▼────┐
 │ STREAM A │   │ STREAM B │   │ STREAM C │   │ STREAM D │   … (máx 4 activos)
 │ shell/ux │   │ editor   │   │ pdf      │   │ latex    │
 │ worktree │   │ worktree │   │ worktree │   │ worktree │
 └──────────┘   └──────────┘   └──────────┘   └──────────┘
       │              │              │              │
 ┌─────▼──────────────▼──────────────▼──────────────▼──────────┐
 │ QA VERIFIER (sesión independiente, modelo top)               │
 │ · nunca revisa código que escribió                           │
 │ · ejecuta gates + E2E + adversarial por gate de hito         │
 └──────────────────────────────────────────────────────────────┘
 STREAMS EN COLA (se activan cuando hay hueco): E chat · F fs · G packaging
```

Roles:

| Rol | Sesión | Modelo recomendado | Responsabilidad |
|---|---|---|---|
| Orchestrator | Primaria | Top (razonamiento fuerte) | Board, interfaces, merges, gates, ADRs, P0 |
| Spike agent | Hija | Top para S2/S3/S4; medio para S1/S5/S6 | Un spike de M0; entrega demo + respuestas de interfaz |
| Stream agent | Hija (worktree) | Coder medio/rápido | Tareas de TAREAS.md de su stream |
| QA verifier | Hija | Top | Gates, E2E, adversarial, informes en `docs/qa/reports/` |
| Docs agent | Hija | Medio | READMEs, guías, THIRD-PARTY, demo (M3) |

Reglas de contexto (clave para no quemar tokens):
- Los agentes leen **secciones concretas** de los docs del plan (links en cada tarea), no el pack completo.
- Los skills del proyecto (§4) son la vía preferente para conocimiento estable (APIs, convenciones, comandos).
- Prohibido re-investigar lo ya documentado en INTERFACES.md; si falta, se añade allí y se propaga.

---

## 2. Protocolo de git y worktrees

1. `main` = trunk estable; CI verde obligatoria.
2. Stream: `git worktree add .worktrees/<stream> -b feat/<stream>-<tema>`.
3. Commits convencionales: `feat(editor): ...`, `fix(pdf): ...`, `chore(ci): ...`.
4. Rebase sobre `main` antes de pedir review (mínimo diario).
5. Merge: QA verifica en la rama → orchestrator `git merge --no-ff` → actualiza `docs/board.md`.
6. Un PR/tarea = una unidad revisable (< ~600 líneas netas ideal). Tareas grandes → subtareas.
7. `.worktrees/` en `.gitignore`.

**Matriz de propiedad de archivos** (evita conflictos):

| Path | Dueño | Notas |
|---|---|---|
| `crates/core/**` | F + orchestrator | FsService/project/config |
| `crates/latex/**` | D | |
| `crates/pdf/**` | C | |
| `crates/chat/**` | E | |
| `crates/editor/**` | B | |
| `crates/ui/**` | A | Paneles consumen, no implementan lógica de otros |
| `app/**`, `xtask/**` | Orchestrator | Wiring, dist |
| `Cargo.toml` raíz, CI, `.opencode/**`, `AGENTS.md`, docs del plan | Orchestrator | Cambios externos solo vía nota en PR |
| `docs/**` | Docs agent + orchestrator | ADRs siempre por orchestrator |
| `fixtures/**` | Quien la crea; modificación = PR del orchestrator | Inmutables una vez usadas |

Si dos streams necesitan tocar el mismo archivo → el orchestrator extrae un cambio base a `main` primero.

---

## 3. Definiciones

**Ready (listo para empezar)**: interfaces existen (INTERFACES.md o ADR de tarea), fixtures disponibles, DoD claro, rama/worktree creada, skills relevantes instaladas.

**DoD (terminado)**:
1. Código + tests (unit; integración si aplica) que cubren criterios de aceptación.
2. Gates §5 verdes local y en CI.
3. Sin `todo!()`/`unwrap()` en rutas de producción; errores tipados con mensaje de usuario.
4. Docs afectadas actualizadas (INTERFACES/skills/README de crate).
5. Evidencia para cambios visibles: captura/GIF (guardado en el PR/reporte) + pasos de reproducción.
6. `docs/board.md` actualizado (estado + notas + enlaces).
7. Perf dentro del presupuesto (si la tarea toca render/IO).

**Gate (hito)**: lo firma QA verifier en `docs/qa/reports/G<N>-<fecha>.md` con: E2E ejecutado (con evidencia), gates técnicos, presupuestos medidos, riscos nuevos, checklist de riesgos §7 de PLAN-MAESTRO.

---

## 4. Skills (kit de conocimiento del proyecto)

### 4.1 Oficiales (vendorizar desde `longbridge/gpui-kit`)

| Skill | Origen | Nota |
|---|---|---|
| `gpui-kit` | `skills/gpui-kit/` del repo (≈22 KB + `references/`) | Incluye Coding Guides (obligatorias antes de decidir arquitectura de UI, estado, APIs, tests) |
| `gpui-kit-design-guides` | `skills/gpui-kit-design-guides/` | Design Guides: layout, spacing, jerarquía, estados, overlays, copy |

Instalación: `npx skills add longbridge/gpui-kit` **o** copiar las carpetas a `.opencode/skills/` (preferible vendorizar para versión fija). Actualizar solo con bump del snapshot.

### 4.2 Del proyecto (los escribe P0.5; contenido mínimo obligatorio)

| Skill | Contenido | Usado por |
|---|---|---|
| `lsx-architecture` | Grafo de crates + ley de dependencias; decisión "¿dónde va este código?"; flujo AppEvent/intents; enlaces a INTERFACES/TAREAS; convenciones de nombres y errores | Todos |
| `lsx-gpui-patterns` | Ownership de entidades por panel; patrón reducer (estado,evento→estado); puente tokio→GPUI (`async_channel` + `cx.spawn`, runtime en hilo propio); `cx.notify()` y cuándo; registro de `actions`/keybinds; tests con `TestAppContext`; antipatrones (bloquear UI, repaint storms, estado global ilegible); punteros a Coding Guides y `examples/editor`, `examples/dock`, `examples/stream-markdown` del kit | A, B |
| `lsx-editor-ts` | Cómo registrar `tree-sitter-latex`; ubicación de queries; mapeo capturas→estilos; marcadores de diagnóstico; APIs del editor del kit descubiertas en S2 (con snippets) | B |
| `lsx-latex-engine` | Rutas exactas (`tex\bin\windows\...`); comandos latexmk/synctex/tlmgr completos con flags; contrato LogParser + fixtures; flujo auto-install; reglas job-object/timeout | D |
| `lsx-pdf-hayro` | Uso de hayro (abrir, render página→RGBA); convención de coordenadas con ejemplos numéricos; patrón de subida a `RenderImage`; política de caché; cómo medir con criterion | C |
| `lsx-opencode-api` | Cómo levantar `serve` local; tabla de endpoints; **nombres exactos de eventos SSE** (de fixtures); flujo completo de permisos; env vars; política de restart; reproductor de fixtures (`xtask chat-replay`) | E |
| `lsx-quality-gates` | Comandos exactos (§5); cómo correr UI tests; cuándo actualizar snapshots `insta`; presupuestos de rendimiento; plantilla de evidencia | Todos |
| `lsx-ai-fix-flow` | Plantillas de prompt ("Arreglar errores"), definición del agente `latex-fix`, flujo E2E con permisos y watcher; cómo probarlo sin gastar tokens reales (mock) | E, QA |
| `lsx-dist-portable` (M3) | `xtask dist`, layout del zip, checks de primer arranque, Inno | Docs agent |

Formato (verificado): `.opencode/skills/<name>/SKILL.md` con frontmatter YAML `name` + `description` (≤1024 chars, orientada a auto-invocación); material de apoyo en `references/`; nombres `lowercase-con-guiones`.

### 4.3 Agentes (`.opencode/agent/*.md`)

Crear con frontmatter (`description`, `mode: primary|subagent`, `model` opcional, `tools`, `permission`) y prompt corto que referencie: `AGENTS.md` → `docs/board.md` → skill correspondiente → tarea concreta.

| Agente | Charter (resumen del prompt) |
|---|---|
| `orchestrator` | Eres el jefe del swarm. Mantienes board/interfaces/merges. Nunca implementas tareas de stream salvo cambio base. Pides ADR ante cualquier cambio de interface. Reportas al usuario en cada gate con: estado, evidencia, riesgos, siguiente ola. |
| `stream-a-shell` … `stream-f-blocked` | Eres el head del stream X. Solo tocas tu crate. Toda tarea pasa por DoD. Si necesitas cambiar algo fuera de tu territorio, paras y pides al orchestrator. Usas los skills `lsx-*` correspondientes. |
| `qa-verifier` | Verificas, no arreglas. Ejecutas gates + E2E con evidencia. Buscas adversarialmente (rutas raras, archivos enormes, procesos muertos, red caída). Escribes informe y propones tareas de bug (nunca \"arreglado\" sin reproducción). |
| `docs-writer` | READMEs, guías de usuario, THIRD-PARTY, demo. Nunca toca código. |

---

## 5. Gates técnicos (comandos exactos, Windows/pwsh)

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace                       # unit
$env:LSX_TEST_TEXLIVE="1"; cargo nextest run -p lsx-latex   # integración real (requiere tex\)
cargo deny check licenses bans sources
cargo xtask pdf-golden                              # comparativa PNG
taplo fmt --check **/Cargo.toml
typos
```

Presupuestos (PLAN-MAESTRO §6.3) se miden con `cargo xtask bench` + overlay dev `F12` y se registran en `docs/perf.md` en cada gate.

---

## 6. Mecánica de oleadas

| Ola | Quién | Entregable de la ola | Sincronización |
|---|---|---|---|
| O0 | Orchestrator (+1 agente) | P0.1–P0.5 listo, CI verde, skills instaladas | Revisión del usuario: "adelante con M0" |
| O1 | 6 spike agents ∥ (S1–S6), orchestrator consolida | G0: INTERFACES congelado + demos | Revisión del usuario: gate G0 (elige/valida fallbacks) |
| O2 | Streams A–D ∥ (E–F en cola) | M1 completo | QA G1 → revisión usuario |
| O3 | Streams E, F + A/B/C pulido | M2 completo | QA G2 → revisión usuario |
| O4 | Docs + orchestrator | M3 (dist/installer/README) | QA G3 → aceptación final usuario |

Dentro de una ola: el orchestrator lanza agentes en **una sola tanda** por stream, con contexto mínimo (§1) y les exige PR + evidencia; los bloqueos se escalan en < 1 sesión.

---

## 7. Escalado y fallbacks

| Situación | Acción |
|---|---|
| Spike falla su criterio | Usar fallback de TAREAS.md §3; registrar ADR; ajustar INTERFACES |
| Stream bloqueado > 1 sesión | Orchestrator re-scopea (divide tarea) o asigna a otro modelo/agente |
| Bug encontrado por QA | Tarea de bug en board con reproduc, evidencia y severidad; nunca se cierra sin fix + test |
| Cambio de interface necesario | ADR + actualización INTERFACES + nota a afectados; el orchestrator aplica el cambio base |
| Snapshot GPUI roto | `[patch]` vendored en `.cargo/` con nota de licencia; o revert a pin anterior |
| Feature creep | Todo lo no listado → M4 backlog sin discusión |
| Token burn alto | Reducir olas a 3 streams; usar fixtures/mocks; subir solo S2/S3/S4 a modelo top |

---

## 8. Checklist de lanzamiento (cuando el usuario dé la orden)

1. **Salir de Plan mode** y abrir una sesión nueva como `orchestrator` (con el agente ya definido).
2. Confirmar entorno: `rustc --version` (≥1.92), MSVC build tools presentes (`link.exe`), `npx` disponible (skills), rama limpia.
3. Ejecutar O0 tal cual TAREAS.md P0.1–P0.5. **Nada de código de producto antes de P0.5**.
4. Prompt de arranque del orchestrator (plantilla):

```
Lee C:\Users\marco\.opencode\plan\latex-studio\ (“README.md”, luego “TAREAS.md P0” y “SWARM.md”).
Ejecuta P0 completo en orden, con evidencia por tarea.
Al terminar P0: reporta board + CI + skills instaladas, y espera mi “adelante con M0”.
No inicies M0 sin mi confirmación.
```

5. Al confirmar M0: el orchestrator lanza O1 (spikes S1–S6 ∥), consolida resultados y convoca gate G0 con el usuario.
6. Tras G0: olas O2→O4 según SWARM §6.

---

## 9. Plantillas de prompt para agentes hijos (usar literalmente, rellenando `<>`)

**Spike agent**
```
Rol: spike agent. Repo: <repo>. Worktree: <path>.
Lee: TAREAS.md §3 task <Sx>, INTERFACES.md <secciones>, skill <lsx-...>.
Objetivo: demo funcional que responda "<pregunta del spike>".
Entregable: rama + demo (comando exacto para reproducir) + respuestas a las preguntas de interfaz + riesgos.
No refactorices nada fuera del spike. No toques archivos del orchestrator. Gates locales verdes.
```

**Stream agent**
```
Rol: head del stream <X> (crate <lsx-...>). Worktree: <path>.
Lee: docs/board.md (tus tareas <IDs>), INTERFACES.md (<secciones>), skills <...>.
Ejecuta tareas en orden. Por tarea: rama/tarea, tests, gates, evidencia, PR pequeño, actualizar board.
Si algo exige salir de tu crate: para y pregunta al orchestrator.
```

**QA verifier**
```
Rol: QA verifier. Revisa <rama/PR/commit> contra TAREAS.md <IDs> y DoD.
Ejecuta: gates completos (§5), E2E <pasos>, adversarial <lista>.
Salida: docs/qa/reports/<gate>.md con evidencia (capturas/logs), bugs reproducibles y veredicto PASS/FAIL.
Nunca “arregles” código: solo verificas y reportas.
```

---

## 10. Presupuesto de esfuerzo estimado (referencia)

| Fase | Puntos totales | Notas |
|---|---|---|
| P0 | 14 | 1–2 sesiones de orchestrator |
| M0 | 23 + freeze | 6 spikes ∥, 1 ola |
| M1 | ~66 | 4 streams activos, 2–3 oleadas |
| M2 | ~38 | depende de G1 |
| M3 | ~21 | docs + dist |
| Total | **~162 pts** | ≈ 40–60 sesiones-agente; se ajusta en cada gate |
