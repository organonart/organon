//! The console's **parameter bridge** (issue #4 Tier 3, Leaf B).
//!
//! One job: turn the engine's parameter table into the facts a *control descriptor*
//! needs — id, label, kind, range, default, print style, variants — **without a fourth
//! hand-written range table.** `doc/console_discover_schema.md`'s invariant I2 is the
//! whole reason this module exists:
//!
//! > Descriptors are generated from the parameter table, never hand-written.
//!
//! That is not a style preference. The tree already had three hand-written mirrors of
//! `params.rs` (`agent::id_range`, `clip::RANGES`, and the published
//! `doc/reference/parameters.md`) and **9 of 45 ids had drifted** — `trans_amp_*` by 10×
//! on the maximum. A descriptor built on a wrong range puts every knob in the wrong place
//! and lands every agent-set value somewhere other than it meant, and it looks
//! approximately right, so nobody notices. The pre-Tier-3 gate
//! (`cli.rs::taper_round_trips_against_the_engine_range`) made those three true; this
//! module is what stops a fourth from being born.
//!
//! # How the generation works — three joins, all machine-checked
//!
//! 1. **Slot list → field.** [`crate::param_table`]'s `param_block!` macro gained a
//!    `@facts` arm beside its `@cat` / `@from_param` / `@from_preset` arms. It emits code
//!    that touches `p.<field>` **at the slot's declared type**, so a renamed field or a
//!    retyped param is a *build error*, not a silent disagreement. The walk in
//!    [`slot_facts`] names blocks, never fields.
//! 2. **Field → bounds.** Every `min`/`max`/`default` here is read off the real param
//!    object: `Param::preview_plain(0.0)` / `preview_plain(1.0)` /
//!    `default_plain_value()`. **No bound is restated in this file.**
//!    `OrganicMathParams::default()` builds all 1372 host params with no host, no audio
//!    thread and no GPU, which is what makes the whole module headless.
//! 3. **Field → wire id.** `params.rs`'s `#[id = "tax"]` attributes are invisible to
//!    `param_table.rs`, so the two id namespaces (Rust field names, which the CLI, the
//!    agent catalog and the docs speak; nih-plug wire ids, which presets, hosts and MIDI
//!    speak) had **no table joining them**. This module builds that bridge by
//!    **pointer identity**: `Param::as_ptr()` on the generated accessor against the
//!    `ParamPtr` in `Params::param_map()`. Zero hand-written pairs — the same object
//!    reached two ways is the same address, or it is not the same object.
//!
//! # The id set
//!
//! Exactly `cli::catalog_entries()` — `agent::core_catalog()` ∪ `agent::ACTUATABLE_IDS`.
//! [`slot_facts`] deliberately walks a *superset* of blocks and the catalog does the
//! filtering, so adding a param to a walked block cannot silently change the descriptor
//! set.
//!
//! ⚠️ **Two ids have no `param_block!` home at all: `scale_amp` and `tempo`.** Both are
//! `ACTUATABLE_IDS` entries and both are plain `FloatParam` fields on
//! `OrganicMathParams` (`params.rs:4030`, `:5952`) that reach `Shared` through the
//! hand-written packing in `params.rs::to_shared` rather than through a migrated block
//! (#103 is incomplete — the blocks migrate one at a time). They are covered by
//! [`orphan_facts`], which is a *join*, not a table: it still reads the bounds off the
//! param object and still breaks the build if a field is renamed or retyped. **When
//! either lands in a `param_block!`, delete its line there** — see the comment at that
//! function.

use std::collections::HashMap;

use nih_plug::prelude::{
    BoolParam, Enum, EnumParam, FloatParam, IntParam, Param, ParamPtr, Params,
};

use crate::params::OrganicMathParams;

// ===========================================================================
// The vocabulary
// ===========================================================================

/// What a control *is*, as the schema's `kind` field spells it.
///
/// This comes from the `param_block!` slot list, **not** from `cli::catalog_entries()`,
/// which hard-codes `"num"` for the four ids that live outside the curated core blocks —
/// so it calls `bell_physical` (a `BoolParam`) a number. See `kinds_match_the_slot_lists`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactKind {
    Float,
    Int,
    Bool,
    Enum,
}

