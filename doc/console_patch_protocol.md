# The Patch protocol

**What this is.** The contract by which a program running inside the Organon Console claims a
rectangle of its own output and asks the console to paint something there — a rendered scene, a
panel of controls — with its text flowing around the hole the way text flows around a figure on
a page.

**Status:** drafted 2026-08-11, from James's design. **One question gates the wire format and
must be measured before any of it is implemented** (§0). Everything else here is settled.
Amend this file rather than diverge from it.

**Scope.** A *document format and a signalling convention*, not a feature. The console consumes
claims; it does not care which program made one, and a program that has never heard of Organon
is simply one that never makes any.

---

## The three invariants

Everything else is detail. These fail silently if broken.

### 🚨 P1 — The agent owns the hole; the console never writes to the transcript

The console does not open a gap, does not insert rows, and does not move the cursor. **The
program writes its paragraph with the gap already in it** — ordinary spaces and newlines, on
ordinary stdout, through the ordinary PTY — and then claims it.

This is not a stylistic preference. The alternative, which this protocol replaces, had the
console feed synthetic lines into the terminal buffer behind the child's back. That mechanism
works (verified in `alacritty_terminal` 0.26: the absolute-line identity Tier 4 depends on
survives it exactly), but on Windows ConPTY maintains its own screen-buffer model and repaints
by absolute cursor positioning, so rows it does not know about may be painted over. Under P1
the rows are real output through the normal path, so **the shell, ConPTY and the console cannot
disagree that they exist.**

The consequence worth naming: **text flow costs the console nothing.** The program does the
flow; it is just text. The console's entire job is *given a rect and a texture, paint*. There
is no wrapping logic in our tree and there must never be.

### 🚨 P2 — The agent never types the escape sequence; the CLI emits it

A claim is made by invoking the `organon` CLI, which writes the marker **on its own stdout**.
The wire format is a private contract between two Organon components.

The temptation is to teach an agent the byte sequence. Do not. Three things go wrong at once:
the skill acquires a wire format that rots the moment the format changes; an agent that knows
the bytes will emit them somewhere they do not belong (a commit message, a log, a file); and
the id-allocation rule below becomes unenforceable. `skills/organon-cli/SKILL.md` must be able
to teach this without naming a single byte.

### 🚨 P3 — A gap is a picture of a gap at one width

**The rectangle does not survive rewrapping**, and the protocol says so rather than pretending.

Measured in `alacritty_terminal` 0.26: `Cell::is_empty` (`term/cell.rs:226-239`) treats a
literal space with default attributes as byte-identical to a never-written cell — there is no
"was written" bit anywhere in `Cell`, `Row` or `Grid` — so the grid cannot distinguish the gap
from empty space. On a width *decrease* past a paragraph's right edge, `Row::shrink` splits the
row and inserts `WRAPLINE` (`grid/resize.rs:314-319`): the line count grows, absolute indices
renumber, and the gap's columns move. On a width *increase*, any soft-wrapped line above pulls
cells upward (`grid/resize.rs:145-165`) and every column position on the affected lines
changes.

A gap survives a width increase only when the program hard-wrapped its own lines, because
alacritty declines to reflow rows it never wrapped. That is an accident of the transcript's
shape, not a property of the design, and it must not be relied on.

**So: a width change invalidates every live patch.** The console drops them and logs it. The
program may re-emit its passage at the new width — it receives SIGWINCH like any other program
— and that is the only mechanism that keeps a rectangular gap correct at all widths. **The
obligation is the program's, not the console's.**

---

## §0 — MEASURED, 2026-08-11, on ORGANON-ONE

The gate was: does a private marker survive ConPTY? `portable-pty` 0.9 defines
`PSEUDOCONSOLE_PASSTHROUGH_MODE` and **does not enable it** (`win/psuedocon.rs:31`, `:80-94` —
the flags passed are `INHERIT_CURSOR | RESIZE_QUIRK | WIN32_INPUT_MODE`), so ConPTY parses the
child's output into its own screen buffer and re-synthesises VT for the console to read.

**Measured** with `ORGANON_SHELL_PTY_DEBUG=1` and a file containing an APC sequence, a
private-numbered OSC (1338), an OSC 8 hyperlink and a plain sentinel, `type`d in a Windows tab
and `cat`ed in a WSL tab. **Both legs behave identically** — a WSL tab is `wsl.exe`, itself a
Windows process under ConPTY, so it is *not* a real PTY and there is no ConPTY-free path on
this platform.

