### Organon Console: a command line inside every region, and a panel column per region (#98 Tier C)

Every region that does not hold an `agent` now has its own command line along its bottom edge, and
the region it is typed into is **context**: `/add surface` in a panel column does what
`organon console stack add surface` does, in *that* column, and `/panel`, `/agent`, `/3d` and
`/off` assign the rectangle they are typed in. This is the fifth front door onto the one command
registry (`CONSOLE_ARCHITECTURE.md` §1.8) and emphatically **not a fifth vocabulary** — every line
goes through `Registry::resolve` and lands on the same `ConsoleDispatch` the CLI, the MCP tool and
the composer reach, leaving the same `CommandRun` record and the same *accepted, not applied*
receipt.

🚨 **The hard part was focus, not parsing, and the arbitration is a measurement rather than a
policy.** `conversation_view::composer_keys` consumes Tab, Escape and the arrows out of the **raw
event list** — not out of a focused widget — and two of them unconditionally: `arrow_owner` hands
Up to the history whenever the composer is empty. That was safe while the console had exactly one
command input; a second one would have found its own Up already gone before it ran.
`region_line::Lines::owner` is the region whose line had egui focus **last frame**, recorded from
the `TextEdit`'s own `Response::has_focus` — the same fact `composer_box` already reads to decide
whether Enter sends. Nothing invents a focus state and nothing fights egui's: clicking a box is
what moves focus, egui guarantees at most one focused widget, and this only observes which.
`ConversationPane::set_keys` is the single gate the answer feeds, and it defaults to `true`, so a
console that has never had `/viewport` typed at it behaves exactly as it did.

⚠️ **One frame behind, deliberately, and the bound is what makes it affordable.** The region walk
visits regions in `Region::ALL` order, so a line drawn *after* the agent region cannot tell the
composer anything before the composer has read the frame's keys — both sides therefore gate on the
previous frame's measurement, which is the same one-frame-behind arrangement the `3d` region's
rectangle already uses. The cost is that the frame on which focus *moves* is arbitrated by the old
owner, and that frame carries no keystroke: **focus moves by a click, and a click is not a key.**
⚠️ `Lines::begin` runs every frame whether or not any line draws, which is what hands the keys back
when the last line goes away; a latch cleared only by an explicit blur would leave the composer's
keys held by a rectangle that no longer exists. ⚠️ And a region holding `agent` gets **no** line at
all — that rectangle already has the console's original command line in it, and two inputs in one
rectangle with nothing to tell them apart is the problem made worse rather than solved.

🚨 **Prune discovery, never capability.** The band offers a region's own verbs and nothing else;
`region_line::act` accepts **every** console verb in the table. `/theme dark` typed into a panel
column works, because a palette is a console-wide setting and refusing it there would turn a region
into a jail — and loosening a refusal later is far harder than tightening an offer. The one genuine
refusal is the **view lane**, by name: `/surface`, `/media`, `/organon` and `/help` put something in
*a transcript*, and a column is not one, so the refusal says so and names where the verb does work.
`/help` falls under it too, which is right rather than convenient — this line's own band is its
help.

⚠️ **The pruned surface says what it left out, and the type does not let it be silent.**
`RegionPalette::elsewhere` is a `String`, never an `Option<String>` — `Ring::Empty`'s precedent one
scale up. Typing `/th` in a panel column shows no candidates and the line *"`/theme` belongs to the
console line, not this `left` region, but it runs here anyway"*; a bare `/` shows the region's own
words and *"this list is `left`'s own: it holds `panel`. The console line's verbs run here too:
`/background`, `/rig`, `/theme`, `/posture` +N"*. It is
**generated from the registry**, because a hand-kept list of "the other verbs" is the second
vocabulary §1.8 exists to prevent, arriving from the friendliest possible direction. ⚠️ A second
absence earns its own sentence: `stack`'s optional `region` keyword is a real part of the verb's
grammar and the registry offers it, correctly — but in a line whose premise is that the region is
context, offering the word would invite a second, contradicting one, so it is dropped from the ring
and named (*"`region` is supplied by the line you are typing in: it is `left`"*). ⚠️ **Dropped from
the ring is not dropped from the table**: type it anyway and the typed word wins. The supplied
region fills the slot only when it is empty, so `/add surface region right` in the `left` region's
own line reaches `right` — the same answer the full `/stack add surface region right` gives at that
box. Overwriting it would have edited a column nobody named, silently, which is the one failure this
module is written against; refusing it would have made the pruned surface reject what the whole
table accepts, which is the jail rule 2 exists to prevent. A bare trailing `region` with no value is
still malformed rather than defaulted — `parse_args` answers it before the region gets a chance.

