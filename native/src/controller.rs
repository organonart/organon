//! Four-Quadrant Performance Controller (#356 Tier 1).
//!
//! Turns a Launchpad-style 8×8 RGB pad surface (default profile: **Novation
//! Launchpad Mini MK3**) into a hardware front-end for the #354 preset system:
//! each of the four 4×4 quadrants drives one **Scene component** (Generator /
//! Motion / Look / Environment) and each pad recalls one preset slot in that
//! component's list, beat-quantized through the #354 pending-recall scheduler.
//!
//! This module is deliberately **pure + host-agnostic** so it unit-tests without
//! nih-plug: it decodes raw MIDI into [`ControllerEvent`]s via a serializable
//! [`PadLayout`], and carries a wait-free [`Mailbox`] the audio thread uses to
//! hand raw events to the GUI thread (where the #354 recall — a GUI-only
//! `ParamSetter` path — actually runs). The *wiring* into `process()` + the
//! editor lives in `lib.rs`; the *policy* (routing, layout, persistence) lives
//! here.
//!
//! Design decisions pinned in #356 (see the issue for rationale):
//! - **Routed by MIDI note *number*, not name** — the layout is data, captured
//!   from the device (a default ships; a learn flow can re-capture it).
//! - **Pad ordering inside a quadrant** (#356 Open Q2): left-to-right,
//!   **bottom-to-top** — slot 0 is the quadrant's bottom-left pad. Chosen once,
//!   here, and locked by [`tests`].
//! - The Mini MK3 has **no velocity** (fixed) and its top-row / right-column
//!   buttons send **CC**, not notes; the default layout reflects that.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// A quadrant / Scene component. Maps 1:1 onto the four `EditorTab::SCENE`
/// members (#354). Index order matches the physical quadrant identity in #356's
/// factory-colour table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Component {
    /// Top-left quadrant (pink).
    Generator,
    /// Bottom-left quadrant (yellow).
    Motion,
    /// Top-right quadrant (green).
    Look,
    /// Bottom-right quadrant (blue).
    Environment,
}

impl Component {
    pub const ALL: [Component; 4] = [
        Component::Generator,
        Component::Motion,
        Component::Look,
        Component::Environment,
    ];

    /// Stable 0..3 index — keys the `PadLayout::pads` rows and the editor's
    /// per-quadrant feedback arrays.
    pub fn index(self) -> usize {
        match self {
            Component::Generator => 0,
            Component::Motion => 1,
            Component::Look => 2,
            Component::Environment => 3,
        }
    }

    pub fn from_index(i: usize) -> Option<Component> {
        Component::ALL.get(i).copied()
    }

    /// The #354 preset partition this quadrant recalls into.
    pub fn editor_tab(self) -> crate::preset::EditorTab {
        use crate::preset::EditorTab;
        match self {
            Component::Generator => EditorTab::Generator,
            Component::Motion => EditorTab::Motion,
            Component::Look => EditorTab::Look,
            Component::Environment => EditorTab::Environment,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Component::Generator => "Generator",
            Component::Motion => "Motion",
            Component::Look => "Look",
            Component::Environment => "Environment",
        }
    }
}

/// The four arrow buttons (top-left cluster on the Mini MK3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrowDir {
    Up,
    Down,
    Left,
    Right,
}

/// A decoded surface gesture. Tier 1 wires `Pad` (component recall), `Scene`
/// (whole-Scene recall via the right rail), `Arrow` (bank paging / division
/// stepping) and `Function` (cancel-pending). Mode switching (Session/Keys/User)
/// is Tier 2/3 and intentionally not decoded here yet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ControllerEvent {
    /// A quadrant pad: recall `component`'s preset at `slot` (0..15, before bank
    /// offset). `velocity` is forwarded for velocity-capable devices (0 or 127
    /// on the Mini MK3).
    Pad {
        component: Component,
        slot: u8,
        velocity: u8,
    },
    /// A right-column scene-launch button (0 = top).
    Scene { slot: u8 },
    /// An arrow press.
    Arrow(ArrowDir),
    /// The Stop/Solo/Mute button as a momentary function key.
    Function { pressed: bool },
}

/// Raw MIDI kind carried by the wait-free [`Mailbox`]. NoteOn with velocity 0 is
/// normalized to `NoteOff` at push time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RawKind {
    NoteOff,
    NoteOn,
    Cc,
}

/// A raw MIDI message small enough to pack into a `u32` for the lock-free ring.
/// The audio thread produces these with zero layout knowledge; the GUI thread
/// both routes them (via [`PadLayout::route`]) and feeds them to the learn panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RawMidi {
    pub kind: RawKind,
    /// Note number (NoteOn/Off) or CC number (Cc), 0..127.
    pub data1: u8,
    /// Velocity (NoteOn/Off) or CC value (Cc), 0..127.
    pub data2: u8,
    /// MIDI channel 0..15.
    pub channel: u8,
}

impl RawMidi {
    pub fn note_on(note: u8, velocity: u8, channel: u8) -> RawMidi {
        RawMidi {
            kind: if velocity > 0 { RawKind::NoteOn } else { RawKind::NoteOff },
            data1: note & 0x7f,
            data2: velocity & 0x7f,
            channel: channel & 0x0f,
        }
    }
    pub fn note_off(note: u8, channel: u8) -> RawMidi {
        RawMidi {
            kind: RawKind::NoteOff,
            data1: note & 0x7f,
            data2: 0,
            channel: channel & 0x0f,
        }
    }
    pub fn cc(cc: u8, value: u8, channel: u8) -> RawMidi {
        RawMidi {
            kind: RawKind::Cc,
            data1: cc & 0x7f,
            data2: value & 0x7f,
            channel: channel & 0x0f,
        }
    }

