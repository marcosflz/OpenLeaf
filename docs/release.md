# Publicar LaTeX Studio (M3 — distribución)

Resumen de las dos rutas: **CI** (recomendada para el instalador ligero) y **local**
(para el zip `full` con TeX Live, que no puede ir a CI porque esos activos no están en git).

## Atajos con `make` (Windows)

```powershell
make agent        # descarga agent/opencode.exe v1.18.30 si falta
make release      # build release + dist + Setup.exe + SHA-256
make publish      # crea la release v0.2.0 en GitHub (gh autenticado)
make gates        # fmt + clippy + tests + OKF
```

`make release` deja `dist\LaTeX-Studio-Setup.exe` y su `.sha256`. El perfil es ligero:
TinyTeX se descarga al instalar; no hace falta `tex\` en la máquina que compila.

## Qué entra y qué no

| Pieza | ¿En git? | ¿En el instalador ligero? | Notas |
|---|---|---|---|
| `latex-studio.exe` | no (se compila) | sí (~83 MB) | `cargo build --release -p latex-studio` |
| `agent\opencode.json` | **sí** | sí | config del producto (modelo, permisos, agente) |
| `agent\opencode.exe` | no (171 MB) | sí | CI lo descarga del release oficial en la versión fijada |
| `assets\skills\**` | **sí** | sí | skills LaTeX; opencode las ve vía `config/opencode.runtime.json` |
| `tex\` (motor LaTeX) | no (4,4 GB) | **sí** (TinyTeX, se descarga al instalar) | verificación SHA-256 y `tlmgr`; `/DBundledTex` lo trae offline |
| `workspace\` (proyectos) | no | **no** | puede tener documentos personales; `xtask dist` lo omite sin `--with-workspace` |
| `config\settings.json` | no (estado de usuario) | no | se genera en el primer arranque |

## Motor LaTeX del instalador ligero (P2-INST-1)

El perfil ligero ya no deja al usuario sin motor. Después de elegir la carpeta, la página
**Motor LaTeX** ofrece tres caminos:

1. **Usar el motor existente** si ya hay `{app}\tex\bin\windows\latexmk.exe`. El instalador
   no lo modifica ni lo borra; solo verifica `latexmk`, `synctex` y `biber`.
2. **Descargar TinyTeX-1** (recomendado, ~74 MB) cuando no existe `{app}\tex`. El Setup:
   - descarga `TinyTeX-1-windows-v2026.10.exe` con progreso y verifica el SHA-256
     `36c329c644683d981a668419d13c8804696e2816ae4d287ad755788fee231a31`
     (tamaño 73.701.889 bytes);
   - lo extrae con el SFX 7z (`-y -o"{app}"`), renombra `{app}\TinyTeX` → `{app}\tex`;
   - ejecuta `tlmgr update --self` y luego
     `tlmgr install synctex biber` +
     `tlmgr install beamer pgf grfext titlesec enumitem preprint marvosym roboto fancyhdr tools babel-spanish babel-english`;
   - cada paso tiene timeout y escribe salida en `{app}\logs\tex-bootstrap.log`.
3. **No instalar ahora**: la app se instala igualmente, con un aviso accionable. Se puede
   reintentar ejecutando de nuevo el instalador. El reintento guiado desde la app
   (Ajustes → Compilación) es P2-INST-2.

Al terminar se verifican los tres binarios y se escribe `{app}\tex\.lsx-bootstrap.json`
con versión, hash y estado (`ok` / `partial`). Un fallo de red, hash, extracción o `tlmgr`
**no aborta** la instalación: deja el aviso, el log y, si es seguro, limpia los restos
parciales (`{app}\TinyTeX` o un `tex` creado por el propio instalador sin `latexmk`).

En instalación silenciosa (`/VERYSILENT` o `/SILENT`) no hay página: si no hay `tex`, se
descarga TinyTeX; si ya hay `tex`, se usa sin tocarlo. `/DBundledTex` es la vía **offline**
y no descarga nada.

> **Límite conocido**: TinyTeX/`latexmk` falla si la ruta de instalación contiene caracteres
> no ASCII (probado con `ñ`). El instalador detecta esa ruta, no descarga y avisa; para usar
> TinyTeX hay que elegir una carpeta ASCII (p. ej. `C:\LaTeXStudio`) o usar `/DBundledTex`.

Este flujo entra con la ola 0.2. El asset `v0.1.0` ya publicado no incluye la página ni la
descarga; sigue siendo un instalador ligero sin motor.

## Ruta A — CI (instalador ligero)

```powershell
# Local: solo si quieres probarlo igual que lo hará el runner
cargo xtask dist --no-tex --no-zip --clean      # monta dist\latex-ide
pwsh installer\build.ps1 -SkipBuild             # necesita Inno Setup (ver abajo)
```

En GitHub: **Actions → installer → Run workflow** (o `git tag v0.1.0 && git push --tags`).
El workflow `.github/workflows/installer.yml`:

1. descarga `opencode-windows-x64.zip` de la versión de `OPENCODE_VERSION`
   (por defecto `v1.18.30`) y lo deja en `agent\opencode.exe`;
2. compila la app en release y monta `dist\latex-ide` con `cargo xtask dist`;
3. instala Inno Setup 6 con Chocolatey y compila `installer\latex-studio.iss`;
4. publica como artifact `latex-studio-installer`: el `Setup.exe`, su `.sha256`,
   el `SHA256SUMS.txt` y el `VERSION`.

El artifact es el **perfil ligero**: no contiene `tex\`. Si el usuario no tiene un motor,
el propio Setup descarga TinyTeX-1 al instalar (con red); `/DBundledTex` sigue siendo la
vía offline y se monta en local.

Inno Setup 6 ya está instalado en este equipo
(`%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe`); si faltara en otro,
`installer\build.ps1 -InstallInno` lo instala por usuario (winget o curl, sin UAC).

> **P0.4b resuelto (2026-10-18)**: el repo existe y está empujado —
> **https://github.com/marcosflz/OpenLeaf** (público, rama `main`). El tag `v*` dispara esta Action.

## Ruta C — Publicar una release con el instalador hecho en local (hecho en `v0.1.0`)

```powershell
cargo build --release -p latex-studio
cargo xtask dist --no-tex --no-zip --clean
pwsh installer\build.ps1 -SkipBuild
(Get-FileHash dist\LaTeX-Studio-Setup.exe -Algorithm SHA256).Hash.ToLower() |
  Set-Content dist\LaTeX-Studio-Setup.exe.sha256
