<#
    get-agent.ps1: monta el agente LaTeX portable en agent\.

    Uso:
      pwsh -File scripts/get-agent.ps1            # monta solo si falta algo
      pwsh -File scripts/get-agent.ps1 -Force     # rehace agent\python y agent\lib

    Resultado (ignorado por git):
      agent\python\     Python embebido oficial (Windows x64), con ._pth apuntando a ..\lib y ..
      agent\lib\        nas-adk + dependencias (wheels win_amd64 / py3)
      agent\lsx_agent\  el paquete del agente (copiado de python\lsx_agent)

    Requisitos: PowerShell 7, curl.exe y uv (https://docs.astral.sh/uv/). uv solo
    resuelve e instala wheels para la plataforma destino; no se ejecuta nada
    descargado durante el montaje.

    Versiones y hashes fijados: deben coincidir con docs/release.md y el workflow
    installer. Hash del embeddable tomado de python.org por HTTPS (TOFU, 2026-11-13).
#>
[CmdletBinding()]
param(
    [string]$PythonVersion = '3.12.10',
    [string]$PythonSha256 = '4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3',
    [string]$NasAdkVersion = '0.3.0',
    [string]$NasAdkSha256 = '2bfc049455af8ec0456f9ef108910ba3281ac3189cee453fb2a2cf075b469883',
    [string]$Destination,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$dest = if ($Destination) { $Destination } else { Join-Path $repo 'agent' }
$pythonDir = Join-Path $dest 'python'
$libDir = Join-Path $dest 'lib'
$pkgDir = Join-Path $dest 'lsx_agent'
$pythonExe = Join-Path $pythonDir 'python.exe'

function Get-Verified([string]$Url, [string]$OutFile, [string]$Sha256) {
    if (-not (Test-Path $OutFile)) {
        Write-Host "Descargando $Url" -ForegroundColor Cyan
        curl.exe -L --fail --retry 3 -o $OutFile $Url
        if ($LASTEXITCODE -ne 0) { throw "falló la descarga de $Url" }
    }
    $actual = (Get-FileHash $OutFile -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $Sha256.ToLower()) {
        Remove-Item $OutFile -Force
        throw "SHA256 inesperado para $Url`n  esperado: $Sha256`n  obtenido: $actual"
    }
}

if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
    throw 'falta uv en el PATH: https://docs.astral.sh/uv/getting-started/installation/'
}

New-Item -ItemType Directory -Force -Path $dest | Out-Null
$cache = Join-Path ([System.IO.Path]::GetTempPath()) 'lsx-agent-cache'
New-Item -ItemType Directory -Force -Path $cache | Out-Null

# --- 1. Python embebido ---------------------------------------------------------
if ($Force -and (Test-Path $pythonDir)) { Remove-Item -Recurse -Force $pythonDir }
if (-not (Test-Path $pythonExe)) {
    $zip = Join-Path $cache "python-$PythonVersion-embed-amd64.zip"
    Get-Verified "https://www.python.org/ftp/python/$PythonVersion/python-$PythonVersion-embed-amd64.zip" $zip $PythonSha256
    Expand-Archive -Path $zip -DestinationPath $pythonDir -Force
    # ._pth: con él, Python ignora PYTHONPATH y el cwd; solo ve estas rutas.
    $tag = ($PythonVersion -split '\.')[0..1] -join ''
    $pth = Join-Path $pythonDir "python$tag._pth"
    @("python$tag.zip", '.', '..\lib', '..') | Set-Content -Path $pth -Encoding ascii
    Write-Host "Python $PythonVersion embebido en $pythonDir" -ForegroundColor Green
} else {
    Write-Host "Python embebido ya presente: $pythonExe" -ForegroundColor DarkGray
}

# --- 2. nas-adk y dependencias ----------------------------------------------------
if ($Force -and (Test-Path $libDir)) { Remove-Item -Recurse -Force $libDir }
if (-not (Test-Path (Join-Path $libDir 'nas'))) {
    $wheel = Join-Path $cache "nas_adk-$NasAdkVersion-py3-none-any.whl"
    Get-Verified "https://github.com/marcosflz/nas-adk/releases/download/v$NasAdkVersion/nas_adk-$NasAdkVersion-py3-none-any.whl" $wheel $NasAdkSha256
    $short = ($PythonVersion -split '\.')[0..1] -join '.'
    uv pip install --quiet --target $libDir --python-version $short `
        --python-platform x86_64-pc-windows-msvc --only-binary ':all:' `
        "nas-adk[anthropic] @ file:///$($wheel -replace '\\', '/')"
    if ($LASTEXITCODE -ne 0) { throw 'uv pip install falló' }
    # Lo que no hace falta en tiempo de ejecución.
    Get-ChildItem $libDir -Recurse -Directory -Filter '__pycache__' | Remove-Item -Recurse -Force
    Get-ChildItem $libDir -Directory -Filter 'bin' | Remove-Item -Recurse -Force
    # Documentación e IDE de pywin32 (dependencia de mcp): no se usan.
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $libDir 'pythonwin'), (Join-Path $libDir 'PyWin32.chm')
    Write-Host "nas-adk $NasAdkVersion instalado en $libDir" -ForegroundColor Green
} else {
    Write-Host "nas-adk ya presente en $libDir" -ForegroundColor DarkGray
}

# --- 3. Paquete del agente (siempre: es código del repo) ------------------------------
if (Test-Path $pkgDir) { Remove-Item -Recurse -Force $pkgDir }
Copy-Item -Recurse (Join-Path $repo 'python\lsx_agent') $pkgDir
Get-ChildItem $pkgDir -Recurse -Directory -Filter '__pycache__' | Remove-Item -Recurse -Force

# --- 4. Humo: el intérprete embebido importa el agente ---------------------------------
$version = & $pythonExe -c "import lsx_agent, nas.adk; print(lsx_agent.__version__, nas.adk.__version__)"
if ($LASTEXITCODE -ne 0) { throw 'el Python embebido no puede importar lsx_agent/nas.adk' }
$size = (Get-ChildItem $dest -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
Write-Host ("Agente listo: lsx_agent/nas-adk {0} · {1:N1} MB en {2}" -f $version, $size, $dest) -ForegroundColor Green
