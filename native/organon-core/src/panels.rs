//! The editor's **panel table** — every card inside a tab, as data.
//!
//! [`crate::tabs`] names the tabs; this names what is *in* one. Together they are the two
//! rings the Console's `/organon` command walks: `/organon` offers [`crate::tabs::UiTab::ALL`],
//! `/organon look` offers [`in_tab`], and `/organon look surface` opens the panel this table
//! points at.
//!
//! # 🚨 The title lives here, not at the `card()` call site
//!
//! Organon's editor draws its panels as `card(&mut c[0], "Surface", |ui| …)` inside one long
//! pass. Copying those strings into a second list here would be a table that silently stops
//! matching the editor the moment a panel is renamed or added — and the symptom is a ring
//! offering a panel that no longer exists, or missing one that does.
//!
//! So the arrow points the other way: **the editor reads its titles out of this table.** Every
//! entry below is a `pub const` and `lib.rs`'s Look-tab cards are written
//! `card(&mut c[0], panels::LOOK_SURFACE.title, …)`. A renamed panel is one edit, in one place,
//! and the compiler finds the call site.
//!
//! ⚠️ **Only the Look tab is joined that way today.** The other seven tabs are deliberately
//! *absent* from this table rather than transcribed into it: an entry whose title nothing reads
//! is exactly the un-joined copy this module exists to prevent. [`in_tab`] answers empty for
//! them and the Console says so out loud, which is honest; a list that looked complete and was
//! not would not be. A tab joins by converting its `card()` call sites, one tab at a time.
//!
//! # Slug and title are different words on purpose
//!
//! [`Panel::title`] is what the editor draws — `"Calibrated Colour (#349)"`, issue number and
//! all, because that provenance is worth keeping on screen. [`Panel::slug`] is what a person
//! types, so it is one lowercase word with no spaces: the command grammar splits on whitespace,
//! and a value with a space in it would parse as two arguments.
//!
//! ⚠️ **No slug may be a prefix of another slug on the same tab**, and
//! [`no_slug_is_a_prefix_of_another`] pins it. The Console completes a lone remaining candidate
//! automatically, so `surface` and `surface-fx` would leave `/organon look surface` permanently
//! ambiguous — the second panel makes the first one un-completable. `fx` is the answer, not a
//! longer prefix.

use crate::tabs::UiTab;

/// Whether the Console can actually draw this panel yet.
///
/// 🚨 **A ring that lists a choice it cannot honour is worse than a short ring.** Transplanting
/// a panel into the Console is per-panel work (its card body has to be lifted out of the
/// editor's one long pass), so most of this table is [`Status::Declared`] — named, addressable,
/// and honest about opening nothing. The Console says which it is *before* the choice is made,
/// in the candidate's own line, rather than after.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// The Console has a body for this panel and draws it. **[`LOOK_SURFACE`],
    /// [`LOOK_LIGHTING`], [`LOOK_SHADOWS`] and [`LOOK_BLOOM`] today** — the exact list is
    /// [`only_the_transplanted_panels_are_live`], never this sentence.
    ///
    /// ⚠️ **Surface is the odd one and the other three are the pattern.** Surface's body is
    /// hand-written (`organic-math-native`'s `panel_surface::surface_card`) because it is the
    /// one Look panel with disclosure logic and file dialogs in it. The rest are *declared* —
    /// `panel_table` holds their rows, labels, sections and help text as data, and generates
    /// the body that both products draw. That is what lets a panel built from a preset's diff
    /// group its controls the way the editor does without a second list.
    ///
    /// 🚨 **What made this hard was writing a parameter, not drawing one.** Organon's panels
    /// are `card(&mut c[0], "Surface", |ui| …)` closures over `&params.bevel` and a
    /// `nih_plug::ParamSetter`, and **there is no public way to write a param from outside
    /// `nih_plug`**: `ParamMut` and every setter on it are `pub(crate)`, `FloatParam`'s value
    /// fields are private, and nih-plug's own standalone `Wrapper` — the one type outside a
    /// host that implements `GuiContext` — lives in a private module of an upstream git
    /// dependency. A panel drawn without a write path is a panel whose knobs do nothing, which
    /// is precisely why `/panel` was retired.
    ///
    /// The way through is a **writable mirror**: `PresetValues` shares its field identifiers
    /// with `OrganicMathParams` and has its own `to_shared()`, so the Console holds one, the
    /// controls write it, and the world is driven from the difference between it and its
    /// starting state. `organic-math-native`'s `param_sink` module owns the mechanism and
    /// `CONSOLE_ARCHITECTURE.md` §1.11 owns what it costs.
    ///
    /// ⚠️ **Going `Live` is a claim about a body existing, and only that.** It does not
    /// promise the panel is pixel-identical to the editor's; §3's ledger records where the two
    /// differ.
    Live,
    /// Real, named, and in the ring — but the Console cannot draw it yet. Every panel but
    /// [`LOOK_SURFACE`].
    Declared,
}

