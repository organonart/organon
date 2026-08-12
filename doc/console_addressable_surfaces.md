# Console — addressable surfaces beyond the grid

**Status:** recorded 2026-08-11, from James, before any of it is built. **Not Console Spike
scope.** This file exists so two ideas are written down while the machinery that makes them
cheap is fresh, rather than rediscovered later at full price.

**Why record them now.** Both became obvious *because* of what the spike landed, and neither
appears anywhere in issue #3, issue #4, `SHELL_ARCHITECTURE.md` or the spike docs — checked.
Issue #3's patch primitive is the parent of the first idea and stops one step short of it.

---

## Where issue #3 stops

Issue #3 defines the console's core primitive:

> a console rect · a lit surface · a lighting context · an anchor

with two anchors — **scroll-anchored** (belongs to a position in the scrollback and moves with
it) and **screen-anchored** (belongs to a screen region and does not scroll). The landed
backdrop is a screen-anchored full-window patch; Tier 4's look-epoch bands are scroll-anchored
ones.

**Every patch in issue #3 is a material.** A patch is something the renderer *fills*. Nothing
in that issue contemplates a patch you can put your hands on, and nothing contemplates the
region *outside* the grid being addressable at all. Those are the two ideas below.

---

## Idea 1 — interactive patches: egui controls composited into the scrollback

### The claim

A run of rows in the scrollback can hold **arbitrary, dynamically created egui controls** —
real sliders, combos, buttons — that work, stay pinned to their own lines as the transcript
scrolls, and are generated on demand rather than laid out in advance.

### Why this is cheaper than it sounds

Three things, each already true:

1. **The console's entire frame is already egui**, painted through one wgpu pass. A widget in
   the scrollback is not compositing across two systems — it is more egui in the same pass. No
   second render target, no texture, no readback. This is the whole reason the idea is small.
2. **The rect already exists.** Tier 4 built the absolute-line coordinate system
   (`native/src/substrate_epochs.rs` — `abs = screen_top + grid_line`, with `screen_top`
   *derived* every frame so emission, scrolling and resize leave every absolute index
   invariant), the row-range → viewport-band arithmetic, and the quad painter
   (`term_view::band_quads`). Resolving "lines 4,120–4,140" to a rect on screen this frame is
   solved and property-tested.
3. **The editor already makes exactly this move.** `fixed_columns` (`native/src/lib.rs:7047`)
   pins each of its three columns to its own `Rect` as a child `Ui` and clips it to that strip,
   precisely so content cannot change the geometry. A scrollback block is the same move with
   the rect coming from the band arithmetic instead of from the window width.

### Built on the fly, from descriptors

The settled control descriptor (`doc/console_discover_schema.md`) already carries everything a
widget needs: `kind`, `label`, `range` + `taper`, `value`, `default`, `format`, `variants`,
`writable`, and a `widget` **hint**. Turning one into an egui slider is a match statement.

So "insert a panel of sliders for these parameters" is: ask for the descriptors, draw them in
the block's rect. **The strip and the block become two renderings of one vocabulary** — the
same discipline that already makes the CLI, the agent's catalog and `doc/reference/` three
renderings of one table. Tier 3's generated catalog bridge is therefore on this path even
though the strip itself is not.

⚠️ The schema's guardrail carries over unchanged: **a descriptor describes a parameter, never a
layout.** How many rows a block takes and where its controls sit is the console's business. The
moment a descriptor grows `row` or `width`, this has become a bad UI framework.

### The three real problems, in order

1. **Arbitration, which is the actual work.** The console ships *every* keystroke to the PTY
   with no focus check, and the wheel handler scrolls the terminal from anywhere in the window
   (Phase 0, §R4). Mouse-driven widgets work almost immediately; a block must claim the pointer
   and the wheel over its own rect or a slider drag fights the transcript scrolling underneath
   it. Keyboard focus — typing into a field in a block — needs a real focus arbiter between the
   block and the PTY. That is a design decision, not a hard problem, but it is the one that has
   to be made deliberately.