| Sequence | Result through ConPTY |
|---|---|
| **APC** `ESC _ … ESC \` | 🔴 **Stripped entirely.** The surrounding sentinels arrived, the sequence between them did not. |
| **Private OSC 1338** | 🟡 **Survives byte-intact — but hoisted out of stream order.** It arrived in its own read *before* all of the text, though its source position is in the middle of line 2. |
| **OSC 8 hyperlink** | 🟢 **Survives in position, inline, at exactly the right point in the text** — and is **rewritten**: sent with empty params, it arrived as `ESC ] 8 ; id=47476-1 ; https://organon.test/p1 ESC \`. The **URI is preserved exactly**; the params are replaced. |
| plain text | 🟢 intact, but redelivered inside a full repaint (`ESC [ H`, `ESC [ K` per line) |

**The finding that decides the protocol is not which sequences survive — it is which one keeps
its place.**

ConPTY forwards an *unknown* sequence out of band, immediately, because it has nowhere to put
it. It forwards a *known* sequence in position because it can attribute it to cells. So on
Windows, **an unknown marker cannot carry a position**, and "the cursor is here at the marker
byte" — the mechanism §2 was built around — is not available. OSC 8 keeps its place precisely
*because* ConPTY understands it and stores it on the cells.

That is the same property recon found from the other direction: `Cell::hyperlink` is the one
per-cell slot that survives reflow. **The marker should be state in the grid, not an event in
the stream.**

---

## §1 — The sequence: OSC 8, with the payload in the URI

```
ESC ] 8 ; ; organon-patch:<nonce>/<id>/<field>=<value>&… ESC \
   …the glyphs the patch is anchored to…
ESC ] 8 ; ; ESC \
```

- **The payload lives in the URI, never in the params.** Measured: ConPTY replaces the params
  with its own `id=…` and passes the URI through byte-for-byte. A protocol that put fields in
  the params would work in every terminal except the one we ship on.
- **A private URI scheme**, so nothing dereferences it and no other tool claims it.
- **The nonce is in the URI too** — the console injects it into the child environment, as it
  already does with `ORGANON_IPC_NS` (`term.rs:190-195`). It is the only defence against
  `cat`-ing a file that happens to contain a marker, and it makes the claim session-scoped.

**Rejected, with the measurement as the reason:** APC — the recon's recommendation on
degradation grounds, and provably inert in our own parser (`vte` routes `ESC _` to
`SosPmApcString`, which discards every byte but CAN/SUB/ESC, `src/lib.rs:377`, `:438-450`) — is
**stripped by ConPTY and never arrives.** Perfect invisibility is worthless if the message does
not survive. A private OSC number survives but arrives detached from its position, which
defeats the only thing a marker is for.

⚠️ **The cost of OSC 8 is real and must be stated, not discovered:** in any other terminal the
anchored glyphs become an underlined, clickable link with an `organon-patch:` URI. We are
hijacking a standard, and the standard's own rendering is the price. Mitigations: anchor to as
few glyphs as possible, and gate emission on the console's presence (§1's nonce already
requires an environment variable the console injects, so a claim is silent everywhere else).

---

## §2 — How the console reads it: off the cells, not out of the stream

**The console does not scan the byte stream at all.** OSC 8 reaches `Handler::set_hyperlink`,
which writes into `grid.cursor.template`, and `write_at_cursor` clones the template's `extra`
into every cell written (`term/mod.rs:1874-1876`, `:984-990`). The claim therefore ends up
stored on the cells themselves, readable with `Cell::hyperlink()` (`term/cell.rs:219-221`) —
and `renderable_content`'s `display_iter` yields `&Cell`, so it is reachable at paint time in
the loop `term_view.rs:520-557` already walks.

This is strictly better than the stream scanner §2 previously specified, and it deletes four
hazards outright:

- no incremental scanner and no state on `TermSession`;
- no split-read hazard — a marker crossing a 64 KiB boundary is vte's problem, already solved;
- no split-feed and no synchronized-update staleness — position is not inferred from the stream,
  it *is* which cells carry the attribute;
- no `Handler` decorator, and therefore none of the 71-method forwarding surface where an
  unforwarded default is a silent no-op.

**Cost:** re-finding is a grid scan. Bound it to the viewport (~80×50 = 4 000 cells is free per
frame); scanning full scrollback (10 000 lines) is not, and must not be done per frame.

🚨 **The claim must ride on real glyphs, never on the gap's own spaces.** `Cell::is_empty`
(`term/cell.rs:226-239`) does **not** test `hyperlink`, so a space carrying a claim is still
"empty" — droppable by reflow's clear-row path and truncatable by `Row::shrink`. Anchor to the
text around the hole, not to the hole.

📌 **One consequence worth taking:** because the claim lives on cells and cells survive
rewrapping, the console can *re-find* a patch's anchor after a width change. That does not
rescue P3 — reflow still destroys the rectangle, and re-finding recovers where a patch belongs,
never a hole to put it in — but it turns "the anchor went stale" into "the anchor is right and
the geometry is gone," which is the difference between a patch painted over live text and a
patch that knows it must be redrawn.

