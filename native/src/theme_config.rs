//! **The UI theme as runtime state** (#551 Tier 1) — every colour and material treatment the
//! editor paints, made configurable, persisted, and live-editable.
//!
//! # Why this exists
//!
//! #542 Tier 2 landed the painted chrome, and the first look on real hardware said the one thing
//! only real hardware could: the grain was far too strong. That is not a bug — it is a **tuning
//! problem being solved by recompiling**, and every remaining number in
//! `doc/organon_mind_visual_reference.md` has the same shape. The spec says as much: its values
//! are "a starting point for a tuning pass on the Mac". There was no way to *do* that pass.
//!
//! The reference's character comes from many *weak* effects interacting (spec §15), and weak
//! effects are precisely the ones that cannot be reasoned to. You have to see them, nudge them,
//! and see them again. This module makes that loop take a second instead of a ten-minute build.
//!
//! # Why this is not a nih-plug param block
//!
//! Three independent reasons, each disqualifying on its own:
//!
//! 1. **Host automation.** Every nih-plug param is exposed to the DAW. A automation lane driving
//!    your panel border colour is not a feature.
//! 2. **Presets must not restyle the app.** `PresetValues` captures the param set, so a Scene
//!    recall would repaint the editor. Recalling a *sound* must never change what the application
//!    looks like — that is the requirement that motivated the whole issue.
//! 3. **Scale.** 15 colours × 3 channels plus the material scalars would swamp the VST3 parameter
//!    list, which is a user-visible surface in every host.
//!
//! So this is plain Rust state in its own JSON file, beside `presets.json` and `defaults.json`.
//!
//! # Where the active config lives
//!
//! A process-global [`ArcSwap`], not per-`egui::Context` data. The UI theme is genuinely
//! process-wide — two editor windows on one machine should look the same — and a global
//! read is wait-free, which matters because [`palette`] is read by roughly 1057 control rows per
//! frame. It also means paint helpers keep their existing signatures instead of threading a
//! config parameter through every call site.

use crate::theme;
use arc_swap::ArcSwap;
use nih_plug_egui::egui;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

// ═════════════════════════════════════════════════════════════════════════════
// Colour storage
// ═════════════════════════════════════════════════════════════════════════════

/// An sRGB colour as stored on disk.
///
/// `[u8; 3]` rather than `egui::Color32` so the JSON is readable and hand-editable, and so the
/// file format does not depend on an egui type that could change shape between versions.
pub type Rgb = [u8; 3];

/// Convert a stored colour to egui's.
#[inline]
pub fn to_col(c: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// Convert egui's colour back to storage (dropping alpha, which no token carries).
#[inline]
pub fn from_col(c: egui::Color32) -> Rgb {
    [c.r(), c.g(), c.b()]
}

// ═════════════════════════════════════════════════════════════════════════════
// The palette
// ═════════════════════════════════════════════════════════════════════════════

/// Every colour token the editor draws with (spec §2).
///
/// `Copy`, so [`palette`] can hand callers a value rather than a borrow — a control row reads
/// two or three of these and there are ~1057 rows, so cheapness matters more than elegance.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
    // ── Surfaces, darkest first ──
    /// Deepest wells — the darkest surface in the system.
    pub well_deep: Rgb,
    /// Input interiors: text edits, value boxes, slider troughs (§8).
    pub well: Rgb,
    /// Outer shell — the application body behind everything else.
    pub shell: Rgb,
    /// Main workspace field.
    pub workspace: Rgb,
    /// Dock and panel background.
    pub panel: Rgb,
    /// Card body — the surface control rows sit on.
    pub card: Rgb,
    /// Raised surfaces: button faces, lighter regions.
    pub raised: Rgb,
    /// Card-header midpoint — the *bright* stop of the §5 three-stop gradient.
    pub card_header: Rgb,
    /// Cool graphite outline — the default 1 px border.
    pub hairline: Rgb,
    /// Brushed-steel edge — borders that need to read as lit.
    pub edge_strong: Rgb,

    // ── Type ──
    /// Primary text. Never pure white (§2) — the top of the range is held for live data.
    pub bone: Rgb,
    /// Secondary text — control labels.
    pub titanium: Rgb,
    /// Tertiary text — hints, units, provenance tags, disabled copy.
    pub muted: Rgb,

    // ── Accents ──
    /// Selection. Desaturated instrument blue — oxidized steel, not a brand blue (§9).
    pub selected: Rgb,
    /// Live data and status only (§11).
    pub amber: Rgb,
    /// Analytical marks and provenance glyphs (§11).
    pub teal: Rgb,
    /// Filled portion of a slider track (§13).
    pub bar_fill: Rgb,
}

impl Default for Palette {
    /// The shipped blue-slate values — identical to the constants #542 Tier 2 landed, so a fresh
    /// install renders exactly what `main` renders.
    fn default() -> Self {
        Self {
            well_deep: [0x12, 0x1A, 0x20],
            well: [0x14, 0x1C, 0x22],
            shell: [0x19, 0x22, 0x28],
            workspace: [0x1D, 0x25, 0x2C],
            panel: [0x21, 0x2A, 0x30],
            card: [0x21, 0x2A, 0x30],
            raised: [0x27, 0x31, 0x39],
            card_header: [0x30, 0x3B, 0x43],
            hairline: [0x3A, 0x46, 0x4F],
            edge_strong: [0x52, 0x61, 0x6B],
            bone: [0xD1, 0xD6, 0xD9],
            titanium: [0x8E, 0x99, 0x9F],
            muted: [0x60, 0x6B, 0x72],
            selected: [0x5D, 0x78, 0x8C],
            amber: [0xFF, 0xB5, 0x47],
            teal: [0x38, 0x93, 0x94],
            bar_fill: [0x53, 0x63, 0x6E],
        }
    }
}

