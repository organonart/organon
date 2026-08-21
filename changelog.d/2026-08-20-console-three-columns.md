### Organon Console: the pane divides into three columns, and `left` / `right` now mean the outer column

The region grid was four quadrants over 2×2, so `topcenter` was not a missing word — it was a
missing **rectangle**. `region.rs` said so in its own header: *"two regions may coexist iff their
quadrant sets are disjoint, which is the whole of the geometry: there is no layout arithmetic to
get wrong, only a bitmask."* Four bits, one `mid_x`, one `mid_y`; halves only. That made James's
own editor layout — two scrolling control columns flanking the instrument, with the agent beneath
it — unspellable, whatever words you added.

**Four cells become six**, three columns by two rows, and every property survives verbatim: a
region is a set of cells, coexistence is disjointness, refusals name the obstacle,
`Content::only_one_because` still attributes the one-live-`3d` limit to Organon rather than to
viewports, the last `agent` cannot be evicted, and a console nobody has typed `/viewport` at is
unchanged. `region_rect` grew two vertical cuts; nothing else in the model moved.

🚨 **The twelve words are a cross product, not a list.** A region has to be an axis-aligned run of
cells or there is no rectangle to return, and over a 3×2 grid there are eighteen such runs — six
column-spans times three row-spans. The discriminator is the module's own rule that a region is a
word a person says: the column-spans English names are *left*, *center*, *right* and *all three*,
the row-spans are *top*, *bottom* and *both*, and four times three is the vocabulary exactly —
`full`, `top`, `bottom`, `left`, `center`, `right`, and the six cells. The six excluded runs are
the two-column ones; naming them (`leftcenter`?) would mint a word nobody says to complete a
table, and nothing breaks by leaving them out — `plan`'s vacancy walk reports a two-column gap as
two vacant regions, exactly as it already reported a three-quarter gap as two.

🚨 **The side columns are a FIXED WIDTH and the centre takes the remainder — not equal thirds.**
James's call, and it stands on what this tree already does: Organon's own editor sizes control
columns absolutely and lets the subject absorb the rest — `SidePanel::right`'s
`default_width(320.0)` for the theme dock, `exact_width(150.0)` for the presets rail, and
`mind_shell::DockSizes::default()`'s `left: 260.0, right: 300.0` beside a viewport that takes
what is left. Equal thirds would pin the instrument to a third of the window whatever the window
is, which is not what anyone wants to look at. `SIDE_COLUMN` is **320 points**, the widest fixed
control column in the tree, chosen so a panel that fits Organon's own side dock fits a console
`panel` region without the region being what decides. On a 1100-point pane that is 320 · 460 · 320.

⚠️ **`left` and `right` therefore mean something new, and that is the intended change rather than
a regression.** They were **half** the pane. They are now the **outer column** of three, at a
fixed width — so anyone with muscle memory gets a narrower column than they got yesterday, and
`/viewport left panel` does not look the way it looked before. It is said out loud here, in
`region.rs`'s header and in `CONSOLE_ARCHITECTURE.md` §1.14, because a word quietly meaning
something new is exactly the drift this axis spends its refusals preventing.

⚠️ **The narrow-pane rule: the columns vanish and the rows survive.** A fixed side means a pane
can be too narrow to seat two of them — below `2 × SIDE_COLUMN + MIN_SIDE` = **688 points** the
two cuts would cross and `left` and `right` would overlap in the middle. The rule is decided
rather than discovered: **the side columns keep their width or there are no columns.** A region
that needs a cut — anything not spanning all three columns — returns `None`; a region that spans
all three (`full`, `top`, `bottom`) needs no cut and is unaffected. So a narrow console still
splits into rows, every column word refuses until the window is wide enough, and `plan` answers
`None` for a column layout so the seam can say the window is too small for it and name the command
that undoes it. That is actionable, where a twenty-point `left` is a sliver somebody has to guess
about. The boundary is pinned from both sides: at exactly 688 the centre is `MIN_SIDE` wide and
stands, one point under and every column word refuses.

📌 **The rejected rule was "the sides shrink"**, and `mind_shell::layout_workstation` is the
precedent for it — its docks yield proportionally so the viewport survives. Right there and wrong
here: those docks are chrome around a subject, while every region here is somebody's assigned
content and none of them outranks the others. Shrinking would also make `left` mean a different
width at different window sizes with no word to explain it, and a side column narrowed to
`MIN_SIDE` can no longer hold what it exists to hold — which is the console's "refused, not
clamped" rule arriving as geometry.

Two supporting edits worth naming. `Region::quadrants` is now `Region::cells`, because a quadrant
is a quarter and six of them is a sentence that cannot be true. And `Layout`'s array size is read
off `Region::ALL` as `REGION_COUNT` rather than written as a literal in three places — the array
is indexed by position in that list, so the only way to guarantee they agree is for one to be the
other.

🚨 **Green does not say the layout is any good, and this tier is most convincing exactly where it
is least verified.** A fixed width produces a clean, confident, reproducible rectangle whether or
not the rectangle is the right size. Nobody has typed `/viewport topcenter 3d` on a running
console; whether 320 points reads as Organon's editor beside a 460-point instrument, whether a
panel stack is legible at that width, and whether the instrument still looks like the subject are
questions a hand and a screen answer. `CONSOLE_ARCHITECTURE.md` §3's ledger carries them.
