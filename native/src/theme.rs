//! **The house style** (#542 Tiers 1–2) — Organon's design tokens, painted chrome, egui theme,
//! and control-row grid.
//!
//! Everything that decides how the editor *looks* resolves here. The canonical specification is
//! [`doc/organon_mind_visual_reference.md`](../../doc/organon_mind_visual_reference.md), built
//! around `doc/ui/reference-images/BlueSlateUILook.png`; section numbers below (§2, §5, …) refer
//! to it.
//!
//! # The one thing to understand before editing this file
//!
//! The reference's character is **not** in its palette. The spec says so in §0, and it is worth
//! repeating where the code lives: implement the colour table, ship flat fills, and you get a
//! cheap dark UI while concluding the palette was the problem. A flat `#212A30` panel with a
//! 1 px border is not the reference. The difference is entirely in:
//!
//! - **three-layer surfaces** — base colour, a *weak* gradient, and a grain overlay (§3, §4),
//! - **three-stop** header gradients whose middle is lighter than both ends, so they read as
//!   convex rolled metal rather than as a gradient (§5),
//! - a **one-pixel inset top highlight**, which is the single line that makes the UI feel
//!   machined into a faceplate (§6),
//! - **compressed tonal steps** — 5–15 RGB values between hierarchy levels, never more (§14).
//!
//! Every one of those is small enough to look like a rounding error in a diff and load-bearing
//! enough that removing it collapses the look. The [`paint`] module exists to make them cheap.
//!
//! # Why the row grid is a pure function
//!
//! It is the part worth testing, and it is testable only if it does not need an `egui::Context`
//! — the same reasoning (and the same house pattern) as `mind_shell::layout_workstation`. A
//! cloud session has no GPU and can open no window, so arithmetic that decides whether 1057
//! control rows are legible had better be checkable without one.
//!
//! **The rule it encodes: the label yields before the bar does.** The pre-#542 grid held the
//! label at a fixed 62 pt and let the slider absorb every shortfall, which at the default
//! 1280×860 window drove the bar to its 24 pt floor — a control too short to aim at, next to a
//! label reading `connecti…`. Both halves of the row were sacrificed to protect a constant.
//! Here the label is the elastic segment: it opens to [`LABEL_W_MAX`] when there is room and
//! closes to [`LABEL_W_MIN`] — *exactly the old width, so this can never be a regression* — to
//! keep [`BAR_COMFORT`] of draggable slider. Same instinct as the workstation docks: furniture
//! yields before the middle does.

// The palette tokens below are zero-argument accessors that kept their SCREAMING_CASE
// names, so ~30 call sites read `theme::BONE()` exactly as they read `theme::BONE`.
#![allow(non_snake_case)]

use nih_plug_egui::egui;

// ═════════════════════════════════════════════════════════════════════════════
// Tokens — the palette (§2)
// ═════════════════════════════════════════════════════════════════════════════
//
// A low-saturation **blue-slate** system: every surface is gray with a small excess of blue, and
// almost nothing is visibly blue. Most of the UI would survive conversion to grayscale, which is
// why it reads as an instrument rather than as a themed dark mode.
//
// **The rule that generates these values** — and the one to derive any new surface from, rather
// than picking by eye — is `blue − red ∈ 8..=15`, with green between them. `palette_is_cool`
// asserts it. Drift here is how a coherent family quietly becomes a pile of dark colours.

// #551 Tier 1 made these **runtime state**. Each is a zero-argument function reading the live
// `theme_config::Palette` (a wait-free `ArcSwap` load, so it is safe per-widget), and the shipped
// defaults are byte-identical to the constants #542 Tier 2 landed — a fresh install renders
// exactly what it did before the config layer existed.
//
// They keep their SCREAMING_CASE names on purpose: ~30 call sites in `lib.rs` read them, and
// `theme::BONE` → `theme::BONE()` is a one-character diff that leaves every one of those lines
// saying the same thing it said before.
use crate::theme_config::{palette, to_col};

/// Deepest wells — the darkest surface in the system.
pub fn WELL_DEEP() -> egui::Color32 { to_col(palette().well_deep) }
/// Input interiors: text edits, value boxes, slider troughs. Recessed, nearly black (§8).
pub fn WELL() -> egui::Color32 { to_col(palette().well) }
/// Outer shell — the application body behind everything else.
pub fn SHELL() -> egui::Color32 { to_col(palette().shell) }
/// Main workspace field — the primary cool-gray background.
pub fn WORKSPACE() -> egui::Color32 { to_col(palette().workspace) }
/// Dock and panel background.
pub fn PANEL() -> egui::Color32 { to_col(palette().panel) }
/// Card body — the surface control rows sit on.
pub fn CARD() -> egui::Color32 { to_col(palette().card) }
/// Raised surfaces: button faces, lighter regions.
pub fn RAISED() -> egui::Color32 { to_col(palette().raised) }
/// Card-header midpoint — the *bright* stop of the §5 three-stop gradient.
pub fn CARD_HEADER() -> egui::Color32 { to_col(palette().card_header) }
/// Cool graphite outline — the default 1 px border.
pub fn HAIRLINE() -> egui::Color32 { to_col(palette().hairline) }
/// Brushed-steel edge — borders that need to read as lit.
pub fn EDGE_STRONG() -> egui::Color32 { to_col(palette().edge_strong) }
/// Desaturated instrument blue — selection. Oxidized steel, not a brand blue (§9).
pub fn SELECTED() -> egui::Color32 { to_col(palette().selected) }

/// Primary text — silver-white. **Never pure white**: holding the top of the value range in
/// reserve is what lets live data mean something when it finally appears (§2).
pub fn BONE() -> egui::Color32 { to_col(palette().bone) }
/// Secondary text — control labels.
pub fn TITANIUM() -> egui::Color32 { to_col(palette().titanium) }
/// Tertiary text — hints, units, provenance tags, disabled copy.
pub fn MUTED() -> egui::Color32 { to_col(palette().muted) }

/// Teal analytical accent — provenance marks and status glyphs (§11).
pub fn TEAL() -> egui::Color32 { to_col(palette().teal) }
/// Amber — retained from the web app (`#ffb547`) but **only for live data and status** (§11).
///
/// Under the warm shell this painted every card title at once, which is why nothing read as
/// emphasis: an accent spent everywhere is not an accent. The blue-slate system is stricter
/// still — strong chroma belongs to data, selection, and status, and every saturated pixel spent
/// on chrome devalues the data.
pub fn AMBER() -> egui::Color32 { to_col(palette().amber) }
/// Filled portion of a slider track — neutral steel, deliberately unsaturated (§13).
pub fn BAR_FILL() -> egui::Color32 { to_col(palette().bar_fill) }

// ═════════════════════════════════════════════════════════════════════════════
// Tokens — the row grid
// ═════════════════════════════════════════════════════════════════════════════

/// Widest a control row's label may open. Sized so the longest names actually in the param
/// table — `conductivity`, `dendrite count`, `connections`, `ranvier nodes` — set whole at the
/// body size, which is the entire point of making the segment elastic.
pub const LABEL_W_MAX: f32 = 132.0;
/// Narrowest a label may close to. **This is the pre-#542 fixed `LABEL_W`**, deliberately: at any
/// width the grid may reach this floor but never go under it, so the worst case here is exactly
/// the old behaviour and the change cannot regress a cramped window.
pub const LABEL_W_MIN: f32 = 62.0;
/// Slider width protected before the label is allowed to shrink. Below roughly this, a drag
/// stops being aimable and the row is decoration.
pub const BAR_COMFORT: f32 = 120.0;
/// Widest a slider bar grows. Past this, surplus becomes breathing room before the trailing
/// button rather than more track, so a wide window gets a *composed* row and not a smear.
pub const BAR_MAX_W: f32 = 190.0;
/// Absolute floor for the bar, for windows too narrow to honour [`BAR_COMFORT`].
pub const BAR_MIN_W: f32 = 56.0;
/// Fixed width of the value readout beside each bar. `params.rs::v2s_va` caps every float
/// readout at ~6 characters (`-999.9`, `-99.99`, `2160`), which fits with button padding.
pub const VALUE_W: f32 = 58.0;
/// Actual width of the trailing ⟲ reset `small_button`.
pub const RESET_BTN_W: f32 = 24.0;
/// Fixed width for an enum dropdown, so combo boxes don't stretch across the row.
pub const COMBO_W: f32 = 75.0;

/// Inter-item gaps egui inserts across a **slider** row: five items —
/// `label │ bar │ value │ flexible gap │ reset` — therefore four gaps.
const ROW_GAPS: f32 = 4.0;

/// Inter-item gaps across a **combo** row: four items — `label │ combo │ flexible gap │ reset`
/// — therefore three. A combo row draws **no value box** (`param_combo_sized` has no
/// `value_box` call), which is the whole reason [`combo_grid`] cannot simply reuse
/// [`row_grid`]'s leftovers: one fewer item means one fewer gap *and* `VALUE_W` of budget that
/// nothing spends.
const COMBO_GAPS: f32 = 3.0;

/// How a single control row divides its width, in logical points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowGrid {
    /// Leading label. The elastic segment.
    pub label_w: f32,
    /// Slider track (`ParamSlider` rendered `without_value()`).
    pub bar_w: f32,
    /// Value readout box.
    pub value_w: f32,
    /// Flexible breathing room between the readout and the trailing button.
    pub gap: f32,
    /// Trailing ⟲ reset button.
    pub reset_w: f32,
    /// How many inter-item gaps egui will insert across this row — [`ROW_GAPS`] for a slider
    /// row, [`COMBO_GAPS`] for a combo row.
    ///
    /// Carried explicitly because [`total`](Self::total) is otherwise a *fiction*: the pre-review
    /// version hard-coded four gaps and always counted `value_w`, so it reported the same total
    /// for both row kinds even though a combo row physically drew 64 pt shorter. The test that
    /// was supposed to catch exactly that misalignment compared two of those fictions and
    /// passed. Anything that claims to be a row's width has to know what the row actually draws.
    pub gaps: f32,
}

impl RowGrid {
    /// Total width the row will occupy — **what egui actually lays out**, including the
    /// `gaps × spacing` it inserts and counting `value_w` only when the row has one.
    pub fn total(&self, spacing: f32) -> f32 {
        self.label_w + self.bar_w + self.value_w + self.gap + self.reset_w + self.gaps * spacing
    }
}

/// Divide a control row's usable width into the fixed grid every row shares.
///
/// `avail` is the usable row width measured from the owning column (`available_width()` is
/// unreliable once nested inside the card frame plus the collapsing-header body indent, so
/// callers pass it down explicitly); `spacing` is `ui.spacing().item_spacing.x`.
///
/// The label absorbs the slack and the bar is protected — see the module docs for why.
/// Everything is clamped, so the result is well-defined at any `avail`, including degenerate
/// ones: below roughly 230 pt every segment sits at its floor and the row overflows, which the
/// column's clip rect already contains (it cannot bleed into a neighbour), and which no
/// reachable window size produces anyway — `CARD_COL_MIN_W` floors a column at 280 pt.
pub fn row_grid(avail: f32, spacing: f32) -> RowGrid {
    // Everything that is not the label or the bar, plus the gaps egui will insert.
    let overhead = VALUE_W + RESET_BTN_W + ROW_GAPS * spacing;
    // Open the label only with width left over after the bar has its comfortable size.
    let label_w = (avail - BAR_COMFORT - overhead).clamp(LABEL_W_MIN, LABEL_W_MAX);
    // Whatever remains is the bar's, up to the point where more track stops helping.
    let flexible = (avail - label_w - overhead).max(0.0);
    let bar_w = flexible.clamp(BAR_MIN_W, BAR_MAX_W);
    let gap = (flexible - bar_w).max(0.0);
    RowGrid { label_w, bar_w, value_w: VALUE_W, gap, reset_w: RESET_BTN_W, gaps: ROW_GAPS }
}