/// One panel of Organon's editor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Panel {
    /// Which tab draws it — the first ring.
    pub tab: UiTab,
    /// The word a person types. Lowercase, no whitespace. See the module doc.
    pub slug: &'static str,
    /// The heading the editor draws, and the one this table owns. See the module doc.
    pub title: &'static str,
    pub status: Status,
}

impl Panel {
    /// The whole line `/organon <tab> <slug>` — what typing this panel's name produces, and
    /// what a receipt echoes.
    pub fn command(&self) -> String {
        format!("/organon {} {}", self.tab.word(), self.slug)
    }
}

/// Shorthand for the table below: everything but the slug/title/status is the tab.
const fn look(slug: &'static str, title: &'static str, status: Status) -> Panel {
    Panel { tab: UiTab::Look, slug, title, status }
}

// ---------------------------------------------------------------------------
// The Look tab, in the editor's own column-then-row order
// ---------------------------------------------------------------------------

/// Node geometry, palette and the material maps — **the first panel to be transplanted**, and
/// the only [`Status::Live`] entry in this table. Its body is
/// `organic-math-native`'s `panel_surface::surface_card`, called by Organon's editor and by
/// the Console alike.
pub const LOOK_SURFACE: Panel = look("surface", "Surface", Status::Live);
pub const LOOK_COLOUR: Panel = look("colour", "Calibrated Colour (#349)", Status::Declared);
pub const LOOK_MATERIAL: Panel = look("material", "Material", Status::Declared);
pub const LOOK_SURFACE_FX: Panel = look("fx", "Surface FX", Status::Declared);
pub const LOOK_LIGHTING: Panel = look("lighting", "Lighting (Direct)", Status::Live);
pub const LOOK_IBL: Panel = look("ibl", "Environment (IBL)", Status::Declared);
pub const LOOK_SHADOWS: Panel = look("shadows", "Cast Shadows (#152)", Status::Live);
pub const LOOK_AO: Panel = look("ao", "Ambient Occlusion", Status::Declared);
pub const LOOK_REFLECTIONS: Panel = look("reflections", "Reflections (SSR)", Status::Declared);
pub const LOOK_GI: Panel = look("gi", "Global Illumination", Status::Declared);
pub const LOOK_SSGI: Panel = look("ssgi", "Screen-Space GI (#152)", Status::Declared);
pub const LOOK_VXGI: Panel = look("vxgi", "Voxel GI (#152)", Status::Declared);
pub const LOOK_RT: Panel = look("raytracing", "Ray Tracing — Hardware (#195)", Status::Declared);
pub const LOOK_BIOLUMINESCENCE: Panel =
    look("bioluminescence", "Bioluminescence", Status::Declared);
pub const LOOK_SKIN: Panel = look("skin", "Reaction-Diffusion Skin", Status::Declared);
pub const LOOK_POST: Panel = look("post", "Post FX (#152)", Status::Declared);
pub const LOOK_KALEIDOSCOPE: Panel =
    look("kaleidoscope", "Scene Kaleidoscope (#361)", Status::Declared);
pub const LOOK_INSTRUMENTATION: Panel =
    look("instrumentation", "Instrumentation (#391)", Status::Declared);
