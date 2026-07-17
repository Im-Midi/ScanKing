#!/usr/bin/env bash
# 下载离线 OCR 模型（PaddleOCR v4 ONNX，来自 rapidocr_onnxruntime PyPI 包，Apache-2.0）
# 用法：bash scripts/fetch_models.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/ui/models"
TMP="$(mktemp -d)"
mkdir -p "$DEST"

echo "查询 PyPI 获取模型包地址..."
URL=$(curl -s https://pypi.org/pypi/rapidocr-onnxruntime/1.4.4/json | python3 -c "import json,sys; print(next(u['url'] for u in json.load(sys.stdin)['urls'] if u['filename'].endswith('.whl')))")
echo "下载: $URL"
curl -L "$URL" -o "$TMP/rapidocr.whl"

echo "解压模型..."
unzip -q -o "$TMP/rapidocr.whl" -d "$TMP"
M="$TMP/rapidocr_onnxruntime/models"
cp "$M/ch_PP-OCRv4_det_infer.onnx"          "$DEST/det.onnx"
cp "$M/ch_PP-OCRv4_rec_infer.onnx"          "$DEST/rec.onnx"
cp "$M/ch_ppocr_mobile_v2.0_cls_infer.onnx" "$DEST/cls.onnx"
rm -rf "$TMP"

echo "完成！模型已放入 ui/models/："
ls -lh "$DEST"
echo "keys.txt 已随项目提供。重新打包 App 即可使用离线 OCR。"
