# Showing an LLM lens in Organon Console — reconnaissance

> **What this document is.** A **read-only investigation**, taken before any of it is built, into
> what it would cost for Organon Console to show one of Organon Mind's real-time LLM
> visualizations — the `/viz` James asked for on 2026-08-13. It is the same shape as
> `doc/console_portal_recon.md`: a survey pinned to a commit, kept so the next person does not
> re-derive it.
>
> **Read from `main` @ `d9305e4`.** Line numbers are as of that commit and most will move; follow
> the function names. Nothing here was run — every claim is read from source, and where something
> could not be established by reading it says so.

**The framing James gave it:** *"Organon Mind as an app will simply be a different sort of
interface, tailored to working with LLMs. But that's it. All of the core functionality should be
available in Organon and in the console."*

---

## The headline, first and loudest

**The plumbing is nearly free. The honesty is not.**

The console already renders `World` into inline transcript elements and into a floating portal,
already has an absolute camera-rig mechanism it uses per element, and already links every line of
Mind's code with **zero `#[cfg]` gating**. What it does not have is a visualization that is honest
about what it shows.

🚨 **The provenance system does not exist in the 3-D path.** The `=` / `~` / `?` markers live only
in `organon-mind/src/mind_viz.rs`'s 2-D egui widgets. Nothing in `world.rs`, `math.rs`,
`overlay.rs` or `organon-render` reads `FLAG_RESID_MEASURED` or `FLAG_MLP_MEASURED`;
`math.rs::stream_frame_into_scalars` takes `layer_norm`, `mlp_act` and `head_summ` and writes them
into node brightness **without ever looking at `flags`**.

In Mind this is survivable, because the labeled 2-D dashboard sits beside the scene. **In a chat
transcript, `/viz` would be the only thing on screen** — a spinning per-layer glow with nothing
saying that the lights are entropy shaped into a travelling sine.

`CLAUDE.md` is unambiguous about what that means here: *"Provenance is the product, not a nicety…
If you add a readout, it carries its marker."* `/viz` is a readout.

---

## 1. Three things that are NOT obstacles

Each of these was expected to be the hard part and is not.

**The visualizations are not in `organon-mind`.** The 3-D lenses are `organon-core::math` (graph
builders — `gguf_architecture_graph`, `stream_frame_into_scalars`, `embedding_galaxy_graph`,
`attention_to_graph`, `mlp_to_graph`, `brain_graph`, all pure), `native/src/world.rs`
(orchestration — the `topo == 5` live-glow seam at ~`4543`, `build_mind_graph` at ~`11039`), and
`organon-render/src/render.rs` (drawing — `NeuralBatches`, `draw_neural_batches`).
`organon-mind` holds the **data channel** (`mind_ring.rs`, an mmap protocol) and the **2-D
dashboard** (`mind_viz.rs`, pure egui painters).

⚠️ **"The activation ring" is not a visualization.** `mind_ring.rs` is a per-token frame channel.
Nothing draws a ring shape.

⚠️ `organon-render/src/neural.rs` is a false friend — the SIREN-MLP raymarch generator, unrelated
to the LLM lens.

⚠️ `organon-render` does **not** depend on `organon-mind`, despite comments in
`organon-mind/src/lib.rs` and its `Cargo.toml` asserting that it does. The comment describes an
intent, not a state.

**There is no edition gating inside `organon-mind`.** A full scan of `organon-mind/src/` for
`cfg(` returns exactly one hit, and it is `cfg(target_os = "macos")`. The crate's own
`mind-edition` feature is a pure forwarder that nothing in `src/` reads. And `world` — where every
lens seam lives — is compiled for the console: `native/src/lib.rs` guards it
`#[cfg(any(feature = "mind-edition", feature = "shell-edition"))]`. **"Linked" and "reachable"
coincide.** A `/viz` needs no `cfg` surgery.

**Nothing needs moving into `organon-render`.** The hypothesis going in was *a visualization is a
render, so it belongs where any host can consume it*. It is already true: the drawing is in
`organon-render`, the builders are in `organon-core`, the ring reader has no host dependency. What
remains in the root crate is ~150 lines of **orchestration** — edge-detecting `Shared.mind[*]`,
reading sidecars, polling the ring — which is app state by definition and which
`organon-render/Cargo.toml`'s own header says stays put. The move is *unnecessary*, not expensive.

---

