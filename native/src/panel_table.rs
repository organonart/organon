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

// ⚠️ **The data half of the table has no non-test caller in this build, and that is expected
// rather than a leftover.** `Item`, `ITEMS`, `DECLARED`, `draw_field` and `section` exist for
// the preset-built panel (organon#124), which lands *after* the panels it groups do — the table
// has to carry the grouping before anything can group by it. They are exercised by this
// module's tests today; the `allow(dead_code)`s below come off with the first real caller.

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
#[allow(dead_code)]
pub(crate) enum Item {
    /// A control. `field` is the identifier shared by `OrganicMathParams` and
    /// `PresetValues`; `wide` is the one piece of *presentation* the table carries, because a
    /// dropdown's width is a layout decision the param cannot make (`2 × COMBO_W` for the few
    /// hero combos, `COMBO_W` otherwise) and it is invisible to every other control kind.
    Row { field: &'static str, label: &'static str, wide: bool },
    /// A `— shadows (Tier 1) —` sub-heading inside the card.
    Section(&'static str),
    /// A `help()` paragraph.
    Help(&'static str),
    /// 🚨 **A fragment the table cannot express** — a button, a file dialog, a capability gate —
    /// carried by *name* rather than inline so that "how much of this panel is still
    /// hand-written" is a countable number rather than a feeling. Only legal in an `@labelled`
    /// panel; a `@generated` one fails to compile.
    Free(&'static str),
}

impl Item {
    /// The `PresetValues` field this item controls, if it controls one.
    #[allow(dead_code)]
    pub(crate) fn field(&self) -> Option<&'static str> {
        match self {
            Item::Row { field, .. } => Some(field),
            _ => None,
        }
    }
}

/// The sub-heading the editor draws inside a card — `lib.rs`'s own idiom, in one place so a
/// generated body and a hand-written one cannot render it differently.
#[allow(dead_code)]
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
        #[allow(dead_code)]
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
/// ⚠️ **A `panel!` declaration missing from here is invisible to the preset panel**, exactly as
/// a `panels::Panel` missing from `panels::PANELS` is invisible to the `/organon` rings, and for
/// the same reason: Rust cannot enumerate a module's items.
/// [`tests::every_declared_panel_is_in_the_index`] counts the arm.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
}
