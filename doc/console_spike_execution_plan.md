# Console Spike — execution plan

**For:** issue #4 (the spike), a vertical slice through #3 (the console).
**Audience:** the coordinator session that runs this, and the sub-agents it dispatches.
**Status:** written 2026-08-10, before any tier started. Update it as tiers land — a plan
nobody amends is a plan nobody read.

---

## 1. What this document is for

Issue #4 says *what* to build and in what order. This says *how the work is divided*, which
is a different problem, and the one that decides whether parallelism helps or produces a
week of merge conflicts.

The short version: **spread on reconnaissance and on leaf modules; gather on integration.**
Everything that touches the wgpu device, the window, or a hot shared file has exactly one
writer at a time, always.

---

## 2. Preconditions

**Repository.** `organonart/organon` — the public platform repo, checked out locally at
`C:\Users\james\Documents\GitHub\organon`. This is where console development happens; the
private annex keeps strategy only.

**Branch.** The spike lives on **`organon-console-spike`**, already created, pushed and
tracking. It is the home for the whole spike — the plan, the brief, the demo script and every
tier. It is checked out and ready; do not start by branching off `main`.

How tiers land on it: give each tier its own short branch off the spike branch
(`console/spike-t1`, `console/spike-t2`, …) and PR it **into `organon-console-spike`** as its
beat check passes. That is what gets `claude-review.yml` running once per tier instead of once
on an enormous diff at the end. The spike branch itself PRs to `main` when the spike lands.
Phase 0's brief is the first such PR, before Tier 1 — its explicit job is to correct this plan,
and that correction should be a reviewable diff rather than a silent edit.

**Machine.** ORGANON-ONE — Ryzen Threadripper PRO 9955WX, RTX 5090, Windows 11 Pro,
Windows PowerShell 5.1 (`&&` is a parse error; use `cmd; if ($?) { next }`). `cargo`/`rustc`
on PATH. No Node/npm.

**Build.**

```
cargo build --release --features shell-edition --bin organon-console
cargo build --release --bin organon
```

The console requires `shell-edition`; the `organon` CLI does not. `shell-edition` and
`mind-edition` are mutually exclusive (a `compile_error!` in `organon-core`'s `edition.rs`
enforces it).

**Run.** `organon-console --help` is the documentation for every dev flag —
`ORGANON_SHELL_BACKDROP`, `ORGANON_SHELL_SCRIM`, `ORGANON_SHELL_CMD`, `ORGANON_SHELL_TABS`,
`ORGANON_SHELL_DEFAULT`, `ORGANON_SHELL_PTY_DEBUG`. Read it before inventing a flag; the
scrim line is formatted from constants deliberately, so add a flag and update the help in the
same change.

**Read first, in this order:** `SHELL_ARCHITECTURE.md` (the code-grounded state), issue #3
(the design), issue #4 (the beats). `CLAUDE.md` and `CONTRIBUTING.md` in this repo govern
everything else.

**Hooks that will fire.** `.claude/hooks/doc-rules.sh` requires `SHELL_ARCHITECTURE.md` to
move in the same change as `native/organon-shell/*`. `structure-drift-check.sh` watches
`lib.rs`. Do not fight them — they encode real lessons.

**Verify before you start.** Build both binaries, launch the console, open a Pi or Claude
Code tab, run `htop` (WSL) in a third, and confirm `ORGANON_SHELL_BACKDROP=1` shows the world
behind the glyphs. **If Tier 0 does not work on this machine, nothing below is a plan — it is
a wish.** Fix that first and record what it took.

✅ **Verified 2026-08-10 on organon-one** — the full record, including what did not work
(the native `claude` harness is not on this machine's PATH; the default look is not
demo-grade at rest; `snap`/`record` have no reply side in-console), is the Tier 0 section
of `console_spike_as_built_brief.md`. One rule it produced: **a second console instance
always forks `ORGANON_IPC_NS`** — two instances in one namespace are two seqlock writers
on one mmap.

---

## 3. The parallelization model

Three kinds of work. Two of them spread.

### 3.1 Reconnaissance — spreads wide, near-zero risk

Read-only questions answered against the tree. Output is prose, not code, so conflicts are
impossible and the fan-out is free. This is the most under-used form of parallelism and the
highest-value one here, because every tier below currently contains assumptions about code
none of us has read closely.

### 3.2 Leaf modules — spread with declared file ownership

New files containing pure functions: no GPU, no egui context, no window. Camera geometry,
cell-rect mapping, schema types, reserved-row arithmetic, the strip as a function of its
payload. Each has exactly one owner, declared *before* dispatch, and lands with tests that
run headless.

This is also the house verification shape (`row_grid`, `layout_workstation`,
`surface_action`): if a piece of a tier has no pure function, it has probably put logic
somewhere it will rot.

### 3.3 Integration — never spreads

Anything touching `shell_main.rs`, `term_view.rs`, `native/src/lib.rs`, `Cargo.toml`, the
wgpu device, or `SHELL_ARCHITECTURE.md`. **One writer per tier.** These are the widest merge
surfaces in the tree and a spacing or wiring diff conflicts on every hunk.

### 3.4 The shape of a tier

```
[recon, if the tier has unknowns]  →  spread, gather into a brief
[leaves]                           →  spread, one owner per file, headless tests
[integration]                      →  one writer, serial
[beat check]                       →  the coordinator, on this machine, with eyes
```

No tier begins before the previous tier's beat check passes.

### 3.5 Model assignment

The coordinator runs on **Fable 5**. **Every sub-agent runs on Opus 5** — pass
`model: "opus"` explicitly on every dispatch rather than relying on inheritance.

**The coordinator writes no implementation code.** Reconnaissance, leaf modules and
integration are all dispatched. What stays with the coordinator is everything that is not
typing:

- deciding what gets dispatched and to whom, under §6's ownership rules
- reading what comes back, and rejecting it when it is wrong
- running the build and the tests
- the beat checks
- the commits