⚠️ **Every sentence this module composes is ASCII, and the guard's boundary is the interesting
half.** A `✓` in none of egui's four bundled fonts shipped once already and was photographed as an
empty box on a running console, so the rule is real rather than stylistic — and writing the band
found six em-dashes that had to go. But the guard stops at strings this module *composes*: a
registry refusal (*"`/nonesuch` is not a command"*) is the **console line's own words**, drawn by
the composer on every build that has ever shipped, and passed through here byte-for-byte. Asserting
a glyph policy over it would put a second copy of that policy in this file, and the copy's first act
would be to refuse a string the composer draws happily two rectangles away. So the pass-through is
pinned as *unmodified* instead — which is also the property that matters: one mistyped verb must be
refused in one set of words whichever rectangle it was typed in, and re-wording it per region is the
second vocabulary arriving as politeness.

📌 **An unassigned region is now self-describing.** It shows its own command line and a hint instead
of apologising: type `/panel` into the empty rectangle and it holds a column. That was the standing
question §1.14 left open, and it falls out of the region being context rather than needing anything
of its own.

**The panel stack is now one column per region**, which reverses what `panel_stack.rs`'s header and
§1.14 said through Tiers A and B. Both of the reasons they gave are answered rather than overruled.
The **mechanical** one — *"the add verb has two rings and no room for a region word, so a per-region
stack would give every region after the first a column nothing could ever fill"* — is dissolved by
the command line supplying the word. The **architectural** one — *"two panel regions are two views of
one instrument"*, on `OrganonPanels`' one-mirror-per-console precedent — 🚨 **conflated two objects.**
The *mirror* (the `PresetValues` a control writes, which is what reaches `Shared`) stays one per
console and must: two mirrors would be two claims about one instrument's state. The *composition* —
which panels a column lists — is a property of the **column**, and nothing ever argued that two
columns must list the same panels. Tier A could not tell them apart because there was only one
column. 📌 It took a running console to see it: on 2026-08-20 James built the four-region layout
(`left` panel · `topcenter` 3d · `right` panel · `bottomcenter` agent), typed one
`stack add surface`, and both side columns rendered an identical `organon · look · Surface`.

🚨 **`console.stack` therefore grows a third, OPTIONAL `region` argument, and every part of that
sentence is load-bearing.** Optional, because three of the four older front doors have no region to
be typed into — a CLI line, an agent's tool call and a `/organon` typed at a conversation all fall
to `panel_stack::Home`, the first `panel` region in `Region::ALL` order, exactly as they did before
the word existed. A **keyword** rather than a bare third word, because the slash grammar fills
optional arguments by name, and a bare word would make `/stack add surface left` and
`stack add surface --region left` two spellings of one verb — the drift this tree spends most of its
refusals preventing. And absent, `console_op_to_line` writes the byte-identical line it always
wrote, so a sidecar line from an older build still means what it meant. ⚠️ Half of it is malformed
rather than defaulted: `stack add surface region` with nothing after it is **skipped**, because the
caller did name a column and the line lost the answer — defaulting there would edit whichever column
the destination rule picked, which is precisely the one they did not name. A named region that does
not hold `panel` is refused by name and never redirected.

