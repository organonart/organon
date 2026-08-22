### `/preset` — load a preset and get a panel of exactly the controls it changed

James, on the idea this closes: *"we can tell from the preset what values we have adjusted from
the default. And thereby, we could construct custom panels **or even a single custom panel** with
sections and sliders and dropdowns that are **tailored to the exact changes that we made on that
preset** … so that we could do `/preset load` and it would cause UI panels to appear where we
have set them up to be."*

`console.preset load <name>` — `/preset load` in a composer — does two things, and the order is
the design. First the preset's values reach the console's parameter mirror, so **the look
changes** on the next frame, through the same write path a dragged slider takes. Second, if any
region is holding a panel column, a card built from what that preset changed replaces whatever
card the last preset left there. `console.preset save <name>` captures the console's current look
as a preset, recording what it changed as what the preset is *about*; `console.preset.list` is
the read beside it.

🚨 **One card with sections, not a column of filtered panels — and the first shape is not merely
worse, it is impossible now.** #130 made `panel_stack::admit` refuse a `Declared` panel at every
door, so a column of Organon's own panels can hold only the four this build can draw. A preset
touching a Generator field would have had no card to go in, leaving only two bad options: drop
the control, or invent a card no `/organon` ring can name. James's own second reading — *"or even
a single custom panel"* — is the shape that works: **one card whose sections are named after
where its controls come from**, a transplanted panel's title where one owns the field and the
editor *tab* where none does.

⚠️ **A control with no transplanted panel is still drawn**, from the param alone.
`panel_table::draw_any_field` generates one arm per field from `preset.rs`'s `for_each_tab_field!`
— the same list `PresetValues`' capture, apply and tab partition are generated from — so **every
field a preset can carry has a control on the day it joins that list**, with no edit anywhere.
What such a row lacks is the short editor label and the grouping: it reads `Kaleido Spin`,
ungrouped, where a joined panel says `spin` under *Scene Kaleidoscope*. The control kind, range,
unit, value formatting and dropdown options are identical, because those come off the param.
That deliberately second-class rendering is the visible argument for joining the next panel;
dropping the field, or inventing a label, would both have hidden the gap. 📌 `preset load`
therefore **reports its own coverage** — how many controls the card holds and how many came from
a transplanted panel — and that second number going up in the same commit as a transplant is the
point of printing it.

**`panel_stack::Entry` now holds a `Held`**, which is one of Organon's panels or a preset's card,
and is therefore no longer `Copy` — a preset card carries a name a person typed. ⚠️ **A synthetic
`Panel` outside `panels::PANELS` was the alternative and was refused**: it would have been the
first thing in a column that no ring could name and no `stack remove` word could address. The
consequence is stated rather than hidden — `Stack::remove_last` never matches a preset card, and
`Stack::remove_presets` filters on the *arm* rather than on a slug that "cannot collide", because
the day somebody names a preset `surface` that stops being true.

🚨 **A preset name has spaces in it and a layout name may not, which is why these are two verbs
rather than one shape used twice.** `layout::check_name` refuses whitespace — right for a name a
person invents, wrong for one that already exists (*Rails — Crystal Throat* is a factory preset).
So the sidecar line takes **the rest of the line** rather than the next word, safe only because
the name is the last field; and `load` matches by **unique case-insensitive substring**, because
the slash grammar fills one word per required argument and an exact match would make most of the
store untypeable in the composer this verb mainly exists for. **Ambiguity and absence are both
refused by name**, listing what would have worked — which is what makes the substring rule safe
rather than merely convenient: it never silently picks one.

⚠️ **A console with no panel region still loads the look.** The card is refused by name and the
look is not, because the two are separate promises and failing the second is no reason to
withhold the first — the opposite of `stack add`'s rule, which has nothing to do *but* fill a
column.

⚠️ **`/preset clear` is not built**, and the absence is stated rather than an oversight: taking
the card back out without loading another preset needs a verb with **no** argument, and every
write on this lane carries at least one word. `console stack remove all` reaches the same state
today, and `Stack::remove_presets` is already there for the verb when it lands.
