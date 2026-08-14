#!/usr/bin/env python3
"""Run a research brief against a **local model** and file the report.

The companion to `research-brief.py`, which builds the prompt. This one sends it to an
OpenAI-compatible endpoint on loopback and writes the answer into
`doc/research/reports/` with the front matter `--validate` requires.

## Why loopback, and why this exact wire shape

This repository already talks to local models, and it settled the convention long before
this script: `agent.rs` POSTs to an OpenAI-compatible endpoint read from the
`organic-math-agent.txt` sidecar, defaulting to `http://127.0.0.1:1234/v1/chat/completions`
— LM Studio's Local Server port, with Ollama on 11434 as the documented alternative. And
Organon's *own* `organic-math-mind-runtime` serves that same shape on the same port, so
the model under test can be the one this project embeds.

So there is nothing to design here. Matching the existing convention means every backend
the Performer agent already works with works here for free, and pointing this at
`mind-runtime` makes an Organon-hosted model audit Organon.

⚠️ **`http://` and loopback only.** No API keys, no TLS, no vendor. If you want a hosted
model's opinion, paste the prompt from `research-brief.py` into it by hand — that is the
normal path and it is what `doc/research/README.md` describes. This script exists for the
leg that can be automated *without* sending the repository to anyone.

## The honest limit of a local leg

A model reached over `/v1/chat/completions` has **no file access**. It sees exactly what
is in the prompt and nothing else, which decides what it can legitimately be asked:

  it CAN   check the prose in the durable docs against the measured fact pack, and against
           each other — stale counts, internal contradictions, claims about the build that
           the manifest data contradicts. All of that is *in the pack*.
  it CANNOT verify a claim about a source file it was never shown. A local run that reports
           `verified` findings about code is hallucinating, and on `doc-code-fidelity` —
           which is scored on precision — that is the expensive failure.

`--context` therefore packs the durable docs (budgeted, largest-first truncation) and
tells the model plainly that `verified` is only available for text it can actually see.
A local report that comes back mostly `inferred` is the system working, not failing.

Usage:
    # against LM Studio / Ollama / organic-math-mind-runtime on loopback
    python3 native/tools/research-run.py --brief doc-code-fidelity --model qwen3-8b

    python3 native/tools/research-run.py --brief doc-code-fidelity --model llama3 \\
        --endpoint http://127.0.0.1:11434/v1/chat/completions

    python3 native/tools/research-run.py --brief doc-code-fidelity --model m --dry-run
"""

import argparse
import datetime
import importlib.util
import json
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "research_brief", Path(__file__).resolve().parent / "research-brief.py")
rb = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(rb)

ROOT = rb.ROOT

# The same default `agent.rs` carries, deliberately character-for-character: LM Studio's
# Local Server. Ollama is the documented alternative on 11434, and
# `organic-math-mind-runtime` serves this shape on 1234 too (ORGANIC_MATH_LLM_PORT).
DEFAULT_ENDPOINT = "http://127.0.0.1:1234/v1/chat/completions"

# Characters of durable-doc text to pack. ~120k chars is roughly 30k tokens, which fits a
# 32k-context local model with the brief and the answer. Raise it for a long-context model;
# the packer reports what it dropped either way, because a silent truncation would make the
# model look like it missed something it was never shown.
DEFAULT_CONTEXT_BUDGET = 120_000


def context_pack(budget):
    """The durable docs, packed to a budget, with what was dropped stated in the pack.

    Largest-first truncation rather than whole-file dropping: a doc that appears at all
    should appear from its beginning, since that is where its structural claims live, and
    a reader (human or model) can see exactly where it was cut.
    """
    docs = [(d, ROOT / d) for d, _, _ in rb.durable_docs()]
    for extra in ("README.md", "CONTRIBUTING.md", "CLAUDE.md", "LICENSING.md",
                  "SECURITY.md"):
        if (ROOT / extra).exists():
            docs.append((extra, ROOT / extra))

    texts = {d: p.read_text(encoding="utf-8", errors="replace") for d, p in docs}
    total = sum(len(t) for t in texts.values())

    # Proportional trim, biggest first, until the whole set fits.
    if total > budget:
        share = budget / total
        for d in sorted(texts, key=lambda k: -len(texts[k])):
            keep = max(2000, int(len(texts[d]) * share))
            if len(texts[d]) > keep:
                texts[d] = texts[d][:keep]

    out = ["## Documents (the full text you are auditing)\n",
           "You have **no file access**. These documents, and the measured repository",
           "state above, are everything you can see. A claim about anything else cannot",
           "be `verified` — mark it `inferred` or `speculative`, or leave it out.\n"]
    for d, p in docs:
        t = texts[d]
        original = len(p.read_text(encoding="utf-8", errors="replace"))
        cut = ("" if len(t) == original
               else f"  ⚠️ TRUNCATED — showing {len(t):,} of {original:,} characters. "
                    "Do not conclude anything from the absence of text below this point.")
        out.append(f"\n### `{d}`{cut}\n")
        out.append("~~~")
        out.append(t)
        out.append("~~~")
    return "\n".join(out)


