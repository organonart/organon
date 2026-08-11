# Changelog

Organon was built in the open for about a year before this repository existed, in a
private monorepo, across ~430 changes. **This file is a reconstruction of that arc** —
what got built, roughly in order — rather than a replay of it. Individual PR entries, and
the issue numbers they reference, stayed private with the original.

From here on, this file gets an entry per meaningful change, newest first.

---

## Unreleased

### Console Spike — Tier 2: a backdrop you can type at

- **`organon console background <name>` changes the console's backdrop live.** Four
  substrate materials (`graphite`, `paper`, `slate`, `metal`) and two lighting rigs
  (`studio`, `daylight`), plus the three sources `world` / `off` / `substrate` — typed in a
  console tab, applied on the next frame, with no window flicker and no terminal
  relayout. `organon console rig <name>` picks the light.
- **A third command transport, because it has a third destination.** Console verbs append
  to their own namespaced sidecar (`<ns>-console.txt`) drained by the **console**, not to
  `cli.txt`, which is drained by the World and cannot reach `Shell` state at all. The
  drain reuses the World's own file-length watermark and its construction-time seed
  verbatim, so a backlog from before the console started never replays while a command
  typed a moment after launch always does. Unknown verbs and unknown names are both
  skipped in silence — that is the format's whole versioning story.
- **The #472 material gate now admits the membrane sheet.** `render.rs` split one
  predicate into `cube_draw` (the bevel morph, unchanged) and `material_draw`, so a flat
  lit plane can carry a procedural map stack — the thing Tier 1 recorded as the reason its
  substrate had no surface variation. A uniform-value gate, not a pipeline one, and inert
  with no material configured.
- **The command service is live in the product for the first time.** `console.background`
  and `console.rig` are registered on `organon-shell`'s `CommandService`, with their
  argument schemas built from the material and rig tables themselves, and every drained op
  is dispatched through it — so each one leaves a `CommandRun` record in a real session
  log, success and rejection alike.