    /// Pack into a `u32`: `[kind:2][channel:4][data1:7][data2:7]` (20 bits used).
    fn pack(self) -> u32 {
        let k = match self.kind {
            RawKind::NoteOff => 0u32,
            RawKind::NoteOn => 1,
            RawKind::Cc => 2,
        };
        (k << 18)
            | ((self.channel as u32 & 0x0f) << 14)
            | ((self.data1 as u32 & 0x7f) << 7)
            | (self.data2 as u32 & 0x7f)
    }

    fn unpack(v: u32) -> Option<RawMidi> {
        let kind = match (v >> 18) & 0x3 {
            0 => RawKind::NoteOff,
            1 => RawKind::NoteOn,
            2 => RawKind::Cc,
            _ => return None,
        };
        Some(RawMidi {
            kind,
            channel: ((v >> 14) & 0x0f) as u8,
            data1: ((v >> 7) & 0x7f) as u8,
            data2: (v & 0x7f) as u8,
        })
    }
}

/// Sentinel channel = accept any MIDI channel (Tier 1 default; the exact
/// Drums-mode channel is #356 Open Q3, resolved on-Mac via the learn screen).
pub const ANY_CHANNEL: u8 = 0xff;

/// The device profile: which note number is which pad, and which CC is which
/// side button. Serialized next to `keymap.json` and re-capturable via the learn
/// panel. `Default` is the Novation Launchpad Mini MK3 factory-grid profile.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadLayout {
    /// `pads[component_index][slot]` = the pad's MIDI note number. Slot order is
    /// left-to-right, bottom-to-top within the quadrant (#356 Open Q2).
    pub pads: [[u8; 16]; 4],
    /// Arrow CC numbers: `[up, down, left, right]`.
    pub arrows: [u8; 4],
    /// Right-column scene-launch CC numbers (top → bottom), the top 7 buttons.
    pub scene: [u8; 7],
    /// Stop/Solo/Mute CC number (bottom of the right column).
    pub function: u8,
    /// Accepted MIDI channel (0..15), or [`ANY_CHANNEL`] to accept all.
    #[serde(default = "any_channel")]
    pub channel: u8,
}

fn any_channel() -> u8 {
    ANY_CHANNEL
}

impl Default for PadLayout {
    fn default() -> Self {
        PadLayout::mini_mk3()
    }
}

impl PadLayout {
    /// The factory Novation Launchpad Mini MK3 grid (Session / Programmer note
    /// convention: `note = 10·row + col`, row 1..8 bottom-to-top, col 1..8
    /// left-to-right), split into the four 4×4 quadrants of #356's colour table.
    ///
    /// ⚠️ The Mini MK3's *custom* modes are user-configurable, so these numbers
    /// are the documented best-guess default — verify/override on the Mac with a
    /// MIDI monitor or the Tier 1 learn screen.
    pub fn mini_mk3() -> Self {
        // (base_row, base_col) = the quadrant's lowest row / leftmost column.
        let quad = |base_row: u8, base_col: u8| -> [u8; 16] {
            let mut out = [0u8; 16];
            for slot in 0..16u8 {
                let local_row = slot / 4; // 0..3, bottom-to-top
                let local_col = slot % 4; // 0..3, left-to-right
                let row = base_row + local_row;
                let col = base_col + local_col;
                out[slot as usize] = row * 10 + col;
            }
            out
        };
        let mut pads = [[0u8; 16]; 4];
        pads[Component::Generator.index()] = quad(5, 1); // top-left
        pads[Component::Motion.index()] = quad(1, 1); // bottom-left
        pads[Component::Look.index()] = quad(5, 5); // top-right
        pads[Component::Environment.index()] = quad(1, 5); // bottom-right
        PadLayout {
            pads,
            // Mini MK3 top-row CCs: 91 up, 92 down, 93 left, 94 right.
            arrows: [91, 92, 93, 94],
            // Right-column scene CCs, top → bottom: 89,79,69,59,49,39,29.
            scene: [89, 79, 69, 59, 49, 39, 29],
            // Bottom of the right column.
            function: 19,
            channel: ANY_CHANNEL,
        }
    }

    /// Reverse-lookup: which quadrant + slot does this note number belong to?
    pub fn find_pad(&self, note: u8) -> Option<(Component, u8)> {
        for c in Component::ALL {
            if let Some(slot) = self.pads[c.index()].iter().position(|&n| n == note) {
                return Some((c, slot as u8));
            }
        }
        None
    }

    /// Decode a raw MIDI message into a surface gesture, honoring the channel
    /// filter. Returns `None` for anything not on the surface (or a note-off on a
    /// pad, which is a no-op in the latching model).
    pub fn route(&self, raw: RawMidi) -> Option<ControllerEvent> {
        if self.channel != ANY_CHANNEL && raw.channel != self.channel {
            return None;
        }
        match raw.kind {
            RawKind::NoteOn => {
                let (component, slot) = self.find_pad(raw.data1)?;
                Some(ControllerEvent::Pad {
                    component,
                    slot,
                    velocity: raw.data2,
                })
            }
            // Pad release is a no-op in the latching model; a note-off on a
            // non-pad is ignored.
            RawKind::NoteOff => None,
            RawKind::Cc => {
                // Buttons are momentary; act on the press edge (value > 0).
                if raw.data2 == 0 {
                    // The function key is reported on both edges so the editor
                    // can implement press-and-hold (Shift); everything else acts
                    // on press only.
                    if raw.data1 == self.function {
                        return Some(ControllerEvent::Function { pressed: false });
                    }
                    return None;
                }
                if raw.data1 == self.function {
                    return Some(ControllerEvent::Function { pressed: true });
                }
                if let Some(dir) = self.arrow_for(raw.data1) {
                    return Some(ControllerEvent::Arrow(dir));
                }
                if let Some(slot) = self.scene.iter().position(|&cc| cc == raw.data1) {
                    return Some(ControllerEvent::Scene { slot: slot as u8 });
                }
                None
            }
        }
    }