**This is a feature, not a constraint.** The standard failure of a coordinator session is
that it starts implementing, stops coordinating, and then dispatches an agent into a file it
is itself editing. A coordinator that does not write code cannot make that mistake, which is
what makes §6's one-writer-per-file rule enforceable rather than aspirational.

⚠️ **"Delegate everything" does not mean "delegate everything at once."** The integrator lane
is **one sub-agent at a time**, and the coordinator waits for it to finish
(`run_in_background: false`) before dispatching anything that touches the same files.

📌 **Tier 3's schema no longer needs a serial design step** — it was settled ahead of the
spike in `console_discover_schema.md`, so Tier 3's leaves fan out immediately. That was the
point of settling it away from a demo deadline.

---

## 4. Phase 0 — reconnaissance fan-out

Six independent read-only questions. Dispatch all six at once; none depends on another.
Each returns a written answer with file paths and line references, no code changes.

✅ **Ran 2026-08-10; the brief is ANSWERED.** Read `console_spike_as_built_brief.md` before
dispatching any tier — several tier descriptions below were corrected by it, and the
corrections are folded into §5 and §6.

**R1 — the compositing seam.** In `shell_main.rs`, where exactly is the `World` rendered and
painted under the glyphs? Function signatures, the render-target format, and what the
"measured render-sRGB/sample-linear gamma pair" concretely is. **What would a second,
alternative background source have to satisfy to drop into that same seam?**

**R2 — the camera.** How does the `World`'s camera acquire its projection? Is it
parameterizable to an arbitrary near-orthographic rig, or is the projection assembled
somewhere fixed? What is the smallest change that yields a long-focal-length camera pointed
at a plane?

**R3 — the command surface.** How is `command.rs::register_spec` seeded today, and by whom?
What is the shape of `agent.rs::core_catalog` (the mechanically-generated engine
vocabulary)? Separately: what does `native/src/bin/ctl.rs` — the `organon` CLI — support
today, how does it parse and dispatch, and where would a `console` subcommand branch attach?

**R4 — grid geometry and PTY sizing.** In `term_view.rs`, how are the grid rect, rows and
columns computed, and how does that reach the PTY resize path in `term.rs`? **Enumerate every
place that would need to know the grid is N rows shorter than the window.** This is the
reserved-row question and getting it wrong makes every full-screen TUI render one line off.

**R5 — material and surface reuse.** What exists in `organon-render` (passes, materials,
shaders) that a flat lit plane could reuse rather than reinvent? Which existing generator is
closest to "a plane with a material and a light"? What is the cheapest honest path to one
good-looking lit surface?

**R6 — the parameter model, and what a descriptor can honestly say.**

`console_discover_schema.md`'s control descriptor claims `min`, `max`, `value`, `default`,
`unit`, `format` and — critically — `taper`, all in the **display domain**. The schema
deliberately declines to pin the permitted taper set from memory and defers it to
implementation. **Answer it here instead, because it is reconnaissance, not implementation.**

- What range types does `param_table.rs` actually use? **Enumerate every variant**, including
  nih-plug's float ranges and any of our own. Which are expressible as `linear` / `log` /
  `skewed{factor}`, and **what can the schema currently not say?**
- Is a parameter's current value readable outside the audio thread, and in which domain —
  normalized `0..1`, or display?
- Where do unit and display formatting live today — the plugin's value-to-string, `param_desc`,
  somewhere else?
- Which of `core_catalog()`, `ACTUATABLE_IDS` and `param_desc` is the right source for each
  descriptor field? Is there one place carrying all of them, or must the emitter join?

🚨 **If the schema cannot express a range the engine actually uses, the schema gets widened
here in Phase 0 — not worked around in Tier 3.** A taper the schema cannot say is a control the
console renders wrong, and it will look approximately right.

**Gather:** the coordinator merges the six answers into a single *as-built brief* committed
alongside this plan. Every tier below reads the brief instead of re-deriving it. If two
answers contradict each other, resolve it before writing code — that contradiction is the
most valuable thing the recon found.

---

## 5. Per-tier division

File ownership is declared here. If a tier's work does not fit the declaration, **re-split
it — do not power through a conflict.**

### 🚨 Amendment, 2026-08-11 (James): the backdrop is not the claim, and every tier below is coloured by that

Read this before any tier description here. In his words:

> *"Setting the background of a terminal is nothing. People have been doing that for years. It
> doesn't communicate what we are really doing — in fact it breaks the whole concept that we're
> creating an illusion that we are in a terminal."*

It is not a matter of taste. **Painting the whole window is the one move that says *this is a
picture with text on it*, which is the opposite of what the console asserts.** The illusion of
being a real terminal is load-bearing, and a backdrop spends it. A rendered object living *in*
the page is the claim nobody has seen; a themed background is a thirty-year-old idea that
invites exactly the wrong comparison.

**What this does and does not mean.** Tiers 1, 2 and 4 are not wasted — the lit substrate, the
material library, the camera rig, the compositing seam and its measured gamma pair, and the
absolute-line band arithmetic are all the machinery a patch is made of, and every one of them
is used by Tier 5. What changes is **where the pixels land**: in a claimed rectangle, not
across the window. `Shell::render_source` now separates *what the engine draws* from *what the
backdrop paints*, so a patch renders with the backdrop off and nothing paints across the
window.

⚠️ **The tier tables below still read as though the backdrop is the deliverable.** They are
kept as the record of how the work was divided, not as a statement of what to build next. **A
new lane that proposes to paint more of the window is out of scope by this amendment**, whatever
the table beneath it says.

### Tier 1 — the lit substrate

| Lane | Owns | Output |
|---|---|---|
| Leaf A | new `substrate/camera.rs` (root crate) | narrow-FOV **perspective** rig as a pure function (not true ortho — R2); tests for projection, and edge-to-centre view-vector deviation **as a function of aspect** — vertical FOV is what the engine takes |
| Leaf B | new substrate `Shared`-state builder (pure, root crate) | one plane via `RenderPath::Membrane` — **no new shader** (R5); per-vertex albedo + the narrow FOV carry the read |
| Integrator | `shell_main.rs`, `world.rs`, `term_view.rs`, `SHELL_ARCHITECTURE.md`, `doc/arch/render.md` | wire into the backdrop seam **fixing its aspect** (size the texture to the panel rect — R1/R4: it is stretched today); the `world.rs` camera arm + both FOV clamps + the auto-follow latch (R2); extract a testable `scrim_alpha` and assert `SCRIM_FLOOR` holds |

