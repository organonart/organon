# Organon Console — what it is

> **Who this is for.** A session that knows Organon — the engine, the generators and
> surfaces, the `Shared` snapshot, the plugin/visual split, Organon Mind, the `organon` CLI
> and its catalog, the edition mechanism — and knows nothing about the console, because the
> console did not exist yet. None of that is re-explained here. What follows is the console
> on top of it.
>
> **Names, before anything else.** The product is **Organon Console**; the binary is
> `organon-console`; the crate is `native/organon-shell`, the cargo feature is
> `shell-edition`, the IPC namespace is `organon-shell`, and the living architecture doc is
> `SHELL_ARCHITECTURE.md`. The gap is deliberate — each of the working names is read by
> something else (a feature resolver, another process, a hook table) — and issue #3 owns
> closing it with deprecation aliases rather than find-and-replace. Expect both spellings
> in the tree and do not tidy them.
>
> **This is an overview, not the authority.** `SHELL_ARCHITECTURE.md` is the code-grounded
> state and wins every disagreement with this file. What this file adds is the shape and
> the argument, which are spread across an execution plan, three protocol docs, a demo
> script and two issues.

## Status vocabulary — read this before believing any sentence below

Every capability named here carries one of four words. They are not degrees of confidence;
they are different kinds of claim, and flattening them into a feature list is the failure
this document exists to avoid.

| Word | Means |
|---|---|
| **seen** | built, and a person watched it work on real hardware. The date, the machine and what was checked are in `doc/console_spike_demo_script.md` or in `SHELL_ARCHITECTURE.md` §3 |
| **unseen** | built, headless-tested, green — and nobody has looked at it. This is the normal state of recent work, and it is **not** a synonym for "works" |
| **declined** | deliberately not built, with the reason recorded. Often the more informative entry |
| **planned** | argued and named, not built |

`SHELL_ARCHITECTURE.md` §3 is the honesty ledger and is where these distinctions are
maintained in the same change as the code. If you extend the console, you add to it.

---

## 1. Why

A terminal renders a character grid. Everything that reaches it has been flattened into
cells first, and the flattening is lossy in a specific way: a tool call had a name, an
argument object and a correlation id before it became eighty columns of text, and none of
that structure survives the printing. The reader's job is to parse it back out. So is the
next program's.

There is an established lineage of GPU-accelerated terminal emulators — Alacritty and Kitty,
and more recently Ghostty. What the GPU buys in that lineage is the grid drawn quickly, and
in some of them background imagery and visual effects behind it. The canvas is still the
character cell. That is context rather than criticism: the console starts from the same
hardware and asks a different question about what the pixels are for.

The second pressure is newer. An agent's work arrives as text somebody has to read, in a
scrollback that cannot hold an object — no control that changes something, no rendered
surface, no diff that is a diff rather than a paragraph shaped like one. The interaction is
rich; the channel is a teletype. Organon already owns a renderer, a parameter vocabulary and
a CLI that agents can already drive. The console is where those meet the place the work
actually happens.

---

## 2. What the console is

**Two front-ends over one renderer.** A tab is one or the other; the fork is
`Pane` in `native/src/shell_main.rs`.

| Front-end | What it is | Rule it lives under |
|---|---|---|
| **Terminal host** | a real terminal — PTY, VT state machine, GPU-drawn grid — that runs any program unmodified and paints its cells. `htop` runs in it, `vim` runs in it, an unmodified Claude Code tab runs in it, and none of them will ever know the console exists | harness-agnostic, in full |
| **Conversation view** | no PTY at all. It spawns an agent over pipes, consumes its structured event stream, and renders turns, tool calls, results, diffs, approvals and control surfaces as native elements | harness-specific, and says which harness |

They share the window, the tab strip, the harness registry, the `organon console` command
lane and the backdrop. Below that they share nothing, which is why `Pane` is an enum and not
a flag: a conversation has no grid, no cursor, no scrollback and no absolute-line coordinate,
so every terminal-only path is skipped by construction rather than by remembering to check.

### The sentence the whole design turns on

From the execution plan's §5.9 amendment:

> **We already own every pixel. What we do not own is the conversation.**