    fn arrow_for(&self, cc: u8) -> Option<ArrowDir> {
        if cc == self.arrows[0] {
            Some(ArrowDir::Up)
        } else if cc == self.arrows[1] {
            Some(ArrowDir::Down)
        } else if cc == self.arrows[2] {
            Some(ArrowDir::Left)
        } else if cc == self.arrows[3] {
            Some(ArrowDir::Right)
        } else {
            None
        }
    }

    /// Does the pad surface use this CC number (arrows / scene rail / function
    /// key), honoring the channel filter? Used by [`knob_claims`] to arbitrate
    /// CC collisions between the two surfaces — deliberately value-independent
    /// (a knob sweeping through 0 must not slip past a pad-owned CC).
    pub fn claims_cc(&self, raw: RawMidi) -> bool {
        if raw.kind != RawKind::Cc {
            return false;
        }
        if self.channel != ANY_CHANNEL && raw.channel != self.channel {
            return false;
        }
        raw.data1 == self.function
            || self.arrows.contains(&raw.data1)
            || self.scene.contains(&raw.data1)
    }
}

// ---------------------------------------------------------------------------
// Rotary knob layer (#448 Tier 1) — a Launch Control XL-style bank of 24
// encoders (3 rows × 8) that drives PARAMS, where the pad grid drives PRESETS.
// Same architecture as the pads: this module owns the pure policy (layout,
// learn, claim arbitration, pickup, persistence); `lib.rs` owns the wiring
// (the mailbox drain resolves a knob's target param and sets it through the
// host `ParamSetter`, so sliders follow and automation records).
//
// Two mapping modes (#448):
// - **Explore** — context-aware: the bank follows what the editor is focused
//   on (the selected generator's param block on the Generator tab; curated
//   Motion / Look / Environment banks on those tabs), numbered linearly onto
//   the 24 knobs, row-major.
// - **Performer** — hand-assigned: 24 slots each bound to any param by ID
//   (Ableton-macro-style), saved as named pages for a given set/track.
// ---------------------------------------------------------------------------

/// Knobs on the surface: 3 rows × 8 (the Launch Control XL's Send A / Send B /
/// Pan-Device rows).
pub const KNOB_COUNT: usize = 24;
pub const KNOB_COLS: usize = 8;

/// How the knob bank picks its 24 target params.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum KnobMode {
    /// Follow the editor's focus (tab + selected generator).
    #[default]
    Explore,
    /// A hand-assigned page of 24 param bindings.
    Performer,
}

/// The knob device profile: which CC number is which knob (row-major, top-left
/// = 0), plus the accepted channel. `Default` is the Novation Launch Control XL
/// factory-template rows.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct KnobLayout {
    /// `ccs[row * KNOB_COLS + col]` = the knob's CC number.
    pub ccs: [u8; KNOB_COUNT],
    /// Accepted MIDI channel (0..15), or [`ANY_CHANNEL`]. The learn flow adopts
    /// the device's channel, which is what disambiguates the LCXL's factory CCs
    /// from the Launchpad's side buttons (they collide on 19/29/49).
    #[serde(default = "any_channel")]
    pub channel: u8,
}

impl Default for KnobLayout {
    fn default() -> Self {
        KnobLayout::launch_control_xl()
    }
}

impl KnobLayout {
    /// The Launch Control XL factory-template knob CCs: Send A row 13–20,
    /// Send B row 29–36, Pan/Device row 49–56.
    ///
    /// ⚠️ Documented best-guess default (factory templates transmit on channel
    /// 9–16, user templates 1–8, and user templates are re-mappable) — verify /
    /// re-capture on the Mac with the learn flow, exactly like the pad grid.
    pub fn launch_control_xl() -> Self {
        let mut ccs = [0u8; KNOB_COUNT];
        for col in 0..KNOB_COLS {
            ccs[col] = 13 + col as u8;
            ccs[KNOB_COLS + col] = 29 + col as u8;
            ccs[2 * KNOB_COLS + col] = 49 + col as u8;
        }
        KnobLayout { ccs, channel: ANY_CHANNEL }
    }

    /// Reverse-lookup: which knob index (0..24, row-major) is this CC?
    pub fn find_knob(&self, cc: u8) -> Option<usize> {
        self.ccs.iter().position(|&c| c == cc)
    }

    /// One learn step: capture knobs by twisting them in ORDER (row-major, left
    /// to right, top row first). Returns the new "next index". An encoder twist
    /// streams many messages of the same CC, so a CC already captured for an
    /// earlier knob is the previous knob still turning — ignored, the walk only
    /// advances on a NEW CC number. Adopts the device's channel.
    pub fn learn_capture(&mut self, next: usize, raw: RawMidi) -> usize {
        if raw.kind != RawKind::Cc || next >= KNOB_COUNT {
            return next;
        }
        if self.ccs[..next].contains(&raw.data1) {
            return next;
        }
        self.ccs[next] = raw.data1;
        self.channel = raw.channel;
        next + 1
    }
}

