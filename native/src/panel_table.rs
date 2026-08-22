//! **Organon's editor panels, declared rather than written** — the one table that both
//! Organon's own editor and Organon Console render from, and that a preset reads to work out
//! which controls its diff is *about* (organon#124).
//!
//! # Why a table at all
//!
//! Two jobs turned out to be the same job from opposite ends. Transplanting a panel into the
//! Console (`CONSOLE_ARCHITECTURE.md` §1.11) hand-writes, panel by panel, a mapping of
//! **field → section → widget**. Building a panel *from a preset* needs exactly that mapping,
//! **as data**. Writing twenty-four imperative bodies first and extracting the table afterwards
//! means writing the mapping twice and then reconciling it — so the table comes first and both
//! renderers read it.
//!
//! # 🚨 What the table carries, and what it deliberately does not
//!
//! It carries only what a `nih_plug` param **cannot say about itself**. Measured across all 519
//! rows of the Look tab before a line of this was written:
//!
//! | | in the table? | why |
//! |---|---|---|
//! | which panel, which section, the order | **yes** | it exists nowhere else — a `PresetValues` field is a name and a type |
//! | the row's label | **yes** | 361 of 519 differ from `Param::name()`, *systematically* — see below |
//! | the control kind | **no** | derivable, 519/519, with zero exceptions — see [`crate::param_sink::AutoRow`] |
//! | a dropdown's options | **no** | [`crate::param_sink::choice_row`] already walks the param's own steps |
//! | range, unit, value formatting, the ⟲ default | **no** | all read off the real `OrganicMathParams` |
//!
//! ⚠️ **The label is not the param's name, and the reason is structural rather than sloppy.**
//! `kal_spin` is `"Kaleido Spin"` to a DAW's flat automation list and `"spin"` inside a card
//! already headed *Scene Kaleidoscope*; `bevel` is `"Bevel"` and `"node bevel"`. **The label is
//! a function of the grouping**, which is why the same table has to own both — and why deriving
//! it from the param would be wrong twice over: redundant on screen, and it would change what
//! the plugin draws.
//!
//! # The identity join, kept
//!
//! This is **a macro list, not a `&[Row]` array of `&'static str` field names**, and that is the
//! whole reason it is safe. A string `"bevel"` in an array is checked by nothing; `row bevel`
//! here expands to `&p.bevel` *and* `|pv| &mut pv.bevel`, so a rename on either side is a
//! compile error — the property [`crate::param_sink`]'s macros exist to provide, and the idiom
//! `preset.rs`'s `for_each_tab_field!` and `param_table.rs`'s `param_block!` already use.
//!
//! # Two kinds of panel, and the count is a number worth watching
//!
//! - [`panel!`]`(@generated …)` — the body **is** the list. The table draws it, and a `free`
//!   item is a `compile_error!`, so a generated panel provably has no hand-written fragment
//!   hiding in it. Twenty-one of the Look tab's twenty-four un-transplanted panels qualify:
//!   measured, they contain 352 rows, 40 help texts, 36 section headings and **six**
//!   conditionals in total.
//! - [`panel!`]`(@labelled …)` — the body is hand-written because the panel has control flow,
//!   a file dialog or a GPU-capability gate. The table still owns its labels and its grouping;
//!   only the *order* lives in the body. **Three panels need this** — Ray Tracing, Liquid and
//!   Post — plus Surface, which holds nearly all of the Look tab's complexity on its own.
//!
//! Every panel declares which it is, so "how much of this tab is still hand-written" is a
//! question the source answers by grepping for `@labelled`.
//!
//! # ⚠️ egui ids come from the field, never from widget order
//!
//! §1.11 records the id-collision bug being fixed twice, and dynamically filtered widgets are
//! exactly where it comes back: a preset panel drawing rows 3, 7 and 40 of a card must give
//! each the same id it would have had drawing all forty. `stringify!($f)` is what every
//! generated control is keyed on — stable under filtering, unique by construction, and unrelated
//! to how many rows preceded it.

use crate::param_sink::Sink;
use crate::params::OrganicMathParams;
use nih_plug_egui::egui;

