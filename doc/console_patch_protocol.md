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

## §0 — The gate: does the marker survive ConPTY?

🚨 **Measure this before implementing anything below it. It decides §1 and §2, and a negative
result is entirely plausible.**

`portable-pty` 0.9 defines `PSEUDOCONSOLE_PASSTHROUGH_MODE` and **does not enable it**
(`win/psuedocon.rs:31`, `:80-94` — the flags passed are `INHERIT_CURSOR | RESIZE_QUIRK |
WIN32_INPUT_MODE`). So on Windows, ConPTY parses the child's output into its own screen buffer
and re-synthesises VT for the console to read. A sequence ConPTY does not implement is very
likely **not forwarded at all** — this is the known reason Sixel and the Kitty graphics
protocol historically did not work through ConPTY. If that holds, an APC marker never reaches
the console on the primary development platform and the recommendation in §1 inverts.

**The measurement**, which takes minutes: run a console tab with `ORGANON_SHELL_PTY_DEBUG=1`
(the raw-read trace at `term.rs:137-146`, `:236-239`), `printf` each of an APC sequence, a
private-numbered OSC, and an OSC 8 hyperlink, and read which arrive. Run it in **both** a
Windows shell tab (ConPTY) and a WSL tab (a real PTY), because only the first is at risk.

| Outcome | §1 becomes |
|---|---|
| APC arrives intact through ConPTY | **APC**, as specified below |
| APC is swallowed but OSC 8 arrives | **OSC 8**, and §2 changes to a `Handler` decorator — a materially larger change (`Handler` has 71 methods, and an unforwarded one is a silent no-op), which is exactly why this is measured first rather than discovered late |
| Nothing private survives ConPTY | The Windows path falls back to console-side row injection (`feed_local`, already built) and P1 holds only on real PTYs. Record it as a platform split rather than a defeat. |

---

## §1 — The sequence

**Recommended, pending §0: APC, 7-bit introducer, ST-terminated.**

```
ESC _ organon ; <verb> ; <k=v> ; <k=v> … ESC \
```

Why this class:

- **It is provably inert in our own parser.** `vte` 0.15 routes `ESC _` to `SosPmApcString`
  (`src/lib.rs:377`), whose handler discards every byte except CAN/SUB/ESC
  (`src/lib.rs:438-450`). It cannot print, cannot move the cursor, cannot desync the parser.
  The `Perform` trait has no APC hook at all (`src/lib.rs:761-823`).
- **There is no number to collide on.** Private OSC numbers are an unmanaged namespace — `7`,
  `9`, `99`, `133`, `777`, `1337` are all informally taken, with no registry. APC carries only
  a leading token, and `organon` cannot be confused with Kitty's `G`.
- **ST, never BEL.** BEL does *not* terminate APC — inside `SosPmApcString` `0x07` is simply
  discarded — so a BEL-terminated marker would swallow the rest of the stream in any
  conforming terminal. This is a live footgun, not a theoretical one.
- **7-bit `ESC _`, not the 8-bit `0x9F`**, so nothing 8-bit-unclean or UTF-8-normalising
  mangles it.

**Emission is gated.** The CLI emits a marker only when a console is listening, and the marker
carries a **per-session nonce** the console injects into the child environment — the precedent
is `ORGANON_IPC_NS`, already injected at `term.rs:190-195`. The nonce is the only defence
against `cat`-ing a file that happens to contain the marker bytes.

---

## §2 — How the console reads it

**Pre-scan the byte stream in `TermSession::pump`, and feed every byte to the parser anyway.**

`pump` drains 64 KiB reads (`term.rs:219`, `:240`) and calls `parser.advance(&mut term,
&bytes)` (`term.rs:280`). The scanner observes; it never filters. Because APC is provably
swallowed, passing the marker through costs nothing and means **a false positive in the scanner
can never corrupt the terminal** — the worst case is a patch that should not exist.

Three properties the scanner must have:

1. **Incremental and byte-oriented, with state on `TermSession`.** A marker *will* land across
   two reads — this is certain at 64 KiB boundaries, not merely possible — so a
   `windows().position()` scan is wrong on the first long transcript. It needs a bounded
   accumulator and a stated drop policy for a marker that never terminates.
2. **Split-feed for the cursor.** A claim's whole value is *"the cursor is here."* `advance` is
   stateful and safely re-entrant across calls (`vte/src/ansi.rs:298-311`), so the console feeds
   `bytes[..marker_end]`, reads `term.grid().cursor.point` (the accessor `term_view::cursor_row`
   already uses, `term_view.rs:253-255`), then feeds the rest. That resolves "here" to an exact
   absolute line at the exact byte.
3. **It must know about synchronized updates.** While a DEC 2026 update is pending, `advance`
   buffers into `advance_sync` (`vte/src/ansi.rs:302-303`) and applies later — so a marker seen
   inside a BSU…ESU block would read a stale cursor. Bounded in practice (sync updates are
   full-screen-app frames, and the alternate screen is out of patch scope), but named here so
   it is not rediscovered.

⚠️ **A `Handler` decorator cannot do this job.** The escape hatch named in `scroll_anchor.rs:138`
is real but narrower than it sounds: an unrecognised OSC reaches only `unhandled()`, which
`debug!`s (`vte/src/ansi.rs:1339-1343`, `:1519`), and DCS `hook`/`put`/`unhook` are `debug!`-only
(`:1311-1326`). **No `Handler` method is invoked for any private sequence.** The single hook that
can carry an arbitrary payload is `set_hyperlink` (OSC 8), which is why §0's middle outcome is a
bigger change than it looks.

---

## §3 — The claim

The coordinate frame is **relative and in cells**. The program cannot know absolute screen
coordinates and must not try; the console resolves "here" from the cursor at the marker byte.

| Field | Req | Meaning |
|---|---|---|
| `verb` | ● | `patch` to claim, `drop` to release. Reserve the names; build both. |
| `id` | ● | **Console-assigned** (see below), echoed by the CLI. |
| `up` | ● | How many lines *above the marker* the rectangle's first row sits. |
| `rows` | ● | Height in cells. |
| `col`, `cols` | ● | The column range, zero-based, left-inclusive. |
| `kind` | ● | What to paint. A name the console resolves — never a command, never a path. |

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

## Tests that must exist

Pure, headless, no GPU, no egui context — the house shape.

- `a_marker_split_across_reads_is_still_recognised` — the certain case, not the edge case.
- `an_unterminated_marker_is_dropped_at_the_bound`
- `a_marker_without_the_session_nonce_is_ignored`
- `the_parser_is_unaffected_by_a_marker` — feed a marker, assert the grid is byte-identical to
  the same stream without it. This is what makes pre-scanning safe.
- `a_claim_resolves_to_the_cursor_at_the_marker_byte` — the split-feed law.
- `duplicate_ids_are_rejected`
- `a_width_change_invalidates_every_patch` — P3, asserted rather than hoped.
- `nothing_in_a_claim_reaches_a_process_spawn` — the I1-shaped guard.

---

## Deferred, and held deliberately

Patches in the alternate screen. Patches a program can update in place (a claim is create-or-
drop; re-emitting is the update path). Z-order between overlapping patches beyond paint order.
Any claim field describing *how* a patch looks — that is layout. Reflow-surviving anchors: the
only per-cell slot that survives rewrapping is `Cell::hyperlink` (`term/cell.rs:219-221`), it
cannot ride on the gap's own spaces (`is_empty` ignores it), and it makes the text a clickable
link in every other terminal. It recovers *where a patch belongs*, never *a hole to put it in* —
which is why P3 drops instead.
