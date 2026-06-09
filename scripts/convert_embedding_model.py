#!/usr/bin/env python3
"""Convert multilingual-e5-small to a quantized Core ML encoder for iOS."""

import os
import shutil

import coremltools as ct
import numpy as np
import torch
from coremltools.optimize.coreml import (
    OpLinearQuantizerConfig,
    OptimizationConfig,
    linearly_quantize_weights,
)
from transformers import AutoModel, AutoTokenizer

MODEL_ID = "intfloat/multilingual-e5-small"
MAX_INPUT = 256
ROOT = os.path.dirname(os.path.dirname(__file__))
OUTPUT = os.path.join(ROOT, "src-tauri", "gen", "apple", "assets")


class Encoder(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, input_ids, attention_mask):
        return self.model(
            input_ids=input_ids,
            attention_mask=attention_mask,
        ).last_hidden_state


os.makedirs(OUTPUT, exist_ok=True)
tokenizer = AutoTokenizer.from_pretrained(MODEL_ID)
model = AutoModel.from_pretrained(MODEL_ID).eval()

tokenizer_dir = os.path.join(OUTPUT, "EmbeddingTokenizer")
if os.path.exists(tokenizer_dir):
    shutil.rmtree(tokenizer_dir)
tokenizer.save_pretrained(tokenizer_dir)

dummy_ids = torch.zeros(1, MAX_INPUT, dtype=torch.long)
dummy_mask = torch.ones(1, MAX_INPUT, dtype=torch.long)
with torch.no_grad():
    traced = torch.jit.trace(Encoder(model), (dummy_ids, dummy_mask))

coreml_model = ct.convert(
    traced,
    inputs=[
        ct.TensorType(name="input_ids", shape=(1, MAX_INPUT), dtype=np.int32),
        ct.TensorType(name="attention_mask", shape=(1, MAX_INPUT), dtype=np.int32),
    ],
    outputs=[ct.TensorType(name="last_hidden_state", dtype=np.float16)],
    minimum_deployment_target=ct.target.iOS16,
    compute_precision=ct.precision.FLOAT16,
)
coreml_model = linearly_quantize_weights(
    coreml_model,
    config=OptimizationConfig(
        global_config=OpLinearQuantizerConfig(
            mode="linear_symmetric",
            dtype=np.int8,
        )
    ),
)
coreml_model.save(os.path.join(OUTPUT, "EmbeddingModel.mlpackage"))
print(f"Wrote EmbeddingModel.mlpackage and EmbeddingTokenizer to {OUTPUT}")