/// One entry of a panel's declaration, in the editor's own order.
///
/// This is the *data* half of the table — what [`crate::preset`]'s diff is grouped by, and what
/// a dynamically constructed panel walks. The drawing half is generated from the same list, so
/// the two cannot disagree about what a panel contains.
///
/// ⚠️ **[`Item::Section`] is a marker, not a container.** A section owns the rows that follow it
/// until the next one, which is exactly how the editor draws it — a heading, then rows. A filter
/// therefore emits a heading *lazily*, before the first row under it that survives, so a section
/// whose rows were all filtered away draws nothing rather than an empty heading.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Item {
    /// A control. `field` is the identifier shared by `OrganicMathParams` and
    /// `PresetValues`; `wide` is the one piece of *presentation* the table carries, because a
    /// dropdown's width is a layout decision the param cannot make (`2 × COMBO_W` for the few
    /// hero combos, `COMBO_W` otherwise) and it is invisible to every other control kind.
    Row { field: &'static str, label: &'static str, wide: bool },
    /// A `— shadows (Tier 1) —` sub-heading inside the card.
    ///
    /// ⚠️ Not constructed by any declaration yet — none of the three panels in [`DECLARED`] has
    /// an internal sub-heading, and the eleven that do are still `Declared`. (Surface has none
    /// either, but it is undeclared rather than sub-heading-free by luck: `surface_card`
    /// separates with `ui.separator()`, and nothing in this table sees it.) `group_exposed`
    /// nevertheless has to know what one means, because a preset-built card's headings are the
    /// same idiom one level out.
    #[allow(dead_code)]
    Section(&'static str),
    /// A `help()` paragraph.
    Help(&'static str),
    /// 🚨 **A fragment the table cannot express** — a button, a file dialog, a capability gate —
    /// carried by *name* rather than inline so that "how much of this panel is still
    /// hand-written" is a countable number rather than a feeling. Only legal in an `@labelled`
    /// panel; a `@generated` one fails to compile.
    ///
    /// ⚠️ Not constructed yet either: the first `@labelled` declaration is Surface's, and it is
    /// the next change. The `compile_error!` in `panel!`'s `@draw` arm is what makes this a
    /// guarantee rather than a convention, and that arm is checked today.
    #[allow(dead_code)]
    Free(&'static str),
}

impl Item {
    /// The `PresetValues` field this item controls, if it controls one.
    pub(crate) fn field(&self) -> Option<&'static str> {
        match self {
            Item::Row { field, .. } => Some(field),
            _ => None,
        }
    }
}

/// The sub-heading the editor draws inside a card — `lib.rs`'s own idiom, in one place so a
/// generated body and a hand-written one cannot render it differently.
pub(crate) fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).weak().small());
}

/// **Declare one editor panel.** See the module doc for the two forms and what they promise.
///
/// ```ignore
/// panel!(@generated shadows, LOOK_SHADOWS;
///     (row  shadow_enabled,  "enable (shadow map)"),
///     (row  shadow_bias,     "bias"),
///     (help "Off by default. A world-space depth map from the KEY light …"),
/// );
/// ```
///
/// The item kinds are `row` (any control — the kind is derived from the param), `wide` (a
/// dropdown at `2 × COMBO_W`), `sect`, `help` and `free`.
macro_rules! panel {
    // ---- the two entry forms ------------------------------------------------------------
    (@generated $module:ident, $panel:ident; $( ( $($it:tt)* ) ),* $(,)?) => {
        pub(crate) mod $module {
            use super::*;
            panel!(@inner $panel; $( ( $($it)* ) ),*);

            /// **This panel's whole body**, generated from the list above and drawn by both
            /// products: Organon's editor passes `Sink::Host`, Organon Console `Sink::Mirror`.
            /// There is no second rendering to keep in step.
            #[allow(unused_variables)]
            pub(crate) fn body(
                ui: &mut egui::Ui,
                w: f32,
                p: &OrganicMathParams,
                sink: &mut Sink,
            ) {
                $( panel!(@draw ui, w, p, sink, $($it)*); )*
            }
        }
    };
    (@labelled $module:ident, $panel:ident; $( ( $($it:tt)* ) ),* $(,)?) => {
        pub(crate) mod $module {
            use super::*;
            panel!(@inner $panel; $( ( $($it)* ) ),*);
        }
    };

    // ---- what both forms share ----------------------------------------------------------
    (@inner $panel:ident; $( ( $($it:tt)* ) ),* $(,)?) => {
        /// The panel this declaration belongs to — the join to `organon_core::panels`, so a
        /// declaration can never name a panel the `/organon` rings do not offer.
        pub(crate) const PANEL: &organon_core::panels::Panel = &organon_core::panels::$panel;

        /// Every item, in the editor's own order. **The data half of the table** — what a
        /// preset's diff is grouped by.
        pub(crate) const ITEMS: &[Item] = &[ $( panel!(@item $($it)*) ),* ];

        /// Each row's label, addressable by the field's **own identifier**, so a hand-written
        /// body says the label once — here — and names it rather than repeating it.
        #[allow(non_upper_case_globals, dead_code)]
        pub(crate) mod label {
            $( panel!(@label $($it)*); )*
        }

        /// **Draw exactly one of this panel's rows, chosen at runtime by field name.** This is
        /// what a preset-filtered panel calls: it knows a field name and needs the control that
        /// belongs to it, with this panel's label and this panel's width.
        ///
        /// Answers `false` for a name this panel does not own, so a caller can walk the panels
        /// in table order and stop at the first that claims it.
        #[allow(unused_variables)]
        pub(crate) fn draw_one(
            ui: &mut egui::Ui,
            w: f32,
            p: &OrganicMathParams,
            sink: &mut Sink,
            name: &str,
        ) -> bool {
            $( panel!(@one ui, w, p, sink, name, $($it)*); )*
            false
        }
    };

    // ---- per-item expansions ------------------------------------------------------------
    (@item row $f:ident, $l:expr) => {
        Item::Row { field: stringify!($f), label: $l, wide: false }
    };
    (@item wide $f:ident, $l:expr) => {
        Item::Row { field: stringify!($f), label: $l, wide: true }
    };
    (@item sect $t:expr) => { Item::Section($t) };
    (@item help $t:expr) => { Item::Help($t) };
    (@item free $n:expr) => { Item::Free($n) };

    (@label row $f:ident, $l:expr) => { pub(crate) const $f: &str = $l; };
    (@label wide $f:ident, $l:expr) => { pub(crate) const $f: &str = $l; };
    (@label $($rest:tt)*) => {};

    (@draw $ui:expr, $w:expr, $p:expr, $sink:expr, row $f:ident, $l:expr) => {
        $crate::param_sink::auto_row($ui, $w, $l, &$p.$f, $sink, |pv| &mut pv.$f)
    };
    (@draw $ui:expr, $w:expr, $p:expr, $sink:expr, wide $f:ident, $l:expr) => {
        $crate::param_sink::auto_row_wide($ui, $w, $l, &$p.$f, $sink, |pv| &mut pv.$f)
    };
    (@draw $ui:expr, $w:expr, $p:expr, $sink:expr, sect $t:expr) => {
        $crate::panel_table::section($ui, $t)
    };
    (@draw $ui:expr, $w:expr, $p:expr, $sink:expr, help $t:expr) => {
        $crate::help($ui, $t)
    };
    // 🚨 The guarantee `@generated` makes, enforced rather than promised.
    (@draw $ui:expr, $w:expr, $p:expr, $sink:expr, free $n:expr) => {
        compile_error!(
            "a `free` item cannot be generated — declare this panel `@labelled` and write its body"
        )
    };

    (@one $ui:expr, $w:expr, $p:expr, $sink:expr, $n:expr, row $f:ident, $l:expr) => {
        if $n == stringify!($f) {
            $crate::param_sink::auto_row($ui, $w, $l, &$p.$f, $sink, |pv| &mut pv.$f);
            return true;
        }
    };
    (@one $ui:expr, $w:expr, $p:expr, $sink:expr, $n:expr, wide $f:ident, $l:expr) => {
        if $n == stringify!($f) {
            $crate::param_sink::auto_row_wide($ui, $w, $l, &$p.$f, $sink, |pv| &mut pv.$f);
            return true;
        }
    };
    (@one $($rest:tt)*) => {};
}

