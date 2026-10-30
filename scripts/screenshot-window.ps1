# 開發用：擷取指定視窗畫面
param([Parameter(Mandatory)][IntPtr]$Handle, [Parameter(Mandatory)][string]$Out)
Add-Type -AssemblyName System.Drawing
if (-not ('W2' -as [type])) {
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class W2 {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
'@
}
[W2]::SetProcessDPIAware() | Out-Null
[W2]::SetForegroundWindow($Handle) | Out-Null
Start-Sleep -Milliseconds 400
$r = New-Object W2+RECT
[W2]::GetWindowRect($Handle, [ref]$r) | Out-Null
$bmp = New-Object Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
[Drawing.Graphics]::FromImage($bmp).CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
$bmp.Save($Out)
