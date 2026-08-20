---
id: product-landscape
title: Where does Organon actually sit, and against what?
one_line: The outside view — what already exists near these three products, and what is genuinely different.
scoreable: no
cadence: every release
models: at least three, from different labs
---

## Question

Three products ship from this one workspace: **Organon** (a parametric generative
visualizer — VST3/CLAP plugin plus standalone, with a separate-process fullscreen
visual), **Organon Mind** (a standalone instrument for watching a language model think,
built by reading a `.gguf` and drawing the model's real wiring), and **Organon Console**
(a GPU-composited workspace for operating AI agents).

For each of the three, answer:

1. **What already exists that does this?** Name real, specific tools — not categories.
   Say what each one does that Organon does not, and what Organon does that it does not.
2. **Who is the buyer, and what do they use today?** Be concrete about the person: what
   they already own, what they would have to give up, what would make them switch.
3. **What is the honest differentiator** — the one sentence that survives contact with
   someone who owns the incumbent?
4. **What is the strongest argument this product should not exist?** Make it properly,
   at full strength. A weak version of this answer is worth nothing.

Then, across all three: **is the shared-engine, three-front-of-house structure a real
advantage or a story told after the fact?** The workspace shares an algorithm, a shader
set, an IPC layout and a preset store across a music-visual tool, a model-inspection tool
and an agent workstation. Argue both sides, then commit to one.

## Scope

Mostly **outside** the repository. Read enough of `README.md`, `doc/guide/`,
`doc/reference/` and the three architecture documents to know what the products actually
do — then spend your budget on the world, not on the tree.

Relevant neighbourhoods to search, not an exhaustive list: real-time visual/VJ tooling and
node-based visual environments; creative-coding frameworks; plugin-format visualizers that
live inside a DAW; model-inspection and interpretability interfaces, including graph
viewers for model files and attention/activation visualizers; and native or terminal-based
workspaces for coding agents. Search for what shipped in the last two years — this space
moves, and a landscape written from a stale memory is the main way this brief fails.

Out of scope: the code's quality, its architecture, and whether it works. Other briefs
own those.

## Method

Search the live web. Prefer primary sources — the tool's own site, repository, release
notes, pricing — over roundups and listicles. When you cite adoption, licensing, or price,
cite where you got it and when it was published; when you can't confirm one, say so
instead of estimating.

Two failure modes to avoid deliberately:

- **Category inflation.** "It's like TouchDesigner but simpler" is not a landscape entry.
  Name the specific product, version, and the concrete thing it does or doesn't do.
- **Politeness.** You are not reviewing a pitch deck. Where a product is a solution
  looking for a problem, the useful report says so plainly and explains why.

## Deliverable

Beyond the standard output contract, include:

- A comparison table per product: rows are named competitors, columns are what they do
  better / worse / not at all.
- One paragraph per product headed **"The case against"**, stated as strongly as you can.
- A final section, **"What I would want to know that I couldn't find out"** — the market
  facts that would change your assessment and where they'd come from.

Mark every claim about another product's behaviour as `verified` only if you found it in
that product's own documentation. Anything from memory is `speculative`, regardless of how
confident you feel.
