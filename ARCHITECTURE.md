# Organon — Architecture

> **Naming.** Three products, one engine: **Organon** (the visualizer — plugin name in
> the host, window titles, bundle `Organon.vst3`/`.clap`), **Organon Mind** (the
> standalone analysis instrument) and **Organon Console** (the agent-operating
> workstation) — the latter two standalone-only, each with its own name, window title
> and IPC namespace, and no bundle; §4.1 owns the mechanism. Mind and the Console are
> **spin-outs of capabilities that live primarily in Organon**, not peers of it.
> **"Organic Math"** is the *original cube-field generator*
> (`GeneratorMode::Original`) and its algorithm/papers — the seed they all grew from.
> Internal identifiers (crate `organic-math-native`, binaries
> `organic-math-visual`/`-standalone`, `OrganicMathParams`, IPC paths, the
> `~/Library/Application Support/OrganicMath/` store, VST3 class ID / CLAP ID)
> deliberately keep the old name for save/session compatibility.
>
> **What this document is.** The durable, code-grounded technical reference for the
> **native** crate: how the pieces fit, where the seams are, and how to extend it
> without breaking the things that are easy to break. Read this to *build
> intelligently* on what exists.
>
> **How it relates to the other docs:**
> - **`doc/arch/render.md`** — the **render pipeline in depth** (the old §9, split out in
>   organon#590 T3). A *child* of this file, not a peer: §9 here is the altitude version
>   and points there. **Not auto-injected — open it when you work on the renderer.**
> - `MIND_ARCHITECTURE.md` — **Organon Mind's living state** (what exists right now)
>   and its honesty ledger. This file owns everything Mind *reuses*; that one owns what
>   is Mind-specific. **Not auto-injected.**
> - `CONSOLE_ARCHITECTURE.md` — the same, for **Organon Console**. Same split: the engine
>   it reuses is documented here, the compositor and terminal there. **Not
>   auto-injected.**
> - `doc/guide/` + `doc/reference/` — **the user documentation**: how to *operate*
>   Organon rather than extend it. `doc/reference/` is **generated** from the descriptions
>   in `agent.rs`/`recipe.rs` by `organon docs` and pinned by a test, so a new generator
>   or a reworded gloss must be regenerated in the same commit.
> - `CLAUDE.md` — project context, conventions, the toolchain, the build/install
>   workflow. It deliberately does **not** describe architecture; that is this file.
> - `CONTRIBUTING.md` — the *process* above all of these.
> - `CHANGELOG.md` — per-release history, and where the `Shared` layout's accretion story
>   lives (it is a ledger, not architecture — organon#590).
>
> ⚠️ **This is the public repository, and a few pointers below still aim at the private
> upstream.** `web/`, `site/`, `original_code/`, `scripts/`, `STATUS.md` and
> `.claude/skills/organon-dev` are not here — §18 and the export/status notes that mention
> them are inherited text, not a map of this tree. `README.md`'s doc table is the reliable
> index.
>
> **When the architecture changes, update this file in the same change.** A Stop hook
> (`.claude/hooks/architecture-doc-check.sh`) reminds you when `params.rs` / `ipc.rs` /
> `render.rs` / `param_table.rs` move without it — and separately points Mind-only
> changes at `MIND_ARCHITECTURE.md`.
>
> ⚠️ **Don't pin a number here that nobody re-measures.** The sections a hook does not
> watch are the ones that rotted (organon#590): §7 said "~737 params" against 1 372,
> §15 said "206 lib" against ~800, §6 carried a 742-line size ledger whose own header
> was 592 bytes stale. Prefer the command that computes a number, or a pointer to the
> doc that maintains it.

---

## 1. What this document covers

**`native/` is one crate that builds two products** (§4.1):

1. **Organon** — the VST3/CLAP **plugin + standalone**, plus a separate fullscreen
   **visual process**. The default build.
2. **Organon Mind** — a **standalone** instrument for watching a language model think.
   A *build-time edition* of the same crate, not a fork: same algorithm, same shaders,
   same `Shared`, same visual binary. Its living state is `MIND_ARCHITECTURE.md`.

**Everything below is about `native/`.** Three other surfaces exist and are owned
elsewhere:

| Surface | What | Owner doc |
|---|---|---|
| `web/` | ⏸ **PARKED** (#418, 2026-08-04) — the **WebGPU port**: raw WebGPU + React for UI only, running the *same* `math.rs` compiled to WASM. Kept, not deleted; development is Rust-native only | `web/ARCHITECTURE.md` |
| `/src` | the **legacy** React-Three-Fiber app, the original public artifact | §18 (brief) |
| `site/`, `site-mind/` | the static sites (organon.art, organonmind.org) | their own `README`s |

**The legacy `/src` app has diverged** from native — it still runs the pre-#14 algorithm
(loop-step, `angle_inc`, no base grid, a node cap; see §3). `web/` has *not* diverged,
by construction: it compiles `math.rs` rather than re-porting it. Treat the native code
as the source of truth for current behaviour.

---

## 2. Repository layout

```
native/                 THE CRATE — both products, 7 binaries (§4)
  src/                  Rust + WGSL in ONE directory (~85 .rs + 54 .wgsl)
    bin/                visual.rs · ctl.rs (the `organon` CLI) · mind_{writer,runtime}.rs
    overlay/            fonts + pre-rendered formula plates
  tests/                wgsl.rs (naga, offline) · egui_popup_contract.rs
  examples/imgdiff.rs   the frame comparator verify.sh gates on
  verify/               verify.sh's scenes, PR checks, and goldens
  vendor/               egui-wgpu (ported to wgpu 30) · nih_plug_egui
  assets/networks/      the #226 network gallery (installed by deploy.sh)
  organon-wasm/         math.rs → WASM, for web/
  organon-manifest/     param-manifest codegen, for web/
  bundle.sh  deploy.sh  verify.sh  xtask/  Cargo.toml (nih-plug is a GIT dep)

doc/guide/              the USER documentation — hand-written, narrative
doc/reference/          GENERATED by `organon docs`; never hand-edited
doc/arch/               render.md · topology.md (children of this file)
doc/                    Organon Mind's public doc set (PRD, build plan, the essay)
.claude/skills/         organon-cli — driving the running app via the CLI

ARCHITECTURE.md (this file)  ·  MIND_ARCHITECTURE.md  ·  CONSOLE_ARCHITECTURE.md
CLAUDE.md  ·  CONTRIBUTING.md  ·  CHANGELOG.md
```

That is the whole tree. `web/`, `src/`, `site/`, `original_code/`, `scripts/`, `brand/`
and `songs/` appear in older text below (§18 especially) because this file crosses from
the private upstream unmodified; **they are not in this repository**.

---

## 3. The core algorithm (shared by native and `web/`)

The organic motion comes from two things a naïve port gets wrong:

1. **Rotate-then-translate (R·T), not T·R.** Mirrors OpenGL `glRotatef`×3 then
   `glTranslatef`×3: translation happens in the *already-rotated* frame, so a loop
   whose rotation grows with its index sweeps nodes around an arc → spiral/helix.
2. **The accumulating `q`-strand.** A 4th loop that compounds transforms with no
   reset (turtle-style) → tentacles/jellyfish/DNA-helix. This is the integration of
   a moving frame along a path (Frenet–Serret growth).

- **Native — the source of truth:** `native/organon-core/src/math.rs` (`compose_step`,
  `draw_tissue`), pure and unit-tested. (It moved out of `native/src/` with the #626 T3
  crate split; §19.0 owns the crate map.)
- **`web/`:** *the same file*, compiled to WASM via `native/organon-wasm`. Parity is
  structural, not maintained by hand — which is the whole reason it is not re-ported.
- **Legacy `/src`:** `src/math/transform.ts` (`composeStep`) + the `CubeField` loop —
  a separate TypeScript port, and it has **diverged** (below).

**Native-specific divergences** (accreted over PRs #14–#16): loops step by 1
(`loop_step` removed); `rot_mod_{x,y,z}` is the per-axis rotation **speed** the clock
integrates; translation has a **unit base grid** (`tr = index + func(angle)·amp·index
+ mod`, so amp 0 = a clean cube of cubes); no node cap; global speed is split into a
`0..1` dial × a `10^speed_exp` decade.

**Origin mode** (`OriginMode`, Original generator only): the `index` above is the raw
loop index in **Corner** mode (default) — the grid corner sits at the world origin and
each rotating arm/sheet pivots off it (the historical look). In **Centered** mode each
axis's index is re-centred (`index − (count−1)/2`, via `math::origin_offset`) before it
drives rotation, translation-base and scale-growth, so the middle node is the un-rotated
pivot at the origin and the field is point-symmetric about it. Corner ⇒ offset 0 ⇒
byte-identical to the historical layout.

---

## 4. The native crate: one crate → seven binaries

`native/Cargo.toml` builds a single crate into:

| Binary | Kind | What it is |
|---|---|---|
| **plugin** | `cdylib` | the VST3/CLAP plugin (a thin control surface; runs in the host) |
| **`organon-standalone`** | bin | the plugin's editor without a host (sliders) |
| **`organic-math-visual`** | bin | the **renderer** — winit + wgpu fullscreen window. ⚠️ Built from the **`organon-visual`** package since organon#49 T4c-ii (`cargo build -p organon-visual`), not from the root crate. The binary NAME and its built path (`target/release/organic-math-visual`) are unchanged, which is why `spawn_visual()` and the bundlers did not move |
| **`organon`** | bin | #452 — the **CLI command surface** (`src/bin/ctl.rs` owns the **clap** arg layer — per-subcommand `--help`, suggestions, `completions <shell>` tab completion with param-id value completion; brain in `cli.rs`): `status`/`catalog`/`get`/`watch` decode the live `Shared` mmap directly; `set`/`do`/`release`/`generator`/`surface`/`material` append `CliOp` lines the visual drains into the #317 override lane; **`snap`/`record`** (Tier 3 "eyes") ride a request+reply sidecar so the visual reads a frame back to PNG / drives the recorder and hands the path back. ⚠️ **Two namespaces address something other than the world, become no `CtlCmd`, and branch in `main` before the mapping so that staying off `cli.txt` is structural**: **`console …`** (#4 T2 → `CONSOLE_ARCHITECTURE.md`) and **`mind adapter …`** (#147 T3½, writing `ipc::adapter_sidecar_path()` for the Delta lens → `MIND_ARCHITECTURE.md` §2.8.1). For external local agents (Bianca) + terminal use; installed by `deploy.sh` (+ zsh completions) |
| **`organic-math-mind-writer`** | bin | #367 Tier 2 — synthetic activation-ring writer (fake per-token frames, zero inference; the model-free proof) |
| **`organic-math-mind-runtime`** | bin | #367 Tier 2b — the **real** activation-ring writer: an embedded llama.cpp runtime that loads the `.gguf`, runs live inference on a typed prompt, and taps per-token activations into the ring. **`required-features = ["embedded-llm"]`** — the default build never compiles it (no llama.cpp/C++ dep) |
| **`organon-mind`** | bin | #483 Tier 1 — **Organon Mind**, the standalone LLM-analysis instrument: the same editor, Mind-only front-of-house. **`required-features = ["mind-edition"]`** — the default build never compiles it. See §4.1 |
| **`organon-console`** | bin | Console #3 T1 — **Organon Console**, the agent-operating workstation: a winit/wgpu/egui window (`src/console_main.rs`) over the compositor lib in `native/organon-console` (which is nih_plug-free by rule: `cargo tree -p organon-console | grep nih_plug` must stay empty). The bin sits in this crate, like `organon-mind`'s, because the embedded viewport (Console #6) renders `World`. **`required-features = ["console-edition"]`** — the default build never compiles it. ⚠️ **The bin and the package now share a spelling but are not the same thing**: `--bin organon-console` builds from the ROOT package, `-p organon-console` selects the compositor lib and produces no binary. See §4.1 + `CONSOLE_ARCHITECTURE.md` |

`nih-plug` is a **git dependency** (not crates.io) — a remote/Linux session may be
unable to fetch it, in which case the compile gate must be cleared on the Mac.

### 4.1 Editions — one crate, two products (`edition.rs`, #483 Tier 1)

**Organon** (the VST3/CLAP visualizer) and **Organon Mind** (a standalone analysis
instrument for local LLMs — product definition in `doc/organon_mind_prd.md`, living
state in `MIND_ARCHITECTURE.md`) are the *same* codebase with a different
front-of-house, selected at **build time**. This is an **edition, not a fork**: the
algorithm (`math.rs`), every shader, the `Shared` layout, the preset store, and —
critically — the **visual binary** are byte-identical between them.

```
cargo build --release                                      # Organon (default; unchanged)
cargo build --release --features mind-edition --bin organon-mind   # Organon Mind
cargo build --release --features console-edition --bin organon-console # Organon Console
```

`organon-core/src/edition.rs` holds a compile-time `Edition` (`Full` | `Mind` |
`Console`) and the const `EDITION`, selected by the `mind-edition` / `console-edition`
cargo features (**both default OFF**, so `cargo build` / `cargo test` / `bundle.sh` /
`deploy.sh` keep producing exactly today's Organon; enabling both at once is a
`compile_error!`). **The Console** is the third product (Console #3 T1): the
agent-operating workstation, defined in `doc/organon_shell_prd.md` (private annex),
living state in `CONSOLE_ARCHITECTURE.md`; its code is the `native/organon-console`
workspace crate (nih_plug-free, the organon-mind pattern) and its **binary** is
`src/console_main.rs` in **this** crate, `organon-mind`-style — the window renders
`World` (Console #6 T1), which lives here until #618 extracts it.

⚠️ **An edition drives SIX behaviors now, not the original three** — this section
said "three things and nothing else" long after #554 T4/#572 made it six, and
`edition.rs`'s module doc is the authority (branding, IPC namespace, tab set,
instrument-window vs projector-feed, UI-layer start visibility, and the gated
`pub mod world`). The front-of-house three:

| What | Full | Mind | Console |
|---|---|---|---|
| `product_name()` — `Plugin::NAME`, window title, editor heading | `Organon` | `Organon Mind` | `Organon Console` |
| `ipc_namespace()` — the `$TMPDIR` filename prefix (§6) | `organic-math` | `organon-mind` | `organon-shell` ⚠️ |
| `visible_tabs()` / `shows_tab()` / `default_tab()` — the `UiTab` set **and its order** | all 8, its own order | `Mind · Look · Motion · Environment · Settings` | `Look · Motion · Environment · Settings` (provisional — no Console binary draws a tab bar yet) |

⚠️ **`Edition::Console`'s IPC namespace is deliberately still `organon-shell`.** It is a
**wire identifier**, not a name: the `organon` CLI joins on that exact string to find a
running console, and the workstation's launch shims set `ORGANON_IPC_NS=organon-shell`.
The crate, the feature, the binary module and the `Edition` variant all renamed around it
because nothing outside this repo reads *them*. `edition.rs` carries the argument and a
test pins the string.

Every one of those is a pure function of the `Edition` **value**, so every product's
behaviour is unit-tested from a default (feature-off) build — the fork is verified
here, not only on the Mac.

**The IPC namespace fork is the one cross-product invariant.** Every mmap + sidecar in
`ipc.rs` is built by a single `ns_file(suffix)` → `$TMPDIR/<namespace>-<suffix>`, where
the namespace resolves **once per process** (a `OnceLock`) as:

1. `$ORGANON_IPC_NS`, if set and filename-safe (non-empty ASCII alnum / `-` / `_`,
   ≤ 64 chars — anything else is rejected so an env var can't redirect the mmaps out
   of `$TMPDIR`), else
2. `EDITION.ipc_namespace()`.

That env override is how **one** visual binary serves both products: the visual is
compiled once (feature-off, so its own `EDITION` is `Full`) and the editor that spawns
it passes its own namespace in the child environment (`spawn_visual`). A hand-run
inference runtime needs the same: `ORGANON_IPC_NS=organon-mind ./organic-math-mind-runtime`.
Under full Organon the namespace is `organic-math`, i.e. **every path is exactly what
it has always been** — pinned by a unit test.

**Naming a namespace you are not in (#191 T1).** Everything above resolves *this
process's* namespace, which is right while a process talks to its own peers and not
enough the moment one process wants to read **another** namespace's channel — two model
runtimes, base and fine-tune, each writing its own activation ring. `ns_file_checked(ns,
suffix)` composes a path in a caller-named namespace and `mind_ring_path_in(ns)` is the
ring form of it; `mind_ring_path_in(namespace())` equals `mind_ring_path()`, pinned by
test, so the named form generalizes the unnamed one rather than being a second
convention. 🚨 **It returns `None` where the env var falls back**, on purpose: a spawned
visual with a junk namespace must still come up, while a caller that typed a name has
made a mistake and must not be quietly answered about a different ring. One sanitizer
serves both doors.

**Not edition-dependent, ever:** the **VST3 class ID / CLAP ID** (Organon Mind is
standalone-only and needs no plugin identity at all), the `Shared` layout, and the
preset store — a look saved in Organon is a look Organon Mind can pick.

`mind_ui.rs` is the shared Mind-UI module, and it owns every edition-shaped UI decision
as a **pure function of `Edition`** (so each is unit-tested for both products from a
default build):

| Decision | Function | Full | Mind |
|---|---|---|---|
| Which tabs the bar draws, **and in what order** | `tab_bar` / `Edition::visible_tabs` | all 8, its own order | `Mind · Look · Motion · Environment · Settings` (#520 T1) |
| An active-but-hidden tab | `clamp_tab` | never happens | falls back to the default, so the window can't come up blank |
| Auto-point at the specimen on model load | `should_point_at_specimen` | **never** | once per `model_gen` edge |
| The product heading | `heading_text` | `Organon` | `Organon Mind — <tagline>` |

One of those needs care:

- **Auto-pointing exists because the tab filter has a consequence.** The generator +
  topology selectors live on the Generator tab, which Mind doesn't ship — so a loaded
  `.gguf` would light nothing. On the `model_gen` edge the Mind editor sets generator =
  Neural Network and topology = Connectome (loaded) through `ParamSetter` (**GUI thread
  only** — the audio thread can never set params). It fires on the edge, not every
  frame, so a later manual change isn't snatched back, and the Mind card states what it
  did. Full Organon never auto-sets: yanking a chosen generator out from under someone
  would be hostile.

**The Mind tab's card layout is the same in both editions** (#520 Tier 1) — one
arrangement to reason about, no branching. It is a *single* three-column grid (it used
to be three stacked `fixed_columns` blocks, which is why the cards never sat side by
side): col 0 **Neural Network**, col 1 **Model / Specimen**, col 2 **Chat / Agent** then
**Design Space (atlas)**, with the #482 live-telemetry dashboard spanning the width
below. The Neural Network card body is shared with the Generator tab through
`lib.rs::neural_network_card` — one body, two call sites, so they cannot drift.

Organon Mind keeps the **presets rail**: it ships Look / Motion / Environment, which are
exactly the tabs presets capture. (#483 Tier 1 briefly hid it; #520 Tier 1 restored it.)

**Module split by binary:** the plugin `cdylib` compiles `lib.rs`, `params.rs`,
`ipc.rs`, `preset.rs`, `clip.rs`, `recipe.rs` (#452 Layer 3 — the pure recipe library), `keymap.rs`, `audio.rs`, `param_table.rs`,
`synth.rs` (#339 Duo-Field synthesis — the audio-thread DSP: Tier 1 field-probe
microphones + Tier 2 oscillator lattice + Tier 3 modal struck-cavities (damped
two-pole resonators tuned to the eigenmodes) + Tier 4 the-medium-speaks (a
**granular aura** — a probe cloud advected through the field velocity, windowed-sine
grains scheduled by flux; and a **scanned-geometry wavetable** — the shell's
pressure cross-section scanned into a table played at the note pitch), `SynthMode`
selects, all sonifying the live field kernels; its `sn_*` params are audio-only —
read in `process()`, not packed into `Shared`),
`controller.rs` (#356 Four-Quadrant Performance Controller + #448 rotary knob bank —
pure + host-agnostic: a serializable `PadLayout` routes raw pad-surface MIDI into
`ControllerEvent`s, a `KnobLayout`/`KnobConfig` maps a Launch Control XL's 24 encoders
onto params (Explore = context-aware, Performer = hand-assigned pages, pickup soft-
takeover, CC-collision arbitration vs the pads), and a wait-free SPSC `Mailbox` hands
raw MIDI from `process()` (audio thread) to the editor (GUI thread), where the #354
quantized recall and the raw-`GuiContext` knob param-sets — GUI-only paths — run;
default profiles = Novation Launchpad Mini MK3 / Launch Control XL; unit-tested),
`overlay_meta.rs` (pure overlay metadata, shared), `mind_ring.rs` (#367 Tier 2 —
the activation-ring mmap protocol: `MindRing`/`MindFrame` + `MindRingWriter`/
`MindRingReader`, a separate channel from `Shared`), `mind_viz.rs` (#482 Tier 1 —
the Mind-dashboard paint helpers: `MindViz` display state + `paint_*` egui draws for
the "Live Telemetry" widgets, editor-side, reading the mind ring), `audio_ring.rs` (#430 Tier 2 —
the audio-sample ring: `AudioRingWriter`/`AudioRingReader`, the plugin's post-synth
stereo output streamed to the visual recorder, a separate channel from `Shared`). The
visual binary additionally compiles the whole **renderer** (`render.rs`, `post.rs`,
`env.rs`, `terrain.rs`, `ocean.rs`, `stars.rs`, `particles.rs`, `fluid.rs`, `metaball.rs`,
`voxel.rs`, `mandelbulb.rs`, `creature.rs`, `creature_overlay.rs`, `minimal.rs`, `lens.rs`, `splat.rs`, `kifs.rs`, `kaleido.rs`, `rd.rs`, `gi.rs`, `capture.rs`,
`recorder.rs`, `snap.rs` (#452 Tier 3 — single-frame PNG readback of the production texture), `overlay.rs`, `axes.rs`, `chamber.rs`, `fx.rs`, `hdr_macos.rs`, `hdr_windows.rs`, `metal_island.rs`, `gpu_timer.rs`, `rt.rs`, `rt_shadow.rs`, `rt_reflect.rs`, `rt_ao.rs`, `rt_gi.rs`) via
`#[path]` includes — these are **not** part of the plugin dylib. (`splat.rs` + `splat.wgsl` — the
Gaussian Splatting surface — draw the node set as anisotropic Gaussians in the scene pass, reusing
the instance/tint buffers + the shared IBL group, like `particles.rs`.) `math.rs` and
`overlay_meta.rs` are shared (pure, unit-tested). (`chamber.rs` + `chamber.wgsl` — the
#346 Field Chamber analyzer panels — draw in the scene pass right after `axes.rs`, sharing
its camera + depth + back-facing-wall selection.)

---

## 5. The two-process architecture (the big picture)

The defining structural fact: **the plugin and the visual are separate OS
processes**, connected by a one-way shared-memory snapshot.

```
  ┌─────────────────────────────┐         ┌────────────────────────────────┐
  │ PLUGIN  (in the host)       │         │ VISUAL  (organic-math-visual)  │
  │  - nih-plug params          │  mmap   │  - winit + wgpu window         │
  │  - process() on audio thread│ ──────► │  - reads Shared each frame     │
  │  - egui editor on GUI thread│ Shared  │  - OWNS clock/camera/gen state │
  │  - WRITES the IPC snapshot   │ snapshot│  - renders + presents          │
  └─────────────────────────────┘         └────────────────────────────────┘
        ▲   writes Shared each block            │  writes Feedback (fps/res)
        └───────────────────────────────────────┘  (reverse channel, mmap)
```

**Why this design:** a plugin **cannot set its own params from the audio thread**
(nih-plug param setting is GUI-thread only). So MIDI/clip/key-map input that needs to
drive the look bypasses the host param layer and **drives the visual directly via the
IPC snapshot** (§9 + `doc/arch/render.md`, §11). The plugin stays a thin control surface; Ableton
maps/automates/MIDI-learns every parameter natively.

**Launching the visual:** the editor's "Open Visual Window" button spawns
`organic-math-visual`, resolved against the plugin's own dylib path (the visual binary
is **embedded inside the `.vst3`** in `Contents/MacOS/`), with fallbacks to the
`ORGANIC_MATH_VISUAL` env var, a sibling of the current exe, and `PATH`.

`current_dylib_dir()` (`lib.rs`) is that lookup, and it is **per-platform** (#658 T1):
`dladdr` on Unix, `GetModuleHandleExW(…FROM_ADDRESS | …UNCHANGED_REFCOUNT)` +
`GetModuleFileNameW` on Windows (where the visual sits in `Contents/x86_64-win/`
beside the DLL), and `None` on any third platform. All three ask the same question —
each hands the loader an address inside this module and asks which file owns it —
because in a host `current_exe()` is the DAW, not us. Binary names carry
`std::env::consts::EXE_SUFFIX` (`""` off Windows), which matters most for
`mind_runtime_path()`: it filters candidates by `Path::exists()`, so a missing `.exe`
is a silent always-false rather than a fallback.

---

## 6. IPC: the `Shared` snapshot (`ipc.rs`)

The heart of the two-process design.

- **`Shared`** — a `#[repr(C)]` `Pod`/`Zeroable` struct (`ipc.rs`), a flat block of
  `f32`/`u32` arrays, currently **8512 bytes** at `LAYOUT_VERSION` **`0x0285`**.
  > **Read them, don't trust this line** — the two numbers live in **different files**,
  > and that is why one of them rotted here while the other didn't:
  > ```bash
  > grep -n 'EXPECTED_SHARED_SIZE'      native/src/param_table.rs   # → 8512
  > grep -n 'pub const LAYOUT_VERSION'  native/organon-core/src/ipc.rs   # → 0x0285
  > ```
  > Both are golden-pinned, but **in different places**: the size by
  > `shared_layout_is_stable` (`param_table.rs`, alongside the offset table and the
  > default-snapshot hash), the version by a bare `assert_eq!` in `ipc.rs`. This
  > paragraph used to send you to `param_table.rs` for *both* — so anyone verifying it
  > found 8512, correctly left it, and never saw that the version had moved. It sat at
  > `0x0284` while the seqlock note **two paragraphs below** already said
  > `0x0284→0x0285`. **The test suite, not this document, is the authority**: if a
  > number here disagrees with the goldens, the goldens are right.
  The layout grows *only* by appending at the tail (see "Append-only layout discipline"
  below), and its block-by-block accretion history lives in `CHANGELOG.md` and the git
  log, one entry per PR, which is where a ledger belongs. The
  small **`Feedback`** reverse channel (below) also carries the production-frame size
  (`out_w`/`out_h`) so the editor shows the active output resolution, plus (#195 Tier 0,
  appended) **`rt_available`** (1 = the device has ray-query support; the editor greys
  the RT card out without it) and **`tlas_ms`** (the smoothed per-frame TLAS rebuild
  cost while RT is on), plus (#277 Tier 2, appended) **`instances`** (nodes drawn this
  frame) and **`cpu_ms`** (smoothed CPU encode+submit cost of a frame), and (#277 Tier 3,
  appended) **`gpu_ms`** + **`gpu_timing_available`** (the frame's true GPU time from
  wgpu timestamp queries — read back a frame late, so no CPU stall — and whether the
  device offers `TIMESTAMP_QUERY`) — the workload telemetry the editor's **performance
  status bar** reads into per-stat headroom meters + GPU/CPU hero meters.
  **Variable-length
  overlay strings** (handle / title override) ride an `overlay` sidecar + the `overlay_gen`
  counter, exactly like `.hdr` + `hdr_gen`.
- **Transport:** a memory-mapped file at `$TMPDIR/<namespace>-ipc.bin` — built by
  `ns_file("ipc.bin")`, so it is `organic-math-ipc.bin` under Organon and
  `organon-mind-ipc.bin` under the Mind edition (§4.1 owns the namespace rule).
  `Writer` (plugin) calls `write(Shared)`; `Reader` (visual) calls `read()`.
  **The two are a seqlock** (#618 Tier 0a, `LAYOUT_VERSION` 0x0284→0x0285):
  the writer stamps `seq` **odd**, copies the body, then stamps `seq` **even**;
  the reader samples `seq`, reads the body, samples `seq` again, and accepts only
  on the same even value, retrying otherwise. The bulk copy therefore starts at
  byte 8, past `seq`+`layout_version` — copying the whole struct in one go would
  publish the committed counter *before* the body, which is exactly backwards.
  > ⚠️ **This paragraph used to say the `layout_version` check WAS the torn-read
  > guard. It never was.** That check catches a mismatched *build*; it cannot catch
  > a *tear*, because both halves of a torn record carry the same version, so it
  > passes on a blend of two snapshots. `seq` was written every block and no reader
  > ever read it. The old defence — "torn reads of a single float are control-rate
  > and visually irrelevant" — held for independent scalar dials and failed for
  > anything coupled (a mode selector read a page ahead of its own params, a
  > `*_gen` counter seen before the sidecar it announces). It was not theoretical:
  > `a_concurrent_reader_never_observes_a_blend` reproduces a real tear against the
  > pre-Tier-0a writer in well under a second.
  The `layout_version` check remains, unchanged, doing the job it always did.
- **Composing two snapshots: `overlay_changed(dst, base, mine)`** (Console #7). Copies into
  `dst` every lane in which `mine` disagrees with `base`, and nothing else. It exists for a
  front-end that owns only *part* of a look — Organon Console drawing one of the editor's
  panels — and has to put that part on top of a snapshot somebody else composed. `base` is what
  the caller's values were before anyone touched them, so the difference between it and `mine`
  **is** the set of lanes the caller has an opinion about; no hand-written lane manifest exists
  to fall out of date when a param is added.
  🚨 **`base == mine` writes nothing**, so a caller that has changed nothing is byte-inert over
  any `dst` whatsoever — invariant #4 made structural rather than checked.
  ⚠️ **Lane granularity, never byte granularity.** A changed `f32` differs in one to four of its
  bytes, and copying only the differing ones would splice two floats into a value neither side
  held. Every `Shared` field is a `u32`, an `f32` or an array of them, and the struct is `Pod`,
  so `bytemuck` hands the whole thing over as `[u32]` and a 4-byte word *is* a lane —
  `shared_is_a_whole_number_of_lanes` fails rather than corrupting one if a field of another
  width is ever added. `seq` and `layout_version` are lanes like any other and are safe by
  construction: both are equal on any two snapshots from the same build, so neither can ever be
  in the differing set, and `Writer::write` stamps `seq` afterwards regardless.
- **Two runtime-written blocks** are stamped by the plugin's `process()` each block,
  not by params: `transport[4]` (host playing/beat-pos/tempo) and `audio[8]` (live
  band envelopes; `audio[5]` = the smoothed broadband RMS level, #248 Tier 1 — the
  loudness envelope that drives the audio-dipole). Plus `hdr_gen` (a counter; see below).
  (The metering blocks `audiometer[16]`/`audiospectrum[128]`, the `voices[64]` radiators, and
  the #346 `scopewave[260]` oscilloscope frame are likewise runtime-written by `process()`, not
  params — `scopewave` from `audio::ScopeRing` via `audio::scope_frame_into`.)
- **Sidecar files:** `$TMPDIR/organic-math-hdr.txt` carries a chosen `.hdr` path
  (the GUI writes it, then bumps `hdr_gen` in `Shared`; the visual edge-detects
  `hdr_gen` and re-runs the IBL precompute). `$TMPDIR/organic-math-connectome.txt`
  carries a chosen network-JSON path (#226 Tier 3/4/5; the GUI bumps `nn_gen`, the
  visual edge-detects it and ingests an **attention tensor** via `neural_attention_from_json`
  (an `attention` key — checked first, its schema also carries `layers`), a trained **MLP**
  via `neural_mlp_from_json` (a `weights`/`layers` key), or else a **connectome** via
  `neural_graph_from_json`). The "Load Network (JSON)…" dialog opens at the installed
  **network gallery** (`preset::networks_dir()` = `~/Library/Application Support/OrganicMath/networks/`).
  `$TMPDIR/organic-math-field.txt` carries a **Field Engine program** (#381 Tier 1) — the
  expression TEXT itself, not a path (`charge(a,0,0,0)`, `a*(x+i*y)*exp(-0.5*r)`, …); the GUI
  writes it + bumps `field_gen`, and the visual edge-detects it and recompiles via
  `math::FieldProgram::compile` (used only when `FieldPreset` = Custom; the gallery presets are
  built-in source, no sidecar). `$TMPDIR/organic-math-fieldclip.txt` carries a chosen **Field
  Playback `.bin` clip path** (#407 Tier A; the Field Engine card's "Load Field Clip…" button writes
  it + bumps `fieldclip_gen`, the visual edge-detects it and (re)loads `math::FieldClip::from_bytes` —
  used only when the PDE preset = Playback). `$TMPDIR/organic-math-nca.txt` carries a chosen **Neural CA
  weights-JSON path** (#407 Tier B; the Field Engine card's "Load NCA Model (JSON)…" button writes it +
  bumps `nca_gen`, the visual edge-detects it and (re)loads `math::NcaWeights::from_json`, falling back to
  `builtin_default()` — used only when the PDE preset = NeuralCa). `$TMPDIR/organic-math-model.txt` carries a chosen
  **`.gguf` model path** (#367 Tier 1 the visible-mind specimen; the Mind tab's
  "Model / Specimen" card writes it + bumps `model_gen` in `mind[1]`, the visual
  edge-detects it and parses the GGUF **header only** via `gguf::parse_file` — no
  weights — then builds the architecture topology via `math::gguf_architecture_graph`,
  feeding the same `neural_loaded` slot the connectome path fills. **Preset-captured** —
  `PresetValues::model_path` rides the Generator bucket and is restored by re-driving
  this sidecar + `model_gen`, exactly as `hdr_path` is restored through
  `hdr_sidecar_path` + `hdr_gen`; see the preset section below). `$TMPDIR/organic-math-mind-prompt.txt`
  (#367 Tier 2b) carries the typed prompt the Mind card's "Generate" writes for the embedded runtime
  (read when `mind[3]` `prompt_gen` changes); `$TMPDIR/organic-math-mind-reply.txt` is the reverse —
  the runtime appends the streaming decoded reply per token, and the editor polls it for its readout.
  `$TMPDIR/organic-math-feedback.bin` is a
  small **reverse channel** (`Feedback`: render scale, width/height, fps) the visual
  writes and the editor reads to show live resolution/FPS.
  `$TMPDIR/organic-math-mind.bin` (#367 Tier 2 — `mind_ring.rs`) is the **activation ring**: a
  SEPARATE mmap channel from `Shared` (so Tier 2's model-free slice adds **no `Shared` size/LAYOUT_VERSION
  change**), single-writer / single-reader, modeled on `FeedbackWriter`/`FeedbackReader`. A running
  model (`MindRingWriter`) publishes per-token `MindFrame`s (`token_index`, `n_layers`/`n_heads`,
  per-layer `layer_norm`/`mlp_act`, per-(layer,head) `head_summ`) into a 4-slot ring with a monotonic
  `write_seq` + per-frame `seq`/`signature` torn-read guard; the visual's `MindRingReader::latest()` takes
  the newest committed slot each frame. The writer today is the synthetic **`organic-math-mind-writer`**
  bin (fake smooth "thought" frames, ~20/s — zero inference); the **`organic-math-mind-runtime`** bin
  (#367 Tier 2b, `--features embedded-llm`) is the **real** writer — embedded llama.cpp inference on the
  loaded `.gguf`, one `MindFrame` per generated token (activation tap = an honest logit-entropy/confidence
  proxy — see the `mind[3..8]` note above). Reader side: the `topo == 5` Live-streaming seam above.
  **Phase B — the three-way append (done, spine step).** `MindFrame` grew three blocks **in one
  coordinated change, before any of them was implemented**: **A** #507 T2/T3 (`resid_layers`, `lens_k`,
  `resid_proj[64·3]`, `lens_id`/`lens_prob[64·4]` — the residual trajectory projected through the SAME
  basis as the Tier-1 galaxy, plus the per-layer logit lens); **B** #505 T2 (`expert_count`,
  `expert_used`, `expert_id`/`expert_w[64·8]` — live MoE routing, stored **sparse** as fired-(id,weight)
  pairs because models declare 8–256 experts but fire 2–8 per token); **C** #409 T2 (`feat_count`,
  `feat_layer`, `feat_recon_err`, `feat_id`/`feat_act[32]` — SAE features; **ids only**, names resolved
  editor-side from the versioned feature-label corpus). Frame 17 264 → **24 464** bytes; still not
  `Shared`, so **no LAYOUT_VERSION change**. Every block is **zero = absent**, so each can be implemented
  independently, in any order, and a writer that fills none of them behaves exactly as before.
  ⚠️ **Why the coordination matters, and the two guards.** Writer and readers are separate binaries
  indexing one mmap by byte offset, so a disagreement about layout does **not** fail — it compiles, runs,
  and displays wrong numbers. So: (1) every `MindFrame` offset is **pinned by test**
  (`frame_field_offsets_are_pinned`), which turns "inserted instead of appended" into a build failure —
  if it fires, move the field to the tail, never edit the expected numbers; and (2) `MindRing.frame_bytes`
  (the former spare `_pad`) records the writer's `size_of::<MindFrame>()` and the reader **refuses** a ring
  that disagrees, so a stale writer beside a fresh reader yields *no signal* rather than plausible garbage.
- `$TMPDIR/organic-math-audio.bin` (#430 Tier 2 — `audio_ring.rs`) is the **audio-sample ring**: a
  SEPARATE mmap channel from `Shared` (a continuous high-rate stream, not a control-rate snapshot),
  single-writer / single-reader. The plugin's `process()` streams its **post-synth** stereo output
  (passthrough + synth) into a flat circular `f32` buffer (2^19 frames) with a monotonic `write_count`
  header (`AudioRingWriter::push_frame`, two mmap stores/sample — audio output byte-identical); the
  visual's recorder (`AudioRingReader`) `reset_to_now()`s at record start and `drain`s new frames each
  frame, muxing them into the recording. Overrun-safe (skips the lost gap if the reader ever falls a
  full lap behind). Always-on while the plugin processes audio; inert if no plugin is loaded.
- `$TMPDIR/organic-math-glyphs.bin` (organon#217 T1 — `organon-core/src/glyph_ring.rs`,
  `ipc::glyph_ring_path` / `glyph_ring_path_in`) is the **glyph ring**: a terminal-shaped cell
  grid from a text-effect producer to the world, which renders every non-empty cell as an
  instanced, bevelled, **emissive** tile (`doc/pbr_text_engine.md`). A SEPARATE mmap channel on
  the two precedents above — up to a megabyte at the effect's own cadence is neither control-rate
  nor small — so **no `Shared` field and no `LAYOUT_VERSION` move**. Writer: the `organon-glyphs`
  member (links `ttfx`, ticks an effect under a virtual clock, walks `arena` into cells,
  publishes; holds the settled text for a dwell, then the next effect). Reader: `world.rs`'s
  `glyph_grid_geometry`, which replaces the generator's instances with the grid's tiles while
  the ring is live and hands the frame back three seconds after a producer goes quiet. **Double
  buffer with a lap guard**, not a slot ring: two slots, the writer fills the one the reader is
  not on, the reader re-reads `write_seq` after its copy and retries if it advanced by two. Per
  cell: symbol, fg/bg (sRGB8 — decoded to linear only at the consumer, §4), SGR bits, `layer`,
  `character_id`, an `active_path` bit (the slide-vs-cut signal — `lower_grid` interpolates
  `previous → current` only when it is set), and the **sub-cell offset pair** `sub_x`/`sub_y`
  (§7 of the design: `Motion.current_pos − current_coord`, the remainder ttfx's rounding
  dropped, in cells, `+y` up on both sides of the ring — reserved at T1, filled by W6 once
  ttfx carried the pre-rounded point; `lower_grid` slides between the two *exact* positions,
  and a producer that writes zeros lowers exactly as before), and a **`persist` bit** (T11:
  the cell is a phosphor trail — the last lit cell, symbol and all, with its colour decayed by
  the producer *in linear light* and re-encoded to sRGB8, so the colour contract is unchanged
  and the header carries no τ; `--persist-ms`, default 0 = off, byte-identical; a trail is
  never a slide's origin, and `FRAME_SETTLED` is the *source's* — trails decay on through the
  dwell and `generation` moves with them until they cross the floor). Header:
  layout version + cell stride (the reader refuses a disagreeing writer — the `mind_ring`
  `frame_bytes` lesson), the **cell aspect** (ttfx is 2:1; square tiles make ellipses of every
  ring the effects draw), and the producer's tick rate (the interpolation window). ⚠️ **Rows are
  stored top-down**; ttfx numbers them from the bottom, the producer flips once, and the flip is
  pinned on an asymmetric fixture. `generation` bumps only when the cell payload changes (a dwell
  heartbeat keeps it), which is the counter T5 added to the path tracer's content key —
  `world.rs::pt_content_key` carries the ring's `(live, generation)`, so accumulation restarts
  when the glyphs move and accumulates through the dwell, and `pathtrace_active` (the preset's
  toggle OR a live `FRAME_SETTLED` frame) is the raster → path-trace handover. With no ring
  both reduce to what they were before T5. `doc/arch/render.md`'s "Converge on hold" owns it.

### Append-only layout discipline (critical)

Every new feature **appends** its block to the end of `Shared`; existing field offsets
never move. This keeps a running plugin/visual pair compatible across a rebuild of one
side. **If the layout ever must change incompatibly, bump `ipc::LAYOUT_VERSION`** and rebuild
both binaries together. After any layout growth, **close and reopen the visual window**
(and Rescan in Ableton).

---

## 7. Parameters (`params.rs`)

> ⚠️ **Fourteen enum params are SPLIT across two crates, and the split has a rule.**
> `FuncName` (#626 T3); `GeneratorMode`, `BoidsForm`, `OscDivision` (organon#49 T1);
> `SurfaceMode`, `MaterialType`, `CamPath`, `Palette` (organon#49 T2); and `FdtdSource`,
> `FieldVolSource`, `ColourMode`, `CalColourSource`, `FieldKind`, `FluxAxis`
> (organon#49 T4a) are declared
> **plain** in `organon-core::params` and mirrored in `params.rs` as `Host<Name>`, which
> carries nih-plug's `#[derive(Enum)]`. The **orphan rule** makes this unavoidable rather
> than merely preferable: `organic-math-native` cannot
> `impl nih_plug::Enum for organon_core::…`, because both the trait and the type are
> foreign to it. Core owns the semantic type; the host owns the adapter.
>
> **Which one do I name?** The semantic type, unless you are touching an `EnumParam` —
> declaring it, `EnumParam::new`, `setter.set_parameter`, or anything else that wants
> nih-plug's trait. Those are the adapter's; everything else is core's, and `params.rs`
> re-exports the semantic names so `crate::params::GeneratorMode` still resolves.
> `Host*::core()` converts, through the shared index.
>
> **To list, name or index a semantic enum, use `organon_core::params::IndexedEnum`**
> (organon#49 T2) — `all()` / `label()` / `labels()` / `index()` / `from_index()`. It is
> core's counterpart to nih-plug's `Enum`, and it exists because listing an enum's
> variants was never a plugin-host concern. Its method names deliberately differ from the
> inherent `as_str`/`to_u32`/`from_u32`, since same-named trait methods would be silently
> shadowed by the inherent ones.
>
> **Each pair is pinned by a test** (`host_*_mirrors_core`) that compares the two lists
> **element-wise by name, in both directions**. A length check would pass a same-length
> *reordering* — and the index **is** the wire format, shared by `Shared`, presets and
> automation lanes, so a reorder silently recalls the wrong generator rather than failing
> loudly. Add a variant to **both**, at the tail.
>
> 📌 **After T4a, nothing `world.rs` names in `crate::params` requires nih-plug.** That
> was the point of the third wave: `world.rs` pulled 26 references from `params.rs`, all
> of them *value* types, and the six above were the ones not yet in core. The blocker to
> moving `World` below the plugin crate is now the modules it imports, not the params.
>
> 📌 **`cli.rs` and `agent.rs` are nih-plug-free outside their test blocks, and a test
> keeps them that way** (`cli_and_agent_are_free_of_nih_plug_outside_tests`). That is not
> tidiness: both sit on `world.rs`'s dependency path — `world.rs` imports `agent`, and
> `console_main.rs` imports both — so they must travel to a lower crate when §19's Tier 4
> moves `World`. `cli.rs`'s test block is exempt on purpose; it walks the plugin's own
> `Params` tree, which is host-side by nature, and test code does not travel.

- **`OrganicMathParams`** — a nih-plug `#[derive(Params)]` struct, **1 372**
  host-mappable `#[id]` params. Each is automatable, MIDI-learnable, and
  preset-captured. Read at control rate (once per process block).
  > **Counting them:** `grep -c '#\[id = ' native/src/params.rs`. Don't hand-maintain
  > this number — it moves with most feature PRs, and a stale count here is what §7
  > carried for months (it said ~737).
- **Enum params (102 distinct enum types)** drive the discrete choices, e.g. `FuncName`
  (Sin/Cos/Tan/Log/
  Triangle/Square/Saw), `GeneratorMode` (27 generators — 17 = `None`, 18 = `AxonWaveguide`, 19 = `NeuralField`, 20 = `NeuralNetwork`, 21 = `Lens`, 22 = `Demo`, 23 = `Acoustic`, 24 = `FieldEngine`, 25 = `MapAttractor`, 26 = `Creature`), `SceneryMode`
  (None/Zone/Terra) + `ScenerySurface` (cubes/rods/tubes/skin — the #187 pivot + #206 T1/T2), `SurfaceMode`
  (Original/FlowAligned/SweptTubes/Metaball/Membrane/Voxel/Volume/**NeuralTissue** — the #260 anatomical surface: T1 primitives, T2 neuron morphology, T3 myelinated axons, T4 living synapse + tissue context/**Splat** — the Gaussian Splatting surface: the node set as anisotropic 3-D Gaussians via `splat.rs`+`splat.wgsl`, Tier 1 additive + Tier 2 IBL-lit 2DGS + Tier 3 relightable materials (Chrome/Glass) & jittered scatter, `SplatMode` enum/**Plexus** — ordinal 9: wires each node to its nearest neighbours with thin struts + a per-node marker, a breathing "field web". Generator-agnostic (it post-processes whatever node cloud was emitted), so it is a no-op on the raymarch generators, which emit no nodes),
  `OriginMode` (Corner/Centered — the Original cube-field's origin: grid corner at the world origin vs
  grid symmetric about it; Original generator only), `RenderStyle`
  (#152 NPR: None/Toon/Outline/Halftone/Dither/Pixelate), `MaterialType`
  (Standard/Chrome/Glass/Refractive/Anisotropic/Clearcoat/Velvet/Subsurface), `AoSource` + `RtDebugView` (#195 hardware RT),
  `Palette` (Native + 12 IQ cosine gradients), `ToneMap`,
  `CamPath`, `ModTarget` (pulse-routing destinations — **append-only**, indices are
  wire-stable), the #307 camera enums (`TempoSource`, `CamOrder`, `CamTransition`,
  `BarPeriod`, `DollyWave`), `OscDivision` (the Maxwell dipole's tempo-synced
  oscillation period — 1/16…2-Bar), `Msaa`, `ParticleTier`, plus the KIFS family (`KifsSpace`,
  `KifsView`, `KifsPattern`, `KifsPalette`) and per-generator enums
  (`AttractorField`, `DnaForm`, `LSystem`, `PhylSurface`, `TilingFamily`,
  `TilingConstruct`, `TessView`, `TessHeightMode`, `MinimalFamily`,
  `VecFieldPreset`/`VecMagMap`/`VecTint`/`VecFieldView`/`VecSeedMode`/`VecLineColor`/`VecTermFunc`/`VecFieldOp` (#173),
  `RailCellLen` (#187 — musical morph-cell lengths),
  `TerrainNoise/Res/Palette`, `MembraneWeave`, `MembraneArmBuild`, `RippleGeom`, `PulseSource`).
- **`to_shared()`** packs the live params into a `Shared` snapshot. It is called every
  process block.

### `param_table.rs` — the single source of truth for packing (issue #103)

Previously every param's `[f32; N]` slot was hand-written **twice** (in `params.rs`'s
`to_shared` and `preset.rs`'s `to_shared`), an indexed scheme where a wrong slot
silently corrupted everything the visual read. PRs #108–#112 fixed this:

- **`param_block!`** (`param_table.rs`) — one ordered slot list per `Shared` array
  block generates **both** packers (`OrganicMathParams → Shared` and
  `PresetValues → Shared`). Slot kinds: `(f32|i32|bool|enum, ident)`, `_` (reserved
  0.0), `(lit, value)` (fixed literal), `(expr, |binder| …)` (computed slot, e.g.
  `rot_mod[3] = inc_scale·10^speed_exp`). The two packers **can no longer drift**, and
  a renamed field is a **compile error, not a silent zero**.
- `params.rs::to_shared` and `preset.rs::to_shared` now call `param_table::pack_*`
  (~39 / ~37 calls). The only inline arrays left are the runtime-written `transport`
  and `audio`.
- **Safety-net tests** (in `param_table.rs`) — the contract every future migration
  rides behind:
  - `shared_layout_is_stable` — pins `size_of::<Shared>()` + key offsets. **The
    number lives in `param_table.rs::EXPECTED_SHARED_SIZE`, not here** — this line
    carried "2016" against an actual 8512 for months, which §6 contradicted 160 lines
    earlier. Read it with `grep EXPECTED_SHARED_SIZE native/src/param_table.rs`.
  - `default_shared_snapshot_is_stable` — hashes the whole `Default → Shared` byte
    image (proves byte-identical refactors).
  - `bell_packing_is_byte_identical`, `preset_json_round_trips`,
    `captured_params_survive_the_preset_mirror` (a param added to `params` but
    forgotten in `PresetValues` now fails CI).

**Not generated (by design):** the nih-plug field, the `PresetValues` field,
`capture`, and `apply` stay hand-written — `capture` is already a struct literal (a
missing field is a build error), and `apply` can't derive from the packing table
(computed slots) and has no offline test. The drift gap they leave is covered by the
test above. The editor slider (`lib.rs`) and the visual's read are also hand-written.

---

## 8. The generator system (`math.rs`)

The pluggable stage. **A generator emits geometry; everything downstream
(surface/material/light/post/camera/beat) is generator-agnostic.**

### The contract

```rust
struct Frame { position: Vec3, tangent: Option<Vec3>, normal: Option<Vec3>,
               scale: Vec3, tint: Vec4 }     // one oriented sample
type Strand = Vec<Frame>;                     // an ordered polyline
enum Topology { Grid, Streamlines, Tree }     // how strands relate
```

- **`lower_strands(&[Strand], …) → instances + tints`** turns strands into renderer
  primitives: a per-frame model matrix (an instanced cube, or an oriented rod for
  flow-aligned/swept-tube modes) + a per-instance colour tint.
- **`loft_membrane` / `strands_to_mem`** skin **Grid** generators into a continuous
  membrane mesh (bell sheet, DNA ribbon, Frenet ribbon). Streamlines/Tree degrade to
  swept tubes. Two membrane options ride the `membrane`/`membrane_fx` blocks: **Close
  Seam** (`membrane[3]`) auto-bridges a woven line whose end→start gap ≈ its neighbour
  gap (a genuine 360° wrap), closing seams without fusing open lines; **Skin Arms**
  (`membrane[2]`) skips the shell and skins each strand as its own closed capped finger
  with gaps between arms (the volume-render hull), built as a welded **Mesh** or capsule
  **Impostors** (`membrane_fx[1]`). It is a **generator-agnostic surface feature**: Mesh folds
  into the shared `weld` flag (every generator's swept-tube path builds the fingers), Impostor
  drives off the universal `gen_strands`, each generator's shell loft is guarded on
  `!membrane_arms`, and one post-match block suppresses the shell + builds the capsules — so it
  works for Original, Maxwell, Acoustic, Polarization, DNA, Frenet, Harmonic, Phyllotaxis,
  VectorField, and the strand modes of MinimalSurface/NeuralField. The Original cube-field is
  pv-based (no `gen_strands`), so `math::cube_field_strands` produces its node strands. The
  Impostor build is a per-arm-segment **capsule sphere-impostor** pass in
  `particles.rs`/`particles.wgsl` (`vs_capsule`/`fs_capsule` reuse the bead `DrawU` + IBL +
  `shade_bead`, tracing an analytic `sd_capsule` with the strand's tint — no per-frame mesh);
  the visual lowers `gen_strands` to `ArmInstance` capsules (`build_arm_caps`).
- A `trait Generator { fn generate(&self, out) -> Topology }` exists but is
  **vestigial** — real dispatch is a `match GeneratorMode` in the visual (§10), each
  arm calling a free `*_strands` function in `math.rs` then `lower_strands`.

### The 26 generators

| id | `GeneratorMode` | Topology | Notes |
|---|---|---|---|
| 0 | **Original** (cube field) | Grid | the Part I machine (`draw_tissue`) |
| 1 | **Frenet–Serret** | Grid | κ/τ → helices; `frenet_strands` |
| 2 | **DNA double helix** | Grid | L = T + W supercoiling |
| 3 | **Strange attractor** | Streamlines | Lorenz/Aizawa/Thomas/Halvorsen, RK4 |
| 4 | **Spherical harmonics** | Grid | the pulsing bell |
| 5 | **L-system** | Tree | fern/bush/tree/seaweed |
| 6 | **Curl-noise flow** | Streamlines | divergence-free ink/smoke |
| 7 | **Circular polarization** | Grid | E/B helix fan |
| 8 | **Maxwell field** | Grid/Streamlines | real charge/dipole fields, retarded time. The dipole oscillation `cos(ωt−kr)` runs on the free-running global-Speed clock by default; the **Osc Tempo Sync** toggle (`maxwell[22]`) instead phase-locks it to the PLL beat clock as an LFO — one full field there-and-back per **Osc Division** (`maxwell[23]`, `OscDivision`: 1/16…2-Bar), grid-locked while the host plays, else the Manual/Audio BPM (`bin/visual.rs::maxwell_osc_phase`). Sync is applied **centrally where the shared `maxdip_phase` clock is advanced**, so the field lines, the aura/energy cloud, and — on force-drive — the **B swirl** all read one clock (see the **E↔B lock** below). Uses the two spare `maxwell[22..24]` slots → no IPC size/LAYOUT_VERSION change. **E↔B lock:** when Tempo Sync is on, the fluid force-drive reverses the B swirl on `cos(maxdip_phase)` — the E field's own temporal factor — instead of the arbitrary wall-time `stir rate`, so B flips *with* the E wave (the far-field radiation E∥B relationship; `dipole_radiation_e_and_b_are_in_phase` test). Sync-off keeps the turbine/dynamo/manual swirl engines. **E↔B phase dial** (`mx_eb[0]`, degrees, tail-appended after `splat2`; LAYOUT_VERSION 0x0269→0x026A): offsets the locked swirl vs the E clock — `osc = cos(maxdip_phase − φ)` — from **0° = far-field** (in phase, the lock default) to **90° = near-field induction** (quadrature: the swirl peaks at E's zero-crossing, `∂B/∂t ∝ ∇×E` near the source). 0 = byte-identical to the plain lock |
| 9 | **Phyllotaxis** | Grid | golden-angle disk/cone/sphere/shell |
| 10 | **Mandelbulb** | — (raymarch) | **no nodes** — sibling render path |
| 11 | **Kaleidoscopic Fractal (KIFS)** | — (fullscreen) | **no nodes** — own pass; 9 spaces, 5 views |
| 12 | **Boids (flocking)** | Streamlines | **stateful** (the first) — Reynolds rules; trails = strands; PR #105 |
| 13 | **Tessellation (tilings)** | Streamlines / mesh | aperiodic tilings as geometry; families: Penrose P3, **Ammann–Beenker**, **Pinwheel**, **Truchet**, **Hyperbolic {p,q}** (inflation **or** de Bruijn cut-and-project); `view` = edges→rods / filled / extruded prisms / **3-D icosahedral quasicrystal** (Z⁶ rod lattice); **phason flips**, **Ammann bars**, beat inflation-breathe + per-tile ripple; #121 (Hat/Spectre einstein still pending) |
| 14 | **Minimal surfaces** | raymarch **or** Grid | **dual-path by Family** — implicit isosurfaces raymarch (no nodes): TPMS (Gyroid / Schwarz P / D, H ≈ 0) + **Bubbles** (merged soap spheres) + **Foam** (Voronoi Plateau walls, intrinsic thin-film) + the **algebraic bank** (Clebsch / Barth / Kummer / Heart / Tanglecube polynomials); parametric families are a (u,v) Grid skinned by the membrane loft — **Weierstrass** (Enneper / Catenoid / Helicoid, H = 0) + **CMC** surfaces of revolution (**Unduloid / Nodoid**, H = const, Delaunay meridian RK4-integrated). All raymarched families support the **Material** selector (Standard / Chrome / Glass, env-only). #127 P1–P4 (P4a algebraic bank, P4b CMC) — **complete** |
| 15 | **Synchrotron radiation** | Streamlines | Liénard–Wiechert field of relativistic charge(s) orbiting a circle, solved at the **retarded time** of the *moving* source (Newton iteration, `g'(t')=κ>0`; cf. Maxwell's fixed-source retarded *phase*); velocity (1/R²) + relativistically beamed radiation (1/R) terms. Three **views** (`SyncView`): **field arrows** on a plane (`synchrotron_strands`, P1), traced **E field lines** (`synchrotron_lines_strands`, RK4 streamlines seeded around the orbit, P3), **or a field volume** (`synchrotron_volume_strands`, the arrow plane extruded into a `grid²·layers` box, P4). Two volume-legibility toggles (P5): **reveal** (cull arrows below an \|E\| threshold → the dead crust dissolves) + **invert** (sphere-invert display positions → the box turned inside-out). **3-D orbit motion** (P6): the charge's circle can **tilt** + **precess** (`synchrotron_orbit` returns analytic pos/β/β̇ of a precessing plane), so the field tumbles through 3-D instead of one plane; the precession rate is capped to keep the source sub-luminal + the retarded solve uses an adaptive iteration count. #150 P1–P6 |
| 16 | **Vector field** | Streamlines | the maths-Instagram vector-field plot, in 3-D (#173): a curated **function bank** F(x, y, z) (the reel's `(y², −x²)` + `(sin y, sin x)`, rotation, source, dipole, saddle, ABC/Beltrami, Lorenz-as-field, helix, double-well, Taylor–Green, vortex pair) with three **views** (`VecFieldView`): **arrows** (T1 — a `grid³` lattice over ±`extent`, one oriented rod per sample, `vecfield_strands`; one axis at 1 = the literal 2-D plot; `mag_map` soft/log/uniform length; \|F\|-ramp or direction tint; `reveal` culls weak field), **field lines** (T2 — `vecfield_lines_strands`: RK4 streamlines of the same field from a seed set (`VecSeedMode`: lattice / random / ring / plane / \|F\|-weighted), **bidirectional** tracing joined through each seed (saddle topology), \|F\| or sweep-along-line colour, and a **flow pulse** marching brightness/thickness downstream off the clock), **both** (faint arrows under the lines), or a **stream surface** (`vecfield_surface_strands`: equal-length lines traced from an ordered seed curve — Ring = a closed drum, else a straight curtain — returned as **Grid** topology, so **Membrane lofts a flowing sheet** through the field; lines that die early hold their last point, pinching the sheet edge instead of tearing the loft). `evolve` rigidly rotates the field domain; `z_lift` extends the planar classics into 3-D (Fz += k·sin z). **Custom** (T3, bank entry 12 — the **function builder**, `Shared.vecbuild[64]`): each component of F = 3 terms of `gain·func(a·x + b·y + c·z + phase)` (`VecTermFunc`: Const/Linear/Square/Cube/Abs/Sin/Cos/Gauss/soft-Inverse — every knob a host param, so the *function itself* is automatable), then an optional **field operator** (`VecFieldOp`): gradient ∇φ (curl-free), curl ∇×A (divergence-free), or a Helmholtz blend — central differences on `VecBuildSpec::eval`. Builder defaults = the flagship field. Free-text expression entry (sidecar) is the deferred follow-up — see #173 |
| 17 | **None (off)** | — | the primary generator switched off (#187 pivot): the **Scenery layer** below (and the world layers) carry the scene. Took the retired `Rails` variant's wire ordinal |
| 18 | **Axon Waveguide** | Streamlines | #218 Tiers 1–4 + a brain-tract pass — a bundle of myelinated axons as **step-index optical fibres** (myelin n≈1.44 over axoplasm n≈1.38), drawn as swept tubes: Vogel-disc-packed fibres, periodic **Ranvier-node** constrictions, and a travelling emissive "action potential" pulse (staggered per fibre → a travelling wave) on the global clock (`axon_strands`). Declared after `None` so existing ordinals stay stable. View in **Swept Tubes + Glass/Refractive**. **Tier 2:** an `AxonMode` (LP01/LP11/LP21/LP02/LP31/LP12) lights the bundle cross-section with the **LP guided-mode intensity** `\|J_l(j_lm·r)·cos(lθ)\|²` (`lp_mode_intensity`; `mode_amount` 0 = uniform). **Tier 3 — bend-degradation** (`axon[14]`): `bend` makes the **edge-riding fibres leak** their guided light (dimming along the fibre + **scatter-flaring at the Ranvier nodes**) while the centre **LP01 core survives** — drives the OPTICS only (0 = coherent). **Brain tract** (`axon[15..17]`, `axon_spine`): `curve` bends the straight bundle into a broad C-shaped white-matter arc (corpus-callosum / fasciculus sweep, **arc length preserved** so node spacing + the pulse hold — the pulse flows around the bend), `tortuosity` adds hashed per-fibre undulation, and `dti` cross-fades the colour to the diffusion-MRI **tractography** look (fibre tangent → RGB). Brain-like by default (curve 0.6, tortuosity 0.3, bend 0.35). **Tier 4** (`axon[18..19]`): `dispersion` chirps the travelling pulse into a chromatic spread (warm trailing, cool leading edge — a wavelength-dependent group velocity, via `axon_tint`'s `chroma`), and `polarization` adds a coherence shimmer that stays clean on the surviving core but scrambles to noise on the leaking fibres (both 0 = the Tier-3 look, byte-identical). Slots 20–23 reserved |
| 19 | **Neural field** | raymarch **or** Grid | **dual-path** (#200 Tier 1 + 1b). A tiny SIREN MLP `(x,y,z,t) → (density, rgb)` (`mlp.wgsl` + `neural.wgsl`, weights regenerated inline from two seeds). **Raymarch form** (T1): the network raymarched as an implicit isosurface (fixed-step + bidirectional linear-crossing + tetra-normal, `neural.rs` pass), no nodes, shaded by the shared PBR/IBL **and the full Material card** (Standard/Chrome/Glass/Refractive/Anisotropic — see `neural.wgsl`) with a depth-only prepass so **SSR/SSGI** apply. **Strand form** (T1b, `neural3[0]≠0`): `math::neural_strands` samples the SAME network on a `strands × nodes` grid and **displaces** the nodes → Grid topology, so every Surface mode + Material + membrane apply. The organism is a **seed**; a beat-driven latent walk morphs seed A→B in both forms. `Shared.neural[8]` (identity) + `neural2[8]` (raymarch dials) + `neural3[8]` (strand dials) |
| 20 | **Neural Network** | Streamlines | #226 Tier 1 — a **graph** of neuron **nodes** (soma blobs) wired by **edges** — routed fibre tracts (the #218 Axon edge at network scale). A synthetic **topology bank** (`math::neural_graph`, deterministic + unit-tested): **random-geometric** (a neuron cloud, connect within a radius), **layered feed-forward** (planes of units, the ANN layout), **ring lattice**, and **Watts–Strogatz small-world** (rewire the ring). Nodes are single-frame strands (a soma blob in any Surface mode; hubs read bigger/brighter by degree); edges are bowed quadratic-Bézier tracts sampled into frames, carrying the same travelling **action-potential pulse** on the beat clock (`math::neural_net_strands` / `neural_net_lay`). Streamlines topology, so every Surface mode / material / palette apply — best in **Swept Tubes + Glass**. Renders **connectivity + activity, not a neural simulation**; ANN layouts are *imposed* (units, not cells). **Tier 1.5** (`neural_edge[8]`): `edge_fibres > 1` renders each edge as a **myelinated fibre bundle** — the #218 Axon Waveguide tract at network scale (Vogel-packed fibres routed along the Bézier spine, Ranvier-node constrictions, per-fibre staggered pulse) instead of a lone tube; `dendrite > 0` sprouts a short **dendritic arbor** from each soma so nodes read as neurons. Both inert at defaults (fibres 1 / dendrite 0 → the Tier-1 geometry). **Tier 2 — signal propagation** (`math::NeuralSim`, a stateful cascade carried on the visual like Boids): a node integrates arriving activation, fires past `threshold` (unless refractory), emits a pulse down each outgoing edge that arrives after a **conduction delay** (edge chord ÷ speed) and deposits activation at the target → the cascade spreads. Firing modes seed it — **Wavefront** (a rotating sweep), **Oscillation** (a self-sustaining idle), **Stimulus** (one source rippling outward) — all beat-paced; optional **signal motes** (#81) ride the active edges. `neural_net_lay` takes the sim's `NeuralActivityRef` (node glow + per-edge pulse), so firing nodes flare + only edges carrying a real pulse light up (bundles + dendrites apply); **mode Off = byte-identical to Tier 1/1.5**. **Tier 3 — real biological topologies:** a **Cortical sheet** topology (a folded gyri/sulci surface wired short-range local + sparse long-range tracts → small-world) added to `neural_graph`; and **connectome ingestion** — a `Connectome (loaded)` topology fed from a **JSON sidecar** (`math::neural_graph_from_json`, schema `{nodes:[{id,pos?,scalar?}], edges:[{src,dst,weight?}]}`, e.g. the C. elegans 302-neuron graph). Ids may be numbers or strings; supplied positions are rescaled, else a deterministic load-time **force-directed layout** (`layout_graph`) imposes geometry (the ANN/transformer case); nodes/edges are capped + weak edges thresholded. Editor "Load Network…" button → sidecar path + the `nn_gen` counter the visual edge-detects (mirrors `hdr_gen`); the ingested graph flows through Tiers 1/1.5/2 unchanged (bundles + cascade apply). **Tier 4 — artificial networks with real weights:** a trained **MLP** ingested from the SAME sidecar (auto-detected by a `weights`/`layers` key → `math::neural_mlp_from_json` → `NeuralMlp`; else a connectome). `MLP (loaded weights)` topology (id 6) → `mlp_to_graph` lays the layers out plane-by-plane (units in a centred column), edges = the inter-layer **signed weights** (`|w|` → thickness/brightness, sign → colour via `sign_colour` — warm +, cool −; sparsified below `sparsify`·max|w|), and a **live forward pass** (`NeuralMlp::forward`, hidden = tanh/relu/sigmoid, output linear) fills `NeuralGraph.node_scalar` so the real **activations light the nodes** (input optionally beat-driven via `input drive`). Honest: the graph is REAL (its weights) but the layout is imposed — units, not cells. `neural_net_lay` reads `node_scalar` (MLP/connectome activations) for node lighting + `sign_colour` for signed edges (both inert on Tiers 1–3). **Tier 5 — transformer attention** (the showpiece): a self-attention tensor — a real forward pass ingested from the SAME sidecar (`math::neural_attention_from_json`, schema `{type:"attention", tokens, layers, heads, attention:[L][H][T][T]}`, auto-detected first by its `attention` key → `NeuralAttention`), or a **stylized causal synthesis** when none is loaded (recency decay + a BOS attention-sink + an induction-offset head, softmaxed per query, `attn_synth_logit`/`attn_row`). `Attention (transformer)` topology (id 7) → `attention_to_graph` lays the tokens as nodes on a **row** (or **ring**), a faint **residual-stream backbone** links consecutive tokens, and the **causal attention edges** (i→j, j≤i — strictly triangular) carry `A_ij` for the chosen `(layer, head)` (thresholded to declutter); each token glows by its **incoming attention** (`node_scalar` = how attended-to it is, so the BOS sink lights up). `reveal /beat` grows the attended query set (token-by-token generation); `head sweep /beat` auto-cycles the visualized head. Honest: tokens are POSITIONS, not cells; it renders a real (or plausible) attention pattern, not a claim the network "thinks". **Brain model (#275 Tier 1):** a `Brain model` topology (id 8) → `math::brain_graph` builds two **mirrored cerebral hemispheres** of folded cortical mantle (gyri/sulci displaced along the ellipsoid normal) split by a **longitudinal fissure**, plus a foliated **cerebellum** + **brainstem** hint; wired **short-range local cortex** (k-nearest, intra-region → bilaterally symmetric). **Tier 2 — white matter** (`brain[5..8]`): sparse long-range **association tracts** (intra-hemisphere shortcuts → small-world), a **corpus callosum** (commissural fibres bridging homologous mirror points `i ↔ hemi+i` across the fissure — a symmetric fan), and **subcortical** deep-grey nuclei (a midline cluster wired to the nearest cortex). Edges carry a lower weight for the sparse long tracts so #260's myelin/thickness reads them as tracts; a `BTreeMap` keeps edges deterministic. **Tier 3 — parcellation** (`brain_regions` → `Vec<BrainRegion { name, centroid, members }>`): the standard stimulation **landmarks** (M1 hand-knob L/R, L-DLPFC, SMA, V1, superior temporal L/R, subgenual cingulate) as unit directions projected onto the cortical shell, each gathering its nearby neurons — the **address space** the TMS (#271) + entrainment (#264) tools target. `brain[8..10]` = a target-region **highlight** amount + the region id (brightens the selected region's `node_scalar` so its location reads; regions cached with the graph in the visual). **Tier 4 — stimulation** (`brain[10..12]` = stim strength + rate): a **focal coil-like drive** at the target region turns the `NeuralSim` cascade on (the stimulus IS the drive, even with no firing mode) and pulses the target's neurons at the stim rate via `NeuralSim::stimulate`; the cascade + the **corpus callosum** then carry the effect to the **contralateral hemisphere** (unit-tested: the stimulus crosses only when a callosum is present). This is the #271-TMS coupling as a self-contained hook — a coil placed over the target lights it + its network, no #271 dependency. Bilaterally symmetric by construction (right hemisphere generated, left = mirror), deterministic. Built once and **cached in the visual** (`brain_cache`, keyed on the dials) since the k-NN wiring is O(n²); flows through the same `neural_tissue_lay` / `neural_net_lay` path. Best in the **Neural Tissue surface** (#260). Stylized anatomy — plausible + beautiful, **not** an accurate brain; the substrate the TMS (#271) + entrainment (#264) tools will stimulate. Biofeedback-driven firing (#196) is a follow-up. `Shared.neural_net[16]` + `neural_edge[8]` (bundle/dendrite) + `neural_net2[8]` (cascade dials) + `nn_gen` (load counter) + `neural_mlp[8]` (MLP look) + `neural_attn[8]` (attention look) + `brain[16]` (brain-model dials) |
| 21 | **Lens** | — (raymarch) | #258 Tier 3 — an analytic **double-convex / plano-convex lens** body sphere-traced per pixel as a **signed distance field** (`lens.rs` / `lens.wgsl`, a sibling of Mandelbulb/Minimal). **No nodes**, drives its own raymarch render path (`RenderPath::Lens`). The body is the exact CSG of primitives: biconvex = the **intersection of two mirrored spheres** (`max(sd_sphere, sd_sphere)`), plano-convex = one sphere ∩ the flat half-space `z ≤ 0`, both clipped to a clear aperture by an axial **cylinder stop**. The sphere radius is derived from a **focal / curvature** dial (`R = focal · scale`, lensmaker-style — approximate); **aperture** and centre **thickness** are fractions of the world size. Shaded through the shared Standard/Chrome/**Glass** PBR path (tetra-normal + `frag_depth` + material shading reused from the siblings), so under **Glass/Refractive** it refracts the environment. **Focusing needs the #258 Tier-2 dielectric tracer** (a separate branch, not on `main`): this branch delivers the *geometry to aim light through*; on `main`'s diffuse-only tracer it refracts the env but does not converge to a focus. `Shared.lens[8]` = `[focal, aperture, thickness, plano, scale, steps, _, _]` |
| 22 | **Demo (scene bench)** | Instanced (explicit) | #288 — a hand-authored **reference scene** for showing off the ray-tracing stack, NOT a node field. `math::demo_scene` emits **explicit** scaled/oriented box/sphere/cylinder instances straight into the shared `instances`/`tints` buffers (bypassing the strand/parameter machinery), tagged into per-`(mesh, material)` **sub-batches** (`DemoBatch`), so it inherits shadows / the TLAS / the path tracer / SSR/SSGI for free. `DemoScene`: **Cornell box** (T0 — the path tracer's ground-truth scene: 5 tinted walls red-left/green-right + 2 hero boxes), **Sphere pyramid** / **Sphere grid** / **Box + sphere** (T1 — mixed primitives via the multi-mesh sub-batch draw, box↔sphere↔cylinder), **Glass menagerie** (T2 — a chrome mirror sphere + a glass sphere + diffuse coloured walls in ONE frame; **per-primitive materials** via a small palette patched onto per-sub-batch group-0 uniforms — the scenery/water pattern, no WGSL branch; opaque batches drawn first, glass last through `pipeline_skin` LessEqual), **Light stage** (T3 — a turntable pedestal + a rig of coloured **placeable lights**: emissive geometry that blooms **and** drives a real analytic **point light** the shader adds — `Uniforms.demo_light_pos/col`, `cube.wgsl::demo_point_light`, inert at intensity 0 so every non-Demo frame is byte-identical). The scene reports its own AABB; `demo_static_cam` gates the auto-orbit off so the front-on reference framing holds. `Shared.demo[8]`. Off by default / byte-identical unless selected. Follow-ups: a sphere/cylinder **BLAS** for hardware RT (raster/SSR already work), back-to-front glass sort, the optics bench (T4) + material chart (T5), and letting a demo emitter be a first-class ReSTIR many-light with area soft shadows |
| 23 | **Acoustic field** | Grid | #325 (Duo-Field N1) — a radiating **sound** source rendered as a two-channel Duo-Field. Signed harmonic **monopoles** superpose into a monopole / dipole / quadrupole (`AcousticSource`; `math::acoustic_sources`), retarded in time — `p = q·sin(ωt−kr)/r`, `u = q·r̂·(k·sin/r − near·cos/r²)` (`acoustic_field_pu`). The scalar **pressure** drives the geometry (a breathing multipole shell on a (θ,φ) ray lattice, `acoustic_lattice_strands`, Grid → membrane-loftable) and the vector particle-**velocity** drives the Particle Aura (`AnalyticField::Acoustic`, motes advecting along `u` and glowing by the acoustic energy density `(1−t)p²+t\|u\|²`); the near term is 90° out of phase with the pressure (antinodes at nodes — the "visible nodes"), and the multipole's angular nodes are the pressure zeros. `blend` / `aura_blend` are the independent **pressure↔velocity swaps** (the acoustic E/B duality). **Audio (Tier 3)** rides the shared #248 dipole spine: RMS → drive amplitude, spectrum → a band-weighted monopole stack (`acoustic_band_sources`), spectral centroid → the `maxdip_phase` oscillation clock (pitch → wavelength), stereo → X-lean, and the beat **pumps** the source amplitude (`beat_pump`). Reuses the `maxenergy` energization glow. Shares the Maxwell **Osc Tempo Sync** (`maxwell[22]`/`[23]`) — both Duo-Field generators read the same `maxdip_phase` clock, so the acoustic oscillation is beat-lockable too (surfaced on the Acoustic card); there's no B swirl to lock (a longitudinal sound field has no magnetic analog), so the sync applies to the geometry + aura oscillation only. The most on-theme generator — the field IS sound. **Tier 4 (`Shared.acoustic2[8]`):** a **Cavity** model swaps the radiating multipole for a rectangular standing-wave eigenmode `p = ∏cos(kᵢxᵢ)·cos(ωt)` (`cavity_field_pu`) whose pressure nodal planes are the 3-D **Chladni** figures (`acoustic_cavity_lattice_strands` / `AnalyticField::AcousticCavity`), the modes walking on the beat (`cavity_morph_modes`); and an **intensity flux** channel advects the aura along the acoustic intensity `I = p·u` (`acoustic_intensity`) — the energy-flux "third channel". `Shared.acoustic[16]` + `acoustic2[8]`; off by default / byte-identical unless selected |
| 24 | **Field Engine** | Streamlines / Grid | #381 Tier 1 — render an **arbitrary closed-form field equation** over `(x,y,z,t)`. A tiny stack/bytecode evaluator (`math::FieldProgram`/`FieldOp`/`FieldVal`, pure + unit-tested) parses a text expression into RPN and returns a **scalar `φ`, vector `F`, or complex `ψ`**. Vocabulary: `+ - * / ^`, `sin cos exp log sqrt abs tanh`, `dot cross norm normalize vec`, component picks, `re im conj`, live coefficients `a`/`b`, variables `x y z t r pos` + `i pi tau e`, a **source library** `charge` (Coulomb `1/r²`) / `dipole` (`1/r³`) / `vortex` / `planewave` (complex `exp(i(k·pos−ωt))`) / `gaussian`, and (#381 **Tier 2**) numeric **differential operators** `grad` (∇, scalar→vector) / `div` (∇·, vector→scalar) / `curl` (∇×, vector→vector) / `laplacian` (∇², scalar→scalar) / `advect` ((v·∇)u) evaluated by **central finite differences** of a wrapped sub-program (each op carries a `FieldProgram.subs` index; composable/nestable — `curl(grad(f))`), so `E = -grad(phi)`, `B = curl(A)`, `omega = curl(v)` are one-liners. A `FieldKind` (Auto/Scalar/Vector/Complex) picks the renderer: **Vector** builds an `AnalyticField::Field { program }` → the shared volumetric **field-lines** (`maxwell_lines_volumetric_strands`) + the Particle **Aura** (`fill_analytic`, glowing by `|F|²`), like Maxwell/Acoustic; **Scalar** → a density/height **glyph lattice** (`field_lattice_strands`, size ∝ `|φ|`); **Complex** → the same lattice for `|ψ|²` tinted by phase `arg ψ`. Authored from a **Phenomenon Gallery** (`FieldPreset`: Coulomb / Dipole / ABC flow / Hydrogen `|ψ|²` orbital / Plane wave / Vortex / Gaussian, plus the #381 Tier-2 operator-built **E = −∇φ (point charge)** / **B = ∇×A (uniform)** / **Vorticity ∇×v (ABC)**) or **Custom** = the hot-reloaded `$TMPDIR/organic-math-field.txt` sidecar (edge-detected via `Shared.field_gen`, mirroring `hdr_gen`/`nn_gen`). `a`/`b` are host-mappable/automatable (the "field as instrument" seam). Generalizes the VectorField Tier-3 `VecBuildSpec` builder into free expressions. `Shared.field[10]` (`[kind, preset, scale, extent, a, b, density, gain, thickness, _]`) + `field_gen`; off by default / byte-identical unless selected. **Tier 2 numeric operators are done (CPU finite differences, program-internal — no new `Shared`/IPC).** **#381 Tier 3 — time-marched PDEs (`∂u/∂t = L[u]`):** a pure, unit-tested `math::FieldSim` integrates a discretized 2-D **periodic CPU grid** (default 64×64) forward in time off the PLL beat clock (`PdePreset`: **Heat** `∂t u = D·∇²u` / **Wave** `∂tt u = c²·∇²u` leapfrog / **Schrödinger** `∂t ψ = i(½D·∇²ψ − V·ψ)` via a norm-preserving symmetric split / **Gray–Scott** reaction–diffusion → Turing patterns). The stepper is **explicit + CFL-clamped/substepped** (`dt ≤ dx²/(2·D·dim)` for diffusion, etc.) so it can't blow up; grid operators reuse Tier-2's algebra as finite-difference helpers (`grid_laplacian`/`grid_grad`/`grid_div`/`grid_curl`). When `Shared.fieldsim[0]` (`[preset, D, time_scale, feed, kill, potential, forcing, res]`) ≠ Off the FieldEngine arm marches the sim and renders its live grid through the glyph lattice (scalar → `field_sim_strands`) or `|ψ|²`+phase (complex); a minimal **forcing** hook stamps a Gaussian source. `preset = Off` (default) → Tier 1/2 byte-identical. Deferred to follow-up: FitzHugh–Nagumo, full audio-spine forcing wiring, a user-typed PDE *program* binding grid-state `u` into `FieldProgram`, the JSON **Patchbay** node graph, and **GPU/WGSL** compile — all slot in without a rewrite |
| 25 | **Density-Map Attractor** | Point cloud (instanced) | #380 Tier 1 — a **discrete** iterated map (unlike the continuous-ODE `Attractor` at id 3): iterate the complex-holomorphic seed map `x' = sin(x²−y²+a)`, `y' = cos(2xy+b)` (`z ↦ (sin,cos)` of `Re/Im(z²)+p`; the doubled-angle pinwheel is the fingerprint of complex squaring) for many restart orbits and emit the visited set as a node cloud (`math::map_attractor_field` → `instances`/`tints`, so **every surface mode applies** — cubes show the shape, **`SurfaceMode::Splat` + bloom** gives the additive density "fire"; best with the Inferno/Magma palette). Points are tinted warm by local step-speed `\|Δ\|`. `MapKind` (Tier 1: `Complexus` only) + `map_attractor_step` are the pure, unit-tested contract (determinism / boundedness to `[-1,1]²` / spot value). **Tier 2 — the beat-locked parameter orbit** (`maporbit[8]`): `(a,b)` walk a **closed loop** in parameter space so the whole field morphs seamlessly and returns home at the loop's period. `MapOrbitMode` = Off (static) / Linear (the Tier-1 ramp `a_eff = a + a_drive·gen_phase`, byte-identical with drives 0) / **Lissajous** (`a = a0 + Ra·sin(2π·fa·φ)`, `b = b0 + Rb·sin(2π·fb·φ + ψ)`; integer `fa`/`fb` ⇒ seamless closure). The loop phase φ is driven by the **PLL beat clock** (`φ = beat_pos / loop_beats`, one loop = N beats) while the host plays, free-running on `gen_phase · free_rate` otherwise. `math::ParamOrbit` + `map_attractor_effective_ab` + `map_orbit_trajectory` are the pure, unit-tested orbit (closure `eval(φ)==eval(φ+1)`, bounded box). The overlay reproduces the source image's **(a,b) inset plot** — `overlay_meta::eval_map_attractor` feeds the live `(a,b)` + `overlay::draw_param_plot` draws the trajectory + current point ("you are here in chaos-space"). `Shared.mapattractor[10]` = `[kind, a, b, points_k, warmup, scale, size, intensity, a_drive, b_drive]`; `Shared.maporbit[8]` = `[mode, loop_beats, Ra, Rb, fa, fb, psi, free_rate]`; off/Linear-default byte-identical unless selected |
| 26 | **Creature Engine** | — (raymarch) | #476 Tier 1 — a synthetic **sea creature** assembled from a **union of SDF primitives** (ellipsoids / tapered round-cones / flattened paddles) placed along a spine, sphere-traced per pixel (`creature.rs` / `creature.wgsl`, a sibling of Mandelbulb/Minimal/Lens; `RenderPath::Creature`, no nodes). The body is a **per-primitive smooth-union** — each primitive carries its own blend radius `k` (tight at fine features, wide at seams) so a pile of primitives reads as ONE continuous organism — with a per-primitive `glow` blended through the same smin (bright bioluminescent organs, e.g. a ribbon-swimmer's dorsal rod). `form` picks a built-in **body plan** (0 bell jelly / 1 ribbon-swimmer / 2 paddle-finned predator), built **CPU-side** in the visual (`math::creature_body_plan` + `creature_map`/`prim_sdf`/`sd_ellipsoid`/`sd_round_cone`/`sd_round_box`, all unit-tested; mirrored 1:1 in `creature.wgsl`) and uploaded as a compact primitive list, so only 8 scalars ride the wire. A travelling **peristaltic domain warp** (`creature_warp`) along the body axis is the swim, its phase advanced off the global Speed clock (rides the beat via Speed Pulse). Shaded through the shared **material-branched** PBR/IBL path (DE-gradient normal + `frag_depth` + supersample + coverage/premultiplied-alpha, reused from the siblings): **Standard / Chrome / Glass / Refractive** + spectral glass (dispersion/caustic) + thin-film — the **same Material card the cubes use**, ported 1:1 from `minimal.wgsl`, so Glass reads as translucent gel — plus a Fresnel **rim** glow + SSS. A chosen **Palette** retints the whole body along its (smin-blended) spine coordinate **in-shader** (`math::palette_tint` mirrored in `creature.wgsl`; Native = the bioluminescent blue). A depth-only `draw_depth` march (`fs_ray_depth`) writes the surface into the screen-space-FX prepass, so **SSR + SSGI** reconstruct off it and composite in post (in-shader SSAO / contact-shadow / VXGI-gather masks stay excluded, matching the neural/voxel raymarch paths — a follow-up). No node field, so Surface mode doesn't apply. `Shared.creature[8]` = `[form, scale, detail(steps), swim_rate, warp_amp, warp_freq, rim, glow_scale]`; off by default / byte-identical unless selected. **Tier 2a** adds the **metachronal wave** (`creature2[8]`, `math::metachronal_wave`): a beat-driven band of light running along the body, phased by each primitive's spine coordinate, brightening the emissive glow additively (`band amount` 0 = the Tier-1 look). **Tier 2b** adds **authorable body plans** (`creature_gen` + `math::parse_creature_spec`): a JSON schema of `ellipsoid`/`cone`/`paddle`/`chain` emitters + `mirror_x`, hot-reloaded from the "Load Creature (JSON)…" sidecar, with a gallery in `native/assets/creatures/` (installed by `deploy.sh`). **Tier 2c** adds the projected depth-occluded **anatomy overlay** (`creature3[4]`, `creature_overlay.rs`/`.wgsl`): a spine / cross-section-ring / limb-vector diagram drawn over the creature as additive lines, two-pass depth-tested so it dims behind the translucent body — the "diagram over a living creature" look (off by default) |

### The Scenery layer (#187 pivot)

**A second, CONCURRENT generator category** — generated scenery you move through,
running alongside the primary generator with its **own material, surface and
palette**. `SceneryMode` (None / **Zone** / **Terra**) selects it; `ScenerySurface`
picks how it renders — the instanced trio (cubes / flow rods / swept tubes) **or
`Skin`**, a lofted **membrane** surface (#206 Tier 1: `loft_scenery_append` skins the
Grid strand sets; non-Grid — Zone's Gates — fall back to swept tubes). **Zone is
the whole rails machinery** (the beat-parametrized corridor: 7 archetypes, per-cell
+ per-phrase morphing, the quantized-transition latch, ribs/fade — everything the
retired `GeneratorMode::Rails` did). **Terra (#206 Tier 2)** is the second scenery
generator on the SAME machinery: a beat-parametrized **flowing landscape** —
fjords / river banks / canyons (a `TerraForm`) from a continuous fBm heightfield
(`math::terra_strands`) whose shape morphs per cell (contiguous by construction, no
tiles), one Grid strand per lateral sample so the membrane loft skins the valley.
The channel **meanders** (the lateral treadmill: geometry is generated
channel-relative to `u_now`, so the camera flies straight while the valley sweeps
under it) and a **navigable channel is exactly guaranteed open** (the clear-bore
analog: within the channel half-width, height ≤ −clearance). Terra reuses the rails
timing/window slots (speed / cell_len / change_every / variance / seed / evolve /
horizon / rows-per-beat / fade / bore-as-scale) and adds a `Shared.terra[16]`
landform block; the two blocks latch **together** (one combined `[f32; 40]` in the
visual: `rails[..24] ++ terra[24..40]`, `rails_latch_step` is now const-generic over
the width), so a fjord→river preset recall crosses on the bar. Both are evaluated
after the generator dispatch into the scenery instance/tint buffers (and, for Skin,
its own membrane vertex/index buffers).
Renderer-side, scenery is a **second concurrent draw with its own group-0
`Uniforms`** (`Shared.scenery[16]` patches mat/ior/glow/opacity/SSS/irid/palette
onto a copy of the scene uniforms — the same pattern as the #182 liquid material);
instanced scenery draws the cube/cylinder mesh, Skin draws the lofted sheet
(cull None, single-sided, **LessEqual** + write via `pipeline_skin` — LessEqual so
the shared-prepass route, where the FX prepass already wrote the skin's depth,
doesn't reject it with a plain `Less`) with the identity instance + per-vertex
colour. Both rasterize into the FX depth prepass (so SSAO/SSR/DoF/TAA see the
corridor; the prepass scenery draw binds the scenery uniforms so its depth matches)
and the shadow map (pure ride only). The Skin transition skins both worlds — the
two-world loft appends the pending side's vertices past the active side's, indices
offset — and a **mixed-topology** transition (one side Grid → membrane, the other
Streamlines → instanced swept-tube fallback) draws **both** the membrane and the
instances at every scenery site (not either/or), so neither half of the corridor
drops out.

**Terra water floor (#206 Tier 3).** Terra grows a **channel water sheet** — a
flat, valley-spanning membrane at the per-cell water level (`terra_ctrl_at`'s
`ctrl[6]`, so it CR-morphs and latch-quantizes with the landform). `math::water_strands`
emits one Grid strand per lateral sample (mirroring `terra_strands`' windowing +
meander treadmill), displaced by a two-octave `terra_fbm` **ripple** that scrolls
with the beat clock, then `loft_scenery_append` skins it into the visual's
`water_mem_*` buffers. Renderer-side it's a **third** concurrent group-0 uniform set
(`WaterLayer` → `render::Renderer::water_ubuf/bind`, the liquid/scenery pattern):
its own material (`Shared.water[8]` = mat/roughness/ior/opacity/glow/ripple/ripple_freq,
default **Glass**, dielectric metallic 0, tint-as-albedo). It draws in the scene pass
(skin pipeline, alpha-blended) **and the FX depth prepass**, so **SSR reflects the
fjord walls in the channel** (the money shot). The terrain banks occlude it at the
shoreline by depth; `terra_strands` also lays a **shore tint band** on the landform
near the waterline contour. Water isn't drawn in the shadow pass (a flat sheet below
the banks casts nothing useful). The water *material* is an instant Look (not
latched); the water *level* rides the latched terra block.

**Two camera modes** (the composite model): with a **generator visible**, the
**orbit rig stays in charge** (camera paths / drag / zoom orbit the generator;
scenery is excluded from the framing AABB) and the corridor renders
**VIEW-LOCKED** — `SceneryLayer::view_proj` = identity view × the scene
projection (+ the TAA jitter), so the tunnel is glued to the eye, always flying
straight ahead/behind/around, never orbiting. Both depths are eye-relative
distances, so the corridor composites honestly into the shared depth buffer and
the screen-space reconstruction places it at its true view positions. View-locked
scenery **skips the world-space shadow map** (its coordinates aren't world space).
With **generator = None** (the pure ride) the **rails camera** engages (forward
flight, drag = bore-clamped offset), rail space is world space, the scenery joins
the scene AABB, and it casts shadows. **Meander-facing camera (#206):** when a Terra channel winds, the scenery view yaws (`math::terra_channel_heading`, smoothed on `channel_yaw`) so the channel heads straight down −Z — the river rotates underneath as it twists while the centred object floats down the middle (geometry is channel-relative, so the channel centre already sits at the origin — only the heading is applied, as a world-space Y-rotation on the scenery view-proj; the object's orbit view is untouched).

**Pure-ride gate routing.** Because most systems key off the *primary generator's*
geometry, the pure ride (generator None + Zone, where the corridor is the only
geometry) re-points three of them at the scenery instead: the **FX depth prepass**
runs when `scenery_live` even with no generator instances (so SSAO/SSR/SSGI/DoF/TAA
apply to the corridor); the **shadow pass** runs for world-space scenery even when
`draw_instances` is false (view-locked scenery still skips it); and the **node-driven
systems** — particle aura, fluid-ink dye splats, liquid colliders — read the scenery
node set (`scenery_instances`) as their source. In composite mode the view-locked
scenery is eye-space, so it stays excluded from those world-space systems (only the
generator feeds them). Future scenery types: water planes, canyons, ….

**Two kinds of generator.** Most are **node fields** that fit the strand contract.
**Mandelbulb** and **KIFS** are **per-pixel raymarched** (no strands); they run on a
sibling render path that bypasses strand lowering but still shares the
PBR/IBL/HDR/bloom/camera/beat stack (Surface mode / palette don't apply). **Minimal
surfaces** is **dual-path by Family**: implicit TPMS families raymarch like those two,
while parametric families emit a (u,v) Grid that the normal instanced / membrane-loft
path skins (so every Surface mode + Material applies). The visual picks the path from
`minimal_is_parametric(family)`.

**Mostly stateless; the stateful path is now realized (#52/#99/#226 T2).** Most generators are
a pure function of `(params, gen_phase)` — evaluated fresh each frame. A few carry
**persistent simulation state** (Boids' flock, the harmonic Physical bell, and the Neural
Network's activation cascade `NeuralSim`), and they confirmed the planned design needs **no trait
change**: the visual *owns* the sim on its app struct, rebuilds it on a structural key change,
and the match arm calls `step(dt)` (beat-paced) + lays geometry like any other arm; **no IPC growth**
(generators evaluate visual-side).

- **Boids** (id 12, PR #105) — `math::BoidsSim` (Reynolds separation/alignment/
  cohesion). Reseeded on a key change, integrated with a fixed-dt accumulator + a
  stored seed → deterministic and unit-tested offline. Trails → strands (Streamlines);
  the beat can pulse the goal attractor.
- **Soft-body bell** — *not* a separate generator but a stateful **"Physical" mode of
  the Harmonic generator (id 4)**: `math::BellSim` is an XPBD membrane that genuinely
  contracts and recoils on a beat-pulsed stroke instead of replaying a waveform (apex
  ring pinned). Free-body **jet propulsion via fluid coupling is the remaining #99
  work**.

Both `step`s are pure (deterministic, no RNG in the bell) and unit-tested. This is the
foundation any further stateful generator/mode reuses.

**Surface modes / materials are orthogonal to the generator.** `SurfaceMode` picks how
nodes become geometry (cubes / flow-aligned rods / swept tubes / metaball / membrane /
voxel / emissive volume / **neural tissue** — #260 closed anatomical primitives:
soma icospheres + capped capsules + boutons via the multi-mesh draw, recommended for the
Neural Network generator, closed capsules elsewhere, with a waxy membrane driven from
`Shared.neural_surface[16]` through the shared Surface-FX SSS/iridescence path, plus
Tier-2 grown neuron morphology (dendritic arbors + hillock axon + terminal boutons),
plus **Tier-3 myelinated axons** — each edge lowered as a myelinated nerve fibre (fatty
internode-sheath capsules + thin Ranvier-node constrictions, thick tracts fanned into
Vogel-packed fibre bundles) with a **saltatory** action potential (the bright internode
jumps Ranvier-node to Ranvier-node in step with the Tier-2 cascade pulse) + arrival flash
at the boutons, all via `math::myelinate_edge` into the capsule sub-batch — the #218 Axon
Waveguide aesthetic promoted to every edge; `myelin_amount = 0` = plain capsule edges,
byte-identical to Tiers 1/2; plus **Tier-4 (the final tier) — the living synapse + tissue
context**: `synapse_cleft` pulls each terminal bouton back off the post-synaptic membrane
so a visible cleft gap opens, `synapse_vesicles` bursts a **deterministic neurotransmitter
shimmer** (tiny short-lived instances crossing the cleft, keyed to the cascade
`edge_pulse ≥ 0.82` spike-arrival event so it's a pure function of sim state), and
`synapse_glow` lights each soma's **cytoplasmic interior** by its live activation (the
finalized neural material's activation-tied glow, on top of the SSS/iridescence membrane);
optional faint tissue context (`neural_surface2`: `glia` astrocyte scaffolding + `capillary`
threads) seats the network in tissue — all emitted into the existing sub-batches, all inert
at their default 0 dials; extracellular-medium fog + a wet-fresnel membrane rim are
shader-side follow-ups);
**plexus** (ordinal 9, #plexus — the "field web" look; Splat took 8 on its merge) is a generator-agnostic
post-process: whatever node cloud a generator emitted into `self.instances` (each
instance's translation is a node centre) is rebuilt into a proximity graph — every node
wired to its up-to-`max_links` nearest neighbours within a radius by a thin strut
(`push_rod`) plus a marker per node (`math::draw_plexus`, in the post-match pass in
`visual.rs`). The radius/strut/marker sizes are unitless multipliers of the field's
characteristic node spacing (bbox diag ÷ n^⅓), so the web reads consistently across
generators; struts inherit their endpoints' average tint. Raymarch generators emit no
instances, so it's a no-op for them. `Shared.plexus[4]` = `[radius_mul, max_links,
strut_mul, marker_mul]`, captured **Generator**, inert unless `surface_mode == 9`. **Shape
morph** (`plexus4[0..2]` = `[node_shape, edge_shape]`): Tier-1 markers morph cube → rounded
cube → sphere (`math::morph_cube_mesh`) and struts morph a sharp-square → circle cross-section
(`math::morph_strut_mesh`, reusing `cross_section`), independently; both default **1**
(sphere nodes / circular struts — 0 recovers the sharp cube/square look). The two morphed meshes are
uploaded per frame (rebuilt only when a slider changes) and drawn as **two instanced sub-batches**
over the plexus instance buffer (`render::PlexusBatches` / `draw_plexus_batches`, mirroring the
`NeuralBatches` split): markers `[0, markers)` with the node mesh, struts `[markers, markers+struts)`
with the strut mesh (`draw_plexus` returns the counts). Tier-2 impostors are already round, so the
morph is Tier-1 only.
**Tier 2 (impostors + independent materials, `plexus2` + `plexus_node_mat` + `plexus_edge_mat`):**
`plexus2[0]` swaps the instanced cubes for GPU **impostors** — nodes as analytic **sphere**
impostors (built caller-side as A≈B degenerate capsules), edges as **capsule** (tube) impostors —
each with its OWN full material (`plexus_node_mat[8]`/`plexus_edge_mat[8]` = `[mat_type, metallic,
roughness, ior, hue, sat, val, emissive]`). Two self-contained capsule batches on `ParticleSystem`
(`set_plexus`/`draw_plexus`, each its own `DrawU`, reusing the validated `arm_pipeline` + capsule
shaders without touching the membrane-arm path); `visual.rs` builds the two `ArmInstance` lists from
`math::plexus_graph` and clears `self.instances` so the raster cube draw is skipped. **Tier 3
(signal propagation, `plexus3[4]` = `[on, speed, gain, width]`):** a Gaussian activation shell
(`math::plexus_signal`) radiates from the web centre on the PLL beat clock, boosting the per-instance
emissive of the impostors it crosses — the web "fires" to the music (rides the Tier-2 impostor path).
**Overlay (`plexus_overlay[4]` = `[on, shell_scale, shell_depth, shell_bins]`):** the same web
can instead be layered as an **outer shell around ANOTHER surface** (Metaball, etc.), like the Particle
Aura / Water — set `plexus_overlay_on` while a different `SurfaceMode` is active. The overlay block in
the post-match pass reads the base generator's node cloud **non-destructively** (it does NOT clear
`self.instances`, so the base surface keeps rendering), extracts its **outer shell** via
`math::outer_shell` (direction-bin the cloud into `shell_bins²` cells; in each cell keep every node within
a **radial band** of the outermost — `r ≥ r_max·(1 − shell_depth)` — the rind, not the full volume) and
grows it outward by `shell_scale`, then wires it
with the SAME look params (`plexus`/`plexus2`/`plexus3`/`plexus4` + node/edge materials) — so it keeps
every feature: Tier-1 shape-morph markers/struts, Tier-2 impostors + independent materials, Tier-3 beat
signal. Tier-2/3 overlay reuses the existing `plexus_node_caps`/`plexus_edge_caps` capsule path (already
overlay-safe); Tier-1 overlay draws its markers+struts over their **own** instance buffers
(`render::Renderer::plexus_ov_inst_buf`, `draw_plexus_batches_from`) so they layer over the base surface
instead of replacing it — drawn in the scene pass (Less+write, over any base RenderPath), the FX depth
prepass (SSAO/SSR see it) and the shadow-cast pass. Gated to `!plexus` (skipped when Plexus is already
the surface) and off (`plexus_overlay_on = 0`) → byte-identical.
`MaterialType` picks shading (Standard PBR / Chrome mirror /
Glass / Refractive — Glass plus Beer–Lambert absorption through each node's body,
`Shared.refrmat[4]` / Anisotropic — Standard PBR with an elliptical GGX lobe streaked
along the instance's long axis, `Shared.aniso[4]` / Clearcoat — a thin smooth coat lobe
/ Velvet — a grazing sheen lobe, `Shared.coat[8]` / Subsurface — Standard PBR with the
translucency back-glow driven by the **measured body thickness** (Beer–Lambert over the
chord through the instance, `instance_thickness`) so thin edges glow and thick centres go
deep — honest wax/jade/marble, `Shared.body[4]`; anisotropy, clearcoat, and sheen are
also exposed as overlays on Standard/Chrome, and the thickness model + a Glass/Refractive
**interior in-scatter** glow (opal) ride `body[4]` on any material). Both apply to every
node-field generator. **Microstructure** (#214 Tier 4, `Shared.micro[8]`) weaves three
high-frequency dials into the Standard/Chrome returns, all inert at 0: **glitter** (sparse
per-facet sparkle flakes, twinkled by the frame seed and resolved by TAA/blue-noise like the
stochastic glass), **diffraction** (a grating rainbow multiplying the env specular — holo-foil,
strongest over Chrome), and **retroreflection** (a glow back toward the light, `dot(v,l)`).
**Spectral emission** (#214 Tier 5 pt 1, `Shared.emit[4]`) adds two emissive terms woven into
**every** material's `emissive` (so they bloom through the HDR pipeline), both inert at 0:
**fluorescence** (the surface absorbs the env's short-wavelength/blue irradiance and re-emits it
at a chosen hue — a blacklight-poster glow) and **incandescence** (a blackbody-locus glow by
temperature in Kelvin, embers → white-hot, via `blackbody()`).
**Screen-space refraction** (#214 Tier 5 pt 2, `refractsurf.rs`/`.wgsl`, `Shared.ssrefr[4]`)
is the exception for *transmission*: on the Refractive material with the strength dial up,
a `liquidsurf`-style post pass (run at the SSR/liquidsurf seam, after the scene resolves,
before bloom) reconstructs each covered pixel from the depth prepass, refracts the view ray
at the IOR, projects the bent ray back to screen, and replaces the env-only transmission
with the displaced RESOLVED SCENE — so a cube shows its neighbours / the world behind it.
Off-screen fetches fall back to the pixel's own colour, and a **foreground depth test** keeps
the fallback when the landing pixel is nearer than the glass (so nearer objects aren't smeared
through) — no seam; Fresnel keeps the forward reflection. Gated on the instanced Refractive
path + a valid prepass (its strength joins the `depth_fx` OR-chain so the prepass runs), and
**off when scenery/water share the prepass** (their view-locked depth + own materials would
reconstruct wrong / refract non-glass); strength 0 → not dispatched → byte-identical.

---

## 9. The world & render pipeline — altitude

> 📖 **Depth lives in [`doc/arch/render.md`](doc/arch/render.md)** — per-frame data flow,
> the full pass list, geometry/materials/MSAA/resolution, hardware ray tracing,
> lighting/IBL, and the shader inventory. It is **not** auto-injected; open it when you
> are working on the renderer. This section is what you need to know without it.

**The shape.** The **world** owns the animation clocks, the camera, and any generator
state; it **reads** everything else from the IPC `Shared` snapshot (§6). A frame goes:
generator → instanced geometry (or a raymarch path) into a **linear `Rgba16Float` HDR
buffer** carrying raw radiance → screen-space effects → **post** (bloom) → a single
**composite** (exposure → add bloom → tonemap → surface). The 128-bit radiance survives
to that last step, which is why highlights roll off instead of clamping.

**The world/window split (#572, the world hoist).** This is the structural fact to hold:

- **`world.rs` = the world** (~12 900 lines) — everything that draws or decides what to
  draw, plus its `#[path]` module tree (`render`, `capture`, `overlay`, `axes`, `chamber`,
  `hdr_macos`, `rt`, `metal_island`, `gpu_timer`, `recorder`, `snap`, `ui_layer`,
  `winit_platform`). **All generator dispatch is here**, not in the binary.
- **`bin/visual.rs` = the window** (~625 lines) — create it, pick its display, own the
  surface and swapchain, run winit's event loop, forward.

The world was hoisted *out of the binary* so the editor in `lib.rs` could reach it (a
binary's `#[path]` modules are unreachable from the library it depends on). `World` is
therefore a **library type**, so the binary cannot `impl ApplicationHandler` for it — it
wraps it and forwards, and what it cannot do comes back as an `EventResponse`. The host
hands in a **`FrameTarget`** per frame and applies the **`FrameRequests`** the frame
returns; `World` owns no window, surface or swapchain. Gated on `mind-edition` by
measurement (ungated it grows the plugin cdylib by 490 KB).

**`World` is being partitioned into ownership clusters (#618 T4a).** It carried **250 flat
fields**; it now carries **161**, with eleven cohesive subsystems lifted into their own structs
over four passes:

| Cluster | Type | Fields | What it owns |
|---|---|---|---|
| `record` | `RecordState` | 19 | the #430 encode session, its key-latched pending toggles, and the phrase-chunk grid |
| `plexus` | `PlexusScratch` | 17 | the #8 plexus node cloud, impostor caps, morph meshes and overlay shell |
| `chamber` | `ChamberDecor` | 7 | the #346 Field Chamber decor rebuilt each frame |
| `particle_aura` | `ParticleAura` | 7 | the #81 CPU velocity grid, its upload buffer, and the mote respawn's node samples |
| `fluid_grids` | `FluidGrids` | 6 | the #182 dye grid, the MLS-MPM collider occupancy, the #247 ember glow, + their uploads |
| `performer` | `PerformerLink` | 12 | the #317 agent override lane, its live-state snapshot, and the `Shared.agent[8]` edge counters |
| `cmd_chan` | `CmdChannel` | 6 | the #452 `organon` CLI lane and the Tier-3 "eyes" snap/record lane |
| `geom` | `Geometry` | 11 | the strand bundle and everything lowered from it — raster instances/tints, the RT cylinder set, welded-mode node anchors, the welded mesh |
| `hdr` | `HdrState` | 8 | the loaded `.hdr` path + gen counters, live enable/headroom, the wide-gamut pair |
| `field_prog` | `FieldProgram` | 4 | the #381 compiled field program, its source, and the recompile key |
| `fdtd_sim` | `Fdtd` | 3 | the #412 persistent CPU FDTD Maxwell grid + its sub-step counter |

⚠️ **The metric is reachable mutable state, not line count.** `frame_body` stays **7 471 lines
and stays one linear script you read top to bottom** — that is deliberate and it is what the
tier was reframed to preserve. What changes is that a region touching the recorder now reaches
19 fields instead of 250, **and the compiler enforces the disjointness** that #616's fix message
had to assert by hand ("`gfx.ui` is disjoint from `gfx.device`… so borrowing removes the failure
mode"). That is the same idea as `param_block!`: encode the invariant so violating it fails to
compile, rather than being careful.

**Cluster by meaning, never by adjacency.** This has now caught a field on **both** passes.
`maxdip_phase`/`rms_hist` sit between the particle-aura fields and the fluid grids and belong to
neither — they are the #248 audio-dipole's own clock. And `mods_shift` sits between
`recording_fixed` and `record_fps` in the old flat struct and is *not* recorder state — it is live Shift-key input, read
across the seam as `rec.perfect_pending = mods_shift`. It stayed on `World`. Taking the hull from
first to last recorder field would have swallowed it silently; the transform script fails loudly
instead. This is the same proximity-vs-membership error that regressed #618 T1a twice.

**Cluster names must not collide with established locals.** The cluster is `record`, not
`rec`, because `world.rs` has a long-standing local `rec: recorder::Recorder`; a `rec` cluster
puts `self.rec.pending_finalizers.push(rec.finish_async(..))` on one line — two near-identical
tokens meaning different things. Renaming the 32-site local looked like the fix and was the
wrong one: every candidate collided with something already there (`session` is a `String`
filename stem fourteen lines away, `enc` is a `CommandEncoder`, `take` fights
`take_size()`/`take_out`). **The new name is what should move.** `record` also makes the
mapping the most natural available — the old `record_*` prefix simply becomes the cluster.

**#648 T1–T3 made the partition load-bearing. `frame_body` now makes ZERO `&mut self` method
calls** (34 before T1), so a `&mut` binding of any cluster can be held across the whole function
— which is what T4's binding pass needs and could not previously have. The progression was
**34 → 5 → 2 → 1 → 0**: T1 moved `emit_strands` (29 of the 34) onto `Geometry`, T2 moved `set_hdr`,
`ensure_field_program` and `run_fdtd` onto the three clusters above. T2's remainder moved `build_arm_caps` onto `Geometry`
and `ensure_agent_worker` onto `PerformerLink`; **T3 turned `step_agent` into an associated fn**
that names the four clusters it mutates:

```rust
fn step_agent(perf: &mut PerformerLink, cmd: &mut CmdChannel, rec: &mut RecordState,
              geom: &mut Geometry, frame_ms: f32, cpu_ms: f32, s: &mut ipc::Shared)
```

That signature *is* the thesis of Tier 4 — the compiler now knows exactly which subsystems the
call touches, instead of "all 161 fields, trust the author".

⚠️ **Where a method reaches state that is not its cluster's, that state becomes a parameter, not
a field.** `run_fdtd` bakes into `field_vol_grid`, but that is **#348 Field Volume** with five
other writers — pulling it into an FDTD cluster would be the adjacency error wearing a different
hat. Likewise `gen_phase` is generator animation state. Both are parameters; at the call site
they are disjoint borrows of `self`, which Rust allows. The substitutions are **declared** in
`check-world-partition.py` so they ride the mechanical rewrite rather than becoming hand edits.

**#648 T1 made the partition load-bearing for the first time.** Until it, the clusters only
*narrowed* what a region reaches; nothing stopped `frame_body` holding `&mut self` anyway. You
cannot hold `&mut self.cluster` across a `&mut self` method call, and `frame_body` made **34** of
them. `emit_strands` — 26 lines, called **29** of those 34 times, touching only the 11 fields that
became `Geometry` — moved onto the cluster, so its call sites read `self.geom.emit_strands(..)`
and borrow `geom` alone. That one move took `frame_body` **34 → 5** `&mut self` calls, which is
what made a disjoint `&mut` binding possible at all; T2 and T3 took the remaining five to zero
(above), and #648 T4 is the binding pass those cleared the way for (below).

⚠️ **Field names are deliberately unchanged inside `Geometry`.** `self` *is* the `Geometry` inside
the moved method, so its body stays byte-for-byte what it was on `World` — and
`check-world-partition.py` **check D** asserts exactly that (logic only; comments are excluded
because a receiver change makes some of them wrong, e.g. the #276 recursion note that named
`self.emit_strands`).

**The partition is proved, not reviewed** — `native/tools/check-world-partition.py`, following
the `check-editor-extract.py` (#602) precedent. The danger here is not a compile error but a
**type-compatible mis-mapping**: `record_start_beat` → `rec.chunk_bpm` is a silent behaviour
change that compiles, passes every test that doesn't exercise chunked recording, and reads fine.
Both are `f64`. So the script proves the change *is* the declared renaming and nothing else:
(A) every reference site outside the struct declaration and initializer is byte-identical to the
mechanical rewrite of the base commit, (B) no `self.<old>` path survives *outside* those regions
(inside a moved method it is correct — `self` is the cluster there), (C) every field's declared
type is unchanged, and (D) any method moved onto a cluster kept identical logic. All three are exercised by deliberately-broken inputs before being
trusted. Run it with `python3 native/tools/check-world-partition.py [--base <commit>]`.

**#648 T4 — the binding pass — is what makes the partition compiler-enforced rather than
conventional.** Until it, nothing stopped a later edit from reaching `self.record` out of the
middle of the plexus code; the clusters only *narrowed* what a region reached by convention.
`frame_body` now binds a cluster as a `&mut` local over the region that uses it, and inside that
region a second path to the same state is an `error[E0499]`, not a review comment:

```rust
let record = &mut self.record;      // the borrow ends at its last use (NLL)
…
if out.presented && record.toggle_pending {     // `self.record` unreachable here
```

**Bind per block, not once at the top — and scope it by measurement.** This is the part that
inverts on inspection. As a single span most clusters look hopeless: `record`'s first and last
uses are 6 779 lines apart, 91% of the function. But the sites are not spread, they *cluster*,
and binding once per block (shadowing is fine — NLL has already ended the previous borrow at its
last use) collapses the live ranges to a few percent each:

| Cluster | Local | Bindings | Sites | Live lines | % of `frame_body` |
|---|---|---:|---:|---:|---:|
| `record` | `record` | 3 | 104 | 434 | 5.8% |
| `plexus` | `plex` | 2 | 66 | 283 | 3.8% |
| `particle_aura` | `aura` | 1 | 53 | 822 | 11.0% |
| `chamber` | `decor` | 1 | 10 | 28 | 0.4% |

⚠️ **What is *not* bound is as deliberate as what is**, and each omission carries its reason in
`check-world-bindings.py` rather than being silently dropped. **`geom` is skipped**: 205 sites,
but one block is 2 750 lines, and a binding whose live range is 40% of the function is exactly
the "buys nothing the struct partition didn't" case — nobody reads 2 750 lines as a region.
`hdr` is too sparse (17 sites over 879 lines). `fluid_grids` wants a *depth-4* binding inside
`if ink_on {`, a different anchor shape, so it is its own follow-up. Two more blocks sit inside
the `render::Surface { .. }` literal, where a `let` is not legal at all.

⚠️ **Binding enforces aliasing, not reachability — say it that way.** `self` is still in scope
inside a bound region, so `self.geom.x` remains reachable from inside the recorder's. What the
compiler now guarantees is that *within the live range that cluster is reachable only through
the local*. That is precisely the property #616's fix message had to assert by hand, and it is
narrower than "the region reaches 19 fields instead of 250" — the honest claim is the former.

**Proved the same way, by a sibling script** — `native/tools/check-world-bindings.py`. It is a
*simpler* proof than the partition's, not a harder one: no struct declaration, no initializer and
no `impl` block moves, so byte-identity covers **100%** of the file instead of the complement of
two excluded regions. (A) `world.rs` is exactly the base with each declared `let` inserted and
`self.<cluster>` → `<local>` applied inside its declared range, byte for byte; (B) every declared
range is live and no `self.<cluster>` survives inside it — *outside* a range it is correct and
expected, since bindings are per-region; (C) every edit the substitution cannot produce on its own
is declared in `REWRITES` and matches exactly one site; (D) the declared ranges are **pairwise
disjoint**. Both anchors are whole lines that must be **unique in the file**, so a declaration
cannot silently drift onto a different region as `world.rs` changes.

⚠️ **D exists because check A structurally cannot catch what it guards.** The splice walks
bottom-up so earlier indices stay valid, which is only sound for disjoint ranges — an overlapping
pair would double-substitute or mis-splice. And `--apply` and the check share one `transform()`,
so a splice bug writes the same wrong bytes on both sides and **confirms itself** byte-for-byte.
The compiler would catch the wreckage eventually; D names the wrong declaration instead. It
matters most for the deferred `fluid_grids` binding, which sits *inside* `particle_aura`'s range
— the first realistic chance to declare an overlapping pair.

There is exactly one `REWRITES` entry, and it is instructive: the mechanical substitution turns
`&mut self.record` into `&mut record` at the `step_agent` call site, but `record` is *already*
`&mut RecordState`, so that is a double-mut (`E0596`). Passing the binding straight through is an
implicit reborrow. Declared, not hand-fixed.

**`winit::window::Window` no longer appears in `world.rs` at all** (#593 T3). Stage 3 left one
coupling — `FrameTarget::ui_window`, a `&winit::Window` lent so `egui-winit` could ask it for
`inner_size()`/`scale_factor()` — and Tier 3 replaced it with `ui_scale_factor: Option<f32>`
(`None` = draw no interface, as before) plus a host-built UI layer handed to `attach_gpu`.
`winit::event::WindowEvent` *does* remain, in `World::on_window_event`: that is the winit host's
entry point, it carries the visual's whole keymap, and a baseview host never calls it.

**#621 added the world's *second* input entry point rather than widening that one.**
`World::apply_camera_input(CameraInput)` — `Orbit { dx, dy }` in physical pixels and
`Zoom { dy }` — is backend-neutral, and `on_window_event`'s `CursorMoved` / `MouseWheel` arms now
**delegate** to it instead of holding the gesture maths themselves, so the visual and Organon
Mind's embedded viewport cannot orbit at different rates. The keymap deliberately did **not**
cross: most of it is projector work that means nothing in a docked pane, and **Esc** (which
quits) has no settled owner inside a plugin editor. `scene_input.rs` owns the seam, the egui side
that drives it, and the measurement of why `mind_shell::PointerRouter` cannot be the source in a
host that draws a `CentralPanel`.

**The seam to extend through: `RenderFrame` / `RenderPath`** (issue #104). A new render
subsystem threads through `RenderFrame`'s relevant sub-struct — `Background` / `Surface` /
`LightTransport` — **not** a new positional argument. `RenderPath` selects the family
(instanced geometry vs. a per-pixel raymarch such as Mandelbulb/KIFS). §17 is the
checklist; `doc/arch/render.md` is the detail.

**The passes, in order** (each one detailed in the split doc): depth prepass → optional
shadow map → hardware-RT passes (shadow / AO / reflect / GI / caustics / path-trace) →
skybox or terrain → instanced scene into the HDR buffer → screen-space FX (SSR, SSGI,
SSAO, kaleidoscope) → temporal → bloom → composite → overlay/axes/UI.

**Shaders** are naga-parsed and validated offline by `tests/wgsl.rs` — binding, type and
uniformity errors are caught without a GPU. That is the ceiling; the frame itself needs
`verify.sh` (§15).

---

## 10. The world layer (`terrain.rs`, `stars.rs`)

**Not generators** — global display layers drawn *behind* any generator, sharing the
camera/HDR/tone-map:

- **Terrain** — a raymarched infinite fBm landscape (8 noise flavours, 7 palettes,
  emissive HDR, day→night sun cycle, atmospherics + god-rays, reflective water,
  resolution scaling). Replaces the skybox as the background when on. Its sky also
  hosts the **#102A volumetric clouds** — a raymarched coverage/erosion cloud layer
  (`Shared.clouds[12]`; HG forward-scatter + sun light-march for silver linings; casts
  soft shadows on the land) that replaces the old flat cloud sheet when enabled — and
  the **#102B FFT ocean** (`Shared.ocean[12]`): the terrain water shader tiles a
  Tessendorf wave field synthesised CPU-side in `ocean.rs` (Phillips spectrum → inverse
  FFT → normal/height/foam tile). **The terrain pass runs when terrain *or* ocean is on**
  — with the landscape off (a `land_on` shader flag) the ocean fills an infinite
  open-ocean world.
- **Starfield** — the Yale Bright Star Catalog (BSC5, `include_bytes!`, 9110 stars) as
  additive HDR point sprites, rotated into world space by latitude + sidereal time,
  fading in as the day-cycle sun sets; a companion HDR sun disc.
- Organized in the editor's **Environment tab** (was the floating 🌍 panel).

These are **not preset-captured** (they're per-display/world, like HDR/MSAA). The
**physically based atmosphere (#100)** is a third such layer — `Shared.atmosphere[8]`,
selected as `EnvSource::Atmosphere` (`doc/arch/render.md`); it lights the geometry via the IBL bake and
drives the terrain sky + aerial perspective. **#102** landed both halves as further
such layers: **A (volumetric clouds)** `Shared.clouds[12]` and **B (FFT ocean)**
`Shared.ocean[12]` (+ `ocean.rs`), both in the terrain pass. Roadmap in §1's papers: the
open **#101** (deeper night sky: moon / Milky Way / auroras).

---

## 11. Motion: the beat-driven system

One machine couples the renderer to music. All CPU-side; no shader changes.

- **PLL beat clock** (`advance_beat_clock`): a continuous `beat_pos` accumulator
  free-runs each frame at the active BPM (host when locked + playing, else the manual
  `tempo` slider) and gently corrects its phase toward the host's `pos_beats`
  (`PLL_TAU ≈ 0.12 s`). The plugin's `process()` writes `context.transport()` into
  `Shared.transport` + `tempo_sync`; **no animation math on the audio thread.**
  The **Rails generator (#187)** rides this clock directly — its rail coordinate
  `u = beat_pos`, so the ride keeps flying on the manual tempo when the
  transport stops, and musical structure is spatial structure by construction.
- **Auto-orbit camera** (`advance_camera` + `camera_path_offset`): a `CamPath` enum
  (Off / H-circle / V-circle / Figure-8 / Spiral); each beat crossing kicks angular
  velocity that damps off (momentum, not a metronomic spin). Manual drag/scroll remain
  an offset on top.
- **Pulse routing** (`apply_mod` + `mod_span`): two slots send the decaying beat
  envelope `e^(−phase·6)` to a selectable `ModTarget` param with bipolar depth, made
  musical per-target by `mod_span`. Active only while **Pulse** is on.
- **Speed Pulse** + **Breath**: a logarithmic kick to global speed (`10^(env·amount)`)
  and a universal pulse-driven uniform scene scale, both with their own attack/decay.
- **Audio reactivity** (`audio.rs`): the plugin analyses the input signal into band
  envelopes (`Shared.audio[8]`); `PulseSource` selects synthetic-beat vs. audio-bass as
  the pulse driver. *(Roadmap: a general feature→param routing layer — Part III §7.)*
- **Calibrated / analytical metering (#333 Tiers 1–2)** (`audio.rs`): a
  metrologically-honest measurement layer running beside the expressive `Analyzer`,
  everything defined against digital full scale / ITU-R BS.1770-4. **Tier 1** —
  `LoudnessMeter`: BS.1770 **K-weighting** (RBJ high-shelf + RLB high-pass biquads,
  any sample rate), sliding-window mean-square for **momentary (400 ms) / short (3 s)**
  LUFS, gated **integrated** loudness + **LRA** via loudness histograms, BS.1770 Annex-2
  **4× oversampled true-peak** (`TruePeak`, windowed-sinc polyphase), **stereo
  correlation** + L/R/M/S dBFS — all pre-allocated / RT-safe, fed one stereo frame at a
  time from `process()`. **Tier 2** — `CalibratedSpectrum`: **band power** (Σ|X|²,
  window-corrected so a full-scale sine reads 0 dBFS in its band) integrated into **IEC
  61260 fractional-octave** bands (1/1…1/12) **or a raw linear-FFT axis** (`SpectrumMode`),
  **A/C/Z weighting** (IEC 61672, `a_weight_db`/`c_weight_db`) and **fast/slow/peak-hold/Leq**
  averaging. Surfaced three ways: the measured scalars ride `Shared.audiometer[16]` → the
  visual's numeric HUD (`overlay::draw_hud`, gated by `meter_hud`); the calibrated **RTA bins**
  ride `Shared.audiospectrum[128]` (the measured frequency axis, for the Tier-3 in-world
  instrument); and the full meters + RTA also ride the lock-free `AudioViz`/`VizFrame` channel →
  the editor's **Audio tab** (see below). A separate lock-free **`ScopeRing`** captures the raw
  stereo waveform every block (always, independent of Audio Reactive) for the oscilloscope.
  Params: `meter_res` (`SpectrumMode`),
  `meter_weight` (`MeterWeighting`), `meter_averaging` (`MeterAveraging`), `meter_hud`;
  not preset-captured (transient display prefs). Off (no audio) → the HUD is off and the
  expressive path is byte-identical. Unit-tested against BS.1770 (a −20 dBFS 1 kHz sine
  reads the defined LUFS, an inter-sample over is caught, a full-scale tone calibrates to
  0 dBFS). *(Tier 3 — the in-world reference grid / instrument mode — is a follow-up.)*
- **Audio-driven dipole radiation (#248, Tier 1)** (`Shared.audiodip[8]`, Motion-
  captured, off by default): the analyzer's smoothed broadband **RMS level** rides
  `audio[5]`; with the drive on, the visual computes `drive = floor + amount·RMS`
  (`bin/visual.rs::audio_dipole_drive`) and scales the **Maxwell generator's source
  amplitude** by it — the lattice arrows' amp linearly, and `AnalyticField::Maxwell`'s
  `energy()` by `drive²` (E,B scale linearly, energy quadratically), so the whole #247
  energization stack (mote glow, Tier-3 fluid dye) breathes with the music's dynamics.
  The field-line *direction* is drive-invariant (physics), so advection never bends.
  Honest + declared: audio modulates the source's parameters; the rendered field math
  stays the real retarded radiation — the 20 Hz–20 kHz carrier is never rendered.
  **Tier 2 — spectrum → multipole content**: with the `multipole` toggle on, the five
  band envelopes drive **distinct multipole moments** — band b is realized as the
  textbook order-b linear multipole (an axial array of b+1 oscillating dipoles with
  alternating binomial weights, `math::maxwell_band_elements`), so the field's spatial
  shape encodes the spectrum through honest interference (the multipole expansion is
  the spherical-harmonic series). Two declared display choices: per-band wavenumber
  `k_b = k·(f_b/f_sub)^spread` (λ ∝ 1/f, compressed to stay watchable) and a per-band
  build-time normalization that removes the (kd)^b suppression so equal envelopes read
  comparably bright (angular structure untouched). Bands at distinct frequencies don't
  interfere time-averaged, so the mix's honest energy is the **sum of per-band
  energies** — which also gives per-band attribution for the **colour-by-band** tint
  (`band_hue_blend`: bass = the ember hue, highs pull ~⅔ around the wheel), applied to
  the band geometry (`maxwell_band_lattice_strands`/`maxwell_band_lines_strands`, which
  replace the point-source geometry while on), the ink energy dye, and the liquid ember
  glow. `AnalyticField::MaxwellBands` carries the stack to the mote grid.

---

## 12. Presets (`preset.rs`)

- **`PresetValues`** — the complete serializable param state (a serde mirror of
  `OrganicMathParams`, every field `#[serde(default)]`-friendly so old presets still
  load). **Don't pin the field count here** — this line said "~380" against an
  actual 1 174 (#618 T0b). The SessionStart structure watch reports it every
  session; `.claude/hooks/structure-drift-check.sh` computes it.
- **`capture(&OrganicMathParams)`** reads every `param.value()` into `PresetValues`.
  **`apply(&ParamSetter)`** recalls by setting every param **through the host's
  `ParamSetter`** — so recall is automation-recordable and undoable.
- **Store:** one JSON file at `~/Library/Application Support/OrganicMath/presets.json`.
  **Factory presets (#187 T3):** `preset::builtin_rails_presets()` (5 finished Rails
  rides) is seeded into the store **once** on load, guarded by a `seeded_rails_v1`
  marker file — deleting/renaming them is respected forever after.
- **What presets capture (#354 — now everything the UI shows):** all generator params;
  the **Environment** world layer (terrain, sun & day-cycle, atmosphere, clouds, ocean,
  starfield) + sky/IBL/backdrop/env-tint + the loaded **`.hdr`** reference; **Look**
  (materials, surface FX, particles, voxel, bio, membrane, RD, SSR/GI/glass); **Audio**
  (metering + calibrated instrument + pulse routing); the **Synth** engine; and
  **Settings** (HDR/MSAA/tone-map, render-scale, output framing/letterbox, sync/tempo).
  **Still NOT captured:** per-display quality with no "saved" meaning — TAA/temporal,
  path-tracer/RT-debug toggles, the capture **overlay/axes** decoration, and the two
  preset-timing dropdowns themselves.
- **Atomic recall (PR #110).** `apply()` sets every captured param one at a time on the GUI
  thread, while `process()` snapshots `to_shared()` every block — so the visual could
  render half-applied states (geometry before colour). Fix: an `apply_gen:
  Arc<AtomicU32>` **seqlock** — `apply_atomic()` bumps it odd before the apply and even
  after; `process()` skips publishing while odd and drops a snapshot that overlapped an
  apply. The visual gets the new look in **one atomic step**.
- **Recorded defaults (`Defaults`, #131).** A *separate* sparse overlay — param
  **id → normalized value** in its own `defaults.json` — distinct from the named
  presets. The editor's per-slider ⏺ records a value; the per-control ⟲ reset
  targets the recorded value when present, else the factory default. Reset-target
  only: a fresh instance still opens at factory, and **⟲ Reset All** stays a hard
  factory reset. (See §14 for the widget/identity wiring.)
- **The 7-way partition + Scene (#145, grown by #354).** `EditorTab` is now a
  **7-way** partition — Generator / Motion / Environment / Look / Audio / Synth /
  Settings. The first four (`EditorTab::SCENE`) compose a **Scene** (renamed from
  "Global"): `apply()` recalls exactly those four, so Audio/Synth/Settings are never
  touched by a Scene. Each tab also has its own list, letting you mix-and-match (recall
  a Scene, then swap a different Look or Environment on top). Recall of one tab applies
  only that tab's subset via **`apply_tab(tab, p, setter)`**, under `apply_tab_atomic`'s
  seqlock. The **field→tab partition is a single source of truth** — `for_each_tab_field!`
  in `preset.rs` tags every captured field with exactly one tab and drives `apply_tab`,
  the subset save, *and* the `tab_field_list` drift-guard
  (`param_table.rs::tab_partition_is_exactly_the_captured_fields`: partition == captured
  serde fields, no orphan/double-count). **WYSIWYG** — a param's tab = the UI tab its card
  is drawn under (the synth `sn_*` moved to a new **Synth** tab; sync/tempo → Settings;
  IBL/backdrop → Environment). Exceptions: the **Surface** params ride the Generator
  partition though the card sits on Look; and **two out-of-band fields**, `hdr_path`
  (Environment) and `model_path` (Generator), are file paths rather than params, so
  they cannot be `for_each_tab_field!` entries and are skipped from the
  partition/serialization when empty.
- **Out-of-band recall — the loaded `.hdr` and the loaded `.gguf`.** A
  preset captures both sidecar paths in `capture()` (GUI thread — they are file reads;
  `capture_params_only` leaves them empty for the audio-thread Key Map path), and
  `apply_recall` restores each by **writing its sidecar then bumping its counter** —
  `hdr_gen`, and `model_gen` in `Shared.mind[1]`, which `bin/mind_runtime.rs` and the
  visual edge-detect. `model_path` is what makes a GGUF view *"really just an Organon
  preset"*: without it a preset restored the Neural Network generator and every `nw_*`
  dial but left the specimen empty. **One function decides which recalls reach which
  field** — `preset::recall_redrives(scope, owner_tab, value)`: a `Global` (Scene)
  recall reaches an owner that `EditorTab::SCENE` contains, a `Tab(t)` recall reaches
  only `t`, and an **empty** value re-drives nothing. That last clause is a safety
  property for the specimen, not a convenience — a preset that could *clear*
  `model_path` would be a saved look that unloads a multi-GB model as a side effect.
  ⚠️ The **Key Map** `.hdr` follow (a MIDI-held Scene preset swapping the sky and
  swapping it back on release) deliberately has **no** model equivalent: a held note is
  the last place to start a multi-GB load. `param_table.rs`'s partition drift-guard
  drops both fields by name; adding a third is the decision, not the bookkeeping.
- **Subset (sparse) storage (#354).** `save`/`save_tab` write each entry's `values` with
  **only** that bucket's fields (`save_subset` filters by `field_names_for(tabs)`), so a
  Motion preset's JSON is Motion-only. Old full-blob files still load (dropped keys fall
  back to serde defaults; recall touches only the bucket anyway). Files:
  `presets.json` (Scene) + `presets_{generator,motion,environment,look,audio,synth,settings}.json`.
- **Beat-quantized recall (#354).** Two `PresetDivision` params in the Sync/Tempo card
  (`scene_preset_timing` + `component_preset_timing`; Instant / 1&frasl;4 / 1&frasl;2 /
  1&frasl;2&frasl;4&frasl;8 Bars — meta controls, not captured). The plugin publishes the
  absolute host beat to the editor via a `beat_pos: Arc<AtomicU32>` (f32 bits; `-1` =
  stopped); `presets_ui` schedules a Scene or Scene-component recall to the next boundary
  (`pending_recall`) and fires it via `apply_recall` when the beat crosses. Audio/Synth/
  Settings + Instant + a stopped transport recall immediately. The scheduling +
  boundary-fire logic is factored into `enqueue_recall` / `poll_pending_recall`
  (`lib.rs`) so the #356 controller drain schedules recalls through the exact same path.
- **Row UI (#354).** Per-row **R / D / U** (Rename / Delete / **Update** = overwrite to
  the current state). Update & Delete pop a Yes-defaulted confirm. The note→preset
  **Key Map maps to Scene presets only** (v1).
- **Four-Quadrant Performance Controller (#356 Tier 1, `controller.rs`).** A
  Launchpad-style 8×8 pad grid played as an instrument: each 4×4 quadrant drives one
  Scene component (Generator / Motion / Look / Environment) and each pad recalls that
  component's preset slot, beat-quantized on the Component-timing division. Gated behind
  the `perf_enable` param: **off** = the surface is inert (Key Map / synth / clip-CC map
  behave exactly as before); **on** = the controller **owns** the incoming notes/CCs —
  `process()` routes them ONLY to the mailbox, bypassing the Key Map (momentary, Scene-
  recalling), the Duo-Field synth, and the clip-CC map, so a pad press can't double-fire.
  The
  audio→GUI seam is the one genuinely new piece: `process()` pushes raw pad/button MIDI
  into a wait-free `controller::Mailbox` (in-process, **not** `Shared`); the editor drains
  it each frame (`perf_controller_drain`), routes each event through the serializable
  `PadLayout` (default = Novation Launchpad Mini MK3; note-**number** routed, re-capturable
  via the learn flow), and enqueues Pad→component / Scene→Scene recalls via `enqueue_recall`.
  Arrows page banks of 16 (◀▶) and step the division (▲▼); Stop/Solo/Mute cancels a pending
  recall. The editor draws a 4-quadrant mirror grid (dim = has-preset, bright = active,
  pulsing = queued) + a learn/diagnostic panel. `PadLayout` persists next to `keymap.json`
  (`controller.json`). Because the recall is a GUI-thread `ParamSetter` path, the plugin
  **editor window must be open** for the surface to drive recalls (the Key Map avoids this
  by writing `Shared` directly on the audio thread — but it's momentary + un-quantized).
- **Rotary knob bank (#448 Tier 1, `controller.rs` + `lib.rs`).** The knobs sibling of
  the pads: a Launch Control XL's **24 encoders (3×8)** drive **params** the way the pads
  drive presets, riding the same `perf_enable` gate + `Mailbox` + drain. A CC the
  `KnobLayout` claims (`knob_claims` arbitrates the LCXL-factory / Launchpad CC collisions
  on 19/29/49 — pads keep routed CCs until the knob channel is learned, then the knobs own
  their channel) resolves to a target param and is applied via the **raw `GuiContext`**
  (`raw_begin/set/end_set_parameter` on a `ParamPtr` from `Params::param_map()`) — a real
  host param set, so sliders follow, presets capture, hosts record automation (deliberately
  NOT the clip-CC override lane). Two modes (`KnobMode`): **Explore** = context-aware — the
  bank follows the editor's focus via `explore_knob_context` (Generator tab → the selected
  generator's contiguous `params.rs` block, addressed by wire-ID **anchor ranges** in
  `generator_knob_context`, clamped to 24, so new params auto-appear; Motion / Look /
  Environment tabs → curated 24-ID banks; Synth → the #339 Sound blocks); **Performer** =
  hand-assigned named **pages** of 24 param-ID bindings (Ableton-macro style, filterable
  picker in the card). **Pickup** (soft takeover, default on, `pickup_engaged`): a knob
  engages only when it reaches/crosses the param's current value, per-context (engagement
  resets on tab/generator/page switches) — recalls never make a knob jump. Learn flow =
  **twist all 24 in row-major order** (`learn_capture`; an encoder streams repeats, only a
  NEW CC advances; adopts the device channel). `KnobConfig` persists in `knobs.json`
  beside `controller.json`. No `Shared`/IPC change, no new host params — inert when idle.

---

## 13. MIDI clips & Key Map (`clip.rs`, `keymap.rs`)

Two ways input reaches the visual **without** going through the host param layer
(remember: the plugin can't set its own params from the audio thread):

- **MIDI clips (`clip.rs`).** A per-param CC map (**CC 16–47**, 32 slots; CC base 16 to
  avoid mod/volume/pan/sustain). `apply_normalized`/`normalized` map a CC value to/from
  a `Shared` slot. The plugin's `process()` reads incoming CC → fills `cc_override` →
  stamps it into the snapshot. **Last-touched-wins:** if a slider moves (its normalized
  value changes > 1e-4), that param's CC override is released. The "Release MIDI clip"
  button clears all overrides. `.mid` **export** (`clip.rs::write_midi`, a CC burst + a
  held note) is **dormant**: the per-preset export button was removed from the presets
  rail as unused; the writer is kept behind `#[allow(dead_code)]` in case it returns.
- **Key Map (`keymap.rs`).** Maps MIDI notes → presets. A GUI-edited `KeyMapping`
  (`BTreeMap<note, preset name>`) is compiled into a `KeyMap` of pre-resolved `Shared`
  snapshots, published to the audio thread wait-free via `ArcSwap`. A held note's preset
  **wholesale-replaces** the snapshot (highest priority), preserving the per-display
  fields (HDR/MSAA). `HeldKeys` is a last-press-wins stack.

**Priority cascade (low → high):** sliders < MIDI-clip CC overrides < held-key preset.

**Audio-thread discipline:** `process()` is allocation-free — pre-allocated
`HeldKeys`, lock-free reads via `ArcSwap` + atomics (`release`, `hdr_gen`, `apply_gen`,
`active_note`).

---

## 14. The editor UI (`lib.rs`)

> **The look lives in `theme.rs` (#542 Tier 1), not here.** Every colour, font size, corner
> radius, and control-row width the editor draws resolves through that module: the warm
> palette tokens (`doc/organon_mind_visual_reference.md` §1 — warm near-black, *never*
> blue-black), the Inter type ramp installed once per `egui::Context`, `card_frame` /
> `card_title` / `hairline`, and the pure `row_grid` / `combo_grid` partition that `srow`
> and `param_combo_sized` lay every row out against. Change the look there; `lib.rs` owns
> *what* is drawn, `theme.rs` owns *how it reads*.
>
> Two rules that module encodes and that are easy to undo by accident:
> **(a) the accent is scarce** — `theme::AMBER` marks live state only (streaming, meter
> mid-scale, the active key); structural headings are `theme::BONE`, because an accent on
> all 112 card titles at once is not an accent. **(b) the label yields before the bar
> does** — `row_grid` makes the label segment elastic (132 pt down to the historical 62 pt
> floor) so a narrow column costs label characters rather than driving the slider to an
> unaimable 24 pt. Both are unit-tested; see §15's note on what tests can and cannot say
> about a UI.
>
> **Full Organon's** editor renders through **`egui-baseview` → `egui_glow` (OpenGL)**, while
> the visual is **wgpu in a separate process** — which is why the 3-D scene cannot be drawn
> inside *that* editor window: in-window chrome is painted with `epaint` meshes/shadows rather
> than shaders, and its viewport is the #554 T1 CPU frame mirror.
>
> ✅ **Organon Mind is no longer that**, as of #593. Its editor is `wgpu_editor.rs` —
> `World::render_into` and `editor_ui` on one device, in one window — and the mirror is gated
> out of the edition entirely. This is the **default** since the gate that armed it through the
> build-out was inverted at close-out; `ORGANON_EDITOR_WGPU=0` is the bring-up fallback, and it
> lands you in an editor with no viewport at all. Under #617 Tier 1 the scene is either a
> bounded pane (workstation, the default) or the whole window (immersive); in immersive the
> central region's frame is transparent (`theme::workspace_frame`), so what is behind the
> interface is the scene itself rather than a picture of it.
>
> ⚠️ **This paragraph used to add "there is no `egui-wgpu` in the tree", and that is no
> longer true** — #554 T4 vendored `egui-wgpu` 0.33.3 onto wgpu 30 (`vendor/egui-wgpu`).
> The remaining gap is not the renderer, it is the **window**: `nih_plug_egui` owns the
> one the host hands us and gives it an OpenGL context.
>
> **#593 (which supersedes #542's viewport tiers and #572) is the path out**, via "route
> C": our own `nih_plug::editor::Editor` that builds a **wgpu surface on the parent
> view**, keeping nih-plug's wrapper as the owner of the params so `ParamSetter` stays
> real. **Tier 0 of it — `editor_probe.rs` — is in the tree**: parent handle → rwh 0.5 →
> parented baseview window → rwh 0.6 → `wgpu::Surface` → clear to a cycling colour and
> present. It **compiles and tests green in both editions**, and carries no
> `cfg(target_os)`, so the AppKit arms are compiled and unit-tested here rather than merely
> assumed; whether a frame reaches a screen in a parented `NSView` is a Mac question and is
> not yet answered. See §19 and the module docs. Tiers 1–4 (extract the editor body → the
> real custom editor → the baseview input path → collapse Mind to one process) are not
> built.

> **Where the editor body lives (#593 Tier 1).** It is a **top-level function**, not a
> closure: `pub(crate) fn editor_ui(&EditorCtx, &egui::Context, &ParamSetter,
> &mut preset::PresetUi)` (crate-visible, not `pub` — Tier 2's host is returned from
> `Plugin::editor` so it lives in this crate anyway, and three of the types involved sit in
> private modules that going `pub` would have forced open).
> `Plugin::editor` gathers the 43 `Arc` handles the body needs into an `EditorCtx` (one
> field per handle, same names, same order) and hands `create_egui_editor` a one-line update
> closure that forwards to it. `editor_ui` re-materializes those 43 handles as locals under
> their original names and types before the body starts — 43 `Arc` clones per repaint,
> atomic increments next to a full UI pass — which is precisely what let the 4 316-line body
> come across **unedited**. The point is that a **second host can call the same code** —
> #593 Tier 2's custom wgpu `Editor` for Organon Mind draws the identical interface instead
> of growing a parallel one that drifts. It deliberately stays in `lib.rs`: the body calls
> into the 92 private free functions the file defines (`srow`, `crow`, `card`,
> `fixed_columns`, `param_combo`, the `pick_*_async` dialogs, …), so moving it to its own
> module would drag a large incidental diff through a change whose whole value is being
> mechanically verifiable. That verification is real and repeatable —
> `native/tools/check-editor-extract.py` diffs the hoisted body against the pre-hoist closure
> at any base commit, normalizing only the uniform dedent plus a declared (currently empty)
> rename set, and fails on anything else. **Do not reflow that region**; run the script after
> touching it.

nih-plug-egui. **Top-level tabs (#131, grown to five)** — a `selectable_value` tab bar
selects one of six sections (`PresetUi.tab: UiTab`, default Generator), each laid out in
the same **fixed-width 3-column card grid** (`fixed_columns`): three equal columns fill the
tab width edge-to-edge (floored, min `CARD_COL_MIN_W` = 280px) — a small reimplementation
of egui's own `columns_dyn` whose column width depends only on the window size, never on
content, so cards keep a stable width and never reflow when a slider readout changes
length (the groundwork for later drag-to-rearrange cards). The presets rail is a fixed
150px `SidePanel` (its per-preset `.mid` export button was removed — unused; the
`clip.rs` writer is kept dormant). The tabs, in display order:
- **Generator** — the Generator selector + the active generator's param cards.
- **Motion** — Animation, Camera, Pulse, Speed Pulse, Breath.
- **Environment** — the world layer (was the floating 🌍 panel; `environment_ui`): Terrain
  + Atmosphere & Water / Sun & Day Cycle + Atmosphere (physical sky) + Volumetric Clouds /
  FFT Ocean + Starfield, one column each land/sky/sea.
- **Look** — column 0: Surface (moved from Generator; still in the Generator preset
  partition), Material, Surface FX, Lighting (Direct), Environment (IBL); column 1:
  Cast Shadows, AO, Reflections, GI, SSGI, Voxel GI, Bioluminescence, Reaction-Diffusion;
  column 2: Post FX, Temporal, Particle Aura, Bloom. (The quick-toggle "Environment /
  World" card was removed — the Environment tab owns the world layer now.)
- **Audio** (#333, was the floating 🎵 panel; `audio_instrument_ui`) — the performance
  instrument, three columns: **Levels & Loudness** (Audio-Reactive enable + input VU +
  BS.1770 calibrated meter grid + analysis tuning), **Spectrum** (the FFT display + band
  envelopes + the calibrated RTA bar graph + spectrogram + res/weight/avg combos), and
  **Oscilloscope** (the s(M)exoscope-style scope: `audio_scope` reads the lock-free
  `ScopeRing` and does all scope processing GUI-side — TIME/AMP zoom, Free/Rising/Falling/
  Internal trigger + level + retrigger hold-off, sync-redraw, freeze, DC-kill, L/R/Mid) +
  Pulse Routing. Repaints continuously while open so the meters + scope animate.
- **Settings** — infrequently-changed per-display plumbing: Renderer (HDR/MSAA/tone map) +
  Output Resolution (moved from Look) in column 0; the capture / production-frame stack
  (was the floating 🎬 panel; `capture_ui`) + Overlay text in column 1; the Sync / Tempo
  card in column 2.

**`UiTab` vs `EditorTab`.** The tab bar is the 6-way `UiTab`; the per-tab preset system
stays built on the 3-way `EditorTab` partition (`UiTab::preset_tab()` maps
Generator/Motion/Look across; Environment/Settings/Audio → `None`). Environment + Settings hold
per-display / world state that presets don't capture, so they have no per-tab preset list —
the presets rail hides the tab option and falls back to Global while one of them is active.

The generator-mode classification (`gmode`/`raymarch`/`kifs`) is hoisted above the tab bar
(both the Generator and Look tabs read it). Plus a presets rail + floating panels (🎵 Audio,
Key Map) that overlay regardless of tab. Sliders mutate params through
nih-plug; the param change flows out via `to_shared` → IPC.

**Control widgets (#131).** Two row helpers, chosen by param kind: `srow` (numeric
`ParamSlider`) and **`param_combo`** (a `egui::ComboBox` dropdown for any discrete/enum
`Param` — walks `step_count`, labels each step via `normalized_value_to_string`, sets
through `set_parameter_normalized`) at a **fixed `COMBO_W` (75px)** width — the combo
renders inside an exact-size clipped child UI so truncation happens at 75px and even the
longest selected label cannot widen the button; **every Generator-tab combo** runs at 2×
via `param_combo_sized`, eating only the row's flexible gap (Motion/Look-tab and
floating-panel combos stay at 1×). Every row ends in the same
right-aligned **three-button group**: the merged **default button** (`default_btn` —
click = ⟲ reset to the recorded/factory default; **hold ⌘ and click = ● record** the
current value as the default, the glyph flips while ⌘ is held; enums keep a plain ⟲, no
record — #131) plus **two inert placeholder buttons ("1"/"2")** reserving the layout for
upcoming per-row actions (modulation routing etc.). The slider cluster takes 2/3 of the
width it used to fill; the remainder is a gap before the buttons. **Card descriptions**
are compressed behind a small **"?" (`help`)** at each card's bottom-left: hover = the
text as a tooltip, click = pins it open as an in-card bubble (click the ? or the bubble
to collapse); the old always-visible weak/small paragraphs are gone. **Every `srow`
segment is a fixed width** — `label (LABEL_W, ellipsized) | bar | value (VALUE_W) | gap |
buttons` — so rows land on identical grid lines: the bar is a `ParamSlider` rendered
`without_value()` inside an exact-size clipped child UI (`ParamSlider` sizes parts of
itself to content, so `add_sized` alone can't cap it), and the readout is **`value_box`**,
our own constant-width widget (58px) with click-to-type editing (Enter commits via
`string_to_normalized_value`, gesture-wrapped; Esc/click-away cancels; edit state keyed by
the param's pointer hash). Each `fixed_columns` column is additionally clipped to its own
horizontal strip, so nothing can paint across a column seam. Every float slider's readout uses the
**value-aware formatter** (`params.rs::v2s_va`, applied at the single `flin` builder):
decimals scale down as magnitude grows (≥1000 → 0, ≥100 → 1, ≥10 → 2, else 3), trailing
zeros trimmed — every readout is ~6 characters max and always fits the `VALUE_W` reserve. An open
`param_combo` is **keyboard-cyclable**: ↑/↓ live-apply the adjacent variant (the look
scrubs as you move) and Enter commits + closes — the keys are consumed inside the open
popup so egui's focus-nav doesn't also move, and only the (single) open popup reacts.
Every enum
(Generator, funcs, Surface mode, Palette, Material, Tone-map, MSAA, Cam path, Pulse
source, routing targets, per-generator family/form/view enums, …) uses `param_combo`;
`crow` is the `BoolParam` checkbox. **Context-aware panels:** the editor reads the active
`GeneratorMode` and hides cards that don't apply — the **Surface** card for the node-free
raymarch generators (Mandelbulb, KIFS, Lens, and Minimal-surface's *implicit* TPMS families).
Minimal-surface is dual-path (#127 P2): its **parametric** Weierstrass families emit a Grid
that Surface modes skin, so the Surface card stays visible for them (`ms_parametric` gate).
The node/PBR-surface look cards (AO, Material, SSR, GI, Surface FX, Bioluminescence,
Reaction-Diffusion) are hidden for **KIFS** specifically (a self-contained fullscreen colour field).
**Audio/pulse grouping:** the Pulse card is tempo-only; the pulse-source selector + Pulse
Routing slots live in the 🎵 Audio panel.

**Record-as-default (#131).** The ⏺ on a numeric `srow` records that param's current value
as its default into the `preset::Defaults` overlay (§12); ⟲ then resets to it (else factory).
A widget only has `&Param`, so to key the overlay by the param's stable nih-plug **id** without
threading an id through ~200 call sites, a GUI-thread-local maps `hash(ParamPtr) → id` (built
once from `params.param_map()`, rebuilt on a params-instance change) and the widget recovers its
id via `Param::as_ptr()`. The store flushes to disk once per frame if a ⏺ changed it. Enums
(`param_combo`) get no ⏺. *(#131 is complete with the top-level tabs above; the decade-nudge
idea + fresh-instance-applies-defaults were dropped/deferred.)*

---

## 15. Build, run, test, deploy

```bash
source "$HOME/.cargo/env"            # rustup toolchain
cd native
cargo build --release
cargo run --bin organic-math-visual --release      # the visual alone
cargo run --bin organon-standalone --release  # the editor (sliders)
cargo test                                          # math + naga WGSL + layout goldens
./bundle.sh                                         # → target/bundled/Organon.{vst3,clap}
./deploy.sh                                         # Mac-only: build + bundle + install to ~/Documents/vst3
./verify.sh                                         # GPU: drive the visual, snap frames, diff vs goldens

# Organon Mind — a SECOND build. `mind-edition` is default-off, so none of the
# lines above compile it: you can break Mind and still see a green suite (§4.1).
cargo build --release --features mind-edition --bin organon-mind
```

**The offline test bar** (what a Linux/remote session can verify): `cargo build` +
`cargo test`, **plus the mind-edition build above**. Tests cover the **pure math**
(every generator's geometry, the compose-step, GI probes, the fluid projection oracle),
the **param-table layout goldens** (§7), and **offline WGSL validation**
(`tests/wgsl.rs` parses + validates every shader with naga — catches
binding/type/uniformity errors without a GPU). Five suites: **lib / visual / ctl /
popup-contract / wgsl**, roughly a thousand tests in total.

> **`STATUS.md` carries the current count and per-suite split, re-measured per
> handoff — read it there, not here.** A pinned number in this file is a number nobody
> re-measures: §7 carried "~737 params" for months against an actual 1 372, and this
> paragraph said "206 lib" against ~800. State the *shape* here, keep the *count*
> in the volatile doc.

**The frame bar** (`native/verify.sh`, see `native/verify/README.md`): everything above is
green-without-looking — it says nothing about the pixels. `verify.sh` closes part of that
gap *mechanically* on any machine with a GPU. It launches the visual on a **private IPC
namespace** (so it is safe alongside Organon in Ableton), drives it through the `organon`
CLI, snaps frames, and runs three kinds of check per scene: **`nonblack`** (it launched but
drew nothing), **`animates`** (two snaps, zero input, must differ — the only check a frozen
redraw loop fails), and **`golden`** (pixel diff vs a committed reference, via
`examples/imgdiff.rs`). It gates on `diff_frac` — the fraction of *pixels* differing by more
than 2% — because that is what catches a local layout shift; `mean_abs` barely moves when an
egui row shifts 64 pt, which is the #545 class. Output: `target/verify/report.md` +
`summary.json`. Goldens are re-baselined with `--update-golden`, and **a golden update
belongs in its own PR description** — one buried in an unrelated diff is how a real
regression gets laundered into the baseline.

**What cannot be verified without the Mac:** the Ableton integration, MIDI/clip behaviour,
true-HDR EDR output, `CAMetalLayer`/EDR headroom, projector-res performance — and the
judgment call of whether a *new* look is the intended one (a golden proves the render did
not change; it cannot prove it is right). Note the split `verify.sh` introduces: crash,
black-frame, frozen-clock and look-regression classes need **a GPU**, not necessarily *this*
GPU, so they can run on Linux/Vulkan CI; the list above needs **macOS/Metal specifically**.
The standing rule (`deploy-native-build`): **deploy after every native change** via
`native/deploy.sh` on the Mac (it builds + bundles + ad-hoc-signs + installs, then
reminds you to Rescan). Self-built plugins are Gatekeeper-blocked — disabled via
`sudo spctl --global-disable`.

**`deploy.sh` also installs the #226 network gallery** — it copies
`native/assets/networks/*.json` (the connectome / MLP / attention demos) into
`~/Library/Application Support/OrganicMath/networks/`, the dir the "Load Network
(JSON)…" dialog opens at (`preset::networks_dir()`). The gallery is a set of repo
files, **NOT embedded in the `.vst3` bundle**, so it must be **re-deployed whenever
the gallery changes** (add/regenerate a file → `./deploy.sh` to reinstall). The copy
is idempotent, so a normal deploy keeps the installed set in sync; regenerate the
files with `python3 native/assets/networks/generate.py`.

---

## 16. Conventions & gotchas

- **Append-only `Shared`.** Never reorder/insert fields; append, and document the slot
  layout in the struct comment. Incompatible change → bump `ipc::LAYOUT_VERSION` + rebuild both.
  After any growth, close+reopen the visual and Rescan.
- **`ModTarget` indices are wire-stable** — only append variants.
- **Branch off `main`** for each feature; merge back to `main`. **Stacked PRs are a
  recurring foot-gun** (PR #11, #20 stranded children on dead bases) — prefer
  non-stacked PRs, or merge the base first and tick "delete branch" so GitHub
  auto-retargets the child.
- **`.mid` clips that set a *function* don't map 1:1** across versions (the func CC
  range grew 0..3 → 0..6); regenerate saved clips that pick a function.
- **A Linux/cloud session CAN build and test the crate.** (This bullet used to say the
  `nih-plug` git dep could not be fetched remotely; that is no longer true — `cargo
  fetch` succeeds.) What it needs is the **system dev headers** first — ALSA/JACK for
  nih-plug's standalone backends, X11/GL for baseview — or the build dies inside a
  *build script*, which reads like a code error and isn't. `CLAUDE.md` carries the
  `apt-get` line. What still needs the Mac is the *look*, not the compile.
- End commit messages with the `Co-Authored-By` trailer (see `CLAUDE.md`). Only
  commit/push when asked.

---

## 17. Extension guide — how to add things

> The whole architecture is designed so that **adding a natural law costs almost
> nothing**. Here are the integration patterns.

**Add a node-field generator** (the common case):
1. Write the pure math in `math.rs`: a `*_strands(...) -> (Vec<Strand>, Topology)` fn +
   unit tests (geometry is the bar). Tint via the palette helpers.
2. Add a `GeneratorMode` variant (+ `from_u32`/`to_label`) and a param block in
   `params.rs`; append its `Shared.<gen>[]` array in `ipc.rs`; add the
   `param_block!` packer in `param_table.rs`.
3. Add the dispatch arm in **`world.rs`** (build strands → `lower_strands`; membrane
   loft if Grid). ⚠️ This moved in the #572 world hoist — `bin/visual.rs` now owns only
   the window and swapchain and holds **no** generator match arms.
4. Add the editor card in `lib.rs` (gated on the active generator), the
   `PresetValues` fields + `capture`/`apply` in `preset.rs`.
   Everything downstream (surface/material/light/post/camera/beat) is inherited.

**Add a per-pixel/raymarch generator** (like Mandelbulb/KIFS): same param/preset
wiring, but instead of strands add a `RenderPath` variant + a fullscreen/DE shader +
pass; gate the instanced/SSAO/early-Z paths off (see `mandelbulb.rs`/`kifs.rs`).

**Add a parameter:** declare the nih-plug field (`params.rs`), the `PresetValues` field
+ `capture`/`apply` (`preset.rs`), and **one** `param_block!` slot (`param_table.rs`) —
the packing for both `to_shared`s is then generated. Append to an existing `Shared`
block or a new one. The layout-golden test will fail until you re-pin it (intentional).

**Add a render subsystem** (a new world layer, post effect, or light-transport pass):
add a module + shader (naga-validate it), thread it through `RenderFrame`'s relevant
sub-struct (`Background`/`Surface`/`LightTransport`) — **not** a new positional arg —
and a pass in `render.rs`. Add params/preset wiring as above (or leave it a global
display layer if it's per-display like terrain/HDR).
📖 **Read [`doc/arch/render.md`](doc/arch/render.md) first** — it has the pass order, the
`RenderFrame` sub-struct contents, and where your pass slots in. It is not auto-injected,
so nothing will put it in front of you.

**Add a stateful generator or mode** (the pattern is realized by `BoidsSim` and
`BellSim`): hold the sim state as a field on the visual's app struct; the match arm
calls `step(dt)` (fixed-dt substeps, stored seed for determinism) then `emit`s strands.
No trait change, no IPC growth. See the Boids arm (id 12) and the Harmonic "Physical"
mode for the two existing templates.

---

## 18. The web surfaces — brief

**Two web apps, and they are not the same project.**

**`web/` — the WebGPU port (#418). ⏸ PARKED as of 2026-08-04 (#626 §1.4).** Development
is **Rust-native only**; the code is **kept, not deleted**, and `web/ARCHITECTURE.md` is
deliberately left un-edited because it is accurate about a parked program — which is what
makes resuming cheap. Two consequences worth knowing before you touch anything here:

1. **`web/` is not built or tested by CI.** `.github/workflows/ci.yml` is native-only and
   carries `web/**` in `paths-ignore`. Verify: `grep -n "web/\*\*" .github/workflows/ci.yml`
2. **Whether `web/ARCHITECTURE.md` is still SessionStart-injected is decided by
   `.claude/settings.json`, not by this line.** #626 Tier 2 removes that registration; the
   script is kept, so it is one entry to put back. Don't trust this sentence for the
   current state — ask the file:
   ```bash
   grep -c 'load-web-architecture-doc' .claude/settings.json   # 1 = injected, 0 = not
   ```
   *(Written this way on purpose. The first draft asserted "no longer injected" as settled
   fact while the change was still unmerged on a sibling branch — the exact defect §6 of
   this same change is fixing. A pointer to the authority survives the merge in either
   order; a claim about state does not.)*

Either way the **Stop-side reminder still fires** if you change a web contract, so the
same-change discipline is intact. Everything below describes it as built. Raw WebGPU with
React for UI only,
running the *same* `math.rs` compiled to WASM (`native/organon-wasm`) and the *same*
WGSL ported nearly verbatim, with the param manifest codegen'd from `params.rs`
(`native/organon-manifest`). That reuse is the design: parity holds by construction
rather than by hand. **`web/ARCHITECTURE.md` owns it** — contracts, the WASM bridge,
the renderers, its own build/verify commands. Do not describe it here.

**`/src` — the legacy React-Three-Fiber app.** The original public artifact, kept
running and **untouched until `web/` reaches parity**. R3F v8 + drei + three 0.169,
Vite + TS (strict), Leva controls; `InstancedMesh` with a `paramsRef` mutated by Leva
transient `onChange` and read in `useFrame` (no re-render on slider drag); cubes are the
classic RGB colour cube with a shader patch making the emissive glow per-vertex. Key
files: `src/scene/CubeField.tsx`, `Scene.tsx`, `Lighting.tsx`, `src/math/transform.ts`,
`src/presets.ts`, `src/ui/controls.ts`, `src/App.tsx`. **It still runs the old
algorithm** (loop-step + `angle_inc` + no base grid + a node cap) — see §3.

**`site/` and `site-mind/`** are static pages (organon.art, organonmind.org), not apps;
each has its own `README`.

---

## 19. File map (quick reference)

> Every `.rs` in `native/src` has a row here — that is the point of the table, so add one
> when you add a file. For the **render-side** modules (`render.rs`, `post.rs`, `env.rs`,
> `rt_*.rs`, `world.rs`, the shaders) the row says *what it is*; **`doc/arch/render.md`
> says how it works**.

### 19.0 The crate map (#626 Tier 3)

`native/` is a **cargo workspace**, not one crate. Several members carry engine or product
code (`xtask` is the VST3/CLAP bundler and is not part of the engine). Read the roster off
`native/Cargo.toml`'s `members` list rather than counting this table — the sentence that
used to state a number here went stale twice:

| Crate | Path | Depends on | What it is |
|---|---|---|---|
| **`organon-core`** | `native/organon-core` | `memmap2`, `half`, `glam`, `bytemuck`, `serde`, `serde_json` | the **host-free spine**: `math`, `ipc`, `params`, `gguf`, `gguf_data`, `edition`, `tabs` |
| **`organon-mind`** | `native/organon-mind` | `organon-core`, `egui`, `bytemuck`, `memmap2` — **and nothing else** | **T4** — the interpretability instrument: the activation ring, Mind UI, model shell. **No nih-plug.** |
| **`organon-render`** | `native/organon-render` | `organon-core`, `wgpu`, `glam`, `bytemuck`, `image`, `half` | **T4** — the renderer: `render` + its 36 surface submodules, plus `axes`/`chamber`, **`legibility`** (PBR text T2 — the CPU harness, no wgpu in it) and **50 shaders**. **No nih-plug, no egui, no winit.** |
| **`organon-scene`** | `native/organon-scene` | `organon-core`, `glam`, `bytemuck` | **organon#49 T3** — the **substrate**: `substrate_scene` / `substrate_materials` / `substrate_camera` / `substrate_epochs` + `overlay_meta`. Scene *state*, not drawing. **No nih-plug, no wgpu, no egui, no winit.** |
| **`organon-agent`** | `native/organon-agent` | `organon-core`, `serde`, `serde_json` — **and nothing else** | **organon#49 T4c-i** — the **AI Performer**: action set, override lane, actuation vocabulary, tool-call protocol, localhost chat client. **No nih-plug.** ⚠️ `core_catalog` and `scene_features` did *not* come — they read `param_table` / `preset`, so `src/agent.rs` is a host adapter over this crate |
| **`organon-world`** | `native/organon-world` | `organon-core`, `organon-mind`, `egui`, `memmap2`, `bytemuck` — **plus, behind the `world` feature**, `organon-render`, `organon-scene`, `organon-agent`, `wgpu`, `winit`, `glam`, `half`, `image`, `dirs`, `serde_json`, `rfd`, `ab_glyph`, `egui-wgpu`, `egui-winit` | **organon#49 T4b + T4c-ii** — the **window layer and the world**. T4b: `scene_input` / `egui_platform` / `frame_ring` / `audio_ring`, always compiled. T4c-ii: **`world`** (13.5k lines) and its nine `#[path]` submodules (`capture`, `overlay`, `rt`, `metal_island`, `gpu_timer`, `recorder`, `snap`, `ui_layer`, `winit_platform`) behind the **default-off `world` feature** — which is what keeps the +490 KB out of the shipping cdylib now that this crate is an *unconditional* dependency of the plugin crate. The one bar it holds is **no nih-plug**, and it holds it *with* `world` on |
| **`organon-visual`** | `native/organon-visual` | `organon-world` (`world` on), `organon-core`, **`organic-math-native`**, `wgpu`, `winit`, `pollster` | **organon#49 T4c-ii** — one package for one binary: **`[[bin]] organic-math-visual`** plus `hdr_macos` / `hdr_windows` / `launch_macos`. ⚠️ **The only crate here that depends UPWARD on the plugin**, and deliberately: the visual runs the AI Performer's worker, so it needs `agent::core_catalog()`, which reads `param_table`. It exists because the binary could go neither down (loses the catalog) nor stay (cargo features unify across a package's targets, so it would hand the cdylib the `world` feature). **GPL-3.0-or-later**, inherited from that dependency — harmless, since Console never launches the visual |
| **`organon-console`** | `native/organon-console` | `organon-core`, `egui`, `serde`, `serde_json`, `dirs`, `portable-pty`, `alacritty_terminal` | Console #3 T1 — the **compositor UI** for Organon Console. **No nih-plug, permanently** — it is the one crate whose bar is a lifetime commitment rather than a boundary |
| **`organon-glyphs`** | `native/organon-glyphs` | `organon-core`, **`ttfx`** (git, pinned by rev — not on crates.io), `clap` | **organon#217 T1** — the **glyph-ring producer**: `[[bin]] organon-glyphs` runs a `ttfx` text effect headless under its virtual clock, walks the cell grid out of the engine each tick and publishes it into `glyph_ring` (`doc/pbr_text_engine.md` §6.1). The `organic-math-mind-writer` shape, as its own package **so the ttfx dependency touches no existing crate** (features unify across a package). ⚠️ `clap` is the third dependency §6.1 did not name, and it is forced: ttfx's effect configs are clap `Args` with attribute-only defaults and ttfx does not re-export clap, so an effect cannot be built by name without it. **T11:** `persist.rs` — phosphor persistence, a per-cell decay **in linear light** applied to the walk before it is published (`--persist-ms`, default 0 = off and byte-identical; trails carry `SGR_PERSIST`). **No nih-plug, no wgpu, no egui.** `MIT OR Apache-2.0`; `NOTICE` carries both lineages' credits |
| `organic-math-native` | `native/` | every sibling above **except `organon-visual`**, which depends on *it* | everything else — the plugin, the standalone, the editor, the `organon` CLI. ⚠️ **No longer the visual, and no longer `world.rs`** (organon#49 T4c-ii) |

**⚠️ `organon-render` is `world::render`. `world.rs` is NOT part of it — it went to `organon-world` in organon#49 T4c-ii, not here.**
The world is the *app state* — agent chat client, CLI protocol, docks — and its couplings
are host-side, so extracting it is **#618's `World` decomposition**, not #626's. With it
set aside the renderer needed one upstream module (`organon-core::math`) and none of the
six enum splits #626 specced. `doc/arch/render.md` carries the full reasoning, the
`axes`/`chamber` sideways-reference trap, and the double-compile this collapsed.

**⚠️ The dependency direction, recorded because it reads backwards.** When
`organon-render` lands (Tier 4's remaining half), **it will depend on `organon-mind`, not
the reverse.** The visual *builds* the `NeuralGraph` and draws it, so the renderer is the
consumer of Mind's data. #536 T5 asks for this in writing precisely because the intuitive
reading — "the visualiser is upstream of what it visualises" — is wrong, and a later
well-meaning fix would create a cycle cargo then rejects outright.

### 19.0.1 Cross-crate churn — #626 §2.4's deferred measurement

**74% of merges that touch a crate touch more than one** — 73.6% (203/276) at `7e19bc8d`,
400 first-parent merges, `native/tools/crate-churn.py`. **The window slides**, so a later
reading that differs is drift, not disagreement: re-run it rather than trusting this line. #626 §2.4
reads ≳30% as *"still churning too hard to expose a stable API"*, so **the answer is: do
not split repositories** — one repo, crates to crates.io.

**It will not decay on its own.** 96% of the dominant pair touches a param-chain file:
invariant #3's `params.rs` → `ipc.rs` → shader now spans three crates by construction.
**`doc/arch/topology.md`** carries the full reading, the two ways to get a plausible wrong
number out of the script, and Tier 5's open questions.

**The invariant, and the command that checks it:**

```bash
cargo tree -p organon-core     # must contain NO nih_plug, NO wgpu, NO egui, NO winit
cargo tree -p organon-scene    # same bar, one layer up (organon#49 T3)
cargo tree -p organon-agent    # organon#49 T4c-i — holds the FULL bar and cheaply:
                               # its only deps are organon-core, serde and serde_json
cargo tree -p organon-world --features world   # organon#49 T4c-ii — the bar has to be
                               # checked WITH the feature on, or it says nothing about the
                               # world. Bare (T4b) it is a much smaller crate.
cargo tree -p organon-world    # organon#49 T4b — only the nih_plug half applies:
                               # this crate carries egui deliberately
```

That is the tier's acceptance test. It is meaningful only while core's dependency list
stays tiny — one `egui` line and the check passes forever while proving nothing.

🚨 **Test the workspace, not the root package.** `native/Cargo.toml` is a **root package**
with members, so a bare `cargo test` runs `organic-math-native` **only**:

```bash
cargo test --workspace                            # default edition
cargo test --workspace --features mind-edition    # Mind edition
```

This is not style. When Tier 3 extracted core, the bare command silently stopped running
core's **44 tests** — the suite reported **1146 → 1102 and stayed green**, which is how a
coverage loss hides in plain sight. Among the 44 is the **IPC namespace-pinning test**, the
guard on the one cross-product invariant (§4.1). `ci.yml` carries `--workspace` on both
legs for this reason.

**Every future crate extraction has this failure mode.** `--workspace` covers members added
later; a hand-maintained `-p` list would need updating in the same change that adds a crate,
which is precisely the discipline that fails.

> 📌 **Core is not *dependency*-free, it is *host*-free.** An earlier draft of this section
> claimed `[dependencies]` was empty. It isn't: `gguf_data.rs` reaches `memmap2::` and
> `half::` via inline fully-qualified paths, which a scan of `use` lines does not see, and
> the compiler said so immediately. Both are pure data crates. The invariant above is the
> real one and was never weakened — but "empty" was a stronger claim than #536/#626 ever
> made, and stating it here would have been a number nobody could reproduce.

**Two decisions recorded so they are not re-derived:**

1. **`FuncName` moved in PR B; `GeneratorMode` did not.** See §19.0.1.
2. **The main crate re-exports core's modules rather than rewriting ~60 call sites.**
   `lib.rs` carries `pub use organon_core::{edition, gguf, gguf_data};` and `preset.rs`
   carries `pub use organon_core::tabs::{EditorTab, UiTab};`, so every existing
   `crate::gguf::…` path still resolves. **Named, never glob** — a glob would let core
   silently widen the main crate's surface.

   The reason is not convenience: **Tier 4 moves `gguf`/`gguf_data` again**, out of core
   and into `organon-mind`, once the lens builders leave `math.rs` (#536 T4 reference #1).
   Rewriting 19 `crate::gguf` sites to `organon_core::gguf` now and to `organon_mind::gguf`
   next tier is one churn paid twice. The facade absorbs it. When Tier 4 runs, **expect
   `gguf`/`gguf_data` to leave core** — that is the plan, not a regression.

**Why `EditorTab` moved when only `UiTab` was specified.** `UiTab::preset_tab()` returns
`Option<EditorTab>`, and Rust inherent impls must live in the defining crate — so the
method could not stay in `preset.rs`. The alternative was demoting it to a free function
purely to dodge the crate line. `EditorTab`'s impl is self-contained and carries no
nih-plug, so both moved. **`EditorTab::file_slug` stayed behind** as a free function beside
its one caller: the on-disk `presets_<slug>.json` name is *persistence*, not tab identity,
and following the type into core would have dragged a filesystem opinion in with it.

> 🚨 **`organon-core` cannot be published to crates.io as it stands (found in PR B).**
> `math.rs` carries 7 `include_str!("../../assets/…")` sites reaching *out of* the package
> into `native/assets/`. Correct for a workspace path dependency and for the mirror;
> **fatal to `cargo package`**, which bundles only files under the package root. The
> assets can't just move — `deploy.sh` installs `assets/networks/*.json` as the #226
> runtime gallery. #626 §2.5 makes crates.io this crate's developer identity, so **Tier 5
> owns a real decision here** and should not meet it at publish time.

### 19.0.1 Reference #2 — the `FuncName` split (#626 Tier 3 PR B)

#536 T4 called `math.rs → crate::params::{FuncName, ParamValues}` *"the one real design
decision"* of the crate split. Here is what was decided, and the constraint that decided it.

**`ParamValues` moved free** — six fields, all `glam::Vec3`/`f32`, no enum reference.

**`FuncName` exists twice, on purpose:**

| Type | Home | Role |
|---|---|---|
| `organon_core::params::FuncName` | core | the **semantic** type. Plain `Copy` enum. What `math::apply_func` and `world.rs` use. |
| `params::HostFuncName` | native | the **host adapter**. Carries nih-plug's `#[derive(Enum)]` for `EnumParam<_>`. |

**The orphan rule is what forces this, and it is worth stating so nobody tries to
"simplify" it away.** `organic-math-native` cannot
`impl nih_plug::Enum for organon_core::FuncName` — both the trait and the type are
foreign to it. There is no arrangement in which one type serves both sides. Either core
takes the derive, or the host owns a mirror.

**Why not #536's recommended `host-params` feature.** It would have core own the type and
gate the derive behind an optional dependency. Its stated caveat (wasm32 resolution) is
dead — #418 is parked — so the choice was made on native merits, and one argument decides
it: **an optional dependency is still a dependency.** With `host-params`,
`cargo tree -p organon-core` is clean only for the default feature set, and §19.0's
acceptance test degrades from a statement about the crate to a statement about one
configuration. Keeping nih-plug out unconditionally is worth a 7-variant mirror.

⚠️ **The two lists are pinned by `host_func_name_mirrors_core`, and the pin is
element-wise by name, in both directions.** Not a length check: the variant **index is
the wire format** — it is what `to_shared` writes into `Shared` and what presets store —
so a same-length *reordering* would pass a count comparison while silently repointing
every saved preset and automation lane at a different waveform. Add a variant to **both**
lists, at the tail.

**`GeneratorMode` did NOT move, and its 27 variants are not duplicated.** Both #536 and
#626 list it as moving with `FuncName`. Reading `math.rs` showed its only non-comment use
there was a **test** of `from_u32`/`to_u32` round-tripping — `params.rs`'s own
`enum_u32_via_index!` machinery, tested from the wrong file. The test moved to sit beside
what it tests; the enum stayed put. Duplication cost for the tier: 7 variants, not 34.

**Reference #3 (`math.rs → crate::ipc::Shared`) needed no move either.** #536 says it
"resolves by co-location", i.e. `ipc.rs` follows `math.rs` into core. It is **test-only**,
so relocating four items to `native/tests/vecbuild_ipc.rs` resolved it and left `ipc.rs`
at a **zero diff** — a far stronger guarantee about the `Shared` layout than moving the
file and re-verifying it would have been. Those tests assert two crates agree about a wire
format, so an integration test is their correct home regardless.

⚠️ **Every new crate needs an INCLUDE in the export manifest, in the same change.**
`scripts/mirror-platform.manifest` is allow-list / default-deny, so a new directory under
`native/` is silently absent from the public repo otherwise — and the exported
`native/Cargo.toml` would then name a workspace member that isn't on disk, which cargo
rejects outright, but only once somebody builds it. `./scripts/export-public.sh --dry-run`
is the check; `0 unexpected` on the byte-identity line is the gate, and the member-list
guard fails the run outright rather than shipping the broken manifest.

| File | Role |
|---|---|
| `math.rs` | pure algorithm + all generators (incl. Penrose tilings, #121) + lowering/lofting (+ tests) |
| `params.rs` | nih-plug params + enums + `to_shared` |
| `param_table.rs` | `param_block!` SSoT packing + layout goldens |
| `ipc.rs` | `Shared` Pod + mmap Writer/Reader + Feedback channel + mind-ring path + glyph-ring path (`glyph_ring_path` / `_in`, organon#217 T1) + the **edition-namespaced** `$TMPDIR` path builders (`namespace`/`ns_file`, plus the caller-named `ns_file_checked`/`mind_ring_path_in`, §4.1) |
| `organon-core/src/glyph_ring.rs` | organon#217 T1 — the **glyph ring**: `GlyphCell` (32 B, offsets pinned by test) / `GlyphFrame` / `GlyphRingHeader` + `GlyphRingWriter`/`GlyphRingReader` (double buffer with a lap guard; layout-version + cell-stride refusal), the symbol→tile table `tile_for` (block/shade glyphs → sub-cell extent + extrusion depth; unknown → full block at reduced emission), `srgb8_to_linear` and its exact inverse `linear_to_srgb8` (T11 — the producer decays in linear and the ring stays sRGB8), the `SGR_PERSIST` trail bit, and `lower_grid` — grid → instances/tints/**emits** + backplane, each tile at its cell centre **plus the cell's `sub_x`/`sub_y`** and `active_path` cells sliding between the previous and current *exact* positions, never from a trail (+ tests). In core because the writer (`organon-glyphs`) and the reader (`world.rs`) must share ONE definition and core is the only crate both see |
| `organon-core/src/edition.rs` | #483 Tier 1 — build-time product editions: `Edition` (`Full`/`Mind`) + `EDITION`, driving product name / IPC namespace / visible `UiTab`s. Pure + unit-tested for both editions from a default build (§4.1). **#626 T3: moved to `organon-core`**; re-exported as `crate::edition` |
| `organon-core/src/tabs.rs` | #626 T3 — the editor's **tab taxonomy**: `UiTab` (the tab bar) + `EditorTab` (the 7-way preset partition). Lifted out of `preset.rs`, which keeps its nih-plug `ParamSetter` logic. Re-exported as `crate::preset::{UiTab, EditorTab}` (§19.0) |
| `organon-core/src/kind.rs` | #48 T1 — the console's **kind** vocabulary: `Kind` (`scene`/`panel`), `KIND_WORDS`, and `resolve`, whose refusal carries the known list. Here because the two front-ends that had a copy each are in *different* crates (`cli.rs`, `organon-console/conversation.rs`) and this is the only one both can see; a closed set of words needs no host, GPU or UI. ⚠️ No `Default` — the "a kindless `patch` line means `scene`" rule is that lane's and lives in `cli::PATCH_DEFAULT_KIND` |
| `organon-core/src/console_ops.rs` | organon#49 T5a — the **console command lane**: `ConsoleOp`, `PortalCmd`, `CameraFraming`, the word tables (`PORTAL_WORDS`, `CAMERA_WORDS`), `MAX_BLOCK_ROWS`, `PATCH_DEFAULT_KIND`, and the sidecar wire format (`console_cmd_path` / `console_op_to_line` / `parse_console_op` / `append_console_ops`) — lifted whole out of `cli.rs`, tests included. Here for `kind.rs`'s reason: `bin/ctl.rs` writes this channel from the root crate and `console_main.rs` reads it, and T5c moves that reader into `organon-console`. ⚠️ **Versioning is the verb** — an unknown verb is skipped, never a parse error. Re-exported as `crate::cli::*` so no caller moved |
| `organon-core/src/viewpoint.rs` | organon#49 T5a — the viewpoint's band and origin: `PITCH_LIMIT` / `YAW_LIMIT` / `DISTANCE_MIN` / `DISTANCE_MAX` + `DEFAULT_{YAW,PITCH,DISTANCE}`. Lifted from `scene_input`, whose own doc had already flagged the hazard in capitals — **one number, four readers** — now that those readers span three crates. `scene_input` re-exports all seven, so it is one number still |
| `organon-core/src/lib.rs` | #626 T3 — the core crate root. Its header records what may and may not enter core |
| `organon-core/src/params.rs` | #626 T3 / organon#49 T1+T2+T4a — the param types with **no host concern**: `ParamValues` (the algorithm's numeric block), the `IndexedEnum` trait (core's counterpart to nih-plug's `Enum`), and **fourteen** semantic enums — `FuncName`, `GeneratorMode`, `BoidsForm`, `OscDivision`, `SurfaceMode`, `MaterialType`, `CamPath`, `Palette`, `FdtdSource`, `FieldVolSource`, `ColourMode`, `CalColourSource`, `FieldKind`, `FluxAxis`. Each is mirrored in `params.rs` by a `Host*` adapter carrying nih-plug's derive (the orphan rule; §7 owns the contract) and pinned to it by a `host_*_mirrors_core` test. Re-exported, so `crate::params::GeneratorMode` still resolves |
| `mind_ui.rs` | #483 Tier 1 — the shared Mind-UI chrome: edition-filtered tab bar, active-tab clamp, product heading (+ tests). Tier 2 factors the Mind card body in here |
| `mind_main.rs` | #483 Tier 1 — the `organon-mind` standalone entry point (`required-features = ["mind-edition"]`) |
| `console_icon.rs` | **Organon Console's window icon** — the aperture mark (`assets/chrome/aperture-mark-on-dark.svg`) as two `include_bytes!`d PNGs, decoded with the `image` crate already here for `overlay.rs`, hung on the window by `console_icon::apply` in `console_main.rs::resumed`. ⚠️ Two icons, two unrelated APIs: `with_window_icon` reaches Windows' `ICON_SMALL` (title bar, Alt-Tab) and is the whole story on X11/Wayland and a no-op on macOS; the **taskbar** button is `ICON_BIG`, reachable only via `WindowAttributesExtWindows::with_taskbar_icon`. Both behind one call so the platform story has one home. ⚠️ The rasters are **committed, not built** — a `resvg` build-dependency would keep the SVG as the single source, but the root crate has no build script and adding one would put one on the plugin cdylib, the standalone, the visual, the CLI and three editions to give one window an icon; the drift that buys is paid for by the SVG sitting beside them, `assets/chrome/README.md`'s regeneration command, and tests pinning the rasters' size and opacity. Gated on `console-edition`. See `CONSOLE_ARCHITECTURE.md` §1.10 |
| `mind_ring.rs` | #367 Tier 2 activation-ring mmap: `MindRing`/`MindFrame` + `MindRingWriter`/`MindRingReader` (separate channel from `Shared`; per-token model activations → node-glow). Carries the Phase-B three-way append (#507 trajectory+lens / #505 sparse experts / #409 SAE features), its **pinned-offset** test, and the `frame_bytes` layout guard (+ tests) |
| `organon-core/src/gguf.rs` / `gguf_data.rs` | #367 T1 pure GGUF **header** parser (metadata KV + tensor directory + storage geometry, never a weight) / #507 T1 the **payload** side: mmap + ggml dequantizers (F32/F16/Q8_0/Q4_0/Q4_K/Q6_K) + the deterministic PCA-3 basis + `project_embedding_galaxy` (+ tests) (+ tests). **#626 T3: moved to `organon-core`** and re-exported as `crate::gguf` — ⚠️ *provisionally*: Tier 4 lifts the lens builders out of `math.rs` and these follow them into `organon-mind` (#536 T4 ref #1) |
| `mind_viz.rs` | #482 Tier 1 Mind-dashboard paint helpers: `MindViz` display state (peak-hold, auto-gain, per-token effort scroll, tokens/sec) + `paint_*` egui draws for the "Live Telemetry" widgets on `UiTab::Mind`; the editor's own `MindRingReader` reads the same ring (no `Shared` change). Update math pure + unit-tested |
| `mind_console.rs` | #367 Tier 2 UX — the in-plugin Mind console: `MindConsole` spawns `organic-math-mind-runtime` as a managed child (piped stdio), a bounded log ring drained by reader threads, and forwards the Mind-card command REPL to the child's stdin (no separate terminal; plugin never links llama.cpp) |
| `frame_ring.rs` | #554 T1 the **frame mirror** mmap: `FrameRingWriter`/`FrameRingReader` carrying the visual's rendered frames (640×360 RGBA, 3 slots, ~2.7 MB) to the editor so it can draw a live viewport in its own window. A SEPARATE channel from `Shared` (high-rate payload). Boundary is **CPU memory** because no *published* `egui-wgpu` pairs with wgpu 30, so a handle cannot cross; a `memcpy` can. Newest-wins, drop-don't-stall, torn-read guarded (+ 12 tests). **When it runs (#609):** `mirror_requested(editor_open, viewport_drawn)` — a pure predicate `process()` stamps into `Shared.mindview[3]`, ANDing `EguiState::is_open()` (set in `Editor::spawn`, cleared in `EguiEditorHandle::drop`) with a latch stored at `viewport_pane`'s own draw site. It used to be a constant `1` from the plugin's `Default`, so a projector-only session rendered a second 640×360 scene and blocked on a readback at ~15 Hz for a viewport nobody had opened. **✅ #593 Tier 4 gated the whole subsystem out of the Mind edition** — the module itself, `viewport_pane` + its call site, `PresetUi.frame_*`, `OrganicMath.viewport_on` + the `mindview[3]` stamp, and `world.rs`'s `Mirror`/`pump_mirror`/`MIRROR_*` are all `#[cfg(not(feature = "mind-edition"))]`, so **`cargo build --features mind-edition` compiling is the assertion that nothing on Mind's path names any of it**. Everything **stays** for full Organon, where it is the plugin's *only* viewport path (inside Ableton the editor does not own its window). `Shared.mindview[3]` stays **reserved** — a dead `Shared` field is never removed. See `MIND_ARCHITECTURE.md` §2.5 |
| `baseview_input.rs` | the **winit-free** half of `ui_layer` — `baseview::Event` → `egui::RawInput`. Mirrors `egui_winit::State` call for call (`new` / `on_window_event` / `take_egui_input` / `handle_platform_output`) so the existing call site ports across unchanged, against the windowing layer the *editor* already runs on (there is no winit window in the plugin process). Written from `egui-baseview` `11f487f` rather than copied: `command` is Meta on macOS (the reference wires it to Control everywhere, so ⌘C did nothing and ⌃C copied), modifiers come from one source with the event's own key folded in, shifted letters and `physical_key` reach egui at all, the stale macOS scroll-flip and the Retina-halving `Pixels` division are gone, and drag-and-drop is translated. Pure tables + a windowless `State`, so **both** platforms' behaviour is unit-tested from a cloud session (+ 77 tests). `EventResponse` carries a third field `egui_winit` has no analogue for — `accept_drop` — because baseview's `EventStatus::AcceptDrop` is the **gate** the whole drag gesture passes through on macOS/Windows; without it hover works and the drop can never land, and X11 emits no drag events at all so Linux cannot see the difference (found on the Mac). **Declared by `lib.rs` since #593 Tier 2**, which is its first consumer (`wgpu_editor.rs`); `tests/baseview_input.rs` — the shim that existed only to compile it — is gone and `keyboard-types` moved to `[dependencies]`, which is the trade that shim's note specified. Declared **ungated** rather than behind `mind-edition`: gating it would drop its tests out of a default `cargo test`, shrinking the suite by exactly the coverage it added. The cost is the one #599 measured — naming a crate in `[dependencies]` moves this package's `-C metadata` hash, so the shipping binaries are no longer byte-identical to #599's |
| `ui_layer.rs` | #554 T4 **egui on the renderer's own wgpu device**, drawn into the visual's window *after* the composite (so the UI is never tone-mapped, bloomed or exposed). `UiLayer<P: EguiPlatform = WinitPlatform>` owns an `egui::Context` + a platform backend + the vendored `egui-wgpu` renderer; `set_format` rebuilds when the **H** key swaps the swapchain to `Rgba16Float`. Input routes through `mind_shell::PointerRouter` (drag capture: an orbit begun on the scene keeps the camera over a panel). Visible by default in **Mind**, hidden in full Organon (the projector feed); **U** toggles. **#593 T3 made it generic over its windowing backend** — it names no window type, takes an `egui_platform::WindowGeometry` per call, and returns a `UiEvent { target, response }` so a baseview host can answer its platform. A *binary-level* module — as a lib module it would drag a GPU device into the plugin cdylib, i.e. into Ableton (+ tests) |
| `egui_platform.rs` | #593 T3 — **the egui platform seam**, and the file that ended the world's last winit coupling. `WindowGeometry` (physical size + scale factor) is the two facts egui reads off a window, carried as *data* because `baseview::Window` can answer neither — it is a handle you act on, not one you ask. `EguiPlatform` is `egui_winit::State`'s four calls plus `pointer_phase` (which used to be a `match` on `winit::WindowEvent` inside `ui_layer`), with three associated types: `Event`, `Response` (what the backend tells its platform), `Deferred` (what `handle_platform_output` could **not** do itself — `()` for a backend holding its window, `PlatformActions` for one the platform only lends a window inside a callback). **Ungated in `lib.rs`**: `world`/`ui_layer` do not exist in a default build, so a trait living there could never be implemented by the baseview arm (+ tests, incl. a stub backend that is neither winit nor baseview) |
| `winit_platform.rs` | #593 T3 — the **winit arm**: `egui_winit::State` plus the `Arc<Window>` it reads, behind `EguiPlatform`. Deliberately thin; the geometry it is handed is ignored because `egui-winit`'s `take_egui_input` insists on reading its own window, and the two agree by construction (the host derives both from the same window). `winit_platform::ui_layer(device, window, format)` is the visual's one-liner constructor. Lives in the world's tree under the same gate — the *trait* is ungated, only the arms live with their hosts (+ tests pinning the `WindowEvent` → `PointerPhase` table) |
| `vendor/egui-wgpu/` | #554 T4 — **`egui-wgpu` 0.33.3, minimally vendored and ported to wgpu 30.** Only `renderer.rs` is taken (egui meshes → wgpu draws, ~1 180 lines); upstream's `lib.rs`/`setup.rs`/`winit.rs`/`capture.rs` are dropped because they create an instance/adapter/device/surface the visual already owns — and two thirds of the port's errors lived in exactly those files. Eight `ORGANON PATCH (#554)` port sites, each a wgpu 28–30 rename or `Option`-wrapping. One is ours rather than upstream's: `is_linear_target` picks egui's fragment entry point on *does this target hold linear values* rather than `is_srgb()` alone, so the UI is not mis-encoded on the `Rgba16Float` EDR swapchain (+ 4 tests). Deleted the day a published `egui-wgpu` accepts wgpu 30 |
| `audio_ring.rs` | #430 Tier 2 audio-sample ring mmap: `AudioRingWriter`/`AudioRingReader` (separate channel from `Shared`; plugin post-synth stereo → visual recorder for the muxed audio track) (+ tests) |
| `recorder.rs` | #430 in-app recorder (visual-only): production-texture readback → ffmpeg; SDR H.264 / HDR10 HEVC-PQ, beat-synced N-bar stop, audio mux, selectable rational `Fps`, **phrase-chunk** quotas + beat-driven CFR, async `Finalizer`, CPU PQ/WAV/pacing cores (+ tests) |
| `bin/mind_writer.rs` | #367 Tier 2 `organic-math-mind-writer` bin: synthetic per-token frame generator (zero inference) |
| `bin/mind_runtime.rs` | #367 Tier 2b — the **real** visible mind: loads the `.gguf`, runs live inference on a typed prompt, and taps per-token activations into the ring. `required-features = ["embedded-llm"]`, so the default build never compiles it (§4) |
| `mind_shell.rs` | #532 T1 the workstation shell's **pure core** — host-free and GPU-free (`egui_docks` geometry, `layout_workstation`, `PointerRouter`), so it is unit-testable in a cloud session where no window can open. ⚠️ **`PointerRouter` serves the visual's window only.** Its `egui_wants_pointer` input is `Context::wants_pointer_input()`, which a `CentralPanel` makes unconditionally true (`allocate_central_panel` sets `unused_rect = Rect::NOTHING`) — so under an editor that draws one it routes *everything* to the interface. The visual escapes this by drawing a floating `egui::Window` and no central panel; `scene_input.rs` carries the measurement and takes the other route |
| `scene_input.rs` | #621 — **the viewport's camera input**, and the world's second (backend-neutral) input entry point. `CameraInput { Orbit, Zoom }` is what `World::apply_camera_input` consumes and what `on_window_event`'s winit arms now delegate into, so one orbit and one zoom exist. The egui half is `scene_viewport`, which registers the scene as a **drag-sensing widget** and reads its `Response` — egui's hit-test is the authority once a central panel is in play, and it brings capture, arbitration against sliders, no fight with the `ScrollArea`, and a `drag_delta` in *screen* space that a scrolled or moved pane cannot make stale. Workstation registers the pane's rect **after** the scroll area (topmost wins a tie); immersive registers the window's rect **before** the interface (so every control beats it) and adds `press_belongs_to_the_scene`, which requires no **interactive** widget under the pointer — unfiltered is never zero, because egui registers a `WidgetRect` for every `Ui`. `orbit_pixels` puts egui's points into the physical pixels the rig has always used, which is what stops the editor orbiting at half speed on every Retina display. Ungated in `lib.rs` (both editions compile `editor_ui`); only a `scene_behind` host registers a region (+ 22 tests, 12 of them driving a real headless `egui::Context`) |
| `mind_log.rs` | the shared **fine-tuning corpus** (#317 + #367): every prompt, reply, plan, action, acceptance, rejection and model event appended as one JSON line under `…/OrganicMath/mind-log/organon-mind.jsonl` |
| `cli.rs` / `bin/ctl.rs` | #452 the `organon` CLI: arg parsing + catalog/status/get/watch formatting + `CliOp` op building (pure, tested) / the thin bin (mmap reads, command-channel appends, exit codes). Also #147 T3½'s **adapter selection** (`check_adapter_dir` / `select_adapter` / `clear_adapter` / `read_adapter_sidecar` → `MIND_ARCHITECTURE.md` §2.8.1) — ⚠️ they take the sidecar path as a **parameter**, so a test never writes the real one and clears whatever the machine had selected |
| `recipe.rs` | #452 Layer 3 — the **recipe library**: named starting-points applied with one command, so something beautiful is reachable with no saved presets. Compile-time data, **not** saved state |
| `snap.rs` | #452 Tier 3 ("the eyes") — single-frame PNG of the **production texture** (the same one the recorder reads). Closes the see→act→see loop for an external agent |
| `agent.rs` | #317 Tier 1 **AI Performer** — the internal agent that *plays* Organon. Runtime lives in the **visual** (it owns the frame + look-application); the plugin only stamps the request block |
| `preset.rs` | `PresetValues` capture/apply + JSON store. ⚠️ Also, since Console #7, the **writable mirror** an Organon panel drawn outside a host writes into — see `param_sink.rs` |
| `param_sink.rs` | Console #7 — **where a panel control's write goes**: `Sink::Host(&ParamSetter)` for Organon's editor, `Sink::Mirror(&mut PresetValues)` for Organon Console, and the `srow!`/`crow!`/`combo!`/`rd!`/`wr!` macros that name a field once so both sides are compile-checked. Read its module doc before converting a panel — the wall it works round belongs to `nih_plug` |
| `panel_surface.rs` | Console #7 — the **Look ▸ Surface** card's body, the first of Organon's 25 editor panels lifted out of `editor_ui`'s one long pass so the Console can call it too, plus `OrganonPanels` (the Console's mirror + its difference-not-snapshot route into `Shared`) |
| `clip.rs` / `keymap.rs` | MIDI CC clip map / note→preset Key Map |
| `controller.rs` | #356 T1 **four-quadrant performance controller** — a Launchpad-style 8×8 RGB pad surface (default: Novation Launchpad Mini MK3) as a hardware front-end to the #354 preset system |
| `audio.rs` | input band analysis (audio-reactive) |
| `synth.rs` | #339 **Duo-Field synthesis** — stereo listener probes ("virtual microphones") placed *in the synthesized field*. This is what finally writes the plugin's output buffer, which used to be a pure pass-through. Not to be confused with the real mic (`audio.rs`) |
| `chamber.rs` | #346 **Field Chamber** — the analyzer panels on the reference box's walls: oscilloscope on the rear −Z (time), spectrum on the right +X (frequency), so the Duo-Field sits inside a time × frequency frame |
| `lib.rs` | the `Plugin` (VST3/CLAP) + egui editor + `process()`. Since #593 T1 the editor **body** is a top-level `pub(crate) fn editor_ui(&EditorCtx, …)` rather than a `create_egui_editor` closure, so a second host can draw the identical interface (§14) |
| `wgpu_editor.rs` | #593 Tier 2 — **the custom wgpu `Editor`**, grown from `editor_probe.rs` rather than restarted: the handle chain is reused verbatim, and the probe's cycling clear becomes a real frame. Per frame it draws the scene (`World::render_into`, `FrameTarget { presented: true, ui_window: None }` — the world draws no interface, this file does), then the interface over it with the vendored `egui-wgpu` by calling **`lib.rs`'s `editor_ui`** (the same function `nih_plug_egui`'s editor calls, so the two cannot drift) with a real `ParamSetter`, then presents. Input arrives from baseview through `baseview_input`. The device negotiation is **`bin/visual.rs`'s, not the probe's** — the cube pipeline needs `max_bind_groups` past wgpu's default 4, and a `Limits::default()` device opens the window and then fails to create pipelines. §2.4's open question (a `Surface` outliving its `NSView`) is answered structurally: `surface_action` → `SurfaceAction` has **no re-create variant**, so a resize can only reconfigure, pinned by tests. Gated on `mind-edition`, and **the default editor within it** since #593 closed — the `ORGANON_EDITOR_WGPU=1` opt-in that armed it through the build-out was inverted to an `=0` opt-*out* once the Mac pass its own exit condition named had happened. No `Shared` change, through all five tiers. **Tier 4 changed nothing in this file and everything about what it shows**: Tier 2 drew the world into its surface correctly on the first run and it was *invisible*, because `editor_ui`'s `CentralPanel` is opaque and the #554 mirror pane painted a 640×360 photograph over exactly that region. Tier 4 opens it — `EditorCtx::scene_behind` → `theme::workspace_frame` gives the central region a transparent frame under this host, and the mirror pane is gated out — so the world shows through wherever a widget has not painted its own surface, with the workstation drawn over it |
| `editor_probe.rs` | #593 Tier 0 — **the route-C probe**, and the skeleton Tier 2 grows into. A second `nih_plug::editor::Editor` that does *not* go through `nih_plug_egui`: it adapts nih-plug's `ParentWindowHandle` (its own 3-variant enum, not a raw-window-handle type) to **rwh 0.5**, opens a parented **baseview** window on it, converts *that window's* window **and display** handles to **rwh 0.6** (`wgpu::rwh`), and builds a `wgpu::Surface<'static>` via `create_surface_unsafe`. Both handles must come from the baseview window: nih-plug's `X11Window(u32)` carries no display connection and rwh 0.6's X11 display handles need one. Each `on_frame` clears the surface to a **cycling** colour and presents — a static clear and a dead render loop look identical on screen, and this repo has shipped exactly that (#582's first cut). Gated **twice**: `mind-edition` (the plugin cdylib must not move — measured unchanged at 12 749 528 bytes) and `ORGANON_EDITOR_PROBE=1`, checked at the top of `Plugin::editor()`. No `World`, no egui, no input — one question per probe (+ 16 tests, incl. the AppKit conversion, which both rwh crates define on every platform so it is testable from Linux) |
| `standalone.rs` | host-less entry point — opens the same editor as the plugin (`organon-standalone`). Also owns the **Windows HiDPI answer**, because the standalone is the one product with nobody to ask: the visual learns its scale from winit, and the plugin from the host's `IPlugViewContentScaleSupport` → `set_scale_factor`, but a host-less window falls through three layers that each defer the decision upward — `nih_plug_egui` seeds `scaling_factor` to `Some(1.0)` off macOS, so baseview gets `ScaleFactor(1.0)` and never reaches the `GetDpiForWindow` it only calls under `SystemScaleFactor`, and nih-plug's standalone wrapper overrides both with a `--dpi-scale` flag defaulting to 1.0. So this file queries Windows itself and passes the flag through `nih_export_standalone_with_args`; **no vendored file is patched**. ⚠️ `SetProcessDpiAwarenessContext` must precede `GetDpiForSystem` — the latter answers a DPI-*unaware* process with a flat 96, i.e. scale 1.0, indistinguishable from a real 100% display. The factor is clamped to what still fits `SPI_GETWORKAREA`, since the editor's default is `params::EDITOR_DEFAULT_W`×`EDITOR_DEFAULT_H` **logical** and baseview sizes windows as logical × scale — unclamped, 300% would open a 3840×2580 window and trade a tiny UI for a cut-off one. An explicit `--dpi-scale` always wins |
| `theme_config.rs` | #551 T1 the UI theme as **runtime state**: `ThemeConfig` (`Palette` + `Material` + `Depth`, all serde-defaulted), a process-global `ArcSwap` for wait-free per-widget reads, its own `ui_theme.json` store, and the `UI` panel that edits it live. **Not** nih-plug params — a Scene recall must never restyle the editor (+ tests) |
| `theme.rs` | #542 T1 the house style: design tokens (the warm palette from `doc/organon_mind_visual_reference.md` §1), `install` (Inter type ramp — once per context — + the full `Visuals` pass), `card_frame`/`card_title`/`hairline`, and the pure `row_grid`/`combo_grid` control-row partition (+ tests). Everything that decides how the editor *looks* resolves here. #542 T2 added `theme::paint` (gradient meshes, baked grain/mottling tiles, bevels, ambient key — all `epaint`, no shader); #551 T1 turned its colour tokens into accessors over the live `theme_config`. **#593 T4 added `workspace_frame(scene_behind)`** — the one place that decides whether the editor's central region is an opaque faceplate (`None` → `CentralPanel::default()`, every host that owns all its own pixels) or a transparent one the 3-D world shows through (Mind's wgpu editor). Same geometry either way, asserted against egui's own `Frame::central_panel`. **#120 added `CardStyle`** — every colour a card's chrome asks for, gathered into one value so `card()` can be drawn in Organon Console's palette as well as this one; `card_chrome` / `card_header_band` / `card_title` / `framed` take it, the *geometry* stays a shared constant, and `CardStyle::organon()` is the live-`theme_config` value that keeps the editor byte-for-byte what it was |
| `bin/visual.rs` | the visual **process**, ~625 lines after the #572 world hoist: it owns the **window and the swapchain** (`WindowSurface`) — create the window, pick its launch display (`launch_display`/`pick_launch_monitor`), build the wgpu device + surface, acquire→`render_into`→present each frame, apply the frame's `FrameRequests`, drive HDR format swaps + EDR, and run winit's event loop. `impl ApplicationHandler` sits on a `VisualApp { world }` wrapper because `World` is a library type (orphan rule). 🚚 **organon#49 T4c-ii — this file is `native/organon-visual/src/main.rs` now**, its own package (see the crate table for why it could neither descend nor stay). The `use organic_math_native::math` shim it used to keep is **gone**: it was load-bearing only because `crate::` meant *this binary* inside the `#[path]` copy of `world.rs`, and there is no such copy any more. ⚠️ It still depends **upward** on `organic-math-native` for `agent::core_catalog()` — the visual runs the Performer's worker, and an empty catalog would compile and silently gut the agent's prompt. `main` comes up **without activating** (`with_activate_ignoring_other_apps(false)`) so the host's floating plugin editor doesn't vanish, and therefore arms the #588 launch watchdog — see `launch_macos.rs` |
| `world.rs` | #572 route C, the **world hoist** — the renderer *and everything that drives it* as a library module tree, so the editor can reach it (a binary's modules are unreachable from the library it depends on). **Stages 1–3 done:** `World` (was `bin/visual.rs`'s `App`) plus its `#[path]` tree — `axes`/`chamber`/`render`/`capture`/`overlay`/`hdr_macos`/`rt`/`metal_island`/`gpu_timer`/`recorder`/`snap`/`ui_layer`/`winit_platform`. It owns **no window, surface or swapchain** since stage 3 — the host hands in a `FrameTarget` and applies the returned `FrameRequests` — and since **#593 T3 it does not name `winit::window::Window` at all**: the frame states `ui_scale_factor` instead of lending a window, and the host builds the `UiLayer` it hands to `attach_gpu`. The seam is `EventResponse` + `attach_gpu`/`on_window_event`/`render_into`/`present`, forced by the orphan rule. 🚚 **organon#49 T4c-ii — this file lives in `native/organon-world/src/world.rs` now**, behind that crate's **default-off `world` feature**; `organic-math-native` re-exports it as `crate::world` under `any(mind-edition, console-edition)`, exactly the gate it used to declare. **The gate did not change, only the manifest that states it** — and it is still the measured one: ungated the module grows the plugin cdylib 12 749 728 → 13 250 704 bytes, gated it measures 12 749 528 (unchanged; re-measured at the Console #6 T1 widening), and a shipping VST3 must not change for no user-visible reason. ✅ **The dual compilation is GONE.** It used to be compiled twice in a mind-edition build (library module + the binary's `#[path]` include) — not redundancy but the *mechanism*, since a `#[path]` include is not a cargo feature and so gave the visual a world the cdylib did not get. Now the visual is `organon-visual`, a package of its own that turns the feature on, so the world compiles **once**. That also retires the reason `render.rs`/`rt.rs` had to spell siblings `super::`: nothing here needs a path that resolves in two crate roots any more. **#593 T4** gated the `Mirror` block (`mirror`/`mirror_want`/`mirror_tick`, `pump_mirror`, `MIRROR_*`, `drop_mapped`) on `not(mind-edition)` — so the library's `World`, the one Mind's editor drives, has no mirror at all, and `bin/visual.rs`'s copy keeps it in the default build that produces the projector both products install |
| `render.rs` | `Renderer` + `RenderFrame`/`RenderPath` + passes |
| `post.rs` | bloom + composite + SSAO/SSR |
| `env.rs` | IBL split-sum precompute + skybox |
| `terrain.rs` / `stars.rs` | world layers (backdrop / sky) |
| `ocean.rs` | FFT (Tessendorf) ocean — spectrum + per-frame wave tile (#102B) |
| `particles.rs` / `fluid.rs` | particle aura + Navier–Stokes tier (#182 T1: the solver also carries the RGB dye field — inject / advect / MacCormack kernels; T2: no-slip node-occupancy walls, heat/buoyancy in `dye_a.w`, beat splash, substeps). **Maxwell energization** (#247 T1, `Shared.maxenergy[8]`): the velocity grid's `w` channel carries the field energy density (`math::VelGrid::fill_analytic` → `AnalyticField::energy` → `maxwell_energy_density`); a mote advects along the field direction (`xyz`) but lights by the sampled energy magnitude (`w`), log/soft-knee tone-mapped in `particles.wgsl` — the fluorescent-tube demo. **Two sources feed `w`:** (a) **Lite + Maxwell** → the analytic EM energy density above; (b) **Aura-Fluid / Navier–Stokes tier** (any generator) → the flow's own energy density `½\|u\|² + ½\|ω\|²` (kinetic + enstrophy), baked in by `fluid.wgsl::cs_energy` after the final projection (with a freshly-recomputed curl so `\|ω\|` matches the projected field), gated by `FluidParams::energize`. Same downstream glow either way; 0 = byte-identical. **Tier 2** (`maxenergy[4..5]`): source (a) can instead use a **finite-antenna source** (`maxwell_finite_field_eb` — the standing-wave current `I(z)=I₀·sin(k(L/2−|z|))` as a line of quadrature elements; its charge `ρ=−∂_z I` peaks at the tips) so the cloud shows the **bright-ends/dim-centre** pattern of a real driven rod (in the Fluid tier it shapes the stir direction while energy still comes from `cs_energy`). **Tier 3** (`maxenergy[6]`): energized nodes shed **bright energy by the local field energy** (`VelGrid::sample_energy` + the shared `math::energy_tonemap`, tinted by the ember hue) into **two** targets — (a) the **Fluid Ink** dye (Navier–Stokes advects + swirls it), and (b) the **MLS-MPM liquid** (an HDR ember grid over the tank is splatted into `liquid.wgsl::cs_resolve_field`'s surface `rgb`; `metaball.wgsl` emits the HDR excess so the isosurface glows in the field pattern) — the same `energy → dye + liquid` slider drives both. **Field-force drive** (#248, `Shared.mxforce[4]`): instead of following field lines at constant speed, `VelGrid::fill_analytic_force` fills the grid with a **drive** the medium is stirred by — the right one per tier. **Lite** uses `AnalyticField::force` (the soft-capped E vector, magnitude + sign): motes are pushed by the force, strong near the core, reversing as the dipole oscillates. **Fluid** uses `AnalyticField::stir` (the **azimuthal circulation** `∝ (ẑ×r̂)/r²·cos(ωt−kr)` — the oscillating dipole's B swirl); it's **solenoidal**, so the incompressible pressure projection keeps it (the conservative E it would cancel) and the fluid genuinely swirls around the dipole axis near the core, reversing with the oscillation. `energy_contrast` is applied **at display time** — the mote shader (`particles.wgsl`, `DrawU.params3.z`: `pow(energy, contrast)` before the tone-map) and the dye/liquid reads — so it sharpens the near-core glow in **both** tiers (the Fluid glow is the solver's own `cs_energy`, not this grid). **Shaded beads** (#298 Tier 1, `Shared.pbeads[8]` = `[beads, metallic, roughness, …]`): a **particle-style toggle** that swaps the additive spark billboards for **opaque sphere-impostor droplets** bearing the shared split-sum IBL + key/fill lighting. The particle *system* is untouched (same advection/count/energization/hue cycle); only the **draw** differs — a second `bead_pipeline` (group 0 = `DrawU`, group 1 = the shared `env.rs` IBL) whose `vs_bead`/`fs_bead` reconstruct the front-hemisphere **normal + `frag_depth`** from the billboard UV (discard outside the disc), depth-write **on** so the droplets occlude each other + the scene, and shade with `cube.wgsl`'s PBR/IBL math (metallic/roughness from `pbeads`, the scene's light/env context passed in as `ParticleShade` built from the cube `Uniforms`). The energization glow + hue cycle survive as the beads' **emissive** term. `beads = 0` → the additive sparks (byte-identical). **Tier 2** (fills reserved `pbeads[3..7]`, no size change): a per-system **material** (Standard/Chrome/Glass/Refractive — env-only) + impostor **shape** (Sphere = analytic; Ellipsoid/Teardrop/Rounded-Box/Dice = a per-mote SDF sphere-traced inside the billboard in a velocity-oriented frame, `bead_sdf`/`bead_frame` in `particles.wgsl`, shading factored into `shade_bead`). **Tier 3** (screen-space participation): a **depth-only bead pipeline** (`fs_bead_depth`, single-sample) draws the impostors' reconstructed `frag_depth` into the **FX depth prepass** (`draw_depth`, added last in the prepass block; `beads_live` extends the `depth_fx` gate so the prepass runs even in the particles-only case), so the screen-space effects that reconstruct from that depth — **SSAO / SSR / SSGI / DoF / TAA** — pick the droplets up as first-class scene geometry (cubes reflect them, they contact-darken + bleed colour). The surface trace is shared between the colour + depth draws (`bead_trace`). Shadow-map *casting* from the camera-facing impostor is deferred to Tier 4 (real TLAS geometry — a camera-facing billboard can't cast a correct shadow from a different light). **Tier 4** (hardware-RT groundwork, `rt.rs`): a **unit-sphere BLAS** (`render::rt_sphere_mesh`) + `RtContext::build_with_beads` appends a curated subset of the largest droplets as sphere TLAS instances (`translate·scale(size)`, tagged `RT_BEAD_TAG` in the custom index) after the field, so they can join RT reflections / GI / shadows + the path tracer; `math::curate_beads` picks the subset under the 65 536-instance cap. The `particles_beads_rt` toggle (reserved `pbeads[7]`) is captured but **ships dark** — the remaining on-Mac wiring is the GPU→CPU particle-position **readback** that fills the bead slice + the RT hit-shader `RT_BEAD_TAG` branch (a camera-facing impostor's position lives only on the GPU; the readback is the one step this GPU-less env can't verify). **#305 Tier 3 impostor AA:** `fs_bead` outputs the silhouette **coverage** as alpha (analytic derivative-feathered rim for the sphere; SDF shapes keep the hard hit/miss) and the bead colour pipeline enables **alpha-to-coverage** (when `sample_count > 1`), so MSAA dithers the impostor edge into sub-pixel samples (colour + depth) instead of the hard `discard` crawling. The depth-prepass draw keeps the hard edge (single-sample) |
| `fluidvis.rs` | #182 T1 Fluid Ink: dye 3D-texture blit + volumetric raymarch (HG key scatter + IBL ambient + emissive) + depth-aware half-res upsample onto the HDR buffer (T2: vorticity-scaled curl-noise micro-detail) |
| `liquid.rs` | #182 T3a MLS-MPM liquid: particle/grid buffers + P2G/grid/G2P + density splat into a second `MetaField` (drawn by the metaball isosurface raymarch; Glass = water) |
| `fluidlight.rs` | #182 T4 light-space passes: dye→key transmittance LUT + liquid caustic splat, one 256² map on the cube pipeline's shadow group |
| `sway.rs` | #182 T4 two-way coupling: fluid velocity → per-node sway springs, displacing the instance buffer in place (no readback) |
| `liquidsurf.rs` | #182 T3b refractive water: post-scene Snell refraction of the resolved HDR + measured thickness + Beer–Lambert + Fresnel |
| `refractsurf.rs` | #214 T5 pt 2 screen-space refraction: post-scene, depth-prepass-reconstructed refraction of the resolved HDR for the instanced Refractive material |
| `metaball.rs` / `voxel.rs` / `mandelbulb.rs` / `creature.rs` / `minimal.rs` / `lens.rs` / `kifs.rs` / `neural.rs` | raymarch render paths (metaball also owns the #152 emissive-Volume pipeline; `voxel.rs` = the DDA occupancy-grid splat + raymarch, now **PBR-shaded** — binds group 1 = IBL for the full Material card + a `depth_pipeline`/`draw_depth` feeding SSR/SSGI; `neural.rs` = #200 T1 MLP isosurface; `lens.rs` = #258 T3 analytic lens SDF; `creature.rs` = #476 T1 union-of-SDF-primitives sea creature — **material-branched** (Standard/Chrome/Glass/Refractive) + in-shader palette + a `depth_pipeline`/`draw_depth` feeding SSR/SSGI, like `voxel.rs`) |
| `shadow.rs` | #152 Tier 3 cast-shadow map: key-light depth map + group-4 bind (PCF-sampled in cube.wgsl) |
| `vxgi.rs` | #152 Tier 3 (#10) voxel GI: voxelize compute + world-space gather added into the HDR buffer |
| `fx.rs` | #152 post-composite creative FX: NPR / DoF / lens FX / grade / feedback (history ping-pong) |
| `kaleido.rs` | #361 T1 **Scene Kaleidoscope** — a post-stage kaleidoscopic fold over the *live, physically-lit* render (any generator + surface), as opposed to the procedural KIFS field |
| `splat.rs` | `SurfaceMode::Splat` — 3DGS as a forward-synthesis *primitive*, with **no reconstruction step**: each node's model matrix maps directly to an anisotropic Gaussian |
| `creature_overlay.rs` | #476 T2c — the "diagram over the creature" look: a thin world-space wireframe (spine, per-segment rings, a vector per limb) built from the same body plan the raymarch uses, drawn additively |
| `material_graph.rs` | #472 T4 — the declarative `material.json` graph: a human- and agent-writable description of the procedural material layers; an interchange + gallery format |
| `temporal.rs` | #152 Tier 2 temporal pass: TAA + motion blur (camera-reprojection velocity, history ping-pong) |
| `rd.rs` / `gi.rs` | reaction–diffusion skin / bounced-GI probes (#152 T3: band-1 SH → directional bounce) + emissive-cubes-as-lights (#167 T3: brightest-N point lights in group 3 binding 1) |
| `hdr_macos.rs` | macOS EDR / true-HDR swapchain plumbing |
| `hdr_windows.rs` | #658 Tier 4 — the Windows half of true-HDR output, and **not** a port of `hdr_macos.rs`. macOS EDR has to be negotiated behind wgpu's back through objc; Windows does not, because **wgpu 30 exposes the whole path natively** — `SurfaceColorSpace::ExtendedSrgbLinear` *is* scRGB (`DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709`, applied by `configure`, so nothing needs re-asserting), and `Surface::display_hdr_info` already reads `IDXGIOutput6::GetDesc1`'s `MaxLuminance` **and** the `DISPLAYCONFIG_SDR_WHITE_LEVEL` the headroom division needs. So: no `windows-sys`, no second raw-API island, and no new dependency at all — pure Rust over wgpu, compiled and unit-tested on *every* platform (the interpretation of a display's numbers is the part worth pinning, and it needs no GPU). `bin/visual.rs`'s `set_hdr_output` shim picks between the two at compile time and the composite sees only a headroom number, so `hdr_max`/knee/`hdr_vivid` are unchanged. Two honest gaps it records rather than hides: Rec.2020 wide gamut is **not** reachable on Windows (DXGI's only Rec.2020 swapchain is PQ-encoded HDR10 and our composite writes linear), and the Mac is deliberately left on its own path — unifying them onto this API needs Mac verification. `doc/arch/render.md` owns the seam table |
| `launch_macos.rs` | #588 — the **launch watchdog**, the reason the visual's window is guaranteed to open. winit dispatches `Resumed` (where `bin/visual.rs` builds the window and the device) only from `applicationDidFinishLaunching:`, and AppKit does not always deliver that to a bare, LaunchServices-less executable that nothing activated — which is exactly how the plugin spawns the visual, deliberately, so it does not steal focus from the host. The result was an invisible process burning a core with empty stderr. `arm()` (called from `main` before `run_app`) schedules a run-loop timer; if `Resumed` hasn't happened after `GRACE`, it calls `applicationDidFinishLaunching:` **on winit's delegate itself** — the one missing call, no notification-centre broadcast and no activation, so focus is untouched. `decide()` holds the whole policy as a pure function so it is tested off-Mac. On a healthy launch the first tick sees `mark_resumed()` already set and AppKit is never touched at all. No-op off macOS |
| `window_macos.rs` | #520 Tier 2 — makes the **standalone**'s editor window resizable + zoomable: ORs `NSWindowStyleMaskResizable` onto the `NSWindow` baseview opened without it (+ a `contentMinSize` floor), reached through objc the way `hdr_macos.rs` reaches the `CAMetalLayer`. Gated on a standalone-only flag (`mark_standalone`), so the **plugin never touches the host's windows**; no-op off macOS. Also owns `sync_editor_view`, which is what makes a native resize actually reflow. A nih-plug standalone nests **three** views — the wrapper's baseview view is the `contentView`, the editor's is a subview spawned via `ParentWindowHandle::AppKitNsView`, and baseview's `GlContext` adds its own `NSOpenGLView` under *that* — and AppKit keeps only the first in step, because baseview gives the other two a fixed frame with no autoresizing mask. So it resizes the editor view to the content bounds **and the `NSOpenGLView` beneath it** (both, exactly as baseview's own `Window::resize` does — miss the GL view and egui lays out to the new size but paints onto the old surface), then signals baseview with its colon-form `viewDidChangeBackingProperties:` so `physical_size` — and egui's per-frame `screen_rect` — follow. That signal is **deferred to the run loop** (`performSelector:withObject:afterDelay:`): baseview's handler takes the same `window_handler.borrow_mut()` that `trigger_frame` holds across `on_frame`, so sending it inline from the editor closure is an unconditional `RefCell` double borrow, and the handler being `extern "C"` turns that panic into `abort()`. The plugin's own route is `nih_plug_egui::ResizableWindow` in `lib.rs` |
| `gpu_timer.rs` | #277 Tier 3 GPU frame timing: TIMESTAMP_QUERY query-set bracketing `render()` + async (frame-late) readback → `Feedback.gpu_ms` for the performance status bar. `None` without the feature |
| `metal_island.rs` | #200 Tier 3 Metal interop island (Mac-gated; startup probe → `Feedback.metal_island_available`/`tensor_gflops`; ships dark, live objc2-metal `imp` = on-Mac). `math::island_matmul_ref` is its CPU verify mirror |
| `rt.rs` | #195 Tier 0 hardware RT: feature detect + BLAS/TLAS build (timed → `Feedback.tlas_ms`) + the ray-query debug pass (+ pure tests). #298 Tier 4: a unit-**sphere** BLAS + `build_with_beads` appends curated particle-bead sphere instances (tagged `RT_BEAD_TAG`) to the TLAS; ships dark (the position readback is the on-Mac step) |
| `rt_shadow.rs` | #195 Tier 1 RT shadows: the screen-space key/fill visibility-mask pass off the depth prepass (consumed at `cube.wgsl::shadow_factor`) |
| `rt_reflect.rs` | #195 Tier 2 RT reflections: closest-hit trace + local-space hit shading into the SSR/composite reflection buffer |
| `rt_ao.rs` | #195 Tier 3 RT ambient occlusion: short hemisphere rays into GTAO's raw-AO target (blur/composite/spec-occlusion unchanged) |
| `rt_gi.rs` | #195 Tier 4 RT diffuse GI: per-pixel one-bounce cosine gather into the SSGI buffer (composite add unchanged) |
| `rt_pathtrace.rs` | #200 Tier 4 progressive path tracer: whole-image trace vs the TLAS into the HDR scene buffer (over the raster scene), ping-pong accumulation, camera-still reset; 'P' toggle, per-display. organon#217 T5 (in `world.rs`, not here): the reset also fires on the glyph ring's payload `generation`, and a rastering preset hands a ring's `FRAME_SETTLED` dwell to the tracer (`pathtrace_active`). #258 Tier 2 adds the opt-in dielectric BTDF (Glass/Refractive = two-interface dielectric with Fresnel split + TIR + Beer–Lambert; Chrome = mirror), threaded via `PtU.params2` from `Shared.ptglass`; enable off → diffuse-only. Composite modes (`ptglass[2]`/`[3]`): Replace overwrites the HDR scene (default); Blend alpha-blends the trace over the raster PBR image by `augment`; GI-add skips the primary-hit direct/emissive/sky (`gi_only`) → INDIRECT-only, additive onto the raster. Three scene-target-blend pipeline variants + Load-vs-Clear. Welded Swept Tubes: the ray tracer traces `Surface.rt_instances` (per-segment cylinders) since `instances` is empty (raster draws the welded mesh). #258 T3: the analytic lens (not in the TLAS) is ray-intersected directly (`lens_hit`) so it focuses. #258 T4: `spectral_on` swaps to a hero-wavelength integrator — glass/lens refract at a per-λ Cauchy IOR (Abbe), reconstructed to RGB via CIE CMFs (a prism throws a real spectrum) |
| `rt_caustic.rs` | #258 T5 **photon-mapped caustics** — the GPU side of the light-tracing pass: clears a per-pixel fixed-point splat buffer, then traces photons from the key light |
| `post.rs::denoise` + `rt_denoise.wgsl` | #200 Tier 4½ p2 edge-aware à-trous denoise of the RT reflection/GI buffers (in place; composite unchanged) |
| `post.rs::neural_denoise` + `rt_ndenoise.wgsl` | #200 Tier 5a neural denoiser — kernel-predicting filter (à-trous base × seeded-MLP modulation); `net = 0` ≡ classical à-trous; reuses `denoise_scratch` |
| `capture.rs` | #135 production frame: fixed-res offscreen target + letterbox blit (+ pure tests) |
| `overlay.rs` | #135 P2 overlay renderer: ab_glyph atlas + formula textures + text/quad pass (+ pure tests) |
| `axes.rs` | #135 P5 capture decoration: axes tubes+cones (surface) + box back-wall grids + their pipelines (+ pure tests) |
| `overlay_meta.rs` | #135 P2 per-generator overlay metadata + live-value `eval` (pure, shared, unit-tested) |
| `organon-render/src/legibility.rs` | **PBR text T2 (organon#217)** — the **legibility harness**: `doc/pbr_text_engine.md` §9's two laws as a number. A hand-readable **fixture** format (`organon-render/tests/fixtures/*.txt` — the Omarchy logo, and an asymmetric one that proves orientation), a box-filter **downsample to the cell grid in linear light** (Rec. 709 luma, area-weighted at fractional pixels, the 2:1 cell aspect taken from the fixture), **Pearson** against the fixture's per-cell luma (blanks in the population, deliberately — plus a lit-only coefficient for the gradient's shape), **bleed** (a dark cell's luma over its lit neighbours' mean) and **stray** (energy in blank cells), judged against a `Thresholds` that is a parameter. Plus `synth`, a CPU painter with blur / noise / gain / scramble so the metric is tested without an adapter. Entry points `assess` / `assess_readback_rgba8` for T3's preset gate; **wired nowhere yet**. Pure CPU, deterministic — `tests/legibility.rs` runs the whole chain, every invariant mutation-tested. ⚠️ Pearson cannot see an affine map, so a uniform fog scores 1.0 on correlation and is caught by stray/bleed alone; `pass()` needs all three. → `doc/arch/render.md` "The legibility harness" |
| `*.wgsl` | shaders (naga-validated by `tests/wgsl.rs`) |
