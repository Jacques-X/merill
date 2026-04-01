#!/usr/bin/env python3
"""
Convert google/flan-t5-small to two Core ML models for on-device news summarisation.

Requirements (run once):
    pip install torch transformers coremltools sentencepiece

Usage:
    python scripts/convert_model.py

Output — copy these into your Xcode project sources/resources:
    scripts/output/SummaryEncoder.mlpackage   (~10 MB after INT8 quant)
    scripts/output/SummaryDecoder.mlpackage   (~25 MB after INT8 quant)
    scripts/output/SummaryTokenizer/          (tokeniser files, ~1 MB)

Add SummaryEncoder.mlpackage and SummaryDecoder.mlpackage to the Xcode target
so they are compiled to .mlmodelc at build time.
Add SummaryTokenizer/ as a folder resource (blue folder in Xcode).

The output/ directory is git-ignored by default — do not commit model files.
"""

import os
import shutil

import numpy as np
import torch
from transformers import T5ForConditionalGeneration, AutoTokenizer
import coremltools as ct
from coremltools.optimize.coreml import (
    OpLinearQuantizerConfig,
    OptimizationConfig,
    linearly_quantize_weights,
)

# ── Config ────────────────────────────────────────────────────────────────────

MODEL_ID    = "google/flan-t5-small"
OUTPUT_DIR  = os.path.join(os.path.dirname(__file__), "output")

# Keep these small: longer = slower inference, larger model.
MAX_INPUT   = 256   # encoder tokens  (input headlines + snippets)
MAX_OUTPUT  = 80    # decoder tokens  (headline ~20, summary ~60)

# ── Load model ────────────────────────────────────────────────────────────────

print(f"Loading {MODEL_ID} …")
hf_model  = T5ForConditionalGeneration.from_pretrained(MODEL_ID)
tokenizer = AutoTokenizer.from_pretrained(MODEL_ID)
hf_model.eval()

D_MODEL   = hf_model.config.d_model   # 512

os.makedirs(OUTPUT_DIR, exist_ok=True)

# ── Save tokeniser ────────────────────────────────────────────────────────────

tok_dir = os.path.join(OUTPUT_DIR, "SummaryTokenizer")
if os.path.exists(tok_dir):
    shutil.rmtree(tok_dir)
tokenizer.save_pretrained(tok_dir)
print(f"Tokeniser → {tok_dir}")

# ── Encoder wrapper ───────────────────────────────────────────────────────────

class EncoderWrapper(torch.nn.Module):
    def __init__(self, enc):
        super().__init__()
        self.enc = enc

    def forward(self, input_ids: torch.Tensor, attention_mask: torch.Tensor) -> torch.Tensor:
        return self.enc(
            input_ids=input_ids,
            attention_mask=attention_mask,
        ).last_hidden_state  # [1, MAX_INPUT, D_MODEL]

# ── Decoder wrapper ───────────────────────────────────────────────────────────
#
# Uses a FIXED-length decoder input (always MAX_OUTPUT tokens) plus a
# one-hot float pos_mask that selects which position's logits to return.
# This avoids variable-length tensors and keeps output small (vocab logits only).
#
# In Swift the loop:
#   1. decoderIds[0] = pad_token_id (T5 decoder start)
#   2. For step in 0..<MAX_OUTPUT:
#        posMask = one-hot at index step
#        logits  = decoder(decoderIds, hiddenStates, encMask, posMask)
#        nextTok = argmax(logits)
#        if nextTok == eos: break
#        decoderIds[step+1] = nextTok

class DecoderWrapper(torch.nn.Module):
    def __init__(self, dec, lm_head):
        super().__init__()
        self.dec     = dec
        self.lm_head = lm_head

    def forward(
        self,
        decoder_input_ids:      torch.Tensor,   # [1, MAX_OUTPUT] int
        encoder_hidden_states:  torch.Tensor,   # [1, MAX_INPUT, D_MODEL]
        encoder_attention_mask: torch.Tensor,   # [1, MAX_INPUT] int
        pos_mask:               torch.Tensor,   # [1, MAX_OUTPUT] float one-hot
    ) -> torch.Tensor:                          # → [1, vocab_size]
        out = self.dec(
            input_ids=decoder_input_ids,
            encoder_hidden_states=encoder_hidden_states,
            encoder_attention_mask=encoder_attention_mask,
        )
        logits_all = self.lm_head(out.last_hidden_state)     # [1, MAX_OUTPUT, vocab]
        selected   = (logits_all * pos_mask.unsqueeze(-1)).sum(dim=1)  # [1, vocab]
        return selected