The console runs the PTY, parses it, and paints the glyph grid itself — that is *why* a
rendered rectangle could be pinned into a transcript at all. The thing it did not own was the
structure that existed before the flattening, and every wound in the terminal-host work came
from trying to recover that structure afterwards. And James's other half of it:

> **A TUI is not a design; it is what you build when a character grid is the only canvas
> allowed.**

Which makes "it looks exactly like a terminal" a skin the console chose rather than a
constraint it is under — a stronger claim than the one it replaced, not a weaker one.

### The three measurements that retired the character grid

The fork was not a preference. It was forced, on this machine, by three results:

1. **ConPTY rewrites the byte stream.** Probed with `ORGANON_SHELL_PTY_DEBUG=1`: APC
   sequences are stripped entirely, a private OSC survives byte-intact but is hoisted out of
   stream order, and OSC 8 survives in position with its params rewritten. A WSL tab is
   `wsl.exe` under ConPTY, so **there is no ConPTY-free path on this machine.**
2. **Console-side row injection against a real agent tab destroyed it.** `organon console
   block 10` in a live Claude Code tab shifted the harness's entire frame, scrolled its
   banner off, displaced its input box, and rendered no patch. A harness owns the grid and
   repaints by absolute positioning.
3. **The cursor test inverted a claim written twice in these docs.** Against an *idle*
   shell, a console-fed hole lands between the prompt and the cursor — which is where you
   type. "It works when the shell is idle" was exactly backwards: idle is when a prompt is
   sitting there waiting.

The conclusion drawn was not that the console owns too little. It was that the conversation
is the thing worth owning, and that a front-end which consumes structure directly needs none
of the negotiation — no claim protocol, no absolute-line anchoring, no reflow invalidation,
no ConPTY.

It was not a rewrite. Nearly all of the expensive work carried — the anchor arithmetic, the
epoch texture cache with its bounded and logged eviction, the seam that separates what the
engine draws from what the backdrop paints, the DPI-cancelling pane sizing, the scrim's
contrast floor, the UV policy that makes a patch a *window* rather than a thumbnail.
**What the pivot deleted is the negotiation, not the primitive.**

### Rule 5′ — the rule that keeps both halves honest

The spike's original rule 5 was *harness-agnostic or it does not ship*. It was repealed **in
writing** rather than quietly dropped, so nobody enforces it against the pivot later:

> **Rule 5′ — the terminal host is harness-agnostic; the conversation view is
> harness-specific and says which harness.** Nothing in the terminal host may require
> cooperation from any program. The conversation view may require exactly one named
> integration at a time, declared in the plan, and **degrading to a terminal tab is always
> available** — a harness we have not integrated is not unsupported, it is supported the old
> way.

That last clause is the original rule's real value, preserved: no user is locked out by our
not having integrated their tool. Claude Code is the one named integration today; Pi is the
declared second, mapped onto the same transcript events rather than a second vocabulary.

---

## 3. Key features

### 3.1 The terminal host

The grid itself — PTY (`portable-pty`), VT state machine (`alacritty_terminal`), the full
colour stack, scrollback, bracketed paste, a pinned key table, tabs and a PATH-detected
harness registry — is **seen**: two unmodified agent harnesses and `htop` in tabs beside each
other. ⚠️ Every recorded beat check was on Windows (ORGANON-ONE), and the demo script's
rehearsal log, which exists to record a full run-through **on both platforms**, is empty. One
Windows fact is worth carrying: the byte path was blocked for a while by ConPTY's DSR-CPR
handshake, whose reply the VT machine computed and then discarded, and Windows behaviour
beyond that one regression test is confirmed by a person on the machine — a green Windows CI
leg is not that confirmation.

