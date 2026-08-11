# Console Spike — as-built brief

**Status: UNANSWERED.** This file is the gather point for the Phase 0 reconnaissance
fan-out in `console_spike_execution_plan.md` §4. Six read-only questions, dispatched
concurrently, answered here.

**Why it exists.** Several assumptions in the spike's tier breakdown are educated guesses
about code none of us has read closely. This is where they get corrected — before any tier
starts, not after a tier fails. Every tier reads this instead of re-deriving it.

**How to fill it.** One section per question. Cite files and line numbers. Prefer a short
correct answer to a long confident one. If you could not determine something, say so
explicitly — an honest gap is useful; a plausible guess recorded as fact is not.

🚨 **Tier 1 does not start until this file is complete and James has seen it.**

---

## R1 — The compositing seam

*Where is the `World` rendered and painted under the glyphs in `shell_main.rs`? Function
signatures, render-target format, and what the "measured render-sRGB/sample-linear gamma
pair" concretely is. **What would a second, alternative background source have to satisfy
to drop into that same seam?***

**Answer:**

**Files/lines:**

**Consequences for the plan:**

---

## R2 — The camera

*How does the `World`'s camera acquire its projection? Is it parameterizable to an arbitrary
near-orthographic rig, or assembled somewhere fixed? What is the smallest change that yields
a long-focal-length camera pointed at a plane?*

**Answer:**

**Files/lines:**

**Consequences for the plan:**

---

## R3 — The command surface

*How is `command.rs::register_spec` seeded today, and by whom? What is the shape of
`agent.rs::core_catalog`? Separately: what does `native/src/bin/ctl.rs` — the `organon` CLI —
support today, how does it parse and dispatch, and where would a `console` subcommand branch
attach?*

**Answer:**

**Files/lines:**

**Consequences for the plan:**

---

## R4 — Grid geometry and PTY sizing

*How are the grid rect, rows and columns computed in `term_view.rs`, and how does that reach
the PTY resize path in `term.rs`? **Enumerate every place that would need to know the grid is
N rows shorter than the window.***

⚠️ This is the reserved-row question. Getting it wrong makes every full-screen TUI render one
line off, and it looks like a rendering bug when it is a sizing bug. Be exhaustive.

**Answer:**

**Files/lines:**

**Consequences for the plan:**

---

## R5 — Material and surface reuse

*What exists in `organon-render` (passes, materials, shaders) that a flat lit plane could
reuse rather than reinvent? Which existing generator is closest to "a plane with a material
and a light"? What is the cheapest honest path to one good-looking lit surface?*

**Answer:**

**Files/lines:**

**Consequences for the plan:**

---

## R6 — The parameter model, and what a descriptor can honestly say

*What range types does `param_table.rs` actually use — every variant, including nih-plug's
float ranges and any of our own? Which are expressible as `linear` / `log` / `skewed{factor}`,
and what can `console_discover_schema.md` currently **not** say? Is a parameter's current value
readable outside the audio thread, and in which domain — normalized or display? Where do unit
and display formatting live? Which of `core_catalog()`, `ACTUATABLE_IDS`, `param_desc` sources
each descriptor field, and must the emitter join them?*

🚨 If the schema cannot express a range the engine uses, **widen the schema here** — do not
work around it in Tier 3. A taper the schema cannot say is a control the console renders
wrong, and it will look approximately right.

**Answer:**

**Files/lines:**

**Schema changes required (if any):**

**Consequences for the plan:**

---

## Contradictions and surprises

*Anything two answers disagree about, and anything that contradicts the execution plan or
issue #4. **Resolve contradictions before writing code — they are the most valuable thing the
recon found.** Amend the plan rather than quietly working around it.*

---

## Corrections to the execution plan

*List every place the plan's tier breakdown or file-ownership declaration turned out to be
wrong, and what it should say instead. Then actually edit the plan.*
