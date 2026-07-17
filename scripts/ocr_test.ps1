# Desktop OCR end-to-end test: renders a test image with GDI+, then runs the
# scanking-core ocr_e2e test against the real models in ui\models.
# ASCII only (PowerShell 5.1 safe). Output -> scripts\ocr_test.log
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Log = Join-Path $PSScriptRoot "ocr_test.log"
Set-Content $Log "=== OCR e2e test $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') ==="

Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap(900, 320)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::FromArgb(252, 250, 246))
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAlias
$f1 = New-Object System.Drawing.Font("Arial", 34, [System.Drawing.FontStyle]::Bold)
$f2 = New-Object System.Drawing.Font("Arial", 26)
$black = [System.Drawing.Brushes]::Black
$g.DrawString("Hello World 123", $f1, $black, 40, 40)
$g.DrawString("Scan Test 456", $f2, $black, 40, 130)
$g.DrawString("Invoice No 2026-0717", $f2, $black, 40, 210)
$g.Dispose()
$img = Join-Path $env:TEMP "sk_ocr_test.png"
$bmp.Save($img, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Add-Content $Log "test image: $img"

$env:SK_MODELS = Join-Path $Root "ui\models"
$env:SK_OCR_IMG = $img
Add-Content $Log "SK_MODELS=$env:SK_MODELS"

Set-Location $Root
$env:CARGO_TERM_COLOR = "never"
# route through cmd so cargo's stderr doesn't trip PowerShell's Stop preference
& cmd /c "cargo test -p scanking-core --test ocr_e2e -- --nocapture >> $Log 2>&1"
Add-Content $Log "EXITCODE=$LASTEXITCODE"
if ($LASTEXITCODE -eq 0) { Add-Content $Log "=== OCR TEST OK ===" } else { Add-Content $Log "=== OCR TEST FAILED ===" }