Leaves A and B are genuinely concurrent — A is arithmetic, B is a state builder. They meet
only at the integrator. **The World stays selectable as a backdrop source beside the
substrate** — replacing it kills the live `organon set/generator/recipe` response (the
override lane drains inside `World::frame_body`; R1). If the plane wants the #472 material
maps, that gate lift (`render.rs:3640-3654`) is Tier 2's; Tier 1 ships per-vertex albedo
and FOV shading only.

### Tier 2 — change it by asking

| Lane | Owns | Output |
|---|---|---|
| Leaf A | the material set, + the map-gate lift in `render.rs:3640-3654` | four materials worth showing (graphite/paper/slate need the #472 maps, which the Membrane path cannot sample until the gate lifts — R5); two lighting rigs (the fill's direction is derived, not aimable — promise intensity only) |
| Leaf B | the CLI arm: `ctl.rs` + `cli.rs` `console` subcommand | `organon console background <name>` in clap `--help`, args validated; ops written to a NEW `ns_file("console.txt")` sidecar — **not** `CliOp`/`cli.txt`, which the World drains, not the Shell (R3) |
| Integrator | `shell_main.rs`, `SHELL_ARCHITECTURE.md` | stand up the product's **first `CommandService` instance** (specs must register into something real — none exists in the product today, R3); drain `console.txt` in the frame path; dispatch → live substrate change, under a second, no grid relayout |

### ⚡ Sequence amendment (2026-08-11, coordinator + James): Tier 4 lands before Tier 3

Tier 4's beat — a look change scrolling in from the bottom while history keeps its own
styling — is the night's priority, and it has **no technical dependency on Tier 3**: it
needs Tier 2's command lane and R4's scrollback map, not the strip. Tier 3 (the biggest
tier, plus its pre-gate) follows. Pacing note: a tier's **leaves** (pure, conflict-free by
construction) may dispatch once the previous tier's leaves have fixed its shape; a tier's
**integration** still waits for the previous tier's beat check to pass.

Tier 4 design cuts, decided now so the leaves build the same thing:

- **Look-epochs are substrate looks only.** Each `organon console background <material>`
  (or `rig`) change closes the current epoch at the current absolute scrollback line and
  opens a new one. Substrate looks are still lifes, so each epoch renders to **one cached
  texture** (re-rendered on resize only) and the viewport paints row-aligned band quads —
  no per-frame multi-world rendering. `background world` collapses history to a single
  live epoch (a live world is not a still life; freezing it would be a lie labelled look).
- **Band edges snap to cell rows** — the pure module owns (epoch ledger, display_offset,
  rows, history_size) → bands, with property tests: bands partition the viewport, edges
  monotone, pre-first-change is one epoch, **alt screen is always exactly one band** (no
  scrollback there).
- **The cap is small, honest and logged**: bounded epoch textures; evicted epochs merge
  into their older neighbour, logged to stderr. No restyle-everything path exists.

### Tier 3 — the strip *(the biggest tier)*

✅ **The schema is already settled** — `doc/console_discover_schema.md`, decided 2026-08-10
away from the demo deadline. **Tier 3 implements it; it does not design it.** Read the two
invariants there before dispatching anything: nothing from a payload is ever executed, and
descriptors are generated from the parameter table rather than hand-written. Both fail
silently if broken.

Because the vocabulary is settled, all four leaves fan out at once:

