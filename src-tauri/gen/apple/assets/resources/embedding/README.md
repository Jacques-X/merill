# Merill Embedding Model

This directory contains the 4-bit `multilingual-e5-small` model used for
private, on-device story clustering.

- Model: `intfloat/multilingual-e5-small`
- ONNX source: `Teradata/multilingual-e5-small`
- Quantization: ONNX Runtime blockwise 4-bit `MatMul` and `Gather`
- License: MIT
- Output dimensions: 384
- Input prefix: `passage: `
- Bundled model size: approximately 60 MB

Run `scripts/download_embedding_model.sh` from the repository root to refresh
the checked-in model and tokenizer files with pinned SHA-256 verification.