impl FactKind {
    /// The schema's wire spelling (`doc/console_discover_schema.md` §Kinds).
    pub fn as_str(self) -> &'static str {
        match self {
            FactKind::Float => "float",
            FactKind::Int => "int",
            FactKind::Bool => "bool",
            FactKind::Enum => "enum",
        }
    }
}

/// How a value prints — the schema's `format` object.
///
/// `Magnitude` is the engine's actual rule (`params.rs::v2s_va`), which a fixed decimal
/// count cannot express: it renders `2160` as `2160.000` and `0.005` as `0.01`, both
/// visibly wrong beside the editor.
///
/// ⚠️ Only load-bearing for [`FactKind::Float`] and [`FactKind::Int`]. A bool prints
/// `On`/`Off` and an enum prints a variant name; for those the descriptor's `variants`
/// (or the bool itself) is the print path, and `format` is carried only so the field is
/// never absent. `format_matches_the_parameter_itself` asserts the law for exactly the
/// two kinds it holds for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatStyle {
    Fixed { decimals: u8 },
    Magnitude,
}

/// Everything a control descriptor needs that the *parameter table* can answer.
///
/// Deliberately **not** a descriptor: no `value` (that is live state, read from `Shared`
/// by the emitter), no `widget`, no `mapped`, no layout. Leaf A composes those.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlFacts {
    /// The `params.rs` field name — the id the CLI, the agent and the docs all speak.
    pub id: &'static str,
    /// The nih-plug `#[id]` string, **joined by pointer identity, never typed by hand**.
    /// `None` means the param is genuinely absent from `param_map()`.
    pub wire_id: Option<String>,
    /// `Param::name()`.
    pub label: String,
    /// `agent::param_desc(id)` — the curated gloss, compile-guarded by
    /// `every_documented_param_has_a_gloss`.
    pub help: Option<&'static str>,
    pub kind: FactKind,
    /// `Param::preview_plain(0.0)`, in the **display domain**. Bools are 0..1; enums are
    /// 0..=(variants − 1).
    pub min: f32,
    /// `Param::preview_plain(1.0)`.
    pub max: f32,
    /// `Param::default_plain_value()`.
    pub default: f32,
    pub format: FormatStyle,
    /// `(id, label)` per enum variant — id is the lowercased label, matching
    /// `cli::resolve_enum`'s case-insensitive name match. Empty for non-enums.
    pub variants: Vec<(String, String)>,
}

// ===========================================================================
// The generated walk
// ===========================================================================

/// One slot's facts as the macro produces them, before the catalog filter and the
/// wire-id join. Carries the `ParamPtr` that makes the join possible.
///
/// Crate-internal on purpose: the pointer is only valid while the `OrganicMathParams`
/// it came from is alive, and [`ControlFacts`] — the thing that leaves this module —
/// carries no pointer at all.
pub(crate) struct SlotFacts {
    pub(crate) id: &'static str,
    pub(crate) kind: FactKind,
    pub(crate) label: String,
    pub(crate) min: f32,
    pub(crate) max: f32,
    pub(crate) default: f32,
    pub(crate) format: FormatStyle,
    pub(crate) variants: Vec<(String, String)>,
    pub(crate) ptr: ParamPtr,
}

/// A float slot. The range **is** `preview_plain` at the two ends of normalized space.
pub(crate) fn float_facts(id: &'static str, q: &FloatParam) -> SlotFacts {
    SlotFacts {
        id,
        kind: FactKind::Float,
        label: q.name().to_string(),
        min: q.preview_plain(0.0),
        max: q.preview_plain(1.0),
        default: q.default_plain_value(),
        format: FormatStyle::Magnitude,
        variants: Vec::new(),
        ptr: q.as_ptr(),
    }
}

/// An int slot. The actuation lane carries every id as an `f32` regardless, so the
/// bounds widen to `f32` here rather than at every reader.
pub(crate) fn int_facts(id: &'static str, q: &IntParam) -> SlotFacts {
    SlotFacts {
        id,
        kind: FactKind::Int,
        label: q.name().to_string(),
        min: q.preview_plain(0.0) as f32,
        max: q.preview_plain(1.0) as f32,
        default: q.default_plain_value() as f32,
        format: FormatStyle::Fixed { decimals: 0 },
        variants: Vec::new(),
        ptr: q.as_ptr(),
    }
}