// ===========================================================================================
// The Look tab
// ===========================================================================================
//
// In the editor's own column-then-row order, which is `organon_core::panels::PANELS`' order and
// the order a preset-built panel is grouped in. A panel joins this table in the change that
// gives Organon Console a body for it and flips its `panels::Status` to `Live`.

panel!(@generated shadows, LOOK_SHADOWS;
    (row  shadow_enabled,  "enable (shadow map)"),
    (row  shadow_bias,     "bias"),
    (row  shadow_strength, "strength"),
    (help "Off by default. A world-space depth map from the KEY light — \
           cubes cast real shadows on each other. Raise bias if you see \
           shadow acne (stippling), lower it if shadows detach. \
           Instanced/cube paths only (raymarch + membrane don't cast). \
           On an M3+ Mac, RT Shadows (Ray Tracing card) supersede this \
           map with traced per-pixel occlusion — no bias tuning needed."),
);

panel!(@generated lighting, LOOK_LIGHTING;
    (row ambient,        "ambient"),
    (row key_intensity,  "key"),
    (row fill_intensity, "fill"),
    (row elevation,      "elevation"),
    (row azimuth,        "azimuth"),
);

panel!(@generated bloom, LOOK_BLOOM;
    (row bloom_intensity, "bloom"),
    (row bloom_threshold, "threshold"),
);

/// Every panel this table declares, in the editor's order — what a preset-built panel walks.
///
/// 🚨 **Three, and a column can draw four. Both numbers are true and they are different
/// questions.** *What a column can hold* is `panels::Status::Live`, and there are **four** Live
/// panels — Surface, Lighting, Shadows, Bloom — because `panel_surface::OrganonPanels::draw`
/// answers Surface from its own hand-written `surface_card` before it ever consults
/// [`body_for`]. *What this table can home* is `DECLARED`, and there are **three**: Surface has
/// no declaration, so [`draw_field`] and [`group_exposed`] cannot reach it. Any sentence
/// carrying one of these counts has to say which — a bare "four" in this file is how the two
/// come to be confused.
///
/// ⚠️ **A `panel!` declaration missing from here is invisible to the preset panel**, exactly as
/// a `panels::Panel` missing from `panels::PANELS` is invisible to the `/organon` rings, and for
/// the same reason: Rust cannot enumerate a module's items.
/// [`tests::every_declared_panel_is_in_the_index`] counts the arm.
pub(crate) const DECLARED: &[(&organon_core::panels::Panel, &[Item])] =
    &[(shadows::PANEL, shadows::ITEMS), (lighting::PANEL, lighting::ITEMS), (bloom::PANEL, bloom::ITEMS)];

