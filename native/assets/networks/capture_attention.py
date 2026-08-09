#!/usr/bin/env python3
"""Capture a REAL transformer's self-attention for a prompt and dump it in the
Organon "attention" JSON schema (#367 Tier 1) — so the Neural Network generator's
"Load Network (JSON)…" dialog gets its first genuine attention tensors, not the
synthesized gallery ones.

This runs an actual forward pass (no training) and writes the attention weights
`A[layer][head][query][key]` that the visual's `math::neural_attention_from_json`
already loads (topology = "Attention (transformer)"): tokens become a causal,
triangular attention graph, each node lit by how attended-to it is.

Schema (matches native/src/math.rs::neural_attention_from_json):
    {
      "type": "attention",
      "tokens": ["The", " cat", ...],   # optional, informational
      "layers": L, "heads": H,
      "attention": [ L ][ H ][ T ][ T ]  # row = query token, col = key token
    }

Run (Hugging Face transformers + torch — CPU is fine for a short prompt):
    pip install torch transformers
    python3 capture_attention.py --model gpt2 --prompt "The cat sat on the mat" \
        --out ~/Library/Application\\ Support/OrganicMath/networks/attention-gpt2.json

Then in Organon: Generator = Neural Network → "Load Network (JSON)…" → pick the file
→ topology auto-detects to "Attention (transformer)".

Small models keep the file sane (attention is O(L·H·T²)). gpt2 (12×12) on a
6-token prompt ≈ 12·12·6·6 floats. Cap the prompt length with --max-tokens.

Apple-silicon alternative: swap in `mlx_lm` and its `model(..., output_attentions)`
equivalent; the dump code below is framework-agnostic once you have an
`[L][H][T][T]` array.
"""

import argparse
import json
import sys


def capture(model_name: str, prompt: str, max_tokens: int):
    try:
        import torch
        from transformers import AutoModel, AutoTokenizer
    except ImportError:
        sys.exit(
            "Missing deps. Install with:  pip install torch transformers\n"
            "(Apple silicon: you can adapt this to mlx_lm instead.)"
        )

    tok = AutoTokenizer.from_pretrained(model_name)
    model = AutoModel.from_pretrained(model_name, attn_implementation="eager")
    model.eval()

    enc = tok(prompt, return_tensors="pt", truncation=True, max_length=max_tokens)
    token_strs = tok.convert_ids_to_tokens(enc["input_ids"][0].tolist())

    with torch.no_grad():
        out = model(**enc, output_attentions=True)

    attentions = out.attentions  # tuple(L) of [batch, heads, T, T]
    if not attentions:
        sys.exit(
            f"{model_name} returned no attentions. Try an encoder/decoder that "
            "exposes output_attentions (e.g. gpt2, distilgpt2, bert-base-uncased)."
        )

    n_layers = len(attentions)
    n_heads = attentions[0].shape[1]
    # [L][H][T][T], rounded to keep the file small and human-readable.
    grid = [
        [
            [[round(float(v), 5) for v in row] for row in attentions[l][0][h]]
            for h in range(n_heads)
        ]
        for l in range(n_layers)
    ]

    return {
        "type": "attention",
        "model": model_name,
        "prompt": prompt,
        "tokens": token_strs,
        "layers": n_layers,
        "heads": int(n_heads),
        "attention": grid,
    }


def main():
    ap = argparse.ArgumentParser(description="Dump a transformer's attention as Organon JSON.")
    ap.add_argument("--model", default="gpt2", help="HF model id (default: gpt2)")
    ap.add_argument("--prompt", default="The cat sat on the mat.", help="prompt to run")
    ap.add_argument("--max-tokens", type=int, default=32, help="truncate the prompt to N tokens")
    ap.add_argument("--out", default="attention-capture.json", help="output JSON path")
    args = ap.parse_args()

    data = capture(args.model, args.prompt, args.max_tokens)
    with open(args.out, "w") as f:
        json.dump(data, f)
    print(
        f"Wrote {args.out}: {data['layers']} layers × {data['heads']} heads × "
        f"{len(data['tokens'])} tokens ({args.model})."
    )


if __name__ == "__main__":
    main()
