---
id: kb/changes/changelog
title: Changelog incremental de OpenLeaf
status: stable
updated: 2026-10-24
source:
  - docs/kb/changes/PLAN-0.2.md
  - git log
tags: [cambios, historia]
related: [kb/changes/plan-0.2]
verified: true
---

# Changelog — OpenLeaf / LaTeX Studio

> Registro incremental de cambios con **porqué**, **territorio** y **evidencia**.
> Formato: `CHG-NNNN`. Estado: `planned` · `doing` · `done` · `reverted`.
> El estado vivo de las tareas está en `PLAN-0.2.md`; aquí queda la historia.

## CHG-0007 — Validación automática del OKF

- **Fecha:** 2026-10-24
- **Tarea:** `P2-KB-1`
- **Estado:** done
- **Territorio:** `xtask/src/{main,kb}.rs`, `AGENTS.md`, `docs/kb/schema.md`
- **Qué cambia:** `cargo run -p xtask -- kb check` valida frontmatter, `id` ↔ ruta, estados,
  fechas, enlaces relativos y que el changelog tenga entradas `## CHG-`.
- **Por qué:** evitar que el OKF se degrade a documentación muerta.
- **Evidencia:** 18 notas válidas; 3 tests en `xtask`.

## CHG-0006 — Tema claro coherente en el chat y retirada de la demo de permisos

- **Fecha:** 2026-10-24
- **Tarea:** `P2-UI-2`, `P2-UI-4`
- **Estado:** done
- **Territorio:** `crates/ui/src/panels/chat.rs`, `palette.rs`, `app.rs`
- **Qué cambia:** el chat resuelve fondo/código/superficies desde `cx.theme()` (antes tenía
  una paleta oscura fija que rompía el tema claro); se retira el botón “Probar permiso” y
  su comando de la paleta.
- **Por qué:** el tema claro dejaba el chat oscuro; la demo era un resto de MVP visible.
- **Evidencia:** capturas `shots/p2-settings-light-05.png`; 104 tests UI; clippy UI verde.

## CHG-0005 — Ventana, menús, cierre seguro y persistencia del picker

- **Fecha:** 2026-10-24
- **Tarea:** `P2-UI-3`, `P2-AI-2`
- **Estado:** done
- **Territorio:** `app/src/main.rs`, `crates/ui/src/app.rs`, `panels/{chat,editor}.rs`,
  `crates/core/src/config.rs`, `docs/plan/INTERFACES.md` §7
- **Qué cambia:** `ui.window` persiste geometría/maximizado con clamp a monitores; barra de
  menús Archivo/Ver/Herramientas/Ayuda; título con proyecto y `•` dirty; cierre con
  Guardar/Descartar/Cancelar; `Ctrl+B/S/Q/F1`; picker de modelo/agente persiste en
  `ai.model`/`ai.agent`. Fix: `validate()` acepta `project`/`all` para permisos.
- **Por qué:** la ventana no se comportaba como una app de escritorio y el chat olvidaba
  el modelo al reiniciar.
- **Evidencia:** `docs/qa/reports/P2-UI-3-2026-10-24.md`, `shots/p2-window-*.png`,
  test `dirty_count_y_save_all_cierran_el_flujo_de_ventana` y test de bounds/menús.

## CHG-0004 — Instalador con motor LaTeX automático

- **Fecha:** 2026-10-24
- **Tarea:** `P2-INST-1`
- **Estado:** done
- **Territorio:** `installer/latex-studio.iss`, `installer/build.ps1`, `docs/release.md`,
  `docs/kb/architecture/05-instalador.md`, informe `P2-INST-1-2026-10-24.md`
- **Qué cambia:** página “Motor LaTeX”; descarga TinyTeX-1 fijada y verificada; extracción,
  `tlmgr update --self`, `synctex`/`biber` + paquetes base (incluido `grfext`); log,
  marcador y fallback sin abortar la instalación.
- **Por qué:** el perfil ligero no compilaba al abrir por primera vez.
- **Evidencia:** instalación silenciosa limpia con ruta con espacios; `article`, `report` y
  `beamer` compilan a PDF con SyncTeX; `ISCC` exit 0.

