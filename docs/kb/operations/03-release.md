---
id: kb/operations/release
title: Release: instalador, assets y verificación
status: stable
updated: 2026-10-24
source:
  - docs/release.md
  - installer/build.ps1
  - .github/workflows/installer.yml
  - docs/qa/reports/M3-instalador-local-2026-10-18.md
tags: [operaciones, release, instalador]
related: [kb/architecture/instalador, kb/operations/latex-bootstrap]
verified: true
---

# Release: instalador, assets y verificación

## Ruta rápida (instalador ligero en local)

```powershell
cargo build --release -p latex-studio
cargo xtask dist --no-tex --no-zip --clean
pwsh installer\build.ps1 -SkipBuild
(Get-FileHash dist\LaTeX-Studio-Setup.exe -Algorithm SHA256).Hash.ToLower() |
  Set-Content dist\LaTeX-Studio-Setup.exe.sha256
```

Publicar con `gh`:

```powershell
gh release create v0.2.0 dist\LaTeX-Studio-Setup.exe dist\LaTeX-Studio-Setup.exe.sha256 `
  --title "LaTeX Studio 0.2.0" --notes-file docs\release-notes.md
```

## Ruta CI

`.github/workflows/installer.yml` se dispara a mano o con un tag `v*`:

1. descarga `opencode-windows-x64.zip` de `OPENCODE_VERSION`;
2. compila release y monta `dist/latex-ide` con `cargo xtask dist --no-tex --no-zip --clean`;
3. instala Inno Setup 6 con Chocolatey;
4. compila el `.iss` y publica `Setup.exe`, `.sha256`, `SHA256SUMS.txt` y `VERSION` como
   artifact.

## Verificación mínima de una release

- [ ] `Setup.exe` instala por usuario en `%LOCALAPPDATA%\LaTeX Studio` sin UAC.
- [ ] Primer arranque: la app abre, crea `workspace/` y `config/`.
- [ ] Con el perfil nuevo: el instalador deja TeX o la app ofrece instalarlo (P2-INST-1).
- [ ] Un proyecto de plantilla compila y el PDF se ve.
- [ ] El chat responde y ve las 4 skills (`latex-beamer`, `latex-compilar-depurar`,
      `latex-matematicas`, `latex-tablas-figuras`).
- [ ] Reinstalar no toca `tex/`, `workspace/` ni `config/settings.json`.
- [ ] Desinstalar deja `workspace/` y `settings.json`; se lleva `logs/`.
- [ ] Ruta con espacios y acentos funciona.
- [ ] SHA-256 del asset descargado coincide con el local.

## Hechos de la release 0.1.0

- `LaTeX-Studio-Setup.exe`: 53,8 MB, SHA-256 `fe068434…5288c`, tag `v0.1.0`.
- Perfil ligero (sin `tex/`), probado con un *junction* a un TeX existente.
- El tag `v*` publica el artifact del workflow, no toca los assets de la release.