def build_prompt(brief_id, budget, with_context):
    briefs = [b for b in rb.load_briefs() if b[0].stem == brief_id]
    if not briefs:
        raise SystemExit(f"error: no brief `{brief_id}`. Try research-brief.py --list.")
    path, meta, body = briefs[0]
    prompt = rb.dispatch(path, meta, body, rb.fact_pack())
    if with_context:
        prompt += "\n\n---\n\n" + context_pack(budget)
    return path, meta, prompt


def call(endpoint, model, prompt, timeout):
    """POST the OpenAI-compatible shape `agent.rs` already speaks. Loopback http:// only."""
    if not endpoint.startswith("http://"):
        raise SystemExit(f"error: only http:// loopback endpoints are supported: {endpoint}")
    host = re.sub(r"^http://", "", endpoint).split("/")[0].split(":")[0]
    if host not in ("127.0.0.1", "localhost", "[::1]", "::1"):
        raise SystemExit(
            f"error: `{host}` is not loopback. This script is for a model running on this "
            "machine; it deliberately cannot send the repository to a remote host.")

    payload = json.dumps({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        # Low but not zero: a drift audit wants the model's best reading, not a creative
        # one, and greedy decoding makes a re-run reproducible enough to compare.
        "temperature": 0.2,
        "max_tokens": 8192,
        "stream": False,
    }).encode()

    req = urllib.request.Request(endpoint, data=payload,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            data = json.load(r)
    except urllib.error.URLError as e:
        raise SystemExit(
            f"error: could not reach {endpoint} ({e}).\n"
            "       Start a local server first — LM Studio (port 1234), Ollama (11434),\n"
            "       or this repo's own `organic-math-mind-runtime` (--features embedded-llm).")
    try:
        return data["choices"][0]["message"]["content"]
    except (KeyError, IndexError):
        raise SystemExit(f"error: unexpected response shape: {json.dumps(data)[:400]}")


def slug(s):
    return re.sub(r"[^a-z0-9.-]+", "-", s.lower()).strip("-")


def write_report(brief_id, model, endpoint, body, packed):
    sha = rb.git("rev-parse", "--short", "HEAD", default="unknown")
    today = datetime.date.today().isoformat()
    dest = rb.REPORTS / f"{brief_id}-{slug(model)}-{today}.md"
    n = 2
    while dest.exists():
        dest = rb.REPORTS / f"{brief_id}-{slug(model)}-{today}-{n}.md"
        n += 1

    # `status: unreviewed` is not a formality — nothing has checked this text, and the
    # note records the two things a later reader most needs: that the model had no file
    # access, and which endpoint served it.
    note = (f"local run via {endpoint}; no file access — the model saw only the fact pack"
            f"{' and the packed documents' if packed else ''}, so `verified` claims about "
            "source it was never shown are hallucinations and should be refuted on "
            "adjudication")
    head = (f"---\nbrief: {brief_id}\nmodel: {model}\n"
            f"model_surface: local OpenAI-compatible endpoint\nrun_date: {today}\n"
            f"commit: {sha}\nstatus: unreviewed\nadjudicated_by:\nnotes: {note}\n---\n\n")

    if not re.search(r"^## Claims\s*$", body, re.MULTILINE):
        body += ("\n\n## Claims\n\n_The model returned no `## Claims` section. Recorded as "
                 "returned; the missing section is itself a result about this model's "
                 "instruction-following, not something to paper over._\n")
    dest.write_text(head + body.strip() + "\n", encoding="utf-8")
    return dest


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--brief", required=True, help="brief id (see research-brief.py --list)")
    ap.add_argument("--model", required=True, help="model name as the local server knows it")
    ap.add_argument("--endpoint", default=DEFAULT_ENDPOINT)
    ap.add_argument("--context-budget", type=int, default=DEFAULT_CONTEXT_BUDGET)
    ap.add_argument("--no-context", action="store_true",
                    help="send the brief alone, without the packed documents")
    ap.add_argument("--timeout", type=int, default=1800,
                    help="seconds; a local model on CPU is slow, so this is generous")
    ap.add_argument("--dry-run", action="store_true",
                    help="build and print the prompt, contact nothing")
    args = ap.parse_args()

    _, _, prompt = build_prompt(args.brief, args.context_budget, not args.no_context)
    if args.dry_run:
        print(prompt)
        print(f"\n--- {len(prompt):,} characters (~{len(prompt)//4:,} tokens) ---",
              file=sys.stderr)
        return 0

    print(f"→ {args.endpoint}  model={args.model}  prompt={len(prompt):,} chars",
          file=sys.stderr)
    body = call(args.endpoint, args.model, prompt, args.timeout)
    dest = write_report(args.brief, args.model, args.endpoint, body, not args.no_context)
    print(f"wrote {dest.relative_to(ROOT)} ({len(body):,} chars returned)")
    print("status is `unreviewed`. Adjudicate its claims against the tree and record the "
          "verdicts in doc/research/FINDINGS.md before anything cites it.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
