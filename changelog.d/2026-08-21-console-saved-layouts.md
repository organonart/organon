### Organon Console: an arrangement of the pane has a name now, and loading one is a transaction

`organon console viewport` and `organon console stack` build an arrangement; nothing wrote one
down. `organon console layout <save|load|delete> <name>` — `/layout save desk` in a composer —
records the whole pane, every region and what each one holds, under a name, and brings it back.
The listing is `/layout.list`. The library is `layouts.json` at the console's store root, beside
`harnesses.json` and `preferences.json`.

🚨 **A layout is not a convenience, which is why this was worth promoting out of #98's deferral
order.** `doc/organon_is_the_product.md` §4 is James's reframe: a layout is **the unit of product
identity**. "Claude Code Desktop", "Organon standalone" and "an LLM visualiser" become *named
arrangements of one program* rather than three programs. That is a different weight from the one
#98 deferred — and the work needed none of the tiers it was deferred behind, because it records
whatever arrangement exists and derives every word from `Region::ALL`. ⚠️ Nothing here touches
`Edition`; §4 is a proposal, not a ratified change, and no edition is being collapsed.

🚨 **`load` is a transaction, and that constraint decided the whole design.** §4, verbatim: *"a
layout that cannot be drawn must say so and leave the current one standing, never half-apply."* A
saved arrangement arrives **all at once, from a file, possibly written by someone else** — so
`layout::resolve` checks the whole thing and answers **either one finished `region::Layout` or one
sentence**, and `Console::set_layout` applies it in a *single assignment*. There is no loop over
placements and there must never be one: a partial apply that had evicted the last `agent` region
is a console with nothing to type into, recoverable only by a verb typed at an agent. The property
is pinned as the **signature** rather than as discipline — a refused load returns no layout at
all, so there is nothing partial for a caller to take a piece of.

⚠️ **A layout naming something this build does not have is refused whole, never loaded in part.**
A region word that was renamed, a content kind that was removed, `off` stored as what a region
holds — each earns a sentence naming what is missing. Loading the rest would hand somebody a
different arrangement from the one they saved, and nothing on screen would say so. Beside those,
a saved layout meets every refusal a typed one meets: two regions whose cell sets intersect,
two regions holding `3d` (refused with `Content::only_one_because`'s reason, so it still says the
limit is *Organon's* rather than the idea of a viewport's), and nothing holding an `agent`.

🚨 **The rules live in `region.rs`, in a new whole-layout constructor, because a set of placements
has no order.** `Layout::from_placements` is `Layout::assign`'s counterpart: `assign` answers *one
command meeting a layout that already exists* and resolves containment by **displacing** — which
is what makes `viewport left agent` work from a console holding `full`. A file has no "asked", so
`full` beside `left` in one file is a contradiction rather than a move, and reusing `assign`'s
refusal type would have meant inventing an `asked` region out of iteration order. Disjointness is
still `Region::cells`, uniqueness still `Content::only_one_because`, the agent rule still
`Layout::has_agent`: one implementation, two doors.

⚠️ **The window's size is the refusal that is about the window rather than the file**, so it is
checked only when a pane has been measured, names the pane that refused it, and says the layout is
fine and the window is not. The same file loads once the window is bigger.

🚨 **And it is TWO rules, which is a should-fix this tier earned in review.** `plan` says no
either because a region falls under `MIN_SIDE` **or** — since #98 Tier B — because the pane is
narrower than `MIN_COLUMNS_WIDTH` (688 pt) and the layout uses a column word, whatever room its
rectangles would otherwise have had. Quoting only the first meant a three-column arrangement
against a 500-point pane was told *"every region needs 48 on a side"*: true, irrelevant, and
misleading about what to do. The coarser rule is asked first now and the sentence names the
threshold that actually tripped, with the region that needs the cut and a pointer at rows, which
need none. The predicate is `Region::needs_column_cut`, **extracted out of `region_rect`** rather
than re-derived from the cell mask — one implementation, so the explanation and the geometry
cannot come to disagree.

⚠️ **And the edge case that split could have missed is pinned as a counterfactual rather than
argued.** `plan` measures the *vacant* regions it fills in as well as the occupied ones, while the
refusal inspects only what the layout holds — so a column-shaped **gap** would fail the plan with
no occupied region needing a cut, and the sentence would fall through to the wrong rule. It is
unreachable on today's grid, and Tier B is the proof that the grid moves, so the test asserts the
thing that matters instead: whenever the `MIN_SIDE` refusal is the answer, **widening the pane
must not rescue the layout** — because if it did, the width was the real reason. Reverting the
split makes it fail and name the case.