/// Arbitrate an incoming CC between the two surfaces: `Some(knob index)` when
/// the knob bank owns it. Once the knob layout has a learned channel it owns
/// its CCs on that channel outright; at [`ANY_CHANNEL`] (never learned) the pad
/// surface keeps every CC it routes, because the LCXL factory rows collide with
/// the Launchpad Mini MK3's function/scene CCs (19, 29, 49) — running learn on
/// either surface resolves the ambiguity.
pub fn knob_claims(knobs: &KnobLayout, pads: &PadLayout, raw: RawMidi) -> Option<usize> {
    if raw.kind != RawKind::Cc {
        return None;
    }
    if knobs.channel != ANY_CHANNEL {
        if raw.channel != knobs.channel {
            return None;
        }
        return knobs.find_knob(raw.data1);
    }
    if pads.claims_cc(raw) {
        return None;
    }
    knobs.find_knob(raw.data1)
}

/// Soft-takeover ("pickup"): should this absolute CC value drive the param?
/// All values normalized 0..1. Engaged stays engaged (per context, reset by the
/// caller on a context switch); pickup off = always engaged; otherwise engage
/// when the knob lands near the param's current value or sweeps across it —
/// prevents a recalled preset from jumping to wherever the knob was left.
pub fn pickup_engaged(
    pickup: bool,
    engaged: bool,
    last_cc: Option<f32>,
    cc: f32,
    current: f32,
) -> bool {
    if engaged || !pickup {
        return true;
    }
    if (cc - current).abs() <= 0.04 {
        return true;
    }
    match last_cc {
        // Crossed the param's value between two consecutive messages.
        Some(prev) => (prev <= current && cc >= current) || (prev >= current && cc <= current),
        None => false,
    }
}

/// One Performer-mode page: 24 slots, each optionally bound to a param by its
/// nih-plug ID (the stable wire ID, e.g. `"metl"`), row-major like the layout.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct KnobPage {
    pub name: String,
    pub slots: [Option<String>; KNOB_COUNT],
}

impl KnobPage {
    pub fn new(name: &str) -> Self {
        KnobPage { name: name.to_string(), slots: std::array::from_fn(|_| None) }
    }
}

/// The persisted knob-bank configuration (`knobs.json`, beside
/// `controller.json`): device layout, mode, pickup, and the Performer pages.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct KnobConfig {
    #[serde(default)]
    pub layout: KnobLayout,
    #[serde(default)]
    pub mode: KnobMode,
    #[serde(default = "default_true")]
    pub pickup: bool,
    #[serde(default)]
    pub pages: Vec<KnobPage>,
    #[serde(default)]
    pub active_page: usize,
}

fn default_true() -> bool {
    true
}

impl Default for KnobConfig {
    fn default() -> Self {
        KnobConfig {
            layout: KnobLayout::default(),
            mode: KnobMode::default(),
            pickup: true,
            pages: vec![KnobPage::new("Page 1")],
            active_page: 0,
        }
    }
}

impl KnobConfig {
    /// The active Performer page, clamped (a corrupt/stale index falls back to
    /// page 0; pages is never empty after `load_knobs`).
    pub fn page(&self) -> &KnobPage {
        &self.pages[self.active_page.min(self.pages.len().saturating_sub(1))]
    }
    pub fn page_mut(&mut self) -> &mut KnobPage {
        let i = self.active_page.min(self.pages.len().saturating_sub(1));
        &mut self.pages[i]
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(knobs_path(), json);
        }
    }
}

fn knobs_path() -> std::path::PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("knobs.json")
}

/// Load the saved knob config, or the default if none/corrupt. Guarantees at
/// least one Performer page exists.
pub fn load_knobs() -> KnobConfig {
    let mut cfg: KnobConfig = std::fs::read_to_string(knobs_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if cfg.pages.is_empty() {
        cfg.pages.push(KnobPage::new("Page 1"));
    }
    cfg
}

// ---------------------------------------------------------------------------
// Persistence — mirrors `keymap.rs`: a best-effort JSON blob next to keymap.json
// in the OrganicMath app-support directory.
// ---------------------------------------------------------------------------

fn config_path() -> std::path::PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("controller.json")
}

/// Load the saved profile, or the Mini MK3 default if none/corrupt.
pub fn load() -> PadLayout {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

impl PadLayout {
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(config_path(), json);
        }
    }
}

// ---------------------------------------------------------------------------
// Explore-mode contexts (#448): what the 24 knobs point at, given the editor's
// focus. Two shapes: a RANGE of the param declaration order (a generator's
// contiguous block in `params.rs`, addressed by stable wire IDs — first ID to
// end-exclusive ID, `None` = to the end of the map), or a curated LIST of
// exactly 24 IDs (the cross-cutting tabs, whose params are scattered).
// `lib.rs` resolves either against `Params::param_map()`, whose order IS the
// declaration order. The IDs are wire-stable by project rule (presets/hosts
// depend on them), so these tables don't rot when params are renamed — only a
// *reordering* of `params.rs` blocks could break a range, and the tests below
// pin every anchor's existence (`lib.rs` pins the resolution).
// ---------------------------------------------------------------------------

/// An Explore-mode knob context: a declaration-order range or a curated list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KnobContext {
    /// Params from `first` (inclusive) to `end` (exclusive; `None` = map end),
    /// in declaration order, clamped to [`KNOB_COUNT`].
    Range(&'static str, Option<&'static str>),
    /// Exactly 24 hand-picked param IDs, row-major.
    List(&'static [&'static str; KNOB_COUNT]),
}

