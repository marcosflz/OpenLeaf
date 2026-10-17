# scripts/e2e-file.ps1 — E2E del gate G1 dirigido por ARCHIVO (sin input sintético).
#
# Nota de investigación (2026-10-17): la automatización por teclado desde un shell
# en segundo plano no es fiable en esta máquina (foreground lock de Windows: el
# clic no activa la ventana GPUI desde un proceso sin derechos de foreground).
# Por eso este E2E conduce el ciclo edit->save->compile->PDF->problemas editando
# `main.tex` DESDE FUERA: el watcher del shell recarga el editor y el
# autoCompile de la cola compila — exactamente el mismo pipeline que Ctrl+S.
# La validación por teclado (SyncTeX Ctrl+Alt+J) queda cubierta por los tests
# gated de lsx-latex y pendiente de clic manual (documentado en el informe G1).

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

$tex = "$repo\workspace\cv\plantilla-clasica\main.tex"
$backup = "$env:TEMP\e2e-main.tex.bak"
Copy-Item $tex $backup -Force

function Capture([string]$name) {
    & pwsh -File "$repo\scripts\capture-window.ps1" -ProcessName latex-studio -Out "$repo\shots\$name.png" -DelaySeconds 1
    "capturado $name"
}

$p = Start-Process "$repo\target\debug\latex-studio.exe" -PassThru
Start-Sleep -Seconds 8
if ($p.HasExited) { throw "la app no arranco (exit $($p.ExitCode))" }
Capture "e2e-a-arranque"

$stamp = Get-Date -Format "HHmmss"
try {
    # Paso 2: edición válida -> compila -> PDF se actualiza
    Add-Content $tex "`n% E2E-G1 $stamp (edicion externa)"
    Start-Sleep -Seconds 7
    Capture "e2e-b-compilado"

    # Paso 3: error inyectado -> problemas + squiggle + status Failed
    Add-Content $tex "`n\undefinedcommand{}"
    Start-Sleep -Seconds 9
    Capture "e2e-c-error"

    # Paso 4: arreglo (quitar el error) -> compila verde
    $clean = (Get-Content $tex -Raw) -replace "(?m)^\\undefinedcommand\{\}\r?\n?", ""
    Set-Content $tex $clean -NoNewline
    Start-Sleep -Seconds 7
    Capture "e2e-d-recuperado"

    # Evidencia de archivos (el ciclo real ocurrió)
    "--- main.log ultimo run ---"
    Select-String -Path "$repo\workspace\cv\plantilla-clasica\main.log" -Pattern 'Output written|Latexmk: .* failed|Emergency stop' | Select-Object -Last 3 | ForEach-Object { $_.Line.Trim() }
} finally {
    Copy-Item $backup $tex -Force
    Start-Sleep -Seconds 3
    taskkill /PID $p.Id /T /F | Out-Null
    Start-Sleep -Seconds 1
    "restaurado main.tex; procesos latex-studio: $((Get-CimInstance Win32_Process -Filter \"Name='latex-studio.exe'\" | Measure-Object).Count)"
}
