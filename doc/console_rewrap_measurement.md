# The re-wrap measurement

**What this is.** One number, and what it does and does not license. `console_view_paradigm.md`
§9 named this as open — *"the re-wrap cost of a layout that changes width … this is the same
measurement posture's tween needs, and nobody has taken it"* — and this is the taking of it.

**Status:** measured 2026-08-13 on `ORGANON-ONE`. **A measurement, not a decision.** The
options in §7 are laid out with their costs and none of them is chosen; that is James's call and
it is downstream of these figures rather than contained in them.

**The instrument is in the tree**, at `native/organon-console/src/conversation_view/rewrap_bench.rs`,
so these numbers can be re-taken on another machine or after an egui bump rather than believed.
Its module doc is the authority on method; §3 below is the summary.

---

## 0. The answer, in one line

**About 7 µs to lay out one wrapped galley from scratch, against about 0.9 µs to fetch the same
galley from epaint's cache — so a frame whose width has moved costs roughly 6–9× a frame whose
width has not**, for the whole retained scrollback, every frame the width moves.

At the conversation front-end's default 1100 pt column that is **~24 µs per transcript element
per frame while the width is moving, against ~2.9 µs while it is still.**

Nothing culls. Every element in the transcript pays, on screen or not (§4).

---

## 1. The question, and who is waiting on it

Two roadmaps change the transcript's available width, and both were scoped without knowing what
that costs.

1. **Posture's tween.** The `Form` animates a `margin` token 0 → 90 pt. Available width moves on
   *every frame of the tween*, so the whole scrollback re-wraps on every frame, for the duration.
   ⚠️ **The token was called `gutter` and applied on the left alone when this was measured; it
   is now `margin` and is applied on both sides**, so the true travel between the two ends is
   `2 × 90` rather than 90 and the tween has at most 181 distinct wrap widths rather than 91.
   **Every figure below stands**, because the finding is per-*change*, not per-magnitude — a
   width that moves by one point is as total a cache miss as one that moves by a hundred. What
   doubles is the *length* of a one-point-per-frame triangle, not the cost of a frame in it.
   `rewrap_bench`'s `SWEEP` const is deliberately still 90, so the numbers here can be re-taken
   over the range they were taken over.