**The backdrop, and why it is not the opening move.** The engine renders behind the glyphs,
under a legibility scrim whose floor is clamped in code and cannot be configured away. Two
sources: the live `World`, or a flat lit plane dressed by four materials and two lighting
rigs. **seen** — slate, metal and paper are demo-grade and graphite reads light and is
ledgered. ⚠️ Note a live disagreement between two sources here: the honesty ledger still says
*"no GPU has seen Tier 2's materials"*, and the demo script records them checked
material-by-material on 2026-08-11. The demo script is the later record; the ledger entry was
not revised after the beat check. Trust the demo script and fix the ledger.
But the console **opens flat black, indistinguishable from an ordinary terminal**, on James's
ruling: *"setting the background of a terminal is nothing … when you set the whole background
like that, it is going against our entire paradigm."* A themed background is a solved
thirty-year-old idea, and painting the whole window is the one move that says *this is a
picture with text on it*. The material is a later beat, not the opening one.

**Look-epochs — the transcript remembers what it was written under.** A look change applies
*forward*: it closes the live look at the current absolute scrollback line and opens the next
one below the cursor, so the new material scrolls in from the bottom and every older region
keeps the look it was written under. Nothing is restyled after the fact, and a
restyle-everything path is explicitly **declined**. **seen** — three switches then one
continuous scroll, boundaries row-aligned and pinned to their rows on a 225 % display. That
check also found a real bug (an epoch picture filed at 2.25× too small), which is the case for
beat checks in one line.

**`block` — built, and superseded by its own measurement.** `organon console block <rows>`
feeds blank rows into the parser at the cursor. It works, it is bracketed correctly, and
measurements 2 and 3 above say there is **no console-side injection that can be correct**. It
is kept only for a shell that is provably idle. Treat it as **declined** as a mechanism.

**`patch` — the writer prints its own gap.** `organon console patch --up N --rows M --kind
<scene|panel>`: the program prints ordinary blank lines through the ordinary PTY — rows the
shell, ConPTY and the console all agree exist, because they arrived the normal way — and then
says where they are. The console writes nothing into the terminal, ever. The `scene` kind
samples the rendered substrate through the rows as a window onto it; the `panel` kind draws a
live egui panel into the rect, whose buttons enter the same command lane a typed
`organon console background metal` lands on. Scene: **seen** (prose, a twelve-row figure,
prose resuming beneath it, prompt directly after the text). Panel: **unseen** — its pointer
claim, the one check that matters, is listed unperformed.

The known limits are recorded rather than pending: a width change reflows and invalidates a
patch, eviction erodes one from the top, `\x1b[3J` wipes scrollback silently, and the sidecar
is drained once per frame and so is out of band with the PTY byte stream. The in-band fix (an
OSC claim resolved from the cells) is specified in `doc/console_patch_protocol.md` and
**planned**.

### 3.2 The conversation view

A tab spawns the agent itself — `-p --input-format stream-json --output-format stream-json
--include-partial-messages --replay-user-messages --verbose` — and that argv is a measured
contract, not a guess: it keeps **one session alive across many turns**, with one
`session_id` and a `result` per *turn*. Spawn once, never let the process go; resume is the
recovery path, not the interaction model. **There is no attach** in any of Claude Code's
programmatic surfaces, so a conversation tab cannot mirror a session running elsewhere — it
must *be* the session. That is a product consequence, not a protocol detail.

Six modules carry it — a decoder that owns its own line buffering (a chunk boundary mid-line
is the normal case), a transcript model with no egui and no clock, the one seam file that
knows both types, a live-child driver, the drawing, and a small pure text-alignment module.
The load-bearing mapping rules each come from a measurement and each produce a view that looks
*nearly* right if you get them wrong; `SHELL_ARCHITECTURE.md` §1.1 has them, and the first —
an `assistant` line carries **one content block, not a whole message** — is the one that
silently eats the assistant's prose if you pass `message_id` straight through.

What the flow renders:

- **Tool cards** — name, arguments as labelled fields, a correlated id, a status accent, and
  the output clipped with a count of what was clipped. "A tool is running" has no event
  anywhere in the stream; it is derived from an unresolved id. **seen** (a real two-turn
  conversation with a `Bash` card in it, 2026-08-12).
- **The `Edit` diff** — `old_string`/`new_string` arrive as *fields*, so the card aligns them
  (prefix/suffix trim, LCS, elided runs) rather than printing one block of red and one of
  green. Measured: one changed character in a ten-line block is one removal and one addition,
  and the same change 200 lines into a 400-line block costs the same rows. Three bounds, each
  naming what it kept back. **unseen** — not one row of it has been drawn in front of anyone.
