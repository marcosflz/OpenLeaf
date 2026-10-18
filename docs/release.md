# Publicar LaTeX Studio (M3 — distribución)

Resumen de las dos rutas: **CI** (recomendada para el instalador ligero) y **local**
(para el zip `full` con TeX Live, que no puede ir a CI porque esos activos no están en git).

## Qué entra y qué no

| Pieza | ¿En git? | ¿En el instalador ligero? | Notas |
|---|---|---|---|
| `latex-studio.exe` | no (se compila) | sí (~83 MB) | `cargo build --release -p latex-studio` |
| `agent\opencode.json` | **sí** | sí | config del producto (modelo, permisos, agente) |
| `agent\opencode.exe` | no (171 MB) | sí | CI lo descarga del release oficial en la versión fijada |
| `assets\skills\**` | **sí** | sí | skills LaTeX; opencode las ve vía `config/opencode.runtime.json` |
| `tex\` (TeX Live reducido) | no (4,4 GB) | **no** | el usuario copia/actualiza su `tex\`, o usa el zip `full` |
| `workspace\` (proyectos) | no | **no** | puede tener documentos personales; `xtask dist` lo omite sin `--with-workspace` |
| `config\settings.json` | no (estado de usuario) | no | se genera en el primer arranque |

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

Inno Setup **no** está instalado en el equipo de desarrollo; el script
`installer\build.ps1 -InstallInno` lo instala por usuario (winget o curl, sin UAC)
si prefieres compilar en local.

> **Bloqueo actual (P0.4b)**: el repo no tiene remoto, así que la Action no puede
> correr todavía. Hace falta crear el repositorio en GitHub y hacer el primer push
> (privado por defecto). Es decisión del usuario.

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
2. Primer arranque: el instalador avisa si no hay `tex\bin\windows\latexmk.exe` en la
   carpeta de instalación (chequeo de M3-3); copia tu `tex\` ahí y recompila desde el botón
   **Compilar**.
3. Desinstalar: quita la app, `logs\` y la config generada; **no** toca `workspace\` ni
   `settings.json` del usuario, ni un `tex\` que el propio usuario haya puesto (solo se
   borra si el instalador lo trajo: `/DBundledTex`).
4. Actualizar: vuelve a lanzar el instalador ligero sobre la misma carpeta; sobrescribe
   la app y el agente y deja `tex\`, `workspace\` y `config\` intactos.

## Verificar un zip/payload

```powershell
Get-Content dist\latex-ide\SHA256SUMS.txt | Select-Object -First 5
Get-FileHash dist\LaTeX-Studio-Setup.exe -Algorithm SHA256
```
