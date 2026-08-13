# Organon — project context

**This repo houses three products**, built from one cargo **workspace**:

1. **Organon** — a parametric 3D generative visualizer, a "hyperscope" for the space of
   possible forms. A VST3/CLAP **plugin + standalone** with a separate-process
   fullscreen visual. It began as a faithful reimplementation of *Organic Math* (a
   cube-field visualizer) and grew far past it: 27 generators, a full PBR/HDR/ray-traced
   render stack, beat- and audio-driven motion.
2. **Organon Mind** — a **standalone app for watching a language model think.** Load a
   `.gguf` and it draws the model's true wiring, read from the file, then lights it up
   while it runs. Not a feature of Organon and not a fork of it: **its own crate**
   (`native/organon-mind`, nih-plug-free) over a shared engine.
3. **Organon Shell** — an **agent-operating workstation**: a native GPU-composited
   workspace for working with AI agents. Its own crate
   (`native/organon-shell`, nih_plug-free, standalone-only permanently — no plugin
   identity, ever) + a third `Edition`.

**Naming convention.** *Organon* = the visualizer product (host plugin name, window
titles, bundle `Organon.{vst3,clap}`); *Organon Mind* = the analysis product (its own
name, window title and IPC namespace, no bundle). *"Organic Math"* = the original
cube-field generator (`GeneratorMode::Original`) + its algorithm. Internal identifiers
keep the old name **on purpose** — crate `organic-math-native`, the binary
`organic-math-visual`, `OrganicMathParams`, IPC/sidecar paths, the
`~/Library/Application Support/OrganicMath/` store, and especially the
**VST3 class ID / CLAP ID** (changing those orphans the device in every saved DAW
session).

⚠️ **"On purpose" is a reason, not a blanket rule — apply the reason.** Everything in
that list is load-bearing *because something else reads it*: a host reads the class ID,
another process reads the IPC paths, an existing install reads the store directory.
A binary's **file name** is read by whatever launches it, so the test is "does anything
launch this by name?". For the visual, yes — `spawn_visual()` probes for
`organic-math-visual`, so renaming it silently breaks "Open Visual Window" from inside
a host. For the two front-of-house apps, no: nothing spawns them, so
**`organon-standalone`** and **`organon-mind`** carry the product name. Don't rename an
internal identifier for tidiness, and don't refuse to rename a leaf out of deference to
this list.

- Repo: `github.com/organonart/organon`
- Sites: **organon.art** and **organonmind.org** (built elsewhere; not in this repo)

> **A note on issue numbers.** Comments and docs throughout this tree cite `#N` — mostly
> below ~#700. Those point into the tracker this project used before it was
> open-sourced, and they are **not resolvable here**. They are kept rather than stripped
> because they are load-bearing provenance: a comment saying "measured in #658, the
> shebang pin does not fire under MSYS" records *that someone checked*, which is the
> part worth keeping even when the ticket is gone. `Shell #N` is the same thing for
> Organon Shell, which was planned in a tracker of its own. New work cites issues in
> this repo normally.

---

## The docs — who owns what

**This file owns**: project context, conventions, the toolchain, and the build/install
workflow. It does **not** describe the architecture or the current state — those have
their own maintained homes, and duplicating them here is how this file rotted once
already.

| Doc | Owns | Cadence |
|---|---|---|
| **`ARCHITECTURE.md`** | the **native** app: two-process IPC, `Shared`, generators, params, the file map, "how to add X" | update in the **same change** as any architectural shift |
| **`doc/arch/render.md`** | the **render pipeline in depth** — passes, `RenderFrame`/`RenderPath`, hardware RT, IBL, shaders. A *child* of `ARCHITECTURE.md`; **not** auto-injected | same, whenever a render-side file moves |
| **`doc/arch/topology.md`** | the crate graph and what may depend on what | same |
| **`MIND_ARCHITECTURE.md`** | **Organon Mind**'s living state — what exists *right now* (not a spec, not a roadmap), plus the honesty ledger | update in the **same change** as every Mind PR |
| **`SHELL_ARCHITECTURE.md`** | **Organon Shell**'s living state | update in the **same change** as every Shell PR |
| **`doc/guide/`** | **the user documentation** — installing into a DAW, the generator/surface/material model, playing it from clips and controllers, presets, output. Narrative and hand-written; describes *mechanisms*, never counts | update when a user-visible behaviour changes |
| **`doc/reference/`** | every generator / surface / material / parameter / recipe. **GENERATED** by `organon docs` from the prose in `agent.rs` + `recipe.rs`; never hand-edit — a test (`generated_reference_is_current`) fails the build on drift | regenerate in the **same commit** as any description change |
| **`CONTRIBUTING.md`** | **the process**: how to scope work, the tier pattern, the review cycle, the verification bar | read before scoping feature work |
| **`SECURITY.md`** | how to report privately, and what the real attack surface is — including which parts are by design | update when you change a trust boundary |
| **`LICENSING.md`** | why the licence is split across crates, and what that constrains | read before touching a `license` field |
| **`CHANGELOG.md`** | per-release history | an entry per meaningful change |

