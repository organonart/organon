# Console Spike — demo script

The sequence we can actually perform, kept current as beats land. **Update the status column
in the same change as the tier that delivers a beat** — what we can show should never be a
matter of memory, and the honest answer to "can we demo this yet" is this table.

Statuses: `landed` · `partial` (say what is missing) · `not yet`.

---

## Setup

| | |
|---|---|
| Machine | ORGANON-ONE — RTX 5090, Windows 11 |
| Build | `cargo build --release --features shell-edition --bin organon-console` and `cargo build --release --bin organon` |
| Before starting | Close anything else on the GPU. Full screen. Cursor parked off-window between beats. |
| Fallback | If a beat fails live, the previous beat is the stopping point — never improvise past a broken one. |

---

## The beats

### 1 — "This is just my terminal." · Tier 0 · **landed**

Launch. `⌘T` → Pi. `⌘T` → Claude Code. `⌘T` → `htop`. Type in each.

*The point:* nobody has to be told it is a real terminal — they can see `htop` in it, and two
unmodified agent harnesses beside it.

---

### 2 — "…except the background is a lit surface, not a colour." · Tier 1 · **landed**

Reveal the substrate behind the glyphs: `ORGANON_SHELL_BACKDROP=substrate`.

*The point:* that surface is being lit, in real time, behind a working shell.
*Check:* put it beside a stock terminal on the same screen. If nobody looks twice, the
material is wrong — not the mechanism.
*Checked 2026-08-10 on organon-one:* slate plane, directional sheen (the sky's bright
quadrant + key), dither holding the gradient band-free, prompt legible at the scrim floor —
against a stock Windows Terminal it is unmistakably a lit surface. Taste headroom deliberately
unspent: the 4° lens, the key azimuth (live-tunable over the override lane), and the material
library are Tier 2's.

---

### 3 — "Watch — I'll ask for a different one." · Tier 2 · **landed** (perform with slate/metal/paper)

Ask the agent in English for a different surface. It issues `organon console background`.

*The point:* not the render — that you asked in a sentence and a real machine changed.
*Check:* under a second, no flicker, no relayout of the grid.
*Checked 2026-08-11 on organon-one:* `background metal` / `rig daylight` / back to `slate`
land within a frame of the drain, prompt row pixel-identical across switches. **Two of the
four materials are demo-grade today** — slate (Tier 1's look, formalized) and metal (dark
anisotropic brushed steel, genuinely good). Graphite reads as light corduroy and paper as
blotchy static — taste-debt with named dials (graphite: darken the albedo stops, widen the
stripe period; paper: pull contrast/AO, tighten the gradient), fix in flight. Perform the
beat with slate ↔ metal ↔ world until then.
*Updated 2026-08-11 (materials revision `6da6af4`, re-seen on the Tier 4 beat check):*
paper's clots were the bake shader's hidden ×4 AO derive — fixed, and paper is demo-grade;
perform with **slate / metal / paper**. Graphite's lamination is right (fine, matte, no
moiré) but its value still reads light — one more matte iteration, ledgered, not blocking
the beat.

---

### 4 — "I never told it what I meant." · Tier 3 · **not yet**

Type `organon`. The bottom row fills with choices. Tap one. Keep talking without naming the
subject.

*The point:* the UI was not designed — it was generated from the same vocabulary the agent
speaks.
*Check:* `htop` still renders correctly with the row reserved. Verify this **before** the tap
flow; if the sizing is off by a row, stop.

---

### 5 — "And it remembers." · Tier 4 · **landed**

Scroll up through the session. The material changes back through its own history.

*The point:* it is not a wallpaper — it is the material of the page, and the page has a
history.
*Check:* three look changes, then one continuous scroll bottom to top.
*Checked 2026-08-11 on organon-one (225 % display):* three switches (metal → paper →
graphite) with output between, then one continuous wheel-scroll to the top — four
materials banded in one transcript, every band crisp at full width including the oldest,
boundaries row-aligned and pinned to their rows through the scroll, the newest look
scrolling in from the bottom. The first check caught wide bands washing out at exactly
2.25× — the DPI-sized snapshot bug; fixed (`scene_input::pane_pixels_in` — the pane is
sized as its share of the window, so the scale cancels instead of being guessed) and
regression-pinned before this flip. `background substrate → world → substrate` probed
live between looks, no crash. One operational note for the demo: the boundary lands at
the row that is current when the command drains, so a switch fired while nothing is
printing puts the new look *below* the last line — switch while the agent is talking,
not after it stops.

---

### 6 — "…and that's the instrument, inline." · Tier 5 · **not yet**

The agent writes a paragraph with a rectangular gap in it, and something of ours is rendered
into the gap while the text flows around it.

*The point:* the closer — and note what it is *not*. The agent did not ask the console for a
window. It wrote its own page and left a space, the way a newspaper leaves a space, and the
console filled it.
*Check:* survives a scroll past and back without stalling the grid.
*Order to perform, cheapest first:* a panel of working controls in the gap needs only the rect
and one paint call. A live Organon scene in the gap additionally needs a second render whose
frame cost is unmeasured and whose clocks are shared with the backdrop — real risk, named in
the execution plan. Rehearse the controls version first; it is the one that cannot stall.
*No longer a stretch goal* (2026-08-11, James): this is the beat the spike is now aimed at, and
Tier 3's integration parked behind it.

---

## Rehearsal log

*One line per full run-through: date, platform, which beat broke, what was done about it.*
*Both platforms before a tier is called done — the ConPTY class of bug does not exist on
macOS, and Mac-only paths do not exist on Windows.*

| Date | Platform | Reached beat | Broke on | Action |
|---|---|---|---|---|
| | | | | |
