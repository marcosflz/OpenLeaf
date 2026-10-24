---
id: kb/product/primer-arranque
title: El primer arranque debe terminar con un proyecto compilado
status: draft
updated: 2026-10-24
source:
  - installer/latex-studio.iss
  - crates/core/src/root.rs
  - crates/ui/src/app.rs
  - docs/qa/reports/M3-instalador-local-2026-10-18.md
tags: [producto, onboarding, instalador]
related: [kb/architecture/instalador, kb/operations/latex-bootstrap]
verified: false
---

# El primer arranque debe terminar con un proyecto compilado

## Qué

El camino mínimo de un usuario nuevo debe ser:

1. Ejecuta `LaTeX-Studio-Setup.exe`.
2. El instalador deja la app, el agente y **un motor LaTeX utilizable**.
3. Abre la app; si no hay proyecto, ve una bienvenida con “Nuevo proyecto” y “Abrir”.
4. Crea un proyecto desde plantilla.
5. Pulsa **Compilar**; el PDF aparece en el visor.

Hoy el paso 2 **no se cumple** en el instalador ligero: copia la app y avisa de que falta
`tex\`; el usuario tiene que buscar/copiar un TeX Live a mano. Es el problema P2-INST-1.

## Estado actual (2026-10-24)

- `installer/latex-studio.iss` solo incluye `tex\` con `/DBundledTex`.
- Con el perfil ligero, al terminar comprueba `tex\bin\windows\latexmk.exe` y muestra un
  `MsgBox` informativo; no instala ni descarga nada.
- `PortableRoot::ensure_layout()` crea `workspace/` y `config/`, pero no `tex/`.
- La app detecta el motor con `latexmk.is_file()` y pinta un triángulo en la toolbar/status
  bar; no ofrece instalar.
- El instalador local `v0.1.0` se probó con un *junction* a un `tex\` existente, no en una
  máquina limpia.

## Criterio de aceptación (P2-INST-1 / P2-UI-3)

- Instalación limpia + sin TeX previo + con red → el usuario puede compilar una plantilla
  sin tocar una terminal.
- Instalación sin red → la app explica el problema y ofrece reintentar; no se queda en un
  aviso ambiguo.
- La descarga se verifica (SHA-256) y se puede cancelar sin corromper la instalación.
- `latexmk`, `synctex` y `biber` existen al terminar (o se instalan en el primer arranque).
- La app muestra progreso y el resultado; el usuario puede abrir la carpeta `tex\`.

## Notas

- `tlmgr` de TinyTeX puede pedir `update --self` antes de instalar paquetes; ver
  `operations/04-latex-bootstrap.md`.
- El instalador completo (`/DBundledTex`) sigue siendo válido para offline.