/// A bool slot. A `BoolParam` carries no range of its own; the lane spells it 0/1.
pub(crate) fn bool_facts(id: &'static str, q: &BoolParam) -> SlotFacts {
    SlotFacts {
        id,
        kind: FactKind::Bool,
        label: q.name().to_string(),
        min: 0.0,
        max: 1.0,
        default: q.default_plain_value() as u8 as f32,
        format: FormatStyle::Fixed { decimals: 0 },
        variants: Vec::new(),
        ptr: q.as_ptr(),
    }
}

/// An enum slot. An `EnumParam` is an `IntParam` over variant **indices**, so its top is
/// `variants().len() - 1` — exactly the bound `cam_path` got wrong, by one, in the
/// direction that admits a variant which has never existed.
pub(crate) fn enum_facts<E: Enum + PartialEq>(id: &'static str, q: &EnumParam<E>) -> SlotFacts {
    let names = <E as Enum>::variants();
    SlotFacts {
        id,
        kind: FactKind::Enum,
        label: q.name().to_string(),
        min: 0.0,
        max: names.len().saturating_sub(1) as f32,
        default: <E as Enum>::to_index(q.default_plain_value()) as f32,
        format: FormatStyle::Fixed { decimals: 0 },
        variants: names
            .iter()
            .map(|n| (n.to_lowercase(), (*n).to_string()))
            .collect(),
        ptr: q.as_ptr(),
    }
}

/// Walk every block that can contribute a catalogued id.
///
/// The nine curated blocks are **the same list `agent::core_catalog()` walks**, in the
/// same order; `pack_matcol` and `pack_bell` join them because two `ACTUATABLE_IDS`
/// entries (`mat_hue`, `bell_physical`) live in blocks the curated prompt does not walk.
/// The extra ids those two blocks contribute are dropped by the catalog filter in
/// [`control_facts_for`], which is the point: the walk is generous, the catalog decides.
pub(crate) fn slot_facts(p: &OrganicMathParams) -> Vec<SlotFacts> {
    let mut out = Vec::new();

    // --- the curated core (mirrors `agent::core_catalog`) -----------------------
    crate::param_table::pack_loop_count::facts(p, &mut out);
    crate::param_table::pack_rot_amp::facts(p, &mut out);
    crate::param_table::pack_rot_mod::facts(p, &mut out);
    crate::param_table::pack_trans_amp::facts(p, &mut out);
    crate::param_table::pack_trans_mod::facts(p, &mut out);
    crate::param_table::pack_lighting::facts(p, &mut out);
    crate::param_table::pack_pbr::facts(p, &mut out);
    crate::param_table::pack_surface_fx::facts(p, &mut out);
    crate::param_table::pack_camera::facts(p, &mut out);
    // organon#217 T3 — the PBR text look, its held camera, the capsule core.
    crate::param_table::pack_glyph::facts(p, &mut out);
    crate::param_table::pack_glyph_cam::facts(p, &mut out);
    crate::param_table::pack_capsule::facts(p, &mut out);
    // organon#217 T13 / #240 — the glyph-lights, in the core because they bring nothing extra.
    crate::param_table::pack_manylight::facts(p, &mut out);
    crate::param_table::pack_restir::facts(p, &mut out);

    // --- blocks holding an actuatable id outside the curated core ---------------
    crate::param_table::pack_matcol::facts(p, &mut out); // mat_hue
    crate::param_table::pack_bell::facts(p, &mut out); // bell_physical
    // organon#217 T13 / #240 — the dark room's switches and the halation, whose blocks the
    // prompt does not walk (28 route-less ids between them; see `agent::core_catalog`).
    crate::param_table::pack_atmosphere::facts(p, &mut out); // atmos_enabled
    crate::param_table::pack_fx::facts(p, &mut out); // fx_enabled
    crate::param_table::pack_finishing::facts(p, &mut out); // hal_amount

    orphan_facts(p, &mut out);
    out
}

