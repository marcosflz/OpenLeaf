# AGENTS.md — Proyecto LaTeX

Este directorio es un **proyecto de escritura LaTeX** gestionado por LaTeX Studio
(IDE nativo con editor, visor PDF y asistente IA). La «codebase» son documentos
`.tex`: aquí no se programa una app, se **escribe, compila y repara LaTeX**.

## Entorno
- Distribución TeX portátil del producto en `..\tex\bin\windows` (`pdflatex`, `lualatex`, `xelatex`, `latexmk`, `biber`, `tlmgr`).
- El agente vive en `..\agent\opencode.exe` y usa `..\agent\opencode.json`.
- Los errores de compilación aparecen en el panel **Problemas** de LaTeX Studio.

## Comandos
- Compilar: `latexmk -synctex=1 -interaction=nonstopmode -file-line-error -pdf main.tex`
- Limpiar: `latexmk -c` (o `-C` para borrar también el PDF)
- Buscar paquete: `kpsewhich <paquete>.sty`
- Instalar paquete que falta: `tlmgr install <paquete>`
- Actualizar la distro: `tlmgr update --self --all`

## Flujo obligatorio para arreglar errores de compilación
Cuando la compilación falle, sigue SIEMPRE este orden:

1. **Compila y captura el log**:
   `latexmk -synctex=1 -interaction=nonstopmode -file-line-error -pdf <archivo>.tex`
2. **Diagnostica**:
   - `File '<x>.sty' not found` o `You have requested package '<x>'` → **falta un paquete**.
   - `Undefined control sequence` / `Missing $` / `Runaway argument` → **error de código** en el `.tex`.
   - Referencias/citas rotas (`??`, `Citation undefined`) → recompila varias veces o ejecuta `biber`/`bibtex`.
3. **Repara**:
   - Paquete que falta → `tlmgr install <paquete>` y **vuelve a compilar**. No edites la
     distribución a mano: usa siempre `tlmgr`.
   - Error de código → edita el `.tex` (cambio mínimo), recompila y verifica.
4. **Verifica**: repite hasta que la compilación termine sin errores y exista el PDF.
5. **Informa**: di qué paquete instalaste o qué editaste, y el estado final.

## Reglas
- **Cambios mínimos**: edita solo lo necesario para compilar; no reformatees el documento.
- **No toques `..\tex\`** salvo con `tlmgr` / `kpsewhich`.
- **No inventes paquetes**: si `tlmgr` no encuentra uno, busca el `.sty` con `kpsewhich` y
  comprueba el nombre real; si no existe en CTAN, propón una alternativa y pide confirmación.
- Conserva el idioma y el estilo del documento (por defecto: español).
- Antes de borrar archivos o ejecutar `latexmk -C`, confirma con el usuario.

## Estructura
- `*.tex` — documento principal (`main.tex`) y secciones.
- `*.bib` — bibliografía.
- `figures/` (o similar) — imágenes.
- `.opencode/agents/latex-fix.md` — agente especializado en reparar compilaciones.
