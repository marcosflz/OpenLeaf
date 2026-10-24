---
id: kb/quality/checklist-ui
title: Checklist de revisión de UI antes de cerrar una tarea
status: stable
updated: 2026-10-24
source:
  - .opencode/skills/gpui-kit-design-guides/references/design-guides.md
  - docs/kb/product/02-promesas-ux.md
tags: [calidad, ui, diseno]
related: [kb/product/02-promesas-ux, kb/quality/01-auditoria-2026-10-24]
verified: true
---

# Checklist de revisión de UI antes de cerrar una tarea

## Tarea y jerarquía

- [ ] La pantalla deja claro cuál es la tarea principal y la siguiente acción.
- [ ] Hay un solo foco visual por superficie; no compiten badges/colores/box.
- [ ] El estado vacío, cargando, error, offline y sin permiso están diseñados.
- [ ] La acción principal no está escondida en un icono de una esquina.

## Componentes y tokens

- [ ] Se usan componentes del kit (`Button`, `Input`, `Dialog`, `Menu`, `Switch`…), no
      invenciones locales.
- [ ] Colores por token (`cx.theme()`), sin hex/rgb en paneles.
- [ ] Radios, spacing y tamaños salen de la escala; no hay números mágicos.
- [ ] Iconos en slot fijo; labels no se mueven si falta un icono.
- [ ] Botones primario/secundario/ghost/danger tienen una jerarquía consistente.

## Estados

- [ ] Hover, foco, seleccionado, disabled, loading y error son distinguibles.
- [ ] El foco de teclado es visible y no lo roba el chrome del panel.
- [ ] Escape cierra el overlay superior y devuelve el foco al disparador.
- [ ] Las acciones destructivas piden confirmación con el objeto y el verbo.

## Copy

- [ ] Español, sin jerga interna (“spike”, “dot”, “M1”).
- [ ] El error dice qué pasó y qué hacer; no solo “Error”.
- [ ] Los nombres de modelo/proveedor son los reales, no placeholders.
- [ ] No hay botones de demo en producción.

## Ventana y teclado

- [ ] La ventana recuerda tamaño/posición/estado.
- [ ] Cerrar con cambios sin guardar pregunta (Guardar / Descartar / Cancelar).
- [ ] Todo comando importante está en menú o palette, además del atajo.
- [ ] El título refleja proyecto y estado sucio.

## Temas y accesibilidad

- [ ] Se revisó en dark y light (y sistema si existe).
- [ ] El acento no rompe contraste ni estados semánticos.
- [ ] Nada depende solo del color.
- [ ] Tooltips y `accessibility_label` en icon-only.
- [ ] Orden de tabulación razonable; no hay trampas de foco.

## Evidencia

- [ ] Test de interacción si el comportamiento es verificable.
- [ ] Captura DPI-aware de la pantalla final (dark y light si aplica).
- [ ] Nota del OKF actualizada + entrada en `changes/CHANGELOG.md`.
- [ ] Gates verdes (`fmt`, `clippy`, `test`).
