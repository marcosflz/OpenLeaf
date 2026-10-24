<#
    get-opencode.ps1 — descarga el binario portable de opencode si falta.

    Uso:
      pwsh -File scripts/get-opencode.ps1
      pwsh -File scripts/get-opencode.ps1 -Version v1.18.30

    El binario se guarda en agent\opencode.exe (ignorado por git). La versión
    fijada debe coincidir con la que usa el workflow installer y con
    docs/release.md.
#>
[CmdletBinding()]
param(
    [string]$Version = 'v1.18.30',
    [string]$Destination
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$dest = if ($Destination) { $Destination } else { Join-Path $repo 'agent' }
$exe = Join-Path $dest 'opencode.exe'

if (Test-Path $exe) {
    $installed = (& $exe --version 2>&1 | Select-Object -First 1).ToString().Trim()
    Write-Host "opencode ya presente: $exe ($installed)" -ForegroundColor DarkGray
    if ($installed -ne $Version.TrimStart('v')) {
        Write-Warning "La versión instalada ($installed) no coincide con la fijada ($Version)."
    }
    exit 0
}

New-Item -ItemType Directory -Force -Path $dest | Out-Null
$zip = Join-Path $env:TEMP "opencode-$Version.zip"
$url = "https://github.com/sst/opencode/releases/download/$Version/opencode-windows-x64.zip"

Write-Host "Descargando opencode $Version..." -ForegroundColor Cyan
curl.exe -L --fail --retry 3 -o $zip $url
if ($LASTEXITCODE -ne 0) { throw "falló la descarga de $url" }

$tmp = Join-Path $env:TEMP "opencode-$Version-extract"
if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
Expand-Archive -Path $zip -DestinationPath $tmp -Force
$found = Get-ChildItem $tmp -Recurse -Filter 'opencode.exe' | Select-Object -First 1
if (-not $found) { throw "el zip de opencode no contiene opencode.exe" }

Copy-Item $found.FullName $exe -Force
$installed = (& $exe --version 2>&1 | Select-Object -First 1).ToString().Trim()
Write-Host "opencode instalado: $exe ($installed)" -ForegroundColor Green
if ($installed -ne $Version.TrimStart('v')) {
    throw "versión inesperada: $installed (esperaba $Version)"
}