**Hooks enforce the doc discipline** (`.claude/settings.json` — the file is the
authority; re-count with
`python3 -c "import json;d=json.load(open('.claude/settings.json'));print(sum(len(h['hooks']) for g in d['hooks'].values() for h in g))"`):

- *SessionStart* — `load-architecture-doc.sh` injects the **root** `ARCHITECTURE.md`
  into every session, so it is in context from turn one (the harness auto-loads
  `CLAUDE.md` but not it); `doc-staleness-check.sh` reports any durable doc that has
  **drifted** — more than 8 commits have touched its subject since it was last updated;
  `structure-drift-check.sh` reports the largest functions and widest structs with
  per-session deltas; `context-budget-check.sh` prices what the first of those costs.
  The first three are silent when nothing has moved, which is the normal case; the
  budget line always prints, because a number you only see when it is already bad is how
  the injected core doubled without anyone noticing.
- *Stop* — `architecture-doc-check.sh` reminds you if a load-bearing file changed
  without a matching architecture-doc update; `doc-coherence.sh` checks the durable docs
  for duplicate table keys and unbalanced code fences.

`architecture-doc-check.sh` carries one rule per durable doc:

| Changed | Reminds about |
|---|---|
| `params.rs`, `ipc.rs`, `param_table.rs` | `ARCHITECTURE.md` |
| `render.rs`, `world.rs`, `post.rs`, `env.rs`, **`rt*.rs`**, `gi.rs`, `shadow.rs`, `temporal.rs`, `fx.rs`, `vxgi.rs`, `bin/visual.rs`, **`*.wgsl`** | `doc/arch/render.md` |
| `edition.rs`, `mind_*.rs`, `gguf*.rs`, `bin/{mind_runtime,mind_writer}.rs` | `MIND_ARCHITECTURE.md` |
| `organon-shell/src/*.rs`, `organon-shell/Cargo.toml` | `SHELL_ARCHITECTURE.md` |

A trigger may be a literal path **or a glob** (`*` within one path segment) — the RT
subsystem is seven `rt*.rs` files and the render doc covers 50+ shaders, so enumerating
today's files would silently stop covering tomorrow's. **The table lives in
`.claude/hooks/doc-rules.sh`**, shared by the Stop check ("you changed X without Y") and
the SessionStart staleness check ("Y has fallen behind X"). Two questions, one table —
a second copy would rot.

⚠️ **"Changed" means *this session*, and it is deliberately not the branch-vs-main diff.**
`.claude/hooks/session-changes.sh` owns that definition for both Stop checks: commits whose
committer date falls inside this session, plus the working tree. The obvious spelling —
`git diff $(git merge-base origin/main HEAD)...HEAD` — is what it replaced, and on a
long-lived branch it makes every session inherit every earlier session's obligations, which
trains everyone to dismiss the reminder. The session boundary comes from the stdin payload's
`transcript_path`; that file has the measurements and the fallback behaviour.

**Exactly ONE doc is SessionStart-injected: the root `ARCHITECTURE.md`.** Everything
else — `doc/arch/render.md`, `MIND_ARCHITECTURE.md`, `SHELL_ARCHITECTURE.md` — is **read
on demand**, which is the whole point: the injected core stays small and the depth is
one `Read` away. Don't trust this sentence for the count; ask the file that decides it:

```bash
grep -o 'load-[a-z-]*-doc\.sh' .claude/settings.json | sort -u
```

⚠️ **So `MIND_ARCHITECTURE.md` and `doc/arch/render.md` are Stop-checked but never
injected** — open them deliberately when your work touches Mind or the render pipeline,
rather than waiting for the reminder.

