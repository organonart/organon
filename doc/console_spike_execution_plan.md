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

Five independent read-only questions. Dispatch all five at once; none depends on another.
Each returns a written answer with file paths and line references, no code changes.

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

**Gather:** the coordinator merges the five answers into a single *as-built brief* committed
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
| Leaf A | new `substrate/camera.rs` | near-ortho rig as a pure function; tests for projection, and edge-to-centre view-vector deviation within a documented bound |
| Leaf B | new substrate scene + shader (path per R5) | one plane, one material, one lighting rig |
| Integrator | `shell_main.rs`, `SHELL_ARCHITECTURE.md` | wire into the backdrop seam; assert `SCRIM_FLOOR` holds |

Leaves A and B are genuinely concurrent — A is arithmetic, B is a surface. They meet only at
the integrator.

### Tier 2 — change it by asking

| Lane | Owns | Output |
|---|---|---|
| Leaf A | the material set | four materials worth showing, two lighting rigs |
| Leaf B | command specs | `organon console background <name>`, registered so `--help` lists it, args validated |
| Integrator | `shell_main.rs`, dispatch path | dispatch → live substrate change, under a second, no grid relayout |

### Tier 3 — the strip *(the biggest tier; do the design serially)*

✅ **The schema is already settled** — `doc/console_discover_schema.md`, decided 2026-08-10
away from the demo deadline. **Tier 3 implements it; it does not design it.** Read the two
invariants there before dispatching anything: nothing from a payload is ever executed, and
descriptors are generated from the parameter table rather than hand-written. Both fail
silently if broken.

Because the vocabulary is settled, all four leaves fan out at once:

| Lane | Owns | Output |
|---|---|---|
| Leaf A | schema types + serde + the `organon --discover` emitter | round-trip tests, tolerant defaults, the test list at the end of the schema doc |
| Leaf B | catalog → command-service adapter | `core_catalog` entries become `CommandSpec`s — the one-table keystone. Also where descriptors get **generated** rather than authored (schema doc, I2), guarded by `taper_round_trips_against_the_engine_range` |
| Leaf C | new reserved-row module | pure arithmetic: window rows − strip rows → PTY rows, tested across resize and fractional scale |
| Leaf D | the strip widget | the existing tab strip extracted to be data-driven: labels in, index out, callback |
| Integrator | `shell_main.rs`, `term_view.rs`, `term.rs` | bottom region, PTY resize, click → compose into the input line, prompt-ready buffering |

⚠️ **`term_view.rs` and `term.rs` both belong to the integrator in this tier.** Leaf C writes
arithmetic in its own file and never calls it from theirs.

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
   or `SHELL_ARCHITECTURE.md`** except the current tier's integrator.
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
