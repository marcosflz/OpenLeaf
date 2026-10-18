---
name: latex-beamer
description: Presentaciones Beamer en LaTeX. USE FOR: diapositivas, presentación, slides, beamer, transparencias, \frame, bloques, columnas, overprint, notas del orador, tema de presentación, "se sale de la diapositiva", añadir/quitar diapositivas.
---

# Beamer

## Esqueleto

```latex
\documentclass[aspectratio=169,11pt]{beamer}
\usetheme{metropolis}                 % moderno; alternativa: Madrid, default
\usepackage[utf8]{inputenc}
\usepackage{amsmath,booktabs}

\title{Título}
\subtitle{Subtítulo}
\author{Nombre}
\date{\today}

\begin{document}
\frame{\titlepage}
\section{Introducción}
\begin{frame}{Primera diapositiva}
  \begin{itemize}
    \item Punto uno.
    \item Punto dos.
  \end{itemize}
\end{frame}
\end{document}
```

## Reglas de contenido

- **Una idea por diapositiva**; el título dice la idea, no el tema.
- Poco texto: máximo 6 líneas y ≤ 10 palabras por línea. Si no cabe, parte la
  diapositiva en dos.
- Negritas con `\textbf`/`\alert{...}` (lo importante), nunca todo el texto.
- `\pause` y `\onslide<2->` para revelar por pasos; `\begin{overprint}` si el
  contenido cambia entre pasos sin mover el resto.

## Patrones

```latex
% Dos columnas
\begin{columns}[T]
  \begin{column}{.5\textwidth} Texto \end{column}
  \begin{column}{.5\textwidth} \includegraphics[width=\linewidth]{fig.pdf} \end{column}
\end{columns}

% Bloque destacado
\begin{block}{Resultado} ... \end{block}
\begin{alertblock}{Atención} ... \end{alertblock}

% Notas del orador (solo visibles con \setbeameroption{show notes})
\note{No leer la diapositiva; contar el caso real.}

% Código
\begin{lstlisting}[language=Python] ... \end{lstlisting}   % paquete listings
```

## Errores típicos

- `\section` **fuera** del entorno `document`.
- Olvidar `\frame{\titlepage}`: el título no aparece.
- Tablas o imágenes más altas que la caja: usa `[shrink]`, `\small`, o parte la
  figura; `\begin{frame}[t]` si va arriba.
- Bibliografía: en beamer va con `\footnote`/`\cite` y
  `\begin{frame}[allowframebreaks]{Referencias}\bibliography{refs}\end{frame}`.