**Growth is not the failure mode; unnoticed growth is.** The docs grow because the
same-change discipline works. If `context-budget-check.sh` starts complaining, the fix
is to move a subsystem section into `doc/arch/` or to raise the budget *on purpose,
with a reason written in the hook* — never to let it drift past silently.

```bash
bash .claude/hooks/context-budget-check.sh          # what a session costs right now
CONTEXT_BUDGET_BYTES=160000 bash .claude/hooks/context-budget-check.sh   # see the warning
```

⏸ **Some hook machinery here is inert, and none of it is an oversight.** The hooks
directory crosses from upstream whole, so it carries rules for things this repo does not
have: `load-web-architecture-doc.sh` is present but **not registered** at all, while
`web-architecture-doc-check.sh`, `doc-rules.sh`'s web row and `status-week-check.sh` **are**
registered and simply never match — the files they watch (`web/`, `STATUS.md`) are not
here. That is safe by construction rather than by luck: each opens with an existence guard
(`doc-coherence.sh`'s loop is `[ -f "$f" ] || continue`, `status-week-check.sh`'s first line
is `[ -f "$status_file" ] || exit 0`), so a missing target is skipped silently, never an
error. Present-but-unregistered and registered-but-inert are different states;
`.claude/settings.json` is what decides which is which.

**Keep this file short.** If something you want to add belongs to one of the docs above,
put it there.

---

## Invariants that bite

1. **Never touch the VST3 class ID / CLAP ID.** It orphans the device in every saved DAW
   session — the single most destructive possible edit. Equally: never *add* a second
   one. Organon Mind and Organon Shell are standalone-only on purpose.
2. **`Shared` (the IPC snapshot) is append-only.** Never reorder or insert fields;
   existing byte offsets are load-bearing across the plugin↔visual boundary. Append at
   the tail (or into a documented spare slot), bump `LAYOUT_VERSION`, re-pin the
   goldens, and give new preset fields a serde default so old presets still load.
3. **A param is a chain, not a line**: `params.rs` → `param_table.rs` → `to_shared()`
   → `ipc.rs` → the visual's `build_uniforms`/shader, usually plus `clip.rs` (CC map)
   and `preset.rs` (capture/apply). Follow the whole chain or the param exists and
   does nothing. `ARCHITECTURE.md` §17 is the checklist.