/// 🚨 **The three catalogued ids with no `param_block!` slot list anywhere.**
///
/// `scale_amp` (`params.rs:4030`), `tempo` (`params.rs:5952`) and — since organon#217
/// T13 / #240 — `bg_visible` (a `u32` scalar on `Shared`, `params.rs::to_shared`'s
/// `bg_visible: self.bg_visible.value() as u32`) are `ACTUATABLE_IDS` entries — an agent
/// can set them, the CLI lists them, `doc/reference/parameters.md` documents them — but
/// none appears in any `param_block!` in `param_table.rs`. They still reach `Shared`
/// through the hand-written packing in `params.rs::to_shared`, because #103's block
/// migration is deliberately incremental and has not reached them.
///
/// So the generated walk **cannot** see them, and covering them is a two-line join. It is
/// a join and not a table: both bounds are still read off the real param object by
/// [`float_facts`], and `&p.scale_amp` typed as `&FloatParam` still fails the build if the
/// field is renamed or retyped. Nothing is restated here that `params.rs` also says.
///
/// **Delete the corresponding line the moment either id lands in a `param_block!`** —
/// [`slot_facts`] would then push it twice, and `the_walk_yields_each_id_once` is the
/// failure that tells you to. (It has to be checked on the *walk*: [`control_facts_for`]
/// funnels the slots through a `HashMap` keyed by id and then iterates
/// `cli::catalog_entries()`, so by the time the facts exist a duplicate has already been
/// silently collapsed and no assertion downstream of it can see one.)
fn orphan_facts(p: &OrganicMathParams, out: &mut Vec<SlotFacts>) {
    out.push(float_facts("scale_amp", &p.scale_amp));
    out.push(float_facts("tempo", &p.tempo));
    out.push(bool_facts("bg_visible", &p.bg_visible));
}

// ===========================================================================
// The public bridge
// ===========================================================================

/// The control facts for every catalogued id, built from a fresh headless parameter set.
///
/// `OrganicMathParams::default()` needs no host, no audio thread and no GPU, so this is
/// callable from a test, a CLI process, or the console — anywhere.
pub fn control_facts() -> Vec<ControlFacts> {
    let p = OrganicMathParams::default();
    control_facts_for(&p)
}

/// The same, against a caller-owned parameter set (a live plugin instance, say), so the
/// labels and defaults come from the object actually in play.
///
/// Ordering follows `cli::catalog_entries()` so the CLI, the docs table and a descriptor
/// listing are three renderings of one order.
pub fn control_facts_for(p: &OrganicMathParams) -> Vec<ControlFacts> {
    let slots = slot_facts(p);
    let by_id: HashMap<&'static str, &SlotFacts> = slots.iter().map(|s| (s.id, s)).collect();
    let wire: HashMap<ParamPtr, String> = p
        .param_map()
        .into_iter()
        .map(|(wire_id, ptr, _group)| (ptr, wire_id))
        .collect();

    crate::cli::catalog_entries()
        .into_iter()
        .filter_map(|(id, _kind, _range)| by_id.get(id).copied())
        .map(|s| ControlFacts {
            id: s.id,
            wire_id: wire.get(&s.ptr).cloned(),
            label: s.label.clone(),
            help: crate::agent::param_desc(s.id),
            kind: s.kind,
            min: s.min,
            max: s.max,
            default: s.default,
            format: s.format,
            variants: s.variants.clone(),
        })
        .collect()
}

/// Print a display-domain value exactly as the engine's own editor would.
///
/// [`FormatStyle::Magnitude`] is `params.rs::v2s_va` transcribed, not re-derived: decimals
/// 0 at |v| ≥ 1000, 1 at ≥ 100, 2 at ≥ 10, else 3; trailing zeros and a trailing `.`
/// trimmed; an empty result or `"-0"` renders as `"0"`. `format_matches_the_parameter_itself`
/// asserts the two agree over a sweep of every float and int in the catalog, which is what
/// makes this a copy that cannot quietly stop being one.
pub fn format_value(style: FormatStyle, v: f32) -> String {
    match style {
        FormatStyle::Fixed { decimals } => {
            let dp = decimals as usize;
            format!("{v:.dp$}")
        }
        FormatStyle::Magnitude => {
            let a = v.abs();
            let dp = if a >= 1000.0 {
                0
            } else if a >= 100.0 {
                1
            } else if a >= 10.0 {
                2
            } else {
                3
            };
            let s = format!("{v:.dp$}");
            let trimmed = if dp > 0 {
                s.trim_end_matches('0').trim_end_matches('.')
            } else {
                s.as_str()
            };
            if trimmed.is_empty() || trimmed == "-0" {
                "0".to_string()
            } else {
                trimmed.to_string()
            }
        }
    }
}

