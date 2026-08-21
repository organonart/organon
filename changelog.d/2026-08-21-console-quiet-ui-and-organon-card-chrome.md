### Organon Console: quiet by default, one line per panel column, and the panel stack draws Organon's own cards

James, 2026-08-20: *"What I want to focus on is massively cleaning up the UI. … I don't
want to see any of the things presently visible in the status panel. My working model here
is Claude Desktop. … **Consider you are building this for me, not for some unknown user.**"*
Three tiers, one governing principle: much of the console's chrome existed to explain the
console to a stranger, and James is not a stranger.

**Tier 1 — the conversation is quiet, and `/trace on` turns the narration back on.** The pane
printed a receipt for every command (`ok /viewport center agent —
{"accepted":"viewport center agent"}`), plus `no messages yet — type below and press Enter…`
and `working directory C:\Users\james (where the console started …)`. The rule that replaced
all of it, and the only rule:

> **A refusal is always seen. An acceptance is seen only under `/trace on`.**

`conversation_view::Remark { text, always }` carries it per line and `Remark::seen(tracing)`
is the one predicate, used by both surfaces that draw the log — the head of the scrollback
and the status band's slot — because a band saying something the scrollback above it is
hiding reads as a bug in whichever one you distrust. 🚨 **The default stays the loud one on
purpose:** `ConversationPane::note` keeps its signature and its meaning, so a line written by
somebody who did not think about this is **seen**; `ConversationPane::trace` is the opt-in for
the quiet half. A surface whose default is silence eventually swallows the one message that
mattered.

⚠️ **The test for the quiet half is not "is this routine".** It is whether the thing the line
describes is visible some *other* way. A console command's acceptance is — the layout moves,
the panel appears, the palette repaints. A refusal is not, and neither is `⚠ no .claude/,
CLAUDE.md or .git here — this agent starts with no project skills`, so `harness::cwd_notes`
now answers a `CwdNote` per line and only the **resolution** is traced. stderr keeps both
either way, which is what stops the quiet default from costing a diagnostic rather than a
distraction. A **successful** receipt band over the composer is likewise held-but-not-drawn:
`pane.receipt` is still set and still ages, so `/trace on` mid-receipt shows one already in
hand.

⚠️ **`/trace` is a view-lane verb, so it is per TAB rather than per console.** Everything it
un-hides is one pane narrating itself; a console-lane spelling would put a sidecar line and an
MCP tool in an agent's catalog for a preference about how loudly the console talks to the
person in front of it. Two conversations can therefore disagree about it — the honest
consequence of that scope rather than an oversight. `on` and `off`, no toggle
(`console.screen`'s rule). `ORGANON_TRACE=1` opens every tab tracing. ⚠️ One vocabulary cost,
recorded because it is a fact about the table rather than a bug: **`/t` no longer settles** —
`theme` and `trace` share the letter, so the completion cascade needs `/th`.

**Tier 2 — a panel column's add/remove control is literally one line.** `region_line`'s band
drew a candidate row, the box, and two rows for a note. James: *"completely remove the status
lines in the add remove inputs on the panels. It should just be a single line. If we need to
pop something up over top of it, we can do that, but it's way too messy."* `BAND_ROWS` is 1;
the candidate row and the refusal became `region_line::popover`, a floating `egui::Area`
anchored `LEFT_BOTTOM` to the box's own top-left so it grows **upwards over** the column
instead of out of it. `region_line::overlay` is the pure rule for what it says, and it carries
one asymmetry: **candidates only while the line has focus, a refusal whatever the focus is
doing.** A success is discoverable; a refusal is news, and the person who has clicked away is
exactly the one who would otherwise never learn it. The hint text is now `> add | remove`
rather than a sentence describing the control. The third row went too: `console_main` writes a
region-line receipt back only when `receipt.ok` is false.

🚨 **#112's clip guarantee was re-established, not inherited.** That fix was the *row order* —
the only unbounded row last, so an overflow costs the tail of an explanation and never the
input. There are no other rows now, so the property is structural instead: the band holds one
**single-line** `TextEdit`, which does not wrap, and everything unbounded lives in a floating
layer that allocates nothing in the band's `Ui`. Structural **only while nothing else is put
in the band**, which `BAND_ROWS`' doc now states for whoever next wants a row.
`the_box_stays_inside_the_band_however_long_the_refusal_is` drives a real refusal at 320 pt
and asserts `band.contains_rect(box)`. **Mutation-measured**: draw the note as a label in the
band and the box lands at `y 657.9…673.0` against a band of `574.9…600.0` — **73 pt outside**,
twice #112's original 35.6, because a one-row band has less slack to absorb a wrap than a
four-row one did. ⚠️ `BOX_ID` survives; `a_note_appearing_does_not_take_the_box_with_it` still
*passes* under that mutation, which is why there are two tests.

**Tier 3 — the panel column draws Organon's own cards, from Organon's own code.** James: *"I
want us to adopt the styling … so that it looks just like it does here in terms of the padding
and the fact that it just has the one word for the panel and not all of the words you have
now. **In fact, it should use the same exact code somehow.**"*

`panel_stack::OrganonDraw` widened from `FnMut(&mut Ui, &'static Panel)` to
`FnMut(&mut Ui, &'static Panel) -> bool`, and is handed **every** panel in the column rather
than only the `Live` one. `panel_surface::OrganonPanels::card` is now a second caller of
`lib.rs`'s `card()` — `theme::framed`, the three-stop silver header band, an
`egui::CollapsingHeader` — alongside the Look tab. One card function in the tree, two
products, so the padding and the corners are the editor's by construction rather than by
spec.

🚨 **The alternative was rejected for a stated reason.** The other way to run one copy of the
chrome is to move it down into a crate both can see — and there is no such crate:
`organon-console` cannot depend on the root crate (the root crate depends on *it*),
`organon-core` is host-free *by acceptance test* (`cargo tree -p organon-core` must show no
egui), and `theme.rs` reaches `nih_plug_egui`, `theme_config` and the paint helpers. Widening
a seam that already existed and already pointed the right way was the smaller change.

⚠️ **The `bool` is what keeps `organon-console` a library.** `false` means this build drew no
card, and the stack falls through to `panel_stack::plain_card` — the frame and heading it drew
before. Every test in the crate takes that arm, and
`a_console_with_no_organon_behind_it_still_draws_the_panel` reads the frame's own text shapes
rather than a return value, because the claim is about pixels. ⚠️ `NOT_TRANSPLANTED` is still
the console's sentence and now sits *inside* the card: twenty-four of the twenty-five panels
are `Declared`, and one that skipped the chrome would be the single card in a column of
twenty-five wearing something else. `panel_stack::absent_body` is the one place that decides
which panels get it. ⚠️ The inter-card gap moved with the chrome — `card()` ends on its own
`add_space(6.0)`, so `panel_stack::draw` no longer adds a second one.

✏️ **`panel_stack::heading` is now `panel.title` and nothing else** — `Surface`, where it said
`◈ organon · look · Surface`. The breadcrumb answered a question this surface no longer asks:
it was written when a panel was an element scrolling past in a *transcript*, where "which of
Organon's tabs is this from" is real. In a panel column every card is one of Organon's, and
the column is the answer.

🚨 **Nothing here has been seen on a screen.** Every tier is a claim about how something looks
or how quiet it feels, and all that is established is that it compiles and that the *rules*
hold: whether a conversation with the narration removed reads as calm or as broken, whether a
popover over a panel column beats three rows under it, and whether an editor card sized for a
three-column editor pass survives in a region column — including that a stacked panel can now
be **folded**, which nobody asked for and nobody has tried — are all James's to judge.