/// The row grid for a **discrete-choice** row, whose control is a fixed-width `ComboBox` rather
/// than an elastic bar.
///
/// Shares [`row_grid`]'s label and trailing button so a combo row and a slider row in the same
/// card land on identical grid lines; the width the bar would have taken becomes gap.
pub fn combo_grid(avail: f32, spacing: f32, combo_w: f32) -> RowGrid {
    let g = row_grid(avail, spacing);
    // Take only the label from `row_grid` — that is what alignment needs — then size the gap so
    // the row fills `avail` exactly, given what a combo row *actually draws*.
    //
    // Deriving the gap from `row_grid`'s leftovers (`bar_w + gap`) is what the review caught: that
    // sum has `VALUE_W` already subtracted out as part of `overhead`, reserved for a `value_box`
    // this row never draws, and it assumes four gaps where there are three. Both shortfalls are
    // invisible in `RowGrid`'s own arithmetic and add to ~64 pt at the reference width, putting a
    // combo row's reset button that far left of its neighbours' — precisely the misalignment this
    // function exists to prevent.
    let gap = (avail - g.label_w - combo_w - g.reset_w - COMBO_GAPS * spacing).max(0.0);
    RowGrid { bar_w: combo_w, value_w: 0.0, gap, gaps: COMBO_GAPS, ..g }
}

// ═════════════════════════════════════════════════════════════════════════════
// Painted chrome (§3–§6, §10)
// ═════════════════════════════════════════════════════════════════════════════

/// Gradients, bevels, and the material grain — the treatments that carry the reference's
/// character (see the module docs).
///
/// All of it is `epaint` meshes and strokes: no shader, no GPU work beyond what egui already
/// does, and nothing here needs the wgpu backend that #542 Tier 3 would bring. The editor
/// renders through `egui-baseview` → `egui_glow`, and every effect in this module was chosen to
/// be reachable from that.
pub mod paint {
    use super::*;
    use egui::{Color32, Pos2, Rect, Shape, Stroke, StrokeKind};

    /// Linear interpolation between two colours in gamma space — the same space egui blends
    /// vertex colours in, so a mesh built from these matches what the GPU interpolates between
    /// them.
    pub fn lerp_col(a: Color32, b: Color32, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
    }

    /// Sample a stop list (each `(position, colour)`, positions ascending in `0..=1`) at `t`.
    pub fn sample_stops(stops: &[(f32, Color32)], t: f32) -> Color32 {
        if stops.is_empty() {
            return Color32::TRANSPARENT;
        }
        let t = t.clamp(0.0, 1.0);
        if t <= stops[0].0 {
            return stops[0].1;
        }
        for w in stops.windows(2) {
            let ((t0, c0), (t1, c1)) = (w[0], w[1]);
            if t <= t1 {
                let span = (t1 - t0).max(f32::EPSILON);
                return lerp_col(c0, c1, (t - t0) / span);
            }
        }
        stops[stops.len() - 1].1
    }

