# 開發用：以 Word 開啟 DOCX 驗證可讀，並另存 PDF 供目視比對
param([Parameter(Mandatory, ValueFromRemainingArguments)][string[]]$Docx)
$ErrorActionPreference = 'Stop'
$word = New-Object -ComObject Word.Application
$word.Visible = $false
$word.DisplayAlerts = 0
try {
    foreach ($f in $Docx) {
        $full = (Resolve-Path $f).Path
        $doc = $word.Documents.Open($full, $false, $true)
        $pdf = [IO.Path]::ChangeExtension($full, '.word.pdf')
        $doc.SaveAs2($pdf, 17)
        Write-Output ("OK {0} pages={1} paragraphs={2} tables={3} shapes={4}" -f (Split-Path $f -Leaf), $doc.ComputeStatistics(2), $doc.Paragraphs.Count, $doc.Tables.Count, $doc.InlineShapes.Count)
        $doc.Close($false)
    }
}
finally {
    $word.Quit()
    [void][Runtime.InteropServices.Marshal]::ReleaseComObject($word)
}
