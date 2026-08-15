### The console's one pane divides into regions, and each region holds one kind of thing

`organon console viewport <region> <content>` — `/viewport` from a conversation composer — splits
the console window into up to four rectangles: four ways, two and two, or one beside two. The
vocabulary is nine words over a 2×2 grid (`full`, the four halves, the four quarters) and a
region holds one thing and **never splits again**. `off` empties one.

**This is a fourth axis, not a posture.** James's first framing folded it into `console posture`
and he changed his mind to this explicitly, and the argument is the one `console screen` already
had to make: `Posture` is a *scalar*, so there is no slot to add a third to, and every one of
`Form`'s tokens is a margin, a corner, a padding or a line height — **a split changes none of
them**, it changes how many rectangles there are. A split terminal-posture console and a split
desktop-posture one are both real, so the two axes compose and each verb means exactly what it
says in every combination.

**Flat rather than nested, and the reason is the vocabulary rather than the geometry.** A tree is
the obvious model and it has no *names*: `/viewport left agent` is a sentence a person says and
an agent writes, whereas the same intent in a tree is a path through splits that must already
exist — and the console lane is fire-and-forget with no return path, so a caller cannot ask what
the tree currently looks like in order to describe a place in it. Nine fixed words are
addressable from a line that gets no answer, which is the only transport this verb has. What that
costs is stated rather than hidden: no thirds, no uneven splits, no dragging a divider.

Overlap is a bitmask and nothing else. Every region is a set of the four quadrants; two may be
held at once iff their sets are disjoint. When an assignment meets something already held, the
answer is decided by containment — disjoint regions both stand, a region that *contains* the one
being asked for (or is contained by it) gives up its place and the displacement is **reported**,
and a **partial** overlap is refused by name quoting both regions. ⚠️ **That containment arm is
the one place this does something rather than refuse it, and it is not a convenience**: the
console opens holding `full`, so a rule that refused every overlap would refuse the first word of
every split, and `full off` cannot be the way out because it is refused by the last-agent rule.
Measured in the module's own tests — without it, no split is reachable at all. It is safe where a
partial overlap is not, because containment has exactly one reading. So `left` and `topleft` can
never both be held, and a test walks every ordered pair of assignments to prove no two held
regions overlap by any route.

🚨 **The last `agent` region cannot be evicted.** A console with no agent region is a window with
nothing to talk to, and the way back is not obvious from inside it because the verb that would
fix it is typed *at* an agent. `full off`, `full panel` and `left panel` from a default console
are the same eviction under three names, and one invariant on the **resulting** layout — checked
once — closes all three. A per-verb special case is how the second route comes to be the one
nobody remembered. Clearing a region that already holds nothing is refused too: a command that
changes nothing and says nothing is indistinguishable from one that never arrived.

**Unassigned space is a sentence, never a blank.** The plan carries every vacant region as well
as every held one, so a quarter of the window nobody has filled says what it is and how to fill
it — `Ring::Empty`'s argument at the scale of a window pane. Vacancy is coalesced largest-first,
so a layout holding only `left` reports one vacant `right` rather than two vacant corners, and
the word in the notice is the word a person would type. A pane too small for the layout yields no
plan at all and says so across the whole pane with the command that undoes it, rather than
drawing slivers.

🚨 **A console that has had no `/viewport` typed runs the identical code, not merely equivalent
code.** The frame compares the layout against the value the console is constructed with and, on a
match, draws through exactly the pre-region path: no child `Ui`, no id salt, no clip rect, no
separator. `region_rect(pane, full)` returns the pane bit for bit, so nothing about that rests on
a float comparison. The backdrop is still rendered once at the whole pane's size and every region
is drawn over the same picture — which is also why a `viewport` op folds into no look and opens no
Tier-4 epoch — and the portal still floats over the whole pane. The layout is deliberately **not**
stored: a console opens undivided however it was left, so a saved layout can never make a launch
look broken with no command having been typed.

⚠️ **Only `agent` draws anything live in this tier.** `panel` is a **named placeholder** — the
region says an Organon editor panel belongs there and that a later tier gives it a body — and
`3d` and `media` are absent from the vocabulary entirely rather than present and inert, because a
word an agent can type and nothing can honour is worse than a word refused with the list that
would have worked. ⚠️ Only one region can show the live tab, and that is **structural rather than
a policy**: `conversation_view::draw` takes the pane `&mut`, so a second live copy of one tab is
not something the seam declines to draw, it is something it cannot express. A second `agent`
region says so and names what would fix it.

🚨 **None of it has been seen split.** The model, the rectangle arithmetic and the command lane
are green; whether a half-height conversation is any good, whether the hairline separators read
as separators at this display's scaling, and whether the glyph grid lays out correctly in a
clipped child `Ui` rather than merely compiling, are all unobserved. That is the question the
tier exists to let a hand answer, and it is unanswered.