    /// A **vertical gradient** across `rect` from an arbitrary stop list.
    ///
    /// `chamfer` bevels the four corners by that many points. egui meshes have no corner radius,
    /// so a square-cornered gradient painted under a rounded border would poke out at the
    /// corners; chamfering the top and bottom rows turns the quad into an octagon that reads as
    /// rounded at the 4–6 px radii this system uses (§6). Pass `0.0` for a full-bleed surface
    /// like a dock background, where there is no rounded border to sit inside.
    pub fn gradient_v(rect: Rect, stops: &[(f32, Color32)], chamfer: f32) -> Shape {
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return Shape::Noop;
        }
        let h = rect.height();
        let c = chamfer.min(h * 0.5).max(0.0);
        let ct = c / h;
        let mut rows: Vec<(f32, f32)> = vec![(0.0, c), (ct, 0.0), (1.0 - ct, 0.0), (1.0, c)];
        // Interior stops become their own rows so the gradient bends where the spec says it
        // does rather than only at the ends.
        for &(t, _) in stops {
            if t > ct && t < 1.0 - ct {
                rows.push((t, 0.0));
            }
        }
        rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        rows.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-4);

        let mut mesh = egui::Mesh::default();
        for &(t, inset) in &rows {
            let y = rect.top() + h * t;
            let col = sample_stops(stops, t);
            mesh.colored_vertex(Pos2::new(rect.left() + inset, y), col);
            mesh.colored_vertex(Pos2::new(rect.right() - inset, y), col);
        }
        for i in 0..rows.len().saturating_sub(1) {
            let (a, b) = (2 * i as u32, 2 * i as u32 + 1);
            let (c2, d) = (a + 2, b + 2);
            mesh.add_triangle(a, b, c2);
            mesh.add_triangle(b, d, c2);
        }
        Shape::mesh(mesh)
    }

    /// The three-stop silver gradient used by panel headings and raised faces (§5).
    ///
    /// The middle stop is *lighter than both ends*, which is what makes the surface read as
    /// gently convex — rolled metal, not glossy. A two-stop gradient cannot do this; the three
    /// stops are the whole trick, and flattening it to two is the most likely way to lose the
    /// header treatment while believing it is still there.
    /// #551 Tier 1 — derived from the live header colour rather than fixed, and scaled by
    /// `depth.header_gradient` so the treatment can be dialled from full to flat. The outer
    /// stops are computed *from* the middle one, so editing "header" in the UI panel moves the
    /// whole gradient coherently instead of desynchronising its ends.
    pub fn silver_stops() -> [(f32, Color32); 3] {
        silver_stops_of(&crate::theme_config::active())
    }

    /// [`silver_stops`] against a **named** config rather than the live one.
    ///
    /// ⚠️ **The live form reads a file** — `theme_config::active()` resolves through
    /// `ThemeConfig::load()`, so a machine with a saved theme has different colours than a
    /// cloud checkout. A test that pins the shipped look therefore has to name
    /// `ThemeConfig::default()`; this is what lets it, and it is the same reasoning
    /// `surfaces()` states at the bottom of this file.
    pub fn silver_stops_of(cfg: &crate::theme_config::ThemeConfig) -> [(f32, Color32); 3] {
        let mid = to_col(cfg.palette.card_header);
        let k = cfg.depth.header_gradient.clamp(0.0, 1.0);
        // At k = 0 every stop collapses onto the midpoint: a flat fill, no gradient.
        let lip = lerp_col(mid, Color32::from_rgb(0x20, 0x29, 0x30), k);
        let foot = lerp_col(mid, Color32::from_rgb(0x2B, 0x35, 0x3D), k);
        [(0.00, lip), (HEADER_MID_STOP, mid), (1.00, foot)]
    }

    /// Where the *bright* stop sits down a header band. Above the middle, so the convexity
    /// reads as light falling from above rather than as a symmetrical bulge.
    ///
    /// ⚠️ A constant because [`super::card_header_band`] paints the band from a
    /// [`super::CardStyle`]'s three colours rather than from this list, and a second spelling of
    /// `0.42` is a place for the two to disagree about where the light is.
    pub const HEADER_MID_STOP: f32 = 0.42;

    /// [`silver_stops`] as a paintable shape.
    ///
    /// ⚠️ **Not what a card's header band paints any more** — that goes through
    /// [`super::CardStyle`], because a card is now drawn in two products' palettes. This
    /// remains the *shape* form of Organon's own stops for any other raised face that wants
    /// them, and [`super::CardStyle::organon`] is the reader that keeps the card on them.
    pub fn silver_face(rect: Rect, chamfer: f32) -> Shape {
        gradient_v(rect, &silver_stops(), chamfer)
    }

    /// The card-body gradient's two stops (§6): a weak top-to-bottom darkening over the panel
    /// colour, scaled by `depth.card_gradient`.
    ///
    /// ✏️ **A stop list rather than a shape**, since #120: [`super::CardStyle`] reads the
    /// colours and the painting happens once, in [`super::card_chrome`], against whichever
    /// palette that style came from.
    pub fn card_stops() -> [(f32, Color32); 2] {
        card_stops_of(&crate::theme_config::active())
    }

    /// [`card_stops`] against a named config — see [`silver_stops_of`] for why that exists.
    pub fn card_stops_of(cfg: &crate::theme_config::ThemeConfig) -> [(f32, Color32); 2] {
        let base = to_col(cfg.palette.card);
        let k = cfg.depth.card_gradient.clamp(0.0, 1.0);
        [
            (0.0, lerp_col(base, Color32::from_rgb(0x24, 0x2E, 0x35), k)),
            (1.0, lerp_col(base, Color32::from_rgb(0x1D, 0x25, 0x2B), k)),
        ]
    }

    /// The application shell gradient (§3) — a weak top-to-bottom darkening, full bleed.
    pub fn shell_face(rect: Rect) -> Shape {
        let mid = WORKSPACE();
        let k = crate::theme_config::active().depth.shell_gradient.clamp(0.0, 1.0);
        gradient_v(
            rect,
            &[
                (0.0, lerp_col(mid, Color32::from_rgb(0x20, 0x2A, 0x31), k)),
                (0.5, mid),
                (1.0, lerp_col(mid, Color32::from_rgb(0x19, 0x21, 0x28), k)),
            ],
            0.0,
        )
    }

    /// A **diagonal** overlay, used by the preset tiles (§10).
    ///
    /// This is the one place the diagonal is genuinely present. On the panel headings the
    /// diagonal impression is *emergent* — horizontal luminance drift plus mottling plus uneven
    /// panel lighting — and painting a literal 45° gradient there would be a misreading of the
    /// reference (§5). Here it is real, and it produces the satin-metal reflection that makes
    /// each tile read as an individual physical module.
    pub fn diagonal_sheen(rect: Rect, strength: f32) -> Shape {
        let strength = strength * crate::theme_config::active().depth.sheen.clamp(0.0, 1.0);
        if rect.width() <= 0.0 || rect.height() <= 0.0 || strength <= 0.0 {
            return Shape::Noop;
        }
        let al = |f: f32| (f * strength * 255.0).clamp(0.0, 255.0) as u8;
        let hi = Color32::from_rgba_unmultiplied(0x59, 0x6A, 0x75, al(0.20));
        let mid = Color32::from_rgba_unmultiplied(0x27, 0x31, 0x39, al(0.08));
        let lo = Color32::from_rgba_unmultiplied(0x0F, 0x16, 0x1C, al(0.22));
        // 135°: bright at top-left, dark at bottom-right, with the mid stop riding the
        // anti-diagonal so the falloff is not a flat ramp.
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(rect.left_top(), hi);
        mesh.colored_vertex(rect.right_top(), mid);
        mesh.colored_vertex(rect.left_bottom(), mid);
        mesh.colored_vertex(rect.right_bottom(), lo);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        Shape::mesh(mesh)
    }

    /// The **inset bevel** (§6): a one-pixel top highlight and a dark lower seam.
    ///
    /// The top highlight is the most important single line in this module. It is what creates
    /// the sensation that the interface has been machined into a physical faceplate; without it
    /// the same colours read as flat panels with borders.
    pub fn bevel(rect: Rect) -> Vec<Shape> {
        let d = crate::theme_config::active().depth;
        let i = rect.shrink(0.5);
        let (l, r) = (i.left() + 1.0, i.right() - 1.0);
        let mut v = Vec::with_capacity(2);
        if d.bevel_top > 0 {
            v.push(Shape::hline(l..=r, i.top(),
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xE1, 0xF0, 0xF8, d.bevel_top))));
        }
        if d.bevel_bottom > 0 {
            v.push(Shape::hline(l..=r, i.bottom(),
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, d.bevel_bottom))));
        }
        v
    }

    /// A **recessed well** (§8) — input interiors, slider troughs, plot backgrounds.
    ///
    /// The inverse of [`bevel`]: dark interior, shadow at the *top* (light comes from above, so
    /// a sunken surface is shadowed under its upper lip), faint bounce at the bottom. Raised
    /// means actionable, recessed means editable.
    pub fn well(rect: Rect, radius: u8) -> Vec<Shape> {
        let i = rect.shrink(0.5);
        vec![
            Shape::rect_filled(rect, egui::CornerRadius::same(radius), WELL()),
            Shape::hline(i.x_range(), i.top(), Stroke::new(1.5, Color32::from_rgba_unmultiplied(0, 0, 0, 166))),
            Shape::hline(i.x_range(), i.bottom(), Stroke::new(1.0, Color32::from_rgba_unmultiplied(0x78, 0x91, 0xA0, 9))),
            Shape::rect_stroke(
                rect,
                egui::CornerRadius::same(radius),
                Stroke::new(1.0, Color32::from_rgb(0x30, 0x3B, 0x43)),
                StrokeKind::Inside,
            ),
        ]
    }

    /// The **ambient key** (§3): one broad, very faint illumination centred near the upper middle
    /// of the window, raising central surfaces by a few RGB levels.
    ///
    /// This is what stops the interface reading as one uniformly black rectangle — it should feel
    /// like the front face of an instrument catching dim light, never like a visible gradient. A
    /// triangle fan: bright centre, transparent rim.
    pub fn ambient_key(rect: Rect) -> Shape {
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return Shape::Noop;
        }
        const SEGMENTS: usize = 32;
        let d = crate::theme_config::active().depth;
        let intensity = d.ambient.clamp(0.0, 1.0);
        if intensity <= 0.0 {
            return Shape::Noop;
        }
        let centre = Pos2::new(
            rect.left() + rect.width() * d.ambient_x,
            rect.top() + rect.height() * d.ambient_y,
        );
        let (rx, ry) = (rect.width() * 0.62, rect.height() * 0.52);
        let core = Color32::from_rgba_unmultiplied(0x7D, 0x96, 0xA5, (14.0 * intensity).round() as u8);
        let rim = Color32::from_rgba_unmultiplied(0x7D, 0x96, 0xA5, 0);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(centre, core);
        for i in 0..=SEGMENTS {
            let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(Pos2::new(centre.x + rx * a.cos(), centre.y + ry * a.sin()), rim);
        }
        for i in 1..=SEGMENTS {
            mesh.add_triangle(0, i as u32, i as u32 + 1);
        }
        Shape::mesh(mesh)
    }

    // ── The material grain (§4) ──────────────────────────────────────────────

    /// Edge length of the repeating fine-grain tile, in texels.
    pub(super) const GRAIN_TILE: usize = 96;
    /// Edge length of the low-frequency mottling tile. Small and stretched with linear
    /// filtering across a whole surface, which is what produces smooth blobs spanning tens or
    /// hundreds of pixels rather than visible texture.
    pub(super) const MOTTLE_TILE: usize = 24;

    /// Deterministic hash → uniform `u32`. A seeded PRNG rather than `rand`: the grain must be
    /// identical every run (it is a *material*, not an effect — an animated or per-launch-random
    /// grain would read as noise in the pejorative sense), and this avoids a new dependency.
    pub(super) fn hash32(mut x: u32) -> u32 {
        x ^= x >> 16;
        x = x.wrapping_mul(0x7FEB_352D);
        x ^= x >> 15;
        x = x.wrapping_mul(0x846C_A68B);
        x ^ (x >> 16)
    }

    /// The "lighten" texel colour.
    ///
    /// Mid-slate rather than near-white so 8-bit alpha has usable resolution. The deviation a
    /// texel produces is proportional to `colour − base` (see [`grain_image`]), so a near-white
    /// source moves ~164 levels per unit of alpha against a dark panel while this moves ~79 —
    /// twice the steps across the same range, which is what makes the grain tunable rather than
    /// all-or-nothing. Staying cool-toned also keeps it monochrome-in-feel, which §4 requires.
    pub(super) const GRAIN_LIGHT: [u8; 3] = [0x6B, 0x7A, 0x85];

    /// Build the fine-grain tile: monochrome, ≤3 RGB levels of deviation at full strength.
    ///
    /// # The pipeline this is tuned against
    ///
    /// Worth writing down, because two earlier cuts of this function were tuned against models
    /// that were wrong in *opposite* directions, and neither error is visible by inspection:
    ///
    /// ```text
    /// stored = from_rgba_unmultiplied(colour, a)   // premultiply
    /// frag   = v_rgba_in_gamma * stored            // componentwise, gamma-encoded
    /// out    = frag_rgb + dst * (1 - frag_a)       // ONE / ONE_MINUS_SRC_ALPHA
    /// ```
    ///
    /// **The blend is in gamma space, not linear.** `egui_glow::Painter` calls
    /// `disable(FRAMEBUFFER_SRGB)` and sets `srgb_textures = false` — in 0.31 *and* 0.33 — so
    /// nothing is ever decoded to linear. An earlier revision of this comment claimed linear
    /// compositing and tuned the alphas to a linear model; that was simply wrong.
    ///
    /// **egui 0.33 also changed the premultiply.** 0.31 did it gamma-aware
    /// (sRGB→linear→×α→sRGB), which inflated the stored value of light texels enormously at low
    /// alpha — that, not the blend, is what made the very first cut come out ~8 levels too
    /// bright. 0.33 does a plain `round(value × α/255)`, which collapses the whole thing to:
    ///
    /// ```text
    /// deviation ≈ strength × (α/255) × (colour − base)
    /// ```
    ///
    /// Linear in alpha, and symmetric: a light texel lifts by its distance above the surface, a
    /// black one darkens by the surface's own value. Balancing the two families is therefore just
    /// `mean(α_light) × (colour_light − base) ≈ mean(α_dark) × base`, which at `GRAIN_LIGHT` over
    /// a `#212A30` panel wants roughly a 1:2 ratio — hence `1..=9` against `1..=18`.
    ///
    /// The ranges are searched so the **peak** lands just under the 3 levels §4 allows
    /// (2.89) and the **mean** within 0.02 of zero: the surface gets texture without getting
    /// lighter or darker. `grain_is_balanced` asserts both against the model above.
    pub(super) fn grain_image(seed: u32) -> egui::ColorImage {
        let n = GRAIN_TILE;
        let mut px = Vec::with_capacity(n * n * 4);
        for i in 0..(n * n) {
            let h = hash32(i as u32 ^ seed);
            if h & 1 == 0 {
                // Lighten: 1..=9. Roughly half the dark range, because a light texel travels
                // `colour − base` per unit alpha while a black one only travels `base`.
                let a = (1 + (h >> 8) % 9) as u8;
                px.extend_from_slice(&[GRAIN_LIGHT[0], GRAIN_LIGHT[1], GRAIN_LIGHT[2], a]);
            } else {
                // Darken: 1..=18.
                px.extend_from_slice(&[0x00, 0x00, 0x00, (1 + (h >> 8) % 18) as u8]);
            }
        }
        egui::ColorImage::from_rgba_unmultiplied([n, n], &px)
    }

    /// Build the broad-mottling tile — the same idea at a much lower frequency and weaker
    /// amplitude, stretched (not tiled) across a surface so it varies over hundreds of pixels.
    /// This is what gives the powder-coated / anodized character rather than plain grain.
    pub(super) fn mottle_image(seed: u32) -> egui::ColorImage {
        let n = MOTTLE_TILE;
        let mut px = Vec::with_capacity(n * n * 4);
        for i in 0..(n * n) {
            let h = hash32(i as u32 ^ seed ^ 0x85EB_CA6B);
            // Deliberately quieter than the fine grain on both sides (§4: mottling under 4%,
            // grain 2–3%) — if the low-frequency layer were the louder one it would read as
            // blotching rather than as anodizing.
            if h & 1 == 0 {
                let a = (1 + (h >> 8) % 4) as u8;
                px.extend_from_slice(&[GRAIN_LIGHT[0], GRAIN_LIGHT[1], GRAIN_LIGHT[2], a]);
            } else {
                px.extend_from_slice(&[0x00, 0x00, 0x00, (1 + (h >> 8) % 8) as u8]);
            }
        }
        egui::ColorImage::from_rgba_unmultiplied([n, n], &px)
    }

    /// The material layers for `rect`: fine grain tiled at 1:1, plus broad mottling stretched
    /// across the whole surface. Two textured quads — cheap enough for every panel every frame.
    pub fn grain(ctx: &egui::Context, rect: Rect) -> Vec<Shape> {
        if rect.width() <= 1.0 || rect.height() <= 1.0 {
            return Vec::new();
        }
        let m = crate::theme_config::active().material;
        let mut out = Vec::with_capacity(2);
        let tex = super::material_textures(ctx);

        // Strength is a **vertex-colour multiplier on the baked tile**, not a re-bake: the tile
        // holds the §4-legal maximum and this scales it down. That is what makes the grain
        // slider drag smoothly at 60fps instead of rebuilding a texture atlas per frame — and
        // it is why `material_key` deliberately ignores strength.
        // **`from_rgba_premultiplied`, deliberately.** egui_glow's fragment shader is
        // `v_rgba_in_gamma * texture_in_gamma` over the *stored* bytes
        // (`v_rgba_in_gamma = a_srgba / 255.0`), so the vertex colour's bytes ARE the scale
        // factor. The tiles are premultiplied, so fading one uniformly needs all four channels
        // carrying the same k — which `from_rgba_premultiplied(k,k,k,k)` states outright.
        //
        // Under egui **0.31** the alternative was actively wrong:
        // `from_rgba_unmultiplied(255,255,255,a)` premultiplied through a *gamma-aware* LUT and
        // yielded ≈`(145,145,145,71)` at k = 0.28 — rgb scaled twice as fast as alpha. **0.33
        // changed that constructor to a plain multiply**, so the two now agree. Keeping the
        // explicit form means this does not silently depend on which of those two behaviours the
        // pinned egui happens to have.
        let tint = |k: f32| {
            let a = (k.clamp(0.0, 1.0) * 255.0).round() as u8;
            Color32::from_rgba_premultiplied(a, a, a, a)
        };

        if m.grain > 0.0 {
            // UVs run 1:1 with points so the tile never scales with the surface, and `Repeat`
            // wrapping lets one quad cover any rect.
            let tile = m.grain_scale.max(4.0);
            let uv = Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(rect.width() / tile, rect.height() / tile),
            );
            let mut fine = egui::Mesh::with_texture(tex.grain.id());
            fine.add_rect_with_uv(rect, uv, tint(m.grain));
            out.push(Shape::mesh(fine));
        }
        if m.mottle > 0.0 {
            // One stretched quad, linear-filtered into smooth low-frequency variation.
            let span = m.mottle_scale.max(0.05);
            let mut broad = egui::Mesh::with_texture(tex.mottle.id());
            broad.add_rect_with_uv(
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(span, span)),
                tint(m.mottle),
            );
            out.push(Shape::mesh(broad));
        }
        out
    }
}