pub const LOOK_TEMPORAL: Panel =
    look("temporal", "Temporal — TAA / Motion Blur (#152)", Status::Declared);
pub const LOOK_PARTICLES: Panel = look("particles", "Particle Aura", Status::Declared);
pub const LOOK_INK: Panel = look("ink", "Fluid Ink (#182)", Status::Declared);
pub const LOOK_LIQUID: Panel = look("liquid", "Liquid (#182 T3)", Status::Declared);
pub const LOOK_LIQUID_MATERIAL: Panel = look("lmat", "Liquid Material", Status::Declared);
pub const LOOK_COUPLING: Panel = look("coupling", "Fluid Coupling (#182 T4)", Status::Declared);
pub const LOOK_BLOOM: Panel = look("bloom", "Bloom", Status::Live);

/// Every panel this table knows, in tab order then editor order.
///
/// organon#217 T3 — the glyph ring's look controls (cell width, extrusion, gap, emission
/// gain, faceplate, backplane, bevel, face crown) and T6's capsule core. Declared, not
/// transplanted: its body is a `card()` in the editor's Look pass.
pub const LOOK_TEXT: Panel = look("text", "PBR Text (#217)", Status::Declared);
/// ⚠️ A `pub const` above that is missing from this array is **unreachable** — the editor would
/// still draw its title, but no ring would offer it. [`the_look_tab_is_whole`] counts the arm,
/// which is the cheapest guard available; Rust cannot enumerate the constants of a module.
pub const PANELS: &[Panel] = &[
    LOOK_SURFACE,
    LOOK_COLOUR,
    LOOK_MATERIAL,
    LOOK_SURFACE_FX,
    LOOK_LIGHTING,
    LOOK_IBL,
    LOOK_SHADOWS,
    LOOK_AO,
    LOOK_REFLECTIONS,
    LOOK_GI,
    LOOK_SSGI,
    LOOK_VXGI,
    LOOK_RT,
    LOOK_BIOLUMINESCENCE,
    LOOK_SKIN,
    LOOK_POST,
    LOOK_TEXT,
    LOOK_KALEIDOSCOPE,
    LOOK_INSTRUMENTATION,
    LOOK_TEMPORAL,
    LOOK_PARTICLES,
    LOOK_INK,
    LOOK_LIQUID,
    LOOK_LIQUID_MATERIAL,
    LOOK_COUPLING,
    LOOK_BLOOM,
];

/// The panels of one tab, in editor order. **Empty is a real answer** — see the module doc on
/// why the other seven tabs are absent rather than transcribed.
pub fn in_tab(tab: UiTab) -> Vec<&'static Panel> {
    PANELS.iter().filter(|p| p.tab == tab).collect()
}

/// The panel a `(tab, slug)` pair names, or `None` if that tab has no such panel.
///
/// ⚠️ **The pair is checked, not just the slug.** `surface` is a Look panel; `/organon motion
/// surface` names nothing, and answering it with the Look panel because the word matched
/// somewhere would open a panel on a tab that does not contain it.
pub fn find(tab: UiTab, slug: &str) -> Option<&'static Panel> {
    PANELS.iter().find(|p| p.tab == tab && p.slug.eq_ignore_ascii_case(slug))
}

/// The panel a slug names on its own, with no tab in hand.
///
/// 🚨 **The one lookup that does not take a tab, and it is sound only because of
/// [`one_tab_owns_each_slug`].** The Console's panel-stack verb (`console stack add surface`)
/// spends both of its argument rings on an action and a panel, so it has no room for a tab
/// word — and there is no tab to *use*, because the stack is a column of panels rather than a
/// walk of Organon's hierarchy. "The first match" is therefore "the match", the same
/// arrangement `region::Layout::region_holding` uses for a content kind that may exist once.
///
/// ⚠️ **When a second tab joins the table and repeats a slug, that test fails**, and this
/// function is what has to change — the honest fix then is to give the stack verb a tab, which
/// is a third ring it does not have, or to make the slugs globally unique. Do not quietly widen
/// this to "the first one, whichever tab it is on": that would open a panel from a tab nobody
/// named, which is the exact failure [`find`]'s doc refuses.
pub fn find_by_slug(slug: &str) -> Option<&'static Panel> {
    PANELS.iter().find(|p| p.slug.eq_ignore_ascii_case(slug))
}