🚨 **The new `region` slot carries #109's short forms, and it did not get them by rebasing
cleanly.** This branch was written before *region abbreviations* landed and merged onto it with
**no textual conflict at all**, which is the misleading part: `git` had nothing to complain about
while the two features had genuinely not been composed. Two things were wrong afterwards and
neither was a merge marker. The new slot was a plain `ArgKind::Choice`, so `viewport tl panel`
worked and `stack add surface --region tl` did **not** — one region vocabulary answering to its
initials in one slot and refusing them in the next, which reads as a typo rather than as a
divergence. And a comment in `console_main.rs` still said *"the one `ChoiceAliased` in the
catalog"*, true when written and quietly false the moment this branch added the second. ⚠️ The
count is not restated anywhere now: `region_slots_all_accept_the_short_forms` walks the catalog and
asserts that **every** slot named `region` carries `REGION_ALIASES` — and that none of the others
does — so a third region slot added tomorrow either composes or fails, with no edit here either
way. The CLI door needed no change at all, because `--region` reuses `region_words()` rather than
restating the table; the test that `tl` arrives **normalised to `topleft`** is the one worth
having, since the wire deliberately passes an unknown region word through for the console to refuse
by name, and a leaked alias would surface as a refusal quoting a word nobody typed.

⚠️ **The third thing the clean rebase hid was a `match` that stopped being exhaustive.**
`every_console_verb_still_runs_in_a_region_line` builds the shortest satisfying line for every verb
in the catalog by matching on `ArgKind`, and `ChoiceAliased` did not exist when it was written. It
fails to compile rather than misbehaving — which is exactly what `command.rs`'s own note asks for
(*"a wildcard is how `ChoiceAliased` gets skipped"*), and is why that match still has no `_` arm: a
wildcard would have fed `viewport` the string `"x"` in place of a region and gone green.
⚠️ **None of the four cheap legs catches it**; it needs `cargo check --tests`, which is why that
leg is in the bar.

⚠️ **A column belongs to the region, not to what the region currently holds.** `Stacks` is an array
over the whole region vocabulary indexed by `Region::slot` — `Layout`'s own arrangement, for its
reason — so `viewport left off` followed by `viewport left panel` finds the column where it was
left. Only `stack remove` empties one.

⚠️ **A restored layout now comes back with two empty columns** where it used to come back with two
views of one empty column. Nothing regressed — §1.15 has always recorded region→content and never
stack contents, deliberately — but the cost of that gap is higher than it was. It is written down
in §1.15 rather than fixed here, because what a layout would have to carry is a list of slugs per
region, which is content rather than arrangement, and the first question it raises (does loading
replace a column somebody assembled, or refuse?) is `set_layout`'s transaction argument again at a
smaller scale.

⚠️ **The region line deliberately has no self-completion and no autorun.** Both are §1.9 rules of
the composer's and both rest on `completion_held`, the latch that keeps a completion from undoing a
backspace on the frame it happens — read off a shadow copy taken at the top of the frame.
Reproducing them without that measurement would reintroduce the worst defect the command panel has
had (*"once I have typed slash surface, I am no longer able to backspace out of it"*). **Tab
accepts, Enter runs**, and every candidate answers `fires: false`, which is the honest reading of a
switch that is off rather than a value copied from the entry. There is no history either: the
composer's recall buffer exists because that box is also where prose is written.

📌 **`compact_join` was split out of `compact_line`** so the region line's band gets the composer's
own fitting rule, separator and `+N` count rather than a second one beside it — two producers of
words, one row.

🚨 **Nobody has typed a character into a region command line.** Every claim above about the
*vocabulary* is measured headless; not one about the *surface* is. No band has been drawn on a
screen, no box clicked, and the arbitration has only ever been exercised as a value. The focus
story is the part most likely to be subtly wrong and the part a green suite says least about — if
some route moves focus *with* a keystroke in the same frame, that frame goes to the wrong reader,
and the symptom is one lost Tab rather than anything that looks broken. Nor has anyone assembled
two genuinely different columns and used them, which is the thing the per-region change is for.