| Lane | Owns | Output |
|---|---|---|
| Leaf A | schema types + serde + the emitter; owns `ctl.rs` and `cli.rs` this tier | a `Discover { path }` subcommand plus `--json` on the existing `describe` — the invocation the schema settled in `acae19a` (subcommands, matching the CLI's own grammar; never global flags); the `discover`/`describe --json` read path skips the ~150 ms `is_live` probe (R3); round-trip tests, tolerant defaults, the test list at the end of the schema doc; SKILL.md gains the new grammar per §10 |
| Leaf B | a NEW root-crate module (e.g. `native/src/console_catalog.rs`) — **not** `organon-shell`, which is forbidden nih-plug (R6) | two pieces, budgeted separately: the field-name↔wire-id **namespace bridge**, generated from `param_table.rs`'s slot lists (a hand-written table would be the fourth copy of the one that already drifted); then `core_catalog` entries → `CommandSpec`s + descriptors **generated** (I2), guarded by `taper_round_trips_against_the_engine_range`. Its `pub mod` line in `lib.rs` belongs to the integrator, added up front |
| Leaf C | new reserved-row module in `organon-shell` | pure arithmetic, **both directions**: `grid_rows(avail_h, cell_h, strip_rows)` AND `strip_height(strip_rows, cell_h)` — one number, two projections, or points and rows diverge at fractional DPI (R4); saturating sub, floor ≥ 2; empty payload → 0 rows (that IS auto-hide); tested across resize, fractional cell heights, ppp ∈ {1.0, 1.25, 1.5, 2.0, 2.25} |
| Leaf D | the strip widget (`tabs.rs`) | the existing tab strip extracted to be data-driven: labels in, index out, callback. Fix the stale "along the bottom" module doc and the upward-anchored `+` menu while there (R4) |
| Integrator | `shell_main.rs`, `term_view.rs`, `term.rs`, `native/src/lib.rs`, `SHELL_ARCHITECTURE.md` | the strip is a bottom `TopBottomPanel` declared **before** the CentralPanel — under that approach `term_view.rs`/`term.rs` need **no arithmetic change** (R4); the one forced structural change is `cell_h` escaping `term_view::draw` (a public `cell_metrics`); suppress auto-hide while scrolled into history (a toggle is a real `Term::resize` and the view jumps a row); tap → compose into the input line, prompt-ready buffering. Taps come from egui widgets in the panel — click→cell mapping is explicitly dropped unless a need appears |

⚠️ **`term_view.rs` and `term.rs` both belong to the integrator in this tier.** Leaf C writes
arithmetic in its own file and never calls it from theirs. The `[grid]` debug line keeps
reporting the **PTY's** rows, or the htop canary goes blind (R4).

📌 **Pre-Tier-3 gate — make the range tables true first.** `agent.rs::id_range` and
`clip.rs::RANGES` have drifted from `params.rs` on **9 of 45 actuatable ids** (`trans_amp`
10× on the max; the published `doc/reference/parameters.md` ships the wrong range — R6).
Land, as its own change before any Tier 3 leaf dispatches: the round-trip test (it fails on
9 ids today — that is the point), the tables re-derived from or pinned to `params.rs`,
`recipe.rs` bounds included, `doc/reference/` regenerated. Tier 3 then builds on a table
that is already true.

### Tier 4 — it scrolls, and it remembers

| Lane | Owns | Output |
|---|---|---|
| Leaf A | new scroll-anchor module | scrollback offset → screen rect, as a pure function |
| Leaf B | patch lifetime policy | a crude, honest, logged GPU cap for off-screen patches |
| Integrator | `shell_main.rs`, `term_view.rs` | anchor patches; look applies forward, history keeps its own |

**Do not build a restyle-everything path.** Nothing restyles history; patches stay where they
were created. That is both the cheap implementation and the correct one.

### ⚡ Sequence amendment #2 (2026-08-11, James): Tier 5 lands before Tier 3, and Tier 5 is no longer a stretch

James's ask, in his words: *to see this working to the point where we are able to insert a
particular set of lines of GPU-rendered content into the scrollback with anything we want
rendered on it.* That is Tier 5, and **Tier 3 is not on the path to it** — Tier 5 depends on
Tier 4's anchor, which landed and beat-checked, not on the strip. Tier 3's *leaves* stay
valuable (Leaf B's generated descriptors are what make a panel of controls renderable at all),
so wave 1 lands and merges; Tier 3's integration is parked behind Tier 5.

**Tier 5 stops being a stretch goal.** It is the point of the exercise now, and the beat that
was "the closer" is the beat that gets built.

### Tier 5 — patches in the transcript

**The design changed on 2026-08-11 and the change is load-bearing: the console does not create
the hole — the agent does.**

The original shape had the console inject rows into the terminal buffer behind the child's
back. That works (recon proved the mechanism exactly: feed `\r\n` × N to the parser the console
already owns, which is the *same* code path the child's own newlines take, and the absolute-line
identity survives it) but it carries one risk that cannot be settled from source: ConPTY keeps
its own screen-buffer model and repaints by absolute cursor positioning, so rows it does not
know about may be painted over.

**James's inversion removes that risk entirely.** An agent in a console tab already has a skill
teaching it the `organon` CLI. It knows what it is about to print. So it writes its paragraph
with a rectangular gap — ordinary spaces and newlines, ordinary stdout, through the ordinary
PTY — and claims the gap. The rows are real output through the normal path, so the shell,
ConPTY and the console cannot disagree that they exist.

Three consequences, each a simplification:

- **Text flow around a patch costs the console nothing.** The agent does the flow; it is just
  text. The console's job collapses to *given a rect and a texture, paint* — newspaper-style
  wrap included, with no wrapping logic anywhere in our tree.
- **The layout intent stays with the author.** The console holds a rect and a texture; the
  agent holds why the gap is that shape. That is what makes resize survivable: the console
  cannot reflow a figure, but the agent can re-emit the passage.
- **It degrades honestly.** The claim rides an in-band escape sequence in the agent's own
  output, which an unaware terminal swallows silently. The same output in Windows Terminal or
  through a pipe is a paragraph with a gap in it.

⚠️ **This does not breach §6 rule 5 (harness-agnostic or it does not ship)** — checked
deliberately, because that rule would kill it otherwise. Nothing here requires Pi, ACP, or any
cooperation a harness must implement: it requires an agent that can print spaces and call a
CLI, which is every agent with a shell. The console works identically with no agent at all; it
simply has nothing interesting to paint.

🚨 **The wire format is settled in `doc/console_patch_protocol.md` before implementation**,
the same discipline that kept Tier 3's schema out of a 2 a.m. design session. Tier 5 implements
it; it does not design it.

| Lane | Owns | Output |
|---|---|---|
| Leaf A | new `block_anchor.rs` in `organon-shell` | ✅ **landed** (`console/t5-anchor`) — blocks → viewport row ranges, with the texture-slice offset for a half-scrolled block; model-agnostic, so the design change above cost it nothing |
| Leaf B | the marker scanner | the in-band claim recognised on the way to the VT parser, split-read safe; pure, headless |
| Integrator | `term.rs`, `term_view.rs`, `shell_main.rs`, `cli.rs`, `ctl.rs`, docs | `feed_local` + the bracket (the **fallback** path, for when nobody is writing); the paint call; the patch ledger |

**Known, accepted, and written into the protocol rather than solved:** a gap made of spaces does
not survive a width change — reflow rewraps the paragraph and the rectangle is destroyed. The
protocol says what happens instead of pretending.

---

## 5.9 🚨 Amendment, 2026-08-12 (James): the console forks into two front-ends

Everything above §5.9 was written under one assumption — that a character grid is the canvas.
Three measurements retired it.

1. **ConPTY rewrites the stream** (probed with `ORGANON_SHELL_PTY_DEBUG=1`): APC stripped
   entirely; a private OSC survives byte-intact but is **hoisted out of stream order**; OSC 8
   survives in position but has its params rewritten. A WSL tab is `wsl.exe` under ConPTY, so
   **there is no ConPTY-free path on this machine.**
2. **A real Claude Code tab, `organon console block 10`:** the harness's entire frame shifted
   up, its banner scrolled off, its input box and status line were displaced, and no patch
   rendered. A harness owns the grid and repaints by absolute positioning.
3. **The cursor test:** against an *idle* shell the prompt is stranded above the hole with the
   cursor below it. The cursor **is** the live input point, so console-side injection always
   puts the hole between the prompt and the typing. "Works when idle" was exactly backwards.

### The decision, stated precisely — the sweeping version is misleading

**We already own every pixel.** The console runs the PTY, parses it, and paints the glyph grid
itself; that is *why* a patch could be painted at all. What we do **not** own is **the
conversation**. The character grid is a lossy encoding of something that had structure before
it was flattened, and every wound above came from trying to recover that structure afterwards.

So the console becomes **two front-ends over one renderer**:

| | What it is | Status |
|---|---|---|
| **Terminal host** | runs any program, paints its grid, patches only by cooperation | **exists — keep it.** It is how `htop` runs and it is the universal fallback |
| **Conversation view** | consumes an agent's structured event stream (turns, deltas, tool calls, results, approvals) and renders it natively | **new.** A patch here is just an element in the flow: no claim protocol, no anchoring, no ConPTY |

James's framing: Telegram, WhatsApp, Claude Desktop, Claude Code, Pi are all the same shape —
scrollback above, composer below. **A TUI is not a design; it is what you build when a
character grid is the only canvas allowed.** "It looks exactly like a terminal" stops being a
constraint and becomes **a skin we chose**, which is a stronger claim, not a weaker one.

### What this is not: a rewrite

Nearly all the expensive work carries — `block_anchor`'s arithmetic, the epoch texture cache
with its bounded logged eviction, `render_source`, `pane_pixels_in`'s DPI-cancelling sizing,
the scrim's structural contrast floor, the UV window-not-thumbnail policy. **Addressing a
region and drawing into it is the entire remaining product.** What the pivot deletes is the
*negotiation*, not the primitive.

### What each tier becomes

- **Tiers 1, 2, 4 — unchanged and reused.** The substrate, materials, camera rig, compositing
  seam and band arithmetic are what a patch is made of, in either front-end.
- **Tier 3 (the strip) — reshaped by the split.** It was designed as chrome reserved out of a
  character grid, with `htop` as its canary and reserved-row arithmetic as its risk. In the
  conversation view there is no grid to reserve from and no canary: the strip is an ordinary
  element. **`strip_layout` and the data-driven widget still stand; the reserved-row problem
  disappears.** Its Leaf B (the generated catalog bridge, `console/t3-bridge`) becomes *more*
  central, not less — descriptors are what make a control panel renderable at all.
- **Tier 5 — splits in two.** In the terminal host it stays as built: `organon console patch`,
  cooperation-only, the protocol's rules in force. In the conversation view an inline artifact
  needs none of that machinery.

**First milestone, deliberately smaller than it wants to be:** *one real agent conversation
rendered natively, containing one inline artifact a terminal could not have shown.* Not a
client. Proof.

### 5.9.1 Which event stream — decided 2026-08-12, on measurement

Two recon passes, plus live probes of the installed CLI on this machine. **The answer is
Claude Code first, Pi second** — and the premise the question was framed on turned out to be
wrong, which is the most important finding.

🚨 **"Pi is where we control both ends" is no longer true.** Pi migrated from the retired
private fork to **stock upstream npm** on 2026-08-11 (`@earendil-works/pi-coding-agent` 0.84.1,
MIT, upstream's own `types.d.ts` and 2988-line extension doc ship inside the package). We
control an *extension*; upstream controls the events it receives — on a `0.x` line averaging a
minor release per week, whose `message_update` payload changed shape **one release before the
installed build**. That was the main argument for Pi and it has evaporated.

**Measured live on this machine** (`claude.exe` **2.1.228**, auth good, `--output-format
stream-json`), because the recon had no shell and everything it said was doc-derived:

- The full event sequence for a tool-using turn: `system/init` → `assistant`(text) →
  `assistant`(`tool_use`) → `user`(`tool_result`) → `assistant`(text) → `result`.
- `tool_use` carries the **complete structured input** (`{"name":"Read","input":{"file_path":…}}`),
  and `tool_result` correlates by `tool_use_id`. Start and end are distinct messages.
- 🚨 **`permission_denials: []` — a read tool executed with no callback and was not denied.**
  The recon's "no callback means deny" warning does not bite for every tool, so **a plain stdio
  consumer can render a real conversation today** without an SDK, an MCP server, or the
  undocumented control protocol.

  ⚠️ **Refined 2026-08-12, from James driving the finished view:** "read-only tools pass" is too
  generous. `Read` passed; three tools in his first real session were **refused**, each with a
  distinct reason — a PowerShell script block ("may execute arbitrary code"), a non-filesystem
  provider path (`Env:`), and a compound `Bash` pipeline whose `env` part "requires approval".
  So the gate is not read-versus-write, it is **whatever the permission layer wants to ask
  about** — and with no callback there is nobody to ask, so it fails.

  **The failure is at least honest**: refusals arrive as ordinary `tool_result`s with
  `is_error`, so the view renders them as error cards carrying the refusal reason, and the agent
  can see and explain them. Nothing is silent. But it means **approvals are the next thing worth
  building, not a comfortable milestone-2 item** — a conversation view where a third of the
  agent's tools bounce is a demo, not a workspace. `canUseTool` is a documented SDK callback
  that can allow, deny, *modify the input*, or persist a rule; over the bare CLI the documented
  route is an MCP `--permission-prompt-tool`. Choosing between those is the next real decision.
- Undocumented events observed that the docs do not list and a view would want:
  `rate_limit_event`, and `system/post_turn_summary` carrying `status_category`,
  `status_detail`, `needs_action`.

**Why Claude Code first, in order of weight:**

1. **Rust talks to it directly.** NDJSON over a child process's stdio — the console already owns
   child processes. Pi is TypeScript in WSL, so a Pi view needs a **Node boundary and the WSL
   seam** (loopback HTTP, as `voice-channel.ts` already does). That is an entire extra transport
   for a proof.
2. **It is what James uses every day**, so the proof lands in the real workflow instead of beside it.
3. **`AskUserQuestion` carries structured options with optional HTML `preview` fragments** — an
   inline rendered artifact delivered as data, which *is* the milestone's requirement, handed to us.
4. `Edit` carries `old_string`/`new_string`, and `TaskCreate`/`TaskUpdate` are ordinary tools —
   native diffs and a live task panel with no parsing.

⚠️ **The cost, stated plainly because it is a product decision wearing a protocol costume:
there is no attach.** Every programmatic surface is a child process you spawn. A conversation
view cannot mirror the Claude Code session James is already running in a terminal — it must
**be** the session. The view replaces his invocation rather than observing it.

⚠️ **And the gap that will show:** token-level deltas from **subagents are never forwarded**.
On a coordinator session that fans out to a dozen agents, a large fraction of visible activity
arrives as complete-message bursts, not as live text.

### 5.9.2 The interaction model, measured — a live session, not a series of one-shots

Run on this machine with two user messages written to the CLI's stdin across a 25-second gap
(`-p --input-format stream-json --output-format stream-json --replay-user-messages`):

- **One `session_id` across both turns.** The process stays live. This is not `--resume`, and
  it does not pay resume's cost (a new process, the transcript re-read, the history re-sent).
- 🚨 **A `result` object arrives per TURN, not per session** — two of them in one stream. So
  `result` is a *turn* terminator and the stream continues past it. Anything that treats it as
  end-of-stream will close a live conversation after its first exchange. `num_turns` was `1` on
  each: it counts that run's turns, it does not accumulate.
- **`--replay-user-messages` echoes the injected human turn back into the output stream**, so a
  human message arrives as an ordinary ordered event rather than something the view splices in
  locally and hopes it ordered correctly.

**The integrator's contract, therefore:** spawn once per conversation tab, write NDJSON user
messages to stdin, read NDJSON events from stdout, and never let the process go. Resume is the
recovery path, not the interaction model.

### 5.9.25 The agentic API is the CLI, and the skill teaches it — settled 2026-08-12 (James)

**The question the pivot raised:** now that the console owns the whole visual and *spawns* the
agent itself, is the `organon` CLI still the way an agent reaches these capabilities?

**Yes, and the argument is stronger than "it already exists": the CLI is the only interface
every agent already has.** Claude Code has Bash, Pi has bash, Codex and Cursor and any foreign
CLI can run a command. Nothing has to implement anything. That is the same property that made
§6's harness-agnostic rule right for the terminal host, and it is the one thing in that
document the pivot did *not* invalidate.

**The genuine alternative, and why it is a supplement at most.** Because the console spawns the
process, it could also hand Claude Code tools over **MCP** (`--mcp-config`) — typed arguments,
no shell quoting, and the call already lands as an element in our transcript. But MCP reaches
only harnesses that support it *and* that we spawn: the terminal host cannot use it, Pi's RPC
is a different shape, a foreign CLI has nothing.

🚨 **If MCP is ever added, it is GENERATED from the same table the CLI is generated from.** One
vocabulary, many renderings — the CLI, the agent's catalog, `doc/reference/`, and MCP as a
fourth if it earns its place. A hand-written MCP server beside a hand-maintained CLI is exactly
the failure this tree already paid for: three hand-written range tables, **9 of 45 ids silently
wrong**, published documentation shipping the wrong bounds.

**Three things the pivot changes about the CLI itself:**

1. **It gains a return path it never had.** In the terminal host, `organon console …` was
   fire-and-forget into a sidecar and the console could not answer. Now the command's stdout
   comes back as a tool result the agent reads *and* the view renders — so commands can be
   **queries**, not only imperatives. `console_discover_schema.md` always assumed this; the
   terminal host could never fully exploit it.
2. **The position arguments disappear.** `--up N --rows M` existed because the agent had to say
   *where* in a character grid. In the conversation view **the tool call is the anchor**. Same
   verb, fewer required arguments.
3. 🚨 **The front-end distinction belongs in the CLI, never in the skill.** An agent in a
   terminal tab must print its own gap and claim it; an agent in a conversation tab must not.
   The wrong fix is teaching the skill to explain the difference. The console already injects
   its namespace per tab, so **the CLI can detect which front-end invoked it and do the right
   thing.** A skill that says "if you are in a terminal, first print twelve newlines" is an
   instruction that rots and that agents get wrong under pressure.

⚠️ **The coupling this creates, worth acting on:** if the CLI is the agentic API, the agent's
**permission layer** is the gate on it. Three tools bounced on approval in the first real
session. If `organon console <verb>` needs approval every time, the artifact never appears —
and unlike a refused `env` read, that failure reads as *our feature is broken* rather than as a
policy working correctly. This is an argument for doing approvals **before** the rendered-patch
work, not after.

### 5.9.3 The mapping contract — decoder → transcript, and the six measured facts that shape it

The decoder (`agent_event.rs`) and the transcript model (`conversation.rs`) were written by
independent leaves against a deliberate seam: **two agents cannot own one type.** The
integrator writes the mapping. These rules are not style — each comes from something measured
in a real capture, and getting any of them wrong produces a view that looks nearly right.

1. 🚨 **An `assistant` line carries ONE content block, not a whole message.** Three consecutive
   lines shared message id `msg_…RU4dqFxH14d2HJ1S`: prose, then tool call #1, then tool call #2.
   `conversation::MessageId` is documented as unique **per rendered text block**, and same-id
   blocks *replace* each other — so mapping `message_id` straight through would let the tool
   call overwrite the prose and silently lose it. **Derive a per-block key** (`message_id` plus
   the block's ordinal), and key the streamed path off `BlockDelta{index}` so streaming and
   authoritative text land on the same element.
2. 🚨 **The human turn comes back on the stream, and must not also be inserted locally.**
   `--replay-user-messages` echoes injected input, flagged `isReplay: true`, as an array of text
   blocks. **The composer writes to stdin and renders nothing**; the transcript renders only
   what returns. That is what makes ordering free rather than a splice-and-hope.
3. 🚨 **`system/init` recurs mid-stream** — a second one arrived before turn two of the live
   session, same `session_id`, different field count. Only the first establishes identity; a
   later one must not reset or re-initialise the transcript.
4. **`total_cost_usd` is session-cumulative while the sibling `usage` is per-turn.** Turn two's
   cache-read figure was exactly turn one's plus its own. **Never sum costs across results** —
   take the latest.
5. **Subagent-scoped events are dropped in milestone 1.** The decoder distinguishes them
   (`AgentScope::Subagent { tool_use_id }`, from `parent_tool_use_id`). Rendered naively they
   appear as free-floating turns belonging to nobody. They belong *inside* the tool card that
   spawned them, which is milestone 2.
6. **The first line of a real run is not JSON.** It is `Warning: no stdin data received in 3s…`,
   plain text on the same pipe. Log and continue; a decoder that treats non-JSON as fatal dies
   before the conversation starts.

📌 Held for milestone 2, deliberately: `tool_use_result` (an undocumented sibling of `message`
carrying structured per-tool detail — for `Read`, `filePath`/`numLines`/`totalLines`, which is
what a rich tool card wants), `Notice`/`post_turn_summary`, `RateLimit`, and approvals.

**Why Pi second, and genuinely second rather than dismissed.** `pi --mode rpc` is documented in
its first sentence as being for "embedding the agent in other applications, IDEs, or custom
UIs" — it is purpose-built for this, and it carries **strictly more lifecycle events** than
Claude Code exposes (`queue_update`, `auto_retry_*`, compaction progress). Its `Edit` result
carries a **real unified patch**, and its truncation is explicit and recoverable
(`truncatedBy`, `totalLines`, `fullOutputPath`). Pi has **no permission system at all** — so
approvals are something we *build* rather than surface, but `tool_call` blocking plus the
documented `extension_ui_request`/`extension_ui_response` pair composes into a genuine
front-end approval loop in about thirty lines. Rule 5′ permits exactly one named integration at
a time; this is the next one.

---

## 6. Rules for parallel work

1. **One writer per file, declared before dispatch.** No exceptions, no "I'll just add one
   line."
2. **No sub-agent touches `shell_main.rs`, `term_view.rs`, `native/src/lib.rs`, `Cargo.toml`,
   `SHELL_ARCHITECTURE.md`, `native/src/world.rs`, or `organon-render/src/render.rs`** except
   the current tier's integrator — the last two joined the list in Phase 0 (R2/R3/R5: the
   camera arm, the CLI drain and the membrane path all live in them). `ctl.rs`, `cli.rs` and
   `agent.rs` are one-writer-per-tier by the declarations in §5.
3. **Every leaf lands with tests that pass without a GPU and without an egui context.** A
   leaf that cannot be tested headless is integration in disguise — reclassify it.
4. **If two agents must touch overlapping ground, give them isolated worktrees** rather than
   hoping. It is cheaper than the merge.
5. ~~**Harness-agnostic or it does not ship.** Nothing may require Pi, ACP, or cooperation from
   any harness. If a tier needs the harness to know about us, it is out of scope for this
   spike.~~ 🚨 **REPEALED 2026-08-12 (James), and repealed in writing so nobody enforces it
   against the pivot three weeks from now.**

   **Why it was right.** For a *terminal host* it is exactly right, and it stays right there.
   A host that only works with cooperating programs is not a terminal — it is a plugin
   system with extra steps. `htop` must run, `vim` must run, an unmodified Claude Code tab
   must run, and none of them will ever know we exist. **Rule 5 still governs the terminal
   host, in full.**

   **Why it is wrong now.** §5.9 split the console in two. The *conversation view* consumes an
   agent's structured event stream **by definition** — that is not a compromise of the design,
   it is the design. Applying rule 5 there forbids the front-end from talking to the only thing
   it exists to render, which is how a rule outlives the problem it was written for.

   **The replacement, scoped rather than deleted:**

   > **Rule 5′ — the terminal host is harness-agnostic; the conversation view is
   > harness-specific and says which harness.** Nothing in the terminal host may require
   > cooperation from any program. The conversation view may require exactly one named
   > integration at a time, declared in the plan, and **degrading to a terminal tab is always
   > available** — a harness we have not integrated is not unsupported, it is supported the
   > old way.

   That last clause is what preserves the original rule's real value: no user is ever locked
   out by our not having integrated their tool.
6. **No tier starts before the previous tier's beat check passes on this machine.**
7. **Findings go in the as-built brief, not in chat.** The brief is the shared memory.

---

## 7. Beat checks — and why they are the point of running this here

Every tier in #4 has a beat, and a beat is a judgment about how something *looks and feels*.
That is exactly what a cloud session cannot do and what this machine can: an RTX 5090, a real
display, and a console that already builds and runs on Windows.

**The coordinator performs every beat check personally.** Do not delegate them and do not
accept "the build is green" as evidence.

| Tier | Beat check |
|---|---|
| 1 | Side by side with a stock terminal on the same screen. If nobody looks twice, the material is wrong — not the mechanism. |
| 2 | Ask in English; the change lands in under a second, no flicker, no grid relayout. |
| 3 | **`htop` first.** If box drawing or the alternate screen is off by a row, stop — the reserved-row math is wrong and nothing else matters. Then: tap, and finish the sentence aloud without naming the subject. |
| 4 | Three look changes in a session, then one continuous scroll bottom to top. |
| 5 | Scroll past the live block and back without stalling the grid. |

**The Mac leg.** #4 asks for both platforms before a tier is called done. The ConPTY class of
bug does not exist on macOS and Mac-only paths (EDR/HDR interaction with the UI pass) do not
exist here. Rehearsing on Windows only is acceptable *during* the spike; calling a tier
finished without the Mac is not.

---

## 8. What the coordinator never delegates

Per §3.5 the coordinator delegates all the *writing*. These are not writing:

- The branch, and every integration commit. A sub-agent may author the change; the
  coordinator reads it, builds it, and commits it.
- Beat checks. They require eyes on a display and a judgment about how something looks, which
  is the one thing that cannot be handed to a sub-agent or inferred from a green build.
- The demo script, kept in the repo and updated as beats land, so what we can actually show is
  never a matter of memory.
- Any decision that amends the spec — notably Tier 3's amendment of the "top strip is the one
  permitted chrome" requirement. That is a decision, not an implementation detail.
- The call to cut Tier 5.

---

## 9. Failure modes, named in advance

- **`htop` breaks in Tier 3.** Stop everything. It is the canary for reserved-row arithmetic
  and it will look like a rendering bug when it is a sizing bug.
- **The material seams on box-drawing characters.** Expected — U+2500–257F must tile
  pixel-exactly at cell boundaries. It is Tier 4's known risk, not a Tier 1 defect, and it is
  why foreground glyph shading is explicitly **out of scope for this spike** (it needs the
  instanced glyph-atlas pass, which is not landing here).
- **Composition fires mid-stream and corrupts input.** Buffer to the next prompt opportunity.
  This will happen on stage if it is not handled in Tier 3.
- **The coordinator is merging conflicts.** The ownership declaration was wrong. Stop, re-split,
  and record why in the brief.
- **Scope creep into #3's later tiers.** The spike deliberately excludes icons beyond font
  glyphs, persistence, theme files, structured agent protocols, foreign-CLI mapping,
  agent-authored strips, and camera motion. Each is real work; none of it is this work.

---

## 10. Docs that move with the work

Per tier, in the tier's own change — never as a cleanup pass at the end:

- **`CONSOLE_ARCHITECTURE.md`** — every tier. A Stop hook enforces it for
  `native/organon-shell/*`.
- **`CHANGELOG.md`** — every tier.
- **`doc/console_spike_demo_script.md`** — the status column, in the same change as the tier
  that delivers the beat. What we can actually show should never be a matter of memory.
- **`doc/console_spike_execution_plan.md`** — this file, whenever Phase 0 or a tier proves part
  of it wrong. It is a plan, not a monument.

### 🚨 Any tier that adds or changes a command updates `skills/organon-cli/SKILL.md`

That is **Tier 2** (`organon console background`), **Tier 3** (`organon discover`,
`organon describe --json`) and **Tier 5** (the patch verbs) — not Tier 3 alone. The tier's own
agent does it.

📌 **Tier 5 is the first tier where the skill is not documentation — it is the mechanism.** A
patch exists because an agent left a gap in its own output and claimed it, and the only way an
agent knows how to do that is the skill. `doc/console_patch_protocol.md` is the contract (the
sequence, the fields, what the console guarantees); the skill is that contract taught in the
shape an agent needs, with the surface still deferred to the live tool. Two renderings of one
source, exactly as with the Discover schema — and here a stale one does not degrade to a
missing feature, it degrades to an agent printing escape sequences nothing will ever claim.

**A skill is what an agent reads *instead of* the source, so a stale one does not degrade
gracefully.** It makes an agent confidently call a command that does not exist, or miss one
that does. Wrong is materially worse than absent here, which is why this is not a tidy-up item.

`.claude/hooks/doc-rules.sh` lists the skill as accountable for `native/src/bin/ctl.rs` and
`native/src/cli.rs`, so a Stop hook will remind you. ⚠️ **Treat the hook as the safety net, not
the instruction.** It fires after the fact, and a sub-agent may never see it at all.

**What goes in: the shape. What never goes in: an enumeration.** The skill already makes this
split correctly — *"the live catalog is the authority … ask the tool, not your memory"* — and it
is the only reason a 150-line file can describe a ~1,370-parameter surface without rotting. A
new command gets its grammar and its place in the loop; what lives *inside* it stays
discoverable from the tool.

---

## Appendix — the handoff prompt

Paste this into a fresh Claude Code session rooted at
`C:\Users\james\Documents\GitHub\organon`.

```
You are the coordinator for the Console Spike — a vertical slice of Organon's console
that has to be demoable, built in five tiers, each one a visible beat.

Read these, in this order, before doing anything:
  1. doc/console_spike_execution_plan.md   — how the work divides (this is your plan)
  2. SHELL_ARCHITECTURE.md                 — the code-grounded state of the console
  3. gh issue view 4                       — the spike: the beats and the tiers
  4. gh issue view 3                       — the console: the design the spike slices
  5. doc/console_discover_schema.md        — the settled wire format Tier 3 implements
  6. CLAUDE.md and CONTRIBUTING.md         — this repo's rules

You are the coordinator and you write no implementation code. Dispatch all of it —
reconnaissance, leaf modules, and integration — to sub-agents, passing
model: "opus" explicitly on every dispatch. What is yours: deciding what gets
dispatched and to whom, reading what comes back and rejecting it when it is wrong,
running the build and tests, the beat checks, and the commits.

Section 6 of the plan governs file ownership. Never let two agents into
shell_main.rs, term_view.rs, native/src/lib.rs, Cargo.toml, or SHELL_ARCHITECTURE.md
at the same time. The integrator lane is one sub-agent at a time and you wait for it
(run_in_background: false) before dispatching anything touching the same files.

Start with section 2 of the plan: prove Tier 0 on this machine. Build both binaries,
launch the console, open a Pi or Claude Code tab, run htop in a third, and confirm
ORGANON_SHELL_BACKDROP=1 renders the world behind the glyphs. Report exactly what
you observed — including anything that did not work. If Tier 0 does not stand up,
that is the first work item and everything else waits.

Then run the Phase 0 reconnaissance fan-out (section 4): five read-only questions,
dispatched concurrently, gathered into an as-built brief you commit beside the plan.
Those five answers are load-bearing — several assumptions in the tier breakdown are
educated guesses about code none of us has read closely, and the brief is where they
get corrected.

Do not start Tier 1 until the brief exists and I have seen it.

Notes on this environment: Windows 11, PowerShell 5.1 (&& is a parse error — use
`cmd; if ($?) { next }`), RTX 5090, cargo/rustc on PATH, no Node/npm. The console
requires --features shell-edition. `organon-console --help` documents every dev flag.
Hooks in .claude/hooks/ will require SHELL_ARCHITECTURE.md to move in the same change
as any native/organon-shell/* edit — that is deliberate; do not work around it.

Report at each gate: after Tier 0, after the brief, and after each tier's beat check.
A green build is never evidence that a beat works — you have a GPU and a display, so
look at it.
```