## 2. 🚨 What the lenses actually show

`MIND_ARCHITECTURE.md` §3's ledger has five rows. The code is finer-grained than the ledger, and
the difference matters:

| Channel | Provenance | Where decided |
|---|---|---|
| `entropy`, `confidence` | **measured** — real temperature-softmax over real logits | `bin/mind_runtime.rs` |
| top-k (id, prob, decoded text) | **measured** — the actual next-token distribution | same |
| `ctx_used` / `ctx_total` | **measured** | `mind_viz.rs` |
| tokens/sec | **derived** | `mind_viz.rs` |
| `layer_norm` (per-layer residual) | **measured IF the `cb_eval` tap fires, else proxy** — `FLAG_RESID_MEASURED` | `mind_ring.rs`, `mind_runtime.rs` |
| `mlp_act` (FFN) | same, separately flagged | same |
| **`head_summ` (per-head heat)** | 🚨 **proxy, unconditionally** — even when the tap succeeds | `mind_runtime.rs` |
| the 3-D layout | **projection** — imposed, not read | `math.rs`, `params.rs` |

The runtime says so itself: per-head attention weights require the attention matrix to materialise
in the graph, which means flash-attention **off** — a trade not yet made.

And the proxy's shape, so nobody has to imagine it:

```rust
f.layer_norm[l] = ((0.30 + 0.70 * entropy) * (0.45 + 0.55 * wave)).clamp(0.0, 1.5);
f.mlp_act[l]    = ((0.25 + 0.75 * confidence) * (0.40 + 0.60 * wave2)).clamp(0.0, 1.5);
```

**A travelling sine, amplitude-modulated by one global scalar.** The ledger's own tell for the
difference: if the tap is real, the depth profile *rises monotonically with depth* instead of
showing the proxy's travelling wave.

**Projection** is the fourth marker and the ledger's table never uses it, though the code does:
*"The graph is REAL (the network's actual structure); the 3-D layout is IMPOSED (an ANN has no
spatial embedding)"*; the attention lens *"renders attention, not 'thinking'"*; the brain model is
*"stylized anatomy, not an accurate brain"*; the embedding galaxy is a PCA shadow, which is why
its nodes are lit by the **full N-D norm** — the geometry the 3-D projection discards.

### 📌 The ten-minute experiment nobody has run

`bin/mind_runtime.rs` prints, on startup, either `activation tap MEASURED` or
`PROXY — capture returned nothing`. **The ledger records that nobody has ever seen which.** One run
of the CUDA build on the RTX 5090 settles whether the glow is mostly-measured or entirely-proxy,
and therefore how much of `/viz` needs a label rather than a caption. It is a local session's job;
it cannot be done from a cloud one.

---

## 3. What feeds a lens at runtime

| Lens | Needs a live process? |
|---|---|
| Architecture specimen, embedding galaxy, atlas, brain, MLP-from-JSON, attention-from-JSON | **No.** File parse only. Fully static, fully honest. |
| The live per-token glow (`topo == 5`) | **Yes** — a writer must be filling `$TMPDIR/<ns>-mind.bin`. With no frames it falls through to the static graph. |

Two writers exist and they are different products:

- **`organic-math-mind-writer`** — in every build, **zero inference**, emits synthetic frames so
  the streaming path can be exercised model-free. Its top-k is *"fake but honest in shape — it IS
  a softmax."* Fine for a demo; **a lie if presented as inference.**
- **`organic-math-mind-runtime`** — `required-features = ["embedded-llm"]`, built only by
  `deploy.sh --with-llm` / `deploy.ps1 -WithLlm`. CUDA on Windows, Metal on macOS.

**A truthful `/viz` needs the runtime.**

### The namespace fork already does the right thing

`ipc.rs::namespace()` resolves once per process from `$ORGANON_IPC_NS`, else the edition's own
name — `Full → "organic-math"`, `Mind → "organon-mind"`, `Shell → "organon-shell"`, pinned
pairwise-distinct by test. So a Mind session's runtime writes `organon-mind-mind.bin` and a
console would read `organon-shell-mind.bin`: **they do not collide, and they also do not see each
other.**

And the console already closes its own loop with no new code — `organon-shell/src/term.rs` sets
`ORGANON_IPC_NS` on every terminal tab it spawns, so `organic-math-mind-runtime` run **in a
console tab** writes into the ring the console's own in-process `World` reads.

