---
id: kb/quality/deuda-tecnica
title: Deuda técnica priorizada
status: stable
updated: 2026-10-24
source:
  - docs/kb/quality/01-auditoria-2026-10-24.md
  - crates/core/src/config.rs
  - crates/ui/src
tags: [calidad, deuda-tecnica]
related: [kb/changes/PLAN-0.2, kb/quality/01-auditoria-2026-10-24]
verified: true
---

# Deuda técnica priorizada

## P0 — antes de llamar “producto”

1. **Motor LaTeX en el instalador** (P2-INST-1): TinyTeX + verificación + progreso + fallback.
2. **Ajustes que gobiernan de verdad** (P2-UI-1): no exponer campos muertos; cablear autosave,
   zoom/modo PDF, `texBinOverride` y `opencodeExeOverride`.
3. **Conexión de IA** (P2-AI-1): auth de opencode, modelos, test y persistencia de
   modelo/agente.
4. **Cierre seguro** (P2-UI-3): `on_window_should_close` + dirty + diálogo de 3 opciones.

## P1 — acabado visible

5. **Menú nativo + atajos visibles** (P2-UI-3): Archivo/Ver/Herramientas/Ayuda.
6. **Tema sistema + acento** (P2-UI-2): `Theme::sync_system_appearance`, presets, QA claro.
7. **Bienvenida / estado vacío** (P2-UI-4): nuevo proyecto, abrir, recientes, instalar TeX.
8. **Retirar demo** (P2-UI-4): permiso demo, copy interna, “spike”.
9. **Persistencia de ventana** (P2-UI-3): bounds + maximizado + clamp multi-monitor.

## P2 — higiene

10. **Unificar settings** en una sola fuente tipada; validar y testear cada campo.
11. **Probar el instalador en limpio** (VM/PC sin TeX) con y sin red.
12. **Logs/diagnóstico** exportable y “abrir carpeta”.
13. **Accesibilidad**: foco visible, orden de tabulación, labels, contraste.
14. **i18n**: decidir si se queda en `es` y ocultar el campo, o preparar `en`.
15. **Tests de UI** para ajustes, tema, cierre y menús.

## Riesgos de no pagarla

- El usuario nuevo no compila → abandona en el primer minuto.
- El usuario avanzado no puede conectar su suscripción → el chat queda como demo.
- La ventana no recuerda nada y cierra sin avisar → pérdida de trabajo y desconfianza.
- Los ajustes muestran opciones que no hacen nada → percepción de producto roto.
