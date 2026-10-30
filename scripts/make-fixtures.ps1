# 以 Microsoft Edge（headless）把 tests\fixtures\src\*.html 印成 PDF，輸出到 tests\fixtures
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Edge = @(
    "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
    "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $Edge) { throw '找不到 Microsoft Edge' }

$Profile = Join-Path ([IO.Path]::GetTempPath()) ("pdfword-edge-" + [guid]::NewGuid())
try {
    Get-ChildItem (Join-Path $Root 'tests\fixtures\src\*.html') | ForEach-Object {
        $Out = Join-Path $Root ("tests\fixtures\" + $_.BaseName + '.pdf')
        $Uri = ([uri]$_.FullName).AbsoluteUri
        & $Edge --headless --disable-gpu --no-pdf-header-footer "--user-data-dir=$Profile" "--print-to-pdf=$Out" $Uri 2>$null | Out-Null
        Write-Host "產生 $Out"
    }
}
finally {
    Remove-Item -Recurse -Force $Profile -ErrorAction SilentlyContinue
}
