# Organon

**One native application whose identity is data.** You divide the window into regions, declare
what each one holds, and save the arrangement under a name — and that named arrangement is what
somebody means when they say which program they are running. No arrangement is valid without a
live agent in it, taught by loadable skills to operate the application it is running inside.

📌 **`doc/organon_prd.md` §1.1 is the canonical description**, in three lengths. This file, the
sites and `CLAUDE.md` quote it rather than re-authoring it — the identity claim was once spelled
a different way in every document that mentioned it, which is how it came to be stale in all of
them at once. ⚠️ The count lives in §1.1 and deliberately not here: a number copied out of its
source is a number that drifts from it, which is the defect this whole arrangement exists to end.

## What an arrangement holds

A region holds an agent conversation, a scrolling column of instrument panels, a live 3D
viewport, or a piece of media. Three arrangements exist today:

| Arrangement | What it is | |
|---|---|---|
| **The visualizer** | A parametric generative visualizer: 27 generators, a PBR/HDR/ray-traced render stack, 50+ WGSL shaders, driven by MIDI, tempo and audio. What Organon grew out of — and **one of the things it hosts**, not what it is | [organon.art](https://organon.art) |
| **Mind** | Load a `.gguf` and it draws the model's true wiring, read from the file, then lights it up while it runs | [organonmind.org](https://organonmind.org) |
| **The Console** | An agent-operating workstation: a GPU-composited terminal for working with AI agents | |

They are the same engine with a different front-of-house. The algorithm (`math.rs`), every
shader, the IPC snapshot layout and the preset store are identical across all three.

🚨 **The one thing that cannot be an arrangement is the plugin.** Inside a DAW a host owns the
window, the audio thread has hard real-time constraints, and the plugin's identity appears in
saved sessions that outlive any decision made here. `Organon.vst3` / `.clap` is a separate
artifact with a separate lifetime, and stays one.

## How it ships today

⚠️ **Those three arrangements are still three binaries**, chosen by a compile-time `Edition`
rather than by a saved layout. That is the mechanism which currently makes them work; collapsing
it into one binary that opens into a named arrangement is issue #111, and it has not started.
`doc/organon_prd.md` §12 is the honest state of play — what is enforced today, what is designed
and unbuilt, and which claims are direction rather than mechanism.

```bash
cd native
cargo build --release                                              # the visualizer
cargo build --release --features mind-edition  --bin organon-mind  # Mind
cargo build --release --features console-edition --bin organon-console # the Console
```

## Build

Rust via [rustup](https://rustup.rs). macOS, Windows and Linux. On Linux the engine pulls in
ALSA/JACK and X11/GL, so install the dev headers first — without them the build dies inside a
*build script*, which reads like a code error but is not:

```bash
sudo apt-get update && sudo apt-get install -y \
  libasound2-dev libjack-jackd2-dev \
  libx11-dev libx11-xcb-dev libxcb1-dev libxcursor-dev libxrandr-dev libxi-dev \
  libxext-dev libgl1-mesa-dev libxkbcommon-dev libwayland-dev
```

```bash
cd native
cargo test --workspace     # unit tests + offline shader validation, no GPU needed
```

`cargo test` includes `tests/wgsl.rs`, which parse-and-validates **every** shader with naga on
the CPU — shader errors are caught without a GPU, on any machine, in CI.

⚠️ `--workspace` is load-bearing, not tidiness: `native/` is a workspace whose root package is
the plugin, and a bare `cargo test` runs that package **only**. That silently skipped an entire
crate's tests once, and the suite stayed green while it did.

## Run it

The quickest way to see something is the standalone plus its visual — no DAW involved:

```bash
cd native
cargo run --release --bin organon-standalone     # the editor: every parameter, as sliders
```

Then **Open Visual Window** in the editor. The visual is a separate process, fullscreen-capable
— the editor owns the controls, the visual owns the pixels, and they talk through a
memory-mapped snapshot.

**Drive it from a terminal** with the `organon` CLI, which is the fastest way to explore and the
one built for scripting and for agents:

```bash
cargo build --release --bin organon
./target/release/organon catalog --manual        # the whole vocabulary, with ranges
./target/release/organon generator dna           # pick a generator
./target/release/organon set metallic 0.9 exposure -1.5
./target/release/organon snap -o /tmp/look.png   # look at what you made
```

The loop that matters is **see → act → see**: read the state, change one thing, take a snapshot
and check. `organon describe <id>` explains any control in prose, with its range — and
[`doc/guide/`](doc/guide/README.md) is the same material written out, starting from a DAW.

⚠️ **A first `snap` can time out while the visual is still coming up.** Retry it — startup takes
a few seconds and a covered or unfocused window does not render. Note also that `status`, `get`
and `watch` need something *writing* the snapshot (the editor or the plugin), while `snap`,
`set` and `generator` work against the visual alone.

`ORGANON_IPC_NS=<name>` forks the IPC namespace, so two of these run side by side without
trampling each other. The CLI reads it too — export it in the same shell.

**The other two instruments:**

```bash
cargo run --release --features mind-edition  --bin organon-mind    # load a .gguf
cargo run --release --features console-edition --bin organon-console # a terminal; --help works
```

**As a plugin:** `./bundle.sh` writes `target/bundled/Organon.{vst3,clap}` with the visual
embedded inside each (`bundle.ps1` on Windows). Then `./deploy.sh --dest ~/Library/Audio/Plug-Ins/VST3`
installs it where a DAW will look; on Windows `deploy.ps1 -Dest F:\vst3`. macOS Gatekeeper
blocks self-built plugins and the "Allow Anyway" button does **not** work for them — you need
`sudo spctl --global-disable`. Then rescan in your DAW.

⚠️ **One exception: the CLAP on Windows carries no visual.** nih-plug emits it as a bare DLL,
so there is no bundle directory to embed anything into — "Open Visual Window" under a Windows
CLAP host does nothing until you point `ORGANIC_MATH_VISUAL` at the full path of
`organic-math-visual.exe`. `bundle.ps1` prints this on every run rather than skipping it
silently. The VST3 is unaffected on either platform, as is the CLAP on macOS.

## The shape of it

```
native/src              the plugin, the standalone, the visual, the `organon` CLI
native/organon-core     the host-free spine: math, IPC, params, GGUF, editions
native/organon-render   the renderer — 36 surface modules, 50 shaders
native/organon-mind     Organon Mind's own code
native/organon-console    Organon Console's compositor and terminal
```

That is the whole repository: Rust, and the documentation for it. No npm, no
TypeScript, no build step outside cargo.

Two processes, on purpose: the editor owns the controls and a separate binary owns the
fullscreen visual, communicating through a memory-mapped snapshot. `ARCHITECTURE.md` §4
explains why and how they attach.

| Doc | What it is |
|---|---|
| [`doc/guide/`](doc/guide/README.md) | **Using Organon** — install it in a DAW, the three choices, playing it from clips and controllers, presets, output and capture. Start here if you want to *operate* it. |
| [`doc/reference/`](doc/reference/README.md) | Every generator, surface, material, parameter and recipe. Generated from the source and pinned by a test, so it cannot fall behind the code. |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | The durable engine reference — the algorithm, the IPC model, the file map. A reference, not a read-through. |
| [`doc/arch/render.md`](doc/arch/render.md) | The render pipeline in depth: passes, hardware RT, IBL, shaders. |
| [`doc/arch/topology.md`](doc/arch/topology.md) | The crate graph, and what may depend on what. |
| [`MIND_ARCHITECTURE.md`](MIND_ARCHITECTURE.md) | What exists **right now** in Organon Mind, plus its honesty ledger. |
| [`CONSOLE_ARCHITECTURE.md`](CONSOLE_ARCHITECTURE.md) | The same, for Organon Console. |
| [`doc/research/`](doc/research/README.md) | **Deep research evals** — the same brief sent to several models, their reports kept as evidence, and the claims that survived checking against the tree. Read [`FINDINGS.md`](doc/research/FINDINGS.md), not the reports. |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | How changes get made here — start here before writing code. |
| [`SECURITY.md`](SECURITY.md) | How to report privately, and what the real attack surface is. |
| [`LICENSING.md`](LICENSING.md) | Why the engine is permissive and the plugin is GPL. |

## Licence

Split, and the split is deliberate — see [`LICENSING.md`](LICENSING.md).

- **The engine** (`organon-core`, `organon-render`, `organon-mind`, `organon-console`, and the
  WASM/codegen/build tools): **MIT OR Apache-2.0**, your choice. This is the part worth
  reusing, and it is unencumbered.
- **The root crate** — the plugin, standalone, visual and CLI: **GPL-3.0-or-later**, forced by
  the GPLv3 VST3 bindings that `nih_export_vst3!` is built on. Not a preference; nih-plug
  itself is ISC.

Third-party material, including the embedded Yale Bright Star Catalog: [`NOTICE`](NOTICE).
The Organon name and marks are the author's — a licence grants copyright, not trademark.
