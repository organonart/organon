# Organon Mind — build plan & tactical roadmap

> **What this is.** The high-level *tactical plan* for building **Organon Mind** — the
> most efficient order to attack the work, and a session-resumable "where are we / what's
> next." It is the companion to the epic **#483** (and its project map): #483 is the
> *what* (the full issue index + specs); **this doc is the *in what order*, and the
> *current position*.** Read both at the start of any Organon Mind session.
>
> **How it relates to the other docs.** `doc/organon_mind_prd.md` is the **product definition**
> (what the product *is* — vision, users, principles, requirements) that sits above both this
> plan and the issues; read it first for the *why/what*, this doc for the *in what order*.
> `ARCHITECTURE.md` (root) owns the durable native
> architecture and the mechanics of every subsystem this plan touches (the `Shared`/IPC
> discipline, `math.rs`, the render path) — this plan points *at* it, never restates it.
> `STATUS.md` is the volatile weekly handoff for the *whole* repo (currently the #447
> music-video rein-in); Organon Mind is a **separate thread**, and §7 below is its own
> session-start orientation. `CLAUDE.md` holds project conventions + the Mac/Ableton
> workflow.
>
> **Keep §3 and §7 current** — they are the "where are we right now" of this thread; update
> them whenever an Organon Mind PR lands.

---

## 1. What Organon Mind is (and is not)

