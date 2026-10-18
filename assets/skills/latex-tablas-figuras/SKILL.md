---
name: latex-tablas-figuras
description: Tablas, figuras, captions y flotantes en LaTeX con booktabs y gráficos. USE FOR: crear o arreglar tablas, tabularx, multicolumn, rotar tablas, insertar imágenes, includegraphics, figure, subfiguras, caption, referencias a figuras/tablas, ajustar el tamaño de una imagen, "tabla que se sale de la página".
---

# Tablas y figuras

## Preámbulo típico

```latex
\usepackage{booktabs}    % \toprule \midrule \bottomrule (nunca \hline)
\usepackage{tabularx}    % anchos que ajustan al texto
\usepackage{graphicx}    % \includegraphics
\usepackage[font=small]{caption}
```

## Tablas

```latex
\begin{table}[t]
  \centering
  \caption{Resultados por configuración.}\label{tab:resultados}
  \begin{tabularx}{\linewidth}{lX r}
    \toprule
    Configuración & Descripción & Tiempo (s) \\
    \midrule
    A & Línea base sin caché & 12.4 \\
    B & Con caché caliente   & 3.1 \\
    \bottomrule
  \end{tabularx}
\end{table}
```

- `\caption` **encima** en tablas, **debajo** en figuras (convención).
- `\label` justo después de `\caption` (si no, la referencia apunta mal).
- Tabla ancha: `tabularx` con columna `X`, o `\small`/`\footnotesize`,
  o `\resizebox{\linewidth}{!}{...}` como último recurso.
- Muchas filas → `longtable` (parte entre páginas) o `\begin{sidewaystable}`
  (`rotating`) si es apaisada.

## Figuras

```latex
\begin{figure}[t]
  \centering
  \includegraphics[width=.8\linewidth]{figuras/esquema.pdf}
  \caption{Esquema del montaje.}\label{fig:esquema}
\end{figure}
```

- **PDF/PNG/JPG** sí; EPS requiere `epstopdf`. Rutas sin acentos ni espacios.
- Escala con `width=`/`scale=`; nunca `\includegraphics` sin acotar tamaño.
- Subfiguras: paquete `subcaption` con `\begin{subfigure}[b]{.48\linewidth}`.
- Los flotantes se "mueven": si algo se va de sitio, revisa `[t]`/`[htbp]` y el
  tamaño real, no lo fuerces con `[H]` (paquete `float`) salvo petición expresa.

## Comprobación

Tras insertar una tabla/figura: recompila y mira que no haya
`Overfull \hbox` ni `Reference ... undefined`. Si la referencia sale `??`, falta
compilar una segunda pasada (latexmk la hace sola).