- **Subagent activity, inside the card that spawned it.** A dispatch card used to sit on
  "running" for eight to sixteen minutes and then produce a wall of text; the events were
  arriving the whole time and were being dropped. They are now folded onto the card by
  `parent_tool_use_id` — the only transcript event that addresses an existing element rather
  than appending one, which is what stops a subagent acquiring a turn of its own. 🚨 **There
  is no live text here and nothing may imply there is**: Claude Code never forwards
  token-level deltas from a subagent, so a step is always a completed burst and the gaps
  between bursts are real. The card reports counts and completed steps. **seen once** — James
  ran a real fan-out on 2026-08-13, the structure was right, and the step marker was a
  missing-glyph box. The replacement markers are **unseen**.
- **An inline artifact, and a rendered surface with the panel that drives it.** `/surface`
  summons a rendered surface and, directly beneath it, a panel whose sliders and buttons
  change *that* surface — control and consequence in one glance, in the same view. The element
  is a *description* (a title and names, no values, rects or closures); live widget values
  live in the view, keyed by stable element id, or a slider snaps back mid-drag. The rect
  comes from one `allocate_exact_size` call; set that against what the terminal host needs for
  the same rectangle and you have the size of the difference the second front-end buys.
  **unseen.** Its predecessor `/panel` was **seen** and then **removed**: it drove the
  console's backdrop, which a conversation has no scrollback to show, so its effect appeared
  on a different tab from the one it was clicked in. A control whose consequence you cannot
  see from where you are sitting is a bad instrument.
- **Thinking blocks** — decoded, given a block ordinal, and rendered as nothing. **declined,
  with a date on it:** no capture on this machine contains one, and building a second render
  path against an unobserved shape is the mistake the subagent fixture already charged for
  once. Re-scope the first time a capture shows one.
- **Notices, rate limits and the five subagent-lifecycle `system` subtypes** — read for facts
  or not at all; nothing is drawn. The five subtypes (`task_started`, `task_progress`,
  `task_updated`, `task_notification`, `task_summary`) carry a rolling description, the last
  tool name, tool counts and a terminal status, each naming its card by a `tool_use_id`. This
  is a **gap, not a decision** — nobody knew the lines existed until a real capture showed
  them — and it is the cheapest remaining improvement to a coordinator view. **planned.**

### 3.3 The console as approval authority

This is the feature that makes the console more than a viewer, and it rests on one measured
flag. `--permission-prompt-tool` names an MCP tool the client consults **for every tool the
agent calls, `Bash` included** — measured against `claude.exe` 2.1.228, with a command that
had bounced unaided earlier the same day routed to the handler and executed on approval. So
one card answers for everything the agent does, not only for the console's own verbs.

Three findings shape the implementation, and two of them are refutations:

- **MCP buys no permission exemption.** A trivial, side-effect-free MCP tool bounced exactly
  like Bash. The gate is entirely client-side — the probe server's log proves it never
  received `tools/call`. The case for MCP had been argued the other way; the measurement
  killed it. MCP's real value is **legibility**: an approval card can name a capability
  ("show a control panel") instead of a shell command a human has to parse.
- **The console serves that MCP tool itself, in-process, over loopback HTTP.** A stdio server
  is a separate process with no access to the UI, so every approval would cross a process
  boundary and come back. Over HTTP the client connects *out* to us and the permission hook
  is a direct call into the state the UI is already drawing.
- **The client's patience is 60 seconds, and the architecture doc used to say "a card with no
  timeout".** That was false and a human found it — a card sat asking a question whose answer
  could no longer matter, while the write it gated had already failed. Measured with a probe that never
  answered: 60.010 s and 60.005 s to socket abort. With `notifications/progress` against the
  request's own token every 5 s: answered at 90 s. Every 10 s: answered at 300.1 s after 29
  beats, the write went through. So progress notifications reset the clock, and the console
  sends them at a sixth of the deadline. The same beat doubles as a liveness check: a closed
  socket ends the wait, the console **denies** (fail closed, always), and the card becomes a
  third state — dimmed, no buttons, *"the agent stopped waiting."*