// ===========================================================================
// Tests — headless: no GPU, no egui context, no host
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent;
    use crate::params::{CamPath, IndexedEnum, MaterialType, Palette};
    use std::collections::BTreeSet;

    /// Read a param's `(min, max, default)` back out of a *type-erased* `ParamPtr`, which
    /// is a genuinely different route to the same object than the generated accessor —
    /// `param_map()` is derive-generated from `params.rs`, the walk is macro-generated from
    /// `param_table.rs`. Agreement between them is worth something; a second read through
    /// the same accessor would not be.
    ///
    /// # Safety
    /// `Params::param_map`'s contract: the pointers are valid for as long as the object
    /// they came from is. Every caller below keeps `p` alive across the loop.
    unsafe fn bounds_via_ptr(ptr: ParamPtr) -> (f32, f32, f32) {
        fn int_like<P: Param<Plain = i32>>(q: &P) -> (f32, f32, f32) {
            (
                q.preview_plain(0.0) as f32,
                q.preview_plain(1.0) as f32,
                q.default_plain_value() as f32,
            )
        }
        match ptr {
            ParamPtr::FloatParam(q) => {
                let q = &*q;
                (
                    q.preview_plain(0.0),
                    q.preview_plain(1.0),
                    q.default_plain_value(),
                )
            }
            ParamPtr::IntParam(q) => int_like(&*q),
            ParamPtr::EnumParam(q) => int_like(&*q),
            ParamPtr::BoolParam(q) => {
                let q = &*q;
                (0.0, 1.0, q.default_plain_value() as u8 as f32)
            }
        }
    }

    /// `id → ParamPtr`, via the wire id this module joined. Built once per test that
    /// needs to reach the engine a second way.
    fn ptr_by_id(p: &OrganicMathParams, facts: &[ControlFacts]) -> Vec<(&'static str, ParamPtr)> {
        let wire: HashMap<String, ParamPtr> = p
            .param_map()
            .into_iter()
            .map(|(wire_id, ptr, _group)| (wire_id, ptr))
            .collect();
        facts
            .iter()
            .filter_map(|f| f.wire_id.as_ref().map(|w| (f.id, wire[w])))
            .collect()
    }

    /// The fact set is **exactly** the catalogued id set — no gaps, no extras.
    ///
    /// A gap means a control the console cannot render and the agent cannot see; an extra
    /// means the walk drifted away from what the CLI actually speaks. Both are silent
    /// without this.
    #[test]
    fn every_catalogued_id_has_facts() {
        let facts = control_facts();
        let got: BTreeSet<&str> = facts.iter().map(|f| f.id).collect();
        let want: BTreeSet<&str> = crate::cli::catalog_entries()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();

        let missing: Vec<&&str> = want.difference(&got).collect();
        let extra: Vec<&&str> = got.difference(&want).collect();
        assert!(
            missing.is_empty(),
            "catalogued ids with no generated facts (add the block that owns them to \
             `slot_facts`, or the id has no `param_block!` home — see `orphan_facts`): \
             {missing:?}"
        );
        assert!(extra.is_empty(), "facts for ids the catalog does not speak: {extra:?}");

        // One fact per catalogued id. This is a property of the *emit* (one pass over
        // `catalog_entries()`, which is itself unique), NOT a duplicate check on the walk
        // — see `the_walk_yields_each_id_once` for that.
        assert_eq!(facts.len(), got.len(), "the emit produced two facts for one id");
        assert!(facts.len() >= 45, "the actuatable set alone is 45; got {}", facts.len());
    }

    /// The walk visits each id exactly once.
    ///
    /// This is the check `orphan_facts` points at, and it has to live here rather than on
    /// the finished facts: [`control_facts_for`] keys the slots by id in a `HashMap` and
    /// then emits one entry per `cli::catalog_entries()` row, so a slot pushed twice is
    /// collapsed before any downstream assertion could notice. The failure it exists to
    /// catch is a real and expected future event — `scale_amp` or `tempo` landing in a
    /// `param_block!` while its hand-joined line in `orphan_facts` is still there, which
    /// would leave the block's copy and the orphan's copy racing for the same id in map
    /// insertion order.
    #[test]
    fn the_walk_yields_each_id_once() {
        let p = OrganicMathParams::default();
        let mut seen: BTreeSet<&'static str> = BTreeSet::new();
        let mut dupes: Vec<&'static str> = Vec::new();
        for s in slot_facts(&p) {
            if !seen.insert(s.id) {
                dupes.push(s.id);
            }
        }
        assert_eq!(
            dupes,
            Vec::<&str>::new(),
            "ids walked twice — if one of these is `scale_amp` or `tempo`, its block has \
             migrated and its line in `orphan_facts` must go; otherwise two blocks in \
             `slot_facts` overlap"
        );
    }

    /// Every bound equals what the parameter object itself says, read back through the
    /// type-erased `param_map()` pointer.
    #[test]
    fn facts_ranges_agree_with_the_engine() {
        let p = OrganicMathParams::default();
        let facts = control_facts_for(&p);
        for (id, ptr) in ptr_by_id(&p, &facts) {
            let f = facts.iter().find(|f| f.id == id).unwrap();
            let (lo, hi, def) = unsafe { bounds_via_ptr(ptr) };
            assert_eq!(f.min, lo, "{id}: min");
            assert_eq!(f.max, hi, "{id}: max");
            assert_eq!(f.default, def, "{id}: default");
            assert!(f.min <= f.max, "{id}: min above max");
            assert!(
                f.default >= f.min && f.default <= f.max,
                "{id}: default {} outside {}..{}",
                f.default,
                f.min,
                f.max
            );
            match f.kind {
                FactKind::Bool => assert_eq!((f.min, f.max), (0.0, 1.0), "{id}: a bool is 0..1"),
                FactKind::Enum => assert_eq!(
                    (f.min, f.max),
                    (0.0, (f.variants.len() - 1) as f32),
                    "{id}: an enum is 0..=(variants - 1)"
                ),
                _ => {}
            }
        }
    }

    /// The generated view and the actuation lane must not re-diverge.
    ///
    /// The pre-Tier-3 gate (`cli::taper_round_trips_against_the_engine_range`) made
    /// `agent::id_range` true against `params.rs`. This asserts the *third* reader agrees
    /// with it. **If this fails, the gate's join and this walk disagree about which field
    /// an id names — find out which is wrong. Do not loosen the assert.**
    #[test]
    fn facts_ranges_agree_with_the_actuation_tables() {
        let mut checked = 0;
        for f in control_facts() {
            let Some((lo, hi)) = agent::id_range(f.id) else { continue };
            assert_eq!(
                (f.min, f.max),
                (lo, hi),
                "{}: generated facts say {:?}, `agent::id_range` says {:?}",
                f.id,
                (f.min, f.max),
                (lo, hi)
            );
            checked += 1;
        }
        assert_eq!(
            checked,
            agent::ACTUATABLE_IDS.len(),
            "every actuatable id should have been range-checked"
        );
    }

    /// Kinds come from the slot lists, and Enum beats "has an `id_range`".
    #[test]
    fn kinds_match_the_slot_lists() {
        let facts = control_facts();
        let kind_of = |id: &str| facts.iter().find(|f| f.id == id).map(|f| f.kind);

        for c in agent::core_catalog() {
            let want = match c.kind {
                agent::SlotKind::Num => FactKind::Float,
                agent::SlotKind::Int => FactKind::Int,
                agent::SlotKind::Flag => FactKind::Bool,
                agent::SlotKind::Enum => FactKind::Enum,
            };
            assert_eq!(kind_of(c.id), Some(want), "{} disagrees with its slot list", c.id);
        }

        // The documented precedence: `cam_path` is `SlotKind::Enum` in `pack_camera` AND
        // carries an `agent::id_range`. Enum wins — it renders as a choice, not a dial.
        assert!(agent::id_range("cam_path").is_some(), "the premise: cam_path has a range");
        assert_eq!(kind_of("cam_path"), Some(FactKind::Enum));

        // The ids outside the curated core. `cli::catalog_entries()` hard-coded "num" for
        // all of them until W19 (organon#217), which was wrong for `bell_physical` — a
        // `BoolParam` — and would have been wrong for three of the dark room's flags and
        // its `IntParam` count too. It now reads the kind off this same walk, so pin both
        // halves: the facts, and the catalog agreeing with them.
        assert_eq!(kind_of("bell_physical"), Some(FactKind::Bool));
        assert_eq!(kind_of("mat_hue"), Some(FactKind::Float));
        assert_eq!(kind_of("scale_amp"), Some(FactKind::Float));
        assert_eq!(kind_of("tempo"), Some(FactKind::Float));
        assert_eq!(kind_of("atmos_enabled"), Some(FactKind::Bool));
        assert_eq!(kind_of("bg_visible"), Some(FactKind::Bool), "the orphan scalar is a flag");
        assert_eq!(kind_of("fx_enabled"), Some(FactKind::Bool));
        assert_eq!(kind_of("hal_amount"), Some(FactKind::Float));
        assert_eq!(kind_of("ml_count"), Some(FactKind::Int));
        let catalog_kind = |id: &str| {
            crate::cli::catalog_entries()
                .into_iter()
                .find(|(i, _, _)| *i == id)
                .map(|(_, k, _)| k)
        };
        assert_eq!(catalog_kind("bell_physical"), Some("flag"), "the catalog reads the slot list");
        assert_eq!(catalog_kind("bg_visible"), Some("flag"));
        assert_eq!(catalog_kind("atmos_enabled"), Some("flag"));
        assert_eq!(catalog_kind("ml_count"), Some("int"));
        assert_eq!(catalog_kind("hal_amount"), Some("num"));
        assert_eq!(catalog_kind("tempo"), Some("num"));
    }

    /// Every joined wire id resolves back to the same parameter — same name, same default.
    ///
    /// This is what licenses the pointer-identity bridge. It also pins the `None` count:
    /// a param genuinely absent from `param_map()` is an honest `None`, but a *rise* in
    /// that count means the join broke, and a count nobody asserted would hide it.
    #[test]
    fn wire_ids_round_trip() {
        let p = OrganicMathParams::default();
        let facts = control_facts_for(&p);
        let wire: HashMap<String, ParamPtr> = p
            .param_map()
            .into_iter()
            .map(|(wire_id, ptr, _group)| (wire_id, ptr))
            .collect();

        let mut unjoined: Vec<&str> = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for f in &facts {
            let Some(w) = f.wire_id.as_deref() else {
                unjoined.push(f.id);
                continue;
            };
            assert!(seen.insert(w), "{}: wire id {w} joined to two params", f.id);
            let ptr = *wire
                .get(w)
                .unwrap_or_else(|| panic!("{}: wire id {w} is not in param_map()", f.id));
            let name = unsafe { ptr.name() };
            assert_eq!(name, f.label, "{}: wire id {w} names a different param", f.id);
            let (lo, hi, def) = unsafe { bounds_via_ptr(ptr) };
            assert_eq!((lo, hi, def), (f.min, f.max, f.default), "{}: wire id {w}", f.id);
        }

        // Every catalogued param is a real host param, so every one of them joins. If this
        // ever becomes non-zero, the ids are listed rather than counted so the report says
        // WHICH — a bare number would just get bumped.
        assert_eq!(unjoined, Vec::<&str>::new(), "ids with no `param_map()` entry");
    }

    /// The schema's round-trip law: `format(v)` equals the parameter's own printing.
    ///
    /// This is the test that stops silently-wrong printing. `v2s_va` is transcribed into
    /// [`format_value`] rather than reachable (it is a private closure factory in
    /// `params.rs`), and a transcription with nothing checking it is just a fourth copy
    /// with extra steps.
    ///
    /// Floats and ints only — a bool prints `On`/`Off` and an enum prints a variant name,
    /// neither of which `format` describes. See [`FormatStyle`]'s note.
    #[test]
    fn format_matches_the_parameter_itself() {
        let p = OrganicMathParams::default();
        let facts = control_facts_for(&p);
        let mut swept = 0;
        for (id, ptr) in ptr_by_id(&p, &facts) {
            let f = facts.iter().find(|f| f.id == id).unwrap();
            if !matches!(f.kind, FactKind::Float | FactKind::Int) {
                continue;
            }
            for step in 0..=20 {
                let n = step as f32 / 20.0;
                // The display-domain value, then back to normalized — the law as the
                // schema states it, so a descriptor consumer holding only `v` is covered.
                let (v, want) = unsafe {
                    match ptr {
                        ParamPtr::FloatParam(q) => {
                            let q = &*q;
                            let v = q.preview_plain(n);
                            (v, q.normalized_value_to_string(q.preview_normalized(v), false))
                        }
                        ParamPtr::IntParam(q) => {
                            let q = &*q;
                            let v = q.preview_plain(n);
                            (
                                v as f32,
                                q.normalized_value_to_string(q.preview_normalized(v), false),
                            )
                        }
                        _ => unreachable!("kind filter above"),
                    }
                };
                assert_eq!(
                    format_value(f.format, v),
                    want,
                    "{id} at normalized {n} (plain {v}): format_value disagrees with the param"
                );
                swept += 1;
            }
        }
        assert!(swept > 500, "the sweep covered only {swept} points");
    }

    /// Variant ids and labels come from the engine's `Enum` impl, never a literal list.
    #[test]
    fn enum_variants_match_the_engine() {
        let facts = control_facts();
        let variants_of = |id: &str| {
            facts
                .iter()
                .find(|f| f.id == id)
                .unwrap_or_else(|| panic!("{id} has no facts"))
                .variants
                .clone()
        };
        for (id, names) in [
            ("cam_path", CamPath::labels()),
            ("mat_type", MaterialType::labels()),
            ("palette", Palette::labels()),
        ] {
            let got = variants_of(id);
            assert_eq!(got.len(), names.len(), "{id}: variant count");
            for (i, name) in names.iter().enumerate() {
                assert_eq!(got[i].1, *name, "{id}: variant {i} label");
                // The id convention `cli::resolve_enum` matches on: the lowercased label.
                assert_eq!(got[i].0, name.to_lowercase(), "{id}: variant {i} id");
                assert_eq!(
                    crate::cli::resolve_enum::<CamPath>("0").is_ok(),
                    true,
                    "sanity: resolve_enum is the consumer of this convention"
                );
            }
        }

        // Non-enums carry no variants, and every enum carries at least two (a one-variant
        // choice is a constant, and its range would be 0..0).
        for f in &facts {
            match f.kind {
                FactKind::Enum => assert!(f.variants.len() >= 2, "{}: {:?}", f.id, f.variants),
                _ => assert!(f.variants.is_empty(), "{} is not an enum", f.id),
            }
        }
    }

    /// `format_value`'s own edge cases, stated as the rule reads. The law test above
    /// proves the rule matches the engine; this proves the boundaries are where the
    /// prose says they are, including the two the prose calls out by name.
    #[test]
    fn magnitude_formatting_edges() {
        let m = FormatStyle::Magnitude;
        assert_eq!(format_value(m, 2160.0), "2160");
        assert_eq!(format_value(m, 999.99), "1000"); // rounds up ACROSS the boundary
        assert_eq!(format_value(m, 108.0), "108");
        assert_eq!(format_value(m, 99.999), "100");
        assert_eq!(format_value(m, 10.5), "10.5");
        assert_eq!(format_value(m, 0.005), "0.005");
        assert_eq!(format_value(m, 0.0), "0");
        assert_eq!(format_value(m, -0.0001), "0"); // "-0.000" → "-0" → "0"
        assert_eq!(format_value(m, -99.99), "-99.99");
        assert_eq!(format_value(FormatStyle::Fixed { decimals: 0 }, 128.0), "128");
        assert_eq!(format_value(FormatStyle::Fixed { decimals: 0 }, -8.0), "-8");
    }

    /// Facts carry the curated gloss where one exists, and the whole catalogued set has
    /// one (`cli::every_documented_param_has_a_gloss` guards the same union from the
    /// other end — this asserts the *bridge* actually carries it through).
    #[test]
    fn help_comes_from_the_curated_glosses() {
        for f in control_facts() {
            assert_eq!(f.help, agent::param_desc(f.id), "{}", f.id);
            assert!(f.help.is_some(), "{} reaches a descriptor with no gloss", f.id);
            assert!(!f.label.is_empty(), "{} has no label", f.id);
        }
    }
}