2. **Pane splitting** (`console_view_paradigm.md` §2, issue #48 Tier 4). Same change of width,
   once rather than sixty times.

A third consumer was not in the brief and is **live today**: dragging the console window's edge
changes the pane's width every frame, through exactly the same path. Whatever the tween would
cost, the console is already paying it during a resize drag — see §6.

⚠️ **Correction to the brief that commissioned this.** It cites `CONSOLE_ARCHITECTURE.md` §1.6
(posture) and `native/organon-console/src/posture.rs`. **Neither exists on `main` at `d09e7c1`.**
`CONSOLE_ARCHITECTURE.md` §1 ends at §1.5 (Preferences), and there is no posture module in the
crate — posture is designed (issue #38) and unbuilt. That does not weaken the question; it
sharpens who this is for. The number arrives **before** the code it constrains, which is the
order this repo prefers and the reverse of how the portal's "immersive is nearly free" claim
went. The `CONSOLE_ARCHITECTURE.md` pointer this document is referenced from therefore sits in §2
("Seams the next tiers consume"), where unbuilt work lives, and **not** in §1, which is
"what exists right now".

---

## 2. What the galley cache is keyed on

**The wrap width is part of the key.** Cited, not inferred, against the versions
`native/Cargo.lock` pins — `egui 0.33.3` / `epaint 0.33.3`:

| Fact | Where |
|---|---|
| The cache key is a hash of the whole `LayoutJob` plus `pixels_per_point` | `epaint-0.33.3/src/text/fonts.rs:884` |
| `LayoutJob`'s `Hash` includes `wrap` | `epaint-0.33.3/src/text/text_layout_types.rs:208–231` |
| `TextWrapping`'s `Hash` includes `max_width` | `epaint-0.33.3/src/text/text_layout_types.rs:439–452` |
| …but `max_width` is **rounded to an integer** first | `epaint-0.33.3/src/text/fonts.rs:879` |
| The cache keeps **only what the current pass used**, and is flushed at the *start* of a pass | `epaint-0.33.3/src/text/fonts.rs:1067–1073`, called from `:552`, called from `egui-0.33.3/src/context.rs:588` |

Three consequences, in the order they matter:

- **A width that moves by a whole point is a total miss.** Not a partial one: the key is per
  galley and the width is in every galley's key, so *every* paragraph in the scrollback misses
  together.
- **A sub-point wobble is free.** The rounding at `:879` exists precisely to stop egui's own
  layout feedback loop from thrashing the cache, and it means a tween finer than one point per
  frame costs nothing extra. It also means a 0 → 90 pt inset has **at most 91 distinct wrap
  widths**, which is what makes §7's quantisation option available at all.
- **The cache cannot remember the width it came from.** `flush_cache` retains an entry only if
  `last_used == current_generation`, so a tween's return leg misses exactly as hard as its
  outward leg. There is no "animate back and it is cheap".

📌 **This is pinned by a test, not left as prose.** `rewrap_bench::tests::width_is_part_of_the_galley_cache_key`
drives the real cache through four probes in one pass and asserts all three behaviours. It runs
in the default suite (it is milliseconds) for the same reason
`native/tests/egui_popup_contract.rs` does: this is a property of a pinned dependency that a
version bump can change in silence.

**Empirical corroboration, independent of the source read.** Because the flush happens at
*begin* pass, reading the cache size after a frame shows the previous frame's survivors plus
this frame's. In steady state the two sets coincide and the count is 1×; while the width moves
they are disjoint and it is **2×, at every size measured** (74 → 141, 330 → 637, 1290 → 2497,
6410 → 12417, 32010 → 62017). That is a 100 % miss rate observed rather than argued.

---

## 3. Method

`rewrap_bench` drives the **real draw path**: `conversation_view::scrollback` — the same
function `conversation_view::draw` calls — over a real `Transcript`, inside the real
`egui::ScrollArea`, on a real `egui::Context` with egui's bundled fonts, in a 1100 × 720 pt
screen rect (the console's default window). No window, no GPU.

Each condition is a `Context` and a pane built fresh, then `WARMUP + n` frames, of which the
**first 6 are discarded**. That is not a ritual number: frame one rasterises glyphs into an
empty font atlas and allocates every widget's memory, and frames two and three settle the
`ScrollArea`'s own state. The medians below are stable to a few percent across the kept
samples, and the reported min/max show the tail.

| Condition | What the width does |
|---|---|
| **steady** | constant at 1100 pt — every galley was in the cache from the frame before |
| **animating** | one point of inset per frame, triangle 0 → 90 → 0, forever |
| **one step** | steady, then a single 90 pt change, then steady at the new width |

**Passes are counted, not assumed.** `egui::Context::run` re-runs the whole ui when something
calls `request_discard`, and a doubled pass would look like a doubled layout cost.
`num_completed_passes` was **1 in every condition at every size**, so nothing below is a
multi-pass artefact.

**The corpus.** Turns of five elements — human input, a settled assistant paragraph of ~600
characters, a `Read` tool card with JSON arguments and twelve lines of output, a closing
paragraph, a run end — folded through the real `Transcript::apply`. ⚠️ **Every string carries
its turn index, deliberately:** identical text at one width is *one* galley in the cache however
many elements draw it, so a corpus of repeated paragraphs would have measured deduplication
rather than layout. A test asserts the prose is all distinct.

### Re-running it

```bash
cd native
cargo test --release -p organon-console --lib -- --ignored --nocapture rewrap
```

⚠️ **`--release` is load-bearing.** The workspace sets `[profile.dev] opt-level = 1`, which the
test profile inherits, and it reports figures about 25 % high (§5.3). The console ships release.

The default suite is unaffected: `cargo test -p organon-console --lib` is **503 passed, 1 ignored**
in 0.17 s, the ignored one being this benchmark.

---

## 4. The premise held, and one thing about it is worth stating loudly

**The transcript does re-wrap, and it re-wraps *entirely*.** `egui::Label::ui` calls
`layout_in_ui` — which builds the galley — at `egui-0.33.3/src/widgets/label.rs:278`, and only
*then* tests `ui.is_rect_visible` at `:282` to decide whether to paint. `scrollback` uses
`ScrollArea::show`, not `show_rows` or `show_viewport`, so the closure lays out the whole
transcript and egui clips the painting.

**So layout cost is a function of the whole retained scrollback, never of the viewport.** A
720 pt viewport shows perhaps a dozen elements; the cap is `Limits::max_elements = 10_000`. That
ratio is the entire shape of §5's table, and a non-ignored test
(`the_whole_scrollback_is_laid_out_not_just_the_visible_slice`) pins it so that a future egui
that *does* cull cannot invalidate this document quietly.

---

## 5. The numbers

**Machine:** `ORGANON-ONE` — AMD Ryzen Threadripper PRO 9955WX (16C), 32 GB, Windows 11 Pro
10.0.26200. **Toolchain:** rustc 1.97.1 (8bab26f4f), `x86_64-pc-windows-msvc`, `--release`
(`opt-level = 3`, `lto = "thin"`). **egui/epaint 0.33.3.** Taken 2026-08-13.

### 5.1 Per frame, by transcript size

Medians in **milliseconds**. "galleys" is the cache's own count of one frame's set — the same
in both conditions, which is the point.

| elements | what that is | steady | animating | ratio | galleys/frame | n (steady/anim) |
|---:|---|---:|---:|---:|---:|---|
| 20 | a short exchange, four turns | **0.071** | **0.483** | 6.8× | 74 | 60 / 60 |
| 100 | a working conversation | **0.288** | **2.380** | 8.3× | 330 | 60 / 60 |
| 400 | a long working session | **1.165** | **9.100** | 7.8× | 1 290 | 40 / 40 |
| 2 000 | a coordinator run | **8.144** | **50.522** | 6.2× | 6 410 | 20 / 20 |
| 10 000 | `Limits::max_elements` — the worst case that can occur | **51.552** | **308.577** | 6.0× | 32 010 | 8 / 8 |

Min and max for the same run, to show the tail:

| elements | steady min/max | animating min/max |
|---:|---:|---:|
| 20 | 0.067 / 0.084 | 0.414 / 0.674 |
| 100 | 0.287 / 0.296 | 1.980 / 3.083 |
| 400 | 1.157 / 1.240 | 7.786 / 13.720 |
| 2 000 | 6.885 / 9.211 | 45.508 / 70.081 |
| 10 000 | 49.806 / 52.218 | 296.824 / 405.109 |

### 5.2 The unit costs the table is made of

Dividing by the galley count gives a figure that does not depend on how much of a transcript is
prose and how much is tool cards, which is the number to carry to another design:

| elements | µs per galley, animating | µs per galley, steady |
|---:|---:|---:|
| 20 | 6.5 | 0.96 |
| 100 | 7.2 | 0.87 |
| 400 | 7.1 | 0.90 |
| 2 000 | 7.9 | 1.27 |
| 10 000 | 9.6 | 1.61 |

**≈ 7 µs to lay out a wrapped galley; ≈ 0.9 µs to reuse one.** The drift upwards at the two
largest sizes is not layout getting slower — it is the working set (32 010 live galleys, plus
every `Ui` allocation behind them) leaving cache; the *steady* column drifts by the same factor,
which is what identifies it as memory rather than text.

Per transcript **element**, at this corpus's mix: **≈ 24 µs animating, ≈ 2.9 µs steady.**

### 5.3 What the dev profile says, for anyone who runs it without `--release`

| elements | steady | animating |
|---:|---:|---:|
| 20 | 0.080 | 0.628 |
| 100 | 0.359 | 3.191 |
| 400 | 1.442 | 13.217 |
| 2 000 | 13.745 | 68.850 |
| 10 000 | 59.619 | 419.912 |

About 25–45 % high, and the same ratios. Do not quote these.

### 5.4 Run-to-run variance

Three release runs of the same binary, animating median (ms):

| elements | run 1 | run 2 | run 3 (the table above) |
|---:|---:|---:|---:|
| 20 | 0.461 | 0.463 | 0.483 |
| 100 | 2.423 | 2.389 | 2.380 |
| 400 | 9.217 | 8.689 | 9.100 |
| 2 000 | 48.923 | 50.651 | 50.522 |
| 10 000 | 353.499 | 308.013 | 308.577 |

⚠️ The two smallest sizes repeat to a few percent; **10 000 varied by 15 % and 2 000's *steady*
figure varied by 33 % across runs (11.258 / 7.564 / 8.144)**. Treat the two largest rows as an
order of magnitude, not as a figure. Nothing in §7 turns on their third significant digit.

### 5.5 A single width change, and what it leaves behind

The condition pane splitting actually produces, and what "snap once at the end" would reduce a
tween to. Each row is the median of whole repeated experiments — a step can only happen once per
context, so `runs` counts experiments, not samples.

| elements | the frame the width moves on | every frame after it | runs |
|---:|---:|---:|---:|
| 20 | 0.451 | 0.064 | 9 |
| 100 | 1.776 | 0.289 | 9 |
| 400 | 7.599 | 1.180 | 9 |
| 2 000 | 39.573 | 8.534 | 5 |
| 10 000 | 342.893 | 52.345 | 3 |

**A width change costs one animating frame and nothing after it.** The right-hand column is the
steady column of §5.1 to within the noise, which is the mechanism of §2 confirmed from the other
end: once the new width's galleys are in the cache they stay there for as long as nothing moves.

⚠️ **This measurement was wrong the first time and the fix is worth recording.** At one
experiment per size the "frame the width moves on" read 14.7 ms at 400 elements — half again the
continuous-tween figure — and it was tempting to explain that (allocator pressure from the
previous width's galleys still being live). Repeating the experiment nine times collapsed it to
7.6 ms, *below* the animating median. The first reading was a single sample wearing a table's
clothes.

---

## 6. The finding nobody asked for, which may be the larger one

**The steady column is already a problem.** At 2 000 elements the transcript costs **8.1 ms per
frame with nothing animating at all** — half a 60 fps budget, before the terminal tab, the
portal, the backdrop or anything else the console draws. At the cap it is **51.6 ms**, i.e. 19 fps
sitting still.

Posture's tween does not create that. It multiplies it by eight and makes it visible eight times
sooner. The honest framing of this whole document is therefore: **the transcript is not
virtualised, so its layout cost is linear in scrollback length in every condition; the re-wrap
question is the special case where the constant is eight times larger.**

Two live consequences, neither hypothetical:

- **Window resize drags already pay the animating column today.** `scrollback` takes
  `ui.available_width()`, which follows the window. A drag is a width change per frame. On a
  2 000-element session that is 50 ms per frame *now*, on `main`, with no posture and no panes.
- **`edit_diff` runs every frame, for every `Edit` card in the transcript**
  (`conversation_view.rs:3599`) — it re-parses the arguments JSON and re-runs `text_diff::line_diff`
  on every draw, cached nowhere. That is **not** in any number here (the corpus is `Read` cards)
  and it is not a wrapping cost, but it is the same shape of defect in the same loop and a
  transcript full of edits will not behave like this table.

---

## 7. The options, and what each costs

Stated with prices, **not ranked, and not chosen.** The 60 fps budget is 16.7 ms for
*everything*, so read "fits" as "fits with nothing else in the frame".

### A — Tween the margin as designed, one point per frame

**Cost:** the animating column, on every frame of the tween.

| session size | per frame | verdict |
|---|---:|---|
| ≤ 100 elements | ≤ 2.4 ms | free |
| 400 | 9.1 ms | fits, with ~45 % of the budget left |
| 2 000 | 50.5 ms | 20 fps — the tween visibly steps |
| 10 000 | 308.6 ms | 3 fps — the tween is three frames long |

Buys nothing else and needs no new code. The failure is exactly the one the brief predicted:
smooth for ten cards, janky after an hour of work, and it degrades gradually so it will read as
"the console got slow" rather than as this decision.

### B — Animate the chrome only; hold the wrap width fixed

**Cost:** the steady column throughout — **zero re-wrap.** The margin appears by moving or
painting chrome beside a transcript whose column never changes.

**What it costs elsewhere:** the transcript must be laid out at a width that is not its
container's, i.e. an explicit width threaded through `scrollback` instead of `ui.available_width()`.
There are **10 sites** in `conversation_view.rs` reading `available_width`, though not all are in
the scrollback path. And the text does not reflow into the space the margin opened, ever or until
the tween ends — which may be the correct *look* (a page whose margin changes without its
measure changing is how print behaves) or may be exactly the thing posture is for. That is a
design question this number cannot answer.

### C — Snap the width once, at the end of the tween

**Cost:** measured directly in §5.5 — one frame at the animating figure, then nothing.

| session size | the one frame | verdict |
|---|---:|---|
| 400 | 7.6 ms | inside one frame budget; invisible |
| 2 000 | 39.6 ms | ~2 dropped frames at the settle |
| 10 000 | 342.9 ms | a third of a second of stall |

This is also, unavoidably, **what pane splitting costs**, whatever posture decides. Splitting a
pane on a 10 000-element transcript is a 340 ms freeze on this machine, and this machine is a
16-core Threadripper.

### D — Quantise the tween

Follows from the rounding at `fonts.rs:879`: a margin that only ever takes N distinct integer
values is N option-C events, with the steady figure on every frame between them. A six-step
margin across a fifteen-frame tween, at 400 elements:

- continuous: 15 × 9.100 = **136.5 ms** total, worst frame **9.1 ms**
- six steps: 6 × 7.599 + 9 × 1.180 = **56.2 ms** total, worst frame **7.6 ms**

Better on both counts — less total work *and* a lower worst frame — which is not the trade-off
one would guess. ⚠️ But it rescales rather than fixes: at 2 000 elements the six expensive frames
still cost 39.6 ms each, and the motion is visibly stepped by construction. It is a way to make
option A affordable at 400, not a way to make it affordable at 2 000.

### E — Virtualise the scrollback

**Cost:** the question disappears. Layout becomes O(visible) rather than O(transcript) in *both*
columns, which is the only option that also fixes §6's steady-state cost.

**What it costs to build:** `ScrollArea::show_rows` does not apply — it assumes a uniform row
height and these elements are content-sized. `show_viewport` does, but it needs a per-element
height that is known *before* layout, and that height is a function of the width, so the cache it
needs is keyed on exactly the thing that changes. It also has to be reconciled with
`timeline::pinned_after_scroll` and `scroll_anchor.rs`, both of which read the content size the
current arrangement produces. This is a tier, not a patch.

---

## 8. What was NOT measured

Stated plainly, per house discipline. Every one of these is a real gap and none of it is hedging
about the figures that *are* here.

- **Anything on a GPU, or in a window.** `Context::tessellate` is never called; no swapchain, no
  paint, no compositor. The numbers are CPU layout in an envelope that also contains egui's
  widget logic and shape emission. Tessellating 32 010 galleys is its own cost and is unknown.
- **The real `draw()`.** Only `scrollback` is driven. The status strip and the composer are
  excluded — both are fixed-height single-band widgets whose cost does not scale with the
  transcript, but "does not scale" is an argument here, not a measurement.
- **Posture, and panes.** Neither exists. Nothing here has been run against an actual tween or
  an actual split; the width is moved by the harness in the shape those two are specified to
  move it.
- **A captured long transcript.** The corpus's *shapes* are the real ones (`Transcript::apply`,
  the real bodies, the real card) but the *words* are written here. The captured fixtures in
  `native/organon-console/fixtures/` are 11–77 lines; a four-hundred-element session has never
  been captured on this machine.
- **Three element kinds.** No approval card, no artifact panel, no rendered surface is in the
  corpus — `scrollback` draws those from pane state and the harness builds a pane with both side
  maps empty. A surface allocates a fixed 260 pt box and a texture; a panel has live sliders.
  Their re-layout behaviour is unknown.
- **`Edit` cards**, and therefore `edit_diff` + `text_diff::line_diff` per frame (§6). The
  corpus is `Read` cards. A transcript of edits is a different measurement and it is not this one.
- **Subagent logs** deeper than none, and elided output past `OUTPUT_LINES = 10`.
- **Any machine but this one.** One CPU, one OS, one build. The per-galley figure in §5.2 is the
  portable part; the per-frame table is not.
- **Whether any of this is perceptible.** 9.1 ms is a number, not a judgement about how a tween
  looks to James at 225 % scaling on his display. Nobody has watched it.

---

## 9. What would change this document

- **An egui or epaint bump.** §2 is a source read of pinned versions.
  `width_is_part_of_the_galley_cache_key` fails loudly if the keying moves; nothing fails if the
  *speed* moves, so re-run §5 after a bump.
- **Anything that culls.** If a future `scrollback` uses `show_viewport`, or a future egui skips
  layout for clipped widgets, `the_whole_scrollback_is_laid_out_not_just_the_visible_slice` fails
  and every figure in §5 is about the wrong thing.
- **A change to `Limits::max_elements`.** The 10 000 row is that constant. Halving it halves the
  worst case exactly.