**Status:** the card and the whole path are **seen** — a human has driven it, which is how the
deadline was found. ⚠️ A second live doc disagreement: the demo script's beat 8 still reads
*"built, not yet checked on screen"* and says the card itself has not been seen. The ledger is
the later record and the deadline discovery is the evidence; the beat's status column was not
updated. **unseen:** a card left unanswered for five minutes and then clicked, which is the
keep-alive fix's own case. **declined for now:** the console serves *no*
capability tools; `McpServer` generates them from the same command table the CLI is generated
from and the console passes an empty one, because dispatch needs a service bound to the UI
thread. The seam is named `NoDispatch` rather than implied.

Two things look exactly like the feature being broken and are neither: safe read-only `Bash`
is auto-approved by a built-in classifier that never consults the handler, and a vaguely
requested file lands in the model's own pre-blessed scratchpad — **only an explicit absolute
path outside it makes the question get asked.** And one hazard worth stating once: 🚨 **never
serve a second approval-shaped tool.** Claude Code removes the *named* handler from the
model's own tool set precisely because the flag names it, so the model cannot hand itself
`{"behavior":"allow"}`. Any other approval-ish tool would be an ordinary model-callable one
with no such protection.

The decision memory is the console's own — there is no upstream persistence, and three
identical calls produced three separate requests. It keys on the **whole call** (`Bash` with
*this* command, never `Bash`), a remembered decision **still renders a card** saying so, and
that card carries a `forget` button: an authority granted once and thereafter invisible is
worse than being asked every time. Scope is the session, nothing is written to disk, and so it
is also **unaudited** — the honest trade for this tier rather than a feature.

### 3.4 The status strip

One line under the composer, decided by one pure function so its priority ordering is testable
without spawning an agent: a dead process, then a halted agent waiting on a human, then N
tools running, then generating, then the agent's own sentence about what it wants, then ready.
Two of those orderings are deliberate — live work outranks a `needs_action` describing a turn
that has already *ended*, and `3 tools running` outranks `generating` because the band has one
line and the specific sentence can be checked against the cards above it.

Beside the standing: a **model plate** carrying the identifier verbatim (not prettified to a
nice name — a lookup table silently mangles the first identifier not on it, which is a strip
lying about which model you are talking to), a **permission-mode plate**, dim chips (session
cost, remembered decisions, last turn's wall time), a truncated diagnostic line, and a
**context ring**.

**Both plates are controls.** A `control_request` goes down the same stdin turns go down and
the ack comes back on the same stdout: `set_model` acked in 272 ms, `set_permission_mode` in
17 ms, no handshake needed. Two things worth knowing before touching this: correlation is the
entire hazard, because a response carries an id the console invented and nothing else saying
which verb it answers; and **nothing is ever gated on an ack** — a 20 s deadline releases a
marker rather than unblocking a wait.

The mode control exists mainly because of one mode. Put a session in `dontAsk` and 🚨 **it is
not a bypass that lets things through** — prompts never reach the console's handler and gated
tools come back *refused*, while the console still passes the flag, still holds the handler
and still looks like the authority. The user's experience is "the agent suddenly cannot do
anything and nobody asked me why". So the picker's rows are labelled by what happens rather
than by the mode's name, and a persistent amber marker sits on the band for as long as the
mode is not `default`, derived every frame so it cannot get stuck on, stuck off, or be
dismissed. Amber and not red on purpose: this band is looked at for hours, and a permanent
klaxon is one the eye learns to skip.

The **context ring** is the clearest worked example of the honesty rule in the tree, and is
worth reading before adding any readout. It was **declined** outright at first — the
denominator lived in an undecoded block and the numerator "lived nowhere". Half of that was
wrong: the window was merely undecoded. But the obvious numerator, a `result`'s `usage`, is
**a turn total wearing the shape of a prompt**. Measured on a capture whose one turn makes two
API round trips: the requests carry prompts of 52 556 and 54 050 tokens, and the `result`
reports 106 606 — exactly their sum, and **1.97× the conversation actually in front of the
model**. A ring built on it would have sat at 11 % where the truth was 5 %, filled at twice
the real rate, and looked entirely plausible. The honest numerator is `message_start.usage`, a
prompt size per request, so the ring measures *context at the last request* and steps per
round trip. Still declined, for the original refusal's reasons: a quota percentage (a status
and a reset time, no numbers anywhere) and a session token total (only cost accumulates on the
wire; summing per-turn usage double-counts every cache read).