/// Draw one control, named at runtime, wherever in the table it lives.
///
/// **This is the seam a preset-built panel is drawn through.** It walks [`DECLARED`] in table
/// order and hands the name to each panel's own `draw_one`, so the row arrives with the label,
/// the width and the control kind the editor would have given it — not a generic re-rendering
/// that happens to look similar.
///
/// Answers `false` for a field no declared panel owns. ⚠️ **That is a real and frequent answer
/// today**, not an error case: the Look tab homes 503 of `PresetValues`' 1,333 fields once it is
/// fully joined, and only three panels are joined so far. A caller draws such a field from its
/// param alone rather than dropping it — a field is never silently absent from a panel that
/// claims to show what a preset changed.
pub(crate) fn draw_field(
    ui: &mut egui::Ui,
    w: f32,
    p: &OrganicMathParams,
    sink: &mut Sink,
    name: &str,
) -> bool {
    shadows::draw_one(ui, w, p, sink, name)
        || lighting::draw_one(ui, w, p, sink, name)
        || bloom::draw_one(ui, w, p, sink, name)
}

/// 🚨 **Draw ANY preset field, by name, whether or not a panel has ever mentioned it.**
///
/// This is what closes the gap [`draw_field`] leaves open, and it closes it completely: the
/// arms are generated from `preset.rs`'s `for_each_tab_field!`, the same list `PresetValues`'
/// capture, apply and tab partition are generated from. **Every field a preset can carry has a
/// control here on the day it is added to that list**, with no edit in this file.
///
/// ⚠️ **The label is the param's own name**, because there is nowhere else for it to come from —
/// a field with no table row has no short label, and §"What the table carries" says why the two
/// differ. That is deliberately second-class rendering: `"Kaleido Spin"` where a joined panel
/// would say `"spin"` under a *Scene Kaleidoscope* heading. A row that reads long and ungrouped
/// is the visible argument for joining the next tab, which is better than the alternatives —
/// dropping the field, or inventing a label.
///
/// ⚠️ **The control kind still comes from the param**, so this is not a lesser *widget*: a
/// slider is the same slider and a dropdown carries the same variant names. Only the label and
/// the grouping are missing, and only those.
///
/// Answers `false` for a name no preset field answers to — a renamed param, or a hand-edited
/// `presets.json`. `Preset::unknown_exposed` is what reports those by name.
pub(crate) fn draw_any_field(
    ui: &mut egui::Ui,
    w: f32,
    p: &OrganicMathParams,
    sink: &mut Sink,
    name: &str,
) -> bool {
    // ⚠️ **One arm per field, not a lookup table**, for the reason the whole of this module is
    // macros rather than data: an arm names the field once and derives `&p.$f` and
    // `|pv| &mut pv.$f` from it, so a rename on either side is a compile error. A `HashMap<&str,
    // …>` would need the accessor pair written out per entry, which is the second list this
    // file exists to avoid. The linear scan costs a few hundred `&str` compares per drawn row
    // and is invisible beside laying out one widget.
    macro_rules! arm {
        ($tab:ident, $f:ident, scalar) => {
            if name == stringify!($f) {
                let label = ::nih_plug::prelude::Param::name(&p.$f).to_string();
                $crate::param_sink::auto_row(ui, w, &label, &p.$f, sink, |pv| &mut pv.$f);
                return true;
            }
        };
        ($tab:ident, $f:ident, enum, $ty:ident) => {
            if name == stringify!($f) {
                let label = ::nih_plug::prelude::Param::name(&p.$f).to_string();
                $crate::param_sink::auto_row_wide(ui, w, &label, &p.$f, sink, |pv| &mut pv.$f);
                return true;
            }
        };
    }
    crate::preset::for_each_tab_field!(arm);
    false
}

// ===========================================================================================
// A panel built from a preset (organon#124)
// ===========================================================================================