```

Publicar la release con esos dos ficheros como assets (con `gh`):

```powershell
gh release create v0.1.0 dist\LaTeX-Studio-Setup.exe dist\LaTeX-Studio-Setup.exe.sha256 `
  --title "LaTeX Studio 0.1.0 - instalador (perfil ligero)" --notes-file docs\release-notes.md
```

Sin `gh` también sirve la API (`POST /repos/:owner/:repo/releases` y subida a
`uploads.github.com/repos/:owner/:repo/releases/:id/assets?name=…`) con un token de scope `repo`.

**Estado**: `v0.1.0` **publicada** el 2026-10-18 con `LaTeX-Studio-Setup.exe` (53,8 MB,
SHA-256 `fe068434…5288c`) y su `.sha256`. Verificado descargando el asset de la release y
comparando su hash con el local: **coincide**. El tag `v*` dispara además el workflow
`installer`, que construye su propio instalador y lo deja como *artifact del run* (no toca
los assets de la release).

## Continuar desde el otro PC (hecho: el repo ya está empujado)

```powershell
git clone https://github.com/marcosflz/OpenLeaf.git
```

Ojo: el clon no trae `tex\` ni `agent\opencode.exe` (ignorados por git) — para *ejecutar* la app
ahí hay que copiarlos; para lanzar la Action no hacen falta.

Antes de hacer el repo **público** (si algún día se decide), comprobar que sigue limpio:

```powershell
git grep -I -E '(sk-[A-Za-z0-9]{20,}|ghp_[A-Za-z0-9]{20,}|AIza[0-9A-Za-z_-]{20,}|BEGIN [A-Z ]*PRIVATE KEY)'
git ls-files config workspace tex agent     # solo debe salir agent/opencode.json y config/.gitkeep
```

Estado verificado el 2026-10-18 en este repositorio: 219 ficheros versionados, sin claves ni
estado de usuario, `main` único con árbol limpio (auditoría del árbol **y** del historial
completo). Los fixtures del proyecto CV están anonimizados (`alexrivera`, `example.com`);
tu CV real vive en `workspace/`, que está ignorado por git y **no** viaja al repo ni al
instalador.

## Ruta B — Local (zip `full` con TeX Live)

```powershell
cargo xtask dist                 # monta dist\latex-ide + latex-studio-full-<version>.zip
pwsh installer\build.ps1 -BundledTex -SkipBuild   # instalador completo (~2 GB comprimido)
```

`xtask dist` copia `tex\` (omitiendo `tex\texmf-var`, el caché de fuentes de ~167 MB
que se regenera solo), escribe `VERSION` y `SHA256SUMS.txt` y empaqueta con `tar -a`
(bsdtar de Windows 10+). El zip tarda 20-40 min en comprimir 4,4 GB.

## Probar el instalador

1. `dist\LaTeX-Studio-Setup.exe` → instala en `%LOCALAPPDATA%\LaTeX Studio` (por usuario).
2. En la página **Motor LaTeX** elige:
   - **Usar el motor existente** si ya tienes `tex\` (no se toca);
   - **Descargar TinyTeX-1** en una instalación limpia (necesita red; ~74 MB + paquetes);
   - **No instalar ahora** para probar el aviso de motor ausente.
3. Si descargaste TinyTeX, comprueba que existen
   `%LOCALAPPDATA%\LaTeX Studio\tex\bin\windows\latexmk.exe`, `synctex.exe` y `biber.exe`,
   y revisa `%LOCALAPPDATA%\LaTeX Studio\logs\tex-bootstrap.log` y
   `tex\.lsx-bootstrap.json`. En una consola:
   ```powershell
   $bin = "$env:LOCALAPPDATA\LaTeX Studio\tex\bin\windows"
   & "$bin\latexmk.exe" -v
   & "$bin\biber.exe" --version
   & "$bin\synctex.exe" --version   # imprime la versión y sale con "Missing options"; es normal
   ```
4. Sin red: elige **No instalar ahora** (o deja que falle la descarga). La app queda
   instalada y el aviso explica cómo reintentar. Para una instalación offline usa
   `pwsh installer\build.ps1 -BundledTex` (perfil completo).
5. Desinstalar: quita la app, `logs\` y la config generada; **no** toca `workspace\` ni
   `settings.json`. El `tex\` descargado con TinyTeX se conserva (puede tener paquetes
   añadidos); solo el perfil `/DBundledTex` borra `tex\`.
6. Actualizar: vuelve a lanzar el instalador ligero sobre la misma carpeta; detecta el
   `tex\` existente, lo reutiliza sin modificarlo y sobrescribe la app y el agente.
7. Si la ruta contiene acentos o caracteres no ASCII, el instalador no descarga TinyTeX
   (limitación conocida); usa una carpeta ASCII o el perfil `/DBundledTex`.

## Verificar un zip/payload

```powershell
Get-Content dist\latex-ide\SHA256SUMS.txt | Select-Object -First 5
Get-FileHash dist\LaTeX-Studio-Setup.exe -Algorithm SHA256
```
