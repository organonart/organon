# Console Spike — demo script

The sequence we can actually perform, kept current as beats land. **Update the status column
in the same change as the tier that delivers a beat** — what we can show should never be a
matter of memory, and the honest answer to "can we demo this yet" is this table.

Statuses: `landed` · `partial` (say what is missing) · `not yet`.

### 🚨 Amendment, 2026-08-12 (James): the beats below demo the terminal host, and that is now half the product

The console split into two front-ends (`console_spike_execution_plan.md` §5.9): the **terminal
host** these beats perform, and a **conversation view** that renders an agent's structured event
stream natively. **Every beat below stays true and stays performable** — they are the terminal
host, it is kept, and it is the universal fallback.

What changes is what the *closer* is. The strongest thing we can show stops being "a rectangle
negotiated into a character grid" and becomes **an inline artifact in a conversation that never
had a grid to negotiate with.** The new milestone — deliberately smaller than it wants to be —
is *one real agent conversation rendered natively, containing one inline artifact a terminal
could not have shown.* Not a client. Proof.

⚠️ **Do not rewrite beats 1–6 for the new front-end.** They are the record of what was built and
checked on this machine, and a demo of the terminal host is still a demo of a real thing. The
conversation view earns its own beat when it has one.

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

### 🚨 Amendment, 2026-08-11 (James): the backdrop is **not** the opening move

*"Setting the background of a terminal is nothing. People have been doing that for years. It
doesn't communicate what we are really doing — in fact it breaks the whole concept that we're
creating an illusion that we are in a terminal. When you set the whole background like that, it
is going against our entire paradigm."*

Two things follow, and they are not stylistic.

1. **The console opens indistinguishable from an ordinary terminal.** Flat black, no shading,
   nothing to notice. Verified by capture 2026-08-11; the shading everyone had been seeing came
   from `organon-console.cmd` forcing `ORGANON_SHELL_BACKDROP=1`, now removed. The console's own
   default was always off.
2. **The reveal is the patch, not the surface.** A rendered object living *in* the page is the
   claim nobody has seen before. A themed background is a solved, thirty-year-old idea, and
   leading with it invites exactly the wrong comparison — worse, painting the whole window is
   the one move that says *this is a picture with text on it*, which is the opposite of what the
   console is asserting. The illusion of being a real terminal is load-bearing, and the backdrop
   spends it.

**The material is not abandoned — it is demoted to a later move**, where the page takes on a
surface for a while and the scrollback carries it. That reads as the transcript acquiring a
material. Opening with it reads as wallpaper.

⚠️ Consequence taken in the same change: a patch no longer borrows the backdrop's texture.
`Shell::render_source` separates *what the engine draws* from *what the backdrop paints*, so a
scene can be rendered purely to fill a patch while the window behind it stays flat black. One
render, no second `World`.

---

### 2 — "…except the background is a lit surface, not a colour." · Tier 1 · **landed, but HELD — see the amendment above**

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

### 6 — "…and that's the instrument, inline." · Tier 5 · **partial — the rendered patch landed; the control panel is in flight**

The writer prints a paragraph with a rectangular gap in it, claims the gap, and something of
ours is rendered into it while the text flows around it.

*The point:* the closer — and note what it is *not*. Nothing asked the console for a window. A
program wrote its own page and left a space, the way a newspaper leaves a space, and the
console filled it. With the backdrop off (per the amendment above) this is the only rendered
thing on screen, which is exactly the claim: **an object in the page, not a picture behind it.**

*Checked 2026-08-11 on organon-one:* prose, a twelve-row figure, prose resuming beneath it, and
the prompt with its cursor **directly after the text** — no hole between. The page ends where
the text ends.

🚨 **What that check cost, and why it is the most valuable thing in this file.** The first
implementation had the *console* feed blank rows at the cursor. James caught it by looking at
the capture — *"you're filling in below the cursor; that's not possible in a console."* The
cursor is the live input point, so feeding there opens a hole **between the prompt and the
typing**, which no terminal does. It is worst precisely when the shell is **idle**, because
idle is when a prompt is sitting there waiting — so "it works against an idle shell," written
twice in these docs, was exactly backwards. Against a real Claude Code tab it was worse again:
the harness's whole frame shifted and it repainted over everything.

**There is no console-side injection that can be correct.** The writer must make its own gap.
`doc/console_patch_protocol.md` reached the same conclusion from ConPTY's byte behaviour before
this was built; the screen reached it independently. Two routes, one answer.

*Check for the panel half:* the pointer must claim the panel's rect — a slider drag must not
also scroll the transcript underneath it.
*Still unrehearsed:* a full scroll past the patch and back without stalling the grid.

---

### 7 — "That was never a terminal." · the conversation view · **landed**

Open a conversation tab (`ORGANON_SHELL_TABS=claude-chat`, or the **+** menu → *◈ Claude Code
(conversation)*). Type into the composer. The agent answers, calls a tool, and the tool call
arrives as **an object on the page** — a bordered card with the tool's name, a status badge,
its correlated id, its arguments as labelled rows, and its output — not as text a terminal
printed.