- **One table, one guard.** The `organon` CLI's clap value lists are now asserted equal to
  the tables the renderer draws from, so a name that completes is a name that can be drawn.
  (The three *source* words are pinned by a literal on each side rather than bound — the
  two ends are separate `[[bin]]`s; recorded in `SHELL_ARCHITECTURE.md`'s honesty ledger.)
- Launching with `ORGANON_SHELL_BACKDROP=substrate` still publishes Tier 1's snapshot byte
  for byte: no material is applied until one is named.

### Console Spike — the binary is `organon-console`

- **The artifact is renamed; nothing an identifier is read by changes.** The `[[bin]]`
  target `organon-shell` is now **`organon-console`** — so it is
  `cargo build --release --features shell-edition --bin organon-console`, and the old
  spelling now fails with "no bin target named `organon-shell`". Explicitly *not* renamed,
  because each of these is read by something outside this crate: the **crate**
  `native/organon-shell` (`-p organon-shell` still names it), the cargo **feature**
  `shell-edition`, every **`ORGANON_SHELL_*` environment variable**
  (a shipped flag surface), the **`organon-shell` IPC namespace** (a wire identifier the
  `organon` CLI joins on), `%APPDATA%\OrganonShell`, and `SHELL_ARCHITECTURE.md`'s filename.
  Collapsing those needs deprecation aliases and a coordinated change, not find-and-replace.
- **The console introduces itself as Organon Console.** The window title, the `--help`
  header and usage line, `--version`, the startup banner and the `organon-console:`
  diagnostic prefixes now read the public name from a local `PRODUCT_NAME`, deliberately
  shadowing `EDITION.product_name()` — which stays "Organon Shell" because `organon-core`'s
  `Edition` is shared spine. A test pins the split: the header must not say "Organon Shell",
  and `ORGANON_SHELL_BACKDROP` must still be there.

### Console Spike — Tier 1: the lit substrate

- **A second backdrop source for Organon Shell.** `ORGANON_SHELL_BACKDROP=substrate` puts
  one flat, still, lit plane behind the glyphs instead of the generative world: a pure
  `Shared`-state builder (`substrate_scene`) drawn through the existing
  `RenderPath::Membrane` — no new shader — and framed by a pure narrow-lens camera rig
  (`substrate_camera`) at a 10° vertical FOV, re-framed on every resize. `=1` still selects
  the world, unchanged, because the `organon` CLI's override lane drains inside the world's
  frame path and replacing it would kill the console's live response.
- **The world gained an absolute camera rig.** A third arm on the camera finalization
  overrides centre/yaw/pitch/distance/roll/FOV as a set and latches off the auto-follow while
  it is installed; the FOV clamp floor moved 10° → 4° at *both* of the two sites that clamp it
  (moving one alone does nothing).
- **Fixed: the backdrop was vertically squashed.** The texture was sized to the window and
  then painted at UV 0..1 into a panel 30 points shorter. It is now sized to that panel, one
  frame behind, with the same clamps the Mind editor's viewport uses — which also changes, and
  corrects, the existing `ORGANON_SHELL_BACKDROP=1` rendering.
- The legibility scrim's alpha is now a pure `term_view::scrim_alpha`, with its structural
  floor pinned by a test against hostile input.

---

## Before this repository

### The instrument

The first thing that existed was a plugin. Organon began as a faithful reimplementation
of *Organic Math*, a cube-field visualizer, and the reimplementation immediately raised
the question the whole project has been answering since: what else can this generator do?

- A **VST3/CLAP plugin plus a standalone**, built on nih-plug, with the fullscreen visual
  as a **separate process** reading a memory-mapped snapshot. Two processes was an early
  and load-bearing decision: a host's audio thread can't be blocked by a renderer.
- Host **tempo sync** via a PLL, MIDI CC routing, clip export, and audio-reactive band
  analysis — the parameters move to the music, which is the point of it being a plugin.
- The algorithm itself, isolated as pure unit-tested functions: rotate-then-translate
  composition (mirroring OpenGL's `glRotatef`/`glTranslatef` order, which is what makes
  the motion organic rather than mechanical) and a fourth accumulating strand that
  compounds transforms without reset — the source of the tentacle and helix families.

### The renderer

What began as instanced cubes became a full real-time stack, because each new generator
asked for a way to be *seen*:

- PBR materials, image-based lighting, punctual lights, HDR output with true EDR on
  macOS, tone mapping, palettes, SSAO.
- Screen-space reflections, bounced GI, spectral glass with dispersion, a hardware
  ray-traced path, voxel GI, temporal accumulation, and a post stack.
- 27 generators and a matching set of surface modes — how nodes become geometry: swept
  tubes, metaballs, membranes, voxels, neural tissue, plexus, splats.
- Later arrivals: a time-marched **field engine** (PDEs on a grid), a **kaleidoscope**
  pass, a **creature engine**, a **neural-network generator** with a gallery of
  synthesized graphs, and an HDR starfield driven by the embedded Yale Bright Star
  Catalog.
- An in-app **production recorder** for capturing HDR clips, and a **frame harness**
  (`native/verify`) that turns rendering into pass/fail against committed goldens — the
  only test in the project that can see a picture.

### Organon Mind

Then the engine was pointed at something other than music: a language model.

- A **GGUF reader** that parses a model file's header and tensor directory and draws the
  model's **true wiring** — layers, heads, experts, the residual stream — as a structure
  in the same 3-D engine.
- An **activation ring** for live inference, an **embedded llama.cpp runtime** behind an
  opt-in feature (the default build stays C++-toolchain-free), and a synthetic frame
  writer that exercises the whole live path with zero inference.
- A set of **lenses** — quantitative instrumentation, an inference-geometry atlas,
  concept views — and the commitment that governs all of them: **every displayed quantity
  is labeled with its provenance** (measured / derived / proxy / projection), with a
  standing ledger recording which is which and what is still a proxy.

### The workshop around the code

Roughly half the effort that does not show up on screen:

- The **`organon` CLI** — drive the running instrument from a terminal: read state, set
  parameters, apply recipes, take a snapshot.
- **Preset and clip machinery**, an app-support store, network galleries.
- **Documentation discipline** that is enforced rather than hoped for: architecture docs
  updated in the same change as the code, session hooks that measure doc drift, structure
  drift and the context each session costs.
- **CI** running the full edition matrix, and an automated review agent on every PR.

### Taking the engine apart

With three products sharing one codebase, the monolith was split — carefully, in tiers,
with the acceptance test written down first:

- **`organon-core`** — the host-free spine: math, IPC, params, GGUF, editions. Its
  acceptance test is a dependency check: no plugin framework, no GPU crates, no UI.
- **`organon-render`** — the renderer as its own crate: the surface modules, the shaders,
  the star catalog.
- **`organon-mind`** — Mind's own code, free of the plugin framework.
- The `World` god-struct partitioned into ownership clusters, then made
  compiler-enforced rather than conventional.
- A measured answer to "are these crates actually independent?" — cross-crate churn,
  computed by a script rather than asserted, because a number nobody can re-derive rots.

### More platforms

- A **Windows port**: bundle and deploy scripts, DLL-lock handling, HiDPI for the
  standalone, true HDR through the scRGB swapchain, CUDA for the embedded runtime, and
  Windows legs in CI.
- A **WebGPU port** of the same `math.rs` compiled to WASM — built, then deliberately
  **parked**. It is not in this repository: development is Rust-native only, and shipping
  a parked port would have meant publishing a second, staler answer to the same question.

### Organon Shell

The third product: an agent-operating workstation. Founded, built as a five-panel
workspace, falsified by actually using it, and re-founded as a **GPU-composited
terminal** — its own crate and a third `Edition` over the same engine.

### Open-sourcing

The work that produced this repository: an audit of the whole tree, a licensing decision
(permissive engine, GPL plugin — see [`LICENSING.md`](LICENSING.md)), and an export tool
that materializes this public tree from the private monorepo with a byte-identity gate
and a fatal privacy scan, so that what is published is exactly what was reviewed.
