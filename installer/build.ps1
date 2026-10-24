<#
    build.ps1 — Compila el instalador de LaTeX Studio con Inno Setup.

    - Monta antes la distribución (`cargo xtask dist`) si falta.
    - Busca ISCC.exe (Inno Setup 6). Con -InstallInno intenta instalarlo con
      winget (o descarga directa) **sin administrador**.
    - Compila installer\latex-studio.iss y deja el Setup en dist\.
    - Perfil ligero (por defecto): el Setup incluye la página "Motor LaTeX" y,
      si no hay un tex\ existente, descarga TinyTeX-1 (~74 MB) y ejecuta tlmgr
      durante la instalación. No necesita tex\ en tiempo de build.
    - Perfil -BundledTex: incluye dist\latex-ide\tex\ (offline, ~4,4 GB).

    Uso:
      pwsh -File installer\build.ps1                 # ligero; TinyTeX se descarga al instalar
      pwsh -File installer\build.ps1 -BundledTex     # completo/offline (incluye tex\)
      pwsh -File installer\build.ps1 -InstallInno    # instala Inno Setup si falta
      pwsh -File installer\build.ps1 -SkipBuild      # no re-monta dist\latex-ide
#>
[CmdletBinding()]
param(
    [switch]$BundledTex,
    [switch]$InstallInno,
    [switch]$SkipBuild,
    [string]$OutputName,
    [string]$Version
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
$iss  = Join-Path $here 'latex-studio.iss'
$dist = Join-Path $root 'dist\latex-ide'

function Find-Iscc {
    $candidates = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
    )
    foreach ($c in $candidates) { if ($c -and (Test-Path $c)) { return $c } }
    $cmd = Get-Command iscc.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    return $null
}

# --- 1. Distribución montada ------------------------------------------------
if (-not $SkipBuild) {
    Write-Host 'Montando dist\latex-ide con cargo xtask dist...' -ForegroundColor Cyan
    $args = @('run', '-q', '-p', 'xtask', '--', 'dist', '--no-zip')
    if (-not $BundledTex) { $args += '--no-tex' }
    Push-Location $root
    try { & cargo @args } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "cargo xtask dist falló ($LASTEXITCODE)." }
}

if (-not (Test-Path (Join-Path $dist 'latex-studio.exe'))) {
    throw "falta $dist\latex-studio.exe — ejecuta: cargo xtask dist --no-tex --no-zip"
}
if ($BundledTex -and -not (Test-Path (Join-Path $dist 'tex\bin\windows\latexmk.exe'))) {
    throw "con -BundledTex, $dist\tex debe ser un TeX Live reducido (falta bin\windows\latexmk.exe)"
}
if (-not $BundledTex) {
    Write-Host 'Perfil ligero: el Setup descargará TinyTeX-1 al instalar si no encuentra un tex existente.' -ForegroundColor DarkGray
}

# --- 2. Inno Setup ----------------------------------------------------------
$iscc = Find-Iscc
if (-not $iscc -and $InstallInno) {
    Write-Host 'Inno Setup no encontrado; instalando (sin administrador)...' -ForegroundColor Cyan
    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if ($winget) {
        & $winget.Source install --id JRSoftware.InnoSetup -e -s winget --scope user `
            --accept-source-agreements --accept-package-agreements --silent | Out-Null
        $iscc = Find-Iscc
    }
    if (-not $iscc) {
        $setup = Join-Path $env:TEMP 'innosetup.exe'
        curl.exe -L --fail -o $setup 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe'
        if ((Test-Path $setup) -and ((Get-Item $setup).Length -gt 1MB)) {
            & $setup /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER /SP- | Out-Null
            $iscc = Find-Iscc
        }
    }
}

if (-not $iscc) {
    throw @'
ISCC.exe no encontrado. Opciones:
  - reejecutar con -InstallInno (instala Inno Setup 6 en tu usuario), o
  - instalarlo a mano desde https://jrsoftware.org/isdl.php
'@
}

# --- 3. Compilar ------------------------------------------------------------
$defines = @()
if ($BundledTex) { $defines += '/DBundledTex' }
if ($OutputName) { $defines += "/DOutputName=$OutputName" }
if ($Version)    { $defines += "/DMyAppVersion=$Version" }
if (-not $Version) {
    $versionFile = Join-Path $dist 'VERSION'
    if (Test-Path $versionFile) {
        $value = ((Get-Content $versionFile -First 1) -split '\s+')[1]
        if ($value) { $defines += "/DMyAppVersion=$value" }
    }
}

Write-Host "Compilando con: $iscc $($defines -join ' ')" -ForegroundColor Cyan
& $iscc @defines $iss
if ($LASTEXITCODE -ne 0) { throw "La compilación falló (código $LASTEXITCODE)." }

$name = if ($OutputName) { $OutputName } else { 'LaTeX-Studio-Setup' }
$out = Join-Path $root "dist\$name.exe"
if (Test-Path $out) {
    $mb = [math]::Round((Get-Item $out).Length / 1MB, 1)
    Write-Host "Instalador generado: $out ($mb MB)" -ForegroundColor Green
} else {
    Write-Warning "Compilación correcta pero no se encontró $out"
}
