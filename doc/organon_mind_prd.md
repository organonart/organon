# Organon Mind — Product Requirements Document (PRD)

> **Status: DRAFT v0.1 (2026-07-24).** This is the **product definition** for Organon Mind —
> the north star that sits *above* the issues and determines what the product is. It is meant
> to be stable: we iterate to "nail it," then it guides the whole build.
>
> **What this doc is (and how it relates to the others).**
> - **This PRD** — *what the product is*: vision, users, principles, the experience, the
>   requirements. Above the issues. Designed to be handed to a fresh session with the
>   instruction "implement this," which is why §12 decomposes it into sub-agent-parallelizable
>   workstreams mapped to issues.
> - `doc/organon_mind_buildplan.md` — the *tactical order* (which phase, what's next).
> - Epic **#483** + its `Mind`-labeled issue map — the *implementation vehicles*.
> - `MIND_ARCHITECTURE.md` (created at the first Mind PR) — the *living state doc*: what
>   **exists now**, updated every PR so a new session is instantly oriented. Not a target spec.
> - Root `ARCHITECTURE.md` — the durable **shared-engine** architecture (`math.rs`, render
>   pipeline, `Shared`/IPC). Authoritative on everything Organon Mind reuses.
> - Ethos reference: `doc/watching_a_mind_think.md` (the public statement of the honesty stance).
>
> **How to build from this PRD.** Read §11 (scope), §12 (workstreams + dependency spine), then
> the buildplan for phase order. Cloud sessions build + `cargo test`; the look / live inference
> need the Mac (see §10). A finished cloud PR is "green and ready to deploy," never "verified."

---

## 1. Vision

**Organon Mind is a standalone instrument for watching a language model think — honestly, in
real time, from many linked perspectives at once.** Point it at a model file, give it a prompt,
and see the model's true architecture and its live internal operation rendered as something you
can both *feel* and *measure*. It is beautiful because it is faithful: every shape is read from
the real model, every live signal comes from the real forward pass, and every projection is
labeled as the shadow it is.

It is the analytical, scientific sibling of Organon (the VST visualizer). Same codebase, same
renderer, different posture: where Organon is a projector-first performance instrument, Organon
Mind is a viewport-first **analysis workstation**.

### 1.1 The trajectory — not only a visualizer

Organon Mind begins by making a model *legible*, but it is not only a visualizer. Its arc runs
through four stages, and the north star includes all of them:

1. **See** — the honest, real-time visualization of structure and internal state (v1 core).
2. **Measure** — quantitative telemetry, reproducible runs, export (v1 core; the instrument).
3. **Intervene** — steering vectors, activation patching, ablation: probes that *act on* the
   running model to test and understand it, all logged and reproducible (first ones land as a
   fast-follow; approved for the product).
4. **Operate** — the workbench: the model-operations practitioners actually do — distillation,
   quantization analysis, editing, adapters — producing or shaping models, not just reading them
   (roadmap; §11 makes clear these are *later stages*, not permanent non-goals).

v1 establishes Stages 1–2 rigorously and opens the door to Stage 3. Everything is designed so the
workbench can grow on the same honest foundation: an intervention is only trustworthy if you can
*see* and *measure* its effect — which is exactly what Stages 1–2 provide. The one thing that
stays permanently out is the **character/agent** track (§11); working *on* models never is.

### 1.2 The frame — a reverse-engineering workbench

**What category of tool is this?** Not "a visualizer that also has tools." Organon Mind is a
**reverse-engineering workbench for a running model**, whose interface happens to be a rendering
instrument. That reframe is the answer to the sceptic's "what is it *for*," and it should govern
how features are chosen: if a capability would belong in a reverse-engineering tool, it belongs
here; if it is decoration, it does not.

**This is the field's own framing, not a metaphor we are importing.** Mechanistic interpretability
defines itself as the effort to *reverse-engineer neural networks into human-understandable
algorithms* by studying concrete weights and activations, and the field's canonical statement of
that goal, Anthropic's *Transformer Circuits Thread* (`transformer-circuits.pub`), opens with
"can we reverse engineer transformer language models into human-understandable computer
programs?" The comparison to **decompiling a stripped binary back to source** appears in the
field's own self-description. Using this vocabulary aligns us with the research rather than
inventing a private idiom, and it means the work has an existing literature to be measured against.

**The IDA Pro workflow is the closest existing product analogue.** Five parallels are
load-bearing, and the second is the most consequential:

1. **An artifact without source.** A stripped binary and a `.gguf` are both "the thing that runs"
   with human-legible intent removed. Neither was authored to be read. Everything else follows.
2. **The annotation database is the real product.** IDA's disassembly regenerates in seconds; what
   analysts guard, version and trade is the `.idb` — the accumulated naming laid over the artifact.
   Our equivalent already has a name in the roadmap: the **feature-label corpus** (#409 Tier 2).
   Its contents arrive two ways, and the difference matters. **Gemma Scope**, our first target,
   comes with **Neuronpedia's autointerp labels** — importable, and someone else's inferred claim,
   so it is credited and versioned rather than adopted silently. **Qwen-Scope** ships weights and
   *no labels at all*, rendering features as bare indices, so there a name is **established by us**
   by contrast-pair experiment. **Treat the corpus as a first-class, versioned, shareable artifact,
   not a config file.** It is the asset that compounds, and it is the reason importing beats
   generating for a first target.
3. **Static and dynamic, which we already have.** IDA analyses the file at rest *and* attaches a
   debugger. Our static half is the specimen read from the GGUF header and tensor geometry
   (#505, #391); our dynamic half is the live activation ring (#367, #482). The split was arrived
   at independently and the analogy confirms it is the right one.
4. **Cross-references.** IDA's most-used view is "show me everywhere this is touched." The
   equivalent writes itself and we do not have it: every token where a feature fired, every layer
   it is active at, which features co-fire with it. A strong near-term lens candidate.
5. **BinDiff.** IDA's companion for structurally comparing two binaries maps directly onto
   comparing a model with its own quantization — which is exactly what #423's quality axis wants.
   A neighbouring field has a decade of UX for that problem.

**What mech interp supplies that IDA does not: the unit of analysis.** A binary decomposes into
functions and a call graph. A transformer's weights are fixed and every prompt runs the same
matmuls, so there is no control flow to recover — but that does **not** mean there is no graph.
The field's units are:

- **Feature** — a *direction* in activation space corresponding to a human-interpretable concept,
  not an individual neuron. This is the unit our SAE work already consumes.
- **Superposition** — models represent far more features than they have dimensions, so features
  are smeared across overlapping neurons and most neurons are **polysemantic**. This is *why*
  reading raw activations fails, and why the SAE step exists at all.
- **Circuit** — a subgraph of features connected by weights that implements a legible algorithm.
  This is the call-graph analogue, and it is the field's word for it. The best-documented example
  in a real model is the **Indirect Object Identification** circuit in GPT-2 Small
  (Wang et al., 2022, `arXiv:2211.00593`): 26 attention heads in seven functional
  classes — duplicate-token and induction heads detect the repeated name, S-inhibition heads
  suppress it, name-mover heads promote the survivor.
- **Attribution graph** — the per-prompt causal graph of which features influenced which, and the
  output, with non-contributing features pruned away. Produced by Anthropic's circuit-tracing work
  using **cross-layer transcoders**, and this is the answer to "what varies per prompt if the
  weights are fixed": the *causally active subgraph* varies, and that is the renderable object.

**Attribution graphs are the single best target this framing surfaces.** Anthropic open-sourced
its `circuit-tracer` library in May 2025 with a Neuronpedia frontend, working on **open-weight**
models (Gemma-2-2B, Llama-3.2-1B) — so the graphs are obtainable without us training anything. Two honest constraints:
transcoder training is out of reach (thousands of H100-hours for a few-billion-parameter model),
so we **consume** published artifacts rather than produce them; and the existing frontend is a 2-D
web UI. **Our differentiator is precisely here.** Node-link graphs collapse into hairball past a
few hundred nodes — every reverse engineer knows the feeling — and rendering large structured
fields legibly at scale is Organon's founding competence.

**Where the analogy breaks. State this before anyone else does.**

- **Disassembly is lossless, deterministic and unique. Feature decomposition is none of the
  three.** x86 bytes → instructions is a bijection: you lose names and types, never information.
  An SAE decomposition is lossy (hence the reconstruction-error readout), learned, and non-unique
  — train two SAEs on the same layer and you get two different dictionaries. Claiming
  "disassembly" claims a rigour we cannot have.
- **There is no source that ever existed.** A binary was compiled *from* something, and with the
  source in hand you can check your work. A model was fit, not written. Our features are a
  description we impose, not the recovery of something discarded. This gap never closes.
- **Instructions have crisp semantics; features have statistical tendencies.** `mov eax, ebx`
  means one thing. "#41203 fires on quotations" holds with exceptions, on evidence we generated.
- **The field itself has not achieved this.** Reviews of mech interp are explicit that
  comprehensively reverse-engineering production models into pseudocode remains far off, with open
  problems in evaluation standards, distinguishing causal circuits from spurious correlation, and
  scaling. There is also live disagreement about how literally to read words like "planning" when
  describing feature dynamics. **We inherit that caution:** a label asserting intent or reasoning
  is a contested claim and must be marked as one, never rendered as measurement.

**Terminology (settled).** Use **reverse engineering** for the activity — it is the field's term
and it promises effort, not success. Use **feature**, **circuit**, **superposition** and
**attribution graph** for the objects, matching the literature. **Debugger** is fair for the live
half (you attach to a running process, inspect state, and steering genuinely writes to it).
**Never call it a disassembler** in any shipped copy or UI: it over-claims in exactly the
direction §4's honesty principles exist to prevent. "IDA Pro for local models" is acceptable as a
*conversational pitch* provided the losslessness caveat follows immediately.

**Consequences for the build.** Three, in priority order: the feature-label corpus becomes a
versioned artifact with its own format and provenance rules (not a config file); **cross-references**
enters the lens candidate list; and **attribution-graph import** becomes the natural Stage-3/4
bridge — a published graph is a ready-made subject for the renderer, and rendering one legibly is
a thing no existing tool does well.

## 2. The product in one screen

A dark, dense, legible desktop app in the spirit of a scientific instrument (think iZotope RX
or a GPU frame debugger, not a toy). A **central visual viewport** — moderately sized, splittable
into 2 or 4 panes, each pane a **dropdown-selected lens** onto the running model. **Left**: the
model and session you're working on. **Right**: the properties of the selected lens and the
selected element (a layer, a head, a token). **Bottom, spanning the width**: the live analytics
dashboard and token log. Select a layer in one pane and it lights up in all of them — **linked
lenses**. Mirror the viewport to an external display with one toggle. Load a `.gguf`, type a
prompt, and watch it think.

## 3. Who it's for, and why

**Primary audience: dual — credible to practitioners, accessible to the curious. Honesty is the
bridge between them.** The same faithfulness that lets an interpretability researcher trust what
they see is what lets a newcomer be genuinely (not misleadingly) awed. The product must never buy
accessibility with a lie, nor rigor with opacity.

Design consequence: **accessible defaults, depth on demand.** A first-time user gets a legible,
guided default view with plain-language labels; a practitioner can open the numeric readouts,
verify counts against the model config, export the data, and drill into per-head/per-layer detail.
Neither is a separate "mode" — it's one product with progressive disclosure.

**Representative use cases**
- *Practitioner:* load a MoE model, watch which experts fire per token, verify the routing and
  the layer/head counts against the config, export per-token entropy for a plot.
- *Curious viewer:* watch Gemma hesitate on a hard word and cruise through easy text; understand,
  correctly, what the glow means.
- *Educator:* show attention and the residual stream as real, to-scale structure, not a cartoon.
- *James / artists:* pull a still or a clip of a real inference for the Workshop, or mirror a
  lens to a display — rigor that also happens to be gorgeous.

## 4. Design principles (non-negotiable)

1. **Scientific honesty, three commitments.** (a) Structure is read from the file — counts and
   wiring, never invented. (b) The live signal comes from the real forward pass — ultimately the
   model's true internal activations, not a decorative animation. (c) Every projection is labeled
   *as* a projection (a 3-D view of a high-D space is shown as a shadow). A **provenance marker**
   (measured / derived / proxy / projection) is attached to every displayed quantity.
2. **Linked lenses.** One subject, many synchronized views. Selecting or hovering an element
   (layer, head, token, expert) in any pane highlights it in all panes and in the analytics.
   This is the feature that makes it an instrument rather than several pretty windows.
3. **Instrument posture.** Serious, dark, meter-dense but legible; RX-grade. Nothing purely
   decorative in the analysis surfaces. The beauty comes from the honesty of the data.
4. **Viewport-first, mirror-optional.** The visual lives *inside* the app; the external
   projector window is an optional mirror, not the main event (the inverse of Organon-the-VST).
5. **Progressive disclosure.** Legible for a newcomer by default; every layer of depth is one
   click away for a practitioner. No dumbing-down, no wall of numbers on first run.
6. **Reproducibility.** A run is defined by (model, prompt, sampling params, seed) and can be
   re-run and exported. Analysis you can't reproduce or export isn't science.
7. **Faithful across the model zoo.** Dense, MoE, GQA/sliding-window, and recurrent/hybrid
   families each draw truthfully (see #505), not all as a generic dense transformer.
8. **No anthropomorphizing beyond the honest signal.** It shows effort, uncertainty, structure,
   and real internals. It is not a character and makes no claim to feelings or intent.
9. **Performance is a feature.** Inference (Metal) and rendering (wgpu) share one GPU; the app
   stays responsive, throttles gracefully, and never blocks the UI on a token.
10. **Interventions are honest and reversible.** As the product grows from reading into acting
    (steering, patching, and later model-operations), every intervention is explicit, logged, and
    reversible/reproducible — you can always see and measure exactly what it changed. Acting on a
    model never compromises the honesty of what is shown.

## 5. The experience — interface

### 5.1 Posture, vs Organon

| | **Organon** (the VST) | **Organon Mind** (this) |
|---|---|---|
| Form | VST3/CLAP plugin + standalone, in Ableton | Standalone native app (Mac-first) |
| Primary surface | Fullscreen external visual (projector) | In-app viewport (analysis workstation) |
| External display | The main event | Optional mirror |
| Control surface | Thin; host automates params | Rich; panels, lenses, analytics |
| Job | Perform | Observe & measure |

### 5.2 The shell — an IDE × Blender × RX hybrid

A dockable workspace with a **central viewport** and three surrounding regions. Deliberately
*not* Blender-huge: the viewport is moderately sized by default so the analysis panels have room.

```
┌───────────────┬───────────────────────────────────┬────────────────┐
│  LEFT         │        CENTRAL VIEWPORT           │   RIGHT        │
│  "Project"    │  split 1 / 2 / quad;              │  "Properties"  │
│  • Model      │  each pane: [lens ▼] dropdown     │  • Lens params │
│    (GGUF)     │  (specimen / galaxy / trajectory /│  • Selected    │
│  • Prompt /   │   logit-lens / attention / …)     │    element     │
│    run        │  [⇄ mirror to external display]   │    (layer/head/│
│  • Saved      │                                   │     token)     │
│    analyses   │                                   │  • Run config  │
├───────────────┴───────────────────────────────────┴────────────────┤
│  BOTTOM (full width): analytics dashboard + live token log/console │
│  token/s · entropy plot · top-k · per-layer heat-strip · per-head  │
└────────────────────────────────────────────────────────────────────┘
```

- **Left — "Project":** the loaded model (with parsed architecture summary), the current
  prompt/session, and saved analyses. The "explorer" of an IDE, reframed for a model.
- **Central viewport:** the visual, embedded in-app. Split 1 / 2 / quad; each pane has a
  **lens dropdown** (§6) choosing which visualization technique it renders (à la Blender's
  editor-type header / Unreal's viewport view-mode). A mirror toggle sends the pane (or grid) to
  the external display.
- **Right — "Properties":** context-sensitive — the parameters of the focused lens, and the
  details of the selected element (a layer's stats, a head's summary, a token's logits).
- **Bottom — analytics + log:** the dashboard (§7) and the live token/console log, spanning the
  width. This is where the quantitative, RX-style readouts live during a run.

### 5.3 Linked lenses (the differentiator)

A single **selection/hover model** shared across every pane and the analytics: selecting layer
*L* / head *H* / token *t* / expert *e* anywhere highlights it everywhere and scrolls the
relevant readouts to it. Panes stay in sync on the current token by default; a pane can be
**pinned** to a specific token/step for comparison.

### 5.4 Layout, command palette, external mirror

- **Savable layouts ("workspaces")** — presets of the pane/lens/dock arrangement (IDE-style).
- **Command palette** — keyboard-first access to lenses, layouts, run controls.
- **External mirror** — reuse the existing separate-process visual as a mirror surface fed the
  same scene state; it mirrors the active pane (or the grid). **No extended-desktop / multi-region
  concept in v1** — mirror only.

### 5.5 Visual identity — the color language *(adopted)*

The look is warm and scientific, deliberately **not** the overdone cyan/magenta cyberpunk. Full
spec (hex values, concept prompts, the reference images) lives in `doc/organon_mind_visual_reference.md`;
the essentials:

- **Warm graphite shell** (near-black with a hint of brown, never blue-black), charcoal panels,
  taupe hairlines; **bone-white / brushed-titanium** type and chrome. Reads as a premium lab
  instrument (Leica / Braun / Teenage Engineering), not a sci-fi HUD.
- **The data speaks in a perceptually-uniform scientific colormap — the magma/inferno family**
  (aubergine → crimson → burnt orange → amber → gold) for continuous scalars (entropy, activation,
  attention weight). It is what real ML tooling uses, so it is instantly credible *and* beautiful —
  and it is the direct antidote to the blue cliché. Discrete elements (heads / experts / lenses)
  use a **muted earthy categorical** set (teal, ochre, terracotta, sage); selection is a single
  warm-white/gold pop.

This palette differentiates Organon Mind (analytical, warm, restrained) from Organon-the-VST
(performance, full-spectrum), and every color is one we will actually implement (real colormaps /
LUTs, straight into the shaders and UI).

## 6. The lenses (visualization techniques)

Each pane hosts one lens. Every lens carries provenance markers. The v1 catalog:

| Lens | Shows | Real / projected | Issue |
|---|---|---|---|
| **Specimen** | The model's true architecture: residual axle, per-layer head rings, MLP rail; MoE expert fans; GQA/SWA structure; recurrent motifs | Real (from file) | #367 T1, **#505** |
| **Embedding galaxy** | The vocabulary embedding space projected to 3-D; semantic clusters | Real geometry, **projected** to 3-D | **#507** T1 |
| **Residual trajectory** | The token's live path up the stack through the galaxy | Real activations, **projected** | #507 T2 |
| **Logit lens** | Per-layer "what it would predict if it stopped here"; the answer resolving | Real (unembedding) | #507 T3 |
| **Geometry scalars** | Exact per-layer measures: rotation angle, residual↔token alignment, norm growth | Real, exact (no projection) | #507 T4 |
| **Raw component band** | The literal activation vector as a luminous band | Real, literal | #507 T5 |
| **Attention river** | Tokens as nodes, causal attention edges, live | Real (attention rows) | #367/#226 |
| **Effort glow** (default) | The stack lit by uncertainty + confidence | **Proxy** (labeled), until the real tap lands | #367 T2 |

The lens set is extensible; the dropdown is the extension point.

## 7. The analytics (bottom dashboard)

RX/oscilloscope-grade live telemetry off the same run: **tokens/sec**, **logit-entropy** across
the generation, **top-k** for the current token, a **per-layer activity heat-strip**, **per-head**
summaries, **KV-cache / memory** readout, and a **logit-lens table**. All linked (§5.3) and
**exportable** (§9). This is the surface that most makes it a measurement tool.

## 8. The subject — models & inference

- **Load any GGUF** (start from the LM Studio cache); parse architecture from the header;
  no weights needed to draw the specimen.
- **Run controlled prompts**: prompt, sampling params (temperature, top-k/p, repetition penalty),
  **seed**, context length, token-rate cap. Deterministic given (model, prompt, params, seed).
- **Embedded runtime** (llama.cpp, Metal) hosts the forward pass and streams per-token data.
- **Real internal activations** via the `cb_eval` tap (the scientific upgrade over today's proxy).
- **Faithful across families**: dense, MoE, GQA/sliding-window, Mamba/RWKV/Jamba (#505).
- **Interventions (Stage 3, fast-follow)**: steering vectors, activation patching, and ablation
  applied to the live forward pass — each explicit, logged, and reproducible; the linked lenses
  and analytics show the before/after, so an intervention's effect is *seen and measured*, never
  merely asserted.

## 9. Functional requirements

Grouped, testable. **MUST** = v1; **SHOULD** = v1 if cheap, else fast-follow.

**Model I/O**
- FR-1 (MUST) Load a `.gguf`; show parsed architecture (layers, heads, experts, dims, vocab) with provenance.
- FR-2 (MUST) Draw the specimen faithfully for dense models; (SHOULD) MoE + GQA/SWA; (later) recurrent.
- FR-3 (SHOULD) Browse/scan a models directory (LM Studio cache default).

**Inference**
- FR-4 (MUST) Enter a prompt and generate; stream tokens to the log and the viewport.
- FR-5 (MUST) Expose sampling params + seed; a run is reproducible from (model, prompt, params, seed).
- FR-6 (MUST) Per-token: entropy + confidence + chosen token + top-k. (MUST) Real per-layer activations via `cb_eval`.
- FR-7 (SHOULD) Token-rate cap + graceful GPU throttle; UI never blocks on a token.

**Viewport & compositor**
- FR-8 (MUST) Central in-app viewport rendering the scene (embedded wgpu).
- FR-9 (MUST) Split into 1 / 2 / 4 panes; per-pane lens dropdown.
- FR-10 (MUST) Mirror the active pane (or grid) to an external display; toggle on/off.
- FR-11 (SHOULD) Orbit/zoom per pane; pin a pane to a token/step.

**Lenses** — FR-12 (MUST) Specimen + Effort glow (proxy, labeled). FR-13 (MUST) Embedding galaxy. FR-14 (SHOULD) Residual trajectory + Logit lens + Geometry scalars. FR-15 (later) Raw component band + Attention river.

**Analytics** — FR-16 (MUST) Live token/s, entropy plot, top-k, per-layer heat-strip. FR-17 (SHOULD) Per-head, KV/mem, logit-lens table.

**Linked selection** — FR-18 (MUST) A shared selection/hover model: selecting layer/head/token/expert highlights across all panes + analytics. FR-19 (SHOULD) Pin/compare across tokens.

**Sessions & export** — FR-20 (MUST) Append a mind-log of every run (prompt/params/seed/outputs). FR-21 (MUST) Export per-token analytics (CSV/JSON). FR-22 (SHOULD) Save/load a "saved analysis" (a run + layout + lens config). FR-23 (SHOULD) Snapshot a pane to an image; (later) record a clip.

**Shell** — FR-24 (MUST) The left/center/right/bottom dock shell. FR-25 (SHOULD) Savable layouts + command palette. FR-26 (MUST) Progressive disclosure: legible default, numeric depth on demand with provenance.

**Provenance** — FR-27 (MUST) Every displayed quantity carries a provenance marker (measured / derived / proxy / projection).

**Platform** — FR-28 (MUST) macOS / Apple Silicon (Metal). FR-29 (SHOULD) Keep the architecture portable (wgpu, no gratuitous Mac-only deps outside the runtime/packaging layer); Windows is the anticipated first port once Mac is established.

**Interventions (Stage 3 — fast-follow, not v1-core)** — FR-30 Apply steering vectors / activation edits / ablation to the live forward pass; each explicit and toggleable. FR-31 Every intervention is part of the run definition (logged + reproducible); the lenses + analytics show before/after.

**Model profile / observatory (fast-follow, independent)** — FR-32 Derive + display resource-aware inference geometry (roofline, memory traffic, KV-cache cost, quantization tradeoffs) from the GGUF header + a hardware profile, with provenance (measured / derived). No forward pass required; independent of the render spine.

## 10. Verification bar

- **Cloud (here):** `cargo build` (± `--features mind-edition`) + `cargo test` (incl. naga WGSL
  validation); pure logic (parsers, projections, geometry scalars, layout/selection reducers,
  export) is unit-tested against synthetic frames. This is the ceiling.
- **Mac (James):** the embedded viewport rendering, live inference + the `cb_eval` tap, the look
  and feel of each lens, the external mirror, and GPU perf. The "does it read / does it feel like
  an instrument" judgment lives here.

## 11. Non-goals (v1)

- **Not a character / agent.** No persona, voice in/out, conversation-as-character, autonomy, or
  the agent that plays Organon (#317 / #368 / #369 and #367 Tiers 3–6 are out).
- **Not a VST/plugin.** Standalone only — no new plugin class ID, no Ableton audio-thread rules.
- **Not a training/distillation tool _in v1_** — but model-operations (distillation, quantization
  workflows, editing, adapters) are an explicit **later stage** of this product (§1.1, Stage 4),
  *not* a permanent non-goal. v1 stays analysis + first interventions.
- **Not projector-first.** The external display is a mirror, not the primary surface.
- **No extended-desktop / multi-region output** in v1 (mirror only).
- **No commercial layer in v1** — accounts, licensing, pricing are deferred (see §13).
- **Not a full editor.** No motion/surface/material/HDR authoring UI; a slim look-preset picker only.

## 12. Implementation decomposition (built to be executed)

Workstreams designed to fan out to sub-agents once the spine is in. Each maps to issues; verify per §10.

**Spine (single-threaded, first — invariant #3 / the param chain):**
- **S1 — Edition + IPC namespace** (#483 T1): the `mind-edition` feature, standalone Mind binary,
  `Edition` abstraction, IPC path fork, Mind-only UI around the existing Mind tab. *Unblocks everything.*
- **S2 — Embedded viewport + `MindFrame`/mindview spine**: bring the renderer in-process into an
  egui panel (egui-wgpu render-to-texture); define the `Shared.mindview` selector and any
  `MindFrame` append layout **once** (coordinate #505's `expert_summ` + #507's `resid_proj`/top-k).

**Then fan out (disjoint surfaces):**
- **WS-A — Compositor & shell** (#484 promoted, #483 T2): pane grid (1/2/quad), per-pane lens
  dropdown, the left/right/bottom dock, savable layouts, command palette, external mirror.
- **WS-B — Runtime & real activations** (#367 T2b): wire `cb_eval`; stream real per-layer
  activations + top-k into the frame; reproducibility (seed/params) + throttle.
- **WS-C — Lenses** (#507, #505): galaxy (independent, anytime) → trajectory → logit lens →
  geometry scalars → raw band; specimen fidelity (MoE/GQA/SWA/recurrent).
- **WS-D — Analytics** (#482, #483 T3): the bottom dashboard readouts + plots.
- **WS-E — Linked selection**: the shared selection/hover model across panes + analytics (the
  differentiator; touches WS-A/C/D contracts — define the selection model early).
- **WS-F — Sessions & export** (#483 T3): mind-log, CSV/JSON export, saved analyses, snapshots.
- **WS-G — Packaging** (#483 T4): the `.app`, its own IPC namespace + identity.
- **WS-H — Interventions (Stage 3, roadmap)** (#409): steering / activation patching / ablation on
  the live pass, logged + reproducible, before/after shown in the lenses. Builds on WS-B (the tap)
  + WS-E (selection). *Not v1-core.*
- **WS-I — Model profile / observatory (fast-follow)** (#423 resource half): derived roofline /
  memory / quant geometry; independent of the render spine — build anytime.

**What parallelizes:** WS-C's galaxy and the specimen-fidelity work, and WS-D, are largely
independent once S1/S2 land; WS-A panes and WS-D dashboard are disjoint files. Keep S1, S2, and
the selection-model contract (WS-E) single-threaded / defined-once. Phase order in the buildplan.

## 13. Decisions & open questions

**Settled (2026-07-27):**
- **The line for Organon Mind is "Because we need to see what it's doing, not just what it
  says."** What stands behind it, and the position it rests on (that the public needs the
  ability to **see inside, understand, and modify** the models it is asked to live with), is
  written up in **`doc/not_just_what_it_says.md`**. That document is a **declaration, not an
  argument**: it states what we hold and why, and it does not litigate its own weaknesses. The
  limits that belong to it (seeing is not yet understanding; the tools are early; the
  capability is dual-use, which argues for openness rather than against it) are stated where
  they bear on the claim rather than gathered into a section of concessions. Two
  editorial points that document settles: **"not just"** is load-bearing, because output is
  real evidence and the instrument shows it, so a version reading "not what it says" would
  overclaim; and the line **deliberately omits "understand"**, because understanding is the
  goal and not yet a promise anyone can keep (§1.2's "where the analogy breaks"), while
  *seeing* is deliverable today. The tagline commits to what ships; the essay carries the
  larger claim.

**Settled (2026-07-26):**
- **The product's category is a reverse-engineering workbench** (§1.2), not a visualizer with
  tools attached. Terminology is fixed: **reverse engineering** for the activity; **feature /
  circuit / superposition / attribution graph** for the objects, matching the mech-interp
  literature; **never "disassembler"** in shipped copy or UI.
- **The feature-label corpus is a first-class artifact** — versioned, shareable, with its own
  provenance rules — not a config file. It is the `.idb` analogue and the asset that compounds.
- **Attribution-graph import is the Stage-3/4 bridge.** We consume published graphs (Anthropic's
  `circuit-tracer` + Neuronpedia, open-weight models); we do **not** train cross-layer transcoders
  — the compute is out of reach and it is not our contribution. Rendering a large graph legibly is.
- **Gemma Scope + Neuronpedia is the first SAE target** (#409). Decisive reason: Neuronpedia ships
  **downloadable human-readable labels**; Qwen-Scope ships **none** (verified by reading the source
  of Qwen's own explorer — it downloads weight files only and renders each feature as `#41203`).
  Qwen-Scope is retained in #409 as a documented second source, not the first target.
- **The tap and the semantics are separate issues.** **#522** owns the residual tap only
  (`llama-cpp-4` `TensorCapture`, a safe API — four issues had independently specified this as an
  unsafe raw-FFI job). **#409** owns all semantics: SAE features, labels, steering, attribution.
  #522 was filed duplicating #409 and has been rescoped; #483's map carries the division.
- **The agent track is out of Organon Mind.** #317 / #368 / #369 were listed as a Mind pillar in
  #483's map; they are not. Building the agent *into* Organon was superseded by the **Organon CLI
  (#452, shipped)** — a plain local command surface an external agent drives. A dedicated Organon
  Mind CLI may follow later. This matches §11 and §1.1's "the one thing that stays permanently out."

**Settled (2026-07-24):**
- **Interventions are in.** Steering + activation patching are approved as **Stage-3** capabilities
  (§1.1): the product is *not only a visualizer* — it grows toward operating on models
  (distillation etc.) as Stage 4.
- **Real activations are a v1 MUST** (FR-6) — the honesty linchpin.
- **The observatory (#423) is split and re-sequenced.** Its **resource-aware inference geometry**
  (roofline / memory / quant tradeoffs) is a cheap, credible, *independent* **fast-follow** — a
  **model-profile panel** (FR-32); its value *rises* once Stage 4 makes quant/distillation choices
  actionable. Its **J-space / feature-geometry lens** overlaps #507 + #409 — **fold it into those**
  rather than run a third representation-geometry track.
- **Multi-model / A-B — out of scope for now.**
- **Cross-platform — stay portable; Windows is the anticipated first port, after Mac is
  established** (not now).
- **Commercial — none in v1.** Eventually access may open to the Workshop / a tight community / on
  request; not yet, and no accounts or licensing to design now.

**Still open:**
- The **sequence within Stage 3** (steering vs patching vs ablation first).
- Whether the **model-profile panel** is a dock panel, a viewport lens, or both.
- The first **Stage-4** target (distillation vs quantization analysis vs editing) — decide when
  Stages 1–3 are solid.

## 14. Glossary

- **Lens** — a visualization technique selectable per viewport pane.
- **Linked lenses** — synchronized selection/hover across all panes + analytics.
- **Specimen** — the model's true architecture drawn from the file.
- **Proxy** — the entropy/confidence effort signal (labeled), the stand-in until real activations.
- **Provenance marker** — measured / derived / proxy / projection tag on every displayed quantity.
- **Mirror** — the optional external-display surface fed the in-app viewport's scene.

*Mechanistic-interpretability terms (§1.2), used as the field uses them:*

- **Reverse engineering** — the activity this product supports: recovering human-understandable
  structure from weights and activations. Names an effort, not a guaranteed result. **The approved
  umbrella term; "disassembler" is not.**
- **Feature** — a *direction* in activation space corresponding to an interpretable concept. Not a
  neuron.
- **Superposition** — more features than dimensions, so features overlap and most neurons are
  **polysemantic**. The reason raw activations are unreadable and the SAE step exists.
- **Circuit** — a subgraph of features connected by weights implementing a legible algorithm. The
  call-graph analogue, and the field's own term.
- **Attribution graph** — the per-prompt causal graph of feature influence with non-contributing
  features pruned. What varies per prompt when the weights do not.
- **Feature-label corpus** — the versioned, shareable file of names assigned to feature indices.
  The `.idb` analogue and the asset that compounds. A name is either **imported** (e.g.
  Neuronpedia's autointerp labels for Gemma Scope — someone else's inferred claim, credited and
  versioned) or **established by us** (contrast-pair experiment, when a release ships weights
  only, as Qwen-Scope does). **Neither is `Measured`**; the UI must distinguish the two, because
  they have different authors and different reliability.