4. **New capability defaults to inert** — off, or to a value that reproduces today's
   look (dispersion 0 = today's glass; palette `Native` = current). This is what lets
   large features land one tier at a time over weeks.
5. **Don't stack PRs.** Branch every change off `main`, merge to `main`. A PR stacked on
   another PR's branch can land on a dead branch and **never reach `main`** even though
   GitHub says "merged". If you must stack, merge the *base* first with "delete branch"
   ticked so GitHub retargets the child.
6. **The licence split is deliberate — don't flatten it.** The engine crates are
   MIT OR Apache-2.0; the root crate is GPL-3.0-or-later because `nih_export_vst3!` is
   implemented over GPLv3 `vst3-sys`. "Making the licences consistent" would relicense
   ~100k lines of reusable engine onto the terms of a plugin binding. `LICENSING.md`
   owns the reasoning.

---

## Three products, one workspace

Organon, Organon Mind, and Organon Shell are the **same engine with a different
front-of-house**. Mind and Shell each have their own crate; what stays shared is the
engine beneath them. This is an **edition, not a fork**: the algorithm (`math.rs`),
every shader, the `Shared` layout, and the preset store are identical across them.

```bash
cargo build --release                                              # Organon (default)
cargo build --release --features mind-edition --bin organon-mind   # Organon Mind
cargo build --release --features shell-edition --bin organon-console # Organon Shell
```

`organon-core/src/edition.rs` holds a compile-time `Edition` (`Full` | `Mind` |
`Shell`), chosen by the `mind-edition` / `shell-edition` cargo features (**both
default OFF**, so a plain `cargo build` / `cargo test` / `bundle.sh` keeps producing
exactly today's Organon; both at once is a `compile_error!`). An edition drives **six
behaviors** — `edition.rs`'s module doc is the authority and `ARCHITECTURE.md` §4.1 owns
the mechanism. (Don't trust a prose count here; that is exactly the line that went stale
before.)

Things that are easy to get wrong:

- **Mind and Shell are standalone-only, and must stay that way.** No VST3/CLAP export —
  which means **no second plugin class ID**, and none of the host audio-thread
  constraints. Adding a plugin identity is not a feature, it's a new lifetime
  commitment.
- **The IPC namespace fork is the one cross-product invariant.** Every `$TMPDIR` mmap and
  sidecar funnels through `ipc.rs`'s `ns_file(suffix)` → `$TMPDIR/<namespace>-<suffix>`,
  resolved once per process. That is what lets a Mind session and an Organon session run
  **simultaneously** without trampling each other. Any new IPC file must go through
  `ns_file` — a hard-coded `$TMPDIR` path silently breaks co-existence. `$ORGANON_IPC_NS`
  overrides it at runtime, which is how the one visual binary serves every edition.
- **Provenance is the product, not a nicety.** Everything Mind displays is labeled
  **measured** (read from the file) / **derived** (an exact function of what was measured)
  / **proxy** (standing in for something not yet instrumented) / **projection**. If you
  add a readout, it carries its marker and goes in `MIND_ARCHITECTURE.md` §3's honesty
  ledger. The current #1 gap is recorded there: the per-layer generation glow is a
  *labeled* proxy (entropy + confidence), not real activations.
- **Two extra Mind binaries, both behind default-off features or opt-in flags:**
  `organic-math-mind-writer` (synthetic activation-ring frames, zero inference — the
  model-free proof) and `organic-math-mind-runtime` (the real embedded llama.cpp runtime,
  `required-features = ["embedded-llm"]`, installed by `./deploy.sh --with-llm`).

⚠️ **Default-off features mean the default build does not compile them.** `cargo build`
and `cargo test` skip `mind_main.rs`, `shell_main.rs` and `bin/mind_runtime.rs`
entirely, so you can break an edition and see a green suite. If your change touches
anything shared — `lib.rs`, `params.rs`, `preset.rs`, `ipc.rs`, `world.rs`, the `mind_*`
modules — build the other editions too before calling it green.

---

## Workflow

Branch per change off `main` → PR it → **close at least one automated review cycle**
→ merge. Every PR is reviewed by a Claude agent (`.github/workflows/claude-review.yml`,
rubric in `.github/organon-review-guide.md`); waiting for it, reading the findings, and
either fixing them or saying why not is part of the job.
`.github/workflows/claude.yml` responds to `@claude` mentions. `.github/workflows/ci.yml`
builds and tests **every edition** on each PR — read it, it is not a required check.
**`CONTRIBUTING.md` is the full protocol**, including how to read a CI result without
being misled.

End commit messages with a co-author trailer naming the model that actually wrote them:

```
Co-Authored-By: Claude <noreply@anthropic.com>
```

**Name the model you actually are**, not whatever this line last said. The trailer is
attribution, so a newer model signing an older name is simply wrong.

⚠️ **Never run `cargo fmt` in this repo.** A bare `cargo fmt` reformats the whole tree
and buries the real diff. Format your own edits manually.

---

## The algorithm

The organic motion comes from two things the first naive port got wrong:

1. **Rotate-then-translate (R·T), not T·R.** Mirrors OpenGL `glRotatef`×3 then
   `glTranslatef`×3: translation happens in the rotated frame, so a loop whose
   rotation grows with its index sweeps nodes around an arc → spiral/helix.
2. **The accumulating `q` strand.** A 4th loop that compounds transforms with no reset
   (turtle-style) → tentacles/jellyfish/DNA-helix.

`native/organon-core/src/math.rs` (`compose_step`, `draw_tissue`) is the **source of
truth**, pure and unit-tested. The web port compiles that same file to WASM rather than
re-porting it, which is what guarantees parity. The legacy R3F app under `/src` still
runs the *original* algorithm and has diverged — see `ARCHITECTURE.md` §3.

---

## Repository map

```
native/       THE ENGINE — a cargo WORKSPACE → THREE products and 8 binaries
              (plugin cdylib, standalone, visual, `organon` CLI, mind-writer,
              mind-runtime, organon-mind, organon-console). Count the root crate's
              files (`ls native/src/*.rs native/src/bin/*.rs | wc -l`) rather
              than trusting a number in prose.
                                → ARCHITECTURE.md · MIND_ARCHITECTURE.md
native/organon-core/   the engine's HOST-FREE spine: math, ipc, params, gguf,
              edition, tabs. No nih_plug / wgpu / egui, enforced by
              `cargo tree -p organon-core`. Re-exported by the main crate, so
              `crate::gguf` etc. still resolve.     → ARCHITECTURE.md §19.0
native/organon-render/ the RENDERER: `render` + 36 surface submodules, `axes`,
              `chamber`, 50 shaders. No nih_plug / egui / winit.
              ⚠️ This is `world::render`, NOT `world.rs` — the world is the app
              state and stays in the root crate.    → doc/arch/render.md
native/organon-mind/   Organon Mind's own code (activation ring, Mind UI,
              model shell). No nih_plug.            → MIND_ARCHITECTURE.md
native/organon-shell/  Organon Shell's compositor lib. No nih_plug, ever.
                                                    → SHELL_ARCHITECTURE.md
doc/arch/     the architecture child docs (render, topology)
doc/          Organon Mind's public doc set (PRD, build plan, the honesty essay)
.claude/skills/  organon-cli — driving the running app via the `organon` command.
              A real directory, NOT a symlink: a git symlink here materialises as a
              24-byte text file on any Windows checkout and the skill silently
              does not load (#19).
```

That is all of it. This repository is Rust and its documentation — no npm, no
TypeScript, no build step outside cargo.

For anything more specific — which file owns a subsystem, the `Shared` layout, the
generator/surface/material tables, the render passes — read `ARCHITECTURE.md` (§19 is a
per-file map). It is hook-enforced and current; this file is not the place to duplicate
it.

---

## Native: toolchain, build, test

Rust via [rustup](https://rustup.rs). **On a fresh Linux host, install the system dev
headers first.** The crate pulls in ALSA/JACK (nih-plug's standalone audio backends) and
X11/GL (baseview); without them `cargo build` dies in a *build script*, which reads like
a code error but isn't:

```bash
sudo apt-get update && sudo apt-get install -y \
  libasound2-dev libjack-jackd2-dev \
  libx11-dev libx11-xcb-dev libxcb1-dev libxcursor-dev libxrandr-dev libxi-dev \
  libxext-dev libgl1-mesa-dev libxkbcommon-dev libwayland-dev
```

Symptoms if you skip it: `failed to run custom build command for alsa-sys`, then
`… for x11` — the real message ("system library `x11-xcb` not found") is buried in the
build script's **stderr**, far below a wall of `rerun-if-env-changed` lines. None of
this applies on macOS or Windows. `nih-plug` is a **git dependency**, not on crates.io.

```bash
cd native
cargo build --release
cargo run --bin organic-math-visual --release   # the visual alone
cargo run --bin organon-standalone --release    # the editor
./bundle.sh                     # → target/bundled/Organon.{vst3,clap}  (bundle.ps1 on Windows)
cargo test --workspace          # math, params, IPC goldens + offline WGSL validation
./verify.sh                     # the frame harness (needs a GPU)

# The other editions — the default build does NOT cover them
cargo build --release --features mind-edition  --bin organon-mind
cargo build --release --features shell-edition --bin organon-console
cargo test  --workspace --features mind-edition
cargo test  --workspace --features shell-edition
cargo test  -p organon-shell    # the compositor lib alone — the tight loop

# organon-core — the host-free spine. Seconds, not minutes: it pulls no
# nih_plug/wgpu/egui, so this is the tight loop for gguf/edition/tabs work.
cargo test -p organon-core
cargo tree -p organon-core      # THE acceptance test: no nih_plug, wgpu, egui
```

🚨 **`--workspace` on `cargo test` is load-bearing, not tidiness.** `native/` is a
workspace whose **root package** is `organic-math-native`, and a bare `cargo test` runs
the root package **only**. Extracting `organon-core` therefore silently stopped running
the **44 tests it had at the time** — the suite shrank and **stayed green**, which is exactly how a
coverage loss hides. Among those 44 is the **IPC namespace-pinning test**, the one
guarding the invariant that lets a Mind session and an Organon session coexist. Always
`--workspace`; it covers members added later, which a hand-maintained `-p` list would
not.

Use `-p` to *narrow* deliberately. The edition features are **forwarded** to core, so
turning one on for the main crate turns it on there too: `EDITION` resolves in exactly
one place and the two cannot disagree.

---

## macOS install / test

- The custom VST3 folder is `~/Documents/vst3` (set the same path in your DAW's prefs).
- **Deploy after every native change**: run **`native/deploy.sh`** — it builds +
  bundles (visual embedded, ad-hoc signed), installs to `~/Documents/vst3`, installs the
  `organon` CLI + zsh completions, and reminds you to Rescan. ⚠️ **macOS only** (needs
  `codesign`).
- `deploy.sh` also installs the **network gallery** (`native/assets/networks/*.json` →
  `~/Library/Application Support/OrganicMath/networks/`). These are repo files, **not**
  embedded in the `.vst3`, so re-deploy whenever the gallery changes (regenerate with
  `python3 native/assets/networks/generate.py`). Idempotent.
- **`./deploy.sh --with-llm`** (opt-in) also builds the embedded llama.cpp inference
  runtime and embeds it *inside* the bundle, so the Mind tab can launch it as a child.
  Needs `cmake`. The plain `deploy.sh` stays llama.cpp/C++-free and fast.
- **Gatekeeper blocks self-built plugins.** The "Allow Anyway" button does NOT work for
  plugins; you need `sudo spctl --global-disable` (re-enable with `--global-enable`).
- After reinstalling, **Rescan** in your DAW. If it cached a failed scan, quit it and
  clear its plugin-scan cache before reopening.
- After a `LAYOUT_VERSION` bump, close and reopen the visual as well.
- The plugin is an audio-effect (stereo passthrough) with MIDI input, so it must sit on
  a MIDI-receiving track for clips to reach it.

## Windows install / test

Same rule, other arm: **`native\deploy.ps1`** (→ `F:\vst3` by default, `-Dest`
overrides; `-WithLlm`, `-AddToPath`, `-Force`) and **`native\bundle.ps1`**. Galleries
land in `%APPDATA%\OrganicMath\` because `preset.rs` uses `dirs::data_dir()`; follow
that function, never a copied path. **The scripts' own headers carry the detail** — read
them before editing. Three things differ from macOS:

- **No codesign, no Gatekeeper** — ad-hoc signing is a macOS concept; nothing replaces it.
- **A loaded DLL cannot be replaced.** Windows holds a mandatory exclusive lock and
  reports it as "Access to the path is denied". `deploy.ps1` detects it and refuses,
  naming the likely host; `-Force` kills it (unsaved work included). The visual is
  stopped automatically — our own child, no user state.
- **CLAP cannot carry the visual**: nih-plug emits it as a bare DLL, so there is no
  `Contents/` to embed into. Set `$env:ORGANIC_MATH_VISUAL` under a CLAP host.
- **HiDPI is the standalone's problem alone.** The visual (winit) and the plugin (the
  host's `set_scale_factor`) both learn the scale for free; the standalone had nobody to
  ask and rendered at 1×. `standalone.rs` now queries Windows and injects `--dpi-scale`,
  clamped so the window still fits. Pass `--dpi-scale N` to override.

---

## What can and can't be verified where

**Without a GPU** (CI, most cloud sessions) the bar is:

- **Native** — `cargo build --release && cargo test --workspace`. `tests/wgsl.rs`
  parse+validates all shaders with naga offline, catching binding/type/uniformity errors
  without a GPU.
- **The other editions** — **plus** their `--features` builds. Those features are
  default-off, so the line above does not compile them and a green suite says nothing
  about them.

That is the ceiling. It does **not** catch pipeline/layout mismatches, runtime GPU
behaviour, egui layout, or the actual look. A finished PR from such a session is
**"green and ready to deploy"**, never "verified working" — say it that way.

📌 **CI runs every native leg, so the bar is something you *check*, not something you
*perform*.** `.github/workflows/ci.yml` builds and tests each edition, plus a Windows
cross-check and a real Windows build, on every PR.

- **What that changes.** The proof of "it compiles and the tests pass" lives on the PR.
  Don't re-run a full `cargo build --release` just to produce evidence you can read off
  a check. A local run is still the right tool while you're *working* (a tight
  `cargo check`, one test module, `cargo test <filter>`).
- **What it does not change.** The ceiling is identical — CI runs the same commands and
  knows nothing more than you would. It also does not excuse *ignoring* an edition: if
  your change touches shared ground, every leg must be green, and "the default one
  passed" is the failure mode that matrix exists to close.
- **When CI can't answer.** The trigger is `pull_request` + `workflow_dispatch` only,
  and the workflow is path-filtered — so pre-PR work and docs-only PRs get no run at
  all. Build locally then, or open the PR early. The checks are **not** marked required
  in branch protection, deliberately (a required check plus a path filter deadlocks a
  docs-only PR). Not required means **you have to look**.

**With a GPU** the bar is higher and it's on you to clear it: deploy, then drive the app
yourself (the `organon` CLI first — `status`, `set`, `snap`, `record`) and report what
you *saw*, with evidence. `native/verify.sh` turns frames into pass/fail against
committed goldens.