/// The two cached material textures (§4), built once per `egui::Context`.
#[derive(Clone)]
struct MaterialTextures {
    grain: egui::TextureHandle,
    mottle: egui::TextureHandle,
    /// The `ThemeConfig::material_key` these were baked for. A mismatch rebuilds; equality
    /// reuses. Strength is **not** part of that key, so dragging the grain slider is free.
    key: u64,
}

/// Fetch (building once) the grain + mottling textures for this context.
///
/// Cached in context data rather than a process-wide `OnceLock` for the same reason the font
/// install is: a host can open two editor instances, each with its own `Context` and its own
/// texture manager, and a texture handle from one is meaningless in the other.
fn material_textures(ctx: &egui::Context) -> MaterialTextures {
    let id = egui::Id::new("organon_theme_material");
    let key = crate::theme_config::active().material_key();
    if let Some(t) = ctx.data(|d| d.get_temp::<MaterialTextures>(id)) {
        if t.key == key {
            return t;
        }
    }
    let seed = crate::theme_config::active().material.seed;
    // `Repeat` so one quad can tile the fine grain over any surface; `Nearest` magnification
    // keeps a grain texel exactly one point, which is what makes it read as material rather
    // than as a blurred wash.
    let grain = ctx.load_texture(
        "organon_grain",
        paint::grain_image(seed),
        egui::TextureOptions {
            magnification: egui::TextureFilter::Nearest,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::Repeat,
            mipmap_mode: None,
        },
    );
    // Linear magnification is the point here: it is what turns 24×24 random texels into smooth
    // blobs spanning hundreds of pixels.
    let mottle = ctx.load_texture(
        "organon_mottle",
        paint::mottle_image(seed),
        egui::TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::ClampToEdge,
            mipmap_mode: None,
        },
    );
    let t = MaterialTextures { grain, mottle, key };
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

// ═════════════════════════════════════════════════════════════════════════════
// Installing the theme
// ═════════════════════════════════════════════════════════════════════════════

/// Inter (latin subset), OFL 1.1 — vendored for the #135 capture overlay and reused here
/// rather than adding a font dependency; see `organon-world/src/overlay/FONTS.txt`.
///
/// ⚠️ **organon#49 T4c-ii — the path reaches into `organon-world` on purpose.** The asset
/// directory travelled with `overlay.rs`, which is the module that owns it (`include_bytes!`
/// resolves against the includer, so they cannot be separated). This file is the *other*
/// reader, and a second copy of two font binaries is a worse answer than a relative path:
/// duplicated assets drift silently, and the licence file that documents their provenance
/// lives beside the originals.
const FONT_REGULAR: &[u8] = include_bytes!("../organon-world/src/overlay/font_regular.ttf");
/// Inter Bold — same provenance.
const FONT_BOLD: &[u8] = include_bytes!("../organon-world/src/overlay/font_bold.ttf");

/// Marks a context as already having had its font atlas built, so [`install`] can be called
/// unconditionally every frame.
const FONTS_INSTALLED: &str = "organon_theme_fonts";

/// Apply the house style. Cheap and idempotent — safe to call every frame, which is what
/// `lib.rs` does.
pub fn install(ctx: &egui::Context) {
    let id = egui::Id::new(FONTS_INSTALLED);
    let done = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if !done {
        install_fonts(ctx);
        ctx.data_mut(|d| d.insert_temp(id, true));
    }
    ctx.style_mut(apply_style);
}

/// Register Inter and put it at the **front** of both font families.
///
/// Front, not replacing: the vendored Inter is a *latin subset*, while the editor leans on
/// glyphs well outside it — `⟲ ● ▼ ↳ ✓ ⌘ ↑ ↓ ⊞ ·` and more. Prepending keeps egui's stock fonts
/// as fallback, so Inter sets the text and the symbols still resolve. Replacing the family
/// outright would render every one of those as tofu.
fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontFamily};
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("organon".into(), std::sync::Arc::new(FontData::from_static(FONT_REGULAR)));
    fonts.font_data.insert("organon-bold".into(), std::sync::Arc::new(FontData::from_static(FONT_BOLD)));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.insert(0, "organon".to_owned());
    }
    fonts
        .families
        .insert(FontFamily::Name("organon-bold".into()), vec!["organon-bold".into(), "organon".into()]);
    ctx.set_fonts(fonts);
}

/// The type ramp (§12). Four working levels doing four jobs — the hierarchy the pre-#542 UI had
/// none of, where a card title, a control label and a numeric readout were all the same size.
fn apply_text_styles(s: &mut egui::Style) {
    use egui::{FontFamily::Proportional, FontId, TextStyle};
    s.text_styles = [
        // Panel titles and dock headings.
        (TextStyle::Heading, FontId::new(13.5, Proportional)),
        // Control labels and general body copy.
        (TextStyle::Body, FontId::new(12.0, Proportional)),
        // Button faces and value readouts.
        (TextStyle::Button, FontId::new(12.0, Proportional)),
        // Hints, units, provenance tags.
        (TextStyle::Small, FontId::new(10.5, Proportional)),
        // Consoles and numeric dumps, where column alignment matters.
        (TextStyle::Monospace, FontId::new(11.5, egui::FontFamily::Monospace)),
    ]
    .into();
}

/// The full `Visuals` pass: surfaces, widget states, strokes, radii, spacing.
fn apply_style(s: &mut egui::Style) {
    apply_text_styles(s);

    let v = &mut s.visuals;
    v.dark_mode = true;

    // ── Surfaces ────────────────────────────────────────────────────────────
    // Panels are painted rather than filled (the `paint` module draws the gradient + grain in
    // `panel_surface`), but the flat fills stay correct as the fallback egui uses for anything
    // this module does not repaint.
    v.panel_fill = PANEL();
    v.window_fill = PANEL();
    v.extreme_bg_color = WELL();
    v.faint_bg_color = RAISED();
    v.code_bg_color = WELL_DEEP();
    v.window_stroke = egui::Stroke::new(1.0, HAIRLINE());
    v.window_corner_radius = egui::CornerRadius::same(5);
    v.menu_corner_radius = egui::CornerRadius::same(5);

    // ── Widget states ───────────────────────────────────────────────────────
    // Read top to bottom as the interaction story. The steps are deliberately tiny — 5–15 RGB
    // values (§14) — because the compressed tonal range *is* the sophistication: cheap dark
    // interfaces jump from near-black panels to bright gray controls, and the jump is what
    // makes them look cheap. Resist "improving" the contrast between adjacent levels.
    let w = &mut v.widgets;

    w.noninteractive.bg_fill = CARD();
    w.noninteractive.weak_bg_fill = CARD();
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0, HAIRLINE());
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0, MUTED());
    w.noninteractive.corner_radius = egui::CornerRadius::same(4);

    w.inactive.bg_fill = RAISED();
    w.inactive.weak_bg_fill = RAISED();
    w.inactive.bg_stroke = egui::Stroke::new(1.0, HAIRLINE());
    w.inactive.fg_stroke = egui::Stroke::new(1.0, TITANIUM());
    w.inactive.corner_radius = egui::CornerRadius::same(4);
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = egui::Color32::from_rgb(0x2F, 0x3A, 0x43);
    w.hovered.weak_bg_fill = egui::Color32::from_rgb(0x2F, 0x3A, 0x43);
    w.hovered.bg_stroke = egui::Stroke::new(1.0, EDGE_STRONG());
    w.hovered.fg_stroke = egui::Stroke::new(1.0, BONE());
    w.hovered.corner_radius = egui::CornerRadius::same(4);
    w.hovered.expansion = 0.0;

    // Even "active" stays desaturated: selection is communicated first by luminance, second by
    // outline, and only third by colour (§9).
    w.active.bg_fill = egui::Color32::from_rgb(0x38, 0x45, 0x50);
    w.active.weak_bg_fill = egui::Color32::from_rgb(0x38, 0x45, 0x50);
    w.active.bg_stroke = egui::Stroke::new(1.0, SELECTED());
    w.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0xE8, 0xEC, 0xEE));
    w.active.corner_radius = egui::CornerRadius::same(4);
    w.active.expansion = 0.0;

    w.open.bg_fill = egui::Color32::from_rgb(0x2A, 0x34, 0x3C);
    w.open.weak_bg_fill = egui::Color32::from_rgb(0x2A, 0x34, 0x3C);
    w.open.bg_stroke = egui::Stroke::new(1.0, EDGE_STRONG());
    w.open.fg_stroke = egui::Stroke::new(1.0, BONE());
    w.open.corner_radius = egui::CornerRadius::same(4);

    // ── Accents ─────────────────────────────────────────────────────────────
    v.selection.bg_fill = egui::Color32::from_rgb(0x33, 0x44, 0x51);
    v.selection.stroke = egui::Stroke::new(1.0, SELECTED());
    v.hyperlink_color = SELECTED();
    v.warn_fg_color = AMBER();
    v.error_fg_color = egui::Color32::from_rgb(0xB5, 0x53, 0x3C); // terracotta (§11 categoricals)
    v.text_cursor.stroke = egui::Stroke::new(1.5, SELECTED());

    // ── Spacing ─────────────────────────────────────────────────────────────
    s.spacing.item_spacing = egui::vec2(6.0, 6.0);
    s.spacing.button_padding = egui::vec2(7.0, 3.0);
    s.spacing.menu_margin = egui::Margin::same(6);
    s.spacing.indent = 14.0;
    s.spacing.slider_width = BAR_MAX_W;
    s.spacing.combo_width = COMBO_W;
    s.spacing.interact_size = egui::vec2(40.0, 18.0);
}

// ═════════════════════════════════════════════════════════════════════════════
// Composed chrome — what `lib.rs` calls
// ═════════════════════════════════════════════════════════════════════════════

/// Paint a dock/panel background: shell gradient + material grain (§3, §4).
///
/// Call at the top of a panel's closure, before its content. The ambient key is *not* included —
/// there is exactly one of those per window, painted by [`workspace_surface`], because the point
/// of §3's illumination is that it is a single light source across the whole faceplate rather
/// than a per-panel effect, and N overlapping keys would read as blotches.
pub fn panel_surface(ui: &egui::Ui) {
    let rect = ui.max_rect().expand(8.0);
    ui.painter().add(paint::shell_face(rect));
    ui.painter().extend(paint::grain(ui.ctx(), rect));
}

/// #593 Tier 4 — the frame the editor's **central region** draws in, given whether the host has
/// already drawn the 3-D world into this surface.
///
/// `None` means "whatever `CentralPanel::default()` does", which is an opaque faceplate filled
/// with [`egui::Visuals::panel_fill`] — every host that owns all of its own pixels (the VST3/CLAP
/// plugin, `organon-standalone`, and Organon Mind under the `nih_plug_egui` editor) gets
/// exactly that, unchanged.
///
/// `scene_behind` is true only under Organon Mind's wgpu editor, which runs `World::render_into`
/// on the same surface immediately before egui's pass. There the central region is not a
/// background to be painted — it **is** the viewport, and painting it opaque is precisely what
/// hid the scene through all of Tier 2 (`references/linux-gpu-host.md`: *"an opaque UI can hide
/// the very thing you are testing"*). So it returns a frame with the panel's geometry and **no
/// fill**: the world shows through wherever a widget has not drawn its own surface, and the
/// workstation's docks, rails, cards and wells — which all paint themselves — sit over it.
///
/// The margin is stated rather than inherited from [`egui::Frame::central_panel`] so the two
/// arms cannot drift apart silently; `8` is that constructor's value and is asserted by a test.
pub fn workspace_frame(scene_behind: bool) -> Option<egui::Frame> {
    scene_behind.then(|| {
        egui::Frame::new()
            .inner_margin(CENTRAL_INNER_MARGIN)
            .fill(egui::Color32::TRANSPARENT)
    })
}

