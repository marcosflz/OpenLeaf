; =============================================================================
; highlights.scm — resaltado LaTeX del editor de LaTeX Studio (spike M0-S2)
; Gramática: latex-lsp/tree-sitter-latex (release 0.6.1 en crates.io como
; `codebook-tree-sitter-latex`; ver crates/editor/Cargo.toml y su nota).
;
; ORIGEN / NOTA (verificado 2026-10-17):
;   - La descarga prevista de
;     https://raw.githubusercontent.com/latex-lsp/tree-sitter-latex/master/queries/highlights.scm
;     devuelve 404: el repositorio NO publica queries/ (tampoco los tarballs de
;     crates.io, ni `tree-sitter-latex` 0.1.0 ni `codebook-tree-sitter-latex`
;     0.6.1 incluyen la carpeta). → Query propia escrita en este repo.
;   - Validada (Query::new + nodos y campos) contra el `node-types.json` de la
;     revisión 0.6.1 que usa el crate; la revisión 0.1.0 coincide en todos los
;     nombres de nodo salvo los grupos de etiquetas (`curly_group_label*`).
;   - Base: la query de nvim-treesitter (queries/latex/highlights.scm), mantenida
;     contra esta misma gramática, recortada a:
;       * solo capturas conocidas por gpui-component::highlighter::HIGHLIGHT_NAMES
;         (41 nombres). El kit ignora el resto y hace fallback al prefijo antes
;         del punto (`function.macro` → `function`).
;       * sin predicados específicos de nvim (#lua-match?, #any-of?, @spell,
;         @nospell, @module) que el highlighter del kit no evalúa.
;   - TODO(M1-A3): `label`, `operator`, `punctuation.*` y `variable` no tienen
;     color en el tema dark por defecto del kit; ajustar tema o remapear.
; =============================================================================

; --- Comandos -----------------------------------------------------------------
(command_name) @function

; --- Comentarios --------------------------------------------------------------
[
  (line_comment)
  (block_comment)
  (comment_environment)
] @comment

; --- Sectioning y títulos -----------------------------------------------------
(section text: (curly_group (_) @title))
(subsection text: (curly_group (_) @title))
(subsubsection text: (curly_group (_) @title))
(chapter text: (curly_group (_) @title))
(part text: (curly_group (_) @title))
(paragraph text: (curly_group (_) @title))
(subparagraph text: (curly_group (_) @title))
(title_declaration text: (curly_group (_) @title))

; --- Entornos ------------------------------------------------------------------
(begin name: (curly_group_text (text) @type))
(end name: (curly_group_text (text) @type))

; --- Definiciones --------------------------------------------------------------
(new_command_definition command: _ @keyword)
(old_command_definition command: _ @keyword)
(let_command_definition command: _ @keyword)
(environment_definition command: _ @keyword)
(theorem_definition command: _ @keyword)
(paired_delimiter_definition command: _ @keyword)
(color_definition command: _ @keyword)
(glossary_entry_definition command: _ @keyword)
(acronym_definition command: _ @keyword)

; --- Definiciones y referencias de etiquetas -----------------------------------
(label_definition name: (curly_group_label (_) @label))
(label_reference names: (curly_group_label_list (_) @link_uri))
(label_reference_range
  from: (curly_group_label (_) @link_uri)
  to: (curly_group_label (_) @link_uri))
(label_number
  name: (curly_group_label (_) @label)
  number: (_) @link_uri)

; --- Citas bibliográficas ------------------------------------------------------
(citation keys: (curly_group_text_list) @string)

; --- Inclusión de ficheros y paquetes ------------------------------------------
(class_include path: (curly_group_path) @string)
(package_include paths: (curly_group_path_list) @string)
(latex_include path: (curly_group_path) @string)
(verbatim_include path: (curly_group_path) @string)
(import_include
  directory: (curly_group_path) @string
  file: (curly_group_path) @string)
(graphics_include path: (curly_group_path) @string)
(svg_include path: (curly_group_path) @string)
(inkscape_include path: (curly_group_path) @string)
(bibstyle_include path: (curly_group_path) @string)
(bibtex_include paths: (curly_group_path_list) @string)
(biblatex_include glob: (curly_group_glob_pattern) @string.regex)
(tikz_library_import paths: (curly_group_path_list) @string)

; --- Matemáticas ----------------------------------------------------------------
[
  (displayed_equation)
  (inline_formula)
  (math_environment)
] @embedded
(math_delimiter) @keyword

; --- Elementos de lista ----------------------------------------------------------
[
  "\\item"
  "\\item*"
] @keyword

; --- Pares clave/valor (opciones) ------------------------------------------------
(key_value_pair key: (_) @property)

; --- Verbatim ---------------------------------------------------------------------
(verbatim_environment) @string

; --- Parámetros de macros (#1, #2, …) ---------------------------------------------
(placeholder) @variable