*The point:* every earlier beat negotiated with a character grid. This one never had one. The
same window, the same renderer, a second front-end — and the terminal host is still right
there in the next tab, unchanged.

*Checked 2026-08-12 on organon-one (225 % display), by the coordinator, on screen:* a real
two-turn conversation. Human turn framed and labelled, assistant prose beneath it, a **Bash
tool card** carrying `toolu_01PGPtYjUu3FQ5YyKGEpTbVv` with its command wrapped across two lines
and honestly truncated (`… (181 chars)`), its description, and its output; then the assistant's
reply, a `turn complete · success` marker, and the session id in the status line. Text wraps,
nothing clips, the composer holds focus, and the human turn renders **only** from the replayed
stream — the composer draws nothing locally.

📌 **The second turn was not scripted.** The spawned agent tripped a repo Stop hook, which
arrived as a genuine second human turn and produced the tool call. So multi-turn, tool
correlation and result rendering were all exercised by a real conversation rather than a rigged
one — which is a better check than the one that was planned.

⚠️ **Known and stated rather than hidden:** the `Edit` diff has no alignment (a one-character
change in a ten-line block shows as ten removals and ten additions); thinking blocks, notices,
rate limits and `tool_use_result` render nothing yet; the composer is single-line; and there is
no backdrop behind a conversation — banding is scrollback arithmetic and a conversation has no
scrollback to anchor to.

*Panel half checked 2026-08-12, by James at the keyboard:* `/panel` puts a live control panel
**in the conversation flow** — framed, titled, four material buttons and three labelled sliders.
Clicking `metal` changes the substrate, **seen on the Shell (WSL) tab**. End to end: a control
in a conversation drove real engine state.

🚨 **And the awkwardness in that sentence is the finding.** The effect appears on a *different
tab from the one you clicked in*, because a conversation has no scrollback for a backdrop to
band across. A control whose consequence you cannot see from where you are sitting is a bad
instrument, and no amount of wiring fixes it. **That is the argument for the next step:** the
panel should drive an artifact *in its own view* — a rendered surface a few elements up,
changing as you drag. Control and consequence in one glance.

⚠️ **Two measurement failures, both mine, both worth more than the bugs they impersonated.**
The coordinator's synthetic mouse input **never reaches this app** — a click on a tab did not
switch it — so every "I clicked and nothing happened" from that direction was worthless
evidence, and the live check had to be James's. And the command first offered for enabling the
backdrop, `set X=y && oc`, **fails silently in both shells**: PowerShell's `set` is an alias for
`Set-Variable` and never touches the environment, and cmd.exe captures the space before `&&`
into the value. `oc` now takes the backdrop as an argument (`oc substrate`) so neither trap can
recur.

⚠️ **Capture note for whoever documents this next:** `GetWindowRect` reports **logical** points
(1100×720 here), so at 225 % the window is **2475×1620 physical**. A 2300×1600 bitmap silently
crops the right edge and reads exactly like a text-wrapping bug. Oversize the bitmap.

---

### 8 — "It asks, and you answer." · approvals · **built, not yet checked on screen**

Ask the agent for something that needs permission. A card appears **under the tool card it
gates**, showing what is being asked and with what arguments, and offering allow / deny /
allow-and-remember. The decision goes back over the wire and the tool runs or does not.

*The point:* the console stops being a viewer and becomes the **authority**. It answers for
everything the agent does — `--permission-prompt-tool` was measured to gate Bash as well as MCP
tools — so the red error cards from the first real session become cards you click.

*Architecture, measured before it was built* (`doc/console_approval_protocol.md`): the console
serves MCP **in-process over loopback HTTP**, so the permission hook is a direct call into the
state the UI is already drawing — no second process, no IPC, no lifetime to supervise. Verified
live: the server binds a port and writes
`{"mcpServers":{"organon":{"type":"http","url":"http://127.0.0.1:<port>/mcp"}}}`.

⚠️ **Two things that look exactly like the feature being dead**, both measured and both worth
saying aloud before anyone demos this: a safe read-only command like `echo` is auto-approved by
a classifier that **never consults our handler**, and a vaguely-requested file lands in the
model's own pre-blessed scratchpad. **An absolute path outside it is what makes the question get
asked.**

*Status:* the machinery is verified from the outside — port bound, config correct, and the
protocol probes confirmed a real permission request arriving and being honoured. **What has not
been seen is the card itself**, because the coordinator's synthetic input neither reaches this
app (clicks) nor stays in it (keystrokes leaked into another window), so this one needs James.

---

## Rehearsal log

*One line per full run-through: date, platform, which beat broke, what was done about it.*
*Both platforms before a tier is called done — the ConPTY class of bug does not exist on
macOS, and Mac-only paths do not exist on Windows.*

| Date | Platform | Reached beat | Broke on | Action |
|---|---|---|---|---|
| | | | | |