---

## §3 — The claim

The coordinate frame is **relative to the anchored cells, and in cells**. The program cannot
know absolute screen coordinates and must not try; the console resolves the anchor from *which
cells carry the claim* (§2), which is why the anchor survives ConPTY and reflow when a stream
position would not.

| Field | Req | Meaning |
|---|---|---|
| `verb` | ● | `patch` to claim, `drop` to release. Reserve the names; build both. |
| `id` | ● | **Console-assigned** (see below), echoed by the CLI. |
| `up` | ● | How many lines *above the anchor row* the rectangle's first row sits. |
| `rows` | ● | Height in cells. |
| `col`, `cols` | ● | The column range, zero-based, left-inclusive. |
| `kind` | ● | What to paint. A name the console resolves — never a command, never a path. |

⚠️ **Anchor above the hole, not below it, and never inside it.** The claim rides on real glyphs
(§2), the gap has none, and text below the gap is the text most likely to be re-wrapped away
from it. Anchoring to the line that introduces the figure is both the most stable choice and
the one a program naturally has to hand.

**`id` is console-assigned and echoed, never invented by the program.** A program has no way to
know what is already live, and a collision is silent. The CLI asks, the console answers, the CLI
emits.

🚨 **Nothing in a claim is ever executed.** `kind` is a name the console looks up in its own
table, exactly as `organon console background <name>` resolves a material today. The moment a
claim can name something to run, this is arbitrary code execution from a byte stream — and it
is the kind of thing that gets designed in by accident. This is the Discover schema's I1, and it
applies here unchanged.

---

## §4 — What the console guarantees back

| Situation | Behaviour |
|---|---|
| Claim parses, id is live | The rect is painted, above the backdrop and the scrim, below every glyph. |
| Claim is malformed | Ignored, logged to stderr. Never a partial patch. |
| Nonce absent or wrong | Ignored. Not an error — this is `cat` of a file, not a claim. |
| Patch scrolls off | Its texture is subject to a **bounded, logged GPU cap, oldest evicted first** — the same policy Tier 4's epoch textures already carry. A second policy would be a second thing to get wrong. |
| Scrollback evicts its lines | The patch erodes from the top, one row at a time, and is reaped when fully evicted. `block_anchor` already computes this. |
| Alternate screen (`htop`) | Nothing is painted. The alt grid has zero scrollback, so its coordinates are meaningless — `scroll_anchor` already refuses on the same grounds. Patches return intact afterwards. |
| **Window width changes** | **Every live patch is invalidated and dropped, and it is logged.** P3. |
| Window height changes | Nothing happens. Row resize is exact (`grid/resize.rs` `grow_lines`/`shrink_lines`); `abs` is invariant. |
| `\x1b[3J` (`clear`, `reset`) | The scrollback is wiped by the terminal itself and every patch in it is gone, with no notification. |

**What an unaware consumer sees**, stated plainly rather than implied:

- **In another terminal:** nothing, plus a rectangle of blank space in a paragraph. Correct
  degradation.
- **In a pipe, a file, `grep`, or a copy-pasted transcript:** the marker bytes, visibly. This is
  intrinsic to in-band signalling and no scheme escapes it; the nonce gate in §1 is the
  mitigation, not a fix.
- **The deeper cost, already recorded in `console_addressable_surfaces.md`:** a transcript
  containing live patches is no longer a text record. It does not reproduce.

---

## §5 — What must escape `term_view::draw`

A patch rect needs `(cell_w, cell_h)` and the pane origin. The pane rect already escapes; the
metrics do not — the cell→point mapping exists in exactly three expressions, all local to `draw`
(`term_view.rs:497`, `:548-551`, `:565-567`), from a `cell_w` computed at `:378-380` and never
returned.

**Make it a public free function of the font, not a return value of `draw`** —
`cell_metrics(ui) -> (cell_w, cell_h)`. Both depend only on the `FontId` and the fonts, not on
any rect, so a caller can ask *before* `draw` decides anything. A `draw`-returns-metrics shape
would hand the caller last frame's numbers and import the one-frame lag the backdrop already
has to reason about, for no reason.

📌 This is the same structural change Tier 3's integrator already owes for `cell_h`; `cell_w`
is free from the same call. Whichever tier lands first owns it, and the other uses it.

---

## §6 — The skill's share

`skills/organon-cli/SKILL.md` teaches the *shape* and defers the surface to the live tool. That
split holds here, with one difference worth stating: **this is the first place where the skill
is the mechanism rather than documentation.** A patch exists because a program left a gap and
claimed it, and the skill is the only route by which an agent knows to do that. Elsewhere a
stale skill degrades to a missed feature; here it degrades to an agent printing escapes nothing
will ever claim.