/// Every slug in the table, deduplicated, in table order. The declared value space of the
/// `panel` argument — a *union* across tabs, because a command schema has one value list and
/// the tab is not known when the schema is built. [`find`] is what checks the pair.
pub fn slugs() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for panel in PANELS {
        if !out.contains(&panel.slug) {
            out.push(panel.slug);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The count the editor's Look tab actually draws, as of the transplant. If a card is added
    /// there and this is not, the new panel is unreachable from `/organon` — see [`PANELS`].
    #[test]
    fn the_look_tab_is_whole() {
        assert_eq!(in_tab(UiTab::Look).len(), 26);
    }

    /// The rule that makes `/organon look su` complete to `surface` at all. See the module doc:
    /// a slug that is a prefix of another slug on the same tab makes the shorter one
    /// permanently ambiguous, and the Console's auto-completion silently stops firing.
    #[test]
    fn no_slug_is_a_prefix_of_another() {
        for a in PANELS {
            for b in PANELS {
                if a.tab == b.tab && a.slug != b.slug {
                    assert!(
                        !b.slug.starts_with(a.slug),
                        "`{}` is a prefix of `{}` on the {} tab — the shorter one can never \
                         auto-complete",
                        a.slug,
                        b.slug,
                        a.tab.word()
                    );
                }
            }
        }
    }

    #[test]
    fn slugs_are_lowercase_single_words() {
        for panel in PANELS {
            assert_eq!(panel.slug, panel.slug.to_ascii_lowercase(), "{}", panel.slug);
            assert!(
                !panel.slug.contains(char::is_whitespace),
                "`{}` has whitespace — the command grammar would read it as two arguments",
                panel.slug
            );
        }
    }

    /// 🚨 **The property [`find_by_slug`] rests on**, and the thing that has to be re-decided
    /// the day a second tab is joined: no two panels anywhere in the table share a slug. The
    /// per-tab rule above is about *completion*; this one is about a lookup with no tab at all.
    #[test]
    fn one_tab_owns_each_slug() {
        assert_eq!(
            slugs().len(),
            PANELS.len(),
            "two panels share a slug — `find_by_slug` can no longer answer, and the stack \
             verb it serves has no ring to spare for a tab"
        );
        for panel in PANELS {
            assert_eq!(find_by_slug(panel.slug), Some(panel), "{}", panel.slug);
        }
        assert_eq!(find_by_slug("SURFACE"), Some(&LOOK_SURFACE), "the lookup folds case");
        assert_eq!(find_by_slug("nonesuch"), None);
    }

    /// The pair, not the word. See [`find`].
    #[test]
    fn find_checks_the_tab_as_well_as_the_slug() {
        assert_eq!(find(UiTab::Look, "surface"), Some(&LOOK_SURFACE));
        assert_eq!(find(UiTab::Motion, "surface"), None);
        assert_eq!(find(UiTab::Look, "nonesuch"), None);
    }

    /// 🚨 **Exactly which panels claim to be drawable**, spelled out rather than counted. A
    /// panel goes [`Status::Live`] in the *same change* that gives the Console a body for it,
    /// and this assertion has to be edited by name to let that happen — so the honesty ledger
    /// in `CONSOLE_ARCHITECTURE.md` cannot fall behind the code without a failing test.
    ///
    /// It was `no_panel_is_live_yet` and asserted the list was empty; Console #7 transplanted
    /// Look ▸ Surface, so the test that guards the claim becomes the list itself.
    #[test]
    fn only_the_transplanted_panels_are_live() {
        let live: Vec<&str> =
            PANELS.iter().filter(|p| p.status == Status::Live).map(|p| p.slug).collect();
        assert_eq!(
            live,
            vec!["surface", "lighting", "shadows", "bloom"],
            "the live set changed without this test being told"
        );
    }

    #[test]
    fn a_panel_names_its_own_command() {
        assert_eq!(LOOK_SURFACE.command(), "/organon look surface");
    }
}
