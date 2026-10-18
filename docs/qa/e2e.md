# E2E — Guion de aceptación manual (12 pasos)

> Ejecuta este guion un agente QA distinto al que implementó. Evidencia por paso:
> captura o vídeo + `logs/app.log` relevante. Resultado en `docs/qa/reports/G<N>-<fecha>.md`.
> Estado P0: **borrador** (los pasos 8–12 se refinan cuando existan los paneles).

Precondición: app construida en modo release/debug, raíz portable con `tex/`, `agent/`, `workspace/cv/plantilla-clasica` compilado.

| # | Acción | Resultado esperado |
|---|--------|--------------------|
| 1 | Abrir la app (último proyecto = `workspace/cv/plantilla-clasica`) | Se restauran layout, pestañas del editor y PDF de la sesión anterior |
| 2 | Editar `main.tex` (añadir una línea de texto), `Ctrl+S` | Status bar muestra compilando → OK; el PDF se actualiza sin perder la posición de scroll |
| 3 | Añadir `\undefinedcmd{}` en el cuerpo, guardar | El error aparece en el panel de problemas y marcado en el editor (línea correcta); deshacer y guardar → compila verde |
| 4 | Quitar `\usepackage{lastpage}` y compilar con un paquete *no instalado* equivalente | Auto-install vía tlmgr (banner de progreso) → recompila → verde (con `latex.autoInstallPackages: true`) |
| 5 | Poner el cursor en una línea conocida del cuerpo y `Ctrl+Alt+J` | El PDF hace scroll a la página correcta y resalta la zona correspondiente (~±20 pt) |
| 5b | Mover el cursor por el cuerpo (flechas) con `latex.syncScroll` activo (interruptor ⇄ de la toolbar) | El PDF **se desplaza solo** siguiendo la línea, sin resaltado; `Ln/Col` de la status bar se actualiza |
| 5c | Desplazar el PDF con la rueda (varias páginas) | El editor se desplaza a la línea origen **sin mover el cursor** y se puede seguir escribiendo; la `Pág. n/N` se actualiza |
| 5d | Apagar el interruptor ⇄ y repetir 5b/5c | Ninguna de las dos direcciones se mueve; el ajuste persiste tras reiniciar |
| 6 | `Ctrl+Click` sobre esa zona del PDF | El editor vuelve a la línea/columna correcta (marcador temporal visible) |
| 7 | Abrir panel IA (si no estaba), nueva sesión, pedir "resume el documento en 3 líneas" | Streaming markdown fluido en la burbuja; sin bloquear la UI |
| 7b | Escribir dos líneas en el chat y pulsar `Enter` / `Shift+Enter` | `Enter` envía; `Shift+Enter` inserta salto de línea sin enviar |
| 7c | Preguntar algo de LaTeX (p. ej. «convierte esto en una tabla booktabs») | El agente carga la skill `latex-tablas-figuras` (o la que toque) y propone `booktabs`/`tabularx` |
| 7d | Menú **Permisos** → «Aprobar ediciones del proyecto» y pedir un cambio | No aparece el modal; el archivo cambia igualmente (y el permiso queda persistido en `ai.permissionPreset`) |
| 7e | Pulsar ↩ en el mensaje anterior (revert) | Diálogo de confirmación → los archivos y el historial vuelven a ese punto; banner «Cambios revertidos» con «Deshacer» |
| 7f | Dock inferior: cambiar entre PROBLEMAS / SALIDA / SÍMBOLOS y colapsar con el chevron | Las tres pestañas funcionan; el editor no pierde altura útil; SÍMBOLOS inserta el comando en el cursor |
| 8 | Pedir "añade al final una sección 'Experiencia' con la fecha de hoy" | Aparece tarjeta de herramienta de edición y **modal de permiso** con diff; al aprobar, el archivo cambia y compila; el editor recarga el buffer limpio |
| 9 | Inyectar un error (paso 3) y pulsar "Arreglar errores con IA" | El agente diagnostica, propone el fix (permiso), se aplica, compila verde; reporte final legible |
| 10 | Matar `opencode.exe` desde el Administrador de tareas con el chat abierto | El indicador del panel pasa a reconectando → listo en < 15 s; el historial no se pierde |
| 11 | Cerrar la app con una compilación en curso | No quedan procesos huérfanos (`latexmk`, `opencode`, `tlmgr`) en el Administrador de tareas |
| 12 | Copiar la carpeta portable a otra ruta con espacios y acentos (`D:\prueba con espacios\`) y abrir | Arranca, encuentra `tex/` y `agent/`, compila y chatea sin reconfigurar |

Notas:
- Los pasos 3, 5, 6 requieren `-synctex=1` (default), el sidecar `main.synctex.gz` junto al PDF y el visor en modo "PDF en vivo". Sin sidecar, SyncTeX responde `NotAvailable` y el scroll sync no mueve nada.
- Los pasos 5b–5d requieren `latex.syncScroll: true` (default) y `synctex.exe` en la distro (`tex/bin/windows`).
- El paso 4 usa un paquete real no instalado; documentar en el reporte cuál se usó (`tlmgr info --only-installed`).
- El paso 11 es bloqueante para el gate G1 (job objects).
