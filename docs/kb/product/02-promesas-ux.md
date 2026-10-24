---
id: kb/product/promesas-ux
title: Promesas de experiencia que la UI debe cumplir
status: stable
updated: 2026-10-24
source:
  - docs/plan/PLAN-MAESTRO.md
  - .opencode/skills/gpui-kit-design-guides/references/design-guides.md
  - docs/kb/quality/01-auditoria-2026-10-24.md
tags: [producto, ux, diseno]
related: [kb/architecture/ui-shell, kb/quality/checklist-ui]
verified: true
---

# Promesas de experiencia que la UI debe cumplir

## Qué

La app debe sentirse como una herramienta de escritorio terminada, no como una demo. Estas
promesas son el criterio para aceptar o rechazar una pantalla.

1. **Compilar desde el primer minuto.** Al abrir por primera vez, el usuario puede crear un
   proyecto y compilar sin instalar nada a mano ni editar JSON.
2. **El estado es visible.** Motor, compilación, IA, permisos, guardado y errores tienen un
   estado textual + visual; nunca solo un color.
3. **La IA se configura desde la app.** Ver qué proveedor/modelo está conectado, conectar la
   suscripción o API key, elegir modelo/agente y probar que responde, sin abrir una terminal
   ni editar `settings.json`.
4. **La ventana se comporta como una ventana.** Recuerda tamaño, posición y maximizado;
   tiene menús y atajos; cerrar con cambios sin guardar pregunta; el título refleja el
   proyecto y el estado sucio.
5. **El tema es del usuario.** Claro/oscuro/sistema, acento elegible, contraste correcto en
   ambos modos, sin colores crudos en los paneles.
6. **Los ajustes son un producto, no un formulario.** Secciones, valores por defecto,
   validación, “restablecer”, ruta de configuración y explicación de cada opción.
7. **Nada de andamios en producción.** Los botones de demo/spike no aparecen en la toolbar.
8. **Keyboard first de escritorio.** Todo comando importante tiene atajo o menú; Escape
   cierra el overlay superior; el foco vuelve al disparador.
9. **Los errores dicen qué pasó y qué hacer.** Sin “Error” a secas; con objeto, causa y
   siguiente acción (instalar, reintentar, abrir carpeta, copiar diagnóstico).
10. **La evidencia es visual y reproducible.** Una tarea de UI no se cierra sin captura o
    test de interacción.

## Cómo se usa

- `docs/kb/quality/03-checklist-ui.md` baja estas promesas a una revisión por pantalla.
- El plan 0.2 (`docs/kb/changes/PLAN-0.2.md`) asigna tareas a cada promesa incumplida.
- Si una decisión de diseño contradice una promesa, se documenta en un ADR y se actualiza
  esta nota; no se ignora en silencio.
