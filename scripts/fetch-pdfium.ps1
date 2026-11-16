# 下載固定版本的 PDFium（bblanchon/pdfium-binaries）到 vendor\pdfium，並驗證 SHA-256
# 版本需與 pdfium-render 的 pdfium_8125 API 一致（pdfium-render 0.9.5 支援的最新版；8152 尚無對應 feature）
param(
    [switch]$Force
)
$ErrorActionPreference = 'Stop'

$Tag     = 'chromium/8125'
$Asset   = 'pdfium-win-x64.tgz'
$Sha256  = '9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b'
$Root    = Split-Path -Parent $PSScriptRoot
$Dest    = Join-Path $Root 'vendor\pdfium'
$Dll     = Join-Path $Dest 'pdfium.dll'

if ((Test-Path $Dll) -and -not $Force) {
    Write-Host "PDFium 已存在：$Dll（用 -Force 重新下載）"
    exit 0
}

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ("pdfium-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    $Tgz = Join-Path $Tmp $Asset
    $Url = "https://github.com/bblanchon/pdfium-binaries/releases/download/$([uri]::EscapeDataString($Tag))/$Asset"
    Write-Host "下載 $Url"
    Invoke-WebRequest -Uri $Url -OutFile $Tgz -UseBasicParsing

    $Actual = (Get-FileHash $Tgz -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Actual -ne $Sha256) {
        throw "SHA-256 不符：預期 $Sha256，實際 $Actual"
    }

    tar -xzf $Tgz -C $Tmp
    if ($LASTEXITCODE -ne 0) { throw "解壓縮失敗" }

    New-Item -ItemType Directory -Force -Path (Join-Path $Dest 'licenses') | Out-Null
    Copy-Item (Join-Path $Tmp 'bin\pdfium.dll') $Dest -Force
    Copy-Item (Join-Path $Tmp 'LICENSE') (Join-Path $Dest 'LICENSE') -Force
    Copy-Item (Join-Path $Tmp 'VERSION') (Join-Path $Dest 'VERSION') -Force
    Copy-Item (Join-Path $Tmp 'licenses\*') (Join-Path $Dest 'licenses') -Force
    Write-Host "完成：$Dll"
}
finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