🚨 **Three actions, and the listing is a verb of its own — the `/stack` lesson applied before it
could bite.** `registry::parse_args` fills *required* arguments positionally and *optional* ones
by keyword, so a verb with an optional name would be typed `/layout save name mine` while the CLI
stayed `console layout save mine` — one verb, two spellings. Both words are therefore required,
which settles `save`/`load`/`delete` and rules `list` out: a name is not a thing a listing takes,
and there is no honest word for that slot (`all` works in the panel ring because it genuinely
names a value *there*; no layout name means "every layout"). So the listing is
**`console.layout.list`**, on `console.camera.read`'s precedent — a read is answered in-process,
because `organon console …` is fire-and-forget with no return path. ⚠️ It differs from the camera
read in one way, stated rather than glossed: the library is a *file*, so a CLI could answer this
one out of the same file. It does not today, because the CLI has no spelling for a dotted verb and
`console layouts` would be a second name for one verb. `layouts.json` is legible by design.

⚠️ **`Reversal::Permanent`, and each action earns it separately.** `delete` takes a layout out of
a file and nothing puts it back; `save` replaces what was stored under that name and nothing
rebuilds it. `load` is the one worth arguing: it puts nothing in the transcript, which is
`viewport`'s whole case for `Recoverable` — but what it *displaces* is the arrangement on screen,
and no second command restores that unless it too was saved. `/viewport full agent` returns to the
**default**, not to what you had. "Only if you had already saved it" is not the same as yes, so
autorun can never fire the verb, which is the right outcome for one that writes to a file.

🚨 **A name with whitespace is refused because the wire cannot carry it** — the one gate here that
is a fact about the transport rather than about taste. The sidecar line is whitespace-delimited,
so `layout save my desk` would arrive as `layout save my` with the rest silently dropped, having
saved something nobody named; the test that pins the parser measures exactly that truncation.
`check_name` refuses it at the clap boundary (where a human reads the error) and again at dispatch
(where a hand-written sidecar line meets it). Names match **exactly**: `Desk` and `desk` are two
layouts, and the refusal for an unknown name lists every name that exists, which makes a case
mismatch visible rather than silently folded.

📌 **The file follows `harness.rs`'s discipline and `prefs.rs`'s writing.** Built-ins seeded in
code, a user's file merged over them by name, serde defaults, unknown fields tolerated — and
because unlike `harnesses.json` this file is one the product *writes*: an atomic temp-then-rename,
a totally forgiving read (a corrupt library costs you your layouts, never your console), never a
BOM, and 🚨 **unknown fields kept across a rewrite rather than merely tolerated on read**. That
last one is stronger on purpose: `save` and `delete` rewrite the file whole, so tolerate-and-drop
would let a console one version behind silently strip a newer one's fields off every layout in the
library — including the ones it never touched.

📌 **The library ships EMPTY, and that is the scope rather than an omission.** Naming the presets
— "desktop", "standalone", "mind" — is James's call, and a preset nobody has looked at on a screen
is worse than none: it would be a shape the product asserts is good, arrived at by a machine that
cannot see it. The seam is built and tested anyway — `merge_over` and `save_over` both take the
built-ins as a parameter, so the merge *and* the rule that an unchanged built-in is never written
into a user's file are exercised against a seeded list rather than against nothing.

⚠️ **A layout records the arrangement and nothing else** — not the panel stack (documented as not
remembered across a launch; changing that is a decision about the stack), not the theme, posture,
screen state, tabs or camera.

📌 **Default-inert.** A console nobody has typed `/layout` at reads no file, writes no file and
runs the identical code it did before.

✏️ **#98 Tier B landed under this while it was in review, and nothing here had to change for it.**
The grid went from four quadrant bits to six cells and `left`/`right` came to mean the outer
*column* rather than the half — and a layout records whatever arrangement exists, deriving every
word from `Region::ALL`, so the only edits were a renamed accessor (`quadrants` → `cells`, one
call site) and one test's example word. That test used `topcenter` as its stand-in for *"a region
a newer build has and this one does not"*, which Tier B made real: it now uses `middleleft`, and
gained the other half of the story — a file naming `topcenter` **loads**, because this build has
the word. So both directions of the forward-compatibility claim are measured rather than argued,
by an event nobody staged.

⚠️ **What a green build does not prove.** No layout has ever been saved, loaded or deleted by a
running console — this was built on a Linux container with no GPU and no window. Whether a load
looks instantaneous or whether the pane visibly re-lays-out under the eye is *inferred* from
§1.7's re-wrap measurement, never observed; whether the arrangement that comes back is the one
somebody meant is a question about `region::Layout` being a faithful description of what a person
sees, which no serialization test can answer; and the receipt lines go to `stderr`, on the console
lane's existing route, which in a GUI launched from Explorer reaches nobody. Those are James's
calls and a hand's, and no amount of green answers any of them.
