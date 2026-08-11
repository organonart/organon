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
| Build | `cargo build --release --features shell-edition --bin organon-shell` and `cargo build --release --bin organon` |
| Before starting | Close anything else on the GPU. Full screen. Cursor parked off-window between beats. |
| Fallback | If a beat fails live, the previous beat is the stopping point — never improvise past a broken one. |

---

## The beats

### 1 — "This is just my terminal." · Tier 0 · **landed**

Launch. `⌘T` → Pi. `⌘T` → Claude Code. `⌘T` → `htop`. Type in each.

*The point:* nobody has to be told it is a real terminal — they can see `htop` in it, and two
unmodified agent harnesses beside it.

---

### 2 — "…except the background is a lit surface, not a colour." · Tier 1 · **not yet**

Reveal the substrate behind the glyphs.

*The point:* that surface is being lit, in real time, behind a working shell.
*Check:* put it beside a stock terminal on the same screen. If nobody looks twice, the
material is wrong — not the mechanism.

---

### 3 — "Watch — I'll ask for a different one." · Tier 2 · **not yet**

Ask the agent in English for a different surface. It issues `organon console background`.

*The point:* not the render — that you asked in a sentence and a real machine changed.
*Check:* under a second, no flicker, no relayout of the grid.

---

### 4 — "I never told it what I meant." · Tier 3 · **not yet**

Type `organon`. The bottom row fills with choices. Tap one. Keep talking without naming the
subject.

*The point:* the UI was not designed — it was generated from the same vocabulary the agent
speaks.
*Check:* `htop` still renders correctly with the row reserved. Verify this **before** the tap
flow; if the sizing is off by a row, stop.

---

### 5 — "And it remembers." · Tier 4 · **not yet**

Scroll up through the session. The material changes back through its own history.

*The point:* it is not a wallpaper — it is the material of the page, and the page has a
history.
*Check:* three look changes, then one continuous scroll bottom to top.

---

### 6 — "…and that's the instrument, inline." · Tier 5 (stretch) · **not yet**

A live ray-traced world animating in the scrollback between two prompts.

*The point:* the closer.
*Check:* survives a scroll past and back without stalling the grid.
*If cut:* beat 5 is the ending, and it is a good one. Do not attempt this live unless it has
survived three rehearsals.

---

## Rehearsal log

*One line per full run-through: date, platform, which beat broke, what was done about it.*
*Both platforms before a tier is called done — the ConPTY class of bug does not exist on
macOS, and Mac-only paths do not exist on Windows.*

| Date | Platform | Reached beat | Broke on | Action |
|---|---|---|---|---|
| | | | | |