2. **Row reservation.** The block's rows must genuinely exist in the terminal buffer so text
   written afterwards flows *below* it rather than over it, and so the run survives reflow. This
   is the open question the Tier 5 reconnaissance is answering; it is shared with the
   live-scene block and it gates both.
3. **Lifetime, and an honesty question.** Widget *state* lives in the console, not in the
   transcript: a panel scrolled deep into history is still live when it comes back, and dies
   when its lines fall off the end of scrollback. Worth naming plainly — **a scrollback
   containing live controls is no longer a text record.** Copying it, piping it, or reading it
   back tomorrow does not reproduce what was on screen. That is a real cost of the idea and it
   should be decided on, not discovered.

---

## Idea 2 — the frame: the region around the grid as an invisible addressable panel

### The claim

The console's borders look like nothing is there. They are nonetheless a GPU-composited,
addressable surface. **A column can be pinned left or right of the terminal and made to appear
at any point**, carrying controls laid out with the same row/column system the primary Organon
editor uses — and staying invisible, costing nothing visually, when there is nothing to say.

### Why this is plausible rather than aspirational

- **Panels declared before the CentralPanel subtract from it for free.** That is exactly the
  mechanism the tab strip uses today (`shell_main.rs:1553` declares
  `TopBottomPanel::top("tab-strip")`, then `:1561` the CentralPanel) and exactly what Tier 3's
  bottom strip uses. `SidePanel::left` / `SidePanel::right` are the same primitive on the other
  axis, and **the Organon editor already uses them** — `native/src/lib.rs:1911`, `:1923`,
  `:2136`. Nothing in the terminal needs to know: the grid derives its rows and columns from
  whatever rect it is handed (`term_view.rs:378-384`).
- **The substrate already renders behind everything.** A column is therefore a *screen-anchored
  patch with controls on it* in issue #3's own vocabulary — a lit surface that happens to be
  interactive. Idea 1 and idea 2 are the same primitive at two anchors.
- **The layout is portable.** `fixed_columns` derives column width from the window alone and
  pins each column to its rect; `mind_shell::layout_workstation` (`native/organon-mind/src/
  mind_shell.rs:124`) is the precedent for splitting a workstation window into docked rects as
  a pure, testable function. Neither is coupled to the editor.

### What it changes about the terminal, and the trap

**A column appearing is a real `Term::resize`.** The grid gets narrower, the child is told, and
the child redraws. That is the same trap as the strip's auto-hide, one axis over: alacritty's
`grow_lines` decrements `display_offset`, so a resize while the user is scrolled into history
makes the view jump. Tier 3's answer — suppress the automatic case while `display_offset != 0`
— is the answer here too.

**The column arithmetic is the row arithmetic transposed.** `strip_layout` (landing with
Tier 3) owns rows↔points in one module precisely so the two cannot diverge at fractional DPI;
a column dock wants the same shape for columns↔points, and for the same reason. Do not write a
second one freehand.

### The rule that must survive

**The console says nothing about itself when there is nothing to say.** `Edition::Shell`'s
tagline is `""` and that empty string is a contract — callers skip it rather than render it
(`organon-core/src/edition.rs`). An empty frame stays invisible. A column appears because
something asked for it, and it goes away again. The moment the frame is *usually* occupied,
this has become an IDE.

---

## What the two ideas share

Both say the same thing: **the console's chrome is not a fixed frame — it is an addressable
surface the agent can summon into, at any anchor.** The scrollback is one anchor, the border is
another, and the vocabulary for what appears there is already settled and generated rather than
hand-written. That is the part worth protecting.

---

## Not spike scope — what would have to be true first

1. Rows can be reserved in the buffer such that text flows below them and the run survives
   reflow (open; under reconnaissance for Tier 5).
2. A pointer/wheel/keyboard arbiter exists between an addressable region and the PTY (nothing
   exists today; the console has no focus concept at all).
3. Control descriptors are generated from the parameter table (Tier 3 Leaf B — in flight).

None of these is a research problem. All three are ordinary work, and the first is the only one
whose answer could change the design.
