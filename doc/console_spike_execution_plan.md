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
cargo build --release --features shell-edition --bin organon-shell
cargo build --release --bin organon
```

The console requires `shell-edition`; the `organon` CLI does not. `shell-edition` and
`mind-edition` are mutually exclusive (a `compile_error!` in `organon-core`'s `edition.rs`
enforces it).

**Run.** `organon-shell --help` is the documentation for every dev flag —
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

### Tier 3 — the strip *(the biggest tier)*

✅ **The schema is already settled** — `doc/console_discover_schema.md`, decided 2026-08-10
away from the demo deadline. **Tier 3 implements it; it does not design it.** Read the two
invariants there before dispatching anything: nothing from a payload is ever executed, and
descriptors are generated from the parameter table rather than hand-written. Both fail
silently if broken.

Because the vocabulary is settled, all four leaves fan out at once:

| Lane | Owns | Output |
|---|---|---|
| Leaf A | schema types + serde + the emitter; owns `ctl.rs` and `cli.rs` this tier | `cmd` becomes optional; top-level `--discover/--describe/--at/--all` (global flags) handled **before** `to_ctl`, skipping the ~150 ms `is_live` probe (R3); round-trip tests, tolerant defaults, the test list at the end of the schema doc |
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

### Tier 5 — the instrument inline *(stretch)*

Almost entirely integration. **One agent, coordinator supervising, no fan-out.** If Tier 4's
anchor is not solid, this tier cannot start — and if it has not started by the time T4's beat
check passes, ship T1–T4 and cut it.

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
5. **Harness-agnostic or it does not ship.** Nothing may require Pi, ACP, or cooperation from
   any harness. If a tier needs the harness to know about us, it is out of scope for this spike.
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

That is **Tier 2** (`organon console background`) and **Tier 3** (`organon discover`,
`organon describe --json`) — not Tier 3 alone. The tier's own agent does it.

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
requires --features shell-edition. `organon-shell --help` documents every dev flag.
Hooks in .claude/hooks/ will require SHELL_ARCHITECTURE.md to move in the same change
as any native/organon-shell/* edit — that is deliberate; do not work around it.

Report at each gate: after Tier 0, after the brief, and after each tier's beat check.
A green build is never evidence that a beat works — you have a GPU and a display, so
look at it.
```
