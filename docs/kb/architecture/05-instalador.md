---
id: kb/architecture/instalador
title: El instalador ligero prepara TinyTeX-1 o reutiliza el TeX existente
status: draft
updated: 2026-10-24
source:
  - installer/latex-studio.iss
  - installer/build.ps1
  - docs/release.md
  - docs/qa/reports/P2-INST-1-2026-10-24.md
tags: [arquitectura, instalador, distribucion, latex]
related: [kb/operations/release, kb/operations/latex-bootstrap, kb/product/primer-arranque]
verified: true
---

# El instalador ligero prepara TinyTeX-1 o reutiliza el TeX existente

## Qué

`installer/latex-studio.iss` (Inno Setup 6) empaqueta lo que monta `cargo xtask dist`:

| Perfil | Contenido | Motor LaTeX |
|---|---|---|
| Ligero (por defecto) | `latex-studio.exe`, `agent/`, `assets/`, `VERSION`, checksums | página **Motor LaTeX**: usa el `tex\` existente, descarga TinyTeX-1 (~74 MB) o se omite |
| `/DBundledTex` | además `dist/latex-ide/tex/` (TeX Live reducido) | incluido; **no descarga nada** (offline) |

Instala **por usuario** en `%LOCALAPPDATA%\LaTeX Studio` (`PrivilegesRequired=lowest`), con
accesos directos opcionales y desinstalador. No borra `workspace/` ni `settings.json`;
`logs/` y `config/opencode.runtime.json` sí.

## Flujo del perfil ligero (P2-INST-1)

1. La página **Motor LaTeX** aparece tras elegir carpeta y ofrece:
   - **Usar el motor existente** si `{app}\tex\bin\windows\latexmk.exe` ya existe. No se
     modifica ni se borra; solo se verifican `latexmk`, `synctex` y `biber`.
   - **Descargar TinyTeX-1** (recomendado) cuando no hay `{app}\tex`.
   - **No instalar ahora**: la app se instala con un aviso accionable.
2. Descarga fijada y verificada con `CreateDownloadPage`:
   - URL `https://github.com/rstudio/tinytex-releases/releases/download/v2026.10/TinyTeX-1-windows-v2026.10.exe`
   - SHA-256 `36c329c644683d981a668419d13c8804696e2816ae4d287ad755788fee231a31`
   - tamaño `73.701.889` bytes.
3. Extracción con el SFX 7z (`-y -o"{app}"`) → `{app}\TinyTeX`, luego
   `RenameFile` a `{app}\tex`. No se toca un `tex` que ya exista: si existe sin `latexmk`,
   se desactiva la descarga y se avisa.
4. `tlmgr update --self` y después:
   - `tlmgr install synctex biber`
   - `tlmgr install beamer pgf grfext titlesec enumitem preprint marvosym roboto fancyhdr tools babel-spanish babel-english`

   Cada paso se ejecuta con `cmd.exe /d /c .\tlmgr.bat …`, timeout y salida a
   `{app}\logs\tex-bootstrap.log`. El timeout se implementa con un runner PowerShell
   temporal (`{tmp}\lsx-run.ps1`): `WaitForExit(ms)` + `taskkill /T /F`; devuelve `124`
   al vencer.
   **Verificado 2026-10-24:** `article`, `report` y `beamer` compilan a PDF con SyncTeX en
   una instalación limpia; `beamer` necesitó `grfext` (dependencia de `epstopdf-base`), ya
   incluido en la lista. Tras un build fallido, `latexmk` no reintenta sin `-g`/limpiar
   auxiliares.

5. Verificación final de `latexmk.exe`, `synctex.exe` y `biber.exe`. Se escribe
   `{app}\tex\.lsx-bootstrap.json` con `schema`, `mode`, `source`, `version`, `sha256`,
   `installedAt`, `status` (`ok`/`partial`) y el estado de cada binario.