# ── Trace ─────────────────────────────────────────────────────────────────────

enc_wrapper = EncoderWrapper(hf_model.encoder)
dec_wrapper = DecoderWrapper(hf_model.decoder, hf_model.lm_head)

dummy_input_ids  = torch.zeros(1, MAX_INPUT,  dtype=torch.long)
dummy_input_mask = torch.ones (1, MAX_INPUT,  dtype=torch.long)
dummy_dec_ids    = torch.zeros(1, MAX_OUTPUT, dtype=torch.long)
dummy_enc_hidden = torch.zeros(1, MAX_INPUT,  D_MODEL)
dummy_pos_mask   = torch.zeros(1, MAX_OUTPUT)
dummy_pos_mask[0, 0] = 1.0

print("Tracing encoder …")
with torch.no_grad():
    traced_enc = torch.jit.trace(enc_wrapper, (dummy_input_ids, dummy_input_mask))

print("Tracing decoder …")
with torch.no_grad():
    traced_dec = torch.jit.trace(
        dec_wrapper,
        (dummy_dec_ids, dummy_enc_hidden, dummy_input_mask, dummy_pos_mask),
    )

# ── Convert encoder ───────────────────────────────────────────────────────────

print("Converting encoder to Core ML …")
cml_enc = ct.convert(
    traced_enc,
    inputs=[
        ct.TensorType(name="input_ids",      shape=(1, MAX_INPUT), dtype=np.int32),
        ct.TensorType(name="attention_mask",  shape=(1, MAX_INPUT), dtype=np.int32),
    ],
    outputs=[
        ct.TensorType(name="encoder_hidden_states", dtype=np.float16),
    ],
    minimum_deployment_target=ct.target.iOS16,
    compute_precision=ct.precision.FLOAT16,
)

# ── Convert decoder ───────────────────────────────────────────────────────────

print("Converting decoder to Core ML …")
cml_dec = ct.convert(
    traced_dec,
    inputs=[
        ct.TensorType(name="decoder_input_ids",
                      shape=(1, MAX_OUTPUT), dtype=np.int32),
        ct.TensorType(name="encoder_hidden_states",
                      shape=(1, MAX_INPUT, D_MODEL), dtype=np.float16),
        ct.TensorType(name="encoder_attention_mask",
                      shape=(1, MAX_INPUT), dtype=np.int32),
        ct.TensorType(name="pos_mask",
                      shape=(1, MAX_OUTPUT), dtype=np.float32),
    ],
    outputs=[
        ct.TensorType(name="logits", dtype=np.float32),   # [1, vocab_size]
    ],
    minimum_deployment_target=ct.target.iOS16,
    compute_precision=ct.precision.FLOAT16,
)

# ── INT8 weight quantisation (~2× size reduction with minimal quality loss) ───

print("Quantising weights (INT8) …")
q_cfg = OptimizationConfig(
    global_config=OpLinearQuantizerConfig(mode="linear_symmetric", dtype=np.int8),
)
cml_enc = linearly_quantize_weights(cml_enc, config=q_cfg)
cml_dec = linearly_quantize_weights(cml_dec, config=q_cfg)

# ── Save ──────────────────────────────────────────────────────────────────────

enc_out = os.path.join(OUTPUT_DIR, "SummaryEncoder.mlpackage")
dec_out = os.path.join(OUTPUT_DIR, "SummaryDecoder.mlpackage")

cml_enc.save(enc_out)
cml_dec.save(dec_out)

print(f"\nDone.")
print(f"  Encoder → {enc_out}")
print(f"  Decoder → {dec_out}")
print(f"  Tokeniser → {tok_dir}")
print(f"\nNext steps:")
print(f"  1. Open the Xcode project (src-tauri/gen/apple/)")
print(f"  2. Drag SummaryEncoder.mlpackage + SummaryDecoder.mlpackage into Xcode")
print(f"     (add to app_iOS target, 'Copy items if needed')")
print(f"  3. Drag SummaryTokenizer/ folder into Xcode as a folder reference (blue folder)")
print(f"  4. Build — Xcode compiles .mlpackage → .mlmodelc automatically")