**Organon Mind is a standalone, scientifically-correct, real-time analysis instrument for
local LLMs.** Point it at a `.gguf`, run inference, and watch and measure the model's true
structure and its live internal operation as it generates each token. It is built from the
same crate as Organon (a build-time *edition*, not a fork — #483), ships only the mind lane,
and its whole value proposition is **honesty**: it is beautiful *because* it is a faithful
diagram of a real model doing real work, not instead of it.

The bar is **scientific correctness**, held to three commitments:
1. **Structure is read from the file** — layer/head/expert counts, the wiring, straight from
   the GGUF, never invented or stylized.
2. **The live signal comes from the real forward pass** — ultimately the model's true internal
   activations, not a decorative animation.
3. **Every projection is labeled as a projection** — a 3-D shadow of a 2048-D space is shown
   *as* a shadow. (See `doc/watching_a_mind_think.md` for the public statement of this ethos.)

**What it is NOT.** Organon Mind is not a character, a VJ, or an agent. The "mind that
**acts**" — conversing as a persona, voice in/out, autonomy, the agent that *plays* Organon —
is explicitly **out of scope** here (that work lives in full Organon and the agent track:
#317 / #368 / #369, and #367 Tiers 3–6). Organon Mind observes and analyzes; it never
performs.

**But it is not *only* a visualizer.** Its arc runs **see → measure → intervene → operate**
(PRD §1.1): after the honest seeing + measuring foundation come **Stage-3 interventions**
(steering, activation patching, ablation — approved, fast-follow) and, on the roadmap, **Stage-4
model-operations** (distillation, quantization analysis, editing, adapters). The permanent
non-goal is the *character/agent* track — never working *on* models.

**The category, settled 2026-07-26 (PRD §1.2).** Organon Mind is a **reverse-engineering
workbench for a running model**, whose interface happens to be a rendering instrument — not a
visualizer with tools bolted on. That is mechanistic interpretability's own framing, and it
decides feature questions: if a capability would belong in a reverse-engineering tool, it belongs
here. **Terminology is fixed** — *reverse engineering* for the activity; *feature / circuit /
superposition / attribution graph* for the objects; **never "disassembler"** in shipped copy or
UI. Read PRD §1.2 for the full argument, including where the analogy breaks.

## 2. Scope — the hard boundary

**In scope**

| Capability | Issue(s) |
|---|---|
| The specimen — true architecture drawn from the GGUF | #367 T1 ✅, deepened by **#505** |
| Live inference observation — drive a prompt, watch it generate token-by-token | #367 T2 ✅ (proxy today) |
| **Real internal activations** — the residual tap (the scientific upgrade) | **#522** (owns it) |
| The workstation interface — tab set, card layout, resizable window | **#520** |
| The latent space — embedding galaxy, residual trajectory, logit lens, geometry scalars | **#507** |
| Telemetry / quantitative readouts — oscilloscope, spectrum, top-k, entropy plots, **export** | **#482** + #483 T3 |
| Multiple simultaneous analytical views | **#484** (split viewports) |
| The standalone product shell | **#483** |
| Model profile — resource/roofline/memory/quant geometry (*independent fast-follow panel*) | #423 (resource half) |
| Interpretability — SAE-feature *meaning* (reading) + **steering / activation patching** (Stage-3 interventions) | #409 |
| **Interventions** — steering vectors, activation patching, ablation (Stage 3, fast-follow) | #409 |

**Out of scope** (full Organon / the agent track — not this product)

- The agent lanes: **#317, #368, #369**. *(Settled 2026-07-26: building the agent **into**
  Organon was superseded by the **Organon CLI, #452, shipped** — a plain local command surface an
  external agent drives. A dedicated Organon Mind CLI may follow later. #483's map previously
  listed these as a Mind pillar; it no longer does.)*
- **#367 Tiers 3–6**: persona, voice in/out, character states, the agent convergence, and
  fine-tuning-as-character. *(A prompt box that **drives** inference to analyze it stays in;
  making the model a **character** goes out.)*
- **Surface / material / HDR *authoring* UI** — the deep authoring cards stay in full Organon.

> ⚠️ **Corrected 2026-07-27 — this boundary was drawn too tight.** It previously read "Motion /
> surface / material / HDR authoring UI — Mind ships only a slim look-preset picker." Using the
> shipped Tier-1 shell showed the opposite: **Look, Motion and Environment are all wanted**, and
> the presets rail earns its place back. It is the same renderer, and an analysis instrument is
> allowed to be beautiful. Only the **Generator** tab genuinely goes, because Mind's generator is
> always a neural network. See **Phase A′** and **#520**.

> **Settled (2026-07-24):** steering / activation patching **are in** — as **Stage-3
> interventions** (PRD §1.1), a fast-follow after the seeing + measuring foundation. Every
> intervention is explicit, logged, and reproducible, with before/after shown in the lenses.
> The observatory (#423) is **split**: its resource-geometry half is an independent fast-follow
> model-profile panel (whose value rises once Stage-4 distillation/quant is on the table); its
> J-space feature-lens half folds into #507 / #409. **Multi-model is out of scope for now.**

## 3. Where we are right now  *(audit: 2026-07-27)*

**Built and working on the Mac** (behind the `embedded-llm` cargo feature):

- The **Mind tab** in the editor (`lib.rs`), the **GGUF header parser** (`gguf.rs`), the
  **architecture specimen** (`math.rs::gguf_architecture_graph`, #367 T1), the **activation
  ring** (`mind_ring.rs`, #367 T2), the **embedded llama.cpp runtime** (`bin/mind_runtime.rs`,
  #367 T2c), and the **synthetic writer** (`bin/mind_writer.rs`). Load `.gguf` → Live →
  Generate already lights the object.

Also already built (the 2026-07-24 audit understated this): the **in-plugin Mind console**
(`mind_console.rs` — spawns the runtime as a managed child with piped stdio + a command REPL,
so no separate terminal), the **mind-log** corpus (`mind_log.rs`), and **#482 Tier 1** — the
live-telemetry widgets (`mind_viz.rs`: `MindViz` + `paint_*` egui draws reading the ring
directly, editor-side, no `Shared` change).

**The two gaps that define this roadmap:**

1. **The live signal is the entropy+confidence *proxy*, not real per-layer activations.** The tap
   is documented in `mind_runtime.rs` but **not wired**. Closing it is the #1 scientific-honesty
   upgrade — the difference between "an honest proxy for effort" and "a readout of the true
   internals." **This is Phase B**, and it is now owned by **#522**.
   - 🔑 **It also got much cheaper, 2026-07-26.** Every issue that specified this described it as
     an unsafe raw-`llama-cpp-sys-2` FFI job. **`llama-cpp-4` 0.4.2 ships it as a *safe* API**
     (`TensorCapture::for_layers` + `LlamaContextParams::with_tensor_capture`, returning
     `Vec<f32>` per layer). Verified by reading the crate source. The blocker on four issues
     (#409 T1, #507 T2–T5, #423 T4, #482's provenance flip) is now a **crate bump**, not an
     FFI project.
2. **The interface is the wrong shape.** #483 Tier 1 gave Organon Mind strictly *less* than
   Organon — Mind + Settings only. In use that turned out backwards: it wants **most** of
   Organon's surface, **rearranged**. This is **Phase A′**, inserted below, and it gates Phase B
   for a practical reason: Phase B's readouts need somewhere to live.

*(Closed 2026-07-25 — there **is** a standalone Mind edition now. See §3.1.)*

**#505 / #484 are specs** (no code yet). **#507 Tier 1 is built**; its Tiers 2–5 are not.

### 3.1 Phase A — ✅ shipped

- **#516 — #483 Tier 1, the edition shell — MERGED.** `edition.rs` (`Edition` `Full`|`Mind` +
  `EDITION`, behind the default-off `mind-edition` feature), the **IPC namespace fork** (all 27
  `$TMPDIR` paths through `ns_file`; `$ORGANON_IPC_NS` override so one visual binary serves both
  products; Full stays byte-identical at `organic-math`), the **`UiTab` filter** (`mind_ui.rs`),
  and the **`organon-mind`** standalone binary. Plus **`MIND_ARCHITECTURE.md`** — the living state
  doc. **The standalone binary runs on the Mac and shows the Mind tab.** Milestone A met.
- **#518 — #507 Tier 1, the embedding galaxy — PR open.** `gguf_data.rs` (GGUF payload reader +
  F32/F16/Q8_0/Q4_0/Q4_K/Q6_K dequant + a bit-deterministic PCA basis),
  `math::embedding_galaxy_graph`, wired as `Shared.mind[2]` (`topo_mode`) **= 2** — no `Shared`
  append, no `LAYOUT_VERSION` bump. Render-changing → **gates on one Mac pass**.
  ⚠️ **Merge #518 before #521** — #521 rearranges the region #518's topology selector sits in, so
  the other order means hand-resolving `lib.rs`.

`MindFrame` was deliberately **not** appended — that is the Phase-B spine step, and it is now a
**three-way** append (see Phase B).

## 4. The ordered path — the critical path, phased

**Guiding principle:** the capability already exists and works; so **stand up the dedicated
build first** (it's mostly a refactor around what runs today), *then* make it scientifically
deep, *then* make it a real instrument. Get something launchable and honest in front of James
fast, and deepen from there.

### Phase A — Stand up the dedicated build  *(✅ shipped — kept for the record)*

- **#483 Tier 1 ✅** — the `mind-edition` cargo feature + a standalone Mind binary + the IPC
  namespace fork + a Mind-only UI, wrapping the **existing** Mind tab.
  - *Why first:* the capability is already there; this is a mostly-headless refactor that turns
    it into a launchable **"Organon Mind."** It is the shortest path to the dedicated build.
  - *The lesson it taught, which is why Phase A′ exists:* "Mind-only UI" was read as *Organon
    minus everything*, and that was the wrong instinct. See Phase A′.
  - *Cloud-verifiable:* compiles with **and** without `--features mind-edition`; namespace +
    tab-filter unit tests. *Needs the Mac:* boot + shared-visual launch against its own namespace.
  - **Milestone A:** *"Organon Mind" launches, loads a model, and shows the specimen + live glow.*
- **#507 Tier 1 — the embedding galaxy** *(concurrent, free)*: static, from the file, zero
  runtime/IPC change — pure `math.rs`. A parallel agent can build it anytime; it delivers an
  immediate second scientific view (semantic "meaning space" from the model's own embeddings).

### Phase A′ — The workstation interface  *(inserted 2026-07-27 — do before Phase B)*

**Why this exists.** Phase A shipped a standalone Mind that *works*, and using it revealed a
design error: **we stripped too much out.** Tier 1 reasoned "Mind is Organon minus the things it
doesn't need" and landed on Mind + Settings. The correct shape is the opposite — Mind wants
**most of Organon's surface, rearranged**, because it is the same renderer and an analysis
instrument is allowed to be beautiful. The Generator *tab* is the only thing that genuinely goes.

**Why it gates Phase B rather than running after it.** Phase B and C add readouts — real per-layer
telemetry, provenance glyphs, the dashboard, lenses. Those need surface area and a settled place
to live. Building them against a layout we already know is wrong means building them twice.

**What lands (#520):**
- **Tier 1 — the tab set and the Mind tab's card layout** (PR **#521**, open). Mind's tabs become
  `Mind · Look · Motion · Environment · Settings`, in that order, deliberately unlike full
  Organon's. **One layout, both editions** — no branching. The Mind tab collapses from three
  independently-laid-out stacked blocks into a **single three-column grid**, which is what makes
  a real arrangement expressible at all: Neural Network / Model-Specimen / Chat-Agent across the
  top, Design Space below, the #482 telemetry dashboard spanning the width underneath. The
  **presets rail comes back** (Tier 1 removed it; Look/Motion/Environment are exactly what
  presets capture, so it earns its place). No generator selector on the Mind tab.
- **Tier 2 — a resizable, maximizable editor window** (both editions). A dense three-column
  workstation is unusable on a laptop at a fixed 1280×860. The columns are *already* proportional
  (`fixed_columns` divides available width), so this is purely about letting the window resize:
  `nih_plug_egui::ResizableWindow` for VST3/CLAP, and objc'ing `NSResizableWindowMask` onto the
  standalone's `NSWindow` (a new `window_macos.rs`, `#[path]`-included, the same pattern
  `hdr_macos.rs` uses). **Needs the Mac** — it is objc poking at a window baseview believes is
  fixed-size.

**Beyond #520 — the target James is specifying.** #520 is the *first correction*, not the whole
interface. The destination is PRD §5: a central splittable viewport with **per-pane lens
dropdowns**, left/right/bottom docks, **linked selection across panes**, savable layouts, a
command palette, and the external mirror. Treat #520 as unblocking the near term and **expect a
further interface pass to be specified before Phase C's workspace work** — that specification is
James's to write, and Phase C should not be started on assumptions about it.

- *Cloud-verifiable:* both builds green (± `--features mind-edition`), `cargo test` both ways, tab
  order/membership tests, and a test pinning full Organon's tab set unchanged. Neither tier
  touches `Shared`, `LAYOUT_VERSION`, the class IDs, shaders, or the renderer.
- **Milestone A′:** *the Mind window reads as a workstation, resizes, and has room for the Phase-B
  and Phase-C readouts.*

### Phase B — Scientific honesty  *(the heart of the thesis)*

- **The real residual tap — #522** (owns it; formerly described as "#367 T2b / the #507 T2
  spine"). Bump `llama-cpp-2` → **`llama-cpp-4`** inside the `embedded-llm` feature and attach a
  `TensorCapture` over a strided layer set, so per-layer residual/activation is **real, not
  proxied**, and the #482 `Provenance` glyphs flip `?` → `=`. Highest-value upgrade in the whole
  plan, and it is now a **safe API rather than an unsafe FFI project**. Everything downstream
  (trajectory, geometry scalars, honest per-layer glow, SAE features) rides it.
  - #522 Tier 2 adds head-level detail (needs flash-attention **off**, which the existing
    `mind[7]` `full_attn` dial already exposes) and the layer-selection dial.
  - #522 Tier 3 is a **spike, not a feature**: `TensorCapture` is read-only, so whether we can
    write *into* the graph is an open question that **#409's steering tier depends on**. Answer it
    here so Phase E doesn't discover it late.
- **#505 Tier 1 (MoE experts) + Tier 3 (GQA / sliding-window)** — faithful structure across
  model families, not just dense-attention transformers.
- **⚠️ Shared `MindFrame` append — assign once, coordinated, and it is now THREE-way**
  (invariant #3): **#507 T2** (`resid_proj` + top-k), **#505 T2** (`expert_summ`), **and #409 T2**
  (the feature block). Offsets are load-bearing and a mismatch **fails silently** — wrong numbers
  on screen, no compile error, no failing test. Do the append layout **single-threaded, all three
  blocks in one sitting, before any of them is implemented.**
- **#507 Tier 4 — honest geometry scalars** (per-layer rotation angle, residual↔token alignment,
  norm growth): *exact* functions of the real vector, so they can replace the proxy as the glow
  driver. Depends on the tap.
- **Milestone B:** *the object is lit by the model's true internals, and draws any architecture
  faithfully.*

### Phase C — The analysis workspace  *(make it an instrument, not just a render)*

> **Gate:** Phase A′ noted that the *fuller* interface (PRD §5 — central viewport, docks, linked
> selection, command palette) is still to be specified by James. **Do not start the workspace work
> below on assumptions about it.** #483 Tier 2's sub-tab plan is already partly superseded by
> #520 Tier 1.

- **#483 Tier 3 (lab instrumentation)** — live plots (token/s, entropy across a generation,
  per-layer heat-strip); logit-lens / top-k readout; **A/B model compare**; mind-log corpus viewer
  with **export**. *(#483 Tier 2's panel list still describes the **content** each area needs,
  even where #520 replaced its layout.)*
- **Cross-references** *(new candidate, from the workbench framing — PRD §1.2)*: "show me every
  token where this feature fired, every layer it is active at, which features co-fire with it."
  The reverse-engineer's most-used view, and we have no equivalent. Depends on #409 Tier 2.
- ⚠️ **A logit-lens readout is specified in three places** — #507 T3, #423 T4 and #483 T3.
  **Build it once and share it.**
- **#482 — the Mind dashboard** (audio-panel-style telemetry: oscilloscope, spectrum, top-k).
  The prime "analysis tool" surface.
- **#484 Tier 3 — split viewports** — show specimen + latent space + dashboard **at once** (the
  multi-view ask). #484 T1/T2 (inside camera, re-embeddings) are secondary for an analysis tool.
- **#507 Tier 3 — logit lens** (watch the prediction resolve up the stack).
- **Milestone C:** *drive a prompt and read quantitative, multi-view answers about the run.*

### Phase D — Depth, packaging & the model profile  *(parallel / late)*

- **#483 Tier 4** — the `.app` bundle: a distributable Organon Mind with its own name / icon /
  IPC namespace, coexisting with the installed `Organon.vst3`.
- **#423 (resource half) — the model-profile / observatory panel**: resource-aware inference
  geometry (roofline / memory / quant tradeoffs), a pure derivation from GGUF headers + a hardware
  profile. Fully **independent** — build anytime. Its value *rises* at Phase E (Stage 4), where it
  becomes the planning surface for distillation/quantization. (Its J-space feature-lens half folds
  into #507 / #409, not a separate track.)
- **#409 — the interpretable mind: SAE feature *meaning*** (reading). The deepest *analysis* lens,
  and **#409 now owns all semantics** (features, labels, steering, attribution). Its Tier 1 is
  #522; it starts at Tier 2.
  - **First target: Gemma Scope + Neuronpedia** (settled 2026-07-26). Decisive reason:
    **Neuronpedia ships downloadable human-readable feature labels; Qwen-Scope ships none**
    (verified by reading the source of Qwen's own explorer — it downloads weight files only and
    renders each feature as `#41203`). Importing labels beats generating them for a first target.
  - The **feature-label corpus is a first-class versioned artifact** (PRD §1.2/§13), not a config
    file — under the workbench framing it is the `.idb` analogue and the asset that compounds. A
    label is **imported** (credited) or **established by us** (contrast-pair); **neither is
    `Measured`**, and the UI must distinguish them.
- **#507 Tier 5** — the raw component band (maximally literal, the actual vector as texture).
- **Attribution-graph import** *(candidate — PRD §13)*: Anthropic's `circuit-tracer` publishes
  per-prompt causal graphs for open-weight models. We **consume** them; we do **not** train
  cross-layer transcoders (thousands of H100-hours, out of reach). Rendering a large graph legibly
  is the part that is ours — node-link graphs hairball past a few hundred nodes, and that is
  exactly Organon's competence.

### Phase E — Intervene & operate  *(roadmap — the workbench; "not only a visualizer")*

The product stops being only a visualizer. Sequenced after the seeing + measuring foundation.

- **Stage 3 — interventions** (#409): steering vectors, activation patching, ablation on the live
  pass — each explicit, **logged, and reproducible**, with before/after shown in the lenses. Builds
  on the `cb_eval` tap (Phase B) + the linked-selection model (Phase C).
- **Stage 4 — model-operations**: distillation, quantization analysis, editing, adapters —
  producing/shaping models on the honest foundation. The model-profile panel (Phase D) is its
  planning surface. Scoped when Stages 1–3 are solid.

### At a glance

| Phase | Lands | Gates on | Milestone |
|---|---|---|---|
| **A** ✅ | #483 T1 (merged) · #507 T1 galaxy (PR #518) | — | Organon Mind launches + shows a live specimen |
| **A′** | **#520 T1 (PR #521) · #520 T2** | A | The window reads as a workstation, and resizes |
| **B** | **#522** tap · #505 T1/T3 · #507 T4 | A′ (room to show it) | Lit by true internals; any architecture faithful |
| **C** | #483 T3 · #482 · #484 T3 · #507 T3 | A′, B, **+ James's interface spec** | Quantitative, multi-view analysis of a run |
| **D** | #483 T4 · #423 profile · **#409 (Gemma Scope)** · #507 T5 | C (mostly) | Distributable app + model profile + deep analysis |
| **E** | interventions (Stage 3) → model-ops (Stage 4) | B (incl. #522 T3 spike), C | The workbench: act on and operate on models |

## 5. The dependency spine (single-threaded, in order)

1. ✅ **`Edition` abstraction + IPC namespace** (#483 T1) — unlocks the entire product. **Done.**
2. **The single grid + tab set** (#520 T1, Phase A′) — `lib.rs`'s Mind panel becomes one
   `fixed_columns` grid and the Neural Network card body is extracted to two call sites. Not
   layout-versioned, but it is a **single-file, order-sensitive refactor** with one open PR
   already touching it (#518) — do it in one agent, and **merge #518 first**.
3. **The tap + the `MindFrame` append layout** (Phase B spine, #522) — assign `resid_proj` +
   top-k (#507), `expert_summ` (#505) **and the #409 feature block** in **one sitting**;
   everything scientific rides this frame, and a mismatch fails silently.

After each spine step, the leaf work fans out to sub-agents (see §6).

## 6. What parallelizes

- **#423 (the observatory / model profile)** — independent of everything; anytime.
- **#520 T1 and T2 are disjoint** (editor layout in `lib.rs`/`mind_ui.rs` vs window plumbing in
  `params.rs` + a new `window_macos.rs`) → the two tiers of Phase A′ can run concurrently.
- **Once the tap + frame append land:** #505 geometry, #507 trajectory / lens / scalars, and
  #409's SAE encode / label mapping / tint all fan out.
- **#409's naming pass is offline tooling** — independent of the render spine, buildable anytime
  once the SAE format is known.
- **Do not** fan out the `Shared`/`MindFrame` layout, the `Edition`/IPC spine, or the #520 T1
  single-grid refactor — those are the order-sensitive single-threaded steps (invariant #3).

## 7. Session quick-start — picking this up cold

1. Read **this doc** + the **#483** project map (the epic index).
2. **Current position (2026-07-27) → Phase A shipped (#516 merged; the standalone runs on the
   Mac). Phase A′ in flight: PR #521 (#520 T1) open, #520 T2 not started. #518 open and should
   merge before #521. Phase B not started.**
   **First action: finish Phase A′** — get #518 and #521 merged, then #520 Tier 2 (the resizable
   window). **Then** Phase B's spine: the **#522** tap (a `llama-cpp-4` crate bump, *not* an FFI
   project) and with it the **three-way** `MindFrame` append (`resid_proj` + top-k for #507 T2,
   `expert_summ` for #505 T2, the feature block for #409 T2 — assigned once, together).
   *(Keep this line current as PRs land.)*
3. Re-audit the build state fast (the truth can move between sessions):
   ```bash
   cd native
   grep -nE "mind-edition|mind_ui" Cargo.toml src/*.rs 2>/dev/null   # is the edition started?
   grep -n "llama-cpp-4\|llama-cpp-2" Cargo.toml                     # has the tap crate bump landed?
   grep -n "TensorCapture\|cb_eval" src/bin/mind_runtime.rs          # is the real tap wired?
   grep -n "MIND_TABS" organon-core/src/edition.rs organon-mind/src/mind_ui.rs 2>/dev/null     # has the A′ tab set landed?
   ```
4. **Scope check before starting any task:** if the task is persona / voice / agent / a character,
   it's the *wrong project* (that's full Organon / #317) — see §2. *Interventions* (steering /
   patching) and *model-operations* (distillation) **are** in scope — they're Stage 3 / Stage 4
   of this product (§1.1 / Phase E), not the agent track.
5. **Verification bar:** `cargo build` (± `--features mind-edition` once it exists) + `cargo test`
   is the cloud ceiling; the look / feel / live inference need the Mac. A finished cloud PR is
   "green and ready to deploy," never "verified working."

## 8. Execution protocol — how to build this autonomously

**Goal: work a *phase* at a time, not an issue at a time.** The old loop (a human driving
"implement one issue → approve one PR → repeat") is the thing this protocol replaces. A session
should take a whole phase to green on its own and hand back **one** review checkpoint. This works
because the decomposition already exists — PRD §12 (workstreams), plus §5 (spine) and §6 (what
parallelizes) above — so a session doesn't need to be told *how* to split the work, only to
execute it autonomously.

**The loop, per phase:**

1. **Orient.** Read `doc/organon_mind_prd.md` + this doc + the phase's issues; load the
   `organon-dev` skill (the how-we-work layer: invariants, sub-agent fan-out, the verification
   bar). Start a fresh branch off `main`.
2. **Build the spine single-threaded first** (§5): the `Edition`/IPC-namespace work and any
   `Shared`/`MindFrame` layout are **order-sensitive and append-only** (invariant #3) — one agent,
   in sequence. Never fan these out.
3. **Then fan out.** Spawn sub-agents (or run a Workflow) across the phase's *disjoint*
   workstreams (§6 / PRD §12). Use **worktree isolation** for agents that edit the crate in
   parallel so they don't collide. Each sub-agent verifies its slice to `cargo test` + naga green.
4. **Open PRs off `main`, never stacked** (the repo's #1 foot-gun — see `organon-dev` / `CLAUDE.md`).
   One coherent PR per landable increment; a phase may produce several independent PRs.
5. **Don't stop for per-issue sign-off.** Take the *whole phase* to green, then surface **one
   handoff**: the batch of PRs + a single **Mac-deploy checklist** (what to `./deploy.sh` and
   exactly what to eyeball). That checklist is the human's one checkpoint for the phase.

**Merge-trust tiers** (so review effort matches risk):
- **Headless / pure-logic PRs** (GGUF parsing, projection + geometry math, export, layout/selection
  reducers, the `Edition`/IPC plumbing) are fully cargo-verified here → **mergeable on green** with
  a glance; no Mac pass required.
- **Render-changing PRs** (anything that alters what's drawn — lenses, shaders, the viewport) still
  **gate on one Mac deploy** before merge. `cargo test` + naga can't judge the look.

**What stays human-owned:** the Mac deploy + look/feel check, and the merge decision — but **batched
once per phase**, not once per issue. That is the whole optimization.

**Guardrails (non-negotiable, from `organon-dev` / `ARCHITECTURE.md`):** `Shared`/`MindFrame` is
append-only (invariant #3); never touch the VST3 class ID / CLAP ID; never run `cargo fmt`; a param
is a full chain, not a line; default-inert new capability; no stacked PRs.

**Kickoff message for a fresh session** (paste this and go):

> Read `doc/organon_mind_prd.md` (especially §1.2, the reverse-engineering frame, and §5, the
> interface) and `doc/organon_mind_buildplan.md`, load the `organon-dev` skill, and start a fresh
> branch off `main`. Build **Organon Mind Phase B** per §8's execution protocol: spine first —
> the **#522** tap (a `llama-cpp-4` crate bump) and the **three-way** `MindFrame` append, assigned
> in one sitting — then fan out sub-agents across the parallel workstreams, verify everything to
> green, open PRs off `main`, and finish with one Mac-deploy checklist + the batch of PRs for me
> to review. Keep `MIND_ARCHITECTURE.md` current. Use a workflow if it helps.

*(Swap the phase name as the plan advances. **Right now Phase A′ is the live one** and is mostly
review/merge work — #518 then #521 — plus #520 Tier 2, so it does not need a full autonomous
phase run.)*
