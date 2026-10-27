# Capturas del README

Imágenes usadas por `README.md`. Se generan desde fixtures del repo y **no contienen datos
personales**: el CV de ejemplo usa nombre, correo y teléfono ficticios.

| Archivo | Escena |
| --- | --- |
| `hero-dark.png` | Cuatro paneles con `fixtures/projects/cv-sidebar` copiado a `workspace/demo-cv` (PDF precompilado desde `fixtures/pdf/cv-sidebar.pdf`). Tema oscuro y un turno real del chat: se le pide a opencode un resumen de `main.tex` y lee el archivo con sus herramientas. |
| `problems.png` | `fixtures/projects/broken-undefined` compilado con **Compilar**; el `LogParser` reporta la secuencia indefinida en el panel de problemas y en la barra de estado. |

## Regenerarlas

1. Copia el proyecto del fixture a `workspace/` y, si aplica, su PDF como `main.pdf`.
2. Arranca la app con ese proyecto como último abierto (`project.last` en
   `config/settings.json`) y `advanced.texBinOverride` apuntando a tu TeX Live.
3. Captura la ventana con DPI real:

   ```powershell
   pwsh scripts/capture-window.ps1 -ProcessName latex-studio -Out shots/readme-hero.png
   ```

4. Recorta unos 14 px del borde (la captura incluye el borde redondeado de Windows) y
   escala a 1920 px de ancho. Las originales se toman a 200 % de DPI (3226×2013).

Las capturas finales pesan ~1 MB entre las dos; `shots/` sigue fuera de git.
