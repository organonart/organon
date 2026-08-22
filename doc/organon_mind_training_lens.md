# Fine-tuning as Organon Mind's second axis — watching a model *become*, not only think

> **What this is.** The reasoning behind
> **[#147](https://github.com/organonart/organon/issues/147)**, which is the five-tier spine of
> work. This document is the design; that issue is the order.
>
> ✏️ **Revised 2026-08-21, hours after first writing, and the revision is the useful part.**
> The first draft was written against [unsloth-buddy](https://github.com/TYH-labs/unsloth-buddy)
> — a *skill*: 62 KB of prose plus python scripts, with training telemetry scraped off a
> `TrainerCallback` that stands up its own HTTP server. James then installed **Unsloth Studio
> (Desktop) 0.1.801-beta**, which is a different animal — a native-Windows app serving a
> **351-route FastAPI backend with a published OpenAPI spec**. §2's taxonomy answer was wrong,
> §4's transport was superseded, §5 was about a constraint that no longer exists, and one feature
> turned out to be blocked. 📌 **The thesis (§0) did not move at all**, which is the thing worth
> noticing: everything the Studio changed is *transport and catalog*, and none of it is the
> instrument.
>
> **Relations.** `doc/organon_prd.md` §6.2 owns the Mind layout, its lens catalog and the honesty
> stance — this proposes *additions* to that catalog and does not restate it. §7 there and
> `doc/organon_modules_plan.md` §12 own the extension taxonomy, which §2 below amends.
> `MIND_ARCHITECTURE.md` owns what exists; nothing here exists yet.

---

## 0. The thesis

Organon Mind watches a model **think**: structure read from the file at rest, activation read
from a live forward pass. It has no account of the half of a model's life where the weights were
*fit*. Fine-tuning produces the one artifact Mind cannot obtain any other way: **two models with
identical architecture, different weights, and a known cause for the difference.** That is a
controlled experiment handed over for free, and it is the premise of every differential lens the
PRD has already wished for (§6.2's BinDiff parallel, its cross-references entry).

🚨 The proposition is not "add a training dashboard." It is that fine-tuning gives Mind its
**second axis** — the model over training time — and the specimen already renders the first.

---

## 1. The two things called "unsloth" here, and they are not the same

| | **unsloth-buddy** (TYH-labs, MIT) | **Unsloth Studio (Desktop)** (Unsloth AI) |
|---|---|---|
| What it is | a **skill** — instructions that teach an agent a 7-phase lifecycle | a **local service** — a native app with a REST/SSE API |
| Substance | `SKILL.md` (62 KB), three sub-skills, ~20 python scripts | `unsloth-studio.exe` + a venv at `~/.unsloth/studio` serving `127.0.0.1:8888` |
| Telemetry | `GaslampDashboardCallback` stands up its own `HTTPServer`; SSE on `/api/stream` | `GET /api/train/progress` — first-party SSE with `Last-Event-ID` |
| Knows how to | *choose* a method, format a dataset, reflect on a run into `~/.gaslamp/` | *execute* and *catalog* runs, exports, adapters, checkpoints |
| Machine surface | one payload shape, scraped | 351 documented routes, `/openapi.json` |

⚠️ **They do not currently compose.** The skill shells its own `train.py` with its own callback and
knows nothing of the Studio's API. They are two front-ends onto the same library. Whether the
skill should learn to drive `POST /api/train/start` is a real question and not one this document
answers.

**What the Studio is, measured.** `unsloth 2026.8.19`, `torch 2.10.0+cu130`, `transformers 5.5.0`,
`trl 0.23.1`, `peft 0.18.1`, `bitsandbytes 0.50.1` — native Windows against the 5090, no WSL. It
vendors and builds llama.cpp and whisper.cpp under `~/.unsloth/`. Auth is `HTTPBearer` per route;
every data route 401s without a key.

The routes that matter to us: `/api/train/{start,stop,status,progress,metrics,hardware}`;
`/api/train/runs` (runs as **persistent objects** — `final_loss`, `total_steps`, `loss_sparkline`,
`output_dir`, `duration_seconds`, `can_resume`); `/api/models/loras` and
`/api/models/loras/{path}/base-model`; `/api/models/checkpoints` →
`ModelCheckpoints{base_model, lora_rank, peft_type, is_quantized}` and
`CheckpointInfo{display_name, loss, path}`; `POST /p/{run}/{checkpoint}/v1/chat/completions`
(per-checkpoint inference, nothing loaded by us); and `POST /api/export/export/{lora,gguf,merged,base}`.

---

## 2. ✏️ Where it lands — a fourth column the taxonomy does not have

The first draft's headline was *"a skill, not a module"*. That is still true of unsloth-buddy. It
is **not** true of the Studio, and the Studio is what is installed.

PRD §7 and the modules plan §12 offer three units of extension — **linked** (a crate in our address
space), **hosted** (a process we spawn and composite), **skill** (instructions the agent loads).
The Studio is none of them. We do not ship it, spawn it, composite it, or link it. **It is a local
service the user installed, and we are a client of it** — the shape `organon-agent`'s
`HttpChatClient` already has against a local model server, and the precedent to follow.

📌 **This is a gap worth recording rather than papering over.** The three kinds all answer *"how
does someone else's code get to run here?"* A local service answers a different question — *"how
do we talk to something already running that we did not install?"* — and its trust profile is
different again:

| | linked | hosted | skill | **local service** |
|---|---|---|---|---|
| Boundary | none | the process | the approval | **a socket and a bearer token** |
| Source | required | optional | it *is* source | **irrelevant — we never build it** |
| Revocation | rebuild | uninstall | withdraw approval | **rotate the key, immediately** |

⚠️ That last cell is not a small thing: §11.9 records that git supplies no revocation and that
whatever the trust model becomes, revocation *"must be designed"*. A local service has it for free,
because the credential is the boundary.

**What this decision buys, and it is most of the work not done.** No ABI, no `dlopen`, no
viewport-producer contract, no frame arbiter, no process for us to supervise. `doc/organon_module_viewport.md`
does not apply. Organon is a *client* of the result, never the owner of the run.

⚠️ **The one thing it costs is a secret.** Organon has never held a bearer token. It is not a
preset value and must not land in the preset store or `ui_theme.json`; #147 T1 owns that decision
and should make it explicitly rather than by whichever file is nearest.

---

## 3. The seams it plugs into, and one it must not

| Seam | Fit |
|---|---|
| **A new lens** — a `math.rs` graph builder feeding `neural_loaded`, selected by `Shared.mind[2]` (`topo_mode`) | ✅ Where Delta and Divergence go. The documented seam for exactly this |
| **A new analytics readout** — `mind_viz.rs`, editor-side | ✅ Where the training strip goes. **No `Shared` change, so no `LAYOUT_VERSION` movement** |
| **`Shared`** — the control-rate plugin→visual snapshot | 🚨 **Not here.** Append-only and offset-sensitive across a process boundary (`ARCHITECTURE.md` invariant #2). Step-rate telemetry from a process that is not ours must not buy a permanent layout commitment |

⚠️ If a `TrainFrame` ring is ever added beside `MindFrame`, **assign its blocks in one sitting
before any is implemented.** `MIND_ARCHITECTURE.md` §2.3's reasoning is not specific to
`MindFrame` — it is specific to mmap: two binaries indexing one map by byte offset disagree by
producing *plausible wrong numbers* and no error.

---

## 4. The features

### A. The Delta lens — the BinDiff parallel, and the cheapest real thing here

*Load a base model and a LoRA adapter; light the specimen by how much each site actually moved.*

A LoRA adapter is `adapter_config.json` (rank `r`, `lora_alpha`, `target_modules`) plus
`adapter_model.safetensors` holding `lora_A [r, in]` and `lora_B [out, r]` per adapted module.
The update is exact: **dW = (alpha/r) · B · A**. So:

> ✏️ **Corrected while building T2 — that formula is not always the update, and both exceptions
> are silent.** `use_rslora` changes the denominator to `sqrt(r)`, so reading it the naive way
> understates every norm by a factor of `sqrt(r)` with nothing erroring; and `use_dora` means the
> update is not that expression at all. ⚠️ **Both are fields of the Studio's own 70-field
> `TrainingStartRequest`**, so the app on this machine can produce adapters the formula above
> misreads. The shipped reader handles rsLoRA and *refuses* DoRA by name, detecting it from the
> config flag **and** from a `lora_magnitude_vector` tensor, because a config can be absent where
> the tensors cannot. Two more traps found the same way: PEFT's `rank_pattern` / `alpha_pattern`
> give per-module overrides — rank is recoverable from the tensor shapes and so is taken from
> them, but **alpha is not recoverable from any measurement**, so an ignored `alpha_pattern` is
> simply a wrong number; and `.lora_A.weight` and `.lora_A.default.weight` are **both** in
> circulation (`save_pretrained` strips the adapter name, a raw `state_dict` dump keeps it), so a
> reader accepting one spelling silently finds *zero* adapted modules in half the adapters it is
> handed.

- **The Frobenius norm per module is free and exact.** `||BA||_F = sqrt(trace((B^T B)(A A^T)))` —
  two r×r matrices, r=16 typically, and `dW` is never materialized.
- **The effective rank of the update is also free and exact** — QR both factors, take the singular
  values of the r×r product. *"The rank-16 adapter at layer 12 used 4.2 effective ranks"* is a
  **measured** statement about how concentrated the learning was, and nobody's tooling shows it.
- ⚠️ **Per-neuron deltas are NOT free** — a per-output-row norm needs the full out×in product. Do
  the cheap tier first and say so, rather than discovering the cliff halfway in.

📌 **The arithmetic is ours; the discovery is not, any more.** `/api/models/loras` finds the
adapters, `/base-model` pairs each with its base, and `/api/models/checkpoints` supplies
`lora_rank` and `is_quantized`. The first draft carried a warning that a delta against unsloth's
default 4-bit base is a delta against a *quantized* base and that the readout must say so; that is
now answerable **from data** rather than from discipline.

📌 **Still the feature to build first, and the reason is the verification bar.** No GPU, no token,
no running Studio — a published LoRA adapter off HuggingFace is the fixture. The whole lens is
developable and unit-testable in a cloud session, which is exactly the ceiling
`MIND_ARCHITECTURE.md` §4 says most sessions are stuck below.

**Checkpoint scrub** follows for free: `CheckpointInfo{path, loss}` is already the index, so it is
one `||dW||` field per checkpoint on a slider, and the changed-weight glow *grows* along the run.
That is "watching a model learn", literally, and it is the demo.

### B. ✏️ The training strip — now off a first-party API

`/api/train/progress` into the Live Telemetry dock, `/api/train/metrics` for backfill on reconnect,
`/api/train/runs` as a browsable shelf rather than a live-only readout.

⚠️ **Two defects the first draft listed as holes are void on this path.** They were the *callback's*
problems, not the Studio's: unsloth-buddy's `on_log` re-serializes the entire log history to every
SSE subscriber (quadratic in steps), and it carries no working cursor. The Studio's SSE has named
events, `id:` per event, `retry:`, and reads `Last-Event-ID`. And "the run as a recorded object",
which the first draft proposed as a feature to build, **is already built** — runs are persistent,
identified, and queryable after the fact.

### C. 🚨 The Crucible lens — blocked, and this is the honest finding

*The specimen lit by learning rather than by activation* is the feature that would make this more
than a chart beside a render. **Neither surface can supply it.** A keyword sweep of the whole
648 KB spec finds no `per_layer`, no `per_module`, no `gradient_norm`; `grad_norm_history` is one
global scalar per step, exactly as the callback had it.

The only lead is `enable_tensorboard` + `tensorboard_dir` in `TrainingStartRequest` — TensorBoard
event files are a documented format and the natural place per-module scalars would land, but
**something upstream still has to log them**, which is a change in unsloth or a custom callback,
not a thing to read. Do not scope this until that is answered.

⚠️ **It was a tier in the first draft and is demoted here rather than quietly kept.** The
compelling feature being the blocked one is worth stating plainly; the alternative is a tier that
looks schedulable and is not.

### D. ✏️ The Divergence lens — and it splits cleanly on cost

*Run one prompt through base and fine-tuned; render where their internals part company.*

- **The behavioural half is nearly free.** `POST /p/{run}/{checkpoint}/v1/chat/completions`
  answers from any checkpoint of any run with nothing loaded on our side.
- **The internals half is ours.** An OpenAI-compatible endpoint returns tokens and no activations,
  so *where in the stack* needs our own runtime plus `MindFrame` **Block A** (residual trajectory
  + logit lens), already reserved and assigned for exactly this shape.

✅ **The binding question is answered, and the answer is the good one.** `llama-cpp-4` 0.4.2
wraps LoRA in a fully safe API — `LlamaModel::lora_adapter_init(path)` (`model.rs:1952`),
`LlamaContext::lora_adapter_set(&mut adapter, scale: f32)` (`context.rs:1065`),
`lora_adapter_remove` (`context.rs:1099`) — with no `unsafe` at the call site and no sys symbol
re-exported as safe. **So the merged-export fallback is not needed**: load the base once,
`lora_adapter_init` once, toggle. ⚠️ The names originally searched for were a llama.cpp
generation stale — both `llama_set_adapter_lora` and `llama_lora_adapter_set` are absent from the
vendored tree; the live C entry point is the plural `llama_set_adapters_lora` (`llama.h:690`) and
the crate bridges the churn itself. Grepping for the old names finds nothing and yields exactly
the wrong conclusion.

📌 **It composes with the `#522` tap by mechanism, not by luck.** Setting an adapter fails the
graph-reuse test and forces a rebuild, and the rebuild branch **re-installs the eval callback**
(`llama-context.cpp:1314-1315`). The captured tensor names are unchanged as well — `build_lora_mm`
returns its result unnamed and the caller names the block output as before — so
`TensorCapture::for_names` matches identically with and without the adapter and the per-layer
difference is taken on identical keys.

📌 **Scaling is live and better than expected.** `lora_adapter_set` takes an `f32` on top of the
adapter's own `alpha/r`, so `1.0` is "as trained" and a fade is `0.0 → 1.0`; `0.0` is a clean
no-op off-switch. ⚠️ But the scale is baked into the graph as a constant, so *changing* it forces
a rebuild + reserve on the next decode — one rebuild per step. Cheap beside a model reload, not
free. Do not animate it per frame.

🚨 **Two hazards, both confirmed against source, both silent.**

1. **`lora_adapter_remove` ignores its argument and clears every adapter** — the parameter is
   literally `_adapter` and the body passes `(null, 0, null)`. Harmless for a two-way A/B; wrong
   the moment a second adapter exists, and the signature reads as though it is not.
2. **`LlamaLoraAdapter` has no lifetime tying it to its model, so drop order is a use-after-free.**
   `~llama_model` does `for (auto * lora : loras) delete lora;` without nulling anything, while
   `LlamaLoraAdapter::drop` calls `llama_adapter_lora_free`, which dereferences `adapter->model`.
   ⚠️ It bites in a place specific to this runtime: `mind_runtime.rs:428` holds
   `Option<LlamaModel>` and `load_model` at `:696` does `*model = Some(m)`, dropping the previous
   model — so a cached adapter surviving a mid-session model swap corrupts the heap. Same class as
   the `TensorCapture` ordering hazard already documented at `mind_runtime.rs:765-778`, and it
   deserves a comment in the same voice.

⚠️ Two smaller notes. `llama_set_adapters_lora` **always returns 0** — its only failure path is a
`GGML_ASSERT`, i.e. abort — so `LlamaLoraAdapterSetError::ErrorResult` is unreachable and no
error-recovery path should be built around it. And the crate ships **no LoRA test and no prelude
export**: this is present-but-untrodden code, so the first run is the real test.

⚠️ **The adapter must be GGUF for this path** — `lora_adapter_init` hands the path to llama.cpp's
GGUF loader, not to safetensors. 📌 That closes rather than costs: `POST /api/export/export/lora`
takes `gguf` and `gguf_outtype`, so the Studio produces it on request. §4A is unaffected — it
parses `adapter_model.safetensors` itself and never touches llama.cpp — though
`LlamaLoraAdapter::metadata()` (`model.rs:453`) reads the adapter GGUF's whole key/value block and
is a second route to `lora_alpha`/rank if that parser ever proves annoying.

### What NOT to build

⚠️ **A demo builder.** unsloth-buddy generates a static HTML base-vs-fine-tuned page, and the
Studio has its own preview pages. Organon renders that comparison better and natively. Do not grow
a third one.

---

## 5. ✏️ On organon-one

📌 **The WSL constraint the first draft worried about is gone.** The Studio runs natively on
Windows against the 5090, so Organon and the API are the same machine and loopback is the whole
story — `http://127.0.0.1:8888`, never `localhost` (::1-first resolution against an IPv4 listener
costs ~200 ms here, already measured).

⚠️ **LAN access is on.** The listener carries a second explicit bind to `192.168.0.7:8888` beside
loopback — the single LAN address rather than `0.0.0.0`, which is the tighter of the two choices.
Verified reachable from outside the Windows network namespace (5.5 ms from WSL), so it is genuinely
open to the subnet and not a same-box artifact. Auth holds there: `/api/train/status`,
`/api/models/loras` and `/p` all 401 on the LAN address. Unauthenticated to the whole subnet:
`/api/health`, `/api/liveness`, `/openapi.json` and `/docs` — the API's entire *shape*, plus a
`studio_root_id` and a SHA-256 of the desktop owner token. That is a deliberate choice and it is
what makes the Mac and nuc8-01 clients too; recorded because it is the kind of thing that gets
forgotten and rediscovered as a surprise.

⚠️ `organon-tts` holds ~21 GB resident in WSL. A 7B QLoRA run alongside it will not fit.

---

## 6. What this does not claim

- ✏️ **The inference honesty gap is narrower than this document first said, and the correction
  arrived in review.** The draft claimed the real activation tap was *"unconfirmed on any
  machine"*. It was already confirmed when that sentence was written: `1f15568` — an ancestor of
  this branch — records the tap printing `activation tap MEASURED — real per-layer tensors` on
  **2026-08-21, on organon-one, running `gemma-4-12B-it-QAT-Q4_0.gguf`**, with frames carrying
  `FLAG_RESID_MEASURED` + `FLAG_MLP_MEASURED`. So `layer_norm` and `mlp_act` are **measured**;
  what remains a labeled proxy is **`head_summ`** alone, and the confirmation is Windows/CUDA —
  **Metal is still unrun**. 🚨 That is a much narrower and still-true gap, and stating the wide
  version was itself the failure principle 5 warns about, pointed the other way: *understating*
  what is measured is as much a provenance error as overstating it. ⚠️ **Consequence beyond this
  document:** #110 was explicitly gated on that tap reporting MEASURED, and it did — so that gate
  is lifted, and anyone still treating #110 as blocked on it is working from the same stale
  reading.
- **Nothing here closes what is left of it.** A measured *training* signal is still not evidence
  about `head_summ`, and none of these tiers touches it. The tiers do not depend on it either: a
  fine-tune's delta and a trainer's loss curve are measured whatever the attention summary turns
  out to be.
- **The Frobenius norm is not importance**, effective rank is not meaning, and a fine-tune moving
  layer 12 the most does not mean layer 12 holds the skill. Each of those is a contested claim
  under principle 5 and must be marked as one.
- **Nothing here has been run.** No adapter has been parsed, no authenticated route called, no SSE
  stream consumed from Rust, no LoRA attached to a context. What has been *measured* is the
  Studio's route list, schemas, auth behaviour and LAN binding; what has been *read from source* is
  §4D's whole binding answer, at the version in `Cargo.lock`. Reading is not running, and the
  crate's own LoRA path is untested upstream — everything else is reasoning. The first tier's job
  is to make one of these claims false.