/// The selected generator's param block (its card's core dials, in declaration
/// order). Anchors are the block's first param ID; the end anchor is the next
/// block's first ID. Blocks longer than 24 clamp; shorter leave knobs unmapped.
pub fn generator_knob_context(g: crate::params::GeneratorMode) -> KnobContext {
    use crate::params::GeneratorMode as G;
    let (first, end) = match g {
        // `None (off)` keeps the classic cube-field dials — harmless + useful.
        G::Original | G::None => ("lcx", Some("fnst")),
        G::Frenet => ("fnst", Some("dnfm")),
        G::Dna => ("dnfm", Some("atfd")),
        G::Attractor => ("atfd", Some("bdct")),
        G::Boids => ("bdct", Some("hm0m")),
        // Includes the soft-body bell block (a physical mode on this generator).
        G::Harmonic => ("hm0m", Some("lssy")),
        G::LSystem => ("lssy", Some("cnsn")),
        G::CurlNoise => ("cnsn", Some("plrn")),
        G::Polarization => ("plrn", Some("mxln")),
        // The core Maxwell block is >24 params, so it clamps well before FDTD.
        G::MaxwellField => ("mxln", Some("acsk")),
        G::Acoustic => ("acsk", Some("fekd")),
        G::FieldEngine => ("fekd", Some("makd")),
        G::MapAttractor => ("makd", Some("snon")),
        G::AxonWaveguide => ("awct", Some("nwtp")),
        G::NeuralNetwork => ("nwtp", Some("ntsz")),
        G::Demo => ("dmsn", Some("syrd")),
        G::Synchrotron => ("syrd", Some("vfpr")),
        G::VectorField => ("vfpr", Some("vbx1f")),
        G::Phyllotaxis => ("physf", Some("tsfam")),
        G::Tessellation => ("tsfam", Some("mbpw")),
        G::Mandelbulb => ("mbpw", Some("crfm")),
        // #476 Creature Engine block sits between Mandelbulb and Minimal in the
        // param table (declared after mb_bailout), so its range is crfm..msfa.
        G::Creature => ("crfm", Some("msfa")),
        G::MinimalSurface => ("msfa", Some("lnfo")),
        G::Lens => ("lnfo", Some("kfsp2")),
        G::Kaleidoscope => ("kfsp2", Some("kalo")),
        // The SIREN neural-field block is the tail of the param struct.
        G::NeuralField => ("nnen", None),
    };
    KnobContext::Range(first, end)
}

/// Motion tab: animation clock + pulse + routing + speed-pulse/breath envelopes
/// + the auto-orbit camera core. Row 1 = clock/pulse, row 2 = routing +
/// envelopes, row 3 = camera.
pub const MOTION_KNOBS: [&str; KNOB_COUNT] = [
    "anim", "incs", "incp", "puls", "tsyn", "tmpo", "psrc", "mat", //
    "mad", "mbt", "mbd", "spamt", "spatk", "spdec", "brama", "bratk", //
    "brdec", "cpth", "cspd", "ckik", "cdmp", "camt", "cdpb", "cddp",
];

/// Look tab: material core + colour (HSV) + surface FX + bloom/tone-map +
/// bioluminescence + cinematic finishing.
pub const LOOK_KNOBS: [&str; KNOB_COUNT] = [
    "mtyp", "metl", "rough", "ior", "expo", "glow", "emis", "opac", //
    "mhue", "msat", "mval", "mhcy", "sss", "irid", "pal", "blmi", //
    "blmt", "tmop", "cyc", "rpli", "rpls", "halamt", "lfamt", "envi",
];

/// Environment tab: IBL/tint + backdrop + the world layers (terrain / stars /
/// atmosphere / clouds / ocean) + scene HSV + the direct key/fill lights.
pub const ENVIRONMENT_KNOBS: [&str; KNOB_COUNT] = [
    "envi", "envr", "etnh", "etna", "bgsh", "bgin", "bgtm", "tren", //
    "trht", "sten", "stbr", "atmen", "clen", "ccov", "ocen", "shue", //
    "ssat", "sval", "shcy", "amb", "key", "fill", "elev", "azim",
];

/// The focused-tab context (Explore mode). Tabs without a natural bank of
/// their own (Audio / Settings / Mind) fall back to the generator block, so
/// the knobs never go dead.
pub fn explore_knob_context(
    tab: crate::preset::UiTab,
    generator: crate::params::GeneratorMode,
) -> KnobContext {
    use crate::preset::UiTab;
    match tab {
        UiTab::Motion => KnobContext::List(&MOTION_KNOBS),
        UiTab::Look => KnobContext::List(&LOOK_KNOBS),
        UiTab::Environment => KnobContext::List(&ENVIRONMENT_KNOBS),
        // The Duo-Field synth "Sound" blocks (#339 Tiers 1–4 + the visual lens).
        UiTab::Synth => KnobContext::Range("snon", Some("acmd")),
        UiTab::Generator | UiTab::Audio | UiTab::Settings | UiTab::Mind => {
            generator_knob_context(generator)
        }
    }
}

// ---------------------------------------------------------------------------
// Wait-free SPSC mailbox — audio thread (single producer) → GUI thread (single
// consumer). Carries raw MIDI; overflow drops the newest event (a control
// surface can tolerate a lost press far better than a stalled queue).
// ---------------------------------------------------------------------------

const CAP: usize = 256;
const MASK: usize = CAP - 1;

/// A fixed-capacity, allocation-free single-producer/single-consumer ring of raw
/// MIDI events. Safe for exactly one producer thread and one consumer thread.
pub struct Mailbox {
    slots: [AtomicU32; CAP],
    /// Consumer index (monotonic, wrapping).
    head: AtomicUsize,
    /// Producer index (monotonic, wrapping).
    tail: AtomicUsize,
}

