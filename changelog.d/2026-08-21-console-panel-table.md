### Organon's editor panels become a table, and three more of them appear in the Console

James, on what a preset ought to be able to do: *"we can tell from the preset what values we have
adjusted from the default. And thereby, we could construct custom panels or even a single custom
panel with sections and sliders and dropdowns that are **tailored to the exact changes that we
made on that preset**."*

🚨 **That and the panel transplant are one problem approached from opposite ends.** Transplanting
a panel into Organon Console (`CONSOLE_ARCHITECTURE.md` §1.11) hand-writes, panel by panel, a
mapping of **field → section → widget**; a panel built from a preset's diff needs exactly that
mapping **as data**. Writing twenty-four imperative bodies first and extracting a table afterwards
means writing the mapping twice and then reconciling it. So the table comes first —
`native/src/panel_table.rs` — and both renderers read it.

**What the table carries is only what a `nih_plug` param cannot say about itself**, and that was
measured across all **519** rows of the Look tab before a line of it was written. Panel, section,
order and the row's **label** are in it, because none of those exists anywhere else — a
`PresetValues` field is a name and a type, and 361 of the 519 labels differ from `Param::name()`.
Range, unit, value formatting, the ⟲ default, a dropdown's option list and **the control kind**
are not, because the param already answers them.

📌 **The control kind being derivable is a measurement, not a hope.** `FloatParam`/`IntParam` →
a slider (399 rows), `BoolParam` → a checkbox (83), `EnumParam` → a dropdown (37) — and across
the whole tab the editor's choice disagrees with the param's Rust type in **no case at all**. So
`param_sink::AutoRow` reproduces the editor's control choice from the param alone, and a fifth
param kind used in a panel is a compile error naming the missing impl rather than a control
quietly drawn as a slider.

⚠️ **The label is not the param's name, and the reason is structural rather than sloppy.**
`kal_spin` is `"Kaleido Spin"` to a DAW's flat automation list and `"spin"` inside a card already
headed *Scene Kaleidoscope*. **The label is a function of the grouping** — which is why one table
owns both, and why deriving it from the param would be wrong twice: redundant on screen, and it
would change what the plugin draws.

🚨 **It is a macro list rather than an array of `&'static str`, and that is the whole of why it
is safe.** A string `"bevel"` in a `&[Row]` is checked by nothing; `row bevel` in the list
expands to `&p.bevel` **and** `|pv| &mut pv.bevel`, so a rename on either side is a compile
error — the property `param_sink`'s macros exist to provide, and the idiom `preset.rs`'s
`for_each_tab_field!` and `param_table.rs`'s `param_block!` already use. One list expands three
ways: the panel's body (drawn by Organon's editor with `Sink::Host` and by the Console with
`Sink::Mirror`), the `&'static [Item]` a preset's diff is grouped by, and a `draw_one(name)` that
answers one control chosen at runtime by field name.

**A panel declares which kind it is, and the count is the honest number.** `@generated` means the
body *is* the list, and a hand-written fragment in one is a `compile_error!`. `@labelled` means
the body stays hand-written because the panel has control flow, a file dialog or a capability
gate, and the table still owns its labels and grouping. Measured, the twenty-four
un-transplanted panels are **352 rows, 40 help texts, 36 section headings, six conditionals and
three hand-written fragments** — so twenty-one qualify for `@generated`, and
`grep -c '@labelled'` is what "how much of this tab is still written by hand" answers to.

**Landed here: Cast Shadows, Lighting (Direct) and Bloom**, ten controls between them, drawn by
Organon's editor and Organon Console from one declaration. `panels::Status::Live` is four panels
now rather than one. ⚠️ Surface stays hand-written for the moment — it holds nearly all of the
tab's complexity by itself (fifteen disclosure reads, two file dialogs, a material-graph loader),
and joining it is a 167-site rewrite that does not belong in the change establishing the
mechanism.

🚨 **Eleven of the Look tab's controls have no writable mirror, and it is deliberate.** Of the
**514 distinct fields** the twenty-five panels draw, 503 are in `PresetValues`; the other eleven
are Temporal's seven and four of Ray Tracing's, and both the card's own help text and `params.rs`
say why — *"Per-display, NOT preset-captured (like HDR/MSAA/rt_debug)"*. **Look ▸ Temporal
therefore cannot be transplanted at all through the mirror**: every one of its seven controls
would draw, drag and move nothing, which is precisely why `/panel` was retired. Neither panel may
be declared until the table can say *editor-only*, and
`panel_table::every_row_the_table_names_has_a_writable_mirror` is what stops one being added by
accident in the meantime.

⚠️ **Three witnesses in `panel_stack.rs` and one in `registry.rs` named Bloom as their example of
a `Declared` panel**, so transplanting Bloom failed all four for a reason that had nothing to do
with what they assert — they wanted *a* declared panel and had been handed *that* one. They now
ask the table for a panel of each status rather than naming one, and each `expect`s loudly on the
day a status has no members left. One of them was worse than a plain failure: `remove_last("bloom")`
against a stack that no longer held Bloom removed nothing and the test still passed, which is a
witness that had quietly stopped witnessing.
