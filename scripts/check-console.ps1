# scripts/check-console.ps1 — verifica que ni la app ni su hijo `opencode serve`
# abren ventanas visibles (CREATE_NO_WINDOW). Con logging a fichero.
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$log = "$env:TEMP\check-console.log"
"" | Set-Content $log

function L($m) { $m | Tee-Object -FilePath $log -Append | Out-Null; Write-Host $m }

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Collections.Generic;
public class Con3 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern int GetClassNameW(IntPtr h, StringBuilder sb, int n);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder sb, int n);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  public static string VisibleWindowsOfPids(string csvPids) {
    var set = new HashSet<uint>();
    foreach (var s in csvPids.Split(',')) { uint v; if (uint.TryParse(s.Trim(), out v)) set.Add(v); }
    var sb = new StringBuilder();
    EnumWindows(delegate(IntPtr h, IntPtr l) {
      if (!IsWindowVisible(h)) return true;
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (!set.Contains(pid)) return true;
      var c = new StringBuilder(80); GetClassNameW(h, c, 80);
      var t = new StringBuilder(120); GetWindowTextW(h, t, 120);
      sb.AppendLine("VISIBLE pid=" + pid + " class='" + c + "' title='" + t + "'");
      return true;
    }, IntPtr.Zero);
    return sb.ToString();
  }
}
'@ -ErrorAction Stop
[Con3]::SetProcessDPIAware() | Out-Null

try {
    $p = Start-Process "$repo\dist\m0-demo\latex-studio.exe" -PassThru
    L "app pid=$($p.Id) lanzada"
    Start-Sleep -Seconds 12
    L "app viva: $(-not $p.HasExited)"
    if ($p.HasExited) { L "APP SALIO: exit=$($p.ExitCode)"; exit 1 }

    $pids = @($p.Id)
    $hijos = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)")
    foreach ($h1 in $hijos) {
        $pids += [int]$h1.ProcessId
        L "hijo: $($h1.Name) pid=$($h1.ProcessId)"
        foreach ($n in @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($h1.ProcessId)")) {
            $pids += [int]$n.ProcessId
            L "nieto: $($n.Name) pid=$($n.ProcessId)"
        }
    }
    $csv = ($pids | Sort-Object -Unique) -join ','
    L "pids del arbol: $csv"
    $wins = [Con3]::VisibleWindowsOfPids($csv)
    if ($wins) { L "VENTANAS VISIBLES DEL ARBOL:`n$wins" } else { L "ventanas visibles del arbol: NINGUNA (aparte de la GPUI) ✓" }
} catch {
    L "ERROR: $($_.Exception.Message)"
    L $_.ScriptStackTrace
} finally {
    if ($p -and -not $p.HasExited) { taskkill /PID $p.Id /T /F | Out-Null }
    Start-Sleep -Seconds 1
    L "latex-studio restantes: $((Get-Process latex-studio -ErrorAction SilentlyContinue | Measure-Object).Count)"
}