/// The inner margin `egui::Frame::central_panel` uses. Mirrored here by [`workspace_frame`] so
/// the scene-backed central region has identical geometry to the opaque one; pinned by a test.
pub const CENTRAL_INNER_MARGIN: i8 = 8;

/// Paint the central workspace: shell gradient, the single ambient key, then grain (§3).
pub fn workspace_surface(ui: &egui::Ui) {
    let rect = ui.max_rect().expand(8.0);
    ui.painter().add(paint::shell_face(rect));
    ui.painter().add(paint::ambient_key(rect));
    ui.painter().extend(paint::grain(ui.ctx(), rect));
}

/// Corner radius shared by cards, wells, and tiles (§6: 4–6 px throughout). #551 Tier 1 made it
/// configurable; the default is still 5.
pub fn radius() -> u8 {
    crate::theme_config::active().depth.radius
}

/// The card's inner margin as `(x, y)` in points — shared by [`card_frame`] and by `lib.rs`'s
/// header band, which must bleed to the card's inner edges rather than float inside them.
pub const CARD_INNER_MARGIN: (f32, f32) = (9.0, 7.0);

/// The frame a card's *content* sits in. Content-only: the gradient, grain, border and bevel are
/// painted behind it by [`card_chrome`], because egui's `Frame` can express a flat fill but not
/// a gradient, and the gradient is not optional (see the module docs).
pub fn card_frame() -> egui::Frame {
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(
        CARD_INNER_MARGIN.0 as i8,
        CARD_INNER_MARGIN.1 as i8,
    ))
}

/// **Every colour a card's chrome asks for, gathered into one value** — so that one card body
/// can be painted in more than one product's palette (#120).
///
/// 🚨 **This exists because #117 shared the card and the palette came with it.** `lib.rs`'s
/// `card()` is now drawn by Organon's editor *and* by Organon Console's panel column, and every
/// colour in it resolved through the accessors at the top of this module — so a Console theme
/// switch moved the terminal, the composer, the status strip and the tab bar, and left the panel
/// column blue-slate. James, on the live build: *"I didn't want to adopt the blue, gray color
/// theme for all of these. I want them to adapt to the current theme colors."*
///
/// ⚠️ **The colour is parameterised and the GEOMETRY is not, deliberately.** Corner radius,
/// inner margin, the header band's bleed, the grain and the bevel stay where they are and stay
/// shared: they are what "it should use the same exact code somehow" was asking for, and a
/// second card function is the drift #117 existed to prevent. What a palette gets to decide is
/// pigment.
///
/// ⚠️ **The three-stop trick is preserved as a trio of fields rather than as a base colour plus
/// a derivation.** The middle header stop is *lighter than both ends*, which is what makes the
/// band read as convex rolled metal (§5); a struct holding one `header` colour would leave every
/// caller to re-derive the other two, and one of them would eventually do it with two stops.
///
/// [`Self::organon`] is the value Organon's own editor passes, and it is read from the live
/// palette exactly as the accessors did before this type existed — which is why this is a
/// refactor for Organon and a change only for the Console.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardStyle {
    /// The body gradient's top and bottom stops (§6).
    pub body_top: egui::Color32,
    pub body_bottom: egui::Color32,
    /// The header band's three stops (§5), in painting order. `mid` must be the *lightest*.
    pub header_lip: egui::Color32,
    pub header_mid: egui::Color32,
    pub header_foot: egui::Color32,
    /// The hairline rule along the band's lower edge.
    pub header_rule: egui::Color32,
    /// The card's 1 px border.
    pub border: egui::Color32,
    /// The heading's type colour.
    pub title: egui::Color32,
    /// **The space a card leaves under itself, in points** — the one field here that is not
    /// pigment, and the one geometry a surface is allowed to name.
    ///
    /// 🚨 **It is here because it is the only part of the card's geometry the two surfaces
    /// genuinely disagree about, and they disagree for a measurable reason.** Organon's editor
    /// runs `item_spacing.y = 6` ([`install`] → `apply_style`) and the Console runs egui's
    /// default `3`, so one number cannot produce one gap in both. James, on the Console column:
    /// *"we need to remove that spacing. See the dark spacing between them? We need to tighten
    /// it up and stack them more tightly together."*
    ///
    /// ⚠️ **Everything else about a card's geometry stays shared and stays out of this struct.**
    /// Radius, inner margin, band bleed and header layout are what "the same exact code" was
    /// asking for; a struct that accumulated them would become a spec two surfaces each keep half
    /// of. The test for a field belonging here is that a surface has a *reason* to differ, not
    /// that it *could*.
    pub gap: f32,
}

impl CardStyle {
    /// **Organon's own card, read from the live [`crate::theme_config`] palette.**
    ///
    /// 🚨 Every field is the expression the corresponding draw site held before [`CardStyle`]
    /// existed, so `card()` painting through this style is arithmetic-for-arithmetic what it
    /// painted before — `organon_card_style_is_the_look_that_shipped` pins each one against the
    /// palette it came from, and that test is the whole of the claim that Organon is unchanged.
    pub fn organon() -> Self {
        Self::of(&crate::theme_config::active())
    }

    /// [`Self::organon`] against a **named** config rather than the live one.
    ///
    /// ⚠️ `theme_config::active()` resolves through `ThemeConfig::load()`, which reads a file —
    /// so a test that pins the shipped card has to name `ThemeConfig::default()` or it is
    /// asserting something about the machine it runs on. Same reasoning as `surfaces()` at the
    /// bottom of this file, and the reason `organon_card_style_is_the_look_that_shipped` can
    /// state literal bytes at all.
    pub fn of(cfg: &crate::theme_config::ThemeConfig) -> Self {
        let [(_, body_top), (_, body_bottom)] = paint::card_stops_of(cfg);
        let [(_, header_lip), (_, header_mid), (_, header_foot)] = paint::silver_stops_of(cfg);
        Self {
            body_top,
            body_bottom,
            header_lip,
            header_mid,
            header_foot,
            header_rule: egui::Color32::from_rgb(0x30, 0x3A, 0x41),
            border: crate::theme_config::to_col(cfg.palette.hairline),
            title: crate::theme_config::to_col(cfg.palette.bone),
            gap: CARD_GAP,
        }
    }

    /// **A card cut from a plate colour a different palette already owns**, plus that palette's
    /// own answers for its border and its heading.
    ///
    /// 🚨 **The steps are neutral — the same number on all three channels — and that is the
    /// point.** Organon's own table tilts blue as it lightens (`+3,+4,+5` from card to body top),
    /// because §2's rule is `blue − red ∈ 8..=15` and every blue-slate surface obeys it. Carrying
    /// those per-channel deltas onto a green or a warm plate would drag it toward blue-slate,
    /// which is the exact complaint this answers. The **magnitudes** are Organon's, rounded from
    /// the mean of its three channels, so a card cut this way has the reference's compressed
    /// 5–15-level tonal steps (§14) in the caller's own hue.
    ///
    /// ⚠️ **`plate` is not required to differ from the surface behind the card, and Organon's
    /// does not** — `palette.panel` and `palette.card` are the same value in blue slate. A card
    /// is separated from what it sits on by its gradient, its border and its bevel highlight, not
    /// by its fill, which is why passing a column's own background here is right rather than a
    /// mistake to correct with an arbitrary lightening.
    ///
    /// ⚠️ **Steps go *up* for the raised parts whatever the plate's lightness**, so a pale plate
    /// gets a paler band rather than a darker one. Checked against the Console's light palette
    /// (`0xc2c3c4` → band `0xd3d4d5`) rather than assumed: nothing clips, and raised-is-lighter
    /// is the convention the bevel highlight already commits this chrome to.
    pub fn from_plate(
        plate: egui::Color32,
        border: egui::Color32,
        title: egui::Color32,
    ) -> Self {
        let mid = step(plate, HEADER_STEP);
        Self {
            body_top: step(plate, BODY_TOP_STEP),
            body_bottom: step(plate, BODY_BOTTOM_STEP),
            header_lip: step(mid, HEADER_LIP_STEP),
            header_mid: mid,
            header_foot: step(mid, HEADER_FOOT_STEP),
            header_rule: step(mid, HEADER_RULE_STEP),
            border,
            title,
            // Organon's own, so a caller that says nothing about density gets the editor's.
            // The Console overrides it — see `panel_surface::console_card_style`.
            gap: CARD_GAP,
        }
    }
}

/// The space Organon's editor leaves under a card, in points. [`CardStyle::gap`]'s default and,
/// until #120, the only value there was.
pub const CARD_GAP: f32 = 6.0;

/// One tonal step of `d` levels on every channel, saturating at both ends. Alpha is dropped:
/// a card's chrome is an opaque plate, and a translucent gradient over a surface that already
/// painted the same colour would darken it twice.
///
/// ⚠️ **The saturation is the one way this stops being neutral**, and it is deliberately not
/// worked around here: a plate with a channel within a step of `0` or `255` comes back with a
/// hue it did not have, because that channel stops moving while the others keep going. Scaling
/// every channel by the worst-case headroom instead would trade a rare tint for a flattened
/// gradient on *every* dark plate, which is worse and harder to notice.
/// `panel_surface::no_shipped_palette_clips_its_way_to_a_tint` is what says the palettes in the
/// window are inside the range where it holds — and fails for a fifth palette that is not.
fn step(c: egui::Color32, d: i16) -> egui::Color32 {
    let f = |x: u8| (x as i16 + d).clamp(0, 255) as u8;
    egui::Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

// The blue-slate table's own steps from `palette.card`, averaged across its three channels.
// `card 0x212A30` → body top `0x242E35` (+3,+4,+5), body bottom `0x1D252B` (−4,−5,−5),
// header `0x303B43` (+15,+17,+19); and from the header, lip `0x202930` (−16,−18,−19),
// foot `0x2B353D` (−5,−6,−6), rule `0x303A41` (0,−1,−2).
const BODY_TOP_STEP: i16 = 4;
const BODY_BOTTOM_STEP: i16 = -5;
const HEADER_STEP: i16 = 17;
const HEADER_LIP_STEP: i16 = -18;
const HEADER_FOOT_STEP: i16 = -6;
const HEADER_RULE_STEP: i16 = -1;

/// Every shape making up a card's chrome (§6): gradient body, grain, 1 px cool border, and the
/// inset top highlight + lower seam that make it read as machined rather than drawn.
///
/// Returns one composite `Shape` rather than painting, because a card's rect is only known
/// *after* its contents have been laid out. Callers reserve a slot with `painter.add(Shape::Noop)`
/// before the content and `painter.set(idx, …)` after — the standard egui way to paint behind
/// something you have not measured yet.
///
/// ⚠️ **The grain and the bevel are not in [`CardStyle`]** and take no palette argument. They are
/// surface *material* — a noise texture and two fixed-alpha hairlines from
/// [`crate::theme_config`]'s `Material`/`Depth` — rather than pigment, and they are exactly the
/// treatments the module header calls load-bearing. Handing a second palette its own copy of
/// them would be a second look, not a themed card.
pub fn card_chrome(ui: &egui::Ui, rect: egui::Rect, style: &CardStyle) -> egui::Shape {
    let mut v = vec![paint::gradient_v(
        rect,
        &[(0.0, style.body_top), (1.0, style.body_bottom)],
        radius() as f32,
    )];
    v.extend(paint::grain(ui.ctx(), rect));
    v.push(egui::Shape::rect_stroke(
        rect,
        egui::CornerRadius::same(radius()),
        egui::Stroke::new(1.0, style.border),
        egui::StrokeKind::Inside,
    ));
    v.extend(paint::bevel(rect));
    egui::Shape::Vec(v)
}

/// A card's **header band** (§5): the three-stop silver gradient plus a hairline rule along its
/// lower edge. Deferred like [`card_chrome`], for the same reason.
pub fn card_header_band(rect: egui::Rect, style: &CardStyle) -> egui::Shape {
    egui::Shape::Vec(vec![
        paint::gradient_v(
            rect,
            &[
                (0.00, style.header_lip),
                (paint::HEADER_MID_STOP, style.header_mid),
                (1.00, style.header_foot),
            ],
            radius() as f32,
        ),
        egui::Shape::hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, style.header_rule),
        ),
    ])
}

