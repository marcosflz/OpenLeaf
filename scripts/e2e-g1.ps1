# scripts/e2e-g1.ps1 — E2E del gate G1 con input sintético (clic + teclado + portapapeles).
#
# ⚠️ AVISO (2026-10-17): este script envía TECLAS/CLICS REALES al escritorio. En este
# entorno la activación de la ventana GPUI desde un shell en segundo plano está
# bloqueada (foreground lock de Windows), así que las teclas pueden acabar en la
# app en primer plano (p. ej. VS Code). NO lo ejecutes sin cerrar/minimizar el resto
# y verificar el foco; el E2E oficial es `scripts/e2e-file.ps1` (dirigido por archivo).
# Se conserva como referencia para máquinas donde el shell sí tenga derechos de foco.
#
# Conduce la app real con SendInput (clic + teclado + portapapeles) y captura
# evidencia en shots/e2e-*.png. Requiere: app compilada (debug), tex/ disponible,
# proyecto por defecto = workspace/cv/plantilla-clasica con main.tex+main.pdf.
#
# Uso:  pwsh scripts/e2e-g1.ps1

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class E2E {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    public static IntPtr FindWindow(uint pid) {
        IntPtr best = IntPtr.Zero; long bestArea = 0;
        EnumWindows(delegate(IntPtr h, IntPtr l) {
            uint wpid; GetWindowThreadProcessId(h, out wpid);
            if (wpid != pid || !IsWindowVisible(h)) return true;
            RECT r; if (!GetWindowRect(h, out r)) return true;
            long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
            if (area > bestArea) { bestArea = area; best = h; }
            return true;
        }, IntPtr.Zero);
        return best;
    }
    public const uint LEFT_DOWN = 0x0002, LEFT_UP = 0x0004, KEYUP = 0x0002;
    public static void Click(int x, int y) {
        SetCursorPos(x, y); System.Threading.Thread.Sleep(150);
        mouse_event(LEFT_DOWN, 0, 0, 0, IntPtr.Zero); System.Threading.Thread.Sleep(60);
        mouse_event(LEFT_UP, 0, 0, 0, IntPtr.Zero);
    }
    public static void Key(byte vk) { keybd_event(vk, 0, 0, IntPtr.Zero); keybd_event(vk, 0, KEYUP, IntPtr.Zero); }
    public static void Combo(byte mod, byte vk) {
        keybd_event(mod, 0, 0, IntPtr.Zero); System.Threading.Thread.Sleep(40);
        Key(vk); System.Threading.Thread.Sleep(40);
        keybd_event(mod, 0, KEYUP, IntPtr.Zero);
    }
    public static void CtrlClick(int x, int y) {
        keybd_event(0x11, 0, 0, IntPtr.Zero); System.Threading.Thread.Sleep(80);
        Click(x, y);
        keybd_event(0x11, 0, KEYUP, IntPtr.Zero);
    }
}
"@

[E2E]::SetProcessDPIAware() | Out-Null

$VK_CONTROL = 0x11; $VK_MENU = 0x12
$VK_S = 0x53; $VK_Z = 0x5A; $VK_J = 0x4A; $VK_V = 0x56
$VK_END = 0x23; $VK_HOME = 0x24; $VK_DOWN = 0x28; $VK_RETURN = 0x0D

function Capture([string]$name) {
    & pwsh -File "$repo\scripts\capture-window.ps1" -ProcessName latex-studio -Out "$repo\shots\$name.png" -DelaySeconds 0
    "capturado $name"
}

# 1) Arranque
$p = Start-Process "$repo\target\debug\latex-studio.exe" -PassThru
Start-Sleep -Seconds 8
$h = [E2E]::FindWindow([uint32]$p.Id)
if ($h -eq [IntPtr]::Zero) { throw "no se encontro la ventana de la app" }
$topmost = 0x0002 -bor 0x0001 -bor 0x0010
[E2E]::SetWindowPos($h, [IntPtr](-1), 0, 0, 0, 0, $topmost) | Out-Null
Start-Sleep -Milliseconds 500
$r = New-Object E2E+RECT
[E2E]::GetWindowRect($h, [ref]$r) | Out-Null
$W = $r.Right - $r.Left; $H = $r.Bottom - $r.Top
"ventana: $W x $H en ($($r.Left),$($r.Top))"
$editorX = $r.Left + [int]($W * 0.42); $editorY = $r.Top + [int]($H * 0.35)
$pdfX = $r.Left + [int]($W * 0.73); $pdfY = $r.Top + [int]($H * 0.55)
$chatX = $r.Left + [int]($W * 0.84); $chatY = $r.Top + [int]($H * 0.89)
Capture "e2e-1-arranque"

