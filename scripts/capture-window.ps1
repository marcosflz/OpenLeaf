# scripts/capture-window.ps1 — captura DPI-aware de la ventana PRINCIPAL de un proceso.
#
# Uso:
#   pwsh scripts/capture-window.ps1 -ProcessName spike-tree -Out shots/tree.png [-DelaySeconds 2]
#
# Notas (verify M0):
# - DPI-unaware en 200% recorta/parece 2x → SetProcessDPIAware().
# - Los procesos con consola tienen varias ventanas: elegimos la de MAYOR área visible
#   (EnumWindows + GetWindowThreadProcessId), no MainWindowHandle.
# - CopyFromScreen con la ventana flotada TEMPORALMENTE con HWND_TOPMOST
#   (no roba el foco; se restaura a NOTOPMOST al terminar).

param(
    [Parameter(Mandatory = $true)][string]$ProcessName,
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$DelaySeconds = 2
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32Cap {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter, int X, int Y, int cx, int cy, uint uFlags);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    public static IntPtr FindBestWindow(uint pid) {
        IntPtr best = IntPtr.Zero;
        long bestArea = 0;
        EnumWindows(delegate(IntPtr h, IntPtr l) {
            uint wpid;
            GetWindowThreadProcessId(h, out wpid);
            if (wpid != pid) return true;
            if (!IsWindowVisible(h)) return true;
            RECT r;
            if (!GetWindowRect(h, out r)) return true;
            long w = r.Right - r.Left;
            long ht = r.Bottom - r.Top;
            long area = w * ht;
            if (area > bestArea) { bestArea = area; best = h; }
            return true;
        }, IntPtr.Zero);
        return best;
    }
}
"@

[Win32Cap]::SetProcessDPIAware() | Out-Null

$proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) { throw "no encontré el proceso '$ProcessName'" }

Start-Sleep -Seconds $DelaySeconds
$h = [Win32Cap]::FindBestWindow([uint32]$proc.Id)
if ($h -eq [IntPtr]::Zero) { throw "el proceso '$ProcessName' no tiene ventana visible" }

# Flotar por encima sin robar foco; restaurar si está minimizada.
$HWND_TOPMOST = [IntPtr](-1)
$HWND_NOTOPMOST = [IntPtr](-2)
$SWP_NOMOVE = 0x0002
$SWP_NOSIZE = 0x0001
$SWP_NOACTIVATE = 0x0010
$flags = $SWP_NOMOVE -bor $SWP_NOSIZE -bor $SWP_NOACTIVATE
[Win32Cap]::ShowWindow($h, 9) | Out-Null   # SW_RESTORE
[Win32Cap]::SetWindowPos($h, $HWND_TOPMOST, 0, 0, 0, 0, $flags) | Out-Null
Start-Sleep -Milliseconds 800

$r = New-Object Win32Cap+RECT
[Win32Cap]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.Right - $r.Left
$ht = $r.Bottom - $r.Top
if ($w -le 0 -or $ht -le 0) { throw "rect inválido: $w x $ht" }

$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }

$bmp = New-Object System.Drawing.Bitmap $w, $ht
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $ht)))
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

[Win32Cap]::SetWindowPos($h, $HWND_NOTOPMOST, 0, 0, 0, 0, $flags) | Out-Null

"captured $Out ($w x $ht, dpi-aware, topmost)"