/// One heading of a preset-built panel, and the controls under it.
///
/// 🚨 **One card with sections, not a card per panel** — James's own second reading of the idea:
/// *"we could construct custom panels **or even a single custom panel** with sections and
/// sliders and dropdowns that are tailored to the exact changes that we made on that preset."*
/// The second shape is the one that works, and it works because of what changed underneath it:
/// `panel_stack::admit` now refuses a `Declared` panel at every door, so a column of Organon's
/// own panels can only ever hold the four this build can draw. A preset touching a Generator
/// field has no card to put it in. **A card of its own, sectioned by where the controls come
/// from, has room for all of them.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Section {
    /// The panel this group of controls comes from, or — for controls no joined panel homes
    /// yet — the editor tab they belong to.
    pub heading: &'static str,
    /// The fields under it, in the editor's own order.
    pub fields: Vec<&'static str>,
    /// Whether [`heading`](Self::heading) names a panel (`true`) or a tab (`false`). ⚠️ It is
    /// the difference between a control drawn with its short editor label and one drawn with
    /// the param's own long name — see [`draw_any_field`] — so it is worth carrying rather than
    /// re-deriving, and it is what a caller counts to report coverage honestly.
    pub homed: bool,
}

/// **Group a preset's exposed fields into the sections of one card.**
///
/// Pure, and outside the drawing, so the shape of a preset-built panel is checkable without an
/// `egui::Ui` — the same reason `panel_stack::admit` is a free function.
///
/// The order is the editor's twice over: joined panels first, in [`DECLARED`] order with each
/// panel's own row order inside it; then everything else in `for_each_tab_field!`'s declaration
/// order, grouped by tab. So two presets exposing the same fields build the same card.
///
/// ⚠️ **A name no field answers to is dropped here**, silently — but not *unreported*:
/// `Preset::unknown_exposed` is what names those, and it is the caller's job to print them.
/// This function's contract is "the sections that can be drawn", and a name with no field
/// cannot be one.
///
/// ⚠️ **`Item::Help` is dropped.** A filtered card showing three rows and a paragraph about the
/// whole panel is exactly the ambient explanatory prose #130 took out of this console; and the
/// paragraph would be describing controls that are not on screen. A preset panel is a control
/// surface, not a document.
///
/// 🚨 **Surface is drawable and is NOT table-homed, so its fields answer `homed: false` — and
/// that is the honest answer rather than a bug.** A column can hold Surface's card
/// (`panels::Status::Live`, drawn by `panel_surface::surface_card`), but this function walks
/// [`DECLARED`], and Surface has no declaration in it. So a preset touching `surface_mode`,
/// `palette` or `bevel` groups them under their editor *tab* and [`draw_any_field`] renders
/// them with the param's own long name — `"Surface Mode"` where the card says `"mode"`.
///
/// ⚠️ **`homed: true` would be a claim this build cannot honour.** Homing a field means drawing
/// it through the panel's own `draw_one`, which is generated from a `panel!` declaration;
/// `surface_card` is one imperative function — disclosure gates, a file dialog, a material-graph
/// loader — with no per-field entry point and its labels as string literals inside `if` arms.
/// There is no cheaper Surface rendering being withheld here.
///
/// ⚠️ **And the gap is 167 fields wide, not three.** The three the eye lands on are simply the
/// ones above `surface_card`'s first mode gate; the card touches **167 preset-capturable
/// fields** in total (95 `Generator`, 72 `Look`). Declaring a handful of them would be worse
/// than declaring none: the card would report Surface as joined while 164 of its controls still
/// fell through to a tab heading, and [`DECLARED`]'s own contract — *every* item, in the
/// editor's order — would stop being true. Joining Surface whole is **organon#124 stage 3**,
/// a stage of its own precisely because of that number, and it is the change that flips this
/// paragraph.
pub(crate) fn group_exposed(exposed: &[String]) -> Vec<Section> {
    let want: std::collections::HashSet<&str> = exposed.iter().map(|s| s.as_str()).collect();
    let mut out: Vec<Section> = Vec::new();
    let mut taken: std::collections::HashSet<&'static str> = std::collections::HashSet::new();

    for (panel, items) in DECLARED {
        let fields: Vec<&'static str> =
            items.iter().filter_map(Item::field).filter(|f| want.contains(f)).collect();
        if !fields.is_empty() {
            taken.extend(fields.iter().copied());
            out.push(Section { heading: panel.title, fields, homed: true });
        }
    }

    // Everything the joined panels did not claim, by tab, in declaration order. `tab_field_list`
    // is the whole of what this build knows about where an un-joined field belongs — a tab, and
    // no finer — which is precisely the coverage gap each panel joining `DECLARED` closes a
    // piece of. ⚠️ **Three panels have joined, not four**: Surface is drawable but undeclared,
    // so its 167 preset-capturable fields arrive here rather than above. See `group_exposed`'s
    // doc, and organon#124 stage 3.
    for (name, tab) in crate::preset::PresetValues::tab_field_list() {
        if !want.contains(name) || taken.contains(name) {
            continue;
        }
        match out.iter_mut().find(|s| !s.homed && s.heading == tab.label()) {
            Some(section) => section.fields.push(name),
            None => out.push(Section { heading: tab.label(), fields: vec![name], homed: false }),
        }
    }
    out
}

