### Organon Console: every region word answers to its initials — `/viewport tl panel`

`topleft`, `bottomcenter`, `bottomright`. Twelve region words, three of them twelve characters
long, and the verb that uses them is one a person types while rearranging a window they are
looking at. They now also answer to their **initials**: `f t b l c r` for
`full`/`top`/`bottom`/`left`/`center`/`right`, and `tl tc tr bl bc br` for the six cells.
`console viewport tl panel` is `console viewport topleft panel`, exactly.

🚨 **At all four front doors, because a vocabulary that exists for one caller and not another is
the defect `registry.rs` was built to prevent.** A region word is checked in three independent
places before it ever reaches `Region::resolve` — the composer and the MCP schema share an
`ArgKind` in `console_main.rs`, the CLI has its own `PossibleValuesParser` in `bin/ctl.rs`, and
tab completion reads the first of those — so a composer-only abbreviation would have been a
second vocabulary wearing the first one's clothes. All four accept the short forms; the palette,
`--help`, the MCP `enum` and every refusal still list twelve words.

🚨 **`REGION_ALIASES` is a declared table, not a rule the code applies.** It sits beside
`REGION_WORDS` in `region.rs`, one pair per word, and it is the only copy — `bin/ctl.rs` turns it
into clap aliases, the console's schema into an `ArgKind::ChoiceAliased`, `Region::resolve` into a
lookup, and `UnknownWord` into a sentence. Computing "initials of the parts" instead would be an
algorithm nobody could contradict: a future region word whose initials collided with an existing
short form would silently shadow it, and one whose natural abbreviation is not its initials would
have nowhere to say so. The rule is enforced *as a test over the real table* rather than as the
implementation — one test derives each compound word's parts from the grid (a cell word is a row
word followed by a column word) and asserts its short form is theirs joined, another asserts no
alias equals a region word and no two aliases are the same string.

⚠️ **A declared short form is a second exact word, never a prefix rule.** `l` resolves and `le`,
`lef` and `L` still refuse, which is the no-approximation rule `Region::resolve` has always kept —
an approximation would rearrange a window somebody is looking at into a shape they did not name.
The short form is rewritten to its long word *before* the search, so there is one matching rule
rather than two and an abbreviation cannot come to resolve to something the long word does not.

⚠️ **The word travels as TYPED and is expanded once, at the console.** clap's
`PossibleValuesParser` returns the string it matched rather than the canonical name, so
`console viewport tl panel` puts `tl` on the sidecar line; the composer passes the typed word
through for the same reason. Expanding at one door and not the other would make one command read
two ways in the session log depending on where it came from. `Region::as_word` is untouched —
nothing is ever *written* short, so a saved layout, a refusal and a displacement notice all still
say `topleft`.

📌 **A new `ArgKind` variant rather than a richer `Choice`.** Counted rather than estimated:
`ArgKind::Choice` appears **43 times** across the tree, **21** of them constructions, and
`ArgSpec { … }` **50 times** — so widening `Choice`'s payload or adding a field to `ArgSpec`
would drag every vocabulary in the console into a change exactly one of them asked for,
including the ones deliberately left alone here (content words, stack actions, panels, screen,
posture, patch kinds, the verbs). `ArgKind::ChoiceAliased { words, aliases }` is additive: one
construction converts and the other twenty do not, every existing `Choice` is
untouched and inert, one argument opts in, and because `ArgKind` is matched exhaustively
everywhere that reads it the compiler named each renderer that had to learn the new arm rather
than leaving one to be discovered on a running console.

⚠️ **An abbreviation nobody can discover is a secret, so three surfaces say it out loud.** The
completion palette shows each region with its short form in the slot that already exists for one
line about a word, so you learn `tl` by looking at `topleft` in the band you were already reading.
`/help` and every refusal carry one shared clause — *"each has a short form: `full` is `f`,
`bottomright` is `br`"* — built from the table's first and last pairs rather than written out, so
a renamed word takes the sentence with it, and two examples rather than twenty-four because the
rule is legible from one short word and one compound. The MCP tool's schema keeps the twelve-word
`enum` and puts the same sentence in its `description`: `enum` is what a model is told to choose
from, and twelve more entries there would present a twelve-shape vocabulary as a twenty-four-word
one.

📌 **The content words deliberately get none.** Three kinds, none compound, and `3d` is already
two characters — a short form there would be a second spelling with nothing to buy. Same for the
stack actions, the panels, the postures and the patch kinds: this is the region vocabulary only,
which is where the length actually costs something.