**Status:** the strip **as it stood on 2026-08-12** is **seen** — James drove a live
conversation tab and the model plate read as an identity at real width. 🚨 **Everything added
since is unseen, and the unseen list is longer than the seen part**: the generating standing,
the model picker and its pending annotation, the permission-mode plate and its marker, the
context ring including the empty-track decision that only a person can settle, the cold-start
cost chip, and the replacement glyphs from the tofu fix. 433 green tests in the compositor lib
are not a substitute for having looked once.

### 3.5 The portal

`organon console portal open`, typed at a prompt *inside* the console, floats a rendered
window over the transcript. The transcript keeps scrolling underneath; the portal holds its
place **on screen**. Drag to orbit, wheel to zoom, `portal close` to give the rows back.

Screen-anchored is the new thing. Every anchor the console had before it was a *scroll*
anchor — a rectangle pinned to a run of lines, riding them off the top. This is the
complement. Three properties are worth carrying:

- **It shows the `World`, not the substrate, and that is correctness rather than taste.** An
  installed substrate rig overrides the camera wholesale, returning its six-tuple before
  yaw/pitch/distance are consulted — and those three are exactly what camera input writes. A
  substrate portal would read a drag, apply it, and draw an identical frame, with a green
  build and no log line.
- **It makes "drive Organon from the console" visible for free.** The CLI's parameter lane
  drains inside the world's frame path, which is what the portal's render runs, and the
  console injects its IPC namespace into every tab it spawns. So `organon set glow 1.0` or
  `organon generator dna`, typed at a prompt in a console tab, changes what is inside the
  window — with no new code. That has been true since the command lane landed; the portal is
  the rectangle that makes it visible.
- **At most one engine frame per console frame, in every state**, proven over the whole input
  space. So an open portal *takes* the frame, and the stated cost is that the backdrop does
  not paint and a scene patch has no picture while it is open.

🚨 **Status: not one pixel has been seen.** It was built in a cloud session with no GPU.
Whether a 42 %-width rect at the top right reads as *floating* rather than as a hole, whether
the drag orbits at a rate a hand likes, whether the default world is legible at that size —
all **unseen**, all needing a person. Two known gaps, both recorded: in a *conversation* tab
the wheel over the portal zooms **and** scrolls the transcript, because that front-end's
scroll area has already read the delta; and a window-resize drag reallocates the portal's
texture every frame. Immersive, full screen and the animated grow between the three rects are
**planned**, with the seam (a state machine total over `(state, event)`) already in place.

---

## 4. The skill and the CLI

**The `organon` CLI is the agentic API, and the argument is stronger than "it already
exists".** The console spawns the agent itself, so it could hand tools over MCP — and it
should, eventually, for legibility. But the CLI is **the only interface every agent already
has**. Claude Code has Bash, Pi has bash, Codex and Cursor and any foreign CLI can run a
command. Nothing has to implement anything. That is the same property that made the
harness-agnostic rule right for the terminal host, and it is the one thing the pivot did not
invalidate. MCP reaches only harnesses that support it *and* that we spawn; the terminal host
cannot use it at all.

🚨 **If MCP is ever added, it is generated from the same table the CLI is generated from.**
One vocabulary, many renderings — the CLI, the agent's catalog, `doc/reference/`, and MCP as a
fourth if it earns its place. A hand-written MCP server beside a hand-maintained CLI is
exactly the failure this tree has already paid for once: three hand-written range tables, 9 of
45 ids silently wrong, published documentation shipping the wrong bounds.

Three consequences of the pivot for the CLI itself:

1. **It gains a return path it never had.** In the terminal host, `organon console …` was
   fire-and-forget into a sidecar and the console could not answer. In a conversation tab the
   command's stdout comes back as a tool result the agent reads *and* the view renders — so
   commands can be **queries**, not only imperatives. The mechanism is there (a tool card
   renders output already); query-shaped verbs are **planned**.
