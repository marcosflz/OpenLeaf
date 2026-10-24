---
id: kb/architecture/configuracion
title: Configuración, estado de usuario y qué campos están cableados
status: stable
updated: 2026-10-24
source:
  - crates/core/src/config.rs
  - crates/ui/src/settings_ui.rs
  - crates/ui/src/ai_setup.rs
  - crates/ui/src/theme.rs
  - crates/ui/src/app.rs
  - docs/plan/INTERFACES.md §7
tags: [arquitectura, configuracion]
related: [kb/architecture/ui-shell, kb/quality/01-auditoria-2026-10-24]
verified: true
---

# Configuración, estado de usuario y qué campos están cableados

## Ficheros

| Ruta | Qué es | ¿Va en git? |
|---|---|---|
| `config/settings.json` | preferencias del usuario | no |
| `config/opencode.runtime.json` | config efectiva de opencode (rutas absolutas + skills) | no (se genera) |
| `config/backups/` | backups y sesión del editor | no |
| `agent/opencode.json` | config del producto (modelo, permisos, provider Ollama) | sí |
| `workspace/` | proyectos del usuario | no |

`Settings::load` nunca falla: defaults + `ConfigWarning` por campo inválido. `save` es
atómico (temp + rename) y preserva campos desconocidos (`ExtraFields`).

## Secciones de `settings.json`

| Sección | Campos | Estado de cableado (2026-10-24) |
|---|---|---|
| `project` | `last`, `recent` | usado por `ProjectService` |
| `editor` | `fontFamily`, `fontSize`, `tabSize`, `softWrap`, `autosave`, `showWhitespace` | `autosave` se pasa a `EditorPanel` al arrancar y se aplica en caliente en `apply_settings`; el resto sigue en defaults |
| `latex` | `engine`, `mainFile`, `autoCompile`, `debounceMs`, `autoInstallPackages`, `extraArgs`, `timeoutSecs`, `cleanOnSuccess`, `syncScroll` | usado por `CompileController`/shell, salvo `engine` (solo texlive) |
| `pdf` | `zoomMode`, `zoom`, `maxCacheMB`, `openExternalOnDoubleClick` | caché, modo y zoom se pasan a `PdfPanel`; el doble clic aún no está conectado |
| `ai` | `autoStart`, `port`, `model`, `agent`, `permissionPreset`, `showReasoning`, `promptTemplatesDir` | `ChatPanelConfig` los lee al construir el panel; el picker del chat ya persiste modelo/agente (P2-AI-2). Los cambios hechos en Ajustes se aplican al reiniciar el chat. `permissionPreset` acepta `ask`/`project`/`all` (`auto` como alias) |
| `ui` | `theme`, `accent`, `language`, `dock`, `window`, `reducedMotion` | tema/acento/dock/ventana sí; `system` consulta el SO al aplicar y sigue cambios con `observe_window_appearance`; `reducedMotion` se proyecta en `App::set_reduce_motion`; `language` solo `es` |
| `advanced` | `logLevel`, `texBinOverride`, `opencodeExeOverride` | log level se persiste; el filtro de `tracing` aún no lo lee. Los overrides **sí se aplican** al arrancar (`resolve_tex_bin`/`resolve_opencode_exe`), pero se editan a mano en `settings.json` |

## Ajustes UI actuales (P2-UI-1)

`settings_ui::open_dialog(window, cx, settings, weak)` conserva la firma para el shell y abre
un `Dialog` de 880 px con navegación lateral:

**Apariencia · Editor · Compilación · PDF · IA · Avanzado**

- **Apariencia:** tema (`dark`/`light`/`system`), cinco presets de acento
  (`theme::ACCENT_PRESETS`) y hex personalizado validado con
  `lsx_core::config::parse_accent`. El tema/acento se previsualizan en vivo; Cancelar y
  Escape restauran el valor original. `system` usa `Theme::sync_system_appearance`.
- **Editor:** `editor.autosave.enabled` e `intervalSecs` (5–3600). Se aplican al
  guardar los ajustes y al arrancar (`EditorPanelConfig`).
- **Compilación:** `autoCompile`, `syncScroll`, `autoInstallPackages`, `cleanOnSuccess`,
  `debounceMs` (0–10000), `timeoutSecs` (5–7200), `mainFile`, `extraArgs` y el motor
  (solo lectura: `texlive`).
- **PDF:** `maxCacheMB` (16–4096), `zoomMode` (`fit-width`/`fit-page`/`actual`/`custom`),
  `zoom` (0,1–8,0) y `openExternalOnDoubleClick`. `apply_settings` pasa modo y zoom al
  `PdfPanel`; la caché se aplica al reiniciar y el doble clic aún no está conectado (la
  UI lo advierte).
- **IA:** estado del ejecutable y comandos de opencode (ver
  [`04-chat-opencode`](04-chat-opencode.md)), `autoStart`, `port` opcional
  (1024-65535), modelo, agente, preset de permisos (`ask`/`project`/`all`) y `showReasoning`.
  Los cambios se aplican al reiniciar el chat.
- **Avanzado:** nivel de log, ruta de `settings.json`, “Abrir carpeta de configuración”,
  “Abrir `agent/opencode.json`” y “Restablecer valores por defecto” con confirmación.
  El reset conserva `project` y `ui.dock`.

El borrador `SettingsDraft` valida en un paso y es atómico: si un campo falla, muestra el
resumen y los errores junto a los controles, y **no** cierra el diálogo. Al guardar,
`AppRoot::apply_settings` vuelve a validar (`Settings::validate`) y escribe de forma
atómica en `config/settings.json`.

## Regla para el plan 0.2

- Un campo que aparece en Ajustes **debe** tener efecto observable. Si no está cableado, o se
  cablea o no se muestra.
- Los defaults viven en `lsx-core`; la UI no inventa valores.
- `config/settings.json` es estado de usuario: nunca se versiona ni se incluye en el
  instalador.