6. Fallo de red, URL, hash, extracción o `tlmgr`: **no aborta** la instalación. Se deja el
   aviso, el log y se limpian restos parciales cuando es seguro (`{app}\TinyTeX`; un `tex`
   creado por el instalador solo se borra si no llegó `latexmk`). Un `tex` del usuario no
   se borra nunca.

## Modos silencioso y offline

- `/VERYSILENT` o `/SILENT`: no se muestra la página. Si no hay `tex`, se descarga
  TinyTeX con `DownloadTemporaryFile` (sin UI); si ya hay `tex`, se usa sin modificarlo.
  Un `tex` incompleto o una ruta no ASCII se degradan a aviso en el log, sin abortar.
- `/DBundledTex`: el `[Code]` no crea la página ni descarga; solo verifica los binarios
  del `tex` incluido.
- La instalación silenciosa limpia con red deja los tres binarios y termina con exit 0;
  la instalación con URL caída o hash incorrecto también termina con exit 0 y sin `tex`.

## Hechos verificados (2026-10-24)

- El SFX es 7z Console, no Inno: `-y -o<dir>` extrae una carpeta `TinyTeX/`.
- TinyTeX-1 incluye `latexmk.exe` y `tlmgr.bat`; **no** incluye `synctex.exe` ni
  `biber.exe`.
- `tlmgr install synctex biber` requiere `tlmgr update --self` antes (el repo
  `tlnet.yihui.org` va por delante del tlmgr empaquetado).
- Todos los paquetes de la lista existen; `marvosym` es correcto y se instala. `tools`
  suele venir ya presente (`package already present`), sin error.
- Instalación limpia silenciosa (Windows 10/11, red): ~0,9 min; descarga 73.701.889
  bytes con SHA-256 correcto; `latexmk 4.88`, `synctex 1.5`, `biber 2.22`; marcador `ok`.
- Reinstalar sobre una instalación existente reutiliza `tex` (0,5 s, sin descarga) y no
  cambia su `mtime`.
- `/DBundledTex` copia `tex` y no descarga; su desinstalador borra `tex`.
- URL caída (`http://127.0.0.1:9`): exit 0, app instalada, sin `tex`, aviso claro y log.
- Hash incorrecto (servidor local + SHA-256 falso): exit 0, app instalada, sin `tex`,
  log «El hash del archivo es incorrecto».
- Ruta con espacios ASCII: descarga, extracción y `tlmgr` funcionan.
- Ruta con `ñ`: `tlmgr` funciona, pero `latexmk.exe` falla con `runscript.tlu`/`kpathsea`;
  por eso el instalador detecta la ruta no ASCII, no descarga y avisa. Es un límite de
  TinyTeX, no del instalador.
- Un `tex` existente con solo `latexmk` se conserva y se avisa de que faltan `synctex` y
  `biber`; un `tex` existente sin `latexmk` se conserva y se avisa de que no se modifica.

## Riesgos

- El runner usa `powershell.exe -ExecutionPolicy Bypass`; una política de grupo que lo
  bloquee degrada el bootstrap a aviso (la app se instala igual).
- El antivirus puede tardar en el SFX; el timeout de extracción es 900 s y se ejecuta
  oculto.
- `tlmgr` depende de `tlnet.yihui.org`; si el mirror falla, quedan los binarios que ya
  estuvieran y un aviso. La app puede auto-instalar paquetes al compilar.
- Cancelar/cerrar el instalador durante `tlmgr` puede dejar un `tex` parcial; una
  reinstalación lo detecta y no lo borra. El auto-install de la app es la red de seguridad.
- El `.exe` no está firmado; SmartScreen puede avisar.
- El perfil ligero **conserva** `tex` al desinstalar (puede tener paquetes del usuario);
  solo `/DBundledTex` lo borra.
- La página y la barra de progreso no se validaron visualmente en modo interactivo; sí la
  lógica y el modo silencioso. Ver el informe de QA.
- El reintento guiado desde la app (Ajustes → Compilación → Motor LaTeX) es P2-INST-2; en
  0.2.0 el reintento es volver a ejecutar el Setup.
