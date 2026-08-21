### Organon Console: the region command line becomes a panel column's add/remove control — and starts accepting input (#98 Tier C, narrowed)

#98 Tier C shipped a command line in **every** region, dispatching the **whole** console registry.
James used it and rejected the scope: *"I don't want every region to have commands like this. At
least that wasn't my original intention. … What I envisioned is not that I want to be able to have
slash commands to set each region to be what it could be. I only particularly wanted to be able to
add and remove panels from a panel section."* So it is now a **two-word control in `panel` regions
only** — `add <panel>` and `remove <panel>`, with `remove all` emptying the column, the words and
the panel names both coming from `panel_stack`'s own tables. A region holding `agent` or `3d`, and
a region holding nothing, gets no line at all.

🚨 **This reverses a rule the tier was built on, and the reversal is written down rather than
quietly applied.** `region_line.rs`'s header, `CONSOLE_ARCHITECTURE.md` §1.14 and the previous
changelog fragment all argued **prune discovery, never capability** — the band listed a region's
own verbs and `act` accepted every verb in the catalog, so `/theme dark` typed into a panel column
ran, with a test and a review round defending exactly that. **It no longer runs, by design.** The
reconciling argument: that rule guards a **general command surface** against becoming a jail — a
surface that offers ten verbs and secretly refuses the eleventh has taught you its list is a lie.
This is no longer a general command surface. It is a **dedicated control for one job**, the way a
scrollbar is not a crippled command line and a volume knob is not a pruned synthesiser; a control
that does one thing is not a pruned version of a thing that does everything, so the rule does not
reach it. There is no hidden capability to discover, and the list **is** the vocabulary.

⚠️ **What was given up, said plainly.** `/theme dark`, `/posture`, `/background` and every other
console verb no longer run in a region; assigning a region is `/viewport <region> panel` at an
agent or `organon console viewport …` from a terminal, which is what it was before Tier C. An
unknown first word is refused **by name** and says what the control does take and where the rest of
the vocabulary lives — nothing was made unreachable, only un-typeable there. The `Shed` table,
`Context::content`, the `RegionPalette::elsewhere` sentence, the view-lane refusal and the
`dismissed` latch went with it. ⚠️ **Escape is now deliberately left alone**, so egui's `TextEdit`
does what it does everywhere else with it — surrender focus, handing the composer its keys back.

🚨 **The shipped line did not accept input at all, and the cause was `egui::TextEdit`'s id.** James:
*"When I type slash in one of the regions, I just get a list of choices and some text below it, and
I can't type or select anything."* The box was added with no explicit id, so egui derived one from
`Ui::next_auto_id` — a counter over the widgets allocated before it. The palette's rows are drawn
*above* the box, so the instant a `/` opened the band two more labels existed ahead of it, the box's
id changed, egui saw a **different widget**, and focus was stranded on an id nothing drew any more.
The first keystroke landing and none of the rest is the signature of that. `BOX_ID` is the fix: an
explicit salt, so the box is the same widget whatever is drawn around it.

⚠️ **A second, independent defect was measured on the way, and stopping at the first would have left
it.** In a 320 pt side column the old band's `elsewhere` sentence wrapped to several rows and pushed
the box **35.6 pt past the band's own clip rect** (content `529.5 → 635.6` in a band ending at
`600.0`) — invisible and unclickable even with a stable id. **The row order is therefore a safety
property rather than a taste: candidate row, box, note.** The candidate row is `compact_join`, which
fits itself to the available columns and is exactly one row, always; the note is the only unbounded
thing in the band, so it goes last and a long refusal clips its own tail instead of the input. Put
the note first and a refusal hides the box that would let you correct it.

⚠️ **The mutation table is in the test's own doc, and it corrects the obvious assumption.** Deleting
`BOX_ID` does **not** fail the suite on today's code, because the narrowing made the candidate row
unconditional and the widget count above the box stopped varying; only *"salt deleted **and** note
moved above the box"* fails. Both fixes are kept because each covers the other's gap.

**Now driven headless.** `typing_survives_the_palette_opening`,
`a_note_appearing_does_not_take_the_box_with_it` and `tab_takes_the_highlighted_candidate` push real
`egui::Event::Text` and `Event::Key` through `region_line::draw` inside a real band rect at a real
320 pt column width. ⚠️ **The general lesson, which cost this tier a round trip**: a headless suite
that never drives the framework's own input path can be entirely green about a widget nobody can
type into — and these tests were cheap and available the whole time.

⚠️ **Kept, because it is what was asked for**: the per-region panel stacks from #112. `left` and
`right` hold different columns, and `add surface` typed in a column names *that* column. The word
`panel_stack::REGION_ARG` is supplied and never offered — but never forced either, so
`add surface region right` typed in the `left` column still reaches `right`.

📌 **Also**: an empty panel column's notice drops the slash (*"type `add surface` in the line
below"*), and a vacant region's notice goes back to naming the composer and the CLI, because it no
longer has a line of its own to point at — its two arms collapse into one.

🚨 **Still unverified without a screen**, and it is the same gap the last fragment named: nobody has
clicked into the box with a mouse, nobody has clicked from the composer into a column's line and
back, and nobody has judged whether a control showing two words at all times reads as a control or
as clutter. What is new is that "a character reaches the box" is no longer among the unverified.