2. **The position arguments disappear.** `--up N --rows M` existed because the agent had to
   say *where* in a character grid. In the conversation view **the tool call is the anchor**.
   **planned.**
3. 🚨 **The front-end distinction belongs in the CLI, never in the skill** — **planned**. An agent in a
   terminal tab must print its own gap and claim it; an agent in a conversation tab must not.
   The console already injects its namespace per tab, so the CLI can detect which front-end
   invoked it. A skill that says *"if you are in a terminal, first print twelve newlines"* is
   an instruction that rots and that agents get wrong under pressure.

There is a real coupling here and it has already bitten: **if the CLI is the agentic API, the
agent's permission layer is the gate on it.** Three tools bounced on approval in the first
real session. If `organon console <verb>` needs approval every time, the artifact never
appears — and unlike a refused `env` read, that failure reads as *our feature is broken*
rather than as a policy working correctly. This is why approvals were built before the
rendered-artifact work rather than after. Note the loop this closes and the one thing in it
that is a derivation rather than a measurement: an agent in a conversation tab running
`organon console background metal` through Bash is gated by **the console's own card**, which
follows from the measured fact that the flag gates Bash — and whether that particular command
is instead swallowed by the safe-read-only classifier has not been measured.

### The skill

`.claude/skills/organon-cli/SKILL.md` is what an agent reads instead of the source. It has two
halves: operating Organon from outside (the see → act → see loop, the grammar, the eyes), and
**changing the console from inside a console tab** — which file owns which subsystem, the
verification bar, and four traps that have each cost real time and none of which the code
admits to.

**What goes in is the *shape*. What never goes in is an enumeration.** The skill already makes
that split correctly — *"the live catalog is the authority … ask the tool, not your memory"* —
and it is the only reason a file this size can describe a surface with ~1,370 parameters
without rotting. A new command gets its grammar and its place in the loop; what lives *inside*
it stays discoverable from the tool.

🚨 **A stale skill degrades worse than an absent one.** It makes an agent confidently call a
command that does not exist, or miss one that does. Wrong is materially worse than absent
here, which is why "any change that adds or changes a command updates the skill" is a rule
with a Stop hook behind it rather than a tidy-up item — and the hook is the safety net, not the
instruction, since it fires after the fact and a sub-agent may never see it at all. The
severity climbs again under §5.9.26: a skill that is merely documentation degrades to *"the
agent does not know a command"*; a skill that is **the self-extension mechanism** degrades to
*"the agent cannot extend the thing it lives in, and has no way to discover that."*

---

## 5. Forward

**What the two front-ends make possible that a grid did not.** An element in a conversation
can be a live control whose consequence is visible a few rows up; a rendered surface can sit
in the flow with a panel driving it; an approval can be a card with the arguments as fields
rather than a line of prose with a y/n after it; a diff can be a diff. None of that needs a
claim protocol, absolute-line arithmetic, reflow invalidation or ConPTY. And the terminal host
does not lose anything: it remains the universal fallback, and the portal shows that a
screen-anchored rendered object is reachable there too.

The nearest concrete step is small and already argued: **the agent summons an artifact with a
tool call**, the integrator answers it with the same method `/surface` calls locally, and the
tool card is the anchor. The local command was scaffolding for exactly this, and deleting it
touches nothing that draws.

One item from the original design is **planned** and worth knowing about, because the fork
made it easier rather than harder. Issue #3's Tier 3 is a UI that *generates itself* from the
same vocabulary the agent speaks: a program emits a small curated payload — what this context
is, and the five to seven things somebody here might mean — and the console renders it, so
tapping composes a phrase into the input rather than firing a command. Curated, not
enumerated, is the whole distinction. In the terminal host that tier's risk was reserved-row
arithmetic, with `htop` as the canary; in the conversation view there is no grid to reserve
from and the strip is an ordinary element, so **the keystone becomes the descriptor bridge**,
which is also what makes a control panel renderable at all. Not built; the schema is drafted
in `doc/console_discover_schema.md`.

### Self-extension — the paradigm being extended, and what is genuinely open