/// Title text for a card header.
///
/// **Bone white, not an accent.** Card titles were the single largest consumer of the old amber —
/// 112 call sites all shouting at once. Making them type-coloured and reserving chroma for live
/// data is most of what separates the pre-#542 UI from the reference (§11). ✏️ The colour now
/// arrives on the [`CardStyle`] rather than from [`BONE`] directly; [`CardStyle::organon`] is
/// what keeps it bone white in Organon's editor.
pub fn card_title(title: &str, style: &CardStyle) -> egui::RichText {
    egui::RichText::new(title).heading().color(style.title)
}

/// A hairline rule across the full width of the current `Ui`.
pub fn hairline(ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    let y = ui.cursor().min.y;
    ui.painter().hline(rect.x_range(), y, egui::Stroke::new(1.0, HAIRLINE()));
    ui.add_space(3.0);
}

/// A **preset tile's** chrome (§10): satin face, diagonal sheen, border, bevel.
///
/// `selected` brightens the face and border a step. It must **not** glow — the reference's
/// selected tile reads as illuminated *by* reflected blue light rather than as emitting it, and
/// a glow is the fastest way to turn a scientific chassis into a game menu.
pub fn preset_tile_chrome(ui: &egui::Ui, rect: egui::Rect, selected: bool) -> egui::Shape {
    let (top, bottom) = if selected {
        (egui::Color32::from_rgb(0x36, 0x41, 0x4A), egui::Color32::from_rgb(0x26, 0x30, 0x38))
    } else {
        (egui::Color32::from_rgb(0x30, 0x3A, 0x42), egui::Color32::from_rgb(0x22, 0x2B, 0x32))
    };
    let mut v = vec![
        paint::gradient_v(rect, &[(0.0, top), (1.0, bottom)], radius() as f32),
        paint::diagonal_sheen(rect, if selected { 1.15 } else { 1.0 }),
    ];
    v.extend(paint::grain(ui.ctx(), rect));
    v.push(egui::Shape::rect_stroke(
        rect,
        egui::CornerRadius::same(radius()),
        egui::Stroke::new(1.0, if selected { SELECTED() } else { HAIRLINE() }),
        egui::StrokeKind::Inside,
    ));
    v.extend(paint::bevel(rect));
    egui::Shape::Vec(v)
}

