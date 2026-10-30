# 開發用：以 UI Automation 操作 GUI 進行端到端轉換測試
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = Split-Path -Parent $PSScriptRoot
$work = Join-Path ([IO.Path]::GetTempPath()) ("pdfword-ui-" + [guid]::NewGuid())
New-Item -ItemType Directory $work | Out-Null
Copy-Item (Join-Path $root 'tests\fixtures\*.pdf') $work
$files = Get-ChildItem $work -Filter *.pdf | ForEach-Object FullName

$p = Start-Process (Join-Path $root 'target\debug\pdfword.exe') -ArgumentList ($files | ForEach-Object { "`"$_`"" }) -PassThru
try {
    Start-Sleep 3
    $p.Refresh()
    $win = [Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
    $cond = New-Object Windows.Automation.PropertyCondition ([Windows.Automation.AutomationElement]::NameProperty), '開始轉換'
    $btn = $win.FindFirst([Windows.Automation.TreeScope]::Descendants, $cond)
    if (-not $btn) { throw '找不到「開始轉換」按鈕' }
    $btn.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline -and (Get-ChildItem $work -Filter *.docx).Count -lt $files.Count) { Start-Sleep -Milliseconds 300 }
    Start-Sleep 1
    & (Join-Path $PSScriptRoot 'screenshot-window.ps1') -Handle $p.MainWindowHandle -Out (Join-Path $root 'target\out\ui-done.png')
    Get-ChildItem $work | Select-Object Name, Length | Format-Table -AutoSize | Out-String | Write-Output
}
finally {
    Stop-Process $p -Confirm:$false -ErrorAction SilentlyContinue
    Start-Sleep 1
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