James's framing, recorded verbatim in §5.9.26:

> "This was the idea behind Pi. It is the first self-extending agentic harness. And what that
> means mostly is that the creator of Pi gave the agent its own docs, installed with it, and
> an accompanying skill or context instructing the agent how to extend itself. I am
> consciously extending that paradigm to organon-console."

**The paradigm is three things, and only one of them is runtime machinery.** The agent's own
docs, installed alongside it. A skill teaching it how to modify itself. And a loop where a
change takes effect.

Two of the three exist here, and the first is stronger than a shipped-docs approach can
normally be: `SHELL_ARCHITECTURE.md` is the console's living state and it is **hook-enforced**
— `.claude/hooks/doc-rules.sh` maps `native/organon-shell/src/*.rs` to it, so the code cannot
move without the doc being called for. A shipped snapshot ships and hopes; here the drift is
caught by machinery. The skill exists too, and now carries the self-extension section.

⚠️ **The third is deliberately not answered, and pretending otherwise is the first mistake.**
James explicitly deferred it: *"we don't need to figure that all out tonight like how to hot
load etc."* Three things are open — how a change takes effect (hot load, rebuild-and-relaunch,
or data-only), what the skill actually says about it, and **whether self-extension reaches
code at all or stays at the data seams**. The skill therefore carries an instruction not to
build a hot-load path, a plugin system, or any new runtime machinery, because reaching for
hot-loading Rust is precisely the first mistake that section exists to prevent.

**There is exactly one working self-extension seam today, and it is the template.** The
harness registry seeds from the built-ins, reads a user JSON file from the store root, and
merges user entries over the built-ins by id — so a new tab type is a **data change with no
rebuild**, and this machine already relies on it. Two silent traps come with it: an
*unparseable* file is swallowed exactly like an absent one (which is how a byte-order mark
makes a valid-looking config do nothing at all), and `cwd` is a path in the namespace the
process actually *starts* in. The general point worth recording before anyone reaches for the
hard version: **the gap between "edit source and rebuild" and "hot load" is far narrower for
anything expressible as data than for code**, and a good deal of the console is data-shaped —
command specs, material names, harness rows. Only harness rows are read from disk today.

### 🚨 The paradigm's delivery has been demonstrated to fail, silently

This is the part to take seriously, because it is the failure mode that cannot be recovered by
the agent noticing.

The mechanism was already broken once, in a way that produced no error at all: the skill was
committed as a git **symlink**, and on a Windows checkout without `core.symlinks=true` it
materialised as a 24-byte text file containing a path. No warning, no error — the skill simply
was not there. A directory junction installed machine-side hid it on the one machine anybody
tested. That is fixed: `SKILL.md` is now an ordinary tracked file at the path the tool reads,
and a fresh clone on any platform gets a real directory.

⚠️ **And it has failed again since.** An agent asked for the skill **by name** answered
*"Unknown skill"* as recently as this session. The specific cause is not established here, and
it is not the same defect as #19 — but the class is: **skill delivery fails without saying so,
and the agent has no way to tell "this skill does not exist" from "this skill did not reach
me".** For self-extension that failure is not a missing feature; it is an agent that cannot
change the thing it lives in and cannot discover why. Anyone building on §5.9.26 should treat
verifying delivery — on the actual machine, from the actual harness, by name — as part of the
mechanism rather than as setup.

---

## Where to go next

| You want | Read |
|---|---|
| The code-grounded state, and what has not been seen | `SHELL_ARCHITECTURE.md` — §1 terminal host, §1.1 conversation view, §2 claimed seams, §3 the honesty ledger |
| Why the fork happened, and the rules for working on it | `doc/console_spike_execution_plan.md` — §5.9, §5.9.25, §5.9.26, §6 |
| The measured wire behaviour | `doc/console_approval_protocol.md`, `doc/console_session_control_protocol.md`, `doc/console_patch_protocol.md` |
| What can actually be demonstrated today | `doc/console_spike_demo_script.md` — the status column is the honest answer |
| The design and the vertical slice | issues #3 and #4 |
| Driving it, and changing it from inside it | `.claude/skills/organon-cli/SKILL.md` |