/// Run `add` inside a card, painting [`card_chrome`] behind whatever it lays out.
///
/// The deferred-paint dance in one place so call sites never repeat it: reserve a shape slot,
/// lay out the content, then fill the slot once the rect is known.
pub fn framed<R>(
    ui: &mut egui::Ui,
    style: &CardStyle,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let slot = ui.painter().add(egui::Shape::Noop);
    let out = card_frame().show(ui, add);
    ui.painter().set(slot, card_chrome(ui, out.response.rect, style));
    out.inner
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// egui's default `item_spacing.x` under this theme.
    const SP: f32 = 6.0;
    /// A representative wide column: the ~2000 pt window in the #542 reference screenshot.
    const WIDE: f32 = 450.0;
    /// A representative cramped column: the 1280×860 default with both docks and the presets
    /// rail claiming their share.
    const NARROW: f32 = 256.0;

    /// The pre-#542 grid, reproduced so the regression tests below compare against the real
    /// thing rather than against remembered numbers.
    fn old_grid(avail: f32, spacing: f32) -> (f32, f32) {
        let w = (((avail - 62.0 - 3.0 * spacing - 2.0 * 24.0) * (2.0 / 3.0)) as f32).max(48.0);
        let bar = (w - VALUE_W - spacing).max(24.0);
        (62.0, bar)
    }

    /// The column width at which the old grid's bar reaches [`BAR_MAX_W`].
    const CROSSOVER: f32 = 510.0;

    // ── The central region's frame (#593 Tier 4) ────────────────────────────

    /// Every host that owns all of its own pixels must keep `CentralPanel::default()`. `None`
    /// is how that is spelled — an explicit frame here, however carefully copied, would be a
    /// second definition of the plugin's faceplate.
    #[test]
    fn an_opaque_host_gets_the_default_central_panel() {
        assert!(workspace_frame(false).is_none());
    }

    /// The scene-backed arm must not paint a fill. This is the whole of #593 Tier 4's half one:
    /// an opaque central region is exactly what hid the world through Tier 2.
    #[test]
    fn the_scene_backed_frame_paints_no_fill() {
        let f = workspace_frame(true).expect("scene_behind must produce a frame");
        assert_eq!(f.fill, egui::Color32::TRANSPARENT);
        assert_eq!(f.fill.a(), 0, "any alpha at all veils the scene");
    }

    /// …while keeping the geometry identical, so switching hosts moves no control by a pixel.
    /// Compared against egui's own constructor rather than against the literal `8`, so an
    /// upstream change to `Frame::central_panel` fails here instead of drifting silently.
    #[test]
    fn the_scene_backed_frame_has_the_default_panels_geometry() {
        let default = egui::Frame::central_panel(&egui::Style::default());
        let scene = workspace_frame(true).unwrap();
        assert_eq!(scene.inner_margin, default.inner_margin);
        assert_eq!(scene.outer_margin, default.outer_margin);
        assert_eq!(scene.corner_radius, default.corner_radius);
        assert_eq!(
            default.inner_margin,
            egui::Margin::same(CENTRAL_INNER_MARGIN),
            "CENTRAL_INNER_MARGIN no longer mirrors egui's own central-panel margin"
        );
    }

    // ── The row grid ────────────────────────────────────────────────────────

    #[test]
    fn grid_tiles_available_width_exactly() {
        for avail in [240.0_f32, 256.0, 300.0, 340.0, 380.0, 450.0, 520.0, 700.0] {
            let g = row_grid(avail, SP);
            assert!(
                (g.total(SP) - avail).abs() < 0.01,
                "avail {avail}: grid totals {} ({g:?})",
                g.total(SP)
            );
        }
    }

    #[test]
    fn label_never_narrower_than_the_old_fixed_width() {
        for avail in [200.0_f32, 256.0, 300.0, 450.0, 900.0] {
            assert!(row_grid(avail, SP).label_w >= LABEL_W_MIN, "avail {avail} went under the floor");
        }
    }

    #[test]
    fn wide_columns_set_labels_whole() {
        assert_eq!(row_grid(WIDE, SP).label_w, LABEL_W_MAX);
    }

    #[test]
    fn narrow_columns_protect_the_bar() {
        let g = row_grid(NARROW, SP);
        assert_eq!(g.label_w, LABEL_W_MIN, "the label should be the segment that yields");
        let (_, old_bar) = old_grid(NARROW, SP);
        assert_eq!(old_bar, 24.0, "the old grid pinned the bar to its floor here");
        assert!(g.bar_w > old_bar * 3.0, "bar {} should dwarf the old {old_bar}", g.bar_w);
    }

    #[test]
    fn bar_is_never_worse_than_before_below_the_cap() {
        let mut avail = 200.0_f32;
        while avail < CROSSOVER {
            let (_, old_bar) = old_grid(avail, SP);
            let g = row_grid(avail, SP);
            assert!(g.bar_w >= old_bar, "avail {avail}: new bar {} < old {old_bar}", g.bar_w);
            assert!(g.label_w >= LABEL_W_MIN, "avail {avail}: label regressed");
            avail += 2.0;
        }
    }

    #[test]
    fn wide_columns_cap_the_bar_on_purpose() {
        for avail in [520.0_f32, 600.0, 700.0, 900.0] {
            let g = row_grid(avail, SP);
            let (_, old_bar) = old_grid(avail, SP);
            assert_eq!(g.bar_w, BAR_MAX_W, "avail {avail}: bar should sit at the cap");
            assert!(old_bar > g.bar_w, "avail {avail}: this test only covers past-crossover widths");
            assert!(g.gap > 0.0, "avail {avail}: the surplus should land in the gap");
        }
        let (_, at_cross) = old_grid(CROSSOVER, SP);
        assert!(
            (at_cross - BAR_MAX_W).abs() < 1.5,
            "crossover moved: old bar is {at_cross} at {CROSSOVER}, cap is {BAR_MAX_W}"
        );
    }

    #[test]
    fn segments_are_monotonic_in_available_width() {
        let mut prev = row_grid(200.0, SP);
        for step in 1..=60 {
            let g = row_grid(200.0 + step as f32 * 10.0, SP);
            assert!(g.label_w >= prev.label_w, "label shrank at {}", 200 + step * 10);
            assert!(g.bar_w >= prev.bar_w, "bar shrank at {}", 200 + step * 10);
            prev = g;
        }
    }

    #[test]
    fn segments_stay_within_their_declared_bounds() {
        for step in 0..=100 {
            let g = row_grid(150.0 + step as f32 * 12.0, SP);
            assert!((LABEL_W_MIN..=LABEL_W_MAX).contains(&g.label_w), "label {} out of range", g.label_w);
            assert!((BAR_MIN_W..=BAR_MAX_W).contains(&g.bar_w), "bar {} out of range", g.bar_w);
            assert!(g.gap >= 0.0, "negative gap");
        }
    }

    #[test]
    fn combo_rows_share_the_slider_rows_grid_lines() {
        // A combo row and a slider row in the same card must agree on where the label ends and
        // where the trailing button sits, or the card reads as two different tables.
        //
        // **Both must fill `avail` exactly.** That is the assertion with teeth: each row ends with
        // its reset button, so equal totals *is* an aligned right edge. The pre-review version
        // compared `c.total()` to `s.total()` while `total()` hard-coded four gaps and always
        // counted `value_w` — so it compared two fictions and passed while combo rows physically
        // drew 64 pt short. Checking against `avail` cannot be satisfied by a bookkeeping error.
        for avail in [256.0_f32, 300.0, 340.0, 450.0, 700.0] {
            for combo_w in [COMBO_W, 2.0 * COMBO_W] {
                let s = row_grid(avail, SP);
                let c = combo_grid(avail, SP, combo_w);
                assert_eq!(c.label_w, s.label_w, "avail {avail}/{combo_w}: labels disagree");
                assert_eq!(c.reset_w, s.reset_w);
                assert_eq!(c.bar_w, combo_w);
                assert_eq!(c.value_w, 0.0, "a combo row draws no value box");
                assert_eq!(c.gaps, COMBO_GAPS, "four items, three gaps");
                assert!(
                    (s.total(SP) - avail).abs() < 0.01,
                    "avail {avail}: slider row draws {}", s.total(SP)
                );
                assert!(
                    (c.total(SP) - avail).abs() < 0.01,
                    "avail {avail}/{combo_w}: combo row draws {} — right edges misaligned by {}",
                    c.total(SP),
                    avail - c.total(SP)
                );
            }
        }
    }

    #[test]
    fn a_too_wide_combo_degrades_without_negative_slack() {
        // The wide combos (`2 × COMBO_W`) genuinely cannot fit a narrow column. That must clamp to
        // zero gap and overflow — which the column's clip rect contains — never wrap to a huge
        // positive gap or a negative width.
        let g = combo_grid(200.0, SP, 2.0 * COMBO_W);
        assert_eq!(g.gap, 0.0);
        assert!(g.total(SP) >= 200.0, "an overfull row may exceed avail, never under-fill it");
    }

    // ── The palette ─────────────────────────────────────────────────────────

    /// Every surface token, for the family-wide invariants below.
    /// The surface ramp **of the specification** — `Palette::default()`, not the live theme.
    ///
    /// This used to call the `WELL_DEEP()` / `PANEL()` accessors, which was a real test-isolation
    /// bug introduced when #551 Tier 1 turned the tokens into runtime state. Those read the
    /// process-global active theme, which is initialised from the user's `ui_theme.json` — so on a
    /// machine with a saved custom theme these invariant tests would validate *that theme* instead
    /// of the spec, and could fail for a reason having nothing to do with the code. It passed in CI
    /// only because a cloud checkout has no such file.
    ///
    /// The invariants below describe what the reference *specifies*, so they must read the
    /// specification. Whether a user's own theme obeys them is not the suite's business — that is
    /// the entire point of the theme being configurable.
    fn surfaces() -> Vec<(&'static str, egui::Color32)> {
        use crate::theme_config::to_col;
        let p = crate::theme_config::Palette::default();
        vec![
            ("well_deep", to_col(p.well_deep)),
            ("well", to_col(p.well)),
            ("shell", to_col(p.shell)),
            ("workspace", to_col(p.workspace)),
            ("panel", to_col(p.panel)),
            ("card", to_col(p.card)),
            ("raised", to_col(p.raised)),
            ("card_header", to_col(p.card_header)),
            ("hairline", to_col(p.hairline)),
            ("edge_strong", to_col(p.edge_strong)),
        ]
    }

    /// The background family — the surfaces §2's `blue − red ∈ 8..=15` rule was actually
    /// derived from. The lighter chrome above `PANEL` is a *proportional* continuation of the
    /// same hue, not a violation of it; see `palette_cast_scales_with_luminance`.
    fn background_surfaces() -> Vec<(&'static str, egui::Color32)> {
        surfaces().into_iter().take(6).collect()
    }

    #[test]
    fn palette_is_cool() {
        // §2's generating rule — but only over the range it actually describes. The literal
        // "8–15 everywhere" reading is false and this test caught it: holding a *constant*
        // channel difference while luminance rises would drift the hue, so the brighter chrome
        // widens the gap to keep the same cool cast (`RAISED` is 18, `EDGE_STRONG` is 25).
        for (name, c) in background_surfaces() {
            let delta = c.b() as i32 - c.r() as i32;
            assert!(
                (8..=15).contains(&delta),
                "{name} has blue-red delta {delta}, outside the 8..=15 §2 specifies for backgrounds"
            );
        }
    }

    #[test]
    fn palette_cast_scales_with_luminance() {
        // What actually keeps the family coherent: one hue direction throughout, with the
        // absolute channel gap widening as surfaces brighten. Constant-delta *or* runaway-delta
        // would both be drift; this pins the shape of the ramp rather than a single number.
        let mut prev = 0;
        for (name, c) in surfaces() {
            let delta = c.b() as i32 - c.r() as i32;
            assert!(
                (8..=26).contains(&delta),
                "{name} has blue-red delta {delta}, outside the family's 8..=26"
            );
            assert!(delta >= prev, "{name} narrows the cast to {delta} after {prev} — hue drifted");
            prev = delta;
        }
    }

    #[test]
    fn palette_is_low_saturation() {
        // "Most of it would survive grayscale conversion" (§1). Green must sit between red and
        // blue on every surface, or the cast stops being a neutral cool bias and becomes a hue.
        for (name, c) in surfaces() {
            let (r, g, b) = (c.r() as i32, c.g() as i32, c.b() as i32);
            assert!(r <= g && g <= b, "{name} is not a clean cool ramp: r{r} g{g} b{b}");
        }
    }

    #[test]
    fn surface_ramp_ascends() {
        // Depth cue: each surface strictly lighter than the one it sits on, or the
        // well/shell/workspace/panel/header planes stop being distinguishable.
        // Reads the spec, not the live theme — see `surfaces()`.
        let p = crate::theme_config::Palette::default();
        let lum = |c: crate::theme_config::Rgb| c[0] as u32 + c[1] as u32 + c[2] as u32;
        assert!(lum(p.well_deep) < lum(p.well), "the deepest well must be the darkest surface");
        assert!(lum(p.well) < lum(p.shell), "wells sit under the shell");
        assert!(lum(p.shell) < lum(p.workspace), "the workspace sits above the shell");
        assert!(lum(p.workspace) < lum(p.panel), "panels sit above the workspace");
        assert!(lum(p.panel) <= lum(p.raised), "raised faces sit above panels");
        assert!(lum(p.raised) < lum(p.card_header), "headers sit above raised faces");
        assert!(lum(p.card_header) < lum(p.hairline), "hairlines read against headers");
        assert!(lum(p.hairline) < lum(p.edge_strong), "the lit edge is brighter than the plain one");
    }

    #[test]
    fn tonal_steps_stay_compressed() {
        // §14: hierarchy levels step by only 5–15 RGB values, and that compression is a major
        // source of the sophistication. A future "let's improve the contrast" edit is exactly
        // what this guards against — an edit to the *specification*, which is why this reads
        // `Palette::default()` and not the live theme (see `surfaces()`). A user's own theme is
        // free to open the ramp up; that is what configurability means, and it must not turn
        // `cargo test` red on the machine of whoever used the theme editor.
        let p = crate::theme_config::Palette::default();
        let lum = |c: crate::theme_config::Rgb| (c[0] as i32 + c[1] as i32 + c[2] as i32) / 3;
        let ladder = [
            ("shell→workspace", p.shell, p.workspace),
            ("workspace→panel", p.workspace, p.panel),
            ("panel→raised", p.panel, p.raised),
            ("raised→card_header", p.raised, p.card_header),
        ];
        for (name, a, b) in ladder {
            let step = lum(b) - lum(a);
            assert!((3..=15).contains(&step), "{name} steps by {step}, outside the compressed 3..=15");
        }
    }

    #[test]
    fn text_never_reaches_pure_white() {
        // §2: holding back the top of the value range is what lets live data mean something.
        // Reads the spec (`Palette::default()`), not the live theme — see `surfaces()`.
        let p = crate::theme_config::Palette::default();
        let c = crate::theme_config::to_col;
        let (bone, tit, mut_) = (c(p.bone), c(p.titanium), c(p.muted));
        for (name, col) in [("bone", bone), ("titanium", tit), ("muted", mut_)] {
            assert!(
                col.r() < 0xFF && col.g() < 0xFF && col.b() < 0xFF,
                "{name} hit pure white"
            );
        }
        let lum = |c: egui::Color32| (c.r() as u32 + c.g() as u32 + c.b() as u32) / 3;
        assert!(lum(bone) > lum(tit), "primary text must outrank secondary");
        assert!(lum(tit) > lum(mut_), "secondary text must outrank disabled");
        assert!(lum(bone) < 0xE0, "primary text is silver, not white");
    }

    // ── The painted layer ───────────────────────────────────────────────────

    #[test]
    fn stop_sampling_hits_its_endpoints_and_interpolates() {
        use paint::sample_stops;
        let a = egui::Color32::from_rgb(0, 0, 0);
        let b = egui::Color32::from_rgb(100, 100, 100);
        let stops = [(0.0, a), (0.5, b), (1.0, a)];
        assert_eq!(sample_stops(&stops, 0.0), a);
        assert_eq!(sample_stops(&stops, 0.5), b);
        assert_eq!(sample_stops(&stops, 1.0), a);
        // Halfway into the first span.
        assert_eq!(sample_stops(&stops, 0.25).r(), 50);
        // Out-of-range clamps rather than wrapping or panicking.
        assert_eq!(sample_stops(&stops, -1.0), a);
        assert_eq!(sample_stops(&stops, 2.0), a);
    }

    #[test]
    fn header_gradient_is_convex() {
        // §5's actual claim, which is what makes a header read as rolled metal instead of as a
        // gradient: the MIDDLE is lighter than BOTH ends. Flattening this to a two-stop ramp is
        // the most likely way to lose the treatment while believing it is still there.
        let lum = |c: egui::Color32| c.r() as i32 + c.g() as i32 + c.b() as i32;
        let stops = [
            (0.00, egui::Color32::from_rgb(0x20, 0x29, 0x30)),
            (0.42, CARD_HEADER()),
            (1.00, egui::Color32::from_rgb(0x2B, 0x35, 0x3D)),
        ];
        let mid = lum(paint::sample_stops(&stops, 0.42));
        assert!(mid > lum(paint::sample_stops(&stops, 0.0)), "middle must outshine the upper lip");
        assert!(mid > lum(paint::sample_stops(&stops, 1.0)), "middle must outshine the lower edge");
    }

    /// A representative panel surface (`PANEL` / `CARD` = `#212A30`).
    const GRAIN_BASE: [u8; 3] = [0x21, 0x2A, 0x30];

    /// Mean RGB levels a texel actually moves a surface, modelling **egui_glow's real pipeline**:
    /// plain premultiply (0.33), componentwise multiply by the vertex colour, then
    /// `ONE / ONE_MINUS_SRC_ALPHA` blending **in gamma space** — `FRAMEBUFFER_SRGB` is disabled.
    ///
    /// Two earlier revisions of this helper modelled the pipeline wrongly in opposite directions
    /// (a gamma-space *premultiply* with gamma blending, then a linear composite), and each time
    /// the grain shipped mis-tuned. The whole value of this test is that it is the pipeline, not
    /// an approximation of it.
    fn texel_deviation(colour: [u8; 3], alpha: u8, base: [u8; 3], strength: f32) -> f32 {
        let (kk, sa) = (strength.clamp(0.0, 1.0), alpha as f32 / 255.0);
        (0..3)
            .map(|i| {
                // `from_rgba_unmultiplied` in egui 0.33: round(value * alpha/255).
                let stored = (colour[i] as f32 * sa).round();
                kk * (stored - base[i] as f32 * sa)
            })
            .sum::<f32>()
            / 3.0
    }

    /// Every `(colour, alpha)` pair in a tile, re-derived from the same generator (the stored
    /// image is premultiplied, so the source colour cannot be divided back out at these alphas).
    fn grain_pairs(seed: u32, light_mod: u32, dark_mod: u32, n: usize) -> Vec<([u8; 3], u8)> {
        (0..(n * n))
            .map(|i| {
                let h = paint::hash32(i as u32 ^ seed);
                if h & 1 == 0 {
                    (paint::GRAIN_LIGHT, (1 + (h >> 8) % light_mod) as u8)
                } else {
                    ([0, 0, 0], (1 + (h >> 8) % dark_mod) as u8)
                }
            })
            .collect()
    }

    /// The fine-grain tile's generator parameters, mirroring `grain_image`.
    const FINE: (u32, u32, u32) = (0x9E37_79B9, 9, 18);
    /// The mottling tile's, mirroring `mottle_image`.
    const BROAD: (u32, u32, u32) = (0x85EB_CA6B, 4, 8);

    fn deviations(p: (u32, u32, u32), n: usize, strength: f32) -> Vec<f32> {
        grain_pairs(p.0, p.1, p.2, n)
            .iter()
            .map(|&(c, a)| texel_deviation(c, a, GRAIN_BASE, strength))
            .collect()
    }

    #[test]
    fn grain_is_balanced() {
        // §4: the grain is a *material*, so it must add texture without shifting the surface's
        // mean value, and must stay inside the ≤3 levels §4 allows at FULL strength (the shipped
        // default scales this down further).
        let devs = deviations(FINE, paint::GRAIN_TILE, 1.0);
        let mean = devs.iter().sum::<f32>() / devs.len() as f32;
        let peak = devs.iter().fold(0.0_f32, |m, d| m.max(d.abs()));
        assert!(mean.abs() < 0.1, "grain shifts the surface by {mean:.3} levels on average");
        assert!(peak <= 3.0, "grain peaks at {peak:.2} levels, louder than the 3 §4 allows");
        // ...and it must actually do something: a grain that rounds to nothing is pure cost.
        assert!(peak > 2.0, "grain peaks at only {peak:.2}; the strength slider would have no range");
    }

    #[test]
    fn grain_at_the_shipped_default_is_subtle() {
        // The acute Mac finding was that §4's *legal* maximum still read as too much texture
        // across every surface at once. This pins what the shipped default actually delivers —
        // under one level of deviation — so a future retune cannot quietly walk it back up.
        let strength = crate::theme_config::Material::default().grain;
        let peak = deviations(FINE, paint::GRAIN_TILE, strength)
            .iter()
            .fold(0.0_f32, |m, d| m.max(d.abs()));
        assert!(peak < 1.2, "the default grain peaks at {peak:.2} levels — back toward too loud");
        assert!(peak > 0.3, "the default grain peaks at {peak:.2} levels — effectively invisible");
    }

    #[test]
    fn mottling_is_quieter_than_grain() {
        // §4: broad mottling sits under the fine grain. If the low-frequency layer were the
        // louder one it would read as blotching rather than as anodizing.
        let peak = |p, n| deviations(p, n, 1.0).iter().fold(0.0_f32, |m, d| m.max(d.abs()));
        let fine = peak(FINE, paint::GRAIN_TILE);
        let broad = peak(BROAD, paint::MOTTLE_TILE);
        assert!(broad < fine, "mottling ({broad:.2}) must be quieter than grain ({fine:.2})");
    }

    #[test]
    fn grain_tiles_are_deterministic_and_reseedable() {
        // Two properties the UI panel depends on, and which are *not* implied by
        // `theme_config`'s `material_key` tests — those only assert that the cache key changes,
        // which is cache invalidation, not that the baked pixels actually differ.
        const SEED: u32 = 0x9E37_79B9;

        // 1. Deterministic. The grain is a *material*: a per-launch-random field would read as
        //    noise in the pejorative sense, and would make screenshots irreproducible.
        assert_eq!(paint::grain_image(SEED).pixels, paint::grain_image(SEED).pixels);
        assert_eq!(paint::mottle_image(SEED).pixels, paint::mottle_image(SEED).pixels);

        // 2. Reseedable — the panel's ↺ reshuffle button has to actually reshuffle. Without this
        //    a broken seed path would leave the control silently inert.
        assert_ne!(
            paint::grain_image(SEED).pixels,
            paint::grain_image(SEED ^ 1).pixels,
            "a new seed must produce a different field, or ↺ reshuffle does nothing"
        );
        assert_ne!(paint::mottle_image(SEED).pixels, paint::mottle_image(SEED ^ 1).pixels);

        // 3. The two layers are distinct fields at the same seed, not one field at two scales —
        //    they are offset by different constants so grain and mottling cannot correlate into
        //    visible structure.
        assert_ne!(paint::hash32(7 ^ FINE.0), paint::hash32(7 ^ BROAD.0));
    }

    #[test]
    fn strength_tint_scales_all_channels_equally() {
        // egui_glow multiplies the stored bytes componentwise, so fading a premultiplied tile
        // uniformly requires rgb and alpha to carry the SAME factor.
        for k in [0.0_f32, 0.28, 0.5, 1.0] {
            let a = (k * 255.0).round() as u8;
            let t = egui::Color32::from_rgba_premultiplied(a, a, a, a);
            assert_eq!((t.r(), t.g(), t.b()), (t.a(), t.a(), t.a()), "k={k}: channels disagree");
        }
        // Under egui **0.31** `from_rgba_unmultiplied(255,255,255,a)` premultiplied through a
        // *gamma-aware* LUT and did **not** satisfy that — it yielded ≈(145,145,145,71) at
        // k = 0.28, scaling rgb about twice as fast as alpha. **0.33 changed it to a plain
        // multiply**, so the two agree today, and this pins the current behaviour rather than
        // either version's.
        //
        // It earns its place as a tripwire: an egui bump that changes the premultiply again is
        // precisely the event that silently re-tunes `grain_image`'s alpha ranges, and the 0.31 →
        // 0.33 bump on `main` did exactly that. If this fails, re-derive those ranges before
        // trusting how the grain looks.
        let naive = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 71);
        assert_eq!(
            (naive.r(), naive.a()),
            (71, 71),
            "egui's premultiply changed — re-derive grain_image's alpha ranges against the new one"
        );
    }

    #[test]
    fn type_ramp_is_ordered_and_legible() {
        let mut s = egui::Style::default();
        apply_text_styles(&mut s);
        let size = |t: egui::TextStyle| s.text_styles[&t].size;
        assert!(size(egui::TextStyle::Heading) > size(egui::TextStyle::Body));
        assert!(size(egui::TextStyle::Body) > size(egui::TextStyle::Small));
        assert_eq!(size(egui::TextStyle::Body), size(egui::TextStyle::Button), "rows and buttons align");
        assert!(size(egui::TextStyle::Small) >= 10.0, "smallest style is still readable");
    }

    // ── The card style (#120) ───────────────────────────────────────────────

    /// 🚨 **The whole of the claim that #120 changed nothing about Organon.** Every colour the
    /// card painted before [`CardStyle`] existed is written out here as a literal, against the
    /// *shipped* config — so a refactor that quietly re-derived one of them, or reached for a
    /// different palette accessor, fails rather than shifting the editor by a few levels where
    /// nobody would look for it.
    ///
    /// ⚠️ Reads `ThemeConfig::default()` rather than `active()`, for `surfaces()`'s reason: the
    /// live config comes off disk, and a machine whose owner has used the theme editor would
    /// otherwise turn this red for a reason having nothing to do with the code.
    #[test]
    fn organon_card_style_is_the_look_that_shipped() {
        let s = CardStyle::of(&crate::theme_config::ThemeConfig::default());
        let rgb = egui::Color32::from_rgb;
        // The body gradient: `paint::card_stops` at `card_gradient = 1.0`.
        assert_eq!(s.body_top, rgb(0x24, 0x2E, 0x35), "card body top");
        assert_eq!(s.body_bottom, rgb(0x1D, 0x25, 0x2B), "card body bottom");
        // The §5 three-stop band at `header_gradient = 1.0`, middle stop = `palette.card_header`.
        assert_eq!(s.header_lip, rgb(0x20, 0x29, 0x30), "header lip");
        assert_eq!(s.header_mid, rgb(0x30, 0x3B, 0x43), "header midpoint");
        assert_eq!(s.header_foot, rgb(0x2B, 0x35, 0x3D), "header foot");
        assert_eq!(s.header_rule, rgb(0x30, 0x3A, 0x41), "the rule under the band");
        assert_eq!(s.border, rgb(0x3A, 0x46, 0x4F), "the 1 px border is HAIRLINE");
        assert_eq!(s.title, rgb(0xD1, 0xD6, 0xD9), "the heading is BONE");
        assert_eq!(s.gap, 6.0, "the editor's inter-card gap is unchanged");
    }

    /// §5's actual trick, asserted rather than described: the band's **middle** stop is lighter
    /// than both ends, which is what makes it read as convex rolled metal. Flattening it to two
    /// stops, or letting a derivation put the bright stop at an end, loses the header treatment
    /// while everything still compiles.
    #[test]
    fn a_header_band_is_brightest_in_the_middle() {
        let lum = |c: egui::Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
        let organon = CardStyle::of(&crate::theme_config::ThemeConfig::default());
        // …and the same must hold for a card cut from someone else's plate, or the Console's
        // column gets a flat band while Organon's keeps the treatment.
        let cut = CardStyle::from_plate(
            egui::Color32::from_rgb(0x0b, 0x12, 0x0e),
            egui::Color32::from_rgb(0x3e, 0x7a, 0x52),
            egui::Color32::from_rgb(0x8f, 0xe0, 0xa8),
        );
        for (name, s) in [("organon", organon), ("from_plate", cut)] {
            assert!(
                lum(s.header_mid) > lum(s.header_lip) && lum(s.header_mid) > lum(s.header_foot),
                "{name}: the band's bright stop must be the middle one"
            );
            assert!(
                lum(s.body_top) > lum(s.body_bottom),
                "{name}: the body gradient darkens downward"
            );
        }
    }

    /// 🚨 **A card cut from a plate keeps that plate's hue.** The steps are neutral on purpose —
    /// Organon's own table tilts blue as it lightens, and carrying those per-channel deltas onto
    /// a green plate would drag the Console's column back toward blue slate, which is the exact
    /// complaint #120 answers.
    #[test]
    fn a_cut_card_does_not_tint_the_plate_it_came_from() {
        // A deliberately green plate: any blue tilt shows up as b rising past r.
        let plate = egui::Color32::from_rgb(0x0b, 0x20, 0x0e);
        let s = CardStyle::from_plate(plate, egui::Color32::BLACK, egui::Color32::WHITE);
        for (name, c) in [
            ("body_top", s.body_top),
            ("body_bottom", s.body_bottom),
            ("header_mid", s.header_mid),
            ("header_lip", s.header_lip),
            ("header_foot", s.header_foot),
        ] {
            let (dr, dg, db) = (
                c.r() as i32 - plate.r() as i32,
                c.g() as i32 - plate.g() as i32,
                c.b() as i32 - plate.b() as i32,
            );
            assert!(
                dr == dg && dg == db,
                "{name} stepped unevenly across channels: {dr}, {dg}, {db} — a hue tilt"
            );
        }
        // The border and the title are the palette's own answers, never derived.
        assert_eq!(s.border, egui::Color32::BLACK);
        assert_eq!(s.title, egui::Color32::WHITE);
    }

    /// A near-white plate must not clip its way to a flat band — the one case where "steps go
    /// up" could have collapsed the treatment, checked rather than assumed.
    #[test]
    fn a_pale_plate_still_has_a_gradient() {
        let s = CardStyle::from_plate(
            egui::Color32::from_rgb(0xc2, 0xc3, 0xc4),
            egui::Color32::BLACK,
            egui::Color32::BLACK,
        );
        assert_ne!(s.header_mid, s.header_lip, "the band flattened on a pale plate");
        assert_ne!(s.body_top, s.body_bottom, "the body gradient flattened on a pale plate");
        assert!(s.header_mid.r() < 0xFF, "the band clipped to white");
    }
}
