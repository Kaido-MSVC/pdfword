# 開發用：啟動程式、等待視窗出現後擷取視窗畫面
[CmdletBinding(PositionalBinding = $false)]
param([string]$Out = 'target\out\ui.png', [int]$Wait = 3, [Parameter(ValueFromRemainingArguments)][string[]]$AppArgs)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
'@
[W]::SetProcessDPIAware() | Out-Null
$exe = Join-Path (Split-Path -Parent $PSScriptRoot) 'target\debug\pdfword.exe'
$p = if ($AppArgs) { Start-Process $exe -ArgumentList $AppArgs -PassThru } else { Start-Process $exe -PassThru }
Start-Sleep $Wait
$p.Refresh()
$h = $p.MainWindowHandle
[W]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 500
$r = New-Object W+RECT
[W]::GetWindowRect($h, [ref]$r) | Out-Null
$bmp = New-Object Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
$g = [Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
$bmp.Save($Out)
Stop-Process $p -Confirm:$false
Write-Output "saved $Out ($($bmp.Width)x$($bmp.Height))"