⚠️ **Not established:** whether the console should have a *managed* runtime child like
`mind_console.rs`. There is no such wiring on `main`.
⚠️ **Neither `deploy.sh` nor `deploy.ps1` builds `organon-console` at all.** Whatever installs the
console today is a separate path, and `--with-llm` is what produces the runtime.

---

## 4. The fixed view already exists

`organon-scene/src/substrate_camera.rs`'s `SubstrateRig` is an **absolute six-tuple camera
override** — `frame_plane(extent, fov, aspect)` computes a framing, `camera_arm()` returns
`(center, yaw, pitch, distance, roll, fov)`. Pure glam, headless-testable. `World::set_substrate_rig`
installs it and **latches off the auto-follow**; absolute is the whole point, because framing off
ratcheted deltas is not a rig.

**The console already calls it, per inline element** — `shell_main.rs` computes a rig from the
target's aspect before every surface render. That is precisely *"a preset framing rather than the
orbit camera"*, already shipping. A `/viz` rig would be a second constructor beside `frame_plane`,
taking the lens's bounds.

⚠️ **What does not exist: a saved, named viewpoint.** `pack_camera_preset` captures camera
*motion* dials (`cam_path`, `cam_speed`, `cam_kick`, `cam_damping`), not yaw/pitch/distance, and
the network gallery JSONs carry no camera at all.

⚠️ **A trap already documented:** installing a rig silently disables `console.camera` for that
element — `camera::viewpoint_is_visible` exists to say so. With the backdrop on `substrate`, or
nothing showing the world, `organon console camera --distance 40` succeeds, moves real state, and
changes not one pixel.

---

## 5. The cheapest honest path

**`/surface` is `/viz` minus the honesty and minus the frame rate.** That path already recognises a
slash word, inserts an element plus a driving panel, allocates a texture, publishes a per-element
`Shared`, installs a `SubstrateRig`, renders, and **restores the console's own snapshot afterwards**
so `organon status` never reports a picture that is not the window.

Five steps:

1. A `/viz` verb in the registry (`console/slash-commands`, PR #62, makes this a table entry).
2. A `viz_shared()` beside `surface_shared` publishing `generator = NeuralNetwork` and the topology
   — snapshot composition, **not engine code**. Today the console's snapshot has `mind[]` all
   zeros, so its `World` renders the default cube field.
3. A `SubstrateRig` constructor that frames the **graph** rather than the substrate plane.
4. Real-time. See below — the one piece of genuine new engineering.
5. **The label.** See §2.

### Ranked obstacles

**#1 — the honesty gap (§2).** Not solvable by plumbing. The honest minimum: thread `flags`
through to the drawn element and render a provenance caption beneath it — a caption under an
inline element is trivial where a label *inside* a 3-D scene is not, and `provenance_row` is the
existing idiom. Plus the ten-minute experiment.

**#2 — inline surfaces are not real-time.** The render is gated on a *look change*, with
`SURFACE_RENDERS_PER_FRAME = 1` and `MAX_SURFACE_TEXTURES = 4`. An inline surface today is a
**still picture re-taken when its declared look changes**. Per-frame invalidation reopens the
stated reason that budget is one — each surface render is a whole `World` frame, so N surfaces
mean N extra advances of everything per-frame in the world — and the shared `frame_index` / TAA
jitter hazard `engine_plan` exists to rule out.
🚨 **The portal has no such gate; it draws every frame.** So **a pane or portal `/viz` is
materially cheaper than an inline one** — which points at #56's T5/T6 rather than T4.

**#3 — no runtime lifecycle in the console.** Solvable without new IPC (§3), but it needs a
managed-child equivalent and a build/install path that produces both the console and the runtime.

---

## Corrections to the record, found on the way

- **Five workspace members, not four** — `organon-scene` has joined `organon-core`,
  `organon-mind`, `organon-render`, `organon-shell`.
- **`doc/arch/topology.md` contains no dependency rules**, despite `CLAUDE.md` assigning it
  ownership of *"the crate graph and what may depend on what"*. The rules that exist are in
  `CLAUDE.md`'s repository map and in each manifest's header. *(Being addressed on
  `console/rename-shell`, PR #59.)*
- **`MIND_ARCHITECTURE.md` still describes `world` as mind-edition-only.** `native/src/lib.rs`
  compiles it for `shell-edition` too, and has for some time.