/// **Draw a preset-built card's body** — the sections [`group_exposed`] worked out, each control
/// through the best rendering this build has for it.
///
/// A field a joined panel homes is drawn by [`draw_field`], so it arrives with the editor's own
/// short label and width. Everything else falls to [`draw_any_field`] and the param's own name.
/// ⚠️ **Nothing is skipped**, and that is the property worth keeping: a panel claiming to show
/// what a preset changed must not quietly leave a control out of it.
pub(crate) fn draw_preset_panel(
    ui: &mut egui::Ui,
    w: f32,
    p: &OrganicMathParams,
    sink: &mut Sink,
    sections: &[Section],
) {
    for group in sections {
        section(ui, &format!("— {} —", group.heading));
        for field in &group.fields {
            if !draw_field(ui, w, p, sink, field) {
                draw_any_field(ui, w, p, sink, field);
            }
        }
    }
}

/// The body of a declared panel, if this build has one — the Console's dispatch from a slug to
/// a rendering, and the editor's from a card to its contents.
///
/// ⚠️ **Keyed on the slug rather than on a pointer**, because `organon-console` addresses a
/// panel by the word a person typed and never holds two `Panel` values it could compare.
pub(crate) fn body_for(
    slug: &str,
) -> Option<fn(&mut egui::Ui, f32, &OrganicMathParams, &mut Sink)> {
    match slug {
        s if s == shadows::PANEL.slug => Some(shadows::body),
        s if s == lighting::PANEL.slug => Some(lighting::body),
        s if s == bloom::PANEL.slug => Some(bloom::body),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::PresetValues;

    /// 🚨 **Every field the table names has a writable mirror.** A row whose field is not in
    /// `PresetValues` is a control that draws, drags and moves nothing in the Console — the
    /// exact failure `/panel` was retired for. The macros make a *missing* field a compile
    /// error; what they cannot see is a field that exists on `OrganicMathParams` and not on the
    /// mirror, because `@item` only ever touches the name.
    ///
    /// ⚠️ **Eleven of the Look tab's 514 fields fail this deliberately** — Temporal's seven and
    /// four of Ray Tracing's — because they are per-display quality settings that presets do
    /// not capture (`params.rs`: *"Per-display, NOT preset-captured"*). They are why no panel
    /// containing them may be declared here until the table gains a way to say
    /// "editor-only"; this test is what stops one being added by accident in the meantime.
    #[test]
    fn every_row_the_table_names_has_a_writable_mirror() {
        let pv = PresetValues::capture_params_only(&OrganicMathParams::default());
        let json = serde_json::to_value(&pv).expect("PresetValues serializes");
        let obj = json.as_object().expect("PresetValues is a struct");
        for (panel, items) in DECLARED {
            for item in items.iter() {
                if let Some(f) = item.field() {
                    assert!(
                        obj.contains_key(f),
                        "`{}` on the {} panel has no `PresetValues` field — in the Console it \
                         would be a control that moves nothing",
                        f,
                        panel.slug
                    );
                }
            }
        }
    }

    /// A declared panel is a `Live` one and vice versa. **The two directions fail differently
    /// and both are silent**, which is why this asserts the pair rather than one of them: a
    /// `Live` panel with no declaration is a card the Console opens to nothing, and a declared
    /// panel still marked `Declared` is a body nobody can reach.
    ///
    /// 🚨 **Surface is subtracted from the `Live` side, and the exemption is the whole reason
    /// this build homes three panels while drawing four.** It is `Live` — a column may hold its
    /// card — but its body is `panel_surface::surface_card`, one hand-written function that
    /// `OrganonPanels::draw` special-cases *before* consulting [`body_for`]. So it is a `Live`
    /// panel with no declaration that is nevertheless not "a card the Console opens to nothing",
    /// which is the single case this assertion cannot express and therefore excludes by slug.
    ///
    /// ⚠️ **The exemption ends at organon#124 stage 3**, which declares Surface's 167 rows and
    /// makes it reachable through the table like the other three. Deleting the filter before
    /// then fails this test; deleting it *after* is how the stage proves it landed.
    #[test]
    fn a_declared_panel_is_exactly_a_live_one() {
        use organon_core::panels::{Status, PANELS};
        let mut declared: Vec<&str> = DECLARED.iter().map(|(p, _)| p.slug).collect();
        let mut live: Vec<&str> = PANELS
            .iter()
            .filter(|p| p.status == Status::Live && p.slug != organon_core::panels::LOOK_SURFACE.slug)
            .map(|p| p.slug)
            .collect();
        declared.sort_unstable();
        live.sort_unstable();
        assert_eq!(
            declared, live,
            "the panel table and `panels::Status::Live` disagree about which panels this build \
             can draw — Surface excepted, whose body is hand-written in `panel_surface.rs`"
        );
    }

    /// [`DECLARED`] is hand-maintained, so it can fall behind the `panel!` calls above it.
    /// Counting is the cheapest guard Rust allows here, and it is the same one
    /// `panels::the_look_tab_is_whole` uses one layer up.
    #[test]
    fn every_declared_panel_is_in_the_index() {
        assert_eq!(DECLARED.len(), 3, "a `panel!` was added or removed without `DECLARED`");
        assert!(body_for(shadows::PANEL.slug).is_some());
        assert!(body_for("nonesuch").is_none());
    }

    /// The dispatch a preset-built panel is drawn through answers for every row of every
    /// declared panel, and for nothing else. ⚠️ It is asserted **without a `Ui`** — by name
    /// only — because the claim is about the table's coverage, not about pixels.
    #[test]
    fn the_field_dispatch_covers_exactly_the_declared_rows() {
        let named: Vec<&str> =
            DECLARED.iter().flat_map(|(_, i)| i.iter().filter_map(Item::field)).collect();
        assert_eq!(named.len(), 10, "the three declared panels draw ten controls between them");
        // No two panels may claim the same field: `draw_field` stops at the first that answers,
        // so a duplicate would silently give one of them the other's label.
        let mut seen = named.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), named.len(), "two panels claim the same field");
    }

    /// A section owns the rows that follow it, and the editor's own headings are the ones a
    /// preset panel groups by. None of the three panels declared so far has one — this pins the
    /// shape of [`Item::Section`] rather than a count, so the first panel that does have
    /// sections cannot quietly change what a section means.
    #[test]
    fn a_section_is_a_marker_and_not_a_container() {
        const SAMPLE: &[Item] = &[
            Item::Section("— shadows (Tier 1) —"),
            Item::Row { field: "rt_shadows", label: "RT shadows (key)", wide: false },
        ];
        assert_eq!(SAMPLE[0].field(), None);
        assert_eq!(SAMPLE[1].field(), Some("rt_shadows"));
    }
    // -----------------------------------------------------------------------
    // A panel built from a preset (organon#124)
    // -----------------------------------------------------------------------

    /// 🚨 **The claim the feature makes, as an assertion**: the fields a preset changed become
    /// sections of one card, headed by where the controls come from.
    #[test]
    fn a_preset_becomes_sections_headed_by_where_its_controls_come_from() {
        let sections = group_exposed(&["shadow_bias".into(), "bloom_intensity".into()]);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, organon_core::panels::LOOK_SHADOWS.title);
        assert_eq!(sections[0].fields, vec!["shadow_bias"]);
        assert!(sections[0].homed, "a joined panel homes its own field");
        assert_eq!(sections[1].heading, organon_core::panels::LOOK_BLOOM.title);
    }

    /// ⚠️ **A field no joined panel homes still gets a section**, headed by its editor *tab*.
    /// The alternative — dropping it — would make a card claiming to show what a preset changed
    /// quietly incomplete, which is the one thing it must not be. `homed` is what says which
    /// kind of section it is, and it is what a caller counts to report coverage honestly.
    #[test]
    fn a_field_with_no_joined_panel_is_grouped_by_tab_and_marked() {
        // `kal_spin` is a Look field on the Scene Kaleidoscope card, which is still `Declared`.
        let sections = group_exposed(&["kal_spin".into()]);
        assert_eq!(sections.len(), 1);
        assert!(!sections[0].homed, "no panel in the table owns it");
        assert_eq!(sections[0].fields, vec!["kal_spin"]);
        assert_ne!(
            sections[0].heading,
            organon_core::panels::LOOK_KALEIDOSCOPE.title,
            "the tab is all this build knows — naming the panel would be a guess"
        );
    }

    /// Joined panels first, then everything else by tab — so two presets exposing the same
    /// fields build the same card whatever order the names arrive in.
    #[test]
    fn the_order_is_the_editors_and_not_the_callers() {
        let a = group_exposed(&["kal_spin".into(), "shadow_bias".into()]);
        let b = group_exposed(&["shadow_bias".into(), "kal_spin".into()]);
        assert_eq!(a, b);
        assert!(a[0].homed, "a joined panel leads");
        assert!(!a[1].homed);
    }

    /// A field claimed by a joined panel is **not** repeated under its tab. The two passes are
    /// over the same field space, so a missing `taken` check would draw every homed control
    /// twice — once well and once badly.
    #[test]
    fn a_homed_field_is_not_repeated_under_its_tab() {
        let sections = group_exposed(&["shadow_bias".into()]);
        let all: Vec<&str> = sections.iter().flat_map(|s| s.fields.iter().copied()).collect();
        assert_eq!(all, vec!["shadow_bias"]);
    }

    /// ⚠️ A name no field answers to is dropped **here** — `Preset::unknown_exposed` is what
    /// reports it, and this function's contract is "the sections that can be drawn".
    #[test]
    fn a_name_with_no_field_makes_no_section() {
        assert!(group_exposed(&["no_such_param".into()]).is_empty());
        assert!(group_exposed(&[]).is_empty());
    }

    /// 🚨 **Every preset field has a control, whether or not a panel names it.** This is the
    /// property `draw_any_field` exists for, asserted without an `egui::Ui` — the dispatch is
    /// generated from `for_each_tab_field!`, so what can be checked here is that the two lists
    /// are the same list. A field in the tab partition with no arm would be a control the
    /// preset panel silently skips.
    #[test]
    fn every_preset_field_can_be_grouped() {
        let every: Vec<String> = crate::preset::PresetValues::tab_field_list()
            .into_iter()
            .map(|(n, _)| n.to_string())
            .collect();
        let sections = group_exposed(&every);
        let grouped: usize = sections.iter().map(|s| s.fields.len()).sum();
        assert_eq!(
            grouped,
            every.len(),
            "a preset-captured field fell out of the grouping and would never be drawn"
        );
    }

    /// 🚨 **Surface is `Live` and drawable and NOT table-homed** — the one place the two counts
    /// come apart, asserted rather than described. `panels::Status::Live` says a column may hold
    /// Surface's card; [`DECLARED`] says [`group_exposed`] cannot reach it, so its fields arrive
    /// under their editor *tab* with `homed: false` and draw through [`draw_any_field`] with the
    /// param's own long name.
    ///
    /// ⚠️ **This exists because three documents now state that in prose**, and the reviewer of
    /// organon#138 found the file's comments claiming the opposite count. A sentence asserting a
    /// property is not evidence; this is. It **fails the day organon#124 stage 3 declares
    /// Surface**, which is the point — that is exactly when those three paragraphs stop being
    /// true and have to be rewritten in the same change.
    #[test]
    fn surface_is_drawable_and_not_table_homed() {
        use organon_core::panels::{Status, LOOK_SURFACE, PANELS};

        // A column can hold it: it is `Live`, and `OrganonPanels::draw` special-cases its
        // hand-written body ahead of `body_for` — which does NOT answer for it.
        let surface = PANELS.iter().find(|p| p.slug == LOOK_SURFACE.slug).expect("Surface exists");
        assert_eq!(surface.status, Status::Live, "a column may hold Surface's card");
        assert!(
            body_for(LOOK_SURFACE.slug).is_none(),
            "Surface's body is `panel_surface::surface_card`, not a generated one — if this \
             starts answering, stage 3 has landed and the three prose paragraphs about the \
             three-versus-four counts are now wrong"
        );

        // The table cannot home it: no declaration, so no `draw_one`, so `homed: false`.
        assert!(
            !DECLARED.iter().any(|(p, _)| p.slug == LOOK_SURFACE.slug),
            "Surface has no `panel!` declaration"
        );
        for field in ["surface_mode", "palette", "bevel"] {
            let sections = group_exposed(&[field.to_string()]);
            assert_eq!(sections.len(), 1, "`{field}` groups into exactly one section");
            assert!(
                !sections[0].homed,
                "`{field}` is a Surface control and reports `homed: false` — honestly, because \
                 `surface_card` has no per-field entry point to draw it through"
            );
            assert_ne!(
                sections[0].heading, LOOK_SURFACE.title,
                "the heading is the editor tab, never Surface's title — naming the panel would \
                 promise a rendering this build cannot produce"
            );
        }
    }

    /// The coverage number this build reports, pinned so it moves visibly as panels join.
    /// ⚠️ **It is a measurement, not a target** — the useful thing about it is that it goes up
    /// in the same commit as a transplant, and is read off the tables rather than written here.
    ///
    /// ⚠️ **Ten comes from the THREE panels in [`DECLARED`], not from the four a column can
    /// draw.** Surface is `Live` and drawable and has no declaration, so [`group_exposed`] never
    /// reaches it and none of its 167 preset-capturable fields is counted here. Naming the
    /// wrong number in this message is what the reviewer of organon#138 caught, and it is worth
    /// a sentence rather than a word: a test message asserting a count the code does not walk
    /// reads as evidence and is not.
    #[test]
    fn the_joined_panels_home_ten_of_the_preset_field_space() {
        let every: Vec<String> = crate::preset::PresetValues::tab_field_list()
            .into_iter()
            .map(|(n, _)| n.to_string())
            .collect();
        let sections = group_exposed(&every);
        let homed: usize = sections.iter().filter(|s| s.homed).map(|s| s.fields.len()).sum();
        assert_eq!(
            homed, 10,
            "the three panels in `DECLARED` home ten controls between them — Surface is drawable \
             but undeclared, so none of its 167 fields is homed (organon#124 stage 3)"
        );
    }
}
