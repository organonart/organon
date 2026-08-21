### Organon Console: the panel column's cards follow the console's palette, and stack flush (#120)

#117 made the Console draw Organon's own `card()` so the padding, the corners and the one-word
heading could not drift — and the **palette came with it**. Every colour that function paints
resolved through `theme.rs`'s accessors, which read *Organon's* `theme_config`, so `/theme dark`
moved the terminal, the composer, the status strip and the tab bar and left the panel column
blue-slate. James, on the live build: *"I didn't want to adopt the blue, gray color theme for all
of these. I want them to adapt to the current theme colors. … Right now, the colors stay fixed no
matter how I set the theme of the panels I'm talking about. Also, we need to remove that spacing.
See the dark spacing between them? We need to tighten it up and stack them more tightly
together."*

**The card is now drawn through a `theme::CardStyle` — pigment is a parameter, geometry is not.**
One card body still, called by both products, which is the whole of what #117 bought. Organon's
editor passes `CardStyle::organon()`, read from the live `theme_config` exactly as the accessors
did before this type existed; the Console passes `panel_surface::console_card_style`, cut from
the palette `/theme` selected. **Corner radius, inner margin, the header band's bleed, the
collapsing header, the grain and the bevel take no parameter and stay shared** — a second card
function is the drift the shared one exists to prevent.

🚨 **The two gradients are DERIVED from `Theme::panel_fill` rather than read from new palette
fields, and the alternative is worth naming.** Four new `Theme` fields (`card_body`,
`card_header`, …) is the more explicit design; it costs more than it buys here. All four shipped
palettes would have to answer four questions they have no opinion about, the coverage test would
need a *fifth* palette disagreeing on each to see them at all — and, the part that decides it,
**a palette James writes later would have to fill them to look right**, when the complaint is
precisely that the column ignores the palette he already wrote. Derivation means a new palette
needs only the fields it already sets. The day a palette *wants* a card that does not follow its
panel plate is the day those fields earn their place.

⚠️ **The steps are neutral — the same number on all three channels — and that is not a
simplification.** Organon's own table tilts blue as it lightens (`card 0x212A30` → body top
`0x242E35`, i.e. `+3,+4,+5`), because §2's generating rule is `blue − red ∈ 8..=15` and every
blue-slate surface obeys it. Carrying those per-channel deltas onto a green or a warm plate would
drag it back toward blue slate, which is the exact complaint. The **magnitudes** are Organon's,
averaged across its three channels, so a cut card keeps the reference's compressed 5–15-level
tonal steps (§14) in the caller's own hue. `a_cut_card_does_not_tint_the_plate_it_came_from`
pins it on a deliberately green plate; `a_header_band_is_brightest_in_the_middle` pins §5's
actual trick — the band's middle stop lighter than both ends — for a cut card as well as for
Organon's, because a derivation that put the bright stop at an end would lose the header
treatment while everything still compiled.

⚠️ **Two colours are read verbatim, not derived**: `panel_edge` becomes the card's border and
`panel_title` its heading. The palette already has an opinion about both, and deriving over the
top of an opinion is how a themed thing stops being themed.

⚠️ **`panel_fill` is premultiplied glass and the card is opaque.** The column paints that plate
over whatever is behind it; a card in the same translucent colour would darken the region twice
and sink into its own background. Its premultiplied components are exactly what the column
contributes over black, which is what makes them the right base to step from —
`a_cut_card_is_opaque` stops an alpha slipping through, which would have looked fine on a black
backdrop and wrong on a lit one.

⚠️ **A card is not separated from the column by its fill, and never was.** `palette.panel` and
`palette.card` are *the same value* in blue slate; what separates a card is its gradient, its
border and its bevel highlight. So handing the column's own background over as the plate is
right rather than something to correct with an arbitrary lightening.

🚨 **The spacing needed two numbers, and measuring it first is what stopped the obvious fix from
looking like a failure.** Measured on a headless egui context rather than reasoned about: a card's
own trailing `add_space` is only half the seam — egui inserts `item_spacing.y` between the
stack's entries as well, and **the two surfaces run different ones.** Organon's editor sets 6
(`theme::install`); the Console never touches spacing and takes egui's default 3. Card to card that
is **12 pt in the editor and 9 pt in the column** — so the column was already the *tighter* of
the two, and cutting the card's constant to 0 alone would have left a 3 pt floor no card setting
could reach. What made the seam *read* as a dark band was that it showed near-black `panel_fill`
beside a blue-slate card; the palette work above is most of that fix.

`panel_stack::draw` now zeroes its own contribution around the loop and **each card restores it
inside its own scope**, so the rows *within* a panel — labels, sliders, value boxes — space
themselves exactly as before; only the distance between two cards moves.
`panel_surface::PANEL_COLUMN_GAP` is 0, `panel_stack::GAP` (the no-Organon fallback's) follows it
to 0, and **the column's seam is 0 pt while the editor's is unchanged at 12.**

Both halves are now pinned where they can be seen.
`panel_stack::the_column_contributes_no_space_between_two_cards` lays a real stack out on a
headless context and measures the stack's own contribution — delete the zeroing line and it
reports 3; drop the restoration and it says in the same run that every row inside every panel
just tightened with the column. `panel_surface::two_editor_cards_leave_twelve_points_between_them`
measures the other product: the 105 cards nobody in this change was looking at still sit exactly
12 pt apart.

⚠️ **`panel_stack::GAP` is `pub` now**, and equal to `PANEL_COLUMN_GAP` by assertion rather than
by comment. The two live either side of a crate boundary, so the compiler has nothing to say
about them; the root crate is the only place that can see both, and
`the_fallback_leaves_the_same_gap_organons_card_does` is where they are compared.

**Organon's own editor is unchanged, and that is a test rather than a claim.**
`organon_card_style_is_the_look_that_shipped` writes out all nine values the card painted before
`CardStyle` existed — `0x242E35`, `0x1D252B`, `0x202930`, `0x303B43`, `0x2B353D`, `0x303A41`,
`0x3A464F`, `0xD1D6D9`, gap 6 — against `ThemeConfig::default()`, so a refactor that quietly
re-derived one of them or reached for a different accessor fails here instead of shifting 105
cards by a few levels where nobody would look. ⚠️ It names the **default** config rather than the
live one for the reason `theme.rs`'s `surfaces()` already states: `theme_config::active()` comes
off disk, and a machine whose owner has used the theme editor would otherwise turn the suite red
for a reason having nothing to do with the code. `CardStyle::of` and `paint::{card,silver}_stops_of`
exist to make that possible.

⚠️ **What is still Organon's palette inside a Console card**: the Surface panel's own row labels,
which `param_sink` draws in `theme::TITANIUM()`. One of the twenty-five panels is `Live`, so it is
one card's rows — but it is real, and it is the next thing to notice if the column still reads as
half-themed.

📌 **Noticed, not fixed, and out of scope here**: `paint::card_stops` blends the palette's `card`
toward two *fixed* blue-slate colours by `depth.card_gradient`, which ships at `1.0` — so at the
default depth the base is interpolated all the way out and a card's body gradient is the same
pair whatever palette is active. It is why Organon's own Warm Instrument and High Contrast themes
have blue-slate card bodies today. Changing it would restyle two shipped themes, which is a
decision for a change about Organon's palettes.

🚨 **Nothing here has been seen on a screen.** Every number above is layout arithmetic from a
headless context and every colour is a value in a test; whether the column now reads as *right* —
whether a 0 pt seam is tight or claustrophobic, whether a card cut from `panel_fill` separates
from the column it sits on — is a judgement only James can make in front of the live build.
