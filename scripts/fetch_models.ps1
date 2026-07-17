# Download offline OCR models (PaddleOCR v4 ONNX from the rapidocr_onnxruntime
# PyPI package, Apache-2.0). ASCII-only output to stay compatible with
# Windows PowerShell 5.1 default encoding.
# Usage:  powershell -ExecutionPolicy Bypass -File scripts\fetch_models.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root "ui\models"
New-Item -ItemType Directory -Force -Path $dest | Out-Null

if ((Test-Path (Join-Path $dest "det.onnx")) -and
    (Test-Path (Join-Path $dest "rec.onnx"))) {
    Write-Host "[OK] models already present in ui\models - nothing to do"
    Get-ChildItem $dest | ForEach-Object { Write-Host ("  {0}  {1:N1} MB" -f $_.Name, ($_.Length / 1MB)) }
    exit 0
}

$tmp = Join-Path $env:TEMP "sk_models_dl"
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

Write-Host "[1/3] querying PyPI for package URL..."
$meta = Invoke-RestMethod "https://pypi.org/pypi/rapidocr-onnxruntime/1.4.4/json"
$whlUrl = ($meta.urls | Where-Object { $_.filename -like "*.whl" } | Select-Object -First 1).url
Write-Host "      $whlUrl"

Write-Host "[2/3] downloading..."
$whl = Join-Path $tmp "rapidocr.whl"
Invoke-WebRequest -Uri $whlUrl -OutFile $whl

Write-Host "[3/3] extracting models..."
$zip = Join-Path $tmp "rapidocr.zip"
Copy-Item $whl $zip
Expand-Archive -Path $zip -DestinationPath $tmp -Force

$models = Join-Path $tmp "rapidocr_onnxruntime\models"
Copy-Item (Join-Path $models "ch_PP-OCRv4_det_infer.onnx")          (Join-Path $dest "det.onnx") -Force
Copy-Item (Join-Path $models "ch_PP-OCRv4_rec_infer.onnx")          (Join-Path $dest "rec.onnx") -Force
Copy-Item (Join-Path $models "ch_ppocr_mobile_v2.0_cls_infer.onnx") (Join-Path $dest "cls.onnx") -Force

Remove-Item -Recurse -Force $tmp
Write-Host ""
Write-Host "[DONE] models installed to ui\models:"
Get-ChildItem $dest | ForEach-Object { Write-Host ("  {0}  {1:N1} MB" -f $_.Name, ($_.Length / 1MB)) }
Write-Host "keys.txt ships with the repo. Rebuild the app to bundle OCR."
