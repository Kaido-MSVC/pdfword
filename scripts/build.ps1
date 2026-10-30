# 建置 Portable（zip）與 MSI 安裝版
#   pwsh -File scripts\build.ps1            完整建置（含測試）
#   pwsh -File scripts\build.ps1 -SkipTests
param([switch]$SkipTests)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Step($msg) { Write-Host "`n=== $msg ===" -ForegroundColor Cyan }
function Check($what) { if ($LASTEXITCODE -ne 0) { throw "$what 失敗（結束代碼 $LASTEXITCODE）" } }

$Version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$Name = "pdfword-$Version"
$Dist = Join-Path $Root 'dist'
$AppDir = Join-Path $Dist $Name

Step "PDFium"
& (Join-Path $PSScriptRoot 'fetch-pdfium.ps1')

if (-not $SkipTests) {
    Step "測試"
    cargo test --release; Check 'cargo test'
}

Step "建置 release（lld-link）"
cargo build --release; Check 'cargo build'

Step "第三方授權聲明"
python -I (Join-Path $PSScriptRoot 'gen-notices.py'); Check 'gen-notices.py'
python -I (Join-Path $PSScriptRoot 'make-license-rtf.py'); Check 'make-license-rtf.py'

Step "整理程式資料夾 $AppDir"
if (Test-Path $AppDir) { Remove-Item -Recurse -Force $AppDir }
New-Item -ItemType Directory -Force (Join-Path $AppDir 'licenses\pdfium') | Out-Null
Copy-Item target\release\pdfword.exe $AppDir
Copy-Item vendor\pdfium\pdfium.dll $AppDir
Copy-Item licenses\THIRD-PARTY-NOTICES.txt (Join-Path $AppDir 'licenses')
Copy-Item vendor\pdfium\LICENSE (Join-Path $AppDir 'licenses\pdfium\pdfium-binaries-LICENSE.txt')
Copy-Item vendor\pdfium\licenses\* (Join-Path $AppDir 'licenses\pdfium')
Copy-Item docs\使用說明.txt $AppDir

# 確認執行檔不依賴 VC++ 執行階段
$imports = & 'C:\Program Files\LLVM\bin\llvm-readobj.exe' --coff-imports (Join-Path $AppDir 'pdfword.exe') | Select-String 'Name: ' | ForEach-Object { $_.Line.Trim() }
if ($imports -match 'VCRUNTIME|MSVCP|ucrtbase') { throw "pdfword.exe 依賴 VC++ 執行階段：$($imports -join ', ')" }

Step "Portable zip"
$Zip = Join-Path $Dist "$Name-portable-x64.zip"
if (Test-Path $Zip) { Remove-Item $Zip }
Compress-Archive -Path $AppDir -DestinationPath $Zip

Step "MSI（WiX 7）"
$Msi = Join-Path $Dist "$Name-x64.msi"
wix build installer\pdfword.wxs -arch x64 -ext WixToolset.UI.wixext -culture zh-TW -acceptEula wix7 `
    -d "Version=$Version" -d "AppDir=$AppDir" -o $Msi
Check 'wix build'
Remove-Item (Join-Path $Dist "$Name-x64.wixpdb") -ErrorAction SilentlyContinue

Step "完成"
Get-ChildItem $Zip, $Msi | Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } } | Format-Table -AutoSize