impl Default for Mailbox {
    fn default() -> Self {
        Mailbox::new()
    }
}

impl Mailbox {
    pub fn new() -> Self {
        Mailbox {
            slots: std::array::from_fn(|_| AtomicU32::new(0)),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Producer side (audio thread). Never blocks; drops the event if the ring
    /// is full. Returns `false` if dropped.
    pub fn push(&self, raw: RawMidi) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) >= CAP {
            return false; // full
        }
        self.slots[tail & MASK].store(raw.pack(), Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Consumer side (GUI thread). Returns the oldest queued event, or `None`.
    pub fn pop(&self) -> Option<RawMidi> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let packed = self.slots[head & MASK].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        RawMidi::unpack(packed)
    }

    /// Consumer side: discard everything currently queued (used when the editor
    /// (re)opens so stale presses from a closed-editor window don't fire).
    pub fn drain(&self) {
        let tail = self.tail.load(Ordering::Acquire);
        self.head.store(tail, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_has_64_distinct_pad_notes() {
        let l = PadLayout::default();
        let mut seen = std::collections::BTreeSet::new();
        for c in Component::ALL {
            for &note in &l.pads[c.index()] {
                assert!(note >= 1, "note 0 is unassigned");
                assert!(seen.insert(note), "duplicate pad note {note}");
            }
        }
        assert_eq!(seen.len(), 64);
    }

    #[test]
    fn mini_mk3_known_corners_route_correctly() {
        let l = PadLayout::mini_mk3();
        // Quadrant bottom-left pad = slot 0; documented note numbers.
        let cases = [
            (51u8, Component::Generator, 0u8), // top-left quadrant, bottom-left pad
            (84, Component::Generator, 15),    // top-left quadrant, top-right pad
            (11, Component::Motion, 0),
            (44, Component::Motion, 15),
            (55, Component::Look, 0),
            (88, Component::Look, 15),
            (15, Component::Environment, 0),
            (48, Component::Environment, 15),
        ];
        for (note, comp, slot) in cases {
            assert_eq!(l.find_pad(note), Some((comp, slot)), "note {note}");
            assert_eq!(
                l.route(RawMidi::note_on(note, 127, 0)),
                Some(ControllerEvent::Pad {
                    component: comp,
                    slot,
                    velocity: 127
                }),
                "route note {note}"
            );
        }
    }

    #[test]
    fn slot_ordering_is_left_to_right_bottom_to_top() {
        let l = PadLayout::mini_mk3();
        let gen = &l.pads[Component::Generator.index()];
        // slot 0 (bottom-left) → 3 (bottom-right) are one row; slot 4 starts the
        // next row up.
        assert_eq!(gen[0], 51);
        assert_eq!(gen[3], 54);
        assert_eq!(gen[4], 61);
    }

    #[test]
    fn note_off_and_unmapped_notes_route_to_none() {
        let l = PadLayout::mini_mk3();
        assert_eq!(l.route(RawMidi::note_off(51, 0)), None);
        assert_eq!(l.route(RawMidi::note_on(127, 100, 0)), None); // not a pad
        assert_eq!(l.route(RawMidi::note_on(51, 0, 0)), None); // vel 0 = note off
    }

    #[test]
    fn control_ccs_route_to_gestures() {
        let l = PadLayout::mini_mk3();
        assert_eq!(
            l.route(RawMidi::cc(91, 127, 0)),
            Some(ControllerEvent::Arrow(ArrowDir::Up))
        );
        assert_eq!(
            l.route(RawMidi::cc(94, 127, 0)),
            Some(ControllerEvent::Arrow(ArrowDir::Right))
        );
        assert_eq!(
            l.route(RawMidi::cc(89, 127, 0)),
            Some(ControllerEvent::Scene { slot: 0 })
        );
        assert_eq!(
            l.route(RawMidi::cc(29, 127, 0)),
            Some(ControllerEvent::Scene { slot: 6 })
        );
        assert_eq!(
            l.route(RawMidi::cc(19, 127, 0)),
            Some(ControllerEvent::Function { pressed: true })
        );
        assert_eq!(
            l.route(RawMidi::cc(19, 0, 0)),
            Some(ControllerEvent::Function { pressed: false })
        );
        // Arrow release (value 0) is a no-op.
        assert_eq!(l.route(RawMidi::cc(91, 0, 0)), None);
    }

    #[test]
    fn channel_filter_rejects_other_channels() {
        let mut l = PadLayout::mini_mk3();
        l.channel = 5;
        assert!(l.route(RawMidi::note_on(51, 127, 5)).is_some());
        assert_eq!(l.route(RawMidi::note_on(51, 127, 4)), None);
        // ANY accepts all.
        l.channel = ANY_CHANNEL;
        assert!(l.route(RawMidi::note_on(51, 127, 9)).is_some());
    }

    #[test]
    fn raw_midi_pack_roundtrips() {
        let cases = [
            RawMidi::note_on(51, 127, 0),
            RawMidi::note_off(84, 15),
            RawMidi::cc(91, 64, 3),
            RawMidi::note_on(0, 1, 15),
            RawMidi::cc(127, 127, 15),
        ];
        for r in cases {
            assert_eq!(RawMidi::unpack(r.pack()), Some(r), "{r:?}");
        }
    }

    #[test]
    fn mailbox_is_fifo_and_bounded() {
        let mb = Mailbox::new();
        assert_eq!(mb.pop(), None);
        let a = RawMidi::note_on(51, 127, 0);
        let b = RawMidi::cc(91, 127, 0);
        assert!(mb.push(a));
        assert!(mb.push(b));
        assert_eq!(mb.pop(), Some(a));
        assert_eq!(mb.pop(), Some(b));
        assert_eq!(mb.pop(), None);
    }

    #[test]
    fn mailbox_drops_when_full_and_survives_wrap() {
        let mb = Mailbox::new();
        // Fill exactly to capacity.
        for i in 0..CAP {
            assert!(mb.push(RawMidi::cc((i % 128) as u8, 127, 0)));
        }
        // Next push is dropped.
        assert!(!mb.push(RawMidi::cc(1, 127, 0)));
        // Drain half, then push past the wrap boundary — must stay FIFO.
        for i in 0..CAP / 2 {
            assert_eq!(mb.pop(), Some(RawMidi::cc((i % 128) as u8, 127, 0)));
        }
        for i in 0..CAP / 2 {
            assert!(mb.push(RawMidi::note_on(60, ((i % 127) + 1) as u8, 0)));
        }
        // The second half of the original fill still comes out first.
        for i in CAP / 2..CAP {
            assert_eq!(mb.pop(), Some(RawMidi::cc((i % 128) as u8, 127, 0)));
        }
    }

    #[test]
    fn mailbox_drain_clears_pending() {
        let mb = Mailbox::new();
        mb.push(RawMidi::note_on(51, 127, 0));
        mb.push(RawMidi::note_on(52, 127, 0));
        mb.drain();
        assert_eq!(mb.pop(), None);
    }

    // --- Rotary knob layer (#448 Tier 1) ---

    #[test]
    fn lcxl_default_has_24_distinct_ccs_in_factory_rows() {
        let l = KnobLayout::launch_control_xl();
        let mut seen = std::collections::BTreeSet::new();
        for &cc in &l.ccs {
            assert!(seen.insert(cc), "duplicate knob CC {cc}");
        }
        assert_eq!(seen.len(), KNOB_COUNT);
        // Documented factory rows: Send A 13–20, Send B 29–36, Pan/Device 49–56.
        assert_eq!(l.ccs[0], 13);
        assert_eq!(l.ccs[7], 20);
        assert_eq!(l.ccs[8], 29);
        assert_eq!(l.ccs[15], 36);
        assert_eq!(l.ccs[16], 49);
        assert_eq!(l.ccs[23], 56);
        assert_eq!(l.channel, ANY_CHANNEL);
        assert_eq!(l.find_knob(13), Some(0));
        assert_eq!(l.find_knob(56), Some(23));
        assert_eq!(l.find_knob(21), None);
    }

    #[test]
    fn knob_learn_walks_in_order_and_ignores_repeats() {
        let mut l = KnobLayout::launch_control_xl();
        let mut next = 0;
        // Twist knob 1 (CC 77 on channel 8) — several messages stream out.
        next = l.learn_capture(next, RawMidi::cc(77, 10, 8));
        assert_eq!(next, 1);
        next = l.learn_capture(next, RawMidi::cc(77, 11, 8));
        next = l.learn_capture(next, RawMidi::cc(77, 12, 8));
        assert_eq!(next, 1, "same-CC repeats must not advance the walk");
        // Twist knob 2 (CC 78).
        next = l.learn_capture(next, RawMidi::cc(78, 60, 8));
        assert_eq!(next, 2);
        assert_eq!(l.ccs[0], 77);
        assert_eq!(l.ccs[1], 78);
        assert_eq!(l.channel, 8, "learn adopts the device's channel");
        // A note during learn is ignored.
        assert_eq!(l.learn_capture(next, RawMidi::note_on(60, 100, 8)), 2);
        // Walk the rest to completion with distinct CCs.
        for i in 2..KNOB_COUNT {
            next = l.learn_capture(next, RawMidi::cc(90 + i as u8, 1, 8));
        }
        assert_eq!(next, KNOB_COUNT);
        // Completed walk: further messages are no-ops.
        assert_eq!(l.learn_capture(next, RawMidi::cc(5, 1, 8)), KNOB_COUNT);
        let mut seen = std::collections::BTreeSet::new();
        assert!(l.ccs.iter().all(|&c| seen.insert(c)), "learned CCs distinct");
    }

    #[test]
    fn knob_claims_arbitrates_the_lcxl_launchpad_collisions() {
        let pads = PadLayout::mini_mk3();
        let knobs = KnobLayout::launch_control_xl(); // channel = ANY
        // CC 49 is BOTH an LCXL Pan-row knob and a Launchpad scene button: at
        // ANY_CHANNEL the pads keep it (value-independent — even at value 0).
        assert_eq!(knob_claims(&knobs, &pads, RawMidi::cc(49, 100, 0)), None);
        assert_eq!(knob_claims(&knobs, &pads, RawMidi::cc(49, 0, 0)), None);
        assert_eq!(knob_claims(&knobs, &pads, RawMidi::cc(19, 64, 0)), None); // function key
        // A non-colliding knob CC routes to its knob even at ANY.
        assert_eq!(knob_claims(&knobs, &pads, RawMidi::cc(13, 64, 0)), Some(0));
        assert_eq!(knob_claims(&knobs, &pads, RawMidi::cc(56, 64, 5)), Some(23));
        // Not a knob CC at all.
        assert_eq!(knob_claims(&knobs, &pads, RawMidi::cc(91, 64, 0)), None);
        // Once the knob channel is learned, the knobs own their CCs on that
        // channel outright — the collision is resolved.
        let mut learned = knobs.clone();
        learned.channel = 8;
        assert_eq!(knob_claims(&learned, &pads, RawMidi::cc(49, 64, 8)), Some(16));
        assert_eq!(knob_claims(&learned, &pads, RawMidi::cc(49, 64, 0)), None);
        // Notes never claim.
        assert_eq!(knob_claims(&learned, &pads, RawMidi::note_on(49, 64, 8)), None);
    }

    #[test]
    fn pickup_engages_near_on_crossing_or_when_off() {
        // Pickup off → always engaged.
        assert!(pickup_engaged(false, false, None, 0.9, 0.1));
        // Already engaged stays engaged.
        assert!(pickup_engaged(true, true, None, 0.9, 0.1));
        // First message far from the param → not engaged.
        assert!(!pickup_engaged(true, false, None, 0.9, 0.1));
        // Near the param → engaged.
        assert!(pickup_engaged(true, false, None, 0.12, 0.1));
        // Sweeping across the param between two messages → engaged (both ways).
        assert!(pickup_engaged(true, false, Some(0.0), 0.5, 0.3));
        assert!(pickup_engaged(true, false, Some(0.8), 0.2, 0.5));
        // Sweeping on one side without crossing → still waiting.
        assert!(!pickup_engaged(true, false, Some(0.9), 0.7, 0.1));
    }

    #[test]
    fn knob_config_roundtrips_and_backfills_pages() {
        let mut cfg = KnobConfig::default();
        assert_eq!(cfg.pages.len(), 1);
        cfg.mode = KnobMode::Performer;
        cfg.pages[0].slots[0] = Some("metl".to_string());
        cfg.pages.push(KnobPage::new("Chrome jam"));
        cfg.active_page = 1;
        let json = serde_json::to_string(&cfg).unwrap();
        let back: KnobConfig = serde_json::from_str(&json).unwrap();
        assert!(back == cfg, "knob config JSON roundtrip");
        // Old/foreign JSON with no pages: every field serde-defaults, and the
        // page accessor still can't panic (guarded by `min`).
        let sparse: KnobConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(sparse.layout, KnobLayout::launch_control_xl());
        assert!(sparse.pickup);
        assert_eq!(sparse.pages.len(), 0);
    }

    #[test]
    fn knob_page_accessor_clamps_a_stale_index() {
        let mut cfg = KnobConfig::default();
        cfg.pages.push(KnobPage::new("Two"));
        cfg.active_page = 7; // stale (pages shrank on another machine)
        assert_eq!(cfg.page().name, "Two");
        cfg.page_mut().slots[3] = Some("rough".to_string());
        assert_eq!(cfg.pages[1].slots[3].as_deref(), Some("rough"));
    }

    #[test]
    fn explore_contexts_cover_every_tab_and_generator() {
        use crate::params::GeneratorMode;
        use crate::preset::UiTab;
        use nih_plug::prelude::Enum;
        // Every generator resolves to a Range whose anchors are non-empty IDs.
        for i in 0..GeneratorMode::variants().len() {
            let g = GeneratorMode::from_index(i);
            match generator_knob_context(g) {
                KnobContext::Range(first, end) => {
                    assert!(!first.is_empty());
                    if let Some(e) = end {
                        assert_ne!(first, e, "{g:?}: empty range");
                    }
                }
                KnobContext::List(_) => panic!("{g:?}: generators use ranges"),
            }
        }
        // Every tab resolves (fallback tabs ride the generator context).
        for tab in [
            UiTab::Generator,
            UiTab::Motion,
            UiTab::Environment,
            UiTab::Look,
            UiTab::Synth,
            UiTab::Audio,
            UiTab::Settings,
            UiTab::Mind,
        ] {
            let _ = explore_knob_context(tab, GeneratorMode::Original);
        }
        // The curated lists hold exactly-24 distinct IDs each.
        for list in [&MOTION_KNOBS, &LOOK_KNOBS, &ENVIRONMENT_KNOBS] {
            let mut seen = std::collections::BTreeSet::new();
            for id in list.iter() {
                assert!(seen.insert(*id), "duplicate knob id {id}");
            }
        }
    }

    #[test]
    fn every_knob_context_id_exists_in_the_param_map() {
        use crate::params::{GeneratorMode, OrganicMathParams};
        use nih_plug::prelude::{Enum, Params};
        let p = OrganicMathParams::default();
        let ids: std::collections::BTreeSet<String> =
            p.param_map().into_iter().map(|(id, _, _)| id).collect();
        let mut check = |id: &str, whence: &str| {
            assert!(ids.contains(id), "knob context id {id:?} ({whence}) is not a param id");
        };
        for list in [&MOTION_KNOBS, &LOOK_KNOBS, &ENVIRONMENT_KNOBS] {
            for id in list.iter() {
                check(id, "curated tab list");
            }
        }
        for i in 0..GeneratorMode::variants().len() {
            let g = GeneratorMode::from_index(i);
            if let KnobContext::Range(first, end) = generator_knob_context(g) {
                check(first, "generator range start");
                if let Some(e) = end {
                    check(e, "generator range end");
                }
            }
        }
        if let KnobContext::Range(first, Some(end)) =
            explore_knob_context(crate::preset::UiTab::Synth, GeneratorMode::Original)
        {
            check(first, "synth range");
            check(end, "synth range");
        }
    }

    #[test]
    fn component_tab_mapping_is_the_scene_partition() {
        use crate::preset::EditorTab;
        assert_eq!(Component::Generator.editor_tab(), EditorTab::Generator);
        assert_eq!(Component::Motion.editor_tab(), EditorTab::Motion);
        assert_eq!(Component::Look.editor_tab(), EditorTab::Look);
        assert_eq!(Component::Environment.editor_tab(), EditorTab::Environment);
        // Every quadrant maps into the four-member Scene set.
        for c in Component::ALL {
            assert!(c.editor_tab().in_scene());
        }
    }
}