# 2) Editar + guardar -> compila y el PDF se actualiza
[E2E]::Click($editorX, $editorY)
Start-Sleep -Milliseconds 400
[E2E]::Combo($VK_CONTROL, $VK_END)
$stamp = Get-Date -Format "HHmmss"
Set-Clipboard "`n% E2E-G1 $stamp"
Start-Sleep -Milliseconds 200
[E2E]::Combo($VK_CONTROL, $VK_V)
Start-Sleep -Milliseconds 400
[E2E]::Combo($VK_CONTROL, $VK_S)
Start-Sleep -Seconds 7
Capture "e2e-2-guardado-compilado"

# 3) Inyectar error -> problemas + squiggle
Set-Clipboard "`n\undefinedcommand{}"
[E2E]::Combo($VK_CONTROL, $VK_V)
Start-Sleep -Milliseconds 300
[E2E]::Combo($VK_CONTROL, $VK_S)
Start-Sleep -Seconds 8
Capture "e2e-3-error"

# 4) Deshacer -> volver a verde
[E2E]::Combo($VK_CONTROL, $VK_Z)
Start-Sleep -Milliseconds 300
[E2E]::Combo($VK_CONTROL, $VK_S)
Start-Sleep -Seconds 7
Capture "e2e-4-recuperado"

# 5) SyncTeX forward (cursor -> PDF)
[E2E]::Click($editorX, $editorY)
[E2E]::Combo($VK_CONTROL, $VK_HOME)
for ($i = 0; $i -lt 6; $i++) { [E2E]::Key($VK_DOWN); Start-Sleep -Milliseconds 60 }
Start-Sleep -Milliseconds 300
# Ctrl+Alt+J (helper propio, sin problemas de quoting)
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class E2E2 {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
    public static void CtrlAltJ() {
        keybd_event(0x11, 0, 0, IntPtr.Zero); keybd_event(0x12, 0, 0, IntPtr.Zero); keybd_event(0x4A, 0, 0, IntPtr.Zero);
        System.Threading.Thread.Sleep(80);
        keybd_event(0x4A, 0, 2, IntPtr.Zero); keybd_event(0x12, 0, 2, IntPtr.Zero); keybd_event(0x11, 0, 2, IntPtr.Zero);
    }
}
'@ -ErrorAction SilentlyContinue
[E2E2]::CtrlAltJ()
Start-Sleep -Seconds 2
Capture "e2e-5-synctex-forward"

# 6) SyncTeX inverse (Ctrl+clic en el PDF)
[E2E]::CtrlClick($pdfX, $pdfY)
Start-Sleep -Seconds 2
Capture "e2e-6-synctex-inverse"

# 7) Chat: prompt real (modelo por defecto = DeepSeek V4.1 Flash)
[E2E]::Click($chatX, $chatY)
Start-Sleep -Milliseconds 500
Set-Clipboard "Responde exactamente: hola E2E"
Start-Sleep -Milliseconds 200
[E2E]::Combo($VK_CONTROL, $VK_V)
Start-Sleep -Milliseconds 300
[E2E]::Key($VK_RETURN)
Start-Sleep -Seconds 18
Capture "e2e-7-chat"

# Fin: restaurar apilamiento y cerrar
[E2E]::SetWindowPos($h, [IntPtr](-2), 0, 0, 0, 0, $topmost) | Out-Null
taskkill /PID $p.Id /T /F | Out-Null
Start-Sleep -Seconds 1
$left = (Get-CimInstance Win32_Process -Filter "Name='latex-studio.exe'" | Measure-Object).Count
"E2E fin. procesos latex-studio restantes: $left"