What it must convey: that the agent owns the hole (the one genuinely new habit, and the only
thing here that cannot be discovered from `--help`); that claiming is a CLI call, not a
sequence to type (P2); that the frame of reference is relative; that a resize can move or
destroy a patch, so check; and that a claim means nothing where no console is listening.

What it must never contain: the wire format, an enumeration of patch kinds or sizes, or a second
home for names that `--help` already lists. **Layout is the console's business** — the Discover
schema's guardrail, unchanged.

---

## §7 — What is built, and where it differs from this document

**Status 2026-08-12.** A first implementation landed and was verified on screen. This section
is the honest delta; do not read the sections above as descriptions of the code.

| | Specified here | Built |
|---|---|---|
| Who makes the hole | the writer (P1) | ✅ the writer — `organon console patch --up N --rows M` |
| How the claim travels | in band, OSC 8, payload in the URI (§1) | ⚠️ **out of band**, over the `console.txt` sidecar |
| How the anchor resolves | which cells carry the attribute (§2) | ⚠️ the cursor's line **at sidecar drain time** |
| Coordinate frame | relative, in cells (§3) | ✅ `up` counts back from the line the writer is on |
| `kind` | required field (§3) | ✅ `--kind scene\|panel` |
| Width change | invalidates the patch (P3) | ⚠️ not yet enforced — a width change is currently allowed to look wrong |

**The one consequence that bites today.** The sidecar drains once per frame, so the anchor
resolves at drain time rather than at the byte. **A writer that prints its gap and claims it in
one breath is fine; one that keeps printing in between is not.** That is precisely the race §1
and §2 exist to remove, and it is the reason the in-band claim is still wanted rather than a
nicety — but the out-of-band version was the cheapest thing that could be looked at, and
looking at it is what found the far bigger error below.

🚨 **P1 has now been confirmed the hard way, and the confirmation is worth more than the
specification.** The first implementation had the *console* feed blank rows at the cursor.
Measured on screen: the prompt stranded at the top, an eight-row hole below it, and the cursor
marooned under that. The cursor is the live input point — the row a prompt sits on and a
keystroke lands in — so feeding there opens a hole **between the prompt and the typing**, which
no terminal does. It is worst when the shell is **idle**, because idle is when a prompt is
sitting there; "works against an idle shell" was written twice in these docs and was exactly
backwards. Against a real Claude Code tab the harness's whole frame shifted and it repainted
over everything.

**So P1 is not a preference about ownership — it is the only arrangement that can be correct.**
There is no console-side injection worth keeping, not even as a fallback. This document reached
that conclusion from ConPTY's byte behaviour; the screen reached it independently the same day.

---

## Tests that must exist

Pure, headless, no GPU, no egui context — the house shape.

- `a_claim_uri_round_trips` — parse/emit, with every optional field absent.
- `a_claim_without_the_session_nonce_is_ignored` — the `cat`-a-file case.
- `a_foreign_hyperlink_is_not_a_claim` — an ordinary OSC 8 link must be inert here.
- `conpty_rewritten_params_do_not_change_the_claim` — **measured behaviour, pinned**: params
  arriving as `id=47476-1` instead of empty must not alter parsing, because the payload is in
  the URI.
- `a_claim_resolves_to_the_cells_that_carry_it` — the §2 law.
- `a_claim_on_a_blank_cell_is_rejected` — `Cell::is_empty` ignores `hyperlink`, so a claim
  anchored to spaces is droppable by reflow; refuse it at the source rather than lose it later.
- `duplicate_ids_are_rejected`
- `a_width_change_invalidates_every_patch` — P3, asserted rather than hoped.
- `nothing_in_a_claim_reaches_a_process_spawn` — the I1-shaped guard.

---

## Deferred, and held deliberately

Patches in the alternate screen. Patches a program can update in place (a claim is create-or-
drop; re-emitting is the update path). Z-order between overlapping patches beyond paint order.
Any claim field describing *how* a patch looks — that is layout.

**No longer deferred, because the measurement moved it:** reflow-surviving anchors were listed
here as a rejected option when the marker was going to be an APC event in the stream. `Cell::
hyperlink` is now the mechanism itself (§2), not an alternative to it. What stays true is its
limit — it recovers *where a patch belongs*, never *a hole to put it in*, which is why P3 still
drops on a width change.

**Also worth revisiting later, not now:** `PSEUDOCONSOLE_PASSTHROUGH_MODE`. `portable-pty`
defines it and leaves it disabled (`win/psuedocon.rs:31`). Enabled, ConPTY would forward the
child's bytes unparsed and APC — the cleanest sequence on every other ground — would work.
That is a dependency change (a fork or an upstream PR) plus a Windows version floor, and it
would also mean giving up ConPTY's input translation. Named here so it is a known door rather
than a rediscovery.
