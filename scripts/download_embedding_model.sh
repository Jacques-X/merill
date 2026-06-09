#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/src-tauri/resources/embedding"
BASE="https://huggingface.co/Teradata/multilingual-e5-small/resolve/main"
TMP="${TMPDIR:-/tmp}/merill-e5-fp32.onnx"

mkdir -p "$DEST"
curl -L --fail --retry 3 -o "$TMP" "$BASE/onnx/model.onnx"
for file in config.json special_tokens_map.json tokenizer.json tokenizer_config.json; do
  curl -L --fail --retry 3 -o "$DEST/$file" "$BASE/$file"
done

python3 - <<PY
import onnx
from onnxruntime.quantization.matmul_nbits_quantizer import (
    DefaultWeightOnlyQuantConfig,
    MatMulNBitsQuantizer,
)

source = onnx.load("$TMP")
config = DefaultWeightOnlyQuantConfig(
    block_size=128,
    is_symmetric=True,
    accuracy_level=4,
    bits=4,
    op_types_to_quantize=("MatMul", "Gather"),
)
quantizer = MatMulNBitsQuantizer(model=source, algo_config=config)
quantizer.process()
quantizer.model.save_model_to_file(
    "$DEST/model.onnx",
    use_external_data_format=False,
)
PY

shasum -a 256 "$DEST/config.json" "$DEST/model.onnx" \
  "$DEST/special_tokens_map.json" "$DEST/tokenizer.json" \
  "$DEST/tokenizer_config.json"
