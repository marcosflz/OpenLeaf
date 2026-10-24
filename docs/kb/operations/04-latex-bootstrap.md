---
id: kb/operations/latex-bootstrap
title: Bootstrap de TeX Live con TinyTeX (verificado 2026-10-24)
status: draft
updated: 2026-10-24
source:
  - https://github.com/rstudio/tinytex-releases/releases/tag/v2026.10
  - pruebas locales en %TEMP%\lsx-tinytex
  - installer/latex-studio.iss
tags: [operaciones, latex, instalador]
related: [kb/architecture/instalador, kb/product/primer-arranque]
verified: true
---

# Bootstrap de TeX Live con TinyTeX (verificado 2026-10-24)

## Qué

TinyTeX es una distribución TeX Live reducida (~74 MB el instalador Windows de la variante
`TinyTeX-1`). Sirve para que el instalador ligero deje un motor utilizable sin bajar los
4,4 GB del `tex/` completo.

## Comandos verificados

```powershell
# 1. Descargar (fijar versión y hash en el instalador)
curl.exe -L --fail -o TinyTeX-1-windows-v2026.10.exe `
  https://github.com/rstudio/tinytex-releases/releases/download/v2026.10/TinyTeX-1-windows-v2026.10.exe

# 2. SHA-256 esperado
# 36c329c644683d981a668419d13c8804696e2816ae4d287ad755788fee231a31

# 3. Es un SFX 7z: extrae una carpeta TinyTeX/
.\TinyTeX-1-windows-v2026.10.exe -y -o"C:\ruta\destino"

# 4. Actualizar tlmgr y añadir lo que TinyTeX-1 no trae
C:\ruta\destino\TinyTeX\bin\windows\tlmgr.bat update --self
C:\ruta\destino\TinyTeX\bin\windows\tlmgr.bat install synctex biber

# 5. Comprobar
Test-Path C:\ruta\destino\TinyTeX\bin\windows\latexmk.exe
Test-Path C:\ruta\destino\TinyTeX\bin\windows\synctex.exe
Test-Path C:\ruta\destino\TinyTeX\bin\windows\biber.exe
```

## Paquetes que TinyTeX-1 no trae y las plantillas usan

`beamer`, `pgf`, `grfext` (lo pide `epstopdf-base` y sin él beamer falla), `titlesec`,
`enumitem`, `preprint` (provee `fullpage`), `marvosym`, `roboto`, `fancyhdr`, `tools`
(provee `tabularx`), `babel-spanish`, `babel-english`.
La app ya auto-instala paquetes faltantes al compilar, pero el instalador debe dejar una base
para que la primera compilación de una plantilla no dependa de una segunda descarga.

**Verificado 2026-10-24** con el instalador silencioso y TinyTeX real: las plantillas
`article` y `report` compilan a PDF con SyncTeX; `beamer` falló por `grfext.sty`, se añadió
`grfext` a la lista y volvió a compilar (tras limpiar auxiliares; `latexmk` no reintenta un
build fallido sin `-g`).

## Estrategia recomendada para P2-INST-1

1. Página “Motor LaTeX” en Inno:
   - “Usar el TeX existente” si ya hay `{app}\tex\bin\windows\latexmk.exe`.
   - “Descargar TinyTeX-1 (recomendado)” con `CreateDownloadPage` y verificación SHA-256.
   - “No instalar ahora” con aviso y reintento desde la app.
2. Extraer con `-y -o{app}` y renombrar `{app}\TinyTeX` → `{app}\tex` (o extraer el tar.gz
   directamente en `{app}\tex`).
3. `tlmgr update --self` + `tlmgr install …` con `CreateOutputProgressPage`; log en
   `{app}\logs\tex-bootstrap.log`.
4. Verificar binarios y escribir un marcador `tex\.lsx-bootstrap.json` con versión/fecha/hash.
5. Si falla: no abortar la instalación; dejar `tex\` limpio (o borrar el parcial) y mostrar
   “Reintentar desde Ajustes → Compilación → Motor LaTeX”.

## Alternativa offline

- Instalador `/DBundledTex` con el `tex/` completo (4,4 GB) sigue siendo la vía sin red.
- Se puede publicar un `tex-bundle.zip` como asset aparte y que el instalador lo descargue;
  no se hace en 0.2 para no multiplicar assets.

## Riesgos

- `tlmgr` puede cambiar de repo/mirror; usar timeout y reintento.
- El SFX puede tardar en antivirus; ejecutar oculto y esperar con timeout.
- No usar `scheme-full` (2 GB+) en el instalador ligero.
