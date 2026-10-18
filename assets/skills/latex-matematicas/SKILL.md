---
name: latex-matematicas
description: Escribir y revisar matemáticas en LaTeX con amsmath. USE FOR: fórmulas, ecuaciones, alineación, matrices, casos, integrales, sumatorios, símbolos griegos, fracciones, numeración de ecuaciones, referencias a ecuaciones, teoremas, demostraciones, matemáticas en línea vs display.
---

# Matemáticas con `amsmath`

## Preámbulo mínimo

```latex
\usepackage{amsmath,amssymb,mathtools}   % mathtools extiende amsmath
```

## Reglas de estilo

- **En línea** (`$...$` o `\(...\)`): varias letras con un significado →
  `$v_{\mathrm{max}}$`. Una variable suelta → `$x$`.
- **Display** (`equation`, `align`, `gather`): solo para fórmulas que merecen su
  línea; nunca `$$...$$` (desaconsejado en LaTeX moderno).
- **Números** con unidades → `\SI{9.81}{\metre\per\second\squared}` (paquete
  `siunitx`) en vez de `9.81 m/s²`.
- **Puntuación**: las display forman parte de la frase — si la fórmula termina la
  oración, pon la coma/punto **dentro** de la fórmula.
- **Referencias**: `\eqref{eq:energia}` (añade paréntesis) con
  `\label{eq:energia}` inmediatamente después de la ecuación.

## Patrones que se usan de verdad

```latex
\begin{align}
  E &= mc^2 \label{eq:masa-energia} \\
  F &= ma \label{eq:newton}
\end{align}

\begin{equation}
  f(x) = \begin{cases}
    1 & x > 0 \\
    0 & x \le 0
  \end{cases}
\end{equation}

% Matriz y vector
\begin{pmatrix} a & b \\ c & d \end{pmatrix}
\vec{v} = \begin{pmatrix} v_x \\ v_y \end{pmatrix}

% Sumatorio con límites y fracción
\sum_{i=1}^{n} \frac{1}{i^2} = \frac{\pi^2}{6}
```

## Símbolos (evita inventarlos)

| Necesitas | Comando | Necesitas | Comando |
|---|---|---|---|
| ≤ ≥ ≠ ≈ | `\le \ge \ne \approx` | ∀ ∃ | `\forall \exists` |
| ∈ ∉ ⊂ ⊆ | `\in \notin \subset \subseteq` | ∞ ∂ ∇ | `\infty \partial \nabla` |
| → ⇒ ↔ | `\to \Rightarrow \leftrightarrow` | × · ± | `\times \cdot \pm` |
| ∫ ∮ ∑ ∏ | `\int \oint \sum \prod` | ℝ ℕ ℤ | `\mathbb{R} \mathbb{N} \mathbb{Z}` |

Si un símbolo no está en `amssymb`, cárgalo con `\usepackage{...}` en vez de
inventar el comando.

## Errores típicos

- `\left(` sin `\right)`: agrupa siempre la pareja.
- Alinear con `&` sin `align` (o `&` suelto en `equation`).
- `\\` al final de la última línea de `align` (sobra).
- Usar `\text` para variables (mal: itálicas perdidas) → `\mathrm` o `\mathit`.
