---
name: latex-compilar-depurar
description: Compilar un proyecto LaTeX con latexmk y resolver errores del log. USE FOR: no compila, error de compilación, "Undefined control sequence", paquete no instalado, "Missing $", referencias sin resolver, recuadros rojos, "Overfull hbox", warnings de fonts, biber/bibtex, cuántas pasadas hacen falta, dónde está el log, qué línea falló.
---

# Compilar y depurar LaTeX

## Ciclo de compilación

1. Compila desde la **raíz del proyecto**, no desde subcarpetas:
   `latexmk -pdf -interaction=nonstopmode -synctex=1 -file-line-error main.tex`
   (`main.tex` = fichero principal; si el proyecto lo declara en `latex.latexmk.args`, respétalo).
2. Lee **`main.log`**, no la salida de consola: el log tiene las líneas reales y el
   contexto. `-file-line-error` hace que los errores salgan como `fichero.tex:LINEA: mensaje`.
3. El **primer** error es el que importa: los siguientes suelen ser consecuencia.
4. Tras arreglar, vuelve a compilar. **No hace falta limpiar** salvo que el estado
   quede corrupto (`latexmk -C` y recompilar).

## Errores más frecuentes y su causa real

| Mensaje | Causa habitual | Arreglo |
|---|---|---|
| `Undefined control sequence` | comando inexistente o paquete sin cargar | cargar el paquete (`\usepackage{...}`) o corregir el nombre; el log da la línea |
| `Missing $ inserted` | `_` `^` `&` `%` `#` sueltos en texto | escapar (`\_`, `\&`, `\%`, `\#`) o meter en `$...$` |
| `File 'X.sty' not found` | paquete no instalado en TeX Live | `tlmgr install X` (el motor reducido ya lo intenta solo) |
| `Runaway argument?` | llave sin cerrar o argumento con `%` | revisar la línea anterior al error |
| `LaTeX Error: Something's wrong--perhaps a missing \item` | lista/entorno mal cerrado | cerrar `\begin{itemize}...\end{itemize}` |
| `! Package biblatex Error: ... rerun` | bibliografía desactualizada | `biber main` (biblatex) o `bibtex main` (bibtex) y recompilar |
| `Label(s) may have changed. Rerun` | referencias cruzadas | recompilar (latexmk ya repite solo) |
| `Overfull \hbox` | texto que no cabe | aviso, no error: `\sloppy`, `\raggedright` o reescribir |
| `Font shape ... undefined` | fuente no instalada/tema beamer | `\usepackage{lmodern}` + `\usefonttheme{professionalfonts}` |

## Reglas

- **Nunca** cambies `\documentclass`, el preámbulo ni las opciones globales para
  "arreglar" un error local: primero diagnostica (`grep -n` en el log).
- Si el log termina con `! Emergency stop` o `Fatal error occurred`, busca la
  primera línea con `!` y su `l.NN` (número de línea del fichero en curso).
- Los `Warning` no bloquean: no los toques salvo que el usuario lo pida.
- Cuando edites: **cambios mínimos** y recompila después; si empeora, revierte y
  prueba otra vía en vez de acumular parches.
- Comprueba el resultado: exit code de latexmk (0 = ok; 12 = error de TeX) y
  `main.pdf` actualizado (mtime).