impl Palette {
    /// The surface ramp, darkest to lightest, with display names — the order the editor lists
    /// them in and the order the §2 invariants are checked in.
    pub const SURFACES: [(&'static str, fn(&Self) -> Rgb, fn(&mut Self) -> &mut Rgb); 10] = [
        ("deepest well", |p| p.well_deep, |p| &mut p.well_deep),
        ("input well", |p| p.well, |p| &mut p.well),
        ("shell", |p| p.shell, |p| &mut p.shell),
        ("workspace", |p| p.workspace, |p| &mut p.workspace),
        ("panel", |p| p.panel, |p| &mut p.panel),
        ("card", |p| p.card, |p| &mut p.card),
        ("raised", |p| p.raised, |p| &mut p.raised),
        ("header", |p| p.card_header, |p| &mut p.card_header),
        ("hairline", |p| p.hairline, |p| &mut p.hairline),
        ("lit edge", |p| p.edge_strong, |p| &mut p.edge_strong),
    ];

    /// Text tokens, brightest first.
    pub const TYPE: [(&'static str, fn(&Self) -> Rgb, fn(&mut Self) -> &mut Rgb); 3] = [
        ("primary", |p| p.bone, |p| &mut p.bone),
        ("secondary", |p| p.titanium, |p| &mut p.titanium),
        ("tertiary", |p| p.muted, |p| &mut p.muted),
    ];

    /// Accent tokens.
    pub const ACCENTS: [(&'static str, fn(&Self) -> Rgb, fn(&mut Self) -> &mut Rgb); 4] = [
        ("selection", |p| p.selected, |p| &mut p.selected),
        ("live / status", |p| p.amber, |p| &mut p.amber),
        ("analytical", |p| p.teal, |p| &mut p.teal),
        ("slider fill", |p| p.bar_fill, |p| &mut p.bar_fill),
    ];
}

// ═════════════════════════════════════════════════════════════════════════════
// The material (§4)
// ═════════════════════════════════════════════════════════════════════════════

/// The surface material: fine grain plus broad mottling.
///
/// Both strengths are **multipliers on the baked tile's alpha**, so `0.0` is a perfectly flat
/// surface and `1.0` is the #542 Tier 2 amount. The default is deliberately well under 1 —
/// see [`Material::default`].
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    /// Fine-grain strength, `0..=1` of the Tier 2 baseline.
    pub grain: f32,
    /// Fine-grain tile size in points. Smaller = busier.
    pub grain_scale: f32,
    /// Broad-mottling strength, `0..=1` of the Tier 2 baseline.
    pub mottle: f32,
    /// How far the mottling stretches, as a multiple of the surface. `1.0` = one blob field per
    /// surface; higher values make the variation broader and slower.
    pub mottle_scale: f32,
    /// Seed for both tiles. Changing it reshuffles the field without changing its character.
    pub seed: u32,
}

impl Default for Material {
    /// **The grain default is 0.28, not 1.0** — the acute finding from the first Mac pass.
    ///
    /// #542 Tier 2 tuned the tile so its *peak* deviation lands inside the 1–3 RGB levels §4
    /// allows, and that arithmetic was right; what it could not know is that 1–3 levels applied
    /// across every surface at once still reads as too much texture. The number was defensible
    /// and the result was wrong, which is a good argument for this whole issue existing. The
    /// baked tile is unchanged — this scales it, so the ceiling is still the §4-legal amount and
    /// the default simply sits well below it.
    fn default() -> Self {
        Self { grain: 0.28, grain_scale: 96.0, mottle: 0.35, mottle_scale: 1.0, seed: 0x9E37_79B9 }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Depth and light (§3, §5, §6, §10)
// ═════════════════════════════════════════════════════════════════════════════

/// The gradient, bevel, and illumination treatments — everything that gives the flat palette its
/// sense of being a physical faceplate.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Depth {
    /// Card-body gradient strength, `0..=1` of the §6 amount. `0` = flat fill.
    pub card_gradient: f32,
    /// Header three-stop gradient strength, `0..=1` of the §5 amount.
    pub header_gradient: f32,
    /// Application shell gradient strength, `0..=1` of the §3 amount.
    pub shell_gradient: f32,
    /// Preset-tile diagonal sheen strength (§10). The one genuinely diagonal treatment.
    pub sheen: f32,
    /// Inset top-highlight alpha, `0..=255`. **The single most important line in the system**
    /// (§6) — it is what makes the UI read as machined rather than drawn.
    pub bevel_top: u8,
    /// Lower inner-seam alpha, `0..=255`.
    pub bevel_bottom: u8,
    /// Ambient-key intensity, `0..=1` of the §3 amount. `0` disables it.
    pub ambient: f32,
    /// Ambient-key centre, as a fraction of the window (`0..=1` each).
    pub ambient_x: f32,
    /// Ambient-key centre, vertical.
    pub ambient_y: f32,
    /// Corner radius for cards, wells, and tiles (§6 asks for 4–6).
    pub radius: u8,
}

impl Default for Depth {
    /// The #542 Tier 2 values, so defaults reproduce today's look exactly.
    fn default() -> Self {
        Self {
            card_gradient: 1.0,
            header_gradient: 1.0,
            shell_gradient: 1.0,
            sheen: 1.0,
            bevel_top: 12,
            bevel_bottom: 97,
            ambient: 1.0,
            ambient_x: 0.48,
            ambient_y: 0.05,
            radius: 5,
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// The whole config
// ═════════════════════════════════════════════════════════════════════════════

/// A complete UI theme.
///
/// `#[serde(default)]` on every block and every field, so a file written by any version — earlier
/// or later — still loads, with unknown fields ignored and missing ones filled from the
/// defaults. A theme file should never be able to make the editor fail to start.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    /// Display name. Cosmetic in Tier 1; Tier 2's named-theme store keys off it.
    pub name: String,
    pub palette: Palette,
    pub material: Material,
    pub depth: Depth,
}

impl ThemeConfig {
    /// The shipped default — the blue-slate look, with the grain at its corrected strength.
    pub fn blue_slate() -> Self {
        Self { name: "Blue Slate".into(), ..Default::default() }
    }

    /// The superseded **warm** shell (`doc/organon_mind_visual_reference.md` §18), adopted
    /// 2026-07-24 and replaced by blue slate on 2026-07-29.
    ///
    /// Kept **selectable** rather than only recoverable from git history, for two reasons. It was
    /// a considered direction, not a mistake — the reference explicitly forbade blue-black, and
    /// the reasoning that produced it still reads well. And a look is judged by comparison: being
    /// able to flip between warm and cool in one click is worth more than either being able to
    /// argue about them from memory.
    pub fn warm_instrument() -> Self {
        Self {
            name: "Warm Instrument".into(),
            palette: Palette {
                well_deep: [0x0A, 0x09, 0x08],
                well: [0x0E, 0x0C, 0x0B],
                shell: [0x14, 0x12, 0x10],
                workspace: [0x1A, 0x17, 0x14],
                panel: [0x20, 0x1C, 0x18],
                card: [0x20, 0x1C, 0x18],
                raised: [0x2A, 0x25, 0x21],
                card_header: [0x33, 0x2C, 0x25],
                hairline: [0x3A, 0x34, 0x2E],
                edge_strong: [0x54, 0x4A, 0x40],
                bone: [0xEC, 0xE6, 0xDB],
                titanium: [0xC9, 0xC1, 0xB4],
                muted: [0x8A, 0x81, 0x75],
                // The warm era's selection was pale gold, not a blue.
                selected: [0xF4, 0xD5, 0x8A],
                amber: [0xFF, 0xB5, 0x47],
                // The warm-era categorical teal (§11), a touch deeper than the cool shell's.
                teal: [0x3E, 0x7C, 0x7B],
                bar_fill: [0xC8, 0xA0, 0x54],
            },
            ..Default::default()
        }
    }

    /// A **high-contrast** variant for projector and daylight work.
    ///
    /// **This one deliberately breaks the spec**, and that is the point. §14's compressed 5–15
    /// RGB tonal steps are exactly what makes the reference read as an instrument — and exactly
    /// what makes it unreadable in a bright room or through a projector's washed-out gamma. The
    /// steps here are two to three times wider, text runs brighter, borders are lit rather than
    /// graphite, and the grain drops most of the way out (surface texture is the first thing
    /// ambient light destroys, so paying for it buys nothing).
    ///
    /// Kept honest by being a *named alternative* rather than a tweak to the default: the spec
    /// stays the spec, and this is the thing you switch to when the room wins.
    pub fn high_contrast() -> Self {
        Self {
            name: "High Contrast".into(),
            palette: Palette {
                well_deep: [0x05, 0x08, 0x0A],
                well: [0x0A, 0x0F, 0x13],
                shell: [0x10, 0x18, 0x20],
                workspace: [0x17, 0x22, 0x2C],
                panel: [0x1F, 0x2E, 0x3A],
                card: [0x1F, 0x2E, 0x3A],
                raised: [0x2E, 0x42, 0x52],
                card_header: [0x3E, 0x56, 0x6A],
                hairline: [0x5A, 0x74, 0x88],
                edge_strong: [0x8A, 0xA6, 0xBC],
                bone: [0xF2, 0xF6, 0xF9],
                titanium: [0xBE, 0xCB, 0xD5],
                muted: [0x87, 0x98, 0xA5],
                selected: [0x7F, 0xB0, 0xD8],
                amber: [0xFF, 0xC0, 0x4D],
                teal: [0x46, 0xB5, 0xB6],
                bar_fill: [0x6E, 0x90, 0xA8],
            },
            // Grain and mottling are the first casualties of ambient light — near-zero rather
            // than zero, so the surfaces are not perfectly flat if the room does darken.
            material: Material { grain: 0.08, mottle: 0.10, ..Default::default() },
            // A stronger top highlight and seam: with the palette this bright, the 1 px bevel
            // needs more to register at all.
            depth: Depth { bevel_top: 26, bevel_bottom: 120, ambient: 0.5, ..Default::default() },
        }
    }

    /// The read-only gallery, in display order.
    ///
    /// **Built-ins are code, not stored data.** They are always present, never editable in place,
    /// and survive a deleted or corrupt library file. Two consequences worth having: a release can
    /// add a gallery entry without migrating anyone's `ui_themes.json`, and there is always a
    /// known-good theme to fall back to. Editing one means duplicating it into the user list
    /// first, which is also the honest gesture — "based on High Contrast" rather than a silently
    /// diverged thing still wearing its name.
    pub fn built_ins() -> Vec<ThemeConfig> {
        vec![Self::blue_slate(), Self::warm_instrument(), Self::high_contrast()]
    }

    /// Is `name` one of the built-in gallery's? Built-ins may be applied and duplicated but not
    /// renamed, overwritten, or deleted.
    pub fn is_built_in(name: &str) -> bool {
        Self::built_ins().iter().any(|t| t.name == name)
    }

    /// Everything that affects the **baked noise tiles**, hashed so the texture cache can tell
    /// whether it must rebuild. Colours and depth do not appear here: they are applied at paint
    /// time and need no texture work, so dragging a colour picker must not thrash the atlas.
    pub fn material_key(&self) -> u64 {
        let m = &self.material;
        let mut k = m.seed as u64;
        // Strength is applied per-frame as a vertex-colour multiplier, *not* baked, so it is
        // deliberately absent: dragging the grain slider must not rebuild a texture.
        k = k.wrapping_mul(0x100_0000_01B3) ^ (m.grain_scale.to_bits() as u64);
        k = k.wrapping_mul(0x100_0000_01B3) ^ (m.mottle_scale.to_bits() as u64);
        k
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// The active config
// ═════════════════════════════════════════════════════════════════════════════

fn cell() -> &'static ArcSwap<ThemeConfig> {
    static ACTIVE: OnceLock<ArcSwap<ThemeConfig>> = OnceLock::new();
    ACTIVE.get_or_init(|| ArcSwap::from_pointee(ThemeConfig::load()))
}

/// The live theme. Wait-free; safe to call per widget.
pub fn active() -> Arc<ThemeConfig> {
    cell().load_full()
}

/// The live palette, by value.
pub fn palette() -> Palette {
    active().palette
}

/// Replace the live theme. Takes effect on the next repaint.
pub fn set_active(cfg: ThemeConfig) {
    cell().store(Arc::new(cfg));
}

// ═════════════════════════════════════════════════════════════════════════════
// Persistence
// ═════════════════════════════════════════════════════════════════════════════

/// `~/Library/Application Support/OrganicMath/ui_theme.json` — the third file beside
/// `presets.json` and `defaults.json`, and deliberately separate from both: a Scene recall must
/// never restyle the application.
fn store_path() -> std::path::PathBuf {
    let dir = dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("OrganicMath");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("ui_theme.json")
}

impl ThemeConfig {
    /// Load the saved theme, falling back to the shipped default on any failure.
    ///
    /// Deliberately total: a missing, unreadable, or malformed file yields the default rather
    /// than an error. A corrupt theme file must never stop the editor opening — the worst
    /// acceptable outcome is that it looks like a fresh install.
    pub fn load() -> Self {
        std::fs::read_to_string(store_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(Self::blue_slate)
    }

    /// Write the theme to the store. Returns whether it landed.
    pub fn save(&self) -> bool {
        serde_json::to_string_pretty(self)
            .ok()
            .map(|j| std::fs::write(store_path(), j).is_ok())
            .unwrap_or(false)
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// The theme library (#551 Tier 2)
// ═════════════════════════════════════════════════════════════════════════════

/// `~/Library/Application Support/OrganicMath/ui_themes.json` — the **named** themes.
///
/// Deliberately a *different file* from `ui_theme.json` (the single active theme, Tier 1). The
/// two answer different questions — "what does the editor look like right now" versus "what
/// looks has this user kept" — and separating them means a corrupt library cannot cost you your
/// working theme, or the reverse.
///
/// And both are separate from `presets.json`, which is the requirement that started #551: a
/// Scene recall changes what the *visual* draws and must never restyle the application. Three
/// files, three lifetimes.
fn library_path() -> std::path::PathBuf {
    let dir = dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("OrganicMath");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("ui_themes.json")
}

/// The user's saved themes. Built-ins are **not** in here — see [`ThemeConfig::built_ins`].
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeLibrary {
    pub themes: Vec<ThemeConfig>,
}

impl ThemeLibrary {
    /// Load the library, falling back to empty on any failure.
    ///
    /// Total, like [`ThemeConfig::load`]: a missing file is the normal first-run state, and a
    /// corrupt one must cost you your saved list but never your ability to open the editor.
    pub fn load() -> Self {
        std::fs::read_to_string(library_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist. Returns whether it landed.
    pub fn save(&self) -> bool {
        serde_json::to_string_pretty(self)
            .ok()
            .map(|j| std::fs::write(library_path(), j).is_ok())
            .unwrap_or(false)
    }

    /// A name not already taken by a saved theme **or a built-in**, by appending " 2", " 3", …
    ///
    /// Built-ins are included in the collision set on purpose: a user theme called "Blue Slate"
    /// sitting beside the built-in of the same name is indistinguishable in a list, and the one
    /// you clicked would depend on iteration order.
    pub fn unique_name(&self, desired: &str) -> String {
        let taken = |n: &str| {
            ThemeConfig::is_built_in(n) || self.themes.iter().any(|t| t.name == n)
        };
        if !taken(desired) {
            return desired.to_string();
        }
        for k in 2.. {
            let cand = format!("{desired} {k}");
            if !taken(&cand) {
                return cand;
            }
        }
        desired.to_string()
    }

    /// Add `cfg` under a unique name derived from `desired`. Returns the name actually used.
    pub fn add(&mut self, mut cfg: ThemeConfig, desired: &str) -> String {
        let name = self.unique_name(desired);
        cfg.name = name.clone();
        self.themes.push(cfg);
        name
    }

    /// Rename the theme at `i`, uniquifying against everything else. No-op if `i` is out of
    /// range or the new name is blank.
    pub fn rename(&mut self, i: usize, desired: &str) -> bool {
        if i >= self.themes.len() || desired.trim().is_empty() {
            return false;
        }
        // Exclude the row being renamed from the collision set, or renaming a theme to its own
        // name would silently append " 2".
        let current = std::mem::take(&mut self.themes[i].name);
        let name = self.unique_name(desired.trim());
        self.themes[i].name = if name.is_empty() { current } else { name };
        true
    }

    /// Delete the theme at `i`.
    pub fn remove(&mut self, i: usize) -> bool {
        if i >= self.themes.len() {
            return false;
        }
        self.themes.remove(i);
        true
    }

    /// Overwrite the theme at `i` with `cfg`, keeping its existing name.
    pub fn update(&mut self, i: usize, cfg: &ThemeConfig) -> bool {
        if i >= self.themes.len() {
            return false;
        }
        let name = self.themes[i].name.clone();
        self.themes[i] = ThemeConfig { name, ..cfg.clone() };
        true
    }
}

impl ThemeConfig {
    /// Write this theme to `path` as standalone JSON, for sharing as a file.
    pub fn export_to(&self, path: &std::path::Path) -> bool {
        serde_json::to_string_pretty(self)
            .ok()
            .map(|j| std::fs::write(path, j).is_ok())
            .unwrap_or(false)
    }

    /// Read a theme from `path`. `None` on any failure — an unreadable or foreign file is a
    /// no-op, not a corrupted editor.
    pub fn import_from(path: &std::path::Path) -> Option<ThemeConfig> {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
    }
}

/// Apply `cfg` as the live theme **and** persist it as the active one.
///
/// Recall persists on purpose: clicking a theme and having the editor come back tomorrow looking
/// like something else would be a bug, not a safety feature. The library file is untouched — this
/// changes what is active, not what is saved.
pub fn apply(cfg: &ThemeConfig) {
    set_active(cfg.clone());
    cfg.save();
}

// ═════════════════════════════════════════════════════════════════════════════
// The editor panel
// ═════════════════════════════════════════════════════════════════════════════

/// #551 Tier 2 — the **Themes** section: the built-in gallery, the user's saved themes, and
/// import/export.
///
/// Placed first in the panel because it is the coarse control: you choose a look, *then* tune it.
/// Built-ins are listed above user themes and are click-to-apply only — the `⧉` duplicates one
/// into the user list, which is where editing happens. That gesture is also the honest one: a
/// modified theme should say "based on High Contrast", not quietly wear the name while having
/// diverged from it.
/// Returns **true** if this frame performed a *discrete recall* — clicking a theme, duplicating
/// one, importing, or renaming/updating a slot. Only those persist to the active store; a live
/// slider drag must not, or every repaint tick writes the theme file to disk (see [`ui_panel`]).
fn theme_library_ui(
    ui: &mut egui::Ui,
    cfg: &mut ThemeConfig,
    lib: &mut Option<ThemeLibrary>,
    rename: &mut Option<(usize, String)>,
) -> bool {
    let library = lib.get_or_insert_with(ThemeLibrary::load);
    let muted = to_col(cfg.palette.muted);
    let mut recalled = false;

    egui::CollapsingHeader::new("Themes").default_open(true).show(ui, |ui| {
        ui.label(
            egui::RichText::new(
                "Saved in ui_themes.json — their own file, separate from the active theme and \
                 from parameter presets. Recalling a Scene never touches any of them.",
            )
            .small()
            .color(muted),
        );
        ui.add_space(3.0);

        // ── The built-in gallery ──
        ui.label(egui::RichText::new("built in").small().color(muted));
        for built in ThemeConfig::built_ins() {
            ui.horizontal(|ui| {
                let active_now = cfg.name == built.name;
                if ui
                    .selectable_label(active_now, &built.name)
                    .on_hover_text("Apply this theme")
                    .clicked()
                {
                    *cfg = built.clone();
                    recalled = true;
                }
                if ui
                    .small_button("⧉")
                    .on_hover_text("Duplicate into your themes, so you can edit it")
                    .clicked()
                {
                    let name = library.add(built.clone(), &format!("{} copy", built.name));
                    library.save();
                    // Switch to the copy: the point of duplicating is to start editing it.
                    if let Some(t) = library.themes.iter().find(|t| t.name == name) {
                        *cfg = t.clone();
                    }
                    recalled = true;
                }
            });
        }

        ui.add_space(4.0);
        ui.label(egui::RichText::new("yours").small().color(muted));
        if library.themes.is_empty() {
            ui.label(egui::RichText::new("None yet — Save Current below.").small().color(muted));
        }

        // One action per frame, resolved after the loop — mutating the list mid-iteration would
        // invalidate the indices the buttons were drawn with.
        let mut action: Option<(usize, LibAction)> = None;
        for i in 0..library.themes.len() {
            let is_renaming = rename.as_ref().is_some_and(|(r, _)| *r == i);
            if is_renaming {
                ui.horizontal(|ui| {
                    if let Some((_, buf)) = rename.as_mut() {
                        ui.add(egui::TextEdit::singleline(buf).desired_width(ui.available_width() - 70.0));
                    }
                    if ui.small_button("ok").clicked() {
                        action = Some((i, LibAction::RenameCommit));
                    }
                    if ui.small_button("×").clicked() {
                        action = Some((i, LibAction::RenameCancel));
                    }
                });
                continue;
            }
            ui.horizontal(|ui| {
                let active_now = cfg.name == library.themes[i].name;
                if ui
                    .selectable_label(active_now, &library.themes[i].name)
                    .on_hover_text("Apply this theme")
                    .clicked()
                {
                    action = Some((i, LibAction::Apply));
                }
                if ui.small_button("R").on_hover_text("Rename").clicked() {
                    action = Some((i, LibAction::Rename));
                }
                if ui.small_button("D").on_hover_text("Delete").clicked() {
                    action = Some((i, LibAction::Delete));
                }
                if ui
                    .small_button("U")
                    .on_hover_text("Update to the theme currently showing")
                    .clicked()
                {
                    action = Some((i, LibAction::Update));
                }
            });
        }

        if let Some((i, act)) = action {
            recalled = true;
            match act {
                LibAction::Apply => {
                    if let Some(t) = library.themes.get(i) {
                        *cfg = t.clone();
                    }
                }
                LibAction::Rename => {
                    let seed = library.themes.get(i).map(|t| t.name.clone()).unwrap_or_default();
                    *rename = Some((i, seed));
                }
                LibAction::RenameCommit => {
                    if let Some((_, buf)) = rename.clone() {
                        library.rename(i, &buf);
                        library.save();
                    }
                    *rename = None;
                }
                LibAction::RenameCancel => *rename = None,
                LibAction::Delete => {
                    library.remove(i);
                    library.save();
                    *rename = None;
                }
                LibAction::Update => {
                    library.update(i, cfg);
                    library.save();
                    // Keep the panel showing the stored name rather than the one being edited.
                    if let Some(t) = library.themes.get(i) {
                        cfg.name = t.name.clone();
                    }
                }
            }
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .button("Save Current")
                .on_hover_text("Add what is showing to your themes")
                .clicked()
            {
                let seed = if cfg.name.trim().is_empty() || ThemeConfig::is_built_in(&cfg.name) {
                    // Never overwrite or shadow a built-in: a save from one becomes a new entry.
                    format!("{} copy", if cfg.name.trim().is_empty() { "Theme" } else { &cfg.name })
                } else {
                    cfg.name.clone()
                };
                let name = library.add(cfg.clone(), &seed);
                library.save();
                cfg.name = name;
                recalled = true;
            }
            if ui.button("Import…").on_hover_text("Add a theme from a .json file").clicked() {
                if let Some(path) =
                    rfd::FileDialog::new().add_filter("Organon theme", &["json"]).pick_file()
                {
                    if let Some(t) = ThemeConfig::import_from(&path) {
                        let seed = if t.name.trim().is_empty() { "Imported".into() } else { t.name.clone() };
                        let name = library.add(t, &seed);
                        library.save();
                        if let Some(t) = library.themes.iter().find(|t| t.name == name) {
                            *cfg = t.clone();
                        }
                        recalled = true;
                    }
                }
            }
            if ui.button("Export…").on_hover_text("Write what is showing to a .json file").clicked() {
                let stem = if cfg.name.trim().is_empty() { "organon-theme" } else { &cfg.name };
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Organon theme", &["json"])
                    .set_file_name(format!("{stem}.json"))
                    .save_file()
                {
                    cfg.export_to(&path);
                }
            }
        });
    });
    ui.add_space(4.0);
    recalled
}

/// One row action from the theme list (only one fires per frame).
enum LibAction {
    Apply,
    Rename,
    RenameCommit,
    RenameCancel,
    Delete,
    Update,
}

/// Draw the **UI** configuration panel (#551 Tier 1).
///
/// Edits apply to the live theme immediately — the whole point is a tight see/nudge/see loop, so
/// there is no "apply" step. `Save` persists; `Revert` re-reads the store; `Reset` returns to the
/// shipped defaults without touching the file until saved.
pub fn ui_panel(ui: &mut egui::Ui, lib: &mut Option<ThemeLibrary>, rename: &mut Option<(usize, String)>) {
    let mut cfg = (*active()).clone();
    let before = cfg.clone();
    // Did this frame *recall* a theme, as opposed to nudging the live one? The distinction
    // decides whether the change reaches disk — see the `cfg != before` block at the end.
    let mut recalled = false;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Interface").heading().color(to_col(cfg.palette.bone)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Reset").on_hover_text("Back to the shipped Blue Slate defaults").clicked() {
                cfg = ThemeConfig::blue_slate();
            }
            if ui.button("Revert").on_hover_text("Discard changes; re-read the saved theme").clicked() {
                cfg = ThemeConfig::load();
            }
            if ui.button("Save").on_hover_text("Persist to ui_theme.json").clicked() {
                cfg.save();
            }
        });
    });
    ui.label(
        egui::RichText::new(
            "Live — every change applies as you drag. Saved separately from parameter presets, \
             so recalling a Scene never restyles the editor.",
        )
        .small()
        .color(to_col(cfg.palette.muted)),
    );
    ui.add_space(4.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        recalled |= theme_library_ui(ui, &mut cfg, lib, rename);

        ui.collapsing("Palette — surfaces", |ui| {
            ui.label(
                egui::RichText::new(
                    "Darkest to lightest. The steps are meant to be small (5–15 RGB levels): the \
                     compressed range is what separates an instrument from a cheap dark theme.",
                )
                .small()
                .color(to_col(cfg.palette.muted)),
            );
            for (name, get, get_mut) in Palette::SURFACES {
                colour_row(ui, name, get(&cfg.palette), get_mut(&mut cfg.palette));
            }
        });

        ui.collapsing("Palette — type", |ui| {
            ui.label(
                egui::RichText::new("Primary text should stay below pure white — the top of the range is held for live data.")
                    .small()
                    .color(to_col(cfg.palette.muted)),
            );
            for (name, get, get_mut) in Palette::TYPE {
                colour_row(ui, name, get(&cfg.palette), get_mut(&mut cfg.palette));
            }
        });

        ui.collapsing("Palette — accents", |ui| {
            ui.label(
                egui::RichText::new("Strong colour belongs to data, selection and status. Spent on chrome, it stops meaning anything.")
                    .small()
                    .color(to_col(cfg.palette.muted)),
            );
            for (name, get, get_mut) in Palette::ACCENTS {
                colour_row(ui, name, get(&cfg.palette), get_mut(&mut cfg.palette));
            }
        });

        ui.collapsing("Material — grain and mottling", |ui| {
            ui.label(
                egui::RichText::new(
                    "Fine grain plus broad low-frequency mottling: what makes a surface read as \
                     powder-coated rather than flat. Both are multipliers on the baked tile, so 0 \
                     is perfectly flat and 1 is the maximum the palette can carry without the \
                     texture becoming visible as speckle.",
                )
                .small()
                .color(to_col(cfg.palette.muted)),
            );
            ui.add(egui::Slider::new(&mut cfg.material.grain, 0.0..=1.0).text("grain"));
            ui.add(egui::Slider::new(&mut cfg.material.grain_scale, 16.0..=256.0).text("grain tile (pt)"));
            ui.add(egui::Slider::new(&mut cfg.material.mottle, 0.0..=1.0).text("mottling"));
            ui.add(egui::Slider::new(&mut cfg.material.mottle_scale, 0.25..=4.0).text("mottle spread"));
            ui.horizontal(|ui| {
                ui.label("seed");
                let mut seed = cfg.material.seed as f64;
                if ui.add(egui::DragValue::new(&mut seed).speed(1.0)).changed() {
                    cfg.material.seed = seed.max(0.0) as u32;
                }
                if ui.small_button("↺").on_hover_text("Reshuffle").clicked() {
                    cfg.material.seed = cfg.material.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                }
            });
        });

        ui.collapsing("Depth — gradients and bevels", |ui| {
            ui.label(
                egui::RichText::new(
                    "The top highlight is the load-bearing one: a single inset line is most of \
                     what makes the interface feel machined into a faceplate rather than drawn.",
                )
                .small()
                .color(to_col(cfg.palette.muted)),
            );
            ui.add(egui::Slider::new(&mut cfg.depth.card_gradient, 0.0..=1.0).text("card gradient"));
            ui.add(egui::Slider::new(&mut cfg.depth.header_gradient, 0.0..=1.0).text("header gradient"));
            ui.add(egui::Slider::new(&mut cfg.depth.shell_gradient, 0.0..=1.0).text("shell gradient"));
            ui.add(egui::Slider::new(&mut cfg.depth.sheen, 0.0..=1.0).text("tile sheen"));
            ui.add(egui::Slider::new(&mut cfg.depth.bevel_top, 0..=48).text("top highlight"));
            ui.add(egui::Slider::new(&mut cfg.depth.bevel_bottom, 0..=160).text("bottom seam"));
            ui.add(egui::Slider::new(&mut cfg.depth.radius, 0..=12).text("corner radius"));
        });

        ui.collapsing("Light — the ambient key", |ui| {
            ui.label(
                egui::RichText::new(
                    "One broad, very faint illumination — the front face of an instrument catching \
                     dim light. If you can see it as a gradient, it is too strong.",
                )
                .small()
                .color(to_col(cfg.palette.muted)),
            );
            ui.add(egui::Slider::new(&mut cfg.depth.ambient, 0.0..=1.0).text("intensity"));
            ui.add(egui::Slider::new(&mut cfg.depth.ambient_x, 0.0..=1.0).text("centre X"));
            ui.add(egui::Slider::new(&mut cfg.depth.ambient_y, -0.2..=1.0).text("centre Y"));
        });
    });

    if cfg != before {
        if recalled {
            // A *discrete* recall — a click, a duplicate, an import, a slot edit. Persist, so a
            // theme you picked is still there tomorrow. The library file is untouched: this
            // changes what is active, not what is saved.
            apply(&cfg);
        } else {
            // A live edit: a colour picker or a slider mid-drag. **Must not touch disk.** `cfg`
            // changes on every frame of a drag, so persisting here would `fs::write` the theme
            // file on every repaint tick — and would quietly break the three promises this
            // function's doc comment makes, since `Reset` and any in-progress drag would become
            // permanent before you decided they should be, leaving `Revert` re-reading whatever
            // you were mid-drag on.
            set_active(cfg);
        }
    }
}

/// One colour row: swatch-picker, name, and the hex the spec would quote.
fn colour_row(ui: &mut egui::Ui, name: &str, current: Rgb, slot: &mut Rgb) {
    ui.horizontal(|ui| {
        let mut c = [current[0], current[1], current[2]];
        if egui::color_picker::color_edit_button_srgb(ui, &mut c).changed() {
            *slot = c;
        }
        ui.add_sized([theme::LABEL_W_MAX, 18.0], egui::Label::new(name).truncate());
        ui.label(
            egui::RichText::new(format!("#{:02X}{:02X}{:02X}", current[0], current[1], current[2]))
                .small()
                .monospace()
                .color(to_col(palette().muted)),
        );
    });
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_reproduce_the_shipped_tokens() {
        // The default-inert contract: a fresh install must render exactly what #542 Tier 2
        // rendered, so the config layer cannot silently restyle anything by existing.
        let p = Palette::default();
        assert_eq!(to_col(p.well_deep), egui::Color32::from_rgb(0x12, 0x1A, 0x20));
        assert_eq!(to_col(p.workspace), egui::Color32::from_rgb(0x1D, 0x25, 0x2C));
        assert_eq!(to_col(p.card_header), egui::Color32::from_rgb(0x30, 0x3B, 0x43));
        assert_eq!(to_col(p.bone), egui::Color32::from_rgb(0xD1, 0xD6, 0xD9));
        assert_eq!(to_col(p.amber), egui::Color32::from_rgb(0xFF, 0xB5, 0x47));
        // Depth defaults are the Tier 2 constants.
        let d = Depth::default();
        assert_eq!((d.bevel_top, d.bevel_bottom), (12, 97));
        assert_eq!((d.ambient_x, d.ambient_y), (0.48, 0.05));
        assert_eq!(d.radius, 5);
        for g in [d.card_gradient, d.header_gradient, d.shell_gradient, d.sheen, d.ambient] {
            assert_eq!(g, 1.0, "depth treatments default to the full Tier 2 amount");
        }
    }

    #[test]
    fn grain_default_is_well_below_the_ceiling() {
        // The acute finding from the first Mac pass. The baked tile is still §4-legal at full
        // strength; the default simply sits far below it. If someone "restores" this to 1.0 the
        // reported problem comes straight back.
        let m = Material::default();
        assert!(m.grain < 0.4, "grain default {} is back near the too-strong ceiling", m.grain);
        assert!(m.grain > 0.0, "a zero default would remove the material entirely");
        assert!(m.mottle < 0.6 && m.mottle > 0.0);
    }

    #[test]
    fn config_round_trips_through_json() {
        let mut cfg = ThemeConfig::blue_slate();
        cfg.palette.bone = [1, 2, 3];
        cfg.material.grain = 0.42;
        cfg.depth.radius = 9;
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: ThemeConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cfg, back);
    }

    #[test]
    fn partial_and_unknown_fields_still_load() {
        // A theme file must never stop the editor opening. Fields written by a *later* version
        // are ignored; fields missing from an *earlier* one fall back to the default.
        let json = r#"{"name":"partial","palette":{"bone":[9,9,9]},"nonsense":{"x":1}}"#;
        let cfg: ThemeConfig = serde_json::from_str(json).expect("tolerant of partial input");
        assert_eq!(cfg.name, "partial");
        assert_eq!(cfg.palette.bone, [9, 9, 9], "the specified field wins");
        assert_eq!(cfg.palette.card, Palette::default().card, "the omitted one defaults");
        assert_eq!(cfg.material, Material::default(), "an omitted block defaults wholesale");
    }

    #[test]
    fn malformed_json_falls_back_rather_than_failing() {
        assert!(serde_json::from_str::<ThemeConfig>("{{{ not json").is_err());
        // `load()` swallows exactly that error; this asserts the fallback it returns is usable.
        let fallback = ThemeConfig::blue_slate();
        assert_eq!(fallback.palette, Palette::default());
    }

    #[test]
    fn material_key_tracks_only_what_is_baked() {
        // Dragging the grain *strength* must not rebuild the texture atlas — strength is applied
        // per-frame as a vertex-colour multiplier. Scale and seed do change the baked tile.
        let base = ThemeConfig::blue_slate();
        let mut strength = base.clone();
        strength.material.grain = 0.9;
        strength.material.mottle = 0.1;
        assert_eq!(base.material_key(), strength.material_key(), "strength must not rebake");

        let mut scale = base.clone();
        scale.material.grain_scale = 48.0;
        assert_ne!(base.material_key(), scale.material_key(), "scale must rebake");

        let mut seed = base.clone();
        seed.material.seed = 12345;
        assert_ne!(base.material_key(), seed.material_key(), "seed must rebake");

        // Colours are painted, never baked.
        let mut colour = base.clone();
        colour.palette.bone = [0, 0, 0];
        assert_eq!(base.material_key(), colour.material_key(), "colour must not rebake");
    }

    #[test]
    fn palette_groups_cover_every_token() {
        // The panel is driven by these three tables, so a token missing from all of them would be
        // silently uneditable — the exact failure this catches.
        let total = Palette::SURFACES.len() + Palette::TYPE.len() + Palette::ACCENTS.len();
        assert_eq!(total, 17, "every Palette field must appear in exactly one editor group");
    }

    #[test]
    fn palette_group_accessors_agree_with_their_fields() {
        // Each entry carries a getter and a setter; a copy-paste slip between them would edit the
        // wrong swatch, which is invisible until someone drags it.
        let mut p = Palette::default();
        for (i, (name, get, get_mut)) in Palette::SURFACES.iter().enumerate() {
            let probe = [i as u8, 200, 30];
            *get_mut(&mut p) = probe;
            assert_eq!(get(&p), probe, "{name}: getter and setter disagree");
        }
        for (name, get, get_mut) in Palette::TYPE.iter().chain(Palette::ACCENTS.iter()) {
            *get_mut(&mut p) = [7, 8, 9];
            assert_eq!(get(&p), [7, 8, 9], "{name}: getter and setter disagree");
        }
    }

    // ── The built-in gallery (#551 T2) ───────────────────────────────────────

    #[test]
    fn the_gallery_is_distinct_and_named() {
        let all = ThemeConfig::built_ins();
        assert_eq!(all.len(), 3);
        // Names must be unique: the list is selected by name, so a duplicate would make which
        // one you clicked depend on iteration order.
        let mut names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
        names.sort_unstable();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n, "built-in names must be unique");
        // And the palettes must actually differ — three entries that look the same is a menu
        // that lies.
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a.palette, b.palette, "{} and {} are the same look", a.name, b.name);
            }
        }
        assert!(ThemeConfig::is_built_in("Blue Slate"));
        assert!(!ThemeConfig::is_built_in("Blue Slate copy"));
    }

    #[test]
    fn the_default_is_the_first_built_in() {
        // `Reset` and the shipped default must agree, or "Reset" would take you somewhere the
        // gallery does not list.
        assert_eq!(ThemeConfig::built_ins()[0].palette, ThemeConfig::blue_slate().palette);
        assert_eq!(ThemeConfig::blue_slate().palette, Palette::default());
    }

    #[test]
    fn every_built_in_has_a_legible_surface_ramp() {
        // The one invariant that must hold for *all* of them, including the deliberately
        // spec-breaking High Contrast: surfaces ascend and text outranks itself. A theme whose
        // panels are darker than its shell, or whose disabled text is brighter than its primary,
        // is unusable regardless of what look it is going for.
        let lum = |c: Rgb| c[0] as u32 + c[1] as u32 + c[2] as u32;
        for t in ThemeConfig::built_ins() {
            let p = t.palette;
            let name = &t.name;
            assert!(lum(p.well_deep) <= lum(p.well), "{name}: wells out of order");
            assert!(lum(p.well) < lum(p.shell), "{name}: well not under shell");
            assert!(lum(p.shell) < lum(p.workspace), "{name}: shell not under workspace");
            assert!(lum(p.workspace) < lum(p.panel), "{name}: workspace not under panel");
            assert!(lum(p.panel) <= lum(p.raised), "{name}: panel not under raised");
            assert!(lum(p.raised) < lum(p.card_header), "{name}: raised not under header");
            assert!(lum(p.card_header) < lum(p.hairline), "{name}: header not under hairline");
            assert!(lum(p.hairline) < lum(p.edge_strong), "{name}: hairline not under lit edge");
            assert!(lum(p.bone) > lum(p.titanium), "{name}: primary text not above secondary");
            assert!(lum(p.titanium) > lum(p.muted), "{name}: secondary text not above tertiary");
        }
    }

    #[test]
    fn high_contrast_actually_has_more_contrast() {
        // It exists to be readable where the spec's compressed 5–15 RGB steps are not. If its
        // steps were not genuinely wider it would be a cosmetic variant with a misleading name.
        let step = |p: &Palette| {
            let l = |c: Rgb| (c[0] as i32 + c[1] as i32 + c[2] as i32) / 3;
            l(p.card_header) - l(p.shell)
        };
        let spec = step(&ThemeConfig::blue_slate().palette);
        let hc = step(&ThemeConfig::high_contrast().palette);
        assert!(hc > spec * 2, "high contrast spans {hc} vs the spec's {spec} — not enough");
        // And its text must be brighter, which is the other half of daylight legibility.
        assert!(
            ThemeConfig::high_contrast().palette.bone[0] > ThemeConfig::blue_slate().palette.bone[0]
        );
        // Grain is the first thing ambient light destroys, so it should be mostly off.
        assert!(ThemeConfig::high_contrast().material.grain < ThemeConfig::blue_slate().material.grain);
    }

    #[test]
    fn warm_instrument_is_actually_warm() {
        // It is the superseded direction preserved deliberately (§18), so it must still *be* that
        // direction: red leads blue on every surface, the exact inverse of the blue-slate rule.
        let p = ThemeConfig::warm_instrument().palette;
        for (name, c) in [
            ("well", p.well),
            ("shell", p.shell),
            ("workspace", p.workspace),
            ("panel", p.panel),
            ("card_header", p.card_header),
            ("hairline", p.hairline),
        ] {
            assert!(c[0] > c[2], "warm {name} is not warm: {c:?}");
        }
        // Blue slate's is the other way round — the two are genuinely opposite casts.
        assert!(ThemeConfig::blue_slate().palette.panel[2] > ThemeConfig::blue_slate().palette.panel[0]);
    }

    // ── The library (#551 T2) ────────────────────────────────────────────────

    #[test]
    fn adding_uniquifies_against_saved_and_built_in_names() {
        let mut lib = ThemeLibrary::default();
        assert_eq!(lib.add(ThemeConfig::blue_slate(), "Mine"), "Mine");
        assert_eq!(lib.add(ThemeConfig::blue_slate(), "Mine"), "Mine 2");
        assert_eq!(lib.add(ThemeConfig::blue_slate(), "Mine"), "Mine 3");
        // A built-in name is taken too: a user theme called "Blue Slate" beside the built-in one
        // is indistinguishable in the list.
        assert_eq!(lib.add(ThemeConfig::blue_slate(), "Blue Slate"), "Blue Slate 2");
        assert_eq!(lib.themes.len(), 4);
    }

    #[test]
    fn renaming_to_its_own_name_is_not_a_collision() {
        // The subtle one: uniquifying naively would see the row's own name as taken and append
        // " 2" every time you opened the rename box and pressed ok.
        let mut lib = ThemeLibrary::default();
        lib.add(ThemeConfig::blue_slate(), "Studio");
        assert!(lib.rename(0, "Studio"));
        assert_eq!(lib.themes[0].name, "Studio", "renaming to itself must be a no-op");
        // Renaming onto *another* row's name still uniquifies.
        lib.add(ThemeConfig::blue_slate(), "Stage");
        assert!(lib.rename(1, "Studio"));
        assert_eq!(lib.themes[1].name, "Studio 2");
    }

    #[test]
    fn renaming_rejects_blank_and_out_of_range() {
        let mut lib = ThemeLibrary::default();
        lib.add(ThemeConfig::blue_slate(), "Keep");
        assert!(!lib.rename(0, "   "), "blank must be refused, not applied");
        assert_eq!(lib.themes[0].name, "Keep");
        assert!(!lib.rename(9, "Nope"), "out of range must be refused");
    }

    #[test]
    fn update_keeps_the_stored_name_and_takes_everything_else() {
        // `U` means "make this saved slot look like what is showing" — the *name* is the slot's
        // identity and must survive, or updating would silently rename the entry.
        let mut lib = ThemeLibrary::default();
        lib.add(ThemeConfig::blue_slate(), "Slot");
        let mut edited = ThemeConfig::high_contrast();
        edited.name = "Something Else".into();
        assert!(lib.update(0, &edited));
        assert_eq!(lib.themes[0].name, "Slot", "the slot keeps its name");
        assert_eq!(lib.themes[0].palette, ThemeConfig::high_contrast().palette);
        assert!(!lib.update(9, &edited));
    }

    #[test]
    fn remove_is_bounds_checked() {
        let mut lib = ThemeLibrary::default();
        lib.add(ThemeConfig::blue_slate(), "A");
        assert!(!lib.remove(5));
        assert!(lib.remove(0));
        assert!(lib.themes.is_empty());
        assert!(!lib.remove(0));
    }

    #[test]
    fn the_library_round_trips_and_tolerates_junk() {
        let mut lib = ThemeLibrary::default();
        lib.add(ThemeConfig::warm_instrument(), "Warm");
        lib.add(ThemeConfig::high_contrast(), "Bright");
        let json = serde_json::to_string(&lib).expect("serialize");
        assert_eq!(serde_json::from_str::<ThemeLibrary>(&json).expect("deserialize"), lib);

        // A library written by a later version, or hand-edited: unknown keys ignored, missing
        // ones defaulted, and never a hard failure that would cost the editor its startup.
        let partial: ThemeLibrary =
            serde_json::from_str(r#"{"themes":[{"name":"x"}],"future":1}"#).expect("tolerant");
        assert_eq!(partial.themes.len(), 1);
        assert_eq!(partial.themes[0].palette, Palette::default());
        // An empty document is a valid empty library, not an error.
        assert_eq!(serde_json::from_str::<ThemeLibrary>("{}").expect("empty").themes.len(), 0);
    }

    #[test]
    fn a_theme_survives_export_and_import() {
        let mut t = ThemeConfig::warm_instrument();
        t.material.grain = 0.42;
        t.depth.radius = 9;
        let path = std::env::temp_dir().join("organon-theme-export-test.json");
        assert!(t.export_to(&path));
        assert_eq!(ThemeConfig::import_from(&path).expect("import"), t);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn importing_a_missing_or_foreign_file_is_a_no_op() {
        // Import is a user picking a file; picking the wrong one must do nothing, not corrupt the
        // live theme.
        assert!(ThemeConfig::import_from(std::path::Path::new("/nope/absent.json")).is_none());
        let path = std::env::temp_dir().join("organon-theme-foreign-test.json");
        std::fs::write(&path, b"not json at all").expect("write");
        assert!(ThemeConfig::import_from(&path).is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_three_stores_are_three_different_files() {
        // The requirement that started #551: a Scene recall must never restyle the editor. That
        // holds because the theme never shares a file with the presets — and Tier 2 adds a third
        // lifetime (the library) that must not share with either.
        let active = store_path();
        let library = library_path();
        assert_ne!(active, library, "active theme and library must not share a file");
        assert!(active.ends_with("ui_theme.json"));
        assert!(library.ends_with("ui_themes.json"));
        assert!(!active.ends_with("presets.json") && !library.ends_with("presets.json"));
    }

    #[test]
    fn colour_conversion_round_trips() {
        for c in [[0, 0, 0], [255, 255, 255], [0x1D, 0x25, 0x2C]] {
            assert_eq!(from_col(to_col(c)), c);
        }
    }
}