## CHG-0003 - Ajustes 2.0: temas, IA y configuración real

- **Fecha:** 2026-10-24
- **Tarea:** `P2-UI-1`, `P2-UI-2`, `P2-AI-1`
- **Estado:** done
- **Territorio:** `crates/ui/src/settings_ui.rs`, `theme.rs`, `ai_setup.rs`, `lib.rs`,
  `locales/es.yml`, `docs/kb/architecture/{03,04,06}-*.md`
- **Qué cambia:**
  - Ajustes pasa de 5 campos a un diálogo de 880 px con navegación lateral (Apariencia ·
    Editor · Compilación · PDF · IA · Avanzado), borrador validado, errores inline,
    estado sucio y reset con confirmación.
  - Tema `system` real vía `Theme::sync_system_appearance`; cinco presets de acento +
    hex validado; preview en vivo con restauración al cancelar; tema claro con alphas
    más suaves y `reducedMotion` proyectado en `App::set_reduce_motion`.
  - `ai_setup.rs` ejecuta `auth list`, `models` y `auth login` en background con
    timeout; la sección IA muestra estado del ejecutable, proveedores, catálogo,
    selector de modelo, agente, permisos, “Probar” y apertura de `agent/`.
- **Por qué:** Ajustes/IA/temas eran las señales de MVP más visibles de la auditoría
  (hallazgos 8–15 y 22–26); además, `system` y el acento existían en el esquema sin UI.
- **Evidencia:**
  - `cargo check --workspace` ✅
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo test --workspace` ✅ (incluye 104 tests de `lsx-ui`) y
    `cargo test -p lsx-ui` ✅ (parsers de opencode, validación de rangos, persistencia
    de `settings.json` y test GPUI de `system`).
  - `cargo fmt --check` ✅ y `rustfmt --check` de los archivos del stream ✅.
  - `cargo run -p xtask -- kb check` ✅ 18 notas válidas.
  - Capturas dark/light de la superficie de Ajustes en
    `%TEMP%\lsx-ui-final-captures\settings-{dark,light}.png` (temporales, no versionadas)
    y capturas de QA en `shots/p2-settings-*.png`.
- **Limitaciones:** `agent/opencode.exe` no está en el repo, así que el flujo live de
  auth/modelos no se pudo ejercitar. El shell ya reaplica autosave y zoom/modo del PDF en
  caliente; quedan `pdf.openExternalOnDoubleClick` y el filtro de `advanced.logLevel`
  (documentado en `03-configuracion.md`).

## CHG-0002 — Plan incremental 0.2 y auditoría de producto

- **Fecha:** 2026-10-24
- **Tarea:** `P2-KB-1`
- **Estado:** done
- **Territorio:** `docs/kb/**`
- **Qué cambia:** se documentan 41 hallazgos con severidad, evidencia y tarea; se define la
  ola O-1/O-2 y el gate G2.5.
- **Por qué:** ejecutar el pulido sin perder el hilo ni re-investigar.
- **Evidencia:** `docs/kb/quality/01-auditoria-2026-10-24.md`, `PLAN-0.2.md`.

## CHG-0001 — OKF (wiki del proyecto) enlazado a AGENTS.md

- **Fecha:** 2026-10-24
- **Tarea:** `P2-KB-1`
- **Estado:** done
- **Territorio:** `docs/kb/**`, `AGENTS.md`, `.opencode/skills/lsx-knowledge-base/**`
- **Qué cambia:** se crea un wiki de notas atómicas con índice, esquema, plantillas y
  protocolo de mantenimiento; `AGENTS.md` obliga a leerlo y actualizarlo.
- **Por qué:** el conocimiento estaba disperso entre plan, board, informes y skills; los
  agentes re-investigaban o tomaban decisiones con contexto parcial.
- **Evidencia:** `docs/kb/index.md`, `docs/kb/README.md`, `docs/kb/schema.md`.
- **Riesgo:** notas que se queden viejas; mitigado con `source`, `updated`, `verified` y el
  checklist de cierre.
