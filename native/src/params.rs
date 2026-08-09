//! The plugin's parameters. Declaring them with nih-plug's `#[derive(Params)]`
//! means the host (Ableton) sees every one of them as an automatable,
//! MIDI-mappable control — no custom mapping engine needed. The visual window
//! reads the live values via `values()`.

use glam::Vec3;
use nih_plug::prelude::*;

/// #626 T3 PR B — the algorithm's `FuncName` lives in `organon-core` now; re-exported
/// here so `crate::params::FuncName` (world.rs, and math.rs before it moved) still
/// resolves, and resolves to the **semantic** type rather than the host adapter.
/// `HostFuncName` below is the nih-plug-facing mirror; see its doc comment.
pub use organon_core::params::{FuncName, ParamValues};
use nih_plug_egui::EguiState;
use std::sync::Arc;

/// #354 — give preset-captured enums a `to_u32`/`from_u32` pair by delegating to
/// nih-plug's derived `Enum` index (declaration order). The generator/look enums
/// hand-write these; these (Environment/Settings/Audio) enums only needed them
/// once presets started capturing them, so delegate rather than duplicate.
macro_rules! enum_u32_via_index {
    ($($t:ty),* $(,)?) => { $(
        impl $t {
            #[inline] pub fn to_u32(self) -> u32 {
                nih_plug::prelude::Enum::to_index(self) as u32
            }
            #[inline] pub fn from_u32(v: u32) -> Self {
                nih_plug::prelude::Enum::from_index(v as usize)
            }
        }
    )* };
}
/// For enums that already hand-write `to_u32` (as `self as u32` / a declaration-
/// order match) but lack the inverse — the derived `from_index` inverts it.
macro_rules! enum_from_u32_via_index {
    ($($t:ty),* $(,)?) => { $(
        impl $t {
            #[inline] pub fn from_u32(v: u32) -> Self {
                nih_plug::prelude::Enum::from_index(v as usize)
            }
        }
    )* };
}
// Lack both: give them the index-based pair.
enum_u32_via_index!(TerrainRes, Msaa, SpectrumMode);
// Have `to_u32` (declaration-order) already: add only the inverse.
enum_from_u32_via_index!(TerrainNoise, TerrainPalette, MeterWeighting, MeterAveraging);
// (AspectPreset already hand-writes both.)

/// The waveshaper applied to a phasor — a synth oscillator's wave family. `Sin`/
/// `Cos` are the smooth defaults; `Tan`/`Log` are exotic curves; `Triangle`/
/// `Square`/`Saw` are the classic synth shapes. All map phase → value and slot
/// into `math::apply_func` wherever a function is used (translation deformation
/// + pendulum rotation). New variants are appended so existing param/preset
/// indices stay stable.
/// **Host-facing mirror of [`organon_core::params::FuncName`]** (#626 T3 PR B).
///
/// Exists for exactly one reason: `EnumParam<T>` requires `T: nih_plug::Enum`, and the
/// **orphan rule** forbids `organic-math-native` from implementing that foreign trait
/// for core's foreign type. So the host side owns an adapter carrying the derive, and
/// core owns the plain semantic type the algorithm uses.
///
/// ⚠️ **This list and core's MUST stay identical, in order.** The index is the wire
/// format — it is what rides `Shared` and what presets store — so a reorder here
/// silently repoints every saved preset and automation lane at a different waveform.
/// `host_func_name_mirrors_core` pins them element-wise, by name, in both directions;
/// if you add a variant, add it to BOTH and at the tail.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum HostFuncName {
    #[name = "sin"]
    Sin,
    #[name = "cos"]
    Cos,
    #[name = "tan"]
    Tan,
    #[name = "log"]
    Log,
    #[name = "triangle"]
    Triangle,
    #[name = "square"]
    Square,
    #[name = "saw"]
    Saw,
}

impl HostFuncName {
    pub fn to_u32(self) -> u32 {
        match self {
            HostFuncName::Sin => 0,
            HostFuncName::Cos => 1,
            HostFuncName::Tan => 2,
            HostFuncName::Log => 3,
            HostFuncName::Triangle => 4,
            HostFuncName::Square => 5,
            HostFuncName::Saw => 6,
        }
    }
    pub fn from_u32(v: u32) -> HostFuncName {
        match v {
            1 => HostFuncName::Cos,
            2 => HostFuncName::Tan,
            3 => HostFuncName::Log,
            4 => HostFuncName::Triangle,
            5 => HostFuncName::Square,
            6 => HostFuncName::Saw,
            _ => HostFuncName::Sin,
        }
    }
}

/// Auto-orbit camera path presets. `Off` leaves the camera fully manual.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum CamPath {
    #[name = "Off"]
    Off,
    #[name = "Horizontal Circle"]
    HCircle,
    #[name = "Vertical Circle"]
    VCircle,
    #[name = "Figure Eight"]
    Figure8,
    #[name = "Spiral"]
    Spiral,
    // --- #307 Tier 2: cinematic moves (append-only; ids are wire-stable) ---
    #[name = "Boom (Crane)"]
    Boom,
    #[name = "Pendulum"]
    Pendulum,
    #[name = "Truck (Lateral)"]
    Truck,
    #[name = "Push / Pull"]
    PushPull,
    #[name = "Over the Top"]
    PolarOver,
    #[name = "Handheld Drift"]
    Drift,
}

impl CamPath {
    pub fn to_u32(self) -> u32 {
        match self {
            CamPath::Off => 0,
            CamPath::HCircle => 1,
            CamPath::VCircle => 2,
            CamPath::Figure8 => 3,
            CamPath::Spiral => 4,
            CamPath::Boom => 5,
            CamPath::Pendulum => 6,
            CamPath::Truck => 7,
            CamPath::PushPull => 8,
            CamPath::PolarOver => 9,
            CamPath::Drift => 10,
        }
    }
    pub fn from_u32(v: u32) -> CamPath {
        match v {
            1 => CamPath::HCircle,
            2 => CamPath::VCircle,
            3 => CamPath::Figure8,
            4 => CamPath::Spiral,
            5 => CamPath::Boom,
            6 => CamPath::Pendulum,
            7 => CamPath::Truck,
            8 => CamPath::PushPull,
            9 => CamPath::PolarOver,
            10 => CamPath::Drift,
            _ => CamPath::Off,
        }
    }
}

/// Where the beat/bar clock gets its BPM (#307 Tier 1). `Host` = the PLL-locked
/// host transport (today's behaviour). `Audio` = a BPM estimated from the incoming
/// audio (a steady kick), so the visual keeps time with no host tempo at all;
/// when the estimate can't be trusted (a breakdown) the last good BPM is held.
/// `Manual` = the `tempo` dial, unconditionally.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum TempoSource {
    #[name = "Host (Transport)"]
    Host,
    #[name = "Audio (Detect BPM)"]
    Audio,
    #[name = "Manual (Dial)"]
    Manual,
}

impl TempoSource {
    pub fn to_u32(self) -> u32 {
        match self {
            TempoSource::Host => 0,
            TempoSource::Audio => 1,
            TempoSource::Manual => 2,
        }
    }
    pub fn from_u32(v: u32) -> TempoSource {
        match v {
            1 => TempoSource::Audio,
            2 => TempoSource::Manual,
            _ => TempoSource::Host,
        }
    }
}

/// How the camera shot sequencer picks the next move at each bar boundary
/// (#307 Tier 1). `Series` cycles the moves in order; `Random` picks one at
/// random (never the same twice in a row).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum CamOrder {
    #[name = "Series"]
    Series,
    #[name = "Random"]
    Random,
    // --- #307 Tier 2 ---
    #[name = "Shuffle"]
    Shuffle,
    #[name = "Weighted"]
    Weighted,
}

impl CamOrder {
    pub fn to_u32(self) -> u32 {
        match self {
            CamOrder::Series => 0,
            CamOrder::Random => 1,
            CamOrder::Shuffle => 2,
            CamOrder::Weighted => 3,
        }
    }
    pub fn from_u32(v: u32) -> CamOrder {
        match v {
            1 => CamOrder::Random,
            2 => CamOrder::Shuffle,
            3 => CamOrder::Weighted,
            _ => CamOrder::Series,
        }
    }
}

/// How the sequencer hands off between shots (#307 Tier 1). `Glide` eases the
/// camera state from the outgoing move to the incoming one over a short window
/// (a Steadicam feel); `Cut` snaps on the bar downbeat (an edit landing on the 1).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum CamTransition {
    #[name = "Glide"]
    Glide,
    #[name = "Cut"]
    Cut,
}

impl CamTransition {
    pub fn to_u32(self) -> u32 {
        match self {
            CamTransition::Glide => 0,
            CamTransition::Cut => 1,
        }
    }
    pub fn from_u32(v: u32) -> CamTransition {
        match v {
            1 => CamTransition::Cut,
            _ => CamTransition::Glide,
        }
    }
}

/// How many bars each sequencer shot holds before changing (#307 Tier 1).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum BarPeriod {
    #[name = "1 Bar"]
    B1,
    #[name = "2 Bars"]
    B2,
    #[name = "4 Bars"]
    B4,
    #[name = "8 Bars"]
    B8,
    #[name = "16 Bars"]
    B16,
    #[name = "32 Bars"]
    B32,
}

impl BarPeriod {
    /// The bar count as a value (what the visual's bar clock counts against).
    pub fn bars(self) -> f32 {
        match self {
            BarPeriod::B1 => 1.0,
            BarPeriod::B2 => 2.0,
            BarPeriod::B4 => 4.0,
            BarPeriod::B8 => 8.0,
            BarPeriod::B16 => 16.0,
            BarPeriod::B32 => 32.0,
        }
    }
    pub fn to_u32(self) -> u32 {
        match self {
            BarPeriod::B1 => 0,
            BarPeriod::B2 => 1,
            BarPeriod::B4 => 2,
            BarPeriod::B8 => 3,
            BarPeriod::B16 => 4,
            BarPeriod::B32 => 5,
        }
    }
    pub fn from_u32(v: u32) -> BarPeriod {
        match v {
            0 => BarPeriod::B1,
            1 => BarPeriod::B2,
            2 => BarPeriod::B4,
            4 => BarPeriod::B16,
            5 => BarPeriod::B32,
            _ => BarPeriod::B8, // 3 and fallback
        }
    }
}

/// Musical note-division for the Maxwell dipole's tempo-synced oscillation. The
/// field's E vectors swing out and back once per selected division (an LFO period),
/// phase-locked to the beat clock. Sub-beat divisions are fixed fractions of a beat;
/// Bar / 2-Bar scale with the session's beats-per-bar.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum OscDivision {
    #[name = "1/16"]
    Sixteenth,
    #[name = "1/8"]
    Eighth,
    #[name = "1/4"]
    Quarter,
    #[name = "1/2"]
    Half,
    #[name = "Bar"]
    Bar,
    #[name = "2 Bars"]
    TwoBar,
}

impl OscDivision {
    /// One full oscillation cycle's length in **beats**. Bar / 2-Bar scale with
    /// `beats_per_bar` (the session time signature); the rest are fixed.
    pub fn beats(self, beats_per_bar: f32) -> f32 {
        match self {
            OscDivision::Sixteenth => 0.25,
            OscDivision::Eighth => 0.5,
            OscDivision::Quarter => 1.0,
            OscDivision::Half => 2.0,
            OscDivision::Bar => beats_per_bar,
            OscDivision::TwoBar => 2.0 * beats_per_bar,
        }
    }
    pub fn to_u32(self) -> u32 {
        match self {
            OscDivision::Sixteenth => 0,
            OscDivision::Eighth => 1,
            OscDivision::Quarter => 2,
            OscDivision::Half => 3,
            OscDivision::Bar => 4,
            OscDivision::TwoBar => 5,
        }
    }
    pub fn from_u32(v: u32) -> OscDivision {
        match v {
            0 => OscDivision::Sixteenth,
            1 => OscDivision::Eighth,
            3 => OscDivision::Half,
            4 => OscDivision::Bar,
            5 => OscDivision::TwoBar,
            _ => OscDivision::Quarter, // 2 and fallback
        }
    }
}

/// FDTD Maxwell solver (#412 Tier 3) source waveform. **Pulse** = a one-shot
/// Gaussian wavelet (the "watch it launch and travel" transient); **CW** = a
/// continuous sinusoid at the source frequency (steady radiation / resonance).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum FdtdSource {
    #[name = "Pulse"]
    Pulse,
    #[name = "CW (continuous)"]
    Cw,
}

impl FdtdSource {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> FdtdSource {
        match v {
            1 => FdtdSource::Cw,
            _ => FdtdSource::Pulse,
        }
    }
}

/// #354 — how a preset recall snaps to the beat. `Instant` recalls immediately
/// (today's behaviour); the rest defer the recall to the next boundary of that
/// division. Two dropdowns use this: one for Scene recalls, one for the
/// individual Scene-component (Generator/Motion/Environment/Look) recalls.
/// Audio/Synth/Settings always recall instantly regardless.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum PresetDivision {
    #[name = "Instant"]
    Instant,
    #[name = "1/4"]
    Quarter,
    #[name = "1/2"]
    Half,
    #[name = "Bar"]
    Bar,
    #[name = "2 Bars"]
    TwoBar,
    #[name = "4 Bars"]
    FourBar,
    #[name = "8 Bars"]
    EightBar,
}
impl PresetDivision {
    /// The quantization step in **beats**; 0 = instant. Bar multiples scale with
    /// the session's beats-per-bar.
    pub fn beats(self, beats_per_bar: f32) -> f32 {
        match self {
            PresetDivision::Instant => 0.0,
            PresetDivision::Quarter => 1.0,
            PresetDivision::Half => 2.0,
            PresetDivision::Bar => beats_per_bar,
            PresetDivision::TwoBar => 2.0 * beats_per_bar,
            PresetDivision::FourBar => 4.0 * beats_per_bar,
            PresetDivision::EightBar => 8.0 * beats_per_bar,
        }
    }
}

/// Calibrated spectrum mode (#333 Tier 2): the frequency-axis resolution for the
/// analytical RTA — a fractional-octave band width (IEC 61260) or a raw linear-FFT
/// axis. `denom()` → the octave denominator (0 = linear FFT). **Append-only** —
/// `Linear` is last so the enum indices (host automation) stay stable.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum SpectrumMode {
    #[name = "1/1 octave"]
    Oct1,
    #[name = "1/3 octave"]
    Oct3,
    #[name = "1/6 octave"]
    Oct6,
    #[name = "1/12 octave"]
    Oct12,
    #[name = "Linear (FFT)"]
    Linear,
}
impl SpectrumMode {
    /// The octave denominator the spectrum integrator uses (0 = linear FFT axis).
    pub fn denom(self) -> u32 {
        match self {
            SpectrumMode::Oct1 => 1,
            SpectrumMode::Oct3 => 3,
            SpectrumMode::Oct6 => 6,
            SpectrumMode::Oct12 => 12,
            SpectrumMode::Linear => 0,
        }
    }
}

/// Frequency weighting for the calibrated spectrum (#333 Tier 2, IEC 61672).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MeterWeighting {
    #[name = "Z (flat)"]
    Z,
    #[name = "A"]
    A,
    #[name = "C"]
    C,
}
impl MeterWeighting {
    pub fn to_u32(self) -> u32 {
        match self {
            MeterWeighting::Z => 0,
            MeterWeighting::A => 1,
            MeterWeighting::C => 2,
        }
    }
}

/// RTA time-averaging (#333 Tier 2).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MeterAveraging {
    #[name = "Fast (125 ms)"]
    Fast,
    #[name = "Slow (1 s)"]
    Slow,
    #[name = "Peak hold"]
    PeakHold,
    #[name = "Leq (infinite)"]
    Leq,
}
impl MeterAveraging {
    pub fn to_u32(self) -> u32 {
        match self {
            MeterAveraging::Fast => 0,
            MeterAveraging::Slow => 1,
            MeterAveraging::PeakHold => 2,
            MeterAveraging::Leq => 3,
        }
    }
}

/// Duo-Field drive mode (#333 Tier 3 — the "Analyzer / Calibrated" instrument).
/// **Expressive** = today's arbitrary gain·RMS drive (default → byte-identical).
/// **Calibrated** = a stated, reproducible law of the *measured* loudness (LUFS,
/// gain-independent), so the same track produces the same field on any machine.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum AnalyticalMode {
    #[name = "Expressive (RMS)"]
    Expressive,
    #[name = "Calibrated (LUFS)"]
    Calibrated,
}
impl AnalyticalMode {
    pub fn to_u32(self) -> u32 {
        match self {
            AnalyticalMode::Expressive => 0,
            AnalyticalMode::Calibrated => 1,
        }
    }
    pub fn from_u32(v: u32) -> AnalyticalMode {
        match v {
            1 => AnalyticalMode::Calibrated,
            _ => AnalyticalMode::Expressive,
        }
    }
}

/// Field Volume density source (#348). Selects how `SurfaceMode::Volume` bakes its
/// density field. **Legacy** (default) is today's node point-set metaball bake →
/// byte-identical; the others render density instead of nodes (killing the
/// scraggle). Ordinals packed into `Shared.fieldvol[0]`; **append-only**.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum FieldVolSource {
    #[name = "Legacy (node metaball)"]
    Legacy,
    #[name = "Auto (field / smoothed)"]
    Auto,
    #[name = "Field-baked (energy)"]
    FieldBaked,
    #[name = "Smoothed node"]
    SmoothedNode,
}
impl FieldVolSource {
    pub fn to_u32(self) -> u32 {
        match self {
            FieldVolSource::Legacy => 0,
            FieldVolSource::Auto => 1,
            FieldVolSource::FieldBaked => 2,
            FieldVolSource::SmoothedNode => 3,
        }
    }
    pub fn from_u32(v: u32) -> FieldVolSource {
        match v {
            1 => FieldVolSource::Auto,
            2 => FieldVolSource::FieldBaked,
            3 => FieldVolSource::SmoothedNode,
            _ => FieldVolSource::Legacy,
        }
    }
}

/// Calibrated-colour mode (#349). **Aesthetic** (default) = today's tint
/// (HSV/palette/RGB-cube) → byte-identical. **Calibrated** = colour that means a
/// measured dB level, sampled from a legend-backed perceptual LUT. Packed into
/// `Shared.colour[0]`; **append-only**.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum ColourMode {
    #[name = "Aesthetic"]
    Aesthetic,
    #[name = "Calibrated (dB)"]
    Calibrated,
}
impl ColourMode {
    pub fn to_u32(self) -> u32 {
        match self {
            ColourMode::Aesthetic => 0,
            ColourMode::Calibrated => 1,
        }
    }
    pub fn from_u32(v: u32) -> ColourMode {
        match v {
            1 => ColourMode::Calibrated,
            _ => ColourMode::Aesthetic,
        }
    }
}

/// Calibrated-colour LUT (#349) — perceptually-uniform gradients where equal
/// colour steps = equal dB steps (unlike the HSV wheel). Packed into
/// `Shared.colour[3]`; **append-only**.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum CalLut {
    Turbo,
    Viridis,
    Inferno,
    Magma,
}
impl CalLut {
    pub fn to_u32(self) -> u32 {
        match self {
            CalLut::Turbo => 0,
            CalLut::Viridis => 1,
            CalLut::Inferno => 2,
            CalLut::Magma => 3,
        }
    }
    pub fn from_u32(v: u32) -> CalLut {
        match v {
            1 => CalLut::Viridis,
            2 => CalLut::Inferno,
            3 => CalLut::Magma,
            _ => CalLut::Turbo,
        }
    }
}

/// What "measured level" the calibrated colour reads (#349). **Auto** (default):
/// field generators (Maxwell/Acoustic) colour by their band's dBFS
/// (`audiospectrum`), every other generator by momentary LUFS (`audiometer[0]`).
/// Packed into `Shared.colour[4]`; **append-only**.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum CalColourSource {
    Auto,
    #[name = "Band (dBFS)"]
    Band,
    #[name = "Loudness (LUFS)"]
    Lufs,
}
impl CalColourSource {
    pub fn to_u32(self) -> u32 {
        match self {
            CalColourSource::Auto => 0,
            CalColourSource::Band => 1,
            CalColourSource::Lufs => 2,
        }
    }
    pub fn from_u32(v: u32) -> CalColourSource {
        match v {
            1 => CalColourSource::Band,
            2 => CalColourSource::Lufs,
            _ => CalColourSource::Auto,
        }
    }
}

/// Acoustic-source multipole order (#325). The signed monopoles that build the
/// field: monopole (a pulsating sphere), dipole (± pair, figure-8 lobe with an
/// equatorial pressure node), and the two quadrupoles. Ordinals are packed into
/// `Shared.acoustic[0]` and matched by `math::AcousticKind::from_u32`;
/// **append-only** so saved presets keep their source.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum AcousticSource {
    #[name = "Monopole"]
    Monopole,
    #[name = "Dipole"]
    Dipole,
    #[name = "Lateral quadrupole"]
    LateralQuad,
    #[name = "Longitudinal quadrupole"]
    LongitudinalQuad,
}

impl AcousticSource {
    pub fn to_u32(self) -> u32 {
        match self {
            AcousticSource::Monopole => 0,
            AcousticSource::Dipole => 1,
            AcousticSource::LateralQuad => 2,
            AcousticSource::LongitudinalQuad => 3,
        }
    }
    pub fn from_u32(v: u32) -> AcousticSource {
        match v {
            1 => AcousticSource::Dipole,
            2 => AcousticSource::LateralQuad,
            3 => AcousticSource::LongitudinalQuad,
            _ => AcousticSource::Monopole,
        }
    }
}

/// Density-Map Attractor kind (#380). The discrete iterated map the generator
/// iterates. Tier 1 ships the holomorphic seed map only; Tier 3 adds Clifford /
/// de Jong / Pickover / Gumowski–Mira / Hopalong. Wire value rides
/// `Shared.mapattractor[0]` and is matched by `math::MapKind::from_u32`;
/// **append-only** so saved presets keep their map.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MapKindParam {
    #[name = "Complexus (holomorphic)"]
    Complexus,
    #[name = "Clifford"]
    Clifford,
    #[name = "de Jong"]
    DeJong,
    #[name = "Pickover (fractal dream)"]
    Pickover,
    #[name = "Gumowski–Mira"]
    GumowskiMira,
    #[name = "Hopalong"]
    Hopalong,
}

impl MapKindParam {
    pub fn to_u32(self) -> u32 {
        match self {
            MapKindParam::Complexus => 0,
            MapKindParam::Clifford => 1,
            MapKindParam::DeJong => 2,
            MapKindParam::Pickover => 3,
            MapKindParam::GumowskiMira => 4,
            MapKindParam::Hopalong => 5,
        }
    }
    pub fn from_u32(v: u32) -> MapKindParam {
        match v {
            1 => MapKindParam::Clifford,
            2 => MapKindParam::DeJong,
            3 => MapKindParam::Pickover,
            4 => MapKindParam::GumowskiMira,
            5 => MapKindParam::Hopalong,
            _ => MapKindParam::Complexus,
        }
    }
}

/// Density-Map Attractor **colour-by-dynamics** mode (#380 Tier 3). How the per-splat
/// tint coordinate is derived. Wire value rides `Shared.mapattractor2[2]` and is matched
/// by `math::MapColor::from_u32`; **append-only**. `StepSpeed` (default) → byte-identical.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MapColorParam {
    #[name = "Step Speed |Δ|"]
    StepSpeed,
    #[name = "Iteration Index"]
    IterIndex,
    #[name = "Jacobian Stretch"]
    JacobianStretch,
}

impl MapColorParam {
    pub fn to_u32(self) -> u32 {
        match self {
            MapColorParam::StepSpeed => 0,
            MapColorParam::IterIndex => 1,
            MapColorParam::JacobianStretch => 2,
        }
    }
    pub fn from_u32(v: u32) -> MapColorParam {
        match v {
            1 => MapColorParam::IterIndex,
            2 => MapColorParam::JacobianStretch,
            _ => MapColorParam::StepSpeed,
        }
    }
}

/// Density-Map Attractor **parameter-orbit** mode (#380 Tier 2). How the map's
/// `(a, b)` are driven over time. Wire value rides `Shared.maporbit[0]` and is
/// matched by `math::MapOrbitMode::from_u32`; **append-only**.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MapOrbitModeParam {
    /// Static `(a, b)` — no animation.
    Off,
    /// The Tier-1 linear ramp (`a += a_drive·gen_phase`). Default → byte-identical
    /// to Tier 1 with the drives at 0.
    Linear,
    /// The closed, beat-locked Lissajous loop (the Tier-2 headline).
    Lissajous,
}

impl MapOrbitModeParam {
    pub fn to_u32(self) -> u32 {
        match self {
            MapOrbitModeParam::Off => 0,
            MapOrbitModeParam::Linear => 1,
            MapOrbitModeParam::Lissajous => 2,
        }
    }
    pub fn from_u32(v: u32) -> MapOrbitModeParam {
        match v {
            0 => MapOrbitModeParam::Off,
            2 => MapOrbitModeParam::Lissajous,
            _ => MapOrbitModeParam::Linear,
        }
    }
}

/// Field Engine **Tier 3** time-marched PDE preset (#381). Selects the RHS of
/// `∂u/∂t = L[u]` the `math::FieldSim` integrates live on a CPU grid (or `Off` =
/// the static Tier-1/2 analytic field). Wire value rides `Shared.fieldsim[0]` and
/// is matched by `math::PdePreset::from_u32`; **append-only**. `Off` (default)
/// keeps the Field Engine byte-identical to Tier 1/2.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum PdePreset {
    #[name = "Off (static field)"]
    Off,
    #[name = "Heat / diffusion"]
    Heat,
    #[name = "Wave"]
    Wave,
    #[name = "Schrodinger |psi|^2"]
    Schrodinger,
    #[name = "Gray-Scott (Turing)"]
    GrayScott,
    /// #407 Tier A: replay a pre-baked physics-field clip (from *The Well*) instead
    /// of integrating a PDE. Wire value 5.
    #[name = "Playback (Dataset)"]
    Playback,
    /// #407 Tier B: a tiny Neural Cellular Automaton rolled out live. Wire value 6.
    #[name = "Neural CA (Learned)"]
    NeuralCa,
}

impl PdePreset {
    /// Wire value packed into `Shared.fieldsim[0]`; matched by
    /// `math::PdePreset::from_u32`. Playback = 5 (Tier A), NeuralCa = 6 (Tier B).
    pub fn to_u32(self) -> u32 {
        match self {
            PdePreset::Off => 0,
            PdePreset::Heat => 1,
            PdePreset::Wave => 2,
            PdePreset::Schrodinger => 3,
            PdePreset::GrayScott => 4,
            PdePreset::Playback => 5,
            PdePreset::NeuralCa => 6,
        }
    }
    pub fn from_u32(v: u32) -> PdePreset {
        match v {
            1 => PdePreset::Heat,
            2 => PdePreset::Wave,
            3 => PdePreset::Schrodinger,
            4 => PdePreset::GrayScott,
            5 => PdePreset::Playback,
            6 => PdePreset::NeuralCa,
            _ => PdePreset::Off,
        }
    }
}

/// Duo-Field synthesis play mode (#339). **Generative** = self-contained (the
/// generator's own sources radiate, no MIDI needed); **Instrument** = each held
/// note is a radiating source you play; **Duet** = both at once (voices layer on
/// the generative bed). Wire index rides `Shared.sonify[1]`; **append-only**.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum SynthPlayMode {
    #[name = "Generative"]
    Generative,
    #[name = "Instrument"]
    Instrument,
    #[name = "Duet"]
    Duet,
}

impl SynthPlayMode {
    pub fn to_u32(self) -> u32 {
        match self {
            SynthPlayMode::Generative => 0,
            SynthPlayMode::Instrument => 1,
            SynthPlayMode::Duet => 2,
        }
    }
    pub fn from_u32(v: u32) -> SynthPlayMode {
        match v {
            1 => SynthPlayMode::Instrument,
            2 => SynthPlayMode::Duet,
            _ => SynthPlayMode::Generative,
        }
    }
}

/// Visual time-lens quantize (#339). **Free** = the continuous power law;
/// **Octave-locked** = each voice's visual rate snaps to an exact power of two
/// (octave dyads breathe in strict 2:1); **Beat-locked** = the rate snaps to the
/// musical-division grid (reserved — treated as Free until the OscDivision snap
/// lands). Wire index rides `Shared.sonify[7]`; **append-only**.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum SynthQuantize {
    #[name = "Free"]
    Free,
    #[name = "Octave-locked"]
    Octave,
    #[name = "Beat-locked"]
    Beat,
}

impl SynthQuantize {
    pub fn to_u32(self) -> u32 {
        match self {
            SynthQuantize::Free => 0,
            SynthQuantize::Octave => 1,
            SynthQuantize::Beat => 2,
        }
    }
    pub fn from_u32(v: u32) -> SynthQuantize {
        match v {
            1 => SynthQuantize::Octave,
            2 => SynthQuantize::Beat,
            _ => SynthQuantize::Free,
        }
    }
}

/// Duo-Field synthesis engine mode (#339). **Probes** = Tier 1 field microphones;
/// **Lattice** = Tier 2 oscillator bank (an additive cluster anchored to the field
/// shell). Append-only; wire index read directly by the DSP.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum SynthMode {
    #[name = "Probes (field mics)"]
    Probes,
    #[name = "Lattice (oscillator bank)"]
    Lattice,
    #[name = "Modal (struck cavities)"]
    Modal,
    #[name = "Granular (field aura)"]
    Granular,
    #[name = "Wavetable (scanned shape)"]
    Wavetable,
}

impl SynthMode {
    pub fn to_u32(self) -> u32 {
        match self {
            SynthMode::Probes => 0,
            SynthMode::Lattice => 1,
            SynthMode::Modal => 2,
            SynthMode::Granular => 3,
            SynthMode::Wavetable => 4,
        }
    }
    pub fn from_u32(v: u32) -> SynthMode {
        match v {
            1 => SynthMode::Lattice,
            2 => SynthMode::Modal,
            3 => SynthMode::Granular,
            4 => SynthMode::Wavetable,
            _ => SynthMode::Probes,
        }
    }
}

/// Tier 2 oscillator-lattice tuning layout (#339, `math::lattice_freq`).
/// Append-only; wire index read directly by the DSP.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum TuningLayout {
    #[name = "Octaves"]
    Octaves,
    #[name = "Harmonic series"]
    Harmonic,
    #[name = "Stretched (inharmonic)"]
    Stretched,
    #[name = "Geometric"]
    Geometric,
}

impl TuningLayout {
    pub fn to_u32(self) -> u32 {
        match self {
            TuningLayout::Octaves => 0,
            TuningLayout::Harmonic => 1,
            TuningLayout::Stretched => 2,
            TuningLayout::Geometric => 3,
        }
    }
    pub fn from_u32(v: u32) -> TuningLayout {
        match v {
            1 => TuningLayout::Harmonic,
            2 => TuningLayout::Stretched,
            3 => TuningLayout::Geometric,
            _ => TuningLayout::Octaves,
        }
    }
}

/// Acoustic source MODEL (#325 Tier 4): a free-radiating multipole, or a bounded
/// rectangular **cavity** standing-wave eigenmode (Chladni nodal patterns).
/// Ordinal is packed into `Shared.acoustic2[0]`. Append-only.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum AcousticModel {
    #[name = "Radiating"]
    Radiating,
    #[name = "Cavity (standing wave)"]
    Cavity,
}

impl AcousticModel {
    pub fn to_u32(self) -> u32 {
        match self {
            AcousticModel::Radiating => 0,
            AcousticModel::Cavity => 1,
        }
    }
    pub fn from_u32(v: u32) -> AcousticModel {
        match v {
            1 => AcousticModel::Cavity,
            _ => AcousticModel::Radiating,
        }
    }
}

/// The waveform the decoupled dolly (in/out breath) traces over its period
/// (#307 Tier 1). All are mean-centred so the dolly swings symmetrically around
/// the current radius.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum DollyWave {
    #[name = "Sine"]
    Sine,
    #[name = "Triangle"]
    Triangle,
    #[name = "Ease (Rest)"]
    Ease,
}

impl DollyWave {
    pub fn to_u32(self) -> u32 {
        match self {
            DollyWave::Sine => 0,
            DollyWave::Triangle => 1,
            DollyWave::Ease => 2,
        }
    }
    pub fn from_u32(v: u32) -> DollyWave {
        match v {
            1 => DollyWave::Triangle,
            2 => DollyWave::Ease,
            _ => DollyWave::Sine,
        }
    }
}

/// Destination for a pulse→param modulation slot. The visual scales the beat
/// envelope by a per-target "span" so one bipolar depth slider gives a musical
/// pump regardless of the target's native units.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum ModTarget {
    #[name = "None"]
    None,
    #[name = "Scale Amp"]
    ScaleAmp,
    #[name = "Glow"]
    Glow,
    #[name = "Rotation Amp X"]
    RotAmpX,
    #[name = "Rotation Amp Y"]
    RotAmpY,
    #[name = "Rotation Amp Z"]
    RotAmpZ,
    #[name = "Rotation Speed X"]
    RotModX,
    #[name = "Rotation Speed Y"]
    RotModY,
    #[name = "Rotation Speed Z"]
    RotModZ,
    #[name = "Translation Mod X"]
    TransModX,
    #[name = "Translation Mod Y"]
    TransModY,
    #[name = "Translation Mod Z"]
    TransModZ,
    #[name = "Exposure"]
    Exposure,
    #[name = "Bloom"]
    Bloom,
    // Appended (discriminant 14) so existing presets' Exposure/Bloom values
    // (12/13) stay valid — don't insert mid-enum. Pumps all three trans_mod axes.
    #[name = "Translation Mod All"]
    TransModAll,
    // Appended (discriminant 15, #187): pumps the rails generator's forward
    // speed (world units per beat) — the Z0NE "collect gems, go faster" loop
    // driven by the beat/audio envelope. Inert outside Rails mode.
    #[name = "Rail Speed"]
    RailSpeed,
}

impl ModTarget {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> ModTarget {
        match v {
            1 => ModTarget::ScaleAmp,
            2 => ModTarget::Glow,
            3 => ModTarget::RotAmpX,
            4 => ModTarget::RotAmpY,
            5 => ModTarget::RotAmpZ,
            6 => ModTarget::RotModX,
            7 => ModTarget::RotModY,
            8 => ModTarget::RotModZ,
            9 => ModTarget::TransModX,
            10 => ModTarget::TransModY,
            11 => ModTarget::TransModZ,
            12 => ModTarget::Exposure,
            13 => ModTarget::Bloom,
            14 => ModTarget::TransModAll,
            15 => ModTarget::RailSpeed,
            _ => ModTarget::None,
        }
    }
}

/// Liquid material (#182 T4 follow-up): `UseScene` = follow the scene's
/// Material selector (the pre-existing behaviour); the rest give the liquid
/// its OWN material — it's a different substance in the scene. The shared
/// fine dials (chrome purity, glass clarity, dispersion, thin-film…) apply
/// on top of whichever type is active.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum LiqMaterial {
    #[name = "Use Scene Material"]
    UseScene,
    #[name = "Standard"]
    Standard,
    #[name = "Chrome"]
    Chrome,
    #[name = "Glass"]
    Glass,
}

impl LiqMaterial {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> LiqMaterial {
        match v {
            1 => LiqMaterial::Standard,
            2 => LiqMaterial::Chrome,
            3 => LiqMaterial::Glass,
            _ => LiqMaterial::UseScene,
        }
    }
}

/// Dendritic-arbor morphology class (#260 Tier 2): the shape a soma's grown tree
/// takes. `Pyramidal` = an apical trunk + a basal skirt (cortical pyramidal cell);
/// `Stellate` = a bushy, radially-symmetric arbor (granule/stellate interneuron);
/// `Degree` = per-node — high-degree hubs grow pyramidal, low-degree nodes stellate.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum NeuronType {
    #[name = "Pyramidal"]
    Pyramidal,
    #[name = "Stellate"]
    Stellate,
    #[name = "By Degree"]
    Degree,
}

impl NeuronType {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> NeuronType {
        match v {
            1 => NeuronType::Stellate,
            2 => NeuronType::Degree,
            _ => NeuronType::Pyramidal,
        }
    }
}

/// Liquid render mode (#182 T3b): `Isosurface` = the in-scene metaball draw
/// (route A, the default). `Refractive` = the post-scene see-through pass —
/// Snell refraction of the RESOLVED scene at the live IOR, real body
/// thickness, Beer–Lambert absorption, energy-conserving Fresnel split.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum LiqRender {
    #[name = "Isosurface"]
    Isosurface,
    #[name = "Refractive (see-through)"]
    Refractive,
}

impl LiqRender {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> LiqRender {
        if v == 1 { LiqRender::Refractive } else { LiqRender::Isosurface }
    }
}

/// Hardware-RT debug view (#195 Tier 0): a fullscreen ray-query visualization
/// drawn over the final frame so the TLAS can be verified against the raster
/// scene on the Mac. Per-display (not preset-captured), like HDR/MSAA.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum RtDebugView {
    #[name = "Off"]
    Off,
    #[name = "Hit Normals"]
    Normals,
    #[name = "Instance Index"]
    Instance,
    #[name = "Hit Distance"]
    Distance,
}

impl RtDebugView {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> RtDebugView {
        match v {
            1 => RtDebugView::Normals,
            2 => RtDebugView::Instance,
            3 => RtDebugView::Distance,
            _ => RtDebugView::Off,
        }
    }
}

/// Path-tracer composite mode — how the traced result reaches the frame.
/// `Replace` overwrites the HDR scene with the trace (the original ground-truth
/// mode: you lose the raster environment + PBR facilities). `Blend` keeps the
/// raster PBR image and cross-blends the trace over it by `pt_augment` (environment
/// + full PBR stay visible, the trace layers on — quick augmentation). `GiAdd` has
/// the tracer contribute INDIRECT lighting only (no double-counted direct/emissive
/// at the primary hit), added onto the raster — physically-clean augmentation.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum PtComposite {
    #[name = "Replace"]
    Replace,
    #[name = "Blend (augment)"]
    Blend,
    #[name = "GI add (indirect)"]
    GiAdd,
}

impl PtComposite {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> PtComposite {
        match v {
            1 => PtComposite::Blend,
            2 => PtComposite::GiAdd,
            _ => PtComposite::Replace,
        }
    }
}

/// Ambient-occlusion source (#195 Tier 3): `Gtao` = the screen-space horizon
/// integration (today's default). `RayTraced` = short hemisphere rays against
/// the TLAS — ground-truth short-range occlusion (no screen-space haloing,
/// off-screen geometry occludes); falls back to GTAO on machines without
/// ray-query support. Both feed the same blur/composite path.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum AoSource {
    #[name = "GTAO (screen-space)"]
    Gtao,
    #[name = "Ray Traced (RT)"]
    RayTraced,
}

impl AoSource {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> AoSource {
        if v == 1 { AoSource::RayTraced } else { AoSource::Gtao }
    }
}

/// Liquid tank container shape (#182 T3a): `Box` = the plain axis-aligned tank.
/// `Sphere` = a free-slip spherical shell (gravity pools into a curved bowl —
/// no flat floor). `Cylinder` = round wall, flat floor. `Boundless` = NO hard
/// wall: a soft absorbing shell fades outward motion over the outer half of
/// the radius and gently pulls strays back — the liquid trails off into space
/// with no defined edges (pair with the reveal for a fully borderless look).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum LiqShape {
    #[name = "Box"]
    Box,
    #[name = "Sphere"]
    Sphere,
    #[name = "Cylinder"]
    Cylinder,
    #[name = "Boundless (soft)"]
    Boundless,
}

impl LiqShape {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> LiqShape {
        match v {
            1 => LiqShape::Sphere,
            2 => LiqShape::Cylinder,
            3 => LiqShape::Boundless,
            _ => LiqShape::Box,
        }
    }
}

/// How each node is turned into geometry. `Original` = an independent cube per
/// node (rotated + scaled in its own frame). `FlowAligned` = each cube is
/// oriented toward its successor and stretched to bridge the gap, so consecutive
/// nodes connect into ribbons/tubes instead of crossing as independent spikes.
/// `SweptTubes` = the same per-segment bridges rendered as round cylinders
/// instead of boxes, for a smoother tube look.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum SurfaceMode {
    #[name = "Original"]
    Original,
    #[name = "Flow-Aligned"]
    FlowAligned,
    #[name = "Swept Tubes"]
    SweptTubes,
    // Metaball: the node set is baked into a 3D scalar field and raymarched as ONE
    // smooth contiguous skin (blobs fuse where nodes are near). Colour blends the
    // per-node tints — RGB-cube by position in Native, the palette/HSV sweep
    // otherwise. Uses its own params (radius/threshold/smoothness), not the
    // box/cylinder geometry.
    #[name = "Metaball"]
    Metaball,
    // Membrane: loft a continuous sheet between adjacent strands (the negative
    // space between tentacles becomes the surface) — a sail / jellyfish bell / web.
    // Ordinary rasterized triangles through the full PBR/material stack.
    #[name = "Membrane"]
    Membrane,
    // Voxel: the Eulerian render path. Splat the node set into a fixed 3D lattice
    // and DDA-raymarch crisp grid-snapped cubes (flat face shading, voxel AO, soft
    // shadows, palette posterize) — a Teardown/MagicaVoxel look, deliberately
    // distinct from the smooth PBR cubes. Works across every generator.
    #[name = "Voxel"]
    Voxel,
    // Volume (#152): bake the node set into the SAME 3D field as Metaball, but
    // raymarch it as a glowing participating medium (emission · density with
    // Beer–Lambert extinction) instead of an isosurface — a nebula/fog look.
    // Uses its own params (radius/density/emission/absorption/steps).
    #[name = "Volume"]
    Volume,
    // Neural Tissue (#260 Tier 1): a "living neural tissue" surface built from
    // CLOSED anatomical primitives — soma cell bodies (icospheres, non-uniformly
    // scaled per node), capped capsule edges (rounded-cap cylinders — no open pipe
    // ends), and synaptic boutons (bulbs at edge terminals). Recommended for the
    // Neural Network generator (multi-mesh soma/capsule/bouton draw); benign
    // elsewhere (other generators render their bridges as closed capsules). A waxy
    // translucent membrane via the shared Surface-FX SSS/iridescence path.
    #[name = "Neural Tissue"]
    NeuralTissue,
    // Splat: render the node set as a cloud of anisotropic 3-D Gaussians — the
    // 3D-Gaussian-Splatting *primitive* used for forward synthesis (no photogrammetry
    // / no optimization). Each node's model matrix becomes a splat (translation → μ,
    // the 3×3 rot·scale → covariance basis, tint → colour). Tier 1 = additive unlit
    // Gaussians; Tier 2 = sorted-alpha, IBL-lit 2DGS oriented disks (the disk normal
    // rides the split-sum IBL + key/fill). A soft, volumetric look between the hard
    // PBR cubes and the raymarched Volume nebula. Reuses the instance/tint buffers —
    // no new geometry payload.
    #[name = "Splat"]
    Splat,
    // Plexus: treat the generator's node cloud as a point set and wire each node to
    // its nearest neighbours with thin struts + a marker per node — a breathing
    // "field web" / plexus. Generator-agnostic (post-processes whatever node cloud
    // was emitted); raymarch generators emit no nodes, so it's a no-op there. Takes
    // ordinal 9 (Splat took 8 on the main merge).
    #[name = "Plexus"]
    Plexus,
}

impl SurfaceMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> SurfaceMode {
        match v {
            1 => SurfaceMode::FlowAligned,
            2 => SurfaceMode::SweptTubes,
            3 => SurfaceMode::Metaball,
            4 => SurfaceMode::Membrane,
            5 => SurfaceMode::Voxel,
            6 => SurfaceMode::Volume,
            7 => SurfaceMode::NeuralTissue,
            8 => SurfaceMode::Splat,
            9 => SurfaceMode::Plexus,
            _ => SurfaceMode::Original,
        }
    }
}

/// Gaussian Splat rendering tier (`SurfaceMode::Splat`). `Additive` (Tier 1) draws
/// each splat as an unlit, additive anisotropic Gaussian that blooms through the HDR
/// post chain — no depth sort, no lighting, the most robust "made of light" look.
/// `Lit` (Tier 2) treats each splat as a 2DGS oriented disk: sorted back-to-front and
/// alpha-composited, shaded by the split-sum IBL + key/fill (the disk normal is the
/// splat's thin axis), so the cloud sits in the same lit world as the PBR geometry.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum SplatMode {
    #[name = "Additive (unlit)"]
    Additive,
    #[name = "Lit (2DGS)"]
    Lit,
}

impl SplatMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> SplatMode {
        match v {
            1 => SplatMode::Lit,
            _ => SplatMode::Additive,
        }
    }
}

/// Where the Original cube-field's loop grid sits relative to the world origin —
/// i.e. what point each arm / sheet pivots off of. Only the `Original` generator's
/// cube-field (`draw_tissue` / `build_swept_tubes` / `draw_membrane`) reads this;
/// the strand generators produce their own geometry and ignore it.
///
/// - **`Corner`** (default, the historical look): the loop index *is* the base
///   position, so the grid runs `0..count` on each axis — its corner node sits at
///   the origin and every rotating arm/sheet pivots off that corner. When the
///   sheets wrap around it the whole shape reads as centred on the origin.
/// - **`Centered`**: each axis's index is re-centred to `idx − (count−1)/2` before
///   it feeds rotation, translation-base and scale-growth, so the *middle* node is
///   the un-rotated pivot at the origin and every arm/sheet fans out symmetrically
///   in both directions — the field is point-symmetric about the origin.
///
/// `Corner` reproduces the pre-existing geometry byte-for-byte (offset 0 ⇒ the
/// centred index equals the raw loop index everywhere).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum OriginMode {
    #[name = "Corner (origin)"]
    Corner,
    #[name = "Centered"]
    Centered,
}

impl OriginMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> OriginMode {
        match v {
            1 => OriginMode::Centered,
            _ => OriginMode::Corner,
        }
    }
}

/// Material-map **projection** (#472 Tier 1): how a 2-D PBR texture set lands on a
/// surface. Most Organon geometry has **no UVs** (the mesh vertex is pos/normal/
/// colour), so the default is `Triplanar` — the map is evaluated in world space on
/// all three axis planes and blended by the surface normal (exactly how the
/// reaction–diffusion skin already samples), needing no unwrap. The planar modes
/// project along a single axis pair for a flat, deliberate tiling look.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MatProjection {
    #[name = "Triplanar (world)"]
    Triplanar,
    #[name = "Planar (world XZ)"]
    WorldPlanar,
    #[name = "Planar (object XY)"]
    ObjectPlanar,
}

impl MatProjection {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> MatProjection {
        match v {
            1 => MatProjection::WorldPlanar,
            2 => MatProjection::ObjectPlanar,
            _ => MatProjection::Triplanar,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            MatProjection::Triplanar => "triplanar",
            MatProjection::WorldPlanar => "world_planar",
            MatProjection::ObjectPlanar => "object_planar",
        }
    }
    pub fn from_str_or(s: &str, default: MatProjection) -> MatProjection {
        match s.trim().to_ascii_lowercase().as_str() {
            "triplanar" => MatProjection::Triplanar,
            "world_planar" | "world" | "planar" => MatProjection::WorldPlanar,
            "object_planar" | "object" => MatProjection::ObjectPlanar,
            _ => default,
        }
    }
}

/// Procedural noise/pattern generator (#472 Tier 2) — the curated ~16-entry
/// library, evaluated by the compute baker (`material_bake.wgsl`) into a channel
/// texture. Value/Perlin/Simplex are the base gradient noises; FBM/Turbulence/
/// Ridged are fractal octave stacks over Perlin; Worley/Cells/Gabor are
/// cellular/sparse; Curl/DomainWarp warp the domain; Checker/Stripes/Hex/Brick/
/// Veins are structured patterns. All return a 0..1 field the baker remaps/tints.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MatNoise {
    #[name = "Value"] Value,
    #[name = "Perlin"] Perlin,
    #[name = "Simplex"] Simplex,
    #[name = "FBM (fractal)"] Fbm,
    #[name = "Turbulence"] Turbulence,
    #[name = "Ridged"] Ridged,
    #[name = "Worley (F1)"] Worley,
    #[name = "Cells (F2−F1)"] Cells,
    #[name = "Gabor"] Gabor,
    #[name = "Curl"] Curl,
    #[name = "Domain Warp"] DomainWarp,
    #[name = "Checker"] Checker,
    #[name = "Stripes"] Stripes,
    #[name = "Hex"] Hex,
    #[name = "Brick"] Brick,
    #[name = "Veins"] Veins,
}

impl MatNoise {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> MatNoise {
        use MatNoise::*;
        match v {
            1 => Perlin, 2 => Simplex, 3 => Fbm, 4 => Turbulence, 5 => Ridged,
            6 => Worley, 7 => Cells, 8 => Gabor, 9 => Curl, 10 => DomainWarp,
            11 => Checker, 12 => Stripes, 13 => Hex, 14 => Brick, 15 => Veins,
            _ => Value,
        }
    }
    /// Stable snake_case name for the `material.json` graph (#472 Tier 4).
    pub fn as_str(self) -> &'static str {
        use MatNoise::*;
        match self {
            Value => "value", Perlin => "perlin", Simplex => "simplex", Fbm => "fbm",
            Turbulence => "turbulence", Ridged => "ridged", Worley => "worley",
            Cells => "cells", Gabor => "gabor", Curl => "curl", DomainWarp => "domain_warp",
            Checker => "checker", Stripes => "stripes", Hex => "hex", Brick => "brick",
            Veins => "veins",
        }
    }
    pub fn from_str_or(s: &str, default: MatNoise) -> MatNoise {
        use MatNoise::*;
        match s.trim().to_ascii_lowercase().as_str() {
            "value" => Value, "perlin" => Perlin, "simplex" => Simplex, "fbm" => Fbm,
            "turbulence" => Turbulence, "ridged" => Ridged, "worley" => Worley,
            "cells" => Cells, "gabor" => Gabor, "curl" => Curl,
            "domain_warp" | "domainwarp" | "warp" => DomainWarp, "checker" => Checker,
            "stripes" => Stripes, "hex" => Hex, "brick" => Brick, "veins" => Veins,
            _ => default,
        }
    }
}

/// Which PBR channel a procedural layer bakes into (#472 Tier 2). Albedo maps the
/// noise scalar through the two-stop gradient; the rest write it as a scalar. Height
/// is baked for Tier-3 derived normal/AO + Tier-5 displacement (not shaded in T1).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MatChannel {
    #[name = "Albedo"] Albedo,
    #[name = "Roughness"] Roughness,
    #[name = "Metallic"] Metallic,
    #[name = "Height"] Height,
    #[name = "AO"] Ao,
    #[name = "Emissive"] Emissive,
}

impl MatChannel {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> MatChannel {
        use MatChannel::*;
        match v {
            1 => Roughness, 2 => Metallic, 3 => Height, 4 => Ao, 5 => Emissive,
            _ => Albedo,
        }
    }
    pub fn as_str(self) -> &'static str {
        use MatChannel::*;
        match self {
            Albedo => "albedo", Roughness => "roughness", Metallic => "metallic",
            Height => "height", Ao => "ao", Emissive => "emissive",
        }
    }
    pub fn from_str_or(s: &str, default: MatChannel) -> MatChannel {
        use MatChannel::*;
        match s.trim().to_ascii_lowercase().as_str() {
            "albedo" | "color" | "colour" => Albedo, "roughness" | "rough" => Roughness,
            "metallic" | "metal" => Metallic, "height" | "displacement" => Height,
            "ao" | "occlusion" => Ao, "emissive" | "emission" => Emissive,
            _ => default,
        }
    }
}

/// Procedural bake resolution (#472 Tier 2) — the size of the baked channel texture.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum BakeRes {
    #[name = "256²"] R256,
    #[name = "512²"] R512,
    #[name = "1024²"] R1024,
}

impl BakeRes {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> BakeRes {
        match v {
            0 => BakeRes::R256,
            2 => BakeRes::R1024,
            _ => BakeRes::R512,
        }
    }
    /// Texture edge length in pixels.
    pub fn px(self) -> f32 {
        match self {
            BakeRes::R256 => 256.0,
            BakeRes::R512 => 512.0,
            BakeRes::R1024 => 1024.0,
        }
    }
}

/// How an overlay layer composites onto the accumulated channel value below it
/// (#472 Tier 3). `Height` blends by relative height (a soft max — the taller layer
/// wins, good for stacking rock/tile over a base). All are per-texel scalar ops the
/// baker applies in `material_bake.wgsl`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum BlendMode {
    #[name = "Normal"] Normal,
    #[name = "Add"] Add,
    #[name = "Multiply"] Multiply,
    #[name = "Overlay"] Overlay,
    #[name = "Screen"] Screen,
    #[name = "Min"] Min,
    #[name = "Max"] Max,
    #[name = "Height"] Height,
}

impl BlendMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> BlendMode {
        use BlendMode::*;
        match v {
            1 => Add, 2 => Multiply, 3 => Overlay, 4 => Screen, 5 => Min, 6 => Max,
            7 => Height, _ => Normal,
        }
    }
    pub fn as_str(self) -> &'static str {
        use BlendMode::*;
        match self {
            Normal => "normal", Add => "add", Multiply => "multiply", Overlay => "overlay",
            Screen => "screen", Min => "min", Max => "max", Height => "height",
        }
    }
    pub fn from_str_or(s: &str, default: BlendMode) -> BlendMode {
        use BlendMode::*;
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" | "replace" => Normal, "add" => Add, "multiply" | "mul" => Multiply,
            "overlay" => Overlay, "screen" => Screen, "min" => Min, "max" => Max,
            "height" | "height_blend" => Height, _ => default,
        }
    }
}

/// How a procedural material animates over time (#472 Tier 5) — the visual injects a
/// time term into the baked layers before re-baking (throttled). `Drift` pans the
/// noise along the flow direction; `Evolve` circles it so the pattern churns;
/// `Rotate` spins the field.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum AnimMode {
    #[name = "Drift"] Drift,
    #[name = "Evolve"] Evolve,
    #[name = "Rotate"] Rotate,
}

impl AnimMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> AnimMode {
        match v {
            1 => AnimMode::Evolve,
            2 => AnimMode::Rotate,
            _ => AnimMode::Drift,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            AnimMode::Drift => "drift",
            AnimMode::Evolve => "evolve",
            AnimMode::Rotate => "rotate",
        }
    }
    pub fn from_str_or(s: &str, default: AnimMode) -> AnimMode {
        match s.trim().to_ascii_lowercase().as_str() {
            "drift" => AnimMode::Drift,
            "evolve" => AnimMode::Evolve,
            "rotate" => AnimMode::Rotate,
            _ => default,
        }
    }
}

/// KIFS space / geometry — the surface (and its symmetry) the field lives on,
/// upstream of the fractal engine, palette, tunnel, invert and dispersion (which
/// all compose on top). Group 1: Euclidean (the flat IFS), Hyperbolic (Poincaré
/// disk → Escher Circle-Limit), and Quasicrystal (aperiodic, no translational
/// symmetry). The id (0..2) packs into `Shared.kifs[23]` and branches in
/// `kifs.wgsl::compute_glow`. (Future groups: Weyl/root-system & E8, gauge-field
/// defects, conformal domain maps.)
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum KifsSpace {
    #[name = "Euclidean"]
    Euclidean,
    #[name = "Hyperbolic"]
    Hyperbolic,
    #[name = "Quasicrystal"]
    Quasicrystal,
    #[name = "Weyl tiling"]
    Weyl,
    #[name = "E8 roots"]
    E8,
    #[name = "U(1) vortices"]
    Vortex,
    #[name = "Apollonian"]
    Apollonian,
    #[name = "Modular"]
    Modular,
    #[name = "Elliptic ℘"]
    Elliptic,
}

impl KifsSpace {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> KifsSpace {
        match v {
            1 => KifsSpace::Hyperbolic,
            2 => KifsSpace::Quasicrystal,
            3 => KifsSpace::Weyl,
            4 => KifsSpace::E8,
            5 => KifsSpace::Vortex,
            6 => KifsSpace::Apollonian,
            7 => KifsSpace::Modular,
            8 => KifsSpace::Elliptic,
            _ => KifsSpace::Euclidean,
        }
    }
}

/// KIFS render dimensionality — how the chosen space is drawn. **Field** is the
/// original per-pixel 2-D field (flat or `kf_tunnel` bore). **Relief** raymarches
/// that same field's brightness as a lit heightfield (idea 1 — works in every
/// space). **Conformal** is the honest-3-D lift of the conformal cluster
/// (Euclidean / Hyperbolic / Modular / Apollonian): circle-inversion becomes
/// **sphere-inversion** (ℂ→ℝ³), raymarched as a distance-estimated Apollonian/
/// Kleinian solid. **Filaments** is the honest-3-D lift of the field cluster
/// (U(1) vortices, Elliptic ℘): both fields carry phase defects that in 3-D are
/// LINES — a U(1) vortex is a filament, and ℘'s doubly-periodic poles extrude
/// into a lattice of phase lines — so this volumetrically raymarches glowing,
/// phase-coloured filaments (a superfluid tangle / a luminous lattice of threads).
/// **Lattice** is the honest-3-D lift of the lattice cluster (Quasicrystal / Weyl /
/// E8): a 3-D icosahedral quasicrystal, a rank-3 reflection-group honeycomb, and the
/// 3-D shadow of the 240 E8 roots — volumetrically raymarched. The id packs into
/// `Shared.kifs[29]` and branches in `kifs.wgsl::fs_kifs`. (Future: a fly-through.)
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum KifsView {
    #[name = "Field (2-D)"]
    Field,
    #[name = "Relief (3-D)"]
    Relief,
    #[name = "Conformal (3-D)"]
    Conformal,
    #[name = "Filaments (3-D)"]
    Filaments,
    #[name = "Lattice (3-D)"]
    Lattice,
}

impl KifsView {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> KifsView {
        match v {
            1 => KifsView::Relief,
            2 => KifsView::Conformal,
            3 => KifsView::Filaments,
            4 => KifsView::Lattice,
            _ => KifsView::Field,
        }
    }
}

/// KIFS fractal engine — the core fold the 2-D field iterates. Each is a
/// fundamentally different self-similar structure; the kaleidoscope symmetry,
/// motifs, rings, rays, palette and projection are shared on top. The id (0..4)
/// is packed into `Shared.kifs[12]` and branched in `kifs.wgsl::fractal_glow`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum KifsPattern {
    #[name = "Inversion"]
    Inversion,
    #[name = "Mandelbox"]
    Mandelbox,
    #[name = "Sierpinski"]
    Sierpinski,
    #[name = "Log-spiral"]
    LogSpiral,
    #[name = "Kleinian"]
    Kleinian,
}

impl KifsPattern {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> KifsPattern {
        match v {
            1 => KifsPattern::Mandelbox,
            2 => KifsPattern::Sierpinski,
            3 => KifsPattern::LogSpiral,
            4 => KifsPattern::Kleinian,
            _ => KifsPattern::Inversion,
        }
    }
}

/// KIFS colour scheme — a bank of IQ cosine palettes. `Spectral` reproduces the
/// original teal↔magenta look. The id (0..6) packs into `Shared.kifs[13]` and
/// selects coefficients in `kifs.wgsl::palette`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum KifsPalette {
    #[name = "Spectral"]
    Spectral,
    #[name = "Ember"]
    Ember,
    #[name = "Ice"]
    Ice,
    #[name = "Toxic"]
    Toxic,
    #[name = "Neon"]
    Neon,
    #[name = "Gold"]
    Gold,
    #[name = "Rainbow"]
    Rainbow,
}

impl KifsPalette {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> KifsPalette {
        match v {
            1 => KifsPalette::Ember,
            2 => KifsPalette::Ice,
            3 => KifsPalette::Toxic,
            4 => KifsPalette::Neon,
            5 => KifsPalette::Gold,
            6 => KifsPalette::Rainbow,
            _ => KifsPalette::Spectral,
        }
    }
}

/// Scene Kaleidoscope (#361 Tier 1) mirror mode — how each pie-slice samples the
/// live HDR scene. Packs into `Shared.kaleido[2]` and branches in `kaleido.wgsl`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum KaleidoMode {
    /// Each slice shows the WHOLE frame squished + mirror-tiled, so adjacent slices
    /// reflect different real geometry (swimmy).
    #[name = "Full frame"]
    FullFrame,
    /// The classic optical kaleidoscope: every slice samples the SAME thin source
    /// wedge, so all sectors are identical mirror images.
    #[name = "Wedge (classic)"]
    Wedge,
}

impl KaleidoMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> KaleidoMode {
        match v {
            1 => KaleidoMode::Wedge,
            _ => KaleidoMode::FullFrame,
        }
    }
}

/// Orientation of the #391 Tier 1 Poynting-flux measurement patch — the surface the
/// instrument integrates energy flux through. Packs into `Shared.instrument[13]`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum FluxAxis {
    /// Patch faces +X.
    #[name = "X"]
    X,
    /// Patch faces +Y.
    #[name = "Y"]
    Y,
    /// Patch faces +Z.
    #[name = "Z"]
    Z,
    /// Patch faces radially outward from the world origin (through its centre) — the
    /// natural choice for reading a radiating source's outgoing power.
    #[name = "Radial"]
    Radial,
}

impl FluxAxis {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> FluxAxis {
        match v {
            1 => FluxAxis::Y,
            2 => FluxAxis::Z,
            3 => FluxAxis::Radial,
            _ => FluxAxis::X,
        }
    }
    /// The patch normal for a patch centred at `center`. Radial faces away from the
    /// origin (falls back to +X at the origin).
    pub fn normal(self, center: glam::Vec3) -> glam::Vec3 {
        match self {
            FluxAxis::X => glam::Vec3::X,
            FluxAxis::Y => glam::Vec3::Y,
            FluxAxis::Z => glam::Vec3::Z,
            FluxAxis::Radial => {
                let n = center.normalize_or_zero();
                if n == glam::Vec3::ZERO {
                    glam::Vec3::X
                } else {
                    n
                }
            }
        }
    }
}

/// Which corner the #391 Tier 1 instrumentation HUD panel docks to. Packs into
/// `Shared.instrument2[3]`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum HudDock {
    #[name = "Top left"]
    TopLeft,
    #[name = "Bottom left"]
    BottomLeft,
    #[name = "Top right"]
    TopRight,
    #[name = "Bottom right"]
    BottomRight,
}

impl HudDock {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> HudDock {
        match v {
            1 => HudDock::BottomLeft,
            2 => HudDock::TopRight,
            3 => HudDock::BottomRight,
            _ => HudDock::TopLeft,
        }
    }
}

/// Which *generative algorithm* builds the node field. `Original` is the classic
/// Organic Math cube-field (rotate-then-translate screw motion + the accumulating
/// q-strand). Future variants (Frenet–Serret synthesis, strange attractors,
/// L-systems, …) emit the same strand-bundle the renderer consumes, so they reuse
/// every surface mode + material + look. See `math::Generator` / GitHub #42.
///
/// The chosen generator only changes *which controls* live in the editor's
/// generator column and *how the node positions are produced*; everything
/// downstream (surface mode, materials, lighting, post) is generator-agnostic.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum GeneratorMode {
    #[name = "Organic Math (cube field)"]
    Original,
    /// Frenet–Serret synthesis: integrate a moving frame from curvature κ(s) and
    /// torsion τ(s) along arc-length → helices / coils / winding ribbons. A
    /// phase-offset bundle of strands (Grid topology), so every surface mode works.
    #[name = "Frenet–Serret"]
    Frenet,
    /// DNA double helix: two antiparallel backbones winding a (super-coiled) spine,
    /// rung by base pairs, obeying the supercoiling invariant L = T + W.
    #[name = "DNA double helix"]
    Dna,
    /// Strange attractors: forward-integrate a chaotic field from a set of seeds →
    /// flowing streamline strands (Lorenz / Aizawa / Thomas / Halvorsen).
    #[name = "Strange attractor"]
    Attractor,
    /// Spherical-harmonic eigenmodes: a sphere displaced by a sum of Yₗᵐ modes,
    /// each pulsing on its own sine — the paper's pulsing medusa bell.
    #[name = "Spherical harmonics"]
    Harmonic,
    /// L-system / 3D turtle: rewrite an axiom and walk it with a branching turtle —
    /// ferns, bushes, trees, seaweed.
    #[name = "L-system (plant)"]
    LSystem,
    /// Curl-noise flow field: advect particles through the curl of a noise potential
    /// (divergence-free) → smooth swirling streamlines (ink/smoke/wind).
    #[name = "Curl-noise flow"]
    CurlNoise,
    /// Circular-polarization radiation field: the rotating E-field helix of a
    /// circularly polarized EM wave traced along a (θ,φ) lattice of rays from a
    /// point source — one ray = a corkscrew, a dense fan = the radiating "eye".
    /// A Grid lattice (membrane-loftable into a rippling shell), with an optional
    /// perpendicular B helix per ray.
    #[name = "Circular polarization"]
    Polarization,
    /// Maxwell radiation field: the E/B fields real sources (point charges /
    /// oscillating dipoles) actually produce — superposed, with retarded time. A
    /// (θ,φ) lattice drawing the field-vector tip (Grid), or field-line streamlines
    /// (the textbook dipole rose). Honestly self-consistent, unlike #65's posited
    /// per-ray solution.
    #[name = "Maxwell field"]
    MaxwellField,
    /// Phyllotaxis / golden-angle lattice: Vogel's sunflower disk, Fibonacci sphere,
    /// cone, or log-spiral shell. The parastichy spiral families are the strands
    /// (Grid → membrane skins a spiral ribbon).
    #[name = "Phyllotaxis"]
    Phyllotaxis,
    /// Mandelbulb: a distance-estimated 3-D fractal (White–Nylander power-8 set),
    /// raymarched per-pixel rather than built from nodes. Has no strands/surface
    /// modes — it drives its own raymarch render path (a sibling of Metaball) but
    /// shares the full PBR/IBL/HDR/bloom/camera/beat stack like every other mode.
    #[name = "Mandelbulb"]
    Mandelbulb,
    /// Kaleidoscopic Fractal (KIFS): N-fold kaleidoscopic symmetry + a circle-
    /// inversion absolute-value fold, drawn as a fullscreen per-pixel field (flat
    /// or as a receding tunnel). Like the Mandelbulb it builds no nodes and drives
    /// its own fullscreen render path, sharing the HDR/bloom/tonemap/beat stack.
    #[name = "Kaleidoscopic Fractal"]
    Kaleidoscope,
    /// Boids / flocking (#52): N agents obeying Reynolds' local rules
    /// (separation / alignment / cohesion) + a beat-pulsed goal attractor. The
    /// first STATEFUL generator — the sim is carried on the visual frame-to-frame
    /// and each agent's recent trail becomes a Streamline strand (tangent =
    /// velocity). Tube / metaball / cube all work; membrane degrades to tubes.
    #[name = "Boids (flocking)"]
    Boids,
    /// Tessellation (#121): real aperiodic *tilings* — Penrose rhombi (Phase 1),
    /// later Ammann–Beenker / einstein monotiles / pinwheel / Truchet / hyperbolic
    /// — built CPU-side by inflation/cut-and-project and emitted as geometry (tile
    /// edges → strands now; filled/extruded tiles via the membrane mesh later). The
    /// discrete cousin of the KIFS Quasicrystal field: same 5-fold/golden-ratio
    /// mathematics, but as discrete tiles you can give body to. A Grid-ish node
    /// generator, so every surface mode / material / the beat system apply.
    #[name = "Tessellation (tilings)"]
    Tessellation,
    /// Minimal surfaces & soap films (#127): triply-periodic minimal surfaces
    /// (gyroid / Schwarz P / Schwarz D) — the surface of least area for its
    /// boundary, mean curvature H = 0 — raymarched per-pixel as an implicit
    /// isosurface (a sibling of the Mandelbulb path). No nodes/surface modes; it
    /// drives its own raymarch render path but shares the full PBR/IBL/HDR/bloom/
    /// camera/beat stack, so glass + thin-film iridescence give soap rainbows.
    #[name = "Minimal surfaces"]
    MinimalSurface,
    /// Synchrotron radiation (#150): the Liénard–Wiechert field of relativistic
    /// charge(s) orbiting a circle, solved at the **retarded time** of the moving
    /// source (Newton iteration, unlike Maxwell's fixed-source retarded phase).
    /// Phase 1 samples E on a plane and emits an oriented arrow rod per sample
    /// (Streamlines) — the velocity (1/R²) + relativistically beamed radiation
    /// (1/R) terms, whose lobe sweeps a searchlight spiral as the charge orbits.
    #[name = "Synchrotron radiation"]
    Synchrotron,
    /// Vector-field plotter (#173): plot an arbitrary function F(x, y, z) →
    /// (Fx, Fy, Fz) from a curated bank — the maths-Instagram vector-field
    /// classic, lifted into 3-D. Tier 1 samples F on a lattice inside a box and
    /// emits an oriented arrow rod per sample (length + colour by |F|); one grid
    /// axis at 1 collapses to the literal 2-D plot. Tier 2 adds RK4 field lines,
    /// Tier 3 the function builder (see the issue).
    #[name = "Vector field"]
    VectorField,
    /// The primary generator switched off (#187 scenery pivot): the scene is
    /// carried by the concurrent Scenery layer (and the world layers) alone.
    /// Takes the wire ordinal the retired `Rails` variant held — the corridor
    /// is a `SceneryMode` now, not a generator.
    #[name = "None (off)"]
    None,
    /// Axon Waveguide (#218): a bundle of myelinated axons as step-index optical
    /// fibres — myelin sheath (n≈1.44) over axoplasm (n≈1.38) — drawn as swept
    /// tubes with periodic Ranvier-node constrictions and a travelling "action
    /// potential" pulse. Best viewed in Swept Tubes + Glass/Refractive. Declared
    /// **after** `None` so every existing wire ordinal stays stable (it takes
    /// ordinal 18; `None` keeps the retired-`Rails` ordinal 17).
    #[name = "Axon Waveguide"]
    AxonWaveguide,
    /// Neural field (#200 Tier 1): a tiny SIREN MLP `(x,y,z,t) → (density,rgb)`
    /// (`mlp.wgsl` / `math.rs::mlp_eval`) raymarched per-pixel as an implicit
    /// isosurface — a sibling of the Mandelbulb/KIFS path. No nodes/surface
    /// modes; it drives its own raymarch render path but shares the full
    /// PBR/IBL/HDR/bloom/camera/beat stack. The whole organism is a seed; the
    /// beat drives a latent walk between two seeds for continuous morphs.
    /// Takes ordinal **19** (declared after `AxonWaveguide`, which took 18 on the
    /// main re-merge) so existing wire ordinals don't shift.
    #[name = "Neural field"]
    NeuralField,
    /// Neural Network (#226 Tier 1): a graph of neuron **nodes** (soma blobs) wired
    /// by **edges** — routed fibre tracts (the Axon Waveguide #218 edge, at network
    /// scale). A synthetic topology bank (random-geometric / layered feed-forward /
    /// ring lattice / Watts–Strogatz small-world), deterministic + unit-tested. A
    /// node-field generator (Streamlines), so every Surface mode / material / the
    /// beat system apply — best in Swept Tubes + Glass for the glowing-tract look.
    /// Renders **connectivity + geometry**, not a neural simulation; ANN layouts are
    /// imposed (a weight matrix has no 3-D embedding). Takes ordinal **20**
    /// (declared after `NeuralField` = 19) so existing wire ordinals stay stable.
    #[name = "Neural Network"]
    NeuralNetwork,
    /// Lens (#258 Tier 3): an analytic double-convex / plano-convex lens body,
    /// raymarched per-pixel as an SDF (the intersection of two spheres, or one
    /// sphere with a half-space) — a sibling of the Mandelbulb / Minimal-surface
    /// path. No nodes/surface modes; it drives its own raymarch render path but
    /// shares the full PBR/IBL/HDR/bloom/camera/beat stack, so the Glass/Refractive
    /// material makes it refract (and, under the #258 Tier-2 dielectric tracer,
    /// focus). Takes ordinal **21** (declared after `NeuralNetwork` = 20) so every
    /// existing wire ordinal stays stable.
    #[name = "Lens"]
    Lens,
    /// Demo (#288): a hand-authored **scene bench** for showing off the ray-tracing
    /// stack — Cornell box, sphere pyramids, a glass menagerie, a light stage. Emits
    /// explicit instanced geometry (not a node field), so it inherits shadows / TLAS /
    /// path tracer / SSR for free. `DemoScene` picks the sub-scene. Declared **last**
    /// so it takes ordinal **22** (after `Lens` = 21) — existing wire ordinals stay
    /// stable; off by default / byte-identical when unselected.
    #[name = "Demo (scene bench)"]
    Demo,
    /// Acoustic field (#325, Duo-Field N1): a radiating *sound* source —
    /// monopole / dipole / quadrupole of signed harmonic monopoles, retarded in
    /// time — rendered as a two-channel Duo-Field. The scalar PRESSURE drives the
    /// geometry (a breathing multipole shell on a (θ,φ) ray lattice, Grid /
    /// membrane-loftable) and the vector particle-VELOCITY drives the aura, glowing
    /// by the acoustic energy density. The most on-theme generator: the field IS
    /// sound. Declared **last** so it takes ordinal **23** (after `Demo` = 22) —
    /// existing wire ordinals stay stable.
    #[name = "Acoustic field"]
    Acoustic,
    /// Field Engine (#381 Tier 1): render an arbitrary **closed-form field
    /// equation** over `(x,y,z,t)`. A tiny expression evaluator (`math::
    /// FieldProgram`) returns a scalar `φ`, vector `F`, or complex `ψ`; a
    /// `FieldKind` tag picks the renderer (Vector → field-lines + aura like
    /// Maxwell/Acoustic; Scalar → density/height glyph lattice; Complex → `|ψ|²`
    /// density tinted by phase `arg ψ`). Authored from a built-in **Phenomenon
    /// Gallery** (Coulomb / dipole / ABC flow / hydrogen orbital / plane wave /
    /// vortex / Gaussian) or a hot-reloaded `organic-math-field.txt` sidecar
    /// (`field_gen` counter, exactly like the connectome JSON). Generalizes the
    /// VectorField Tier-3 function builder. Ordinal **24** (after `Acoustic` = 23).
    #[name = "Field Engine"]
    FieldEngine,
    /// Density-Map Attractor (#380 Tier 1): iterate the discrete complex-holomorphic
    /// map `x' = sin(x²−y²+a)`, `y' = cos(2xy+b)` (`z ↦ (sin,cos)` of `z²+p`) for many
    /// points and emit the visited set as a node cloud → an additive density "fire"
    /// (best in `SurfaceMode::Splat` + bloom, or emissive cubes for the shape). A
    /// point-field generator (fills `instances`/`tints`), so every surface mode /
    /// material / the beat system apply. Declared **last** → ordinal **25** (after
    /// `FieldEngine` = 24). Off by default / byte-identical when unselected.
    #[name = "Density-Map Attractor"]
    MapAttractor,
    /// Creature Engine (#476 Tier 1): a synthetic sea creature assembled from a
    /// union of simple SDF primitives (ellipsoids / tapered round-cones / paddles)
    /// placed along a spine, raymarched per-pixel in `creature.wgsl` (a sibling of
    /// the Mandelbulb path). No node set / surface modes — it drives its own
    /// fullscreen render path but shares the full PBR/IBL/HDR/bloom/camera/beat
    /// stack. The `form` param picks one of the built-in body plans (bell jelly /
    /// ribbon-swimmer / paddle-finned predator); a travelling peristaltic warp
    /// (beat-driven) is the swim. Declared **last** → ordinal **26** (after
    /// `MapAttractor` = 25). Off by default / byte-identical when unselected.
    #[name = "Creature Engine"]
    Creature,
}

impl GeneratorMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> GeneratorMode {
        match v {
            1 => GeneratorMode::Frenet,
            2 => GeneratorMode::Dna,
            3 => GeneratorMode::Attractor,
            4 => GeneratorMode::Harmonic,
            5 => GeneratorMode::LSystem,
            6 => GeneratorMode::CurlNoise,
            7 => GeneratorMode::Polarization,
            8 => GeneratorMode::MaxwellField,
            9 => GeneratorMode::Phyllotaxis,
            10 => GeneratorMode::Mandelbulb,
            11 => GeneratorMode::Kaleidoscope,
            12 => GeneratorMode::Boids,
            13 => GeneratorMode::Tessellation,
            14 => GeneratorMode::MinimalSurface,
            15 => GeneratorMode::Synchrotron,
            16 => GeneratorMode::VectorField,
            17 => GeneratorMode::None, // was Rails — retired into SceneryMode (#187 pivot)
            18 => GeneratorMode::AxonWaveguide,
            19 => GeneratorMode::NeuralField, // #200 Tier 1 (re-seated to 19; Axon took 18 on main)
            20 => GeneratorMode::NeuralNetwork, // #226 Tier 1 (graph of nodes + tract edges)
            21 => GeneratorMode::Lens, // #258 Tier 3 (raymarched analytic lens SDF)
            22 => GeneratorMode::Demo, // #288 (scene bench for the RT stack)
            23 => GeneratorMode::Acoustic, // #325 (acoustic Duo-Field: pressure + velocity)
            24 => GeneratorMode::FieldEngine, // #381 Tier 1 (arbitrary closed-form field equations)
            25 => GeneratorMode::MapAttractor, // #380 Tier 1 (discrete density-map attractor)
            26 => GeneratorMode::Creature, // #476 Tier 1 (raymarched SDF sea creatures)
            // Unknown ids fall back to Original.
            _ => GeneratorMode::Original,
        }
    }
    /// Human-readable label (matches the `#[name]`s).
    pub fn to_label(self) -> &'static str {
        match self {
            GeneratorMode::Original => "Organic Math (cube field)",
            GeneratorMode::Frenet => "Frenet–Serret",
            GeneratorMode::Dna => "DNA double helix",
            GeneratorMode::Attractor => "Strange attractor",
            GeneratorMode::Harmonic => "Spherical harmonics",
            GeneratorMode::LSystem => "L-system (plant)",
            GeneratorMode::CurlNoise => "Curl-noise flow",
            GeneratorMode::Polarization => "Circular polarization",
            GeneratorMode::MaxwellField => "Maxwell field",
            GeneratorMode::Phyllotaxis => "Phyllotaxis",
            GeneratorMode::Mandelbulb => "Mandelbulb",
            GeneratorMode::Kaleidoscope => "Kaleidoscopic Fractal",
            GeneratorMode::Boids => "Boids (flocking)",
            GeneratorMode::Tessellation => "Tessellation (tilings)",
            GeneratorMode::MinimalSurface => "Minimal surfaces",
            GeneratorMode::Synchrotron => "Synchrotron radiation",
            GeneratorMode::VectorField => "Vector field",
            GeneratorMode::None => "None (off)",
            GeneratorMode::AxonWaveguide => "Axon Waveguide",
            GeneratorMode::NeuralField => "Neural field",
            GeneratorMode::NeuralNetwork => "Neural Network",
            GeneratorMode::Lens => "Lens",
            GeneratorMode::Demo => "Demo (scene bench)",
            GeneratorMode::Acoustic => "Acoustic field",
            GeneratorMode::FieldEngine => "Field Engine",
            GeneratorMode::MapAttractor => "Density-Map Attractor",
            GeneratorMode::Creature => "Creature Engine",
        }
    }
}

/// Field Engine (#381 Tier 1) render-kind selector. `Auto` infers the kind by
/// probing the compiled program; the explicit variants override (e.g. to force a
/// vector program's magnitude into a scalar density). Order/`to_u32` ride
/// `Shared.field[0]`; **append-only**.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum FieldKind {
    #[name = "Auto (infer)"]
    Auto,
    #[name = "Scalar (density)"]
    Scalar,
    #[name = "Vector (lines + aura)"]
    Vector,
    #[name = "Complex (|psi|^2 + phase)"]
    Complex,
}

impl FieldKind {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> FieldKind {
        match v {
            1 => FieldKind::Scalar,
            2 => FieldKind::Vector,
            3 => FieldKind::Complex,
            _ => FieldKind::Auto,
        }
    }
}

/// Field Engine (#381 Tier 1) Phenomenon Gallery — a bank of named ready-made
/// equations (mirrors `math::field_gallery_src` order) plus `Custom` (the
/// hot-reloaded `organic-math-field.txt` sidecar program) at the fixed sentinel
/// index `math::FIELD_PRESET_CUSTOM` (7). `to_u32` rides `Shared.field[1]`;
/// **append-only, and new phenomena go *after* `Custom`** so its discriminant
/// never shifts and saved presets/host state keep recalling it (Tier 2 added the
/// operator-built presets at 8/9/10 this way).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum FieldPreset {
    #[name = "Coulomb (1/r^2)"]
    Coulomb,
    #[name = "Dipole (1/r^3)"]
    Dipole,
    #[name = "ABC flow"]
    AbcFlow,
    #[name = "Hydrogen orbital |psi|^2"]
    Hydrogen,
    #[name = "Plane wave"]
    PlaneWave,
    #[name = "Vortex"]
    Vortex,
    #[name = "Gaussian"]
    Gaussian,
    #[name = "Custom (sidecar file)"]
    Custom,
    // #381 Tier 2: phenomena built from the differential operators (grad/div/curl).
    // Appended **after** `Custom` so `Custom` keeps its Tier-1 discriminant (7) —
    // saved presets / Ableton state that recorded `Custom` still recall correctly
    // (matches the codebase's append-at-end convention, e.g. `DemoScene`).
    #[name = "E = -grad phi (point charge)"]
    EGradPhi,
    #[name = "B = curl A (uniform)"]
    BCurlA,
    #[name = "Vorticity curl v (ABC)"]
    VorticityAbc,
}

impl FieldPreset {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> FieldPreset {
        match v {
            1 => FieldPreset::Dipole,
            2 => FieldPreset::AbcFlow,
            3 => FieldPreset::Hydrogen,
            4 => FieldPreset::PlaneWave,
            5 => FieldPreset::Vortex,
            6 => FieldPreset::Gaussian,
            7 => FieldPreset::Custom,
            8 => FieldPreset::EGradPhi,
            9 => FieldPreset::BCurlA,
            10 => FieldPreset::VorticityAbc,
            _ => FieldPreset::Coulomb,
        }
    }
}

/// Demo scene bench sub-scenes (#288). Order = the `.to_u32()` discriminant fed
/// into `Shared.demo[0]` and matched in `math::demo_scene`; **append-only** — new
/// scenes go at the end so saved presets keep their scene. Grouped by tier:
/// T0 Cornell box; T1 mixed primitives; T2 per-material; T3 light stage.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum DemoScene {
    #[name = "Cornell box"]
    CornellBox, // T0 — the path tracer's reference scene
    #[name = "Sphere pyramid"]
    SpherePyramid, // T1 — tetrahedral stack of spheres
    #[name = "Sphere grid"]
    SphereGrid, // T1 — a plane of spheres (AO/GI test)
    #[name = "Box + sphere"]
    BoxSphere, // T1 — Cornell box with one sphere
    #[name = "Glass menagerie"]
    GlassMenagerie, // T2 — mirror sphere + glass sphere in the box (the hero)
    #[name = "Light stage"]
    LightStage, // T3 — turntable pedestal + movable coloured area lights
}

impl DemoScene {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> DemoScene {
        match v {
            1 => DemoScene::SpherePyramid,
            2 => DemoScene::SphereGrid,
            3 => DemoScene::BoxSphere,
            4 => DemoScene::GlassMenagerie,
            5 => DemoScene::LightStage,
            _ => DemoScene::CornellBox,
        }
    }
}

/// Neural Network topology (#226 Tier 1). Order matches `math::neural_graph`'s
/// `topo` ids: the synthetic graph bank.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum NeuralTopology {
    #[name = "Random geometric (cloud)"]
    RandomGeometric,
    #[name = "Layered (feed-forward)"]
    Layered,
    #[name = "Ring lattice"]
    RingLattice,
    #[name = "Small-world (Watts–Strogatz)"]
    SmallWorld,
    /// #226 Tier 3: a folded cortical sheet (gyri/sulci) wired short-range (local
    /// cortex) + long-range (white-matter tracts) → the brain's small-world character.
    #[name = "Cortical sheet"]
    Cortical,
    /// #226 Tier 3: an ingested real connectome loaded from a JSON sidecar (the
    /// C. elegans 302-neuron graph, a human atlas, or any nodes+edges file).
    #[name = "Connectome (loaded)"]
    Connectome,
    /// #226 Tier 4: an ingested trained **MLP** (weight matrices) laid out layer by
    /// layer, edges = signed weights, nodes lit by a live forward pass.
    #[name = "MLP (loaded weights)"]
    Mlp,
    /// #226 Tier 5: a transformer's self-attention tensor (a real forward pass from a
    /// JSON sidecar, or a stylized causal synthesis) as a triangular attention graph —
    /// tokens are nodes, causal attention edges carry A_ij, nodes lit by incoming
    /// attention. Tokens are positions, not cells; it renders attention, not "thinking".
    #[name = "Attention (transformer)"]
    Attention,
    /// #275 Tier 1: a **brain model** — two mirrored cerebral hemispheres of folded
    /// cortical mantle (gyri/sulci) split by a longitudinal fissure, plus a cerebellum
    /// + brainstem. Best in the Neural Tissue surface (#260). Stylized anatomy, not an
    /// accurate brain; the substrate the TMS (#271) + entrainment (#264) tools target.
    #[name = "Brain model"]
    Brain,
}

impl NeuralTopology {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> NeuralTopology {
        match v {
            1 => NeuralTopology::Layered,
            2 => NeuralTopology::RingLattice,
            3 => NeuralTopology::SmallWorld,
            4 => NeuralTopology::Cortical,
            5 => NeuralTopology::Connectome,
            6 => NeuralTopology::Mlp,
            7 => NeuralTopology::Attention,
            8 => NeuralTopology::Brain,
            _ => NeuralTopology::RandomGeometric,
        }
    }
}

/// Rails (#187): morph-cell length in beats — the spacing of the corridor's
/// hashed profile control points. Musical values only, so cell boundaries land
/// on beat multiples by construction.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum RailCellLen {
    #[name = "1 beat"]
    Beat,
    #[name = "2 beats"]
    TwoBeats,
    #[name = "1 bar (4)"]
    Bar,
    #[name = "2 bars (8)"]
    TwoBars,
    #[name = "4 bars (16)"]
    FourBars,
}

impl RailCellLen {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> RailCellLen {
        match v {
            0 => RailCellLen::Beat,
            1 => RailCellLen::TwoBeats,
            2 => RailCellLen::Bar,
            4 => RailCellLen::FourBars,
            _ => RailCellLen::TwoBars,
        }
    }
}

/// The Scenery layer (#187 pivot): a SECOND generator category that runs
/// CONCURRENTLY with the primary generator — generated scenery you move
/// through, with its own material/surface/palette (independent of the main
/// look). `Zone` is the flagship: the beat-parametrized corridor (all the
/// rails machinery). Future types: water planes, canyons, … Wire-stable —
/// append only.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum SceneryMode {
    /// Scenery off — the primary generator alone (the pre-pivot look).
    #[name = "None"]
    None,
    /// The beat-parametrized infinite corridor (#187): superformula throat /
    /// phyllo wall / gates / tissue / tiling / flow / waveguide archetypes,
    /// the quantized-transition latch, evolve — the whole rails machinery.
    #[name = "Zone (corridor)"]
    Zone,
    /// Terra (#206 Tier 2): beat-parametrized flowing landscapes — fjords /
    /// river banks / canyons on the same rails machinery, meandering, with a
    /// navigable channel. The landform shape is `Shared.terra[16]`.
    #[name = "Terra (landscape)"]
    Terra,
}

impl SceneryMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> SceneryMode {
        match v {
            1 => SceneryMode::Zone,
            2 => SceneryMode::Terra,
            _ => SceneryMode::None,
        }
    }
}

/// Terra landform family (#206 Tier 2) — biases the per-cell control ranges.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum TerraForm {
    /// Tall, narrow, steep walls with high water — flying up a fjord.
    #[name = "Fjord"]
    Fjord,
    /// Low, wide, soft banks with a mid waterline — a meandering river.
    #[name = "River banks"]
    River,
    /// Tall terraced strata, drier — a canyon run.
    #[name = "Canyon"]
    Canyon,
}

impl TerraForm {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> TerraForm {
        match v {
            1 => TerraForm::River,
            2 => TerraForm::Canyon,
            _ => TerraForm::Fjord,
        }
    }
}

/// How scenery nodes become geometry: the instanced trio, plus the membrane
/// **Skin** (#206 Tier 1) — the lofted continuous surface, so the Zone
/// corridor's Grid archetypes read as a solid throat under the scenery
/// material (and the prerequisite for Terra's landscape skin).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum ScenerySurface {
    /// An independent oriented cube per node (the Original look).
    #[name = "Cubes"]
    Cubes,
    /// Nodes bridged to their successors with stretched boxes.
    #[name = "Flow-Aligned"]
    Rods,
    /// The same bridges as round cylinders.
    #[name = "Swept Tubes"]
    Tubes,
    /// The membrane loft: a continuous skinned surface (Grid archetypes only;
    /// Streamlines archetypes degrade to swept tubes). #206 Tier 1.
    #[name = "Skin (membrane)"]
    Skin,
}

impl ScenerySurface {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> ScenerySurface {
        match v {
            1 => ScenerySurface::Rods,
            2 => ScenerySurface::Tubes,
            3 => ScenerySurface::Skin,
            _ => ScenerySurface::Cubes,
        }
    }
}

/// Rails (#187 Tier 2): which corridor archetype the rail machinery drives.
/// All four share the beat-parametrized window, per-cell morphing, clear-bore
/// invariant, rib flash, fade, and palette flow. Wire-stable — append only.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum RailArchetype {
    /// The Tier 1 morphing superformula throat.
    #[name = "Throat (superformula)"]
    Throat,
    /// Cylindrical phyllotaxis: golden-angle parastichy spirals as the wall.
    #[name = "Phyllo Wall"]
    PhylloWall,
    /// Discrete Fourier rings + torus knots at musical intervals (major gates
    /// on cell boundaries) with dim axial guide rails.
    #[name = "Rings & Gates"]
    Gates,
    /// The Original R·T signature wrapped cylindrically: concentric shells
    /// whose counter-rotation grows spiral arms around the bore.
    #[name = "Tissue Tube"]
    TissueTube,
    /// #187 Tier 4 — a Truchet arc tiling rolled seamlessly onto the wall
    /// (aperiodic tilings can't wrap a cylinder; Truchet can): wandering-path
    /// mosaics, flips re-rolled per phrase by Evolve.
    #[name = "Tiling Liner"]
    TilingLiner,
    /// #187 Tier 4 — ink streamers wandering the wall annulus (noise-driven,
    /// integration-free): Membrane = a waving silk tube, Swept Tubes = the
    /// ink strands. Swell = wander, spike = frequency, twist = bulk swirl.
    #[name = "Flow Media"]
    FlowMedia,
    /// #187 Tier 4 — flying inside a propagating TE_m1 waveguide mode: radial
    /// E-elements + a mode-rippled wall whose pattern travels with the beat
    /// (twist = cycles/beat). Lobes = the azimuthal mode number m.
    #[name = "Waveguide"]
    Waveguide,
}

impl RailArchetype {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> RailArchetype {
        match v {
            1 => RailArchetype::PhylloWall,
            2 => RailArchetype::Gates,
            3 => RailArchetype::TissueTube,
            4 => RailArchetype::TilingLiner,
            5 => RailArchetype::FlowMedia,
            6 => RailArchetype::Waveguide,
            _ => RailArchetype::Throat,
        }
    }
}

/// Rails (#187 Tier 3): the musical phrase length — the boundary lattice the
/// quantized transitions land on, and the `evolve` re-roll period. Ordinals
/// map to beats via `math::RAILS_CHANGE_TAB` (4/8/16/32/64).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum RailChangeEvery {
    #[name = "1 bar (4)"]
    Bar,
    #[name = "2 bars (8)"]
    TwoBars,
    #[name = "4 bars (16)"]
    FourBars,
    #[name = "8 bars (32)"]
    EightBars,
    #[name = "16 bars (64)"]
    SixteenBars,
}

impl RailChangeEvery {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> RailChangeEvery {
        match v {
            0 => RailChangeEvery::Bar,
            1 => RailChangeEvery::TwoBars,
            2 => RailChangeEvery::FourBars,
            4 => RailChangeEvery::SixteenBars,
            _ => RailChangeEvery::EightBars,
        }
    }
}

/// Minimal-surface family (#127). Implicit (TPMS) families 0..2 drive the
/// raymarched isosurface path (Phase 1; indices match `TPMS_*` + `tpms()` in
/// `minimal.wgsl`); parametric families ≥ 3 drive the Grid / membrane-loft path
/// (Phase 2; indices match `MINIMAL_*` in `math.rs`). Catenoid + Helicoid are one
/// associate family (the bend). (Costa is a deferred follow-up.)
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MinimalFamily {
    #[name = "Gyroid"]
    Gyroid,
    #[name = "Schwarz P"]
    SchwarzP,
    #[name = "Schwarz D (diamond)"]
    SchwarzD,
    #[name = "Enneper"]
    Enneper,
    #[name = "Catenoid"]
    Catenoid,
    #[name = "Helicoid"]
    Helicoid,
    // Implicit soap families (Phase 3) → the raymarch path (like the TPMS families).
    #[name = "Bubbles"]
    Bubbles,
    #[name = "Foam"]
    Foam,
    // Algebraic-surface bank (Phase 4): classic implicit polynomials F(x,y,z)=0,
    // raymarched (fixed-step, like TPMS). (Cayley → Tanglecube, a verified quartic.)
    #[name = "Clebsch cubic"]
    Clebsch,
    #[name = "Barth sextic"]
    Barth,
    #[name = "Kummer quartic"]
    Kummer,
    #[name = "Heart"]
    Heart,
    #[name = "Tanglecube"]
    Tanglecube,
    // CMC surfaces of revolution (Phase 4b): constant mean curvature, parametric
    // (Grid + membrane loft) — the bubble-chain / liquid-bridge Delaunay surfaces.
    #[name = "Unduloid (CMC)"]
    Unduloid,
    #[name = "Nodoid (CMC)"]
    Nodoid,
}

/// Capture / production-frame aspect ratio (#135 Phase 1). `Native` keeps the
/// current behaviour (render straight to the window swapchain); every other
/// variant renders into a fixed-resolution offscreen target that is then blitted
/// centred (letterboxed) into the window — so an OBS capture is pixel-exact. This
/// is a **per-display** setting (not preset-captured), a sibling of HDR/MSAA.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum AspectPreset {
    #[name = "Native (window)"]
    Native,
    #[name = "Portrait 9:16"]
    Portrait916,
    #[name = "Landscape 16:9"]
    Landscape169,
    #[name = "Square 1:1"]
    Square11,
    #[name = "Portrait 4:5"]
    Portrait45,
    #[name = "Cinematic 21:9"]
    Cinematic219,
    #[name = "Custom"]
    Custom,
}

impl AspectPreset {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> AspectPreset {
        match v {
            1 => AspectPreset::Portrait916,
            2 => AspectPreset::Landscape169,
            3 => AspectPreset::Square11,
            4 => AspectPreset::Portrait45,
            5 => AspectPreset::Cinematic219,
            6 => AspectPreset::Custom,
            _ => AspectPreset::Native,
        }
    }
    /// Width:height ratio for the fixed presets. `Native`/`Custom` return `None`
    /// (their size comes from the window / the custom W×H instead).
    pub fn ratio(self) -> Option<(u32, u32)> {
        match self {
            AspectPreset::Portrait916 => Some((9, 16)),
            AspectPreset::Landscape169 => Some((16, 9)),
            AspectPreset::Square11 => Some((1, 1)),
            AspectPreset::Portrait45 => Some((4, 5)),
            AspectPreset::Cinematic219 => Some((21, 9)),
            AspectPreset::Native | AspectPreset::Custom => None,
        }
    }
}

impl MinimalFamily {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> MinimalFamily {
        match v {
            1 => MinimalFamily::SchwarzP,
            2 => MinimalFamily::SchwarzD,
            3 => MinimalFamily::Enneper,
            4 => MinimalFamily::Catenoid,
            5 => MinimalFamily::Helicoid,
            6 => MinimalFamily::Bubbles,
            7 => MinimalFamily::Foam,
            8 => MinimalFamily::Clebsch,
            9 => MinimalFamily::Barth,
            10 => MinimalFamily::Kummer,
            11 => MinimalFamily::Heart,
            12 => MinimalFamily::Tanglecube,
            13 => MinimalFamily::Unduloid,
            14 => MinimalFamily::Nodoid,
            _ => MinimalFamily::Gyroid,
        }
    }
}

/// Boids creature form (#52): how each flocking agent is drawn. `Surface` keeps
/// the normal surface mode (cubes / tubes / metaball …); every other variant
/// overrides it with a per-agent creature mesh oriented by velocity. The
/// non-`Surface` indices map to `render::creature_mesh` kinds (Fish = 0, …).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum BoidsForm {
    #[name = "Surface (normal)"]
    Surface,
    #[name = "Fish"]
    Fish,
    #[name = "Bird"]
    Bird,
    #[name = "Manta ray"]
    Manta,
    #[name = "Dart"]
    Dart,
}

impl BoidsForm {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> BoidsForm {
        match v {
            1 => BoidsForm::Fish,
            2 => BoidsForm::Bird,
            3 => BoidsForm::Manta,
            4 => BoidsForm::Dart,
            _ => BoidsForm::Surface,
        }
    }
    /// Creature-mesh kind (`render::creature_mesh`), or `None` for `Surface`.
    pub fn creature_kind(self) -> Option<u32> {
        match self {
            BoidsForm::Surface => None,
            BoidsForm::Fish => Some(0),
            BoidsForm::Bird => Some(1),
            BoidsForm::Manta => Some(2),
            BoidsForm::Dart => Some(3),
        }
    }
}

/// Built-in L-system rule set. Order matches `math::lsystem_rules`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum LSystem {
    #[name = "Fern"]
    Fern,
    #[name = "Bush"]
    Bush,
    #[name = "Tree"]
    Tree,
    #[name = "Seaweed"]
    Seaweed,
}

impl LSystem {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> LSystem {
        match v {
            1 => LSystem::Bush,
            2 => LSystem::Tree,
            3 => LSystem::Seaweed,
            _ => LSystem::Fern,
        }
    }
}

/// Axon Waveguide guided mode (#218 Tier 2). Order matches `math::lp_mode_intensity`:
/// the LP_lm modes whose transverse intensity lights the bundle cross-section.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum AxonMode {
    #[name = "LP01 (core)"]
    Lp01,
    #[name = "LP11 (2 lobes)"]
    Lp11,
    #[name = "LP21 (4 lobes)"]
    Lp21,
    #[name = "LP02 (core + ring)"]
    Lp02,
    #[name = "LP31 (6 lobes)"]
    Lp31,
    #[name = "LP12 (lobes + ring)"]
    Lp12,
}

impl AxonMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> AxonMode {
        match v {
            1 => AxonMode::Lp11,
            2 => AxonMode::Lp21,
            3 => AxonMode::Lp02,
            4 => AxonMode::Lp31,
            5 => AxonMode::Lp12,
            _ => AxonMode::Lp01,
        }
    }
}

/// Neural Network signal-propagation firing mode (#226 Tier 2). Order matches the
/// `mode` id `math::NeuralSim::step` reads. `Off` = the Tier-1 free-running pulse
/// look (no cascade); the others seed the activation cascade.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum NeuralFireMode {
    #[name = "Off (free-running pulse)"]
    Off,
    #[name = "Wavefront (sweep)"]
    Wavefront,
    #[name = "Oscillation (idle)"]
    Oscillation,
    #[name = "Stimulus (ripple out)"]
    Stimulus,
}

impl NeuralFireMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> NeuralFireMode {
        match v {
            1 => NeuralFireMode::Wavefront,
            2 => NeuralFireMode::Oscillation,
            3 => NeuralFireMode::Stimulus,
            _ => NeuralFireMode::Off,
        }
    }
}

/// Phyllotaxis surface. Order matches `math::phyllotaxis_node`'s surface ids.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum PhylSurface {
    #[name = "Disk"]
    Disk,
    #[name = "Cone"]
    Cone,
    #[name = "Sphere"]
    Sphere,
    #[name = "Shell"]
    Shell,
}

impl PhylSurface {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> PhylSurface {
        match v {
            1 => PhylSurface::Cone,
            2 => PhylSurface::Sphere,
            3 => PhylSurface::Shell,
            _ => PhylSurface::Disk,
        }
    }
}

/// Synchrotron render view (#150). **Arrows** = the field-vector tip per plane
/// sample (Phase 1); **FieldLines** = traced E streamlines (Phase 3); **Volume** =
/// the arrow plane extruded into a 3-D box of samples (Phase 4). Indices are
/// wire-stable / append-only (they pack into the `synchrotron` IPC block).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum SyncView {
    #[name = "Field arrows"]
    Arrows,
    #[name = "Field lines"]
    FieldLines,
    #[name = "Field volume"]
    Volume,
}

impl SyncView {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> SyncView {
        match v {
            1 => SyncView::FieldLines,
            2 => SyncView::Volume,
            _ => SyncView::Arrows,
        }
    }
}

/// Vector-field preset bank (#173 Tier 1). Indices are wire-stable /
/// append-only (they pack into `Shared.vecfield[0]` and index
/// `math::vecfield_eval`). Entries 0–1 are the reference reel's two fields;
/// the planar classics (0/1/2/9/11) accept the `vf_z_lift` 3-D extension.
/// Tier 3 (the function builder) appends a "Custom" entry here.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum VecFieldPreset {
    #[name = "Parabolic swirl (y2, -x2)"]
    ParabolicSwirl,
    #[name = "Sine saddle (sin y, sin x)"]
    SineSaddle,
    #[name = "Rotation (-y, x)"]
    Rotation,
    #[name = "Point source (radial)"]
    Source,
    #[name = "Saddle 3D (x, y, -2z)"]
    Saddle3D,
    #[name = "Dipole"]
    Dipole,
    #[name = "ABC flow (Beltrami)"]
    AbcFlow,
    #[name = "Lorenz field"]
    Lorenz,
    #[name = "Helix (-y, x, h)"]
    Helix,
    #[name = "Double well (y, x - x3)"]
    DoubleWell,
    #[name = "Taylor-Green vortices"]
    TaylorGreen,
    #[name = "Vortex pair"]
    VortexPair,
    /// #173 Tier 3: the function builder — F is composed from the `vb_*` term
    /// params (each component = 3 × `gain·func(a·x + b·y + c·z + phase)`) and
    /// an optional gradient/curl/Helmholtz field operator.
    #[name = "Custom (builder)"]
    Custom,
}

impl VecFieldPreset {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> VecFieldPreset {
        match v {
            1 => VecFieldPreset::SineSaddle,
            2 => VecFieldPreset::Rotation,
            3 => VecFieldPreset::Source,
            4 => VecFieldPreset::Saddle3D,
            5 => VecFieldPreset::Dipole,
            6 => VecFieldPreset::AbcFlow,
            7 => VecFieldPreset::Lorenz,
            8 => VecFieldPreset::Helix,
            9 => VecFieldPreset::DoubleWell,
            10 => VecFieldPreset::TaylorGreen,
            11 => VecFieldPreset::VortexPair,
            12 => VecFieldPreset::Custom,
            _ => VecFieldPreset::ParabolicSwirl,
        }
    }
}

/// Builder term shaping function (#173 Tier 3): the `func` in
/// `gain·func(a·x + b·y + c·z + phase)`. **Off** silences the term slot.
/// Inverse is a soft, odd, bounded 1/u (no pole). Ids are wire-stable — they
/// pack into the `vecbuild` term slots (`math::VecTerm::eval`).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum VecTermFunc {
    #[name = "Off"]
    Off,
    #[name = "Constant (1)"]
    Const,
    #[name = "Linear (u)"]
    Linear,
    #[name = "Square (u2)"]
    Square,
    #[name = "Cube (u3)"]
    Cube,
    #[name = "Abs |u|"]
    Abs,
    #[name = "Sin"]
    Sin,
    #[name = "Cos"]
    Cos,
    #[name = "Gauss e^-u2"]
    Gauss,
    #[name = "Inverse (soft 1/u)"]
    Inverse,
}

impl VecTermFunc {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> VecTermFunc {
        match v {
            1 => VecTermFunc::Const,
            2 => VecTermFunc::Linear,
            3 => VecTermFunc::Square,
            4 => VecTermFunc::Cube,
            5 => VecTermFunc::Abs,
            6 => VecTermFunc::Sin,
            7 => VecTermFunc::Cos,
            8 => VecTermFunc::Gauss,
            9 => VecTermFunc::Inverse,
            _ => VecTermFunc::Off,
        }
    }
}

/// Builder field operator (#173 Tier 3): how the built triple is interpreted.
/// **Direct** = F as built; **Gradient** = ∇φ with φ = the Fx term row
/// (curl-free — sources/sinks/saddles); **Curl** = ∇×A with A = the triple
/// (divergence-free — pure swirl, closed lines); **Helmholtz** blends the two
/// by `vb_mix`. Central differences in the scaled domain. Wire-stable
/// (packs into `Shared.vecbuild[54]`).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum VecFieldOp {
    #[name = "Direct"]
    Direct,
    #[name = "Gradient (curl-free)"]
    Gradient,
    #[name = "Curl (divergence-free)"]
    Curl,
    #[name = "Helmholtz blend"]
    Helmholtz,
}

impl VecFieldOp {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> VecFieldOp {
        match v {
            1 => VecFieldOp::Gradient,
            2 => VecFieldOp::Curl,
            3 => VecFieldOp::Helmholtz,
            _ => VecFieldOp::Direct,
        }
    }
}

/// Vector-field render view (#173 Tier 2). **Arrows** = the Tier-1 lattice;
/// **FieldLines** = RK4-traced streamlines of the same field ("filling it in");
/// **Both** = faint arrows under the lines (the reel's composite shot).
/// Wire-stable (packs into `Shared.vecfield[13]`).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum VecFieldView {
    #[name = "Field arrows"]
    Arrows,
    #[name = "Field lines"]
    FieldLines,
    #[name = "Arrows + lines"]
    Both,
    /// Stream surface (#173 follow-up): equal-length field lines traced from an
    /// ordered seed curve and returned as **Grid** topology, so Membrane mode
    /// lofts a flowing sheet through the field (free-running lines can't loft).
    #[name = "Stream surface"]
    Surface,
}

impl VecFieldView {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> VecFieldView {
        match v {
            1 => VecFieldView::FieldLines,
            2 => VecFieldView::Both,
            3 => VecFieldView::Surface,
            _ => VecFieldView::Arrows,
        }
    }
}

/// Field-line seeding strategy (#173 Tier 2). Lattice = a coarse ∛n grid;
/// Random = a deterministic hash scatter; Ring = a circle in the z = 0 plane
/// (clean for swirls/dipoles); Plane = a √n grid on z = 0 (the 2-D plot,
/// filled); Magnitude-weighted = more lines where the field is strong.
/// Wire-stable (packs into `Shared.vecfield[14]`).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum VecSeedMode {
    #[name = "Lattice"]
    Lattice,
    #[name = "Random scatter"]
    Random,
    #[name = "Ring"]
    Ring,
    #[name = "Plane (z = 0)"]
    Plane,
    #[name = "Magnitude-weighted"]
    MagWeighted,
}

impl VecSeedMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> VecSeedMode {
        match v {
            1 => VecSeedMode::Random,
            2 => VecSeedMode::Ring,
            3 => VecSeedMode::Plane,
            4 => VecSeedMode::MagWeighted,
            _ => VecSeedMode::Lattice,
        }
    }
}

/// Field-line colour source (#173 Tier 2): by local |F| (the native ramp / the
/// loaded palette) or a sweep along each line's length (palette, else the HSV
/// wheel — the #19 swept-tubes look). Wire-stable (packs into
/// `Shared.vecfield[19]`).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum VecLineColor {
    #[name = "Magnitude"]
    Magnitude,
    #[name = "Sweep along line"]
    Sweep,
}

impl VecLineColor {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> VecLineColor {
        match v {
            1 => VecLineColor::Sweep,
            _ => VecLineColor::Magnitude,
        }
    }
}

/// How |F| maps to arrow length (#173). **Soft** = m/(m+1) (the default —
/// bounded, keeps weak arrows visible); **Log** compresses huge pole/far-field
/// spans (1/r² fields); **Uniform** equalizes lengths (direction only, the
/// textbook normalized plot). Wire-stable (packs into `Shared.vecfield[8]`).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum VecMagMap {
    #[name = "Soft saturate"]
    Soft,
    #[name = "Log compress"]
    Log,
    #[name = "Uniform (direction only)"]
    Uniform,
}

impl VecMagMap {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> VecMagMap {
        match v {
            1 => VecMagMap::Log,
            2 => VecMagMap::Uniform,
            _ => VecMagMap::Soft,
        }
    }
}

/// Arrow tint source (#173): by field **magnitude** (the native indigo→cyan→white
/// ramp, or the loaded palette swept by |F|) or by field **direction** (F̂ → RGB;
/// with a palette, the in-plane angle of F̂ walks it). Wire-stable
/// (packs into `Shared.vecfield[9]`).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum VecTint {
    #[name = "Magnitude"]
    Magnitude,
    #[name = "Direction"]
    Direction,
}

impl VecTint {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> VecTint {
        match v {
            1 => VecTint::Direction,
            _ => VecTint::Magnitude,
        }
    }
}

/// Tessellation family (#121). The full roadmap is declared up front so the
/// selector is well-formed (nih-plug can't normalize a single-variant enum) and
/// the indices are **wire-stable / append-only** (they pack into the
/// `tessellation` IPC block). **Phase 1 implements Penrose P3 only**; the others
/// gracefully fall back to Penrose in `math::tessellation_strands` until their
/// construction lands (Ammann–Beenker in Phase 4; the rest in Phase 5).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum TilingFamily {
    #[name = "Penrose (P3 rhombi)"]
    PenroseP3,
    #[name = "Ammann–Beenker (8-fold)"]
    AmmannBeenker,
    #[name = "Hat / Spectre (einstein)"]
    HatSpectre,
    #[name = "Pinwheel"]
    Pinwheel,
    #[name = "Truchet"]
    Truchet,
    #[name = "Hyperbolic {p,q}"]
    Hyperbolic,
    #[name = "Periodic (Archimedean)"]
    Periodic,
}

impl TilingFamily {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> TilingFamily {
        match v {
            1 => TilingFamily::AmmannBeenker,
            2 => TilingFamily::HatSpectre,
            3 => TilingFamily::Pinwheel,
            4 => TilingFamily::Truchet,
            5 => TilingFamily::Hyperbolic,
            6 => TilingFamily::Periodic,
            _ => TilingFamily::PenroseP3,
        }
    }
}

/// Tessellation view (#121, Phase 2): the 2D→3D ladder. `Edges` = Phase-1 glowing
/// wireframe; `Filled` = flat triangulated tiles; `Extruded` = per-tile prisms
/// (the "quasicrystal cityscape"). Filled/Extruded ride the membrane mesh path, so
/// PBR / Chrome / Glass all apply. Wire-stable / append-only.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum TessView {
    #[name = "Edges (wireframe)"]
    Edges,
    #[name = "Filled (flat tiles)"]
    Filled,
    #[name = "Extruded (prisms)"]
    Extruded,
    /// Honest 3-D icosahedral quasicrystal (#121, Phase 5): a Z⁶ cut-and-project
    /// rod lattice (Zometool-like). Overrides the family (it's its own 3-D
    /// structure); `grid range` sets its size, `phason` animates it.
    #[name = "3-D quasicrystal"]
    Quasicrystal3D,
}

impl TessView {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> TessView {
        match v {
            1 => TessView::Filled,
            2 => TessView::Extruded,
            3 => TessView::Quasicrystal3D,
            _ => TessView::Edges,
        }
    }
}

/// Tiling construction method (#121, Phase 4). `Inflation` = substitution /
/// deflation (Penrose only); `CutProject` = de Bruijn multigrid (cut-and-project),
/// which unlocks **phason flips** and the **Ammann–Beenker** family. Ammann–Beenker
/// always uses cut-and-project regardless of this setting. Wire-stable.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum TilingConstruct {
    #[name = "Inflation (substitution)"]
    Inflation,
    #[name = "Cut-and-project (phason)"]
    CutProject,
}

impl TilingConstruct {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> TilingConstruct {
        match v {
            1 => TilingConstruct::CutProject,
            _ => TilingConstruct::Inflation,
        }
    }
}

/// How an extruded tile's prism height is chosen (#121, Phase 2). Wire-stable.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum TessHeightMode {
    #[name = "Uniform"]
    Uniform,
    #[name = "By tile type"]
    ByType,
    #[name = "Radial"]
    Radial,
}

impl TessHeightMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> TessHeightMode {
        match v {
            1 => TessHeightMode::ByType,
            2 => TessHeightMode::Radial,
            _ => TessHeightMode::Uniform,
        }
    }
}

/// Strange-attractor vector field. Order matches `math::attractor_velocity`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum AttractorField {
    #[name = "Lorenz"]
    Lorenz,
    #[name = "Aizawa"]
    Aizawa,
    #[name = "Thomas"]
    Thomas,
    #[name = "Halvorsen"]
    Halvorsen,
}

impl AttractorField {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> AttractorField {
        match v {
            1 => AttractorField::Aizawa,
            2 => AttractorField::Thomas,
            3 => AttractorField::Halvorsen,
            _ => AttractorField::Lorenz,
        }
    }
}

/// DNA conformation preset. A/B/Z fix the geometry table (bp/turn, rise, diameter,
/// groove, handedness); `Custom` uses the editor's Frenet-style sliders.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum DnaForm {
    #[name = "A-DNA"]
    A,
    #[name = "B-DNA"]
    B,
    #[name = "Z-DNA (left)"]
    Z,
    #[name = "Custom"]
    Custom,
}

impl DnaForm {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> DnaForm {
        match v {
            0 => DnaForm::A,
            2 => DnaForm::Z,
            3 => DnaForm::Custom,
            _ => DnaForm::B,
        }
    }
}

/// Geometry of the bioluminescent emissive ripple (the travelling HDR pulse).
/// `Radial` = expanding shells from the field centre; `Axial` = a planar
/// wavefront sweeping along the world +Y axis.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum RippleGeom {
    #[name = "Radial"]
    Radial,
    #[name = "Axial"]
    Axial,
}

impl RippleGeom {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> RippleGeom {
        match v {
            1 => RippleGeom::Axial,
            _ => RippleGeom::Radial,
        }
    }
}

/// Which procedurally synthesized 256² noise tile drives the terrain backdrop.
/// The tile's *statistics* steer the fBm landscape: white = classic fractal
/// mountains; cellular = ridged mesas; smooth = rolling hills; spires = jagged.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum TerrainNoise {
    #[name = "White (fractal)"]
    White,
    #[name = "Cellular (mesas)"]
    Cellular,
    #[name = "Smooth (rolling)"]
    Smooth,
    #[name = "Spires (jagged)"]
    Spires,
    // The following take their character from Cinema 4D's noise collection.
    #[name = "Voronoi (cracked)"]
    Voronoi,
    #[name = "Dents (pocked)"]
    Dents,
    #[name = "Wavy (strata)"]
    Wavy,
    #[name = "Sparse (buttes)"]
    Sparse,
}

impl TerrainNoise {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
}

/// Terrain render resolution (perf): the raymarch runs at this fraction of the
/// scene resolution, then point-upscales. `Full` is the direct draw; `Half` ≈ 4×
/// cheaper, `Quarter` ≈ 16×. The IPC carries the integer divisor (1/2/4).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum TerrainRes {
    #[name = "Full"]
    Full,
    #[name = "Half (≈4× faster)"]
    Half,
    #[name = "Quarter (≈16× faster)"]
    Quarter,
    #[name = "Eighth (≈64× faster)"]
    Eighth,
}

impl TerrainRes {
    /// Pixel divisor: Full = 1, Half = 2, Quarter = 4, Eighth = 8.
    pub fn divisor(self) -> u32 {
        match self {
            TerrainRes::Full => 1,
            TerrainRes::Half => 2,
            TerrainRes::Quarter => 4,
            TerrainRes::Eighth => 8,
        }
    }
}

/// Particle Aura tier (#81). `Off` = no particles (image identical). `Lite` =
/// advection: a drifting halo of motes that ride the generator's velocity field
/// (analytic where available, splatted from node motion otherwise). A `Fluid`
/// (Navier–Stokes) tier is a planned follow-up; `Lite` is the everyday look.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum ParticleTier {
    #[name = "Off"]
    Off,
    #[name = "Lite (advection)"]
    Lite,
    #[name = "Fluid (Navier–Stokes)"]
    Fluid,
}

impl ParticleTier {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> ParticleTier {
        match v {
            1 => ParticleTier::Lite,
            2 => ParticleTier::Fluid,
            _ => ParticleTier::Off,
        }
    }
}

/// Particle bead material (#298 Tier 2): the PBR shading of the shaded droplets
/// (`particles_beads` on). Mirrors the cube `MaterialType` subset the impostor can
/// afford — env-only, so Glass/Refractive reflect+refract the environment (the
/// scene-behind refraction is the Tier-4 RT job). Wire ids are append-only.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum ParticleMaterial {
    #[name = "Standard (pearl)"]
    Standard,
    #[name = "Chrome (mirror)"]
    Chrome,
    #[name = "Glass (refract env)"]
    Glass,
    #[name = "Refractive (murk)"]
    Refractive,
}

impl ParticleMaterial {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> ParticleMaterial {
        match v {
            1 => ParticleMaterial::Chrome,
            2 => ParticleMaterial::Glass,
            3 => ParticleMaterial::Refractive,
            _ => ParticleMaterial::Standard,
        }
    }
}

/// Particle bead shape (#298 Tier 2): the impostor SDF sphere-traced inside the
/// billboard. `Sphere` keeps the cheap analytic Tier-1 impostor; the rest raymarch
/// a per-mote SDF oriented by the mote's velocity (teardrops streak along the flow).
/// Wire ids are append-only.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum ParticleShape {
    #[name = "Sphere"]
    Sphere,
    #[name = "Ellipsoid"]
    Ellipsoid,
    #[name = "Teardrop"]
    Teardrop,
    #[name = "Rounded Box"]
    RoundedBox,
    #[name = "Dice"]
    Dice,
}

impl ParticleShape {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> ParticleShape {
        match v {
            1 => ParticleShape::Ellipsoid,
            2 => ParticleShape::Teardrop,
            3 => ParticleShape::RoundedBox,
            4 => ParticleShape::Dice,
            _ => ParticleShape::Sphere,
        }
    }
}

/// Terrain colour palette: recolours the rock / vegetation / snow albedo. The
/// shader (`terrain.wgsl::palette`) holds the matching colour sets.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum TerrainPalette {
    #[name = "Alpine"]
    Alpine,
    #[name = "Desert"]
    Desert,
    #[name = "Volcanic"]
    Volcanic,
    #[name = "Arctic"]
    Arctic,
    #[name = "Verdant"]
    Verdant,
    #[name = "Mars"]
    Mars,
    #[name = "Monochrome"]
    Monochrome,
}

impl TerrainPalette {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
}

/// How the Membrane mode lofts sheets between adjacent strands. `Auto` weaves
/// across the grid axis with the most strands (one continuous bell/sheet);
/// `AlongX/Y/Z` force a specific axis; `Web` weaves across every axis with more
/// than one strand (sheets on each grid face — a 3-D web).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MembraneWeave {
    #[name = "Auto (sheet)"]
    Auto,
    #[name = "Along X"]
    AlongX,
    #[name = "Along Y"]
    AlongY,
    #[name = "Along Z"]
    AlongZ,
    #[name = "Web (all axes)"]
    Web,
}

impl MembraneWeave {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> MembraneWeave {
        match v {
            1 => MembraneWeave::AlongX,
            2 => MembraneWeave::AlongY,
            3 => MembraneWeave::AlongZ,
            4 => MembraneWeave::Web,
            _ => MembraneWeave::Auto,
        }
    }
}

/// How the Membrane's **Skin-Arms** mode builds the per-arm fingers. `Impostor`
/// chains capsule sphere-impostors along each strand (no per-frame mesh — the
/// bead-impostor trick reused for arms); `Mesh` welds each strand into a real
/// capped swept-tube (seamless, but rebuilt every frame). Both leave open gaps
/// between arms.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MembraneArmBuild {
    #[name = "Impostor (capsules)"]
    Impostor,
    #[name = "Mesh (welded tubes)"]
    Mesh,
}

impl MembraneArmBuild {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> MembraneArmBuild {
        match v {
            1 => MembraneArmBuild::Mesh,
            _ => MembraneArmBuild::Impostor,
        }
    }
}

/// Material/shading model for the cubes. `Standard` = the metallic-roughness PBR.
/// `Chrome` = a polished mirror that reflects the environment sharply. `Glass` =
/// a translucent surface that reflects + refracts the environment (Fresnel-blended)
/// and lets the scene behind show through (alpha). `Refractive` = Glass plus
/// Beer–Lambert **absorption** over the measured path through each node's body
/// (the liquid's see-through-water optics brought to the generators): the chord
/// through the instance along the refracted ray attenuates the transmission, so
/// thin edges stay clear while thick bodies go murky in the node's own colour —
/// `mat_absorb` is the strength. All of these reflect the skybox / loaded HDR via
/// the prefiltered IBL; reflecting the *other cubes* is SSR's job (the Reflections
/// card), and Refractive's see-through of the scene behind is the alpha blend
/// (screen-space displaced refraction of neighbours, like the liquid's post pass,
/// would need a refraction G-buffer — a separate effort).
/// `Anisotropic` = Standard PBR with an **elliptical** GGX specular lobe instead of
/// a round one (brushed metal / satin / hair): the highlight stretches along a brush
/// direction taken from the instance frame's long axis (the rod/tube axis; a
/// rotation dial re-aims it on cubes). `anisotropy` sets the strength/direction and
/// `aniso_rotation` the brush angle; the same lobe is also exposed as an **overlay**
/// (`aniso_overlay`) on Standard/Chrome.
///
/// `Clearcoat` = Standard PBR under a thin **smooth dielectric coat** (a second
/// specular lobe, F0 ≙ IOR 1.5) — car paint / lacquer / ceramic / wet. `Velvet` =
/// Standard PBR with a **sheen** lobe (Charlie NDF) that blooms at grazing angles —
/// velvet / dust / moss. Both are also exposed as **overlays** (`clearcoat_overlay`
/// / `sheen_overlay`) on Standard/Chrome, so you can lacquer a brushed metal or dust
/// any surface. (Glass/Refractive keep their transmissive look — the lobes are inert
/// there.)
/// `Subsurface` = Standard PBR with the **translucency lobe driven by the measured
/// body thickness** (#214 T3): the back-glow is Beer–Lambert-attenuated over the real
/// chord through the instance (`instance_thickness`), so thin silhouette edges glow
/// and thick centres go deep — honest wax / jade / marble / skin. `sss_thickness`
/// controls the drive and `sss_radius` the penetration; the same thickness model is
/// exposed on any material through the existing Surface-FX translucency + the
/// `sss_thickness` dial, and the Glass/Refractive body gains an `interior_scatter`
/// glow (opal). Picking `Subsurface` just forces the lobe on.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MaterialType {
    #[name = "Standard"]
    Standard,
    #[name = "Chrome"]
    Chrome,
    #[name = "Glass"]
    Glass,
    #[name = "Refractive"]
    Refractive,
    #[name = "Anisotropic"]
    Anisotropic,
    #[name = "Clearcoat"]
    Clearcoat,
    #[name = "Velvet"]
    Velvet,
    #[name = "Subsurface"]
    Subsurface,
}

impl MaterialType {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> MaterialType {
        match v {
            1 => MaterialType::Chrome,
            2 => MaterialType::Glass,
            3 => MaterialType::Refractive,
            4 => MaterialType::Anisotropic,
            5 => MaterialType::Clearcoat,
            6 => MaterialType::Velvet,
            7 => MaterialType::Subsurface,
            _ => MaterialType::Standard,
        }
    }
}

/// Field Chamber panel style (#346). `Flat` = the cheap 2-D composite (a thin ribbon
/// for the scope, flat quads for the spectrum bars, drawn on the wall). `Impostor` =
/// the #298 bead technique — a swept rounded tube for the scope + rounded bars for the
/// spectrum, sphere-traced in the fragment shader, taking a `MaterialType` (Chrome /
/// Glass / metallic, env-only reflection) and writing `frag_depth` so they join the
/// depth-prepass FX. Packs into `Shared.chamber[1]`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum PanelStyle {
    #[name = "Flat"]
    Flat,
    #[name = "Impostor"]
    Impostor,
}

impl PanelStyle {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> PanelStyle {
        match v {
            1 => PanelStyle::Impostor,
            _ => PanelStyle::Flat,
        }
    }
}

/// Reflection source (#163 Tier 2). Selects how the environment reflection direction
/// is computed for all three materials. `EnvOnly` = today (a pure direction lookup into
/// the infinitely-distant env map — depends only on face orientation). `Parallax` =
/// box-projected (the reflection ray is intersected against the field's AABB so the
/// reflection also shifts with a cube's *position*). Extensible later to real-time
/// probes / VXGI. Packs into `Shared.refl_probe[0]`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum ReflectionSource {
    #[name = "Env Only"]
    EnvOnly,
    #[name = "Parallax Box"]
    Parallax,
}

impl ReflectionSource {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> ReflectionSource {
        match v {
            1 => ReflectionSource::Parallax,
            _ => ReflectionSource::EnvOnly,
        }
    }
}

/// NPR / stylized post style (#152), applied by the post-composite FX pass
/// (`fx.wgsl`) on the final composited image — orthogonal to the generator and
/// material. `None` = photoreal (the default). The id packs into `Shared.fx[1]`.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum RenderStyle {
    #[name = "None"]
    None,
    #[name = "Toon"]
    Toon,
    #[name = "Outline"]
    Outline,
    #[name = "Halftone"]
    Halftone,
    #[name = "Dither"]
    Dither,
    #[name = "Pixelate"]
    Pixelate,
}

impl RenderStyle {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> RenderStyle {
        match v {
            1 => RenderStyle::Toon,
            2 => RenderStyle::Outline,
            3 => RenderStyle::Halftone,
            4 => RenderStyle::Dither,
            5 => RenderStyle::Pixelate,
            _ => RenderStyle::None,
        }
    }
}

/// Colour palette (1-D LUT) the strand/field sweep reads. `Native` reproduces the
/// current per-mode look (HSV sweep on Swept Tubes, RGB-cube colour elsewhere);
/// any explicit palette applies its LUT across **all** surface modes, replacing
/// the RGB-cube colouring. The non-Native palettes are Inigo-Quilez cosine
/// gradients (compact + smooth); `Spectrum` keeps the exact HSV wheel.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum Palette {
    #[name = "Native (HSV / RGB)"]
    Native,
    #[name = "Spectrum (HSV)"]
    Spectrum,
    #[name = "Coral Reef"]
    CoralReef,
    #[name = "Deep Sea"]
    DeepSea,
    #[name = "Anemone"]
    Anemone,
    #[name = "Jellyfish"]
    Jellyfish,
    #[name = "Nautilus"]
    Nautilus,
    #[name = "Kelp"]
    Kelp,
    #[name = "Bioluminescence"]
    Bioluminescence,
    #[name = "Flesh"]
    Flesh,
    #[name = "Candy"]
    Candy,
    #[name = "Plasma"]
    Plasma,
    #[name = "Neon"]
    Neon,
}

impl Palette {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> Palette {
        match v {
            1 => Palette::Spectrum,
            2 => Palette::CoralReef,
            3 => Palette::DeepSea,
            4 => Palette::Anemone,
            5 => Palette::Jellyfish,
            6 => Palette::Nautilus,
            7 => Palette::Kelp,
            8 => Palette::Bioluminescence,
            9 => Palette::Flesh,
            10 => Palette::Candy,
            11 => Palette::Plasma,
            12 => Palette::Neon,
            _ => Palette::Native,
        }
    }
}

/// SDR tone-mapping operator (the [0,∞)→[0,1] curve used when HDR output is off
/// or the display has no EDR headroom). In HDR mode the headroom shoulder is used
/// instead, so this picks the "look" of the SDR fallback / non-HDR displays.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum ToneMap {
    #[name = "ACES"]
    Aces,
    #[name = "AgX"]
    Agx,
    #[name = "Reinhard"]
    Reinhard,
    /// Khronos **PBR Neutral** (Emmett Lalish): keeps saturated colours saturated
    /// with a smooth luminance roll-off — bright emissive reads as coloured light
    /// (glowing gems), where ACES/Reinhard bleach to white. Replaced the old hard
    /// clip. Best for the vivid emissive/RGB-cube look.
    #[name = "Neutral (PBR)"]
    Neutral,
    /// #174 T3: Stephen Hill's fitted RRT+ODT — a truer ACES than the default
    /// per-channel Narkowicz fit (id 0): saturated bright emissives roll toward
    /// white instead of hue-skewing to neon. Appended (ids are wire-stable).
    #[name = "ACES Fitted"]
    AcesFitted,
}

impl ToneMap {
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> ToneMap {
        match v {
            1 => ToneMap::Agx,
            2 => ToneMap::Reinhard,
            3 => ToneMap::Neutral,
            4 => ToneMap::AcesFitted,
            _ => ToneMap::Aces,
        }
    }
}

/// MSAA sample count for the scene pass. Higher = smoother edges, more GPU cost.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum Msaa {
    #[name = "Off"]
    Off,
    #[name = "2×"]
    X2,
    #[name = "4×"]
    X4,
    #[name = "8×"]
    X8,
}

impl Msaa {
    /// The wgpu sample count (1/2/4/8).
    pub fn samples(self) -> u32 {
        match self {
            Msaa::Off => 1,
            Msaa::X2 => 2,
            Msaa::X4 => 4,
            Msaa::X8 => 8,
        }
    }
    pub fn from_samples(n: u32) -> Msaa {
        match n {
            2 => Msaa::X2,
            4 => Msaa::X4,
            8 => Msaa::X8,
            _ => Msaa::Off,
        }
    }
}

/// What drives the pulse envelope (routing slots + the exposure/glow pump).
/// `Beat` = the synthetic decaying impulse off the PLL beat clock (the original
/// behaviour). `Audio` = the live bass-band envelope from the input analysis, so
/// the visual pulses with whatever music is on the track / in the room.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum PulseSource {
    #[name = "Beat Clock"]
    Beat,
    #[name = "Audio (Bass)"]
    Audio,
}

impl PulseSource {
    pub fn to_u32(self) -> u32 {
        match self {
            PulseSource::Beat => 0,
            PulseSource::Audio => 1,
        }
    }
    pub fn from_u32(v: u32) -> PulseSource {
        match v {
            1 => PulseSource::Audio,
            _ => PulseSource::Beat,
        }
    }
}

#[derive(Params)]
pub struct OrganicMathParams {
    /// Size/zoom of the editor (slider) window, persisted with the project.
    /// Key is suffixed `-v2` so projects that saved the old 760-wide size fall
    /// back to the new wide default instead of restoring the cramped layout.
    #[persist = "editor-state-v2"]
    pub editor_state: Arc<EguiState>,

    // --- Generator (which algorithm builds the node field) ---
    /// The generative algorithm. `Original` = the classic cube-field. The editor's
    /// generator column shows only the active generator's controls; everything
    /// downstream (surface/material/look) is shared. See `GeneratorMode`.
    #[id = "gen"] pub generator: EnumParam<GeneratorMode>,

    // --- Loop geometry (Original generator) ---
    #[id = "lcx"] pub loop_count_x: IntParam,
    #[id = "lcy"] pub loop_count_y: IntParam,
    #[id = "lcz"] pub loop_count_z: IntParam,
    #[id = "lcq"] pub loop_count_q: IntParam,

    // --- Rotation (`rot_mod_*` are now per-axis rotation SPEED, not an offset) ---
    #[id = "rfn"] pub rot_func: EnumParam<HostFuncName>,
    #[id = "rax"] pub rot_amp_x: FloatParam,
    #[id = "ray"] pub rot_amp_y: FloatParam,
    #[id = "raz"] pub rot_amp_z: FloatParam,
    #[id = "rmx"] pub rot_mod_x: FloatParam,
    #[id = "rmy"] pub rot_mod_y: FloatParam,
    #[id = "rmz"] pub rot_mod_z: FloatParam,
    /// Off = pendulum (rotation oscillates via `sin`); on = continuous (rotation
    /// winds monotonically and keeps flowing forward).
    #[id = "cont"] pub continuous: BoolParam,
    /// Continuous-mode wave depth: how strongly the rotation func shapes the
    /// *winding velocity* (always forward). 0 = constant spin (the default look);
    /// up = the waveform's character — sine breathes, triangle ramps, square
    /// gear-shifts, saw revs. No effect in pendulum mode.
    #[id = "cshp"] pub cont_shape: FloatParam,

    // --- Translation ---
    #[id = "tfn"] pub trans_func: EnumParam<HostFuncName>,
    #[id = "tax"] pub trans_amp_x: FloatParam,
    #[id = "tay"] pub trans_amp_y: FloatParam,
    #[id = "taz"] pub trans_amp_z: FloatParam,
    #[id = "tmx"] pub trans_mod_x: FloatParam,
    #[id = "tmy"] pub trans_mod_y: FloatParam,
    #[id = "tmz"] pub trans_mod_z: FloatParam,

    // --- Scaling ---
    #[id = "sfn"] pub scale_func: EnumParam<HostFuncName>,
    #[id = "samp"] pub scale_amp: FloatParam,

    // --- Frenet–Serret generator ---
    // Integrate a moving frame from curvature κ(s) and torsion τ(s) along
    // arc-length. κ/τ are each `base + amp·func(freq·s + phase)`; the phase is the
    // global animation clock, so the curve winds/unwinds in time (beat-syncable).
    /// Strands in the bundle (phase-offset copies → a rippling sheet/ribbon set).
    #[id = "fnst"] pub frenet_strands: IntParam,
    /// Samples (frames) per strand.
    #[id = "fnnd"] pub frenet_nodes: IntParam,
    /// Arc-length step ds between samples (curve "size").
    #[id = "fnds"] pub frenet_step: FloatParam,
    /// Waveform shaping the κ/τ modulation (sine = smooth winding).
    #[id = "fnfn"] pub frenet_func: EnumParam<HostFuncName>,
    /// Base curvature κ₀ (constant part). With τ₀ constant → a helix.
    #[id = "fnkb"] pub frenet_kappa: FloatParam,
    /// Curvature modulation amplitude.
    #[id = "fnka"] pub frenet_kappa_amp: FloatParam,
    /// Curvature modulation frequency (cycles along the strand).
    #[id = "fnkf"] pub frenet_kappa_freq: FloatParam,
    /// Base torsion τ₀ (constant part).
    #[id = "fntb"] pub frenet_tau: FloatParam,
    /// Torsion modulation amplitude.
    #[id = "fnta"] pub frenet_tau_amp: FloatParam,
    /// Torsion modulation frequency.
    #[id = "fntf"] pub frenet_tau_freq: FloatParam,
    /// Phase spread between strands (the bundle's fan-out / sheet width).
    #[id = "fnsp"] pub frenet_spread: FloatParam,
    /// Node thickness (cube size / tube radius).
    #[id = "fnth"] pub frenet_thickness: FloatParam,

    // --- DNA double-helix generator ---
    /// Conformation: A / B / Z preset the geometry table; Custom uses the sliders.
    #[id = "dnfm"] pub dna_form: EnumParam<DnaForm>,
    /// Base-pair count (overall length).
    #[id = "dnbp"] pub dna_bp: IntParam,
    /// (Custom) base pairs per turn — the duplex twist rate.
    #[id = "dnpt"] pub dna_bp_per_turn: FloatParam,
    /// (Custom) rise per base pair (Å).
    #[id = "dnri"] pub dna_rise: FloatParam,
    /// (Custom) helix radius (Å ≈ diameter / 2).
    #[id = "dnrd"] pub dna_radius: FloatParam,
    /// (Custom) groove asymmetry Δ (degrees) — the offset of the 2nd backbone that
    /// splits the circle into major + minor grooves (180° = symmetric, no grooves).
    #[id = "dngv"] pub dna_groove: FloatParam,
    /// (Custom) left-handed duplex (Z-DNA is left-handed).
    #[id = "dnlh"] pub dna_left: BoolParam,
    /// Superhelical density σ (negative = the biological default). Drives writhe via
    /// L = T + W: |σ| up coils the spine into a superhelix.
    #[id = "dnsg"] pub dna_sigma: FloatParam,
    /// Superhelix (writhe) radius (Å) — how wide the spine coils when supercoiled.
    #[id = "dnsr"] pub dna_super_radius: FloatParam,
    /// Sequence seed → deterministic ACGT (rung colour: A–T vs G–C).
    #[id = "dnsd"] pub dna_seed: IntParam,
    /// Node thickness (backbone tube / bead size).
    #[id = "dnth"] pub dna_thickness: FloatParam,
    /// Twist-breathe amplitude (turns): animates twist↔writhe at fixed L off the
    /// global Speed clock — the duplex tightens as the superhelix relaxes.
    #[id = "dntb"] pub dna_twist_breathe: FloatParam,

    // --- Strange-attractor generator ---
    /// Which chaotic field to integrate.
    #[id = "atfd"] pub attr_field: EnumParam<AttractorField>,
    /// Number of seed trajectories (→ strand count).
    #[id = "atsn"] pub attr_seeds: IntParam,
    /// Seed value — deterministic seed-point offsets (reproducible presets).
    #[id = "atsd"] pub attr_seed: IntParam,
    /// Seed spread — how far the seeds start apart (chaos fans them out).
    #[id = "atsp"] pub attr_spread: FloatParam,
    /// Integration step multiplier (× the per-field default dt) — trajectory
    /// sampling / size.
    #[id = "atdt"] pub attr_dt: FloatParam,
    /// Trail length (frames per strand — the visible streamline window).
    #[id = "attl"] pub attr_trail: IntParam,
    /// Head speed: how fast the trail flows along the trajectory (× global Speed).
    #[id = "aths"] pub attr_speed: FloatParam,
    /// Display scale multiplier (× the per-field normalization).
    #[id = "atsc"] pub attr_scale: FloatParam,
    /// Node thickness (tube radius / bead size).
    #[id = "atth"] pub attr_thickness: FloatParam,

    // --- Boids / flocking generator (#52) ---
    /// Agent count (→ strand count).
    #[id = "bdct"] pub boids_count: IntParam,
    /// Neighbour perception radius (sim units).
    #[id = "bdpc"] pub boids_perception: FloatParam,
    /// Desired minimum spacing — agents closer than this push apart.
    #[id = "bdsd"] pub boids_separation: FloatParam,
    /// Separation weight (anti-collision).
    #[id = "bdsw"] pub boids_sep: FloatParam,
    /// Alignment weight (match neighbours' heading).
    #[id = "bdal"] pub boids_align: FloatParam,
    /// Cohesion weight (steer toward the local centre).
    #[id = "bdco"] pub boids_cohere: FloatParam,
    /// Max speed (sim units/s) — the velocity clamp.
    #[id = "bdms"] pub boids_max_speed: FloatParam,
    /// Max steering force (acceleration cap) — higher = tighter manoeuvres.
    #[id = "bdmf"] pub boids_max_force: FloatParam,
    /// Trail length (frames per strand — the visible streamline window).
    #[id = "bdtl"] pub boids_trail: IntParam,
    /// Bounding-sphere radius — agents are softly herded back inside it.
    #[id = "bdbn"] pub boids_bounds: FloatParam,
    /// Goal attractor (origin) pull. Beat-pulsed when Pulse is on → gather/scatter.
    #[id = "bdgl"] pub boids_goal: FloatParam,
    /// Node thickness (tube radius / bead size).
    #[id = "bdth"] pub boids_thickness: FloatParam,
    /// Seed — deterministic initial flock (reproducible presets).
    #[id = "bdse"] pub boids_seed: IntParam,
    /// Simulation speed (× global Speed) — how fast the flock evolves.
    #[id = "bdvs"] pub boids_speed: FloatParam,
    /// Display scale multiplier (sim units → world).
    #[id = "bdds"] pub boids_scale: FloatParam,
    /// Creature form — overrides the surface mode with a fish/bird/… mesh per agent.
    #[id = "bdfm"] pub boids_form: EnumParam<BoidsForm>,
    /// Creature world size (length).
    #[id = "bdsz"] pub boids_size: FloatParam,
    /// Banking: how hard creatures roll into their turns (0 = upright).
    #[id = "bdbk"] pub boids_bank: FloatParam,

    // --- Spherical-harmonic generator ---
    // Three mode slots; each `mode` is a curated Yₗᵐ (0..15), `amp` its weight,
    // `freq` its pulse rate (× global Speed). 0 Y₀₀, 4 Y₂₀, 6 Y₂₂, 8 Y₃₀, … (see
    // `math::real_sh`). Displacement = Σ ampₖ·cos(freqₖ·phase)·Yₗᵐ.
    #[id = "hm0m"] pub harm_mode0: IntParam,
    #[id = "hm0a"] pub harm_amp0: FloatParam,
    #[id = "hm0f"] pub harm_freq0: FloatParam,
    #[id = "hm1m"] pub harm_mode1: IntParam,
    #[id = "hm1a"] pub harm_amp1: FloatParam,
    #[id = "hm1f"] pub harm_freq1: FloatParam,
    #[id = "hm2m"] pub harm_mode2: IntParam,
    #[id = "hm2a"] pub harm_amp2: FloatParam,
    #[id = "hm2f"] pub harm_freq2: FloatParam,
    /// Base sphere radius.
    #[id = "hmrd"] pub harm_radius: FloatParam,
    /// θ (polar) and φ (azimuth) grid resolution.
    #[id = "hmtr"] pub harm_theta: IntParam,
    #[id = "hmpr"] pub harm_phi: IntParam,
    /// Node thickness (tube radius / bead size).
    #[id = "hmth"] pub harm_thickness: FloatParam,
    // --- Soft-body bell (#99): a physical mode on the harmonic generator ---
    /// Physical mode — run the XPBD soft-body bell instead of the closed-form sum
    /// (the bell genuinely contracts + recoils; the stroke fires on the beat).
    #[id = "blph"] pub bell_physical: BoolParam,
    /// Stroke depth — how far a full contraction shrinks the bell's hoops.
    #[id = "blsd"] pub bell_stroke_depth: FloatParam,
    /// Stiffness — constraint solver iterations per sub-step.
    #[id = "blst"] pub bell_stiffness: IntParam,
    /// Damping — per-step velocity retention (lower = the bell settles faster).
    #[id = "bldp"] pub bell_damping: FloatParam,
    /// Bell openness — the rim's polar angle θ_max (how flared the bell is).
    #[id = "blop"] pub bell_open: FloatParam,
    /// Stroke rate — contraction pulses per bar (beat-paced). Lower = slower, flowier.
    #[id = "blsp"] pub bell_speed: FloatParam,

    // --- L-system / 3D turtle generator ---
    /// Built-in rule set (Fern / Bush / Tree / Seaweed).
    #[id = "lssy"] pub ls_system: EnumParam<LSystem>,
    /// Rewrite depth (iterations). Node count explodes with depth — capped at 7.
    #[id = "lsdp"] pub ls_depth: IntParam,
    /// Turn angle (degrees) per +/−/&/^ command.
    #[id = "lsan"] pub ls_angle: FloatParam,
    /// Segment length per F (overall size).
    #[id = "lsst"] pub ls_step: FloatParam,
    /// Sway amplitude (degrees, added to the turn) — animates off the global Speed.
    #[id = "lssa"] pub ls_sway_amp: FloatParam,
    /// Sway frequency (× the global Speed clock).
    #[id = "lssf"] pub ls_sway_freq: FloatParam,
    /// Growth-front fraction (0..1): draw only the first part of the plant (unfurl).
    #[id = "lsgr"] pub ls_grow: FloatParam,
    /// Node thickness (tube radius / bead size).
    #[id = "lsth"] pub ls_thickness: FloatParam,

    // --- Curl-noise flow-field generator ---
    /// Number of advected particles (→ streamline strands).
    #[id = "cnsn"] pub cn_seeds: IntParam,
    /// Seed value — deterministic seed-point placement + noise (reproducible).
    #[id = "cnsd"] pub cn_seed: IntParam,
    /// Seed spread (how far apart the particles start).
    #[id = "cnsp"] pub cn_spread: FloatParam,
    /// Noise field frequency (higher = finer, more turbulent swirls).
    #[id = "cnsc"] pub cn_scale: FloatParam,
    /// Steps (frames) per streamline.
    #[id = "cnst"] pub cn_steps: IntParam,
    /// Integration step dt (streamline length / smoothness).
    #[id = "cndt"] pub cn_dt: FloatParam,
    /// Flow speed: evolves the noise in time (× the global Speed clock).
    #[id = "cnfl"] pub cn_flow: FloatParam,
    /// Containment: a linear pull toward the origin (0 = free flow).
    #[id = "cnbd"] pub cn_bound: FloatParam,
    /// Node thickness (tube radius / bead size).
    #[id = "cnth"] pub cn_thickness: FloatParam,

    // --- Circular-polarization radiation-field generator ---
    /// θ rings: how many ray latitudes across the cone (1 = a single ring/axis).
    #[id = "plrn"] pub pol_rings: IntParam,
    /// φ spokes: rays around each ring (1 = a lone corkscrew; many = the eye).
    #[id = "plsk"] pub pol_spokes: IntParam,
    /// Samples per ray (helix resolution).
    #[id = "plsm"] pub pol_samples: IntParam,
    /// Ray length R (how far each ray reaches from the source).
    #[id = "plln"] pub pol_len: FloatParam,
    /// Wavenumber k — turns per unit length (helix tightness).
    #[id = "plk"]  pub pol_k: FloatParam,
    /// Field amplitude A (helix radius).
    #[id = "plam"] pub pol_amp: FloatParam,
    /// Radiation falloff: 0 = constant radius (clean), 1 = true 1/r (trumpet flare).
    #[id = "plfo"] pub pol_falloff: FloatParam,
    /// Handedness — left vs. right circular polarization (sign of the sin term).
    #[id = "plhd"] pub pol_handed: BoolParam,
    /// Spread: cone half-angle (degrees) about +Y — 0 = single axis … 180 = full sphere.
    #[id = "plsp"] pub pol_spread: FloatParam,
    /// Swirl: precession rate of the ray azimuth off the global Speed clock (0 = still).
    #[id = "plsw"] pub pol_swirl: FloatParam,
    /// Also emit the perpendicular B helix per ray (the interleaved double helix).
    #[id = "plsb"] pub pol_show_b: BoolParam,
    /// Node thickness (tube radius / bead size).
    #[id = "plth"] pub pol_thickness: FloatParam,

    // --- Maxwell radiation-field generator ---
    /// Render mode: off = (θ,φ) lattice of E/B-tip strands (Grid); on = field-line
    /// streamlines (Streamlines).
    #[id = "mxln"] pub mx_lines: BoolParam,
    /// Generator **E↔B blend** ∈ [0,1] for the lattice tips / field lines: 0 = pure
    /// E, 1 = pure B, 0.5 = an equal mix (E and B are perpendicular for a dipole, so
    /// the middle is a genuinely different helical direction).
    #[id = "mxgb"] pub mx_gen_blend: FloatParam,
    /// Sources are oscillating dipoles (radiation lobe); off = point charges (Coulomb).
    #[id = "mxdp"] pub mx_dipoles: BoolParam,
    /// Source count (laid out along X; charges alternate sign → a pair is a dipole).
    #[id = "mxsc"] pub mx_sources: IntParam,
    /// Source spacing along X.
    #[id = "mxsp"] pub mx_separation: FloatParam,
    /// Per-source phase offset (radians) — drives interference between sources.
    #[id = "mxph"] pub mx_phase: FloatParam,
    /// Swirl: orbit the source layout about Y off the global Speed clock.
    #[id = "mxsw"] pub mx_swirl: FloatParam,
    /// Near-field blend: 0 = radiation zone only, 1 = + quasi-static near field (1/r³).
    #[id = "mxnf"] pub mx_near: FloatParam,
    /// Wavenumber k = ω/c — sets the retarded space-lag (and radiation phase).
    #[id = "mxk"]  pub mx_k: FloatParam,
    /// Field display amplitude (tip offset / scale).
    #[id = "mxam"] pub mx_amp: FloatParam,
    /// Near-source clamp radius (avoids the 1/r,1/r³ blow-up at a source).
    #[id = "mxrm"] pub mx_rmin: FloatParam,
    /// Node thickness (tube radius / bead size).
    #[id = "mxth"] pub mx_thickness: FloatParam,
    /// θ rings of the sampling lattice (lattice mode).
    #[id = "mxrn"] pub mx_rings: IntParam,
    /// φ spokes of the sampling lattice (lattice mode).
    #[id = "mxsk"] pub mx_spokes: IntParam,
    /// Samples per ray (lattice mode).
    #[id = "mxsm"] pub mx_samples: IntParam,
    /// Ray length R (lattice mode).
    #[id = "mxrl"] pub mx_raylen: FloatParam,
    /// Lattice cone half-angle (degrees) about +Y — 180 = full sphere; up to 360
    /// wraps the fan past the pole for a fuller disc.
    #[id = "mxsd"] pub mx_spread: FloatParam,
    /// Lattice strand look: OFF = displace each strand by the RAW field magnitude
    /// (flowing "wave" strands that undulate with the field strength — the original
    /// look); ON = displace by the UNIT field direction (uniform-length "spoke"
    /// strands). Lattice mode only.
    #[id = "mxuf"] pub mx_norm_field: BoolParam,
    /// Field lines seeded per source (field-line mode).
    #[id = "mxse"] pub mx_seeds: IntParam,
    /// Max integration steps per field line (field-line mode).
    #[id = "mxst"] pub mx_steps: IntParam,
    /// Field-line integration step length ds (field-line mode).
    #[id = "mxds"] pub mx_ds: FloatParam,
    /// Field-line bound: a line stops if it leaves this radius (field-line mode).
    #[id = "mxbd"] pub mx_bound: FloatParam,
    /// **Tempo-sync the Duo-Field oscillation** (shared by the Maxwell **and**
    /// Acoustic generators — both read the same `maxdip_phase` clock). OFF = the field
    /// alternates on the free-running global Speed clock (the historical behaviour — an
    /// arbitrary rate set by Speed). ON = the oscillation becomes an LFO phase-locked to
    /// the beat clock, one full field there-and-back per `mx_osc_div` (host-locked while
    /// the transport plays, else the Manual/Audio BPM), applied centrally so the field
    /// lines, the aura/energy cloud, and — on Maxwell force-drive — the **B swirl** all
    /// reverse together on one clock (the E↔B lock). Takes precedence over the #248
    /// audio-dipole pitch-rate while on.
    #[id = "mxos"] pub mx_osc_sync: BoolParam,
    /// The tempo-synced oscillation period (note division), used only when
    /// `mx_osc_sync` is on. Shared by Maxwell + Acoustic.
    #[id = "mxod"] pub mx_osc_div: EnumParam<OscDivision>,
    /// **E↔B phase** (near↔far induction dial), degrees. Offsets the tempo-locked
    /// **B-swirl** reversal relative to the E oscillation: `osc = cos(ωt − φ)`. **0° =
    /// far-field** (radiation zone — E and B in phase, the E↔B-lock default); **90° =
    /// near-field induction** (the swirl in quadrature — B peaks at E's zero-crossing,
    /// as ∂B/∂t ∝ ∇×E demands near the source). Only bites with Tempo Sync + Maxwell
    /// fluid force-drive on; 0 = the plain in-phase lock.
    #[id = "mxeb"] pub mx_eb_phase: FloatParam,

    // --- FDTD Maxwell solver (#412 Tier 3, Phase 0): a real-time CPU Yee stepper
    //     that marches the curl equations on a grid, so the field propagates
    //     (retardation emergent). A toggle on the Maxwell generator; feeds the
    //     Volume surface's energy cloud. Off by default → analytic path unchanged. ---
    /// Run the FDTD solver instead of the closed-form field (Maxwell generator only).
    #[id = "fdon"] pub fdtd_on: BoolParam,
    /// Grid resolution (cells per axis). Higher = sharper + slower (CPU, Phase 0).
    #[id = "fdrs"] pub fdtd_res: FloatParam,
    /// Source waveform: one-shot Gaussian Pulse vs continuous CW sinusoid.
    #[id = "fdsm"] pub fdtd_source: EnumParam<FdtdSource>,
    /// Source frequency ω (radians per animation-time unit); CW rate / pulse content.
    #[id = "fdfr"] pub fdtd_freq: FloatParam,
    /// Source drive amplitude.
    #[id = "fddr"] pub fdtd_drive: FloatParam,
    /// CFL sub-steps marched per frame (more = faster wave, steadier at high res).
    #[id = "fdss"] pub fdtd_substeps: FloatParam,
    /// Absorbing-sponge thickness (cells) at the domain walls (0 = reflecting box).
    #[id = "fdbc"] pub fdtd_boundary: FloatParam,
    /// Domain half-extent (world units) — the cubic box the grid spans.
    #[id = "fdex"] pub fdtd_extent: FloatParam,

    // --- Acoustic-field generator (#325, Duo-Field N1) ---
    /// Source multipole: monopole / dipole / quadrupole (of signed harmonic point
    /// monopoles). Dipole shows the figure-8 pressure lobe + its equatorial node.
    #[id = "acsk"] pub ac_source: EnumParam<AcousticSource>,
    /// Wavenumber k = ω/c — sets the retarded space-lag + the radial wavelength.
    #[id = "ack"]  pub ac_k: FloatParam,
    /// Near-field weight (0..1): scales the 1/r² particle-velocity term that is 90°
    /// out of phase with the pressure. 0 = far/radiation field only (p ∥ u).
    #[id = "acnf"] pub ac_near: FloatParam,
    /// Field display amplitude (geometry displacement scale).
    #[id = "acam"] pub ac_amp: FloatParam,
    /// Multipole element spacing (dipole/quadrupole array extent).
    #[id = "acsp"] pub ac_separation: FloatParam,
    /// Near-source clamp radius (avoids the 1/r, 1/r² blow-up at a source).
    #[id = "acrm"] pub ac_rmin: FloatParam,
    /// Geometry **pressure↔velocity blend** ∈ [0,1]: 0 = the pressure shell (radial
    /// breathing displacement), 1 = the velocity flow (a Maxwell-like vector field).
    #[id = "acgb"] pub ac_blend: FloatParam,
    /// Lattice strand look: OFF = displace by the RAW field magnitude (flowing
    /// waves), ON = displace by the UNIT direction (uniform spokes).
    #[id = "acuf"] pub ac_norm_field: BoolParam,
    /// θ rings of the sampling lattice.
    #[id = "acrn"] pub ac_rings: IntParam,
    /// φ spokes of the sampling lattice.
    #[id = "acsx"] pub ac_spokes: IntParam,
    /// Samples per ray.
    #[id = "acsm"] pub ac_samples: IntParam,
    /// Ray length R.
    #[id = "acrl"] pub ac_raylen: FloatParam,
    /// Lattice cone half-angle (degrees) about +Y — 180 = full sphere.
    #[id = "acsd"] pub ac_spread: FloatParam,
    /// Node thickness (tube radius / bead size).
    #[id = "acth"] pub ac_thickness: FloatParam,
    /// Aura **pressure↔velocity blend** ∈ [0,1], independent of the geometry blend:
    /// 1 = the particle-velocity channel (motes advect along `u` — the default Duo
    /// channel), 0 = the pressure radial channel. Drives BOTH the aura's motion AND
    /// its glow energy density together.
    #[id = "acab"] pub ac_aura_blend: FloatParam,
    /// Acoustic **beat pump** (#325 Tier 3): on each beat the source amplitude swells
    /// (a "speaker pushing air"), so the pressure shell + energy cloud punch with the
    /// music. Needs **Pulse** on + audio drive. 0 = off (inert).
    #[id = "acbp"] pub ac_beat_pump: FloatParam,

    // --- #381 Tier 1 Field Engine (arbitrary closed-form field equations) ---
    /// Render kind selector (Auto infers from the compiled program).
    #[id = "fekd"] pub field_kind: EnumParam<FieldKind>,
    /// Phenomenon Gallery preset (Coulomb … Gaussian, or Custom = sidecar program).
    #[id = "fepr"] pub field_preset: EnumParam<FieldPreset>,
    /// Domain scale `k` — spatial-frequency multiplier applied to the sample
    /// position before evaluation (zoom the field in/out).
    #[id = "fesc"] pub field_scale: FloatParam,
    /// Box half-extent — the ±extent cube the field is sampled/traced inside.
    #[id = "feex"] pub field_extent: FloatParam,
    /// Live coefficient `a` (host-mappable / automatable) — bound to the program
    /// variable `a`. Every gallery program uses it as its primary strength/amplitude.
    #[id = "fea"] pub field_a: FloatParam,
    /// Live coefficient `b` (host-mappable / automatable) — bound to the program
    /// variable `b` (e.g. plane-wave ω, Gaussian σ).
    #[id = "feb"] pub field_b: FloatParam,
    /// Field-line seeds (vector) / lattice resolution driver (scalar/complex).
    #[id = "fedn"] pub field_density: IntParam,
    /// Display amplitude / glyph-length gain.
    #[id = "fegn"] pub field_gain: FloatParam,
    /// Line / marker thickness (tube radius).
    #[id = "feth"] pub field_thickness: FloatParam,
    // --- #381 Tier 3 Field Engine (time-marched PDE dynamics) ---
    /// PDE preset: `Off` = the static Tier-1/2 field; else march a live grid sim.
    #[id = "pdpr"] pub pde_preset: EnumParam<PdePreset>,
    /// Diffusion `D` (Heat) / wave speed `c` (Wave) / kinetic coefficient (Schrödinger).
    #[id = "pdif"] pub sim_diffusion: FloatParam,
    /// Sim time-scale: multiplies the per-frame beat advance to set how fast the sim
    /// marches (the sim is CFL-substepped so this can't destabilize it).
    #[id = "pdts"] pub sim_time_scale: FloatParam,
    /// Gray–Scott feed rate `F`.
    #[id = "pdfd"] pub sim_feed: FloatParam,
    /// Gray–Scott kill rate `k`.
    #[id = "pdkl"] pub sim_kill: FloatParam,
    /// Schrödinger harmonic-trap strength `V`.
    #[id = "pdpt"] pub sim_potential: FloatParam,
    /// Audio/source forcing amplitude — stamps a Gaussian source at the grid centre.
    #[id = "pdfc"] pub sim_forcing: FloatParam,
    /// Sim grid resolution (per axis). Perf dial; a change reseeds the sim.
    #[id = "pdrs"] pub sim_res: IntParam,
    // --- Density-Map Attractor (#380 Tier 1) ---
    /// Which discrete iterated map to run (Tier 1: the holomorphic Complexus seed).
    #[id = "makd"] pub ma_kind: EnumParam<MapKindParam>,
    /// Map parameter `a` (the additive constant inside the `sin` branch).
    #[id = "maa"]  pub ma_a: FloatParam,
    /// Map parameter `b` (the additive constant inside the `cos` branch).
    #[id = "mab"]  pub ma_b: FloatParam,
    /// Map parameter `c` (#380 Tier 3) — the third coefficient used by Clifford /
    /// de Jong / Pickover; inert for Complexus / Gumowski–Mira. Static this tier
    /// (the beat orbit only walks `a`/`b`).
    #[id = "mac"]  pub ma_c: FloatParam,
    /// Map parameter `d` (#380 Tier 3) — the fourth coefficient (Clifford / de Jong /
    /// Pickover); inert for the maps that don't read it. Static this tier.
    #[id = "madp"] pub ma_d: FloatParam,
    /// Colour-by-dynamics mode (#380 Tier 3): how the per-splat tint is derived —
    /// **Step Speed |Δ|** (default → byte-identical) / **Iteration Index** / **Jacobian
    /// Stretch** (local-chaos proxy).
    #[id = "macol"] pub ma_color: EnumParam<MapColorParam>,
    /// Points emitted per frame, in thousands (K). Each point is an additive splat /
    /// emissive marker; overlapping points accumulate density → the glow.
    #[id = "mapt"] pub ma_points_k: IntParam,
    /// Warm-up iterations discarded per restart orbit (the transient before the
    /// invariant set).
    #[id = "mawu"] pub ma_warmup: IntParam,
    /// World half-extent the map's `[-1,1]` box is scaled to (cube-field scale).
    #[id = "masc"] pub ma_scale: FloatParam,
    /// Per-point marker size (world units).
    #[id = "masz"] pub ma_size: FloatParam,
    /// Emissive brightness multiplier of the per-point tint (HDR "fire" gain).
    #[id = "main"] pub ma_intensity: FloatParam,
    /// Animation → parameter `a` drive (0..1): how much the animation clock (`gen_phase`)
    /// sweeps `a`. 0 = static `a` (the Tier-1 default), 1 = full-rate sweep. Independent
    /// of `b`, so unequal drives trace a Lissajous path through (a,b) parameter space →
    /// the pattern morphs on its own.
    #[id = "maad"] pub ma_a_drive: FloatParam,
    /// Animation → parameter `b` drive (0..1): the same, independently, for `b`.
    #[id = "mabd"] pub ma_b_drive: FloatParam,
    // --- Density-Map Attractor parameter orbit (#380 Tier 2) ---
    /// How `(a, b)` are driven: **Off** (static) / **Linear** (the Tier-1 ramp;
    /// default → byte-identical) / **Lissajous** (the closed, beat-locked loop).
    #[id = "maom"] pub ma_orbit: EnumParam<MapOrbitModeParam>,
    /// Lissajous loop length in **beats** — one full seamless loop per this many
    /// beats of the PLL beat clock ("two days → one bar"). Free-runs on `gen_phase`
    /// when the host isn't playing.
    #[id = "malb"] pub ma_loop_beats: FloatParam,
    /// Lissajous radius on `a` (`Ra`): half-extent of the swept box on the `a` axis.
    #[id = "mara"] pub ma_orbit_ra: FloatParam,
    /// Lissajous radius on `b` (`Rb`).
    #[id = "marb"] pub ma_orbit_rb: FloatParam,
    /// Lissajous frequency ratio on `a` (`fa`, integer → the loop closes seamlessly).
    #[id = "mafa"] pub ma_orbit_fa: IntParam,
    /// Lissajous frequency ratio on `b` (`fb`, integer). `fa`≠`fb` traces a figure.
    #[id = "mafb"] pub ma_orbit_fb: IntParam,
    /// Lissajous phase offset `ψ` on `b` (radians): 0 = a line, π/2 = an ellipse.
    #[id = "maps"] pub ma_orbit_psi: FloatParam,
    /// Free-run rate (loops per `gen_phase` unit) used when the host isn't playing.
    #[id = "mafr"] pub ma_orbit_free: FloatParam,

    // --- #339 Duo-Field synthesis — the "Sound" card (Tier 1: field probes) ---
    /// Master synth enable. **Off → the passthrough is byte-identical** (the audio
    /// version of the repo's "off = byte-identical" contract).
    #[id = "snon"] pub sn_on: BoolParam,
    /// Play mode: Generative (self-contained) / Instrument (MIDI-played) / Duet.
    #[id = "snpm"] pub sn_play_mode: EnumParam<SynthPlayMode>,
    /// Master synth gain (linear) applied before the soft-knee limiter.
    #[id = "sngn"] pub sn_gain: FloatParam,
    /// Wet level — how much of the synth bus is summed over the passthrough.
    #[id = "snmx"] pub sn_mix: FloatParam,
    /// Field `k` → pitch tuning (Hz per unit k): the bed's pitch = the active field
    /// generator's wavenumber `k` × this, so the generator's own `k` slider IS the
    /// pitch control (the honest ω = c·k mapping). The generative bed always follows
    /// the Acoustic / Maxwell generator's own source / separation / near / clamp —
    /// there are no duplicate synth-side field controls.
    #[id = "sntu"] pub sn_tuning: FloatParam,
    /// Generative bed amplitude (0 mutes the bed; voices still sound in Instrument).
    #[id = "snga"] pub sn_gen_amp: FloatParam,
    // --- #339 Tier 2: the oscillator lattice ---
    /// Engine mode: Probes (Tier 1 field mics) or Lattice (Tier 2 additive bank).
    #[id = "snmd"] pub sn_mode: EnumParam<SynthMode>,
    /// Oscillators in the lattice bank (each anchored to a shell node).
    #[id = "snbk"] pub sn_bank: IntParam,
    /// Lattice tuning layout (Octaves / Harmonic / Stretched / Geometric).
    #[id = "sntl"] pub sn_tuning_layout: EnumParam<TuningLayout>,
    /// Tuning spread — octaves-per-step (Octaves) / ratio (Geometric).
    #[id = "snts"] pub sn_tune_spread: FloatParam,
    /// Inharmonicity `B` for the Stretched layout (0 = harmonic).
    #[id = "sntk"] pub sn_tune_stretch: FloatParam,
    /// Radius of the sampling shell the lattice nodes sit on (world units).
    #[id = "snshr"] pub sn_shell_r: FloatParam,
    /// Field breathing rate (Hz) — the slow clock modulating each oscillator's
    /// amplitude from its node's local energy.
    #[id = "snshrt"] pub sn_shell_rate: FloatParam,
    // --- #339 Tier 3: struck cavities (modal synthesis) ---
    /// Modal decay time (`-60 dB`, seconds) — how long the struck cavity rings.
    #[id = "snt60"] pub sn_t60: FloatParam,
    /// Mallet brightness (0 = soft, low modes only; 1 = hard, all modes ring).
    #[id = "snbrt"] pub sn_bright: FloatParam,
    // --- #339 Tier 4: granular aura ---
    /// Grain length (seconds) for the granular field aura.
    #[id = "sngz"] pub sn_grain_size: FloatParam,
    /// Grain density (0..1) — how thickly grains are sprayed (× the field flux).
    #[id = "sngd"] pub sn_grain_density: FloatParam,
    /// ADSR — attack (s).
    #[id = "snat"] pub sn_attack: FloatParam,
    /// ADSR — decay (s).
    #[id = "snde"] pub sn_decay: FloatParam,
    /// ADSR — sustain level (0..1).
    #[id = "snsu"] pub sn_sustain: FloatParam,
    /// ADSR — release (s); the source keeps radiating while it decays.
    #[id = "snrl"] pub sn_release: FloatParam,
    /// Portamento / glide time (s) — a note glide is an audible pitch slide (and a
    /// visible wavelength morph). 0 = snap.
    #[id = "sngl"] pub sn_glide: FloatParam,
    /// Pitch-bend range (± semitones).
    #[id = "snbr"] pub sn_bend_range: FloatParam,
    /// Keyboard→X placement spread (0 = voices stacked at the origin; up = bass
    /// left / treble right, so the stereo image is literally the keyboard).
    #[id = "snps"] pub sn_place_spread: FloatParam,
    /// Concert-A tuning reference (Hz).
    #[id = "sna4"] pub sn_a4: FloatParam,
    /// Listener probe L (the left microphone) position.
    #[id = "snlx"] pub sn_probe_lx: FloatParam,
    #[id = "snly"] pub sn_probe_ly: FloatParam,
    #[id = "snlz"] pub sn_probe_lz: FloatParam,
    /// Listener probe R (the right microphone) position; its spacing from L sets
    /// the interaural time difference (real ITD, not a pan pot).
    #[id = "snrx"] pub sn_probe_rx: FloatParam,
    #[id = "snry"] pub sn_probe_ry: FloatParam,
    #[id = "snrz"] pub sn_probe_rz: FloatParam,
    /// Whether probe 0 (L) rides the camera (visual gizmo hint; consumed by the
    /// visual). Off = the probes stay fixed in the field.
    #[id = "sncm"] pub sn_probe_cam: BoolParam,
    // --- visual lens (the picture renders at an octave offset of the sound) ---
    /// Lens fixed-point note frequency (Hz) — renders "at scale" here.
    #[id = "snvp"] pub sn_vis_pivot: FloatParam,
    /// Lens visual oscillation rate (Hz) at the pivot.
    #[id = "snva"] pub sn_vis_anchor: FloatParam,
    /// Time-lens slope: 1 = proportional octave shift, 0 = one visual rate for the
    /// whole keyboard, 1/3 = nine audio octaves fold into three visual ones.
    #[id = "snvs"] pub sn_vis_slope: FloatParam,
    /// Space-lens (wavenumber) anchor at the pivot.
    #[id = "snka"] pub sn_vis_k_anchor: FloatParam,
    /// Space-lens slope (0 = every note shares one visible wavelength).
    #[id = "snks"] pub sn_vis_k_slope: FloatParam,
    /// Lens quantize: Free / Octave-locked / Beat-locked.
    #[id = "snvq"] pub sn_vis_quantize: EnumParam<SynthQuantize>,
    // --- Acoustic Tier 4 (#325): cavity modes + intensity flux ---
    /// Source model: Radiating multipole (Tiers 1–3) or a bounded rectangular
    /// **cavity** standing-wave eigenmode (Chladni nodal patterns).
    #[id = "acmd"] pub ac2_model: EnumParam<AcousticModel>,
    /// Cavity mode number along X (nodal planes per axis). Cavity model only.
    #[id = "acnx"] pub ac2_nx: IntParam,
    /// Cavity mode number along Y.
    #[id = "acny"] pub ac2_ny: IntParam,
    /// Cavity mode number along Z.
    #[id = "acnz"] pub ac2_nz: IntParam,
    /// Cavity **beat morph** (0..1): on the beat, sweep the modes so the Chladni
    /// pattern reorganises. 0 = static modes.
    #[id = "acmo"] pub ac2_morph: FloatParam,
    /// Cavity box half-extent (world units) — sets the mode wavelengths.
    #[id = "accs"] pub ac2_cav_scale: FloatParam,
    /// **Intensity flux** (#325 Tier 4, the tri-field): 0 = the compression↔transverse
    /// blend; > 0 = the aura advects motes along the acoustic intensity `I = p·u`
    /// (the direction sound energy flows), glowing by `|p·u|`. Amount scales the glow.
    #[id = "acin"] pub ac2_intensity: FloatParam,

    // --- Acoustic Tier 5 (#325): cavity 3-D tween + per-axis audio breathe ---
    /// Cavity **mode tween** (0..1): soften the beat mode-walk. 0 = hard cut (the
    /// nodal pattern jumps on each beat); up = it HOLDS then glides between mode sets,
    /// so the Chladni figure reorganises smoothly on the beat instead of snapping.
    #[id = "actw"] pub ac2_tween: FloatParam,
    /// Audio → cavity mode **X** (0..8): with the audio drive on, the broadband level
    /// lifts the X mode number — louder music packs more nodal planes along X. 0 = off.
    #[id = "acbx"] pub ac2_audio_x: FloatParam,
    /// Audio → cavity mode **Y** (0..8). Independent per-axis breathe.
    #[id = "acby"] pub ac2_audio_y: FloatParam,
    /// Audio → cavity mode **Z** (0..8). Independent per-axis breathe.
    #[id = "acbz"] pub ac2_audio_z: FloatParam,

    // --- Maxwell field energization (#247, Tier 1) ---
    /// Light the Particle Aura by the field's real **energy density** `½(|E|²+|B|²)` —
    /// the fluorescent-tube-near-an-antenna demo. Motes still advect along the field
    /// direction (as now) but glow by the local magnitude the advector normalized away.
    /// Needs the generator = Maxwell + Particle Aura on the Lite tier. 0 = off (inert).
    #[id = "mnen"] pub mn_energize: BoolParam,
    /// Energy brightness gain (× the tone-mapped energy). The overall glow.
    #[id = "mngn"] pub mn_gain: FloatParam,
    /// Tone-map soft knee — the HDR ceiling the near-field energy rolls off toward, so
    /// the 1/r⁶ near-source spike blooms without flattening into one solid ball.
    #[id = "mnkn"] pub mn_knee: FloatParam,
    /// Base ember hue (0..1 around the wheel) — low-energy motes take this colour,
    /// high-energy motes desaturate toward white-hot.
    #[id = "mnhu"] pub mn_hue: FloatParam,
    /// Finite-antenna source (#247 Tier 2): model the field as a driven rod on the Z
    /// axis carrying the standing-wave current `I(z)=I₀·sin(k(L/2−|z|))` instead of the
    /// idealized point dipole — the near-field bound charge concentrates at the tips, so
    /// the energy cloud shows the literal **bright-ends / dim-centre** fluorescent-tube
    /// pattern. Off = the Tier 1 point dipole. (Energizes only with the Aura on.)
    #[id = "mnan"] pub mn_antenna: BoolParam,
    /// Antenna length L (world units, along Z). Pairs with the wavenumber k: `kL/2 = π/2`
    /// is a half-wave rod (one bright band per tip); longer L adds standing-wave nodes.
    #[id = "mnal"] pub mn_antenna_len: FloatParam,
    /// Fluid dye injection (#247 Tier 3): when the **Fluid Ink** is on, energized nodes
    /// inject **bright dye by the local field energy** (same tone-map as the mote glow,
    /// tinted by the ember hue) into the Navier–Stokes field, so the glow **advects and
    /// swirls** — energy visibly flowing through the field. 0 = off (plain node colour).
    #[id = "mndi"] pub mn_dye_inject: FloatParam,
    /// Aura **E↔B blend** ∈ [0,1], independent of the generator's blend: 0 = pure E,
    /// 1 = pure B, 0.5 = an equal mix. Drives BOTH the aura's motion AND its glow /
    /// lighting together, so the motes can flow along (and the cloud light up by) a
    /// different field/mix than the generator arrows draw.
    #[id = "mxab"] pub mx_aura_blend: FloatParam,
    /// Field-force drive (#248 particles): drive the aura/fluid with the E field as a
    /// real body FORCE (magnitude + sign) instead of following field lines at constant
    /// speed. The medium is pushed by the force — strong near the core, sloshing back
    /// and forth as the dipole oscillates. On the **Fluid** tier the incompressible
    /// solve absorbs the static/conservative field (no stir unless it's oscillating);
    /// the **Lite** tier advects motes by the force directly. Maxwell generator only.
    #[id = "mnfd"] pub mn_force: BoolParam,
    /// Force strength (× the soft-capped E force). Higher = the medium stirs harder.
    #[id = "mnfg"] pub mn_force_gain: FloatParam,
    /// Energization core contrast: >1 sharpens the glow toward the high-energy core
    /// (undoing the tone-map's flattening of the 1/r⁶ gradient); 1 = current look.
    #[id = "mnec"] pub mn_energy_contrast: FloatParam,
    /// Force-drive stir rate (Hz): how fast the fluid swirl reverses back and forth.
    /// A fluid low-passes the raw field-clock oscillation into a steady flow, so this
    /// declared slow rate is what reads as sloshing. 0 = a steady (non-reversing) swirl.
    #[id = "mnsr"] pub mn_stir_rate: FloatParam,
    /// Acoustic **pump** (beat-driven): on each beat the dipole expands in/out along its
    /// axis — a speaker pushing air — imparting a punchy near-term velocity impulse (not a
    /// constant jet). Needs **Pulse** on; 0 = off.
    #[id = "mnpm"] pub mn_pump: FloatParam,
    /// Beat → swirl **spin force**: each beat kicks the swirl's angular momentum in ONE
    /// direction (spinning a turbine), which then coasts down between beats (see `stir
    /// slowdown`). 0 = the manual `stir rate` reversal instead. Needs **Pulse** on.
    #[id = "mnsb"] pub mn_swirl_beat: FloatParam,
    /// Pump core size (world units): how far from the source the axial pump reaches.
    #[id = "mnps"] pub mn_pump_scale: FloatParam,
    /// Beat-swirl **slowdown** (1/s): how fast the turbine's spin decays between beats.
    /// Low = long coast (momentum carries); high = it stops quickly after each kick.
    #[id = "mnsd"] pub mn_swirl_decay: FloatParam,
    /// **Beat mode** crossfade (−1..+1): blends the two beat engines. **−1** = the
    /// turbine + independent pump (spin-up/coast); **+1** = the coupled E↔B dynamo (a
    /// struck cavity ringing energy between pump and swirl); **0** = an even blend of
    /// both. Both run continuously, so the crossfade is smooth end to end.
    #[id = "mnmm"] pub mn_mode_mix: FloatParam,
    /// Dynamo **ring frequency** (Hz): how fast energy sloshes E↔B (pump↔swirl). Higher =
    /// a faster ring; pairs with `spin slowdown` (the ring-down) for the decay envelope.
    #[id = "mnrf"] pub mn_ring_freq: FloatParam,
    /// **Hue cycle**: each beat pulse advances the energized motes' colour around the hue
    /// wheel by this much — so the vortex pulses through the palette with the music. 0 =
    /// off (the fixed ember hue). Needs **Pulse** on + energization.
    #[id = "mnhc"] pub mn_hue_cycle: FloatParam,

    // --- Audio-driven dipole radiation (#248, Tier 1) ---
    /// A speaker is an acoustic dipole — drive ours from the live music. With this
    /// on, the broadband loudness envelope (the analyzer's smoothed RMS, needs
    /// **Audio Reactive** on) scales the Maxwell generator's **drive amplitude**:
    /// E and B scale linearly with the drive, so the #247 energy cloud
    /// `½(|E|²+|B|²)` breathes **quadratically** with the music's dynamics. Honest
    /// and declared: the audio modulates the SOURCE's parameters; the rendered
    /// field math stays the real retarded dipole radiation (we never render the
    /// 20 Hz–20 kHz carrier — unwatchable, off-scale wavelength). Off = inert.
    #[id = "addr"] pub ad_drive: BoolParam,
    /// RMS → drive gain: how hard the loudness envelope pushes the dipole
    /// (drive = floor + amount·RMS). 1 ≈ unity for a full-scale signal.
    #[id = "adam"] pub ad_amount: FloatParam,
    /// Idle drive floor on silence (0..1): 0 = the field goes fully dark between
    /// notes; 0.1 keeps a dim ember of the dipole visible.
    #[id = "adfl"] pub ad_floor: FloatParam,
    /// Spectrum → multipole content (#248 Tier 2): each FFT band drives a distinct
    /// **multipole moment** — bass = the big dipole lobe, highs = higher-order
    /// moments (a binomial-weighted axial dipole array per band; the multipole
    /// expansion IS the spherical-harmonic series, so the spectrum literally
    /// becomes the field's spatial mode structure). Replaces the point/antenna
    /// source while on (needs the audio drive on). Off = the Tier-1 breathing.
    #[id = "admp"] pub ad_multipole: BoolParam,
    /// Per-band wavelength spread (0..1): compresses the honest per-band
    /// wavenumber ratio `k_b = k·(f_b/f_sub)^spread` — 0 = every band at the base
    /// k; 1 = the full (huge) audio ratio. Higher bands ripple at a finer spatial
    /// wavelength, as physics wants (λ ∝ 1/f), scaled to stay watchable.
    #[id = "adsp"] pub ad_spread: FloatParam,
    /// Colour by band (0..1): blends the energy dye / band geometry tint from the
    /// ember hue toward the energy-weighted **band hue** (sub = ember, high =
    /// ~⅔ around the wheel) — a bright bass note glows warm, cymbals sparkle cool.
    #[id = "adbh"] pub ad_band_hue: FloatParam,
    /// #248 Tier 3 — **stereo lean** (0..1): the smoothed mix balance shifts the whole
    /// source stack (point sources + band multipoles) along X by up to ±Separation, so
    /// the field leans with the pan. 0 = centred (no lean). Needs the drive on.
    #[id = "adst"] pub ad_stereo: FloatParam,
    /// #248 Tier 3 — **pitch → rate**: the spectral centroid (brightness) scales the
    /// Maxwell field's oscillation clock — brighter music breathes faster. A declared,
    /// hugely scaled-down rate (never the 20 Hz–20 kHz carrier). 0 = the base clock.
    #[id = "adpt"] pub ad_pitch: FloatParam,
    /// #248 Tier 3 — **waveform shells**: the recent loudness history modulates the
    /// field energy **radially** as retarded amplitude — a loud moment radiates outward
    /// as a bright shell through the energy cloud. 0 = off. Needs the drive on.
    #[id = "adwv"] pub ad_wave: FloatParam,

    // --- Axon Waveguide generator (#218, Tier 1) ---
    /// Number of axon fibres in the bundle.
    #[id = "awct"] pub ax_count: IntParam,
    /// Fibre length (world units, along +Y).
    #[id = "awln"] pub ax_length: FloatParam,
    /// Bundle cross-section radius (how far the fibres spread; 0 = a single axis).
    #[id = "awbd"] pub ax_bundle: FloatParam,
    /// Samples (frames) per fibre — the tube resolution along its length.
    #[id = "awsm"] pub ax_samples: IntParam,
    /// Sheath thickness (tube radius).
    #[id = "awth"] pub ax_thickness: FloatParam,
    /// Ranvier-node spacing (world units between sheath constrictions).
    #[id = "awns"] pub ax_node_spacing: FloatParam,
    /// Ranvier-node pinch depth (0 = smooth tube, 1 = fully constricted at nodes).
    #[id = "awnd"] pub ax_node_dip: FloatParam,
    /// Travelling-pulse speed (fibre-lengths per unit of the global clock).
    #[id = "awps"] pub ax_pulse_speed: FloatParam,
    /// Travelling-pulse width (fraction of the fibre length).
    #[id = "awpw"] pub ax_pulse_width: FloatParam,
    /// Per-fibre pulse stagger (0 = all fire together, 1 = full travelling wave).
    #[id = "awsg"] pub ax_stagger: FloatParam,
    /// Bundle splay: the fibres fan out (+) or in (−) along their length.
    #[id = "awsy"] pub ax_splay: FloatParam,
    /// Deterministic packing seed.
    #[id = "awsd"] pub ax_seed: IntParam,
    /// Guided mode (#218 Tier 2): which LP mode lights the bundle cross-section.
    #[id = "awmd"] pub ax_mode: EnumParam<AxonMode>,
    /// Guided-mode amount (0 = uniform bundle, 1 = full mode intensity pattern).
    #[id = "awma"] pub ax_mode_amount: FloatParam,
    /// Bend-degradation (#218 Tier 3): scatters the edge-riding guided modes —
    /// they leak along the fibre and flare at the Ranvier nodes — while the
    /// centre LP01 core survives. Drives only the optics; the geometric curve is
    /// `ax_curve`. 0 = coherent (no scatter).
    #[id = "awbn"] pub ax_bend: FloatParam,
    /// Tract curvature (#218): 0 = a straight bundle, rising toward 1 bends it
    /// into a broad C-shaped arc — a white-matter tract (corpus callosum /
    /// fasciculus) sweep. Real brain axons run in curved fascicles, not straight.
    #[id = "awcv"] pub ax_curve: FloatParam,
    /// Tortuosity: per-fibre undulation within the tract (0 = parallel fibres,
    /// higher = the organic slack/interweave of real axons).
    #[id = "awto"] pub ax_tortuosity: FloatParam,
    /// DTI colouring (#218): cross-fade to the diffusion-MRI tractography look —
    /// the fibre direction coded as RGB (the "brain wiring" rainbow). 0 = the
    /// action-potential spark colour.
    #[id = "awdt"] pub ax_dti: FloatParam,
    /// Dispersion (#218 Tier 4): chirps the travelling pulse into a chromatic
    /// spread (a wavelength-dependent group velocity — warm trailing, cool leading).
    /// 0 = a single-colour pulse.
    #[id = "awds"] pub ax_dispersion: FloatParam,
    /// Polarization (#218 Tier 4): a coherence shimmer that stays clean on the
    /// surviving core but scrambles to noise on the leaking (bent, edge-riding)
    /// fibres. 0 = off.
    #[id = "awpz"] pub ax_polarization: FloatParam,

    // --- Neural Network generator (#226, Tier 1: nodes + tract edges) ---
    /// Synthetic graph topology (random-geometric / layered / ring / small-world).
    #[id = "nwtp"] pub nw_topology: EnumParam<NeuralTopology>,
    /// Node count (per layer for the Layered topology).
    #[id = "nwnd"] pub nw_nodes: IntParam,
    /// Connectivity: ring/small-world neighbours each side (×2 = k), or the
    /// layered fan-out (edges per unit to the next layer).
    #[id = "nwkc"] pub nw_connectivity: IntParam,
    /// Rewire probability (small-world) / connection radius as a fraction of extent
    /// (random-geometric). Ignored by the ring/layered topologies.
    #[id = "nwpr"] pub nw_rewire: FloatParam,
    /// Feed-forward layer count (Layered topology only).
    #[id = "nwly"] pub nw_layers: IntParam,
    /// Deterministic graph seed.
    #[id = "nwsd"] pub nw_seed: IntParam,
    /// Spatial extent (graph scale in world units).
    #[id = "nwex"] pub nw_extent: FloatParam,
    /// Node soma size (hubs — high-degree nodes — read bigger).
    #[id = "nwns"] pub nw_node_size: FloatParam,
    /// Node soma glow (emissive brightness; hubs brighter).
    #[id = "nwng"] pub nw_node_glow: FloatParam,
    /// Edge fibre thickness (scaled by edge weight).
    #[id = "nwet"] pub nw_edge_thickness: FloatParam,
    /// Edge bow: 0 = straight tracts, higher bends each bundle into an arc.
    #[id = "nweb"] pub nw_edge_bow: FloatParam,
    /// Samples (frames) per edge tract — the tube resolution along its length.
    #[id = "nwes"] pub nw_edge_samples: IntParam,
    /// Travelling-pulse speed along the edges (fires the wiring as a wave).
    #[id = "nwps"] pub nw_pulse_speed: FloatParam,
    /// Travelling-pulse width (fraction of each edge's length).
    #[id = "nwpw"] pub nw_pulse_width: FloatParam,
    /// Edge fibres (#226 Tier 1.5): 1 = a single tube (Tier-1 look); >1 renders each
    /// edge as a **myelinated fibre bundle** — the #218 Axon Waveguide tract at
    /// network scale (Vogel-packed fibres, Ranvier nodes, staggered pulse).
    #[id = "nwef"] pub nw_edge_fibres: IntParam,
    /// Bundle radius: how fat each axon-bundle edge is in the tract's cross-section.
    #[id = "nwbr"] pub nw_bundle_radius: FloatParam,
    /// Ranvier-node constriction depth on the bundle fibres (0 = smooth sheath).
    #[id = "nwrd"] pub nw_edge_node_dip: FloatParam,
    /// Ranvier nodes per edge — the constriction count along each tract.
    #[id = "nwrn"] pub nw_ranvier: IntParam,
    /// Dendrite length (#226 Tier 1.5): 0 = a plain soma blob; >0 sprouts a short
    /// **dendritic arbor** from each neuron (hubs branch more). In soma-size units.
    #[id = "nwdl"] pub nw_dendrite: FloatParam,
    /// Dendrite count — sprouts per soma when `nw_dendrite` > 0.
    #[id = "nwdc"] pub nw_dendrite_count: IntParam,

    // --- Neural Network signal propagation (#226 Tier 2) ---
    /// Firing mode: Off (Tier-1 free-running pulse) / Wavefront / Oscillation /
    /// Stimulus. When ≠ Off, the activation cascade drives node glow + edge pulses.
    #[id = "nwfm"] pub nw_fire_mode: EnumParam<NeuralFireMode>,
    /// Fire threshold: integrated activation a node needs to fire.
    #[id = "nwth"] pub nw_threshold: FloatParam,
    /// Conduction speed (world units per beat) — sets the edge arc-length ÷ speed
    /// conduction delay.
    #[id = "nwcs"] pub nw_conduction: FloatParam,
    /// Refractory period (beats) a fired node can't re-fire.
    #[id = "nwrf"] pub nw_refractory: FloatParam,
    /// Activation decay (leak per beat) — how fast a node forgets sub-threshold input.
    #[id = "nwdk"] pub nw_decay: FloatParam,
    /// Deposit gain: activation added to the target when a pulse arrives.
    #[id = "nwdp"] pub nw_deposit: FloatParam,
    /// Stimulus rate (injections per beat) for the wavefront/oscillation/stimulus seeds.
    #[id = "nwsr"] pub nw_stim_rate: FloatParam,
    /// Signal motes (#81): bright blobs riding the active pulses (0 = off).
    #[id = "nwmo"] pub nw_motes: FloatParam,

    // --- Neural Network — MLP (real weights, #226 Tier 4) ---
    /// Signed-weight edge colouring: 0 = the fibre base, 1 = full warm(+)/cool(−) tint.
    #[id = "nwsc"] pub nw_sign_colour: FloatParam,
    /// Sparsify: drop edges whose |weight| is below this fraction of the max |weight|.
    #[id = "nwsp"] pub nw_sparsify: FloatParam,
    /// Layer spacing for the MLP layout (planes along X).
    #[id = "nwlg"] pub nw_layer_gap: FloatParam,
    /// Live-input drive: 0 = the loaded JSON input (static), >0 = a beat-modulated
    /// input amplitude so the forward-pass activations breathe.
    #[id = "nwid"] pub nw_mlp_drive: FloatParam,

    // --- Neural Network — Attention (transformer, #226 Tier 5) ---
    /// Which transformer layer's attention to visualize (clamped to the loaded depth).
    #[id = "nwal"] pub nw_attn_layer: FloatParam,
    /// Which attention head to visualize (clamped to the loaded head count).
    #[id = "nwah"] pub nw_attn_head: FloatParam,
    /// Attention-edge threshold: hide edges whose weight A_ij is below this (declutter).
    #[id = "nwat"] pub nw_attn_threshold: FloatParam,
    /// Token count for the synthesized pattern (ignored when a tensor is loaded).
    #[id = "nwan"] pub nw_attn_tokens: FloatParam,
    /// Reveal rate (query tokens per beat): grows the attended set — token-by-token
    /// generation. 0 = reveal everything at once.
    #[id = "nwar"] pub nw_attn_reveal: FloatParam,
    /// Sweep rate (layer/head steps per beat): auto-cycle the visualized head over beats.
    #[id = "nwaw"] pub nw_attn_sweep: FloatParam,
    /// Ring layout: 0 = tokens in a row, 1 = tokens on a circle (attention chords).
    #[id = "nwag"] pub nw_attn_ring: FloatParam,

    // --- Neural Tissue surface (#260 Tier 1) ---
    /// Soma (cell-body) size multiplier — scales every node's icosphere.
    #[id = "ntsz"] pub nt_soma_size: FloatParam,
    /// Soma shape: 0 = round, 1 = a teardrop/pyramidal stretch toward the node's
    /// dominant connection (an anisotropic silhouette hint).
    #[id = "ntsh"] pub nt_soma_shape: FloatParam,
    /// Bouton size — the synaptic-terminal bulb at each edge's target end,
    /// relative to the edge thickness.
    #[id = "ntbo"] pub nt_bouton_size: FloatParam,
    /// Membrane translucency / subsurface amount (waxy neural membrane; 0 = inert,
    /// drives the shared Surface-FX SSS path).
    #[id = "ntss"] pub nt_membrane_sss: FloatParam,
    /// Membrane iridescence amount (view-angle sheen on the membrane; 0 = inert).
    #[id = "ntir"] pub nt_membrane_irid: FloatParam,

    // --- Neural Tissue morphology (#260 Tier 2) — dendritic arbors + axon ---
    /// Dendrite density: 0 = no arbor (a bare soma, byte-identical to Tier 1);
    /// higher grows more primary branches + deeper bifurcation per neuron.
    #[id = "ntdd"] pub nt_dendrite_density: FloatParam,
    /// Dendrite length: overall arbor reach, in soma radii.
    #[id = "ntdl"] pub nt_dendrite_length: FloatParam,
    /// Dendrite taper: child-segment radius as a fraction of the parent's (Rall-ish;
    /// < 1 → tips thinner than the trunk).
    #[id = "ntdt"] pub nt_dendrite_taper: FloatParam,
    /// Neuron morphology class (pyramidal / stellate / by node degree).
    #[id = "ntty"] pub nt_neuron_type: EnumParam<NeuronType>,
    /// Dendritic spines: 0 = off; > 0 sprinkles tiny stubs along the branches
    /// (higher-detail; off by default).
    #[id = "ntsp"] pub nt_spines: FloatParam,

    // --- Neural Tissue myelin (#260 Tier 3) — myelinated axons ---
    /// Myelin amount: 0 = plain capped-capsule edges (inert, byte-identical to
    /// Tier 1/2); > 0 lowers each edge as a myelinated nerve fibre — fatty
    /// internode sheath segments separated by Ranvier-node constrictions — and
    /// fans thick tracts (high weight / high degree) into a bundle of parallel
    /// Vogel-packed fibres. The action potential conducts saltatorily (the bright
    /// internode jumps Ranvier-node to Ranvier-node with the Tier-2 sim pulse).
    #[id = "ntmy"] pub nt_myelin_amount: FloatParam,
    /// Ranvier spacing: the internodal length (world units) between Ranvier nodes —
    /// smaller = more, tighter constrictions along the fibre.
    #[id = "ntrv"] pub nt_ranvier_spacing: FloatParam,
    /// Sheath scale: how much fatter the glossy myelin internode is vs the base
    /// edge thickness (the fatty-myelin bulge; the node constrictions stay thin).
    #[id = "ntsc"] pub nt_sheath_scale: FloatParam,

    // --- Neural Tissue synapse + tissue context (#260 Tier 4, the final tier) ---
    /// Synaptic cleft: 0 = inert (the terminal bouton sits at its natural 0.86 along
    /// the axon, byte-identical to Tiers 1–3); > 0 pulls the bulb back off the
    /// post-synaptic membrane so a visible cleft gap opens.
    #[id = "ntcl"] pub nt_synapse_cleft: FloatParam,
    /// Cytoplasmic interior glow: 0 = inert; > 0 lights each soma from within,
    /// scaled by its live activation (the finalized neural material's activation-tied
    /// glow, on top of the membrane SSS/iridescence path).
    #[id = "ntgw"] pub nt_synapse_glow: FloatParam,
    /// Neurotransmitter vesicles: 0 = off; > 0 emits a deterministic vesicle burst
    /// crossing the cleft on each spike arrival (the Tier-2/3 cascade deposit event).
    /// A pure function of sim state (no per-frame flicker); needs the firing sim on.
    #[id = "ntve"] pub nt_synapse_vesicles: FloatParam,
    /// Glia / astrocyte scaffolding: 0 = off; > 0 sprouts faint sparse branching
    /// stubs off a seeded subset of somata (count scales with the dial) so the
    /// network sits in tissue. Emitted into the capsule sub-batch; deterministic.
    #[id = "ntga"] pub nt_glia: FloatParam,
    /// Capillary threads: 0 = off; > 0 routes a few dim wandering capillary tubes
    /// across the tissue volume (count scales with the dial). Capsule sub-batch;
    /// deterministic.
    #[id = "ntcp"] pub nt_capillary: FloatParam,
    // --- Physical thin-film interference (soap film / bubbles, #258 Tier 1) ---
    /// Base film thickness in nanometres. 0 = the physical model is OFF and the
    /// shader keeps the existing cosine-hack iridescence path (byte-identical
    /// default); >0 evaluates a real wavelength-resolved Airy interference model.
    #[id = "tfth"] pub film_thickness: FloatParam,
    /// Thickness noise-marbling amount (swirls in the interference bands).
    #[id = "tfvr"] pub film_thickness_var: FloatParam,
    /// Film refractive index (soap ≈ 1.33).
    #[id = "tfio"] pub film_ior: FloatParam,
    /// Gravity-drainage gradient: thin at the top → thick at the bottom, along
    /// world-space up. 0 = uniform thickness.
    #[id = "tfdr"] pub film_drainage: FloatParam,

    // --- Brain model (#275 Tier 1) — used when NN topology = Brain model ---
    /// Gyri/sulci fold amplitude (0 = a smooth cerebrum ellipsoid).
    #[id = "brfd"] pub br_fold_depth: FloatParam,
    /// Fold frequency — roughly the number of gyri wrapping the cortex.
    #[id = "brff"] pub br_fold_freq: FloatParam,
    /// Longitudinal-fissure width (the midline gap between the two hemispheres).
    #[id = "brhg"] pub br_hemi_gap: FloatParam,
    /// Local cortical connectivity — each neuron wires to its k nearest neighbours.
    #[id = "brlk"] pub br_local_k: IntParam,
    /// Cerebellum + brainstem: 0 = cerebrum only; > 0 adds a foliated cerebellum
    /// (this fraction of the neurons) tucked posterior-inferior + a brainstem stub.
    #[id = "brcb"] pub br_cerebellum: FloatParam,
    /// (#275 T2) Long-range **association tracts** — sparse intra-hemisphere shortcuts
    /// (fraction of cortex) that make the sheet small-world. 0 = local cortex only.
    #[id = "bras"] pub br_assoc: FloatParam,
    /// (#275 T2) **Corpus callosum** — commissural fibres bridging homologous (mirror)
    /// cortical points across the fissure (density). 0 = hemispheres unconnected.
    #[id = "brco"] pub br_callosum: FloatParam,
    /// (#275 T2) **Subcortical** deep-grey nuclei (thalamus/basal-ganglia region) + their
    /// cortical projections. 0 = cortex only.
    #[id = "brsc"] pub br_subcortical: FloatParam,
    /// (#275 T3) **Target highlight** — brighten the selected region's neurons so its
    /// location on the cortex reads (the address the TMS/entrainment tools aim at). 0 = off.
    #[id = "brrh"] pub br_region_hi: FloatParam,
    /// (#275 T3) **Target region** id: 0 L-M1, 1 R-M1, 2 L-DLPFC, 3 SMA, 4 V1, 5 L-temporal,
    /// 6 R-temporal, 7 subgenual cingulate.
    #[id = "brtg"] pub br_target: IntParam,
    /// (#275 T4) **Stimulation strength** — a coil-like focal drive at the target region.
    /// > 0 turns the cascade sim on (the stimulus IS the drive) and pulses the target's
    /// neurons; the effect then propagates along the connectome + corpus callosum. 0 = off.
    #[id = "brsa"] pub br_stim_amount: FloatParam,
    /// (#275 T4) **Stimulation rate** — focal pulses per beat (the rTMS-like frequency).
    #[id = "brsr"] pub br_stim_rate: FloatParam,
    /// (#275) **Signal swell** — how much a firing neuron's soma physically swells with its
    /// activation. 0 = glow only (the anatomy holds still while signals propagate); up to 0.5 =
    /// the "living tissue" throb. Brain topology only; the abstract Neural Network keeps 0.5.
    #[id = "brsw"] pub br_signal_swell: FloatParam,

    // --- Demo scene bench (#288) — used when generator = Demo ---
    /// Which reference sub-scene to build (Cornell box / sphere pyramid / … / light stage).
    #[id = "dmsn"] pub demo_scene: EnumParam<DemoScene>,
    /// Overall scene scale (the box / stack is built at unit proportions × this).
    #[id = "dmsz"] pub demo_size: FloatParam,
    /// Inner objects: for the Cornell box, the two hero boxes/spheres inside. Off = an empty box.
    #[id = "dmob"] pub demo_objects: BoolParam,
    /// Hold the reference framing: 1 = a fixed front-on camera (the auto-orbit is gated off so
    /// the canonical view holds), 0 = let the orbit/camera-path rig move as usual.
    #[id = "dmfc"] pub demo_static_cam: BoolParam,
    /// Light intensity for the scene's emitters (Tier 3) / the analytic key when no emitter drives it.
    #[id = "dmli"] pub demo_light: FloatParam,
    /// Roughness for the scene's smooth/metal materials (the walls stay diffuse).
    #[id = "dmrf"] pub demo_roughness: FloatParam,
    /// Object count knob: pyramid rows / sphere-grid side / light-stage light count.
    #[id = "dmct"] pub demo_count: IntParam,
    /// Turntable spin: rotates the hero objects (and the light rig) on the beat clock. 0 = still.
    #[id = "dmsp"] pub demo_spin: FloatParam,

    // --- Synchrotron radiation generator (#150, Phase 1) ---
    /// Orbit radius R of the radiating charge(s).
    #[id = "syrd"] pub sy_radius: FloatParam,
    /// Orbital speed β = v/c (c = 1). Relativistic beaming sharpens as β → 1
    /// (ω = β/R, so radius and speed jointly set the orbital frequency).
    #[id = "sybt"] pub sy_beta: FloatParam,
    /// Number of bunched charges sharing the ring (evenly phased) — they superpose.
    #[id = "sych"] pub sy_charges: IntParam,
    /// Samples per axis of the square sampling plane (grid × grid arrows).
    #[id = "sygr"] pub sy_grid: IntParam,
    /// Half-extent of the sampling plane (it spans ±extent in each axis).
    #[id = "syex"] pub sy_extent: FloatParam,
    /// Velocity (near, 1/R²) field weight: 1 = full physical field, 0 = radiation only.
    #[id = "synf"] pub sy_near: FloatParam,
    /// Arrow length gain (× the cell size, scaled by a soft-saturated |E|).
    #[id = "syam"] pub sy_amp: FloatParam,
    /// Arrow rod thickness.
    #[id = "syth"] pub sy_thickness: FloatParam,
    /// Source clamp: |r − r_s| is floored here so the 1/R, 1/R² singularity stays finite.
    #[id = "syrm"] pub sy_rmin: FloatParam,
    /// Sample the plane perpendicular to the orbit (XZ) instead of the orbit plane (XY).
    #[id = "sypp"] pub sy_perp: BoolParam,
    /// Render view: field arrows (Phase 1) or traced E field lines (Phase 3).
    #[id = "syvw"] pub sy_view: EnumParam<SyncView>,
    /// Field-line seeds (Fibonacci sphere around the orbit) — field-line view.
    #[id = "syls"] pub sy_line_seeds: IntParam,
    /// Max integration steps per field line — field-line view.
    #[id = "sylt"] pub sy_line_steps: IntParam,
    /// Field-line integration step length ds — field-line view.
    #[id = "syld"] pub sy_line_ds: FloatParam,
    /// Field-line bound: a line stops if it leaves this radius — field-line view.
    #[id = "sylb"] pub sy_line_bound: FloatParam,
    /// Volume depth: number of slices the arrow plane is extruded into — volume view
    /// (in-plane resolution/size reuse `grid`/`extent`; 1 = the central plane).
    #[id = "syvl"] pub sy_vol_layers: IntParam,
    /// Reveal: cull arrows whose soft-saturated |E| is below this — carves away the
    /// dead low-field crust so only the active core/lobes/spiral show (0 = keep all).
    /// Applies to the arrow + volume views.
    #[id = "syrv"] pub sy_reveal: FloatParam,
    /// Sphere-invert the volume view's display positions (turn it inside-out) so the
    /// dense near-source core blows up to fill the view.
    #[id = "syiv"] pub sy_invert: BoolParam,
    /// Inversion sphere radius c: points at |p| = c stay put; inside ↔ outside swap.
    #[id = "syir"] pub sy_invert_radius: FloatParam,
    /// Orbit-plane tilt (degrees off XY) — the charge's circle tips out of the plane.
    /// With precession this makes the activity tumble through 3-D (0 = the planar orbit).
    #[id = "syti"] pub sy_tilt: FloatParam,
    /// Orbital-plane precession rate — the tilted plane's normal cones around Z over
    /// time (Larmor-style), so the whole field sweeps 3-D instead of one plane.
    #[id = "sypr"] pub sy_precess: FloatParam,

    // --- Vector-field plotter generator (#173, Tier 1) ---
    /// Which function F(x, y, z) to plot (the curated bank; ids wire-stable).
    #[id = "vfpr"] pub vf_preset: EnumParam<VecFieldPreset>,
    /// Lattice samples along X (1 collapses the axis to the central plane).
    #[id = "vfgx"] pub vf_grid_x: IntParam,
    /// Lattice samples along Y.
    #[id = "vfgy"] pub vf_grid_y: IntParam,
    /// Lattice samples along Z — set 1 for the literal 2-D vector-field plot.
    #[id = "vfgz"] pub vf_grid_z: IntParam,
    /// Half-extent of the sampling box (it spans ±extent per axis).
    #[id = "vfex"] pub vf_extent: FloatParam,
    /// Domain scale k: F is evaluated at k·p — the field's spatial frequency
    /// (how many sin periods / feature cells fit across the box).
    #[id = "vffs"] pub vf_field_scale: FloatParam,
    /// Arrow length gain (× the lattice cell, scaled by the mapped |F|).
    #[id = "vfam"] pub vf_amp: FloatParam,
    /// Arrow rod thickness.
    #[id = "vfth"] pub vf_thickness: FloatParam,
    /// How |F| maps to arrow length: soft-saturate / log-compress / uniform.
    #[id = "vfmm"] pub vf_mag_map: EnumParam<VecMagMap>,
    /// Arrow colour source: |F| ramp (native / palette) or direction (F̂ → RGB).
    #[id = "vftm"] pub vf_tint_mode: EnumParam<VecTint>,
    /// Animation speed: the whole field rigidly turns about Z as the clock
    /// advances, and the rotation-invariant presets (rotation / source / saddle
    /// / dipole / helix) run their intrinsic motion — nutating axis, breathing
    /// shells, kneading strain, precessing moment, travelling lift — on the
    /// same clock (0 = the static textbook plot).
    #[id = "vfev"] pub vf_evolve: FloatParam,
    /// 3-D lift for the planar classics: adds Fz = lift·sin(z′) so the 2-D
    /// reference fields weave through the volume (3-D presets ignore it).
    #[id = "vfzl"] pub vf_z_lift: FloatParam,
    /// Cull arrows whose soft-saturated |F| is below this (carves away dead
    /// space around sources/poles; 0 = keep the full lattice).
    #[id = "vfrv"] pub vf_reveal: FloatParam,
    /// Render view (#173): arrows / traced field lines / both / stream
    /// surface (equal-length lines from a seed curve — Grid topology, so
    /// Membrane lofts a flowing sheet; other surface modes see even lines).
    #[id = "vfvw"] pub vf_view: EnumParam<VecFieldView>,
    /// Field-line seeding strategy (lattice / random / ring / plane / |F|-weighted).
    #[id = "vfsd"] pub vf_seed_mode: EnumParam<VecSeedMode>,
    /// Number of field-line seeds (each becomes one traced line).
    #[id = "vfls"] pub vf_line_seeds: IntParam,
    /// Max RK4 steps per line (a bidirectional line splits them half each way).
    #[id = "vfst"] pub vf_line_steps: IntParam,
    /// RK4 step length ds (arc-length — the tracer follows the normalized field).
    #[id = "vfds"] pub vf_line_ds: FloatParam,
    /// Trace both directions from each seed and join through it — essential for
    /// saddle topology (off = downstream only).
    #[id = "vfbi"] pub vf_bidir: BoolParam,
    /// Line colour source: local |F| or a sweep along the line's length.
    #[id = "vflc"] pub vf_line_color: EnumParam<VecLineColor>,
    /// Flow pulse amount: brightness/thickness waves that march downstream
    /// along every line as the clock advances (0 = off, the static plot).
    #[id = "vffa"] pub vf_flow: FloatParam,
    /// Flow pulse speed (cycles per clock unit; rides the global Speed → beat).
    #[id = "vffv"] pub vf_flow_speed: FloatParam,
    /// Field-line rod thickness (arrows keep their own `vf_thickness`).
    #[id = "vflh"] pub vf_line_thickness: FloatParam,

    // --- Vector-field function builder (#173, Tier 3; active when the bank = Custom) ---
    /// Builder Fx term 1: shaping function (Off silences the slot).
    #[id = "vbx1f"] pub vb_x1_func: EnumParam<VecTermFunc>,
    /// Builder Fx term 1: output gain (weight in the sum).
    #[id = "vbx1g"] pub vb_x1_gain: FloatParam,
    /// Builder Fx term 1: argument x coefficient (u = a·x + b·y + c·z + phase).
    #[id = "vbx1a"] pub vb_x1_a: FloatParam,
    /// Builder Fx term 1: argument y coefficient.
    #[id = "vbx1b"] pub vb_x1_b: FloatParam,
    /// Builder Fx term 1: argument z coefficient.
    #[id = "vbx1c"] pub vb_x1_c: FloatParam,
    /// Builder Fx term 1: argument phase offset (automate for travelling waves).
    #[id = "vbx1p"] pub vb_x1_phase: FloatParam,
    /// Builder Fx term 2: shaping function (Off silences the slot).
    #[id = "vbx2f"] pub vb_x2_func: EnumParam<VecTermFunc>,
    /// Builder Fx term 2: output gain (weight in the sum).
    #[id = "vbx2g"] pub vb_x2_gain: FloatParam,
    /// Builder Fx term 2: argument x coefficient (u = a·x + b·y + c·z + phase).
    #[id = "vbx2a"] pub vb_x2_a: FloatParam,
    /// Builder Fx term 2: argument y coefficient.
    #[id = "vbx2b"] pub vb_x2_b: FloatParam,
    /// Builder Fx term 2: argument z coefficient.
    #[id = "vbx2c"] pub vb_x2_c: FloatParam,
    /// Builder Fx term 2: argument phase offset (automate for travelling waves).
    #[id = "vbx2p"] pub vb_x2_phase: FloatParam,
    /// Builder Fx term 3: shaping function (Off silences the slot).
    #[id = "vbx3f"] pub vb_x3_func: EnumParam<VecTermFunc>,
    /// Builder Fx term 3: output gain (weight in the sum).
    #[id = "vbx3g"] pub vb_x3_gain: FloatParam,
    /// Builder Fx term 3: argument x coefficient (u = a·x + b·y + c·z + phase).
    #[id = "vbx3a"] pub vb_x3_a: FloatParam,
    /// Builder Fx term 3: argument y coefficient.
    #[id = "vbx3b"] pub vb_x3_b: FloatParam,
    /// Builder Fx term 3: argument z coefficient.
    #[id = "vbx3c"] pub vb_x3_c: FloatParam,
    /// Builder Fx term 3: argument phase offset (automate for travelling waves).
    #[id = "vbx3p"] pub vb_x3_phase: FloatParam,
    /// Builder Fy term 1: shaping function (Off silences the slot).
    #[id = "vby1f"] pub vb_y1_func: EnumParam<VecTermFunc>,
    /// Builder Fy term 1: output gain (weight in the sum).
    #[id = "vby1g"] pub vb_y1_gain: FloatParam,
    /// Builder Fy term 1: argument x coefficient (u = a·x + b·y + c·z + phase).
    #[id = "vby1a"] pub vb_y1_a: FloatParam,
    /// Builder Fy term 1: argument y coefficient.
    #[id = "vby1b"] pub vb_y1_b: FloatParam,
    /// Builder Fy term 1: argument z coefficient.
    #[id = "vby1c"] pub vb_y1_c: FloatParam,
    /// Builder Fy term 1: argument phase offset (automate for travelling waves).
    #[id = "vby1p"] pub vb_y1_phase: FloatParam,
    /// Builder Fy term 2: shaping function (Off silences the slot).
    #[id = "vby2f"] pub vb_y2_func: EnumParam<VecTermFunc>,
    /// Builder Fy term 2: output gain (weight in the sum).
    #[id = "vby2g"] pub vb_y2_gain: FloatParam,
    /// Builder Fy term 2: argument x coefficient (u = a·x + b·y + c·z + phase).
    #[id = "vby2a"] pub vb_y2_a: FloatParam,
    /// Builder Fy term 2: argument y coefficient.
    #[id = "vby2b"] pub vb_y2_b: FloatParam,
    /// Builder Fy term 2: argument z coefficient.
    #[id = "vby2c"] pub vb_y2_c: FloatParam,
    /// Builder Fy term 2: argument phase offset (automate for travelling waves).
    #[id = "vby2p"] pub vb_y2_phase: FloatParam,
    /// Builder Fy term 3: shaping function (Off silences the slot).
    #[id = "vby3f"] pub vb_y3_func: EnumParam<VecTermFunc>,
    /// Builder Fy term 3: output gain (weight in the sum).
    #[id = "vby3g"] pub vb_y3_gain: FloatParam,
    /// Builder Fy term 3: argument x coefficient (u = a·x + b·y + c·z + phase).
    #[id = "vby3a"] pub vb_y3_a: FloatParam,
    /// Builder Fy term 3: argument y coefficient.
    #[id = "vby3b"] pub vb_y3_b: FloatParam,
    /// Builder Fy term 3: argument z coefficient.
    #[id = "vby3c"] pub vb_y3_c: FloatParam,
    /// Builder Fy term 3: argument phase offset (automate for travelling waves).
    #[id = "vby3p"] pub vb_y3_phase: FloatParam,
    /// Builder Fz term 1: shaping function (Off silences the slot).
    #[id = "vbz1f"] pub vb_z1_func: EnumParam<VecTermFunc>,
    /// Builder Fz term 1: output gain (weight in the sum).
    #[id = "vbz1g"] pub vb_z1_gain: FloatParam,
    /// Builder Fz term 1: argument x coefficient (u = a·x + b·y + c·z + phase).
    #[id = "vbz1a"] pub vb_z1_a: FloatParam,
    /// Builder Fz term 1: argument y coefficient.
    #[id = "vbz1b"] pub vb_z1_b: FloatParam,
    /// Builder Fz term 1: argument z coefficient.
    #[id = "vbz1c"] pub vb_z1_c: FloatParam,
    /// Builder Fz term 1: argument phase offset (automate for travelling waves).
    #[id = "vbz1p"] pub vb_z1_phase: FloatParam,
    /// Builder Fz term 2: shaping function (Off silences the slot).
    #[id = "vbz2f"] pub vb_z2_func: EnumParam<VecTermFunc>,
    /// Builder Fz term 2: output gain (weight in the sum).
    #[id = "vbz2g"] pub vb_z2_gain: FloatParam,
    /// Builder Fz term 2: argument x coefficient (u = a·x + b·y + c·z + phase).
    #[id = "vbz2a"] pub vb_z2_a: FloatParam,
    /// Builder Fz term 2: argument y coefficient.
    #[id = "vbz2b"] pub vb_z2_b: FloatParam,
    /// Builder Fz term 2: argument z coefficient.
    #[id = "vbz2c"] pub vb_z2_c: FloatParam,
    /// Builder Fz term 2: argument phase offset (automate for travelling waves).
    #[id = "vbz2p"] pub vb_z2_phase: FloatParam,
    /// Builder Fz term 3: shaping function (Off silences the slot).
    #[id = "vbz3f"] pub vb_z3_func: EnumParam<VecTermFunc>,
    /// Builder Fz term 3: output gain (weight in the sum).
    #[id = "vbz3g"] pub vb_z3_gain: FloatParam,
    /// Builder Fz term 3: argument x coefficient (u = a·x + b·y + c·z + phase).
    #[id = "vbz3a"] pub vb_z3_a: FloatParam,
    /// Builder Fz term 3: argument y coefficient.
    #[id = "vbz3b"] pub vb_z3_b: FloatParam,
    /// Builder Fz term 3: argument z coefficient.
    #[id = "vbz3c"] pub vb_z3_c: FloatParam,
    /// Builder Fz term 3: argument phase offset (automate for travelling waves).
    #[id = "vbz3p"] pub vb_z3_phase: FloatParam,
    /// Builder field operator: direct / gradient (curl-free) / curl
    /// (divergence-free) / Helmholtz blend.
    #[id = "vbop"] pub vb_op: EnumParam<VecFieldOp>,
    /// Helmholtz blend mix (0 = pure gradient ... 1 = pure curl; Helmholtz op only).
    #[id = "vbmx"] pub vb_mix: FloatParam,

    // --- Z0NE rails generator (#187, Tier 1) ---
    /// Forward speed in world units per beat (the rail is beat-parametrized:
    /// speed stretches space, never breaks the beat alignment).
    #[id = "rlsp"] pub rl_speed: FloatParam,
    /// Clear flight-channel radius — the profile's innermost point sits on it.
    #[id = "rlbo"] pub rl_bore: FloatParam,
    /// Morph-cell length in beats (profile control points live at cell seams).
    #[id = "rlcl"] pub rl_cell_len: EnumParam<RailCellLen>,
    /// How far each cell's hashed profile strays from the base throat (0 = one
    /// unchanging tube).
    #[id = "rlva"] pub rl_variance: FloatParam,
    /// World seed — same seed, same infinite ride.
    #[id = "rlsd"] pub rl_seed: IntParam,
    /// Elements around the ring (the wall's angular density).
    #[id = "rlrn"] pub rl_ring_n: IntParam,
    /// Rows per beat along the rail (the wall's axial density).
    #[id = "rlrb"] pub rl_rows_beat: IntParam,
    /// Beats of corridor visible ahead (the perf dial).
    #[id = "rlhz"] pub rl_horizon: FloatParam,
    /// Beat ribs: rows at integer beats scale up and flash on the beat clock.
    #[id = "rlrg"] pub rl_rib_gain: FloatParam,
    /// Element size as a fraction of the natural ring spacing.
    #[id = "rlth"] pub rl_thickness: FloatParam,
    /// Max superformula lobe count the per-cell hash may pick.
    #[id = "rllb"] pub rl_lobes: IntParam,
    /// How sharp/spiky the hashed profile exponents may get (0 = round).
    #[id = "rlsk"] pub rl_spike: FloatParam,
    /// Profile rotation in turns per beat (the corridor corkscrews).
    #[id = "rltw"] pub rl_twist: FloatParam,
    /// Radial amplitude of the profile on top of the bore (0 = a plain pipe).
    #[id = "rlsw"] pub rl_swell: FloatParam,
    /// Beats over which rows fade in at the far horizon (masks spawning).
    #[id = "rlfd"] pub rl_fade: FloatParam,
    /// Palette cycles per beat swept along the corridor.
    #[id = "rlcf"] pub rl_color_flow: FloatParam,
    /// Corridor archetype (#187 Tier 2): Throat / Phyllo Wall / Rings & Gates /
    /// Tissue Tube.
    #[id = "rlar"] pub rl_archetype: EnumParam<RailArchetype>,
    /// Phyllo Wall: divergence angle in degrees (137.508 = the golden angle;
    /// small detunes re-lace the parastichy spirals dramatically).
    #[id = "rldv"] pub rl_diverge: FloatParam,
    /// Tissue Tube: concentric wall shells.
    #[id = "rlsh"] pub rl_shells: IntParam,
    /// Phyllo Wall: parastichy strand families (8/13/21 = Fibonacci looks).
    #[id = "rlps"] pub rl_parastichy: IntParam,
    /// Tier 3: the musical phrase — quantized look transitions land on this
    /// boundary, and `evolve` re-rolls the world per phrase.
    #[id = "rlce"] pub rl_change_every: EnumParam<RailChangeEvery>,
    /// Tier 3: how hard each phrase re-rolls the ride on top of the per-cell
    /// variance (0 = off — the ride develops only cell to cell).
    #[id = "rlev"] pub rl_evolve: FloatParam,

    // --- Scenery layer (#187 pivot): concurrent generated scenery with its
    // --- own material / surface FX, independent of the primary generator. ---
    /// Scenery type: None (off) / Zone (the corridor — all rl_* params).
    #[id = "scmd"] pub sc_mode: EnumParam<SceneryMode>,
    /// How scenery nodes render: cubes / flow-aligned rods / swept tubes.
    #[id = "scsf"] pub sc_surface: EnumParam<ScenerySurface>,
    /// Scenery material: Standard PBR / Chrome / Glass — independent of the
    /// primary generator's material.
    #[id = "scmt"] pub sc_mat: EnumParam<MaterialType>,
    /// Scenery metallic (Standard branch).
    #[id = "scme"] pub sc_metallic: FloatParam,
    /// Scenery roughness.
    #[id = "scrg"] pub sc_roughness: FloatParam,
    /// Scenery emissive glow (blooms in HDR).
    #[id = "scgl"] pub sc_glow: FloatParam,
    /// Scenery material emissive: emit the scenery's own colour into HDR (like the
    /// main material's Emissive, for the zone/scenery walls). 0 = off.
    #[id = "scem"] pub sc_emissive: FloatParam,
    /// Scenery opacity (Glass fades its transmitted body).
    #[id = "scop"] pub sc_opacity: FloatParam,
    /// Scenery glass index of refraction.
    #[id = "scio"] pub sc_ior: FloatParam,
    /// Scenery palette (colour LUT) — independent of the main palette.
    #[id = "scpl"] pub sc_palette: EnumParam<Palette>,
    /// Scenery translucency (SSS) amount — additive, inert at 0.
    #[id = "scss"] pub sc_sss: FloatParam,
    /// Scenery SSS normal distortion.
    #[id = "scsd"] pub sc_sss_dist: FloatParam,
    /// Scenery SSS focus power.
    #[id = "scsp"] pub sc_sss_pow: FloatParam,
    /// Scenery iridescence amount — additive, inert at 0.
    #[id = "scir"] pub sc_irid: FloatParam,
    /// Scenery iridescence scale.
    #[id = "scis"] pub sc_irid_scale: FloatParam,
    /// Scenery iridescence hue shift.
    #[id = "scih"] pub sc_irid_shift: FloatParam,

    // --- Terra scenery landform (#206 Tier 2). Timing/window params are shared
    // --- with the Zone/rails block (speed, cell length, change-every, variance,
    // --- seed, evolve, horizon, rows/beat, fade, bore-as-scale); these shape
    // --- the landscape and quantize on the bar via the same latch. ---
    /// Landform family (Fjord / River banks / Canyon).
    #[id = "tzfm"] pub terra_form: EnumParam<TerraForm>,
    /// Wall/ridge height (× the shared bore-scale).
    #[id = "tzrg"] pub terra_ridge: FloatParam,
    /// Channel half-width — the flat navigable floor (× scale).
    #[id = "tzch"] pub terra_channel: FloatParam,
    /// Total valley half-width (× the channel).
    #[id = "tzwd"] pub terra_width: FloatParam,
    /// Wall steepness (0 gentle bank … 1 sheer cliff).
    #[id = "tzst"] pub terra_steep: FloatParam,
    /// Terracing (0 smooth … 1 hard canyon strata).
    #[id = "tztr"] pub terra_terrace: FloatParam,
    /// fBm roughness on the walls.
    #[id = "tzro"] pub terra_rough: FloatParam,
    /// Meander amplitude (lateral channel wander, × scale).
    #[id = "tzme"] pub terra_meander: FloatParam,
    /// Water surface height (world units).
    #[id = "tzwl"] pub terra_water_level: FloatParam,
    /// Water present (shore tint band; Tier 3 renders the surface).
    #[id = "tzwo"] pub terra_water_on: BoolParam,
    /// Navigable clearance below the flight plane the channel keeps (× scale).
    #[id = "tzcl"] pub terra_clearance: FloatParam,
    /// fBm base frequency (detail along the rail).
    #[id = "tznf"] pub terra_noise_freq: FloatParam,

    // --- Terra water surface (#206 Tier 3): the channel water floor, its OWN
    // --- material (a third uniform set) — a Look, applied instantly (the water
    // --- LEVEL is a landform param and quantizes with Terra). ---
    /// Water material (Standard / Chrome / Glass — Glass reads as water).
    #[id = "wtmt"] pub wt_mat: EnumParam<MaterialType>,
    /// Water roughness (low = a mirror-calm surface).
    #[id = "wtrg"] pub wt_roughness: FloatParam,
    /// Water index of refraction (1.33 = real water).
    #[id = "wtio"] pub wt_ior: FloatParam,
    /// Water opacity (Glass fades the transmitted body — see-through water).
    #[id = "wtop"] pub wt_opacity: FloatParam,
    /// Water emissive glow (bioluminescent water when raised).
    #[id = "wtgl"] pub wt_glow: FloatParam,
    /// Ripple amplitude (surface wave height → wavy reflections).
    #[id = "wtrp"] pub wt_ripple: FloatParam,
    /// Ripple frequency (wave detail).
    #[id = "wtrf"] pub wt_ripple_freq: FloatParam,
    /// Water depth absorption (#206): Beer–Lambert darkening toward the deep
    /// colour — clear at the grazing shallows, deep in the channel body.
    #[id = "wtab"] pub wt_absorb: FloatParam,
    /// Water sun-glitter: sharp specular sparkle from the key light on the ripples.
    #[id = "wtgt"] pub wt_glitter: FloatParam,
    /// Water reflectivity: extra grazing-angle mirror on top of the IOR Fresnel.
    #[id = "wtrl"] pub wt_reflect: FloatParam,

    // --- Phyllotaxis / golden-angle generator ---
    /// Placement surface (disk / cone / sphere / shell).
    #[id = "physf"] pub phyl_surface: EnumParam<PhylSurface>,
    /// Node count (rounded down to a multiple of the parastichy count).
    #[id = "phyct"] pub phyl_count: IntParam,
    /// Divergence angle (degrees; golden ≈ 137.5 — small detunes are dramatic).
    #[id = "phydv"] pub phyl_divergence: FloatParam,
    /// Radius scale (overall size).
    #[id = "phyrd"] pub phyl_radius: FloatParam,
    /// Parastichy count: how many spiral families become the strands (best Fibonacci).
    #[id = "phyps"] pub phyl_parastichy: IntParam,
    /// Height (cone lift / shell axial length; unused on disk/sphere).
    #[id = "phyht"] pub phyl_height: FloatParam,
    /// Shell log-spiral growth rate (radius ∝ e^{growth·t}; shell only).
    #[id = "phygr"] pub phyl_growth: FloatParam,
    /// Radius breathing amplitude (0 = none) — pulses the pattern off the clock.
    #[id = "phyba"] pub phyl_breathe_amp: FloatParam,
    /// Radius breathing frequency (cycles per clock unit).
    #[id = "phybf"] pub phyl_breathe_freq: FloatParam,
    /// Rotation speed of the whole pattern off the global Speed clock.
    #[id = "phyrt"] pub phyl_rot: FloatParam,
    /// Node thickness (tube radius / bead size).
    #[id = "phyth"] pub phyl_thickness: FloatParam,

    // --- Tessellation generator (#121: aperiodic tilings as geometry) ---
    /// Tiling family (Phase 1: Penrose P3 only; more land in later phases).
    #[id = "tsfam"] pub tess_family: EnumParam<TilingFamily>,
    /// Inflation depth — how many substitution levels (tile count ≈ 10·φ^(2·depth);
    /// the perf dial). 0 = the bare 5-fold seed.
    #[id = "tsdep"] pub tess_depth: IntParam,
    /// Overall size (the tiling fills unit radius × this scale).
    #[id = "tssc"] pub tess_scale: FloatParam,
    /// Edge-tube thickness (used by the Edges view: tile edges as glowing rods).
    #[id = "tsth"] pub tess_thickness: FloatParam,
    /// View / 2D→3D ladder (#121, Phase 2): edges / flat-filled / extruded prisms.
    #[id = "tsvw"] pub tess_view: EnumParam<TessView>,
    /// Extrusion height for the Extruded view (the cityscape), as a fraction of the
    /// tiling size (world height = height × scale), so 0..1 reads the same at any scale.
    #[id = "tshg"] pub tess_height: FloatParam,
    /// How the extruded prism height varies (uniform / by tile type / radial).
    #[id = "tshm"] pub tess_height_mode: EnumParam<TessHeightMode>,
    /// Beat-driven inflation breathe (#121, Phase 3): on each beat the whole tiling
    /// swells toward one φ-inflation level, then relaxes (rides the beat pulse +
    /// Breath). 0 = off.
    #[id = "tsbi"] pub tess_beat_infl: FloatParam,
    /// Per-tile beat ripple amount: a radial wave (locked to the tempo clock) lifts
    /// each tile's prism + brightens its emissive crest. 0 = off.
    #[id = "tsra"] pub tess_ripple_amt: FloatParam,
    /// Beat-ripple spatial frequency (wave count across the tiling radius).
    #[id = "tsrf"] pub tess_ripple_freq: FloatParam,
    /// Construction method (#121, Phase 4): inflation (substitution) vs cut-and-
    /// project (de Bruijn multigrid — unlocks phason + Ammann–Beenker).
    #[id = "tscn"] pub tess_construct: EnumParam<TilingConstruct>,
    /// Phason amount (cut-and-project): orbits the acceptance window so the tiling
    /// continuously rearranges (flips, never repeating) as the clock advances. 0 = static.
    #[id = "tsph"] pub tess_phason: FloatParam,
    /// Cut-and-project grid range (line-index half-range) — the tile-count / size
    /// dial for the multigrid construction (the perf dial; analogous to inflation depth).
    #[id = "tsgn"] pub tess_grid_n: IntParam,
    /// Ammann bars (#121 follow-up): overlay the de Bruijn grid lines (which *are*
    /// the Ammann bars) on the cut-and-project Edges view. 0 = off.
    #[id = "tsab"] pub tess_ammann: FloatParam,
    /// Hyperbolic {p,q} family: the polygon side count `p` (regular p-gons).
    #[id = "tshp"] pub tess_hyp_p: IntParam,
    /// Hyperbolic {p,q} family: `q` polygons meet at each vertex. Needs 1/p+1/q < 1/2.
    #[id = "tshq"] pub tess_hyp_q: IntParam,

    // --- Mandelbulb generator (raymarched fractal; no surface mode) ---
    /// Fractal exponent — 8 is the classic Mandelbulb; non-integers give exotic
    /// lobing. Beautiful to automate / pulse.
    #[id = "mbpw"] pub mb_power: FloatParam,
    /// Escape-iteration budget — higher reveals finer recursive detail (costlier).
    #[id = "mbit"] pub mb_iter: IntParam,
    /// World size (the bulb's radius in world units) — frames it like the cube field.
    #[id = "mbsc"] pub mb_scale: FloatParam,
    /// Raymarch step budget — quality vs. GPU cost (raise for a crisp silhouette,
    /// lower for performance on a projector).
    #[id = "mbdt"] pub mb_detail: IntParam,
    /// Auto-rotation speed about the vertical, off the global Speed clock (so it
    /// rides the beat via Speed Pulse). 0 = still.
    #[id = "mbsp"] pub mb_spin: FloatParam,
    /// Morph: an azimuth phase added each iteration, advanced off the global Speed
    /// clock — the bulb breathes/unfolds. 0 = a static fractal.
    #[id = "mbmo"] pub mb_morph: FloatParam,
    /// Orbit-trap colour intensity (0 = near-white shading, up = saturated bands).
    #[id = "mbcm"] pub mb_color: FloatParam,
    /// Escape radius (bailout) — 2 is canonical; larger smooths the surface.
    #[id = "mbbo"] pub mb_bailout: FloatParam,

    // --- Creature Engine (#476 Tier 1; raymarched SDF sea creature, no surface mode) ---
    /// Body plan: 0 = bell jelly, 1 = ribbon-swimmer (glowing dorsal rod), 2 =
    /// paddle-finned predator. The geometry is built CPU-side from this index.
    #[id = "crfm"] pub cr_form: IntParam,
    /// World size (the creature's bound radius in world units) — frames it like the cube field.
    #[id = "crsc"] pub cr_scale: FloatParam,
    /// Raymarch step budget — quality vs. GPU cost (raise for a crisp silhouette,
    /// lower for a projector).
    #[id = "crdt"] pub cr_detail: IntParam,
    /// Swim rate: pulses-per-beat of the travelling peristaltic warp (rides the beat
    /// clock, so it stays musical). 0 = frozen.
    #[id = "crsw"] pub cr_swim: FloatParam,
    /// Swim amplitude: lateral displacement of the peristaltic undulation (unit space).
    #[id = "crwa"] pub cr_warp_amp: FloatParam,
    /// Swim frequency: how many wavelengths of the undulation fit along the body.
    #[id = "crwf"] pub cr_warp_freq: FloatParam,
    /// Fresnel rim glow — the luminous bioluminescent silhouette. 0 = off.
    #[id = "crri"] pub cr_rim: FloatParam,
    /// Bioluminescence: multiplier on the per-primitive emissive glow (the bright organs).
    #[id = "crgl"] pub cr_glow: FloatParam,
    // --- Creature Engine Tier 2a (#476): the metachronal wave (running lights) ---
    /// Band speed: pulses-per-beat of the travelling light band running along the
    /// body (rides the beat clock). 0 = frozen band.
    #[id = "crws"] pub cr_wave_speed: FloatParam,
    /// Band count: how many bright bands fit along the body at once.
    #[id = "crwv"] pub cr_wave_freq: FloatParam,
    /// Band sharpness: higher makes each band a tighter, brighter line of light.
    #[id = "crwp"] pub cr_wave_sharp: FloatParam,
    /// Band amount: 0 = steady glow (the Tier-1 look), up brightens the travelling band.
    #[id = "crba"] pub cr_wave_amt: FloatParam,
    // --- Creature Engine Tier 2c (#476): the projected anatomy overlay (diagram) ---
    /// Draw the anatomy diagram over the creature: the spine, a cross-section ring per
    /// body segment, and a limb vector, depth-dimmed where they pass behind the body.
    #[id = "crov"] pub cr_overlay: BoolParam,
    /// Overlay opacity.
    #[id = "croo"] pub cr_overlay_opacity: FloatParam,
    /// Overlay brightness (how hot the diagram lines glow / bloom).
    #[id = "crob"] pub cr_overlay_bright: FloatParam,

    // --- Minimal surfaces / TPMS generator (#127; raymarched isosurface, no surface mode) ---
    /// Which triply-periodic minimal surface: Gyroid (the star), Schwarz P, or
    /// Schwarz D (diamond).
    #[id = "msfa"] pub ms_family: EnumParam<MinimalFamily>,
    /// World size (the structure's radius in world units) — frames it like the cube field.
    #[id = "mssc"] pub ms_scale: FloatParam,
    /// Cell count — how many surface periods (channels) span the structure. Higher
    /// = a finer labyrinth.
    #[id = "msce"] pub ms_cells: FloatParam,
    /// Isolevel: the surface is `F = iso`. Sweeping it swells one channel and pinches
    /// the other — the breathing soap film. 0 = the balanced minimal surface.
    #[id = "msis"] pub ms_iso: FloatParam,
    /// Wall thickness — the soap-film band half-width. 0 = thinnest (a small floor
    /// keeps it from aliasing); larger = a chunky 3-D-printed-gyroid wall.
    #[id = "msth"] pub ms_thickness: FloatParam,
    /// Domain twist about the vertical (radians per unit height) — shears the
    /// labyrinth into a churning spiral. 0 = the straight lattice.
    #[id = "mstw"] pub ms_twist: FloatParam,
    /// Raymarch step budget — quality vs. GPU cost (raise for a crisp surface,
    /// lower for performance on a projector).
    #[id = "msdt"] pub ms_detail: IntParam,
    /// Channel-band colour intensity (0 = near-white shading, up = saturated bands).
    #[id = "mscm"] pub ms_color: FloatParam,
    /// Beat → isolevel breathe amount: the per-beat envelope pushes `iso` so the
    /// channels swell/pinch on tempo. 0 = still (Pulse must be on).
    #[id = "msbi"] pub ms_beat_iso: FloatParam,
    /// (Parametric families) Associate-family bend speed: rides the global Speed
    /// clock to continuously bend a Catenoid through a Helicoid and back (the famous
    /// isometric deformation). 0 = static at the family's home shape.
    #[id = "msbn"] pub ms_bend: FloatParam,
    /// (Parametric families) (u,v) grid resolution — surface smoothness vs. node
    /// count. Higher = a finer membrane / denser bead grid.
    #[id = "msuv"] pub ms_uv_res: IntParam,
    /// (Parametric families) Domain half-extent: how much of the (infinite) surface
    /// to sample — the v-range for the Catenoid/Helicoid, the square half-width for
    /// Enneper. Larger flares the surface out further.
    #[id = "msex"] pub ms_extent: FloatParam,
    /// (Parametric families) Static bend position: parks the Catenoid↔Helicoid
    /// associate morph at a partial blend without animation (0 = the family's home
    /// shape). `bend speed` adds continuous motion on top.
    #[id = "msbp"] pub ms_bend_phase: FloatParam,
    /// (Parametric families) Turns: how many full revolutions the u-domain spans —
    /// 1 = a single turn, higher = a tall multi-turn Helicoid spiral (the Catenoid
    /// just re-wraps). No effect on Enneper.
    #[id = "mstn"] pub ms_turns: FloatParam,
    /// (Implicit/raymarch families) Form resolution: renders these per-pixel forms at
    /// a fraction of the output resolution (the composite upscales), trading edge
    /// sharpness for a big framerate win on the heavy Bubbles/Foam fields. 1 = full
    /// res; 0.5 ≈ 4× fewer rays. Multiplies the global render scale, raymarch only.
    #[id = "msfr"] pub ms_form_res: FloatParam,

    // --- Lens generator (#258 Tier 3; raymarched analytic lens SDF, no surface mode) ---
    /// Focal / curvature dial: the spherical-cap radius the lens is cut from, as a
    /// multiple of the world size (`R = focal · scale`, lensmaker-style). Low =
    /// gently curved (long focal, nearly flat); high = strongly curved (short focal,
    /// a fat lens). Approximate — geometry to aim the tracer at, not a calibrated optic.
    #[id = "lnfo"] pub lens_focal: FloatParam,
    /// Clear aperture (lens radius) as a fraction of the world size — the disc the
    /// caps are trimmed to by the aperture stop.
    #[id = "lnap"] pub lens_aperture: FloatParam,
    /// Centre thickness (half) as a fraction of the world size — how far apart the
    /// two caps sit (the axial glass down the middle).
    #[id = "lnth"] pub lens_thickness: FloatParam,
    /// Plano-convex toggle: off = symmetric biconvex (two spherical caps); on = one
    /// flat face + one convex cap.
    #[id = "lnpl"] pub lens_plano: BoolParam,
    /// World size (the lens's overall radius in world units) — frames it like the
    /// cube field / the other raymarch generators.
    #[id = "lnsc"] pub lens_scale: FloatParam,
    /// Raymarch step budget — quality vs. GPU cost (a true SDF converges fast).
    #[id = "lndt"] pub lens_detail: IntParam,

    // --- Kaleidoscopic Fractal (KIFS) generator (fullscreen field; no surface mode) ---
    /// Geometry the field lives on: Euclidean (flat), Hyperbolic (Poincaré disk), or
    /// Quasicrystal (aperiodic). Upstream of everything else, which composes on top.
    #[id = "kfsp2"] pub kf_space: EnumParam<KifsSpace>,
    /// N-fold kaleidoscopic (mirror) symmetry — the rose-window sector count.
    /// **Continuous**: fractional values crossfade the two nearest foldings, so the
    /// symmetry morphs smoothly (petals born/reabsorbed) when swept or automated.
    #[id = "kfse"] pub kf_sectors: FloatParam,
    /// Inversion-fold constant c in `z = abs(z)/dot(z,z) − c`. The single biggest
    /// shape lever — gorgeous to automate/pulse (it re-grows the whole fractal).
    #[id = "kffo"] pub kf_fold: FloatParam,
    /// IFS iteration count — more reveals deeper self-similar nesting.
    #[id = "kfit"] pub kf_iter: IntParam,
    /// Extra rotation applied each IFS iteration (radians) — shears the nesting.
    #[id = "kfir"] pub kf_iter_rot: FloatParam,
    /// Overall rotation speed off the global Speed clock (rides the Speed Pulse).
    #[id = "kfsp"] pub kf_spin: FloatParam,
    /// Breathing-zoom amount (the field pulses in/out); rides the global clock.
    #[id = "kfbr"] pub kf_breathe: FloatParam,
    /// Base zoom into the field.
    #[id = "kfzo"] pub kf_zoom: FloatParam,
    /// Tunnel projection: wrap the fractal around a receding bore (breathing →
    /// forward motion) instead of the flat rose-window.
    #[id = "kftu"] pub kf_tunnel: BoolParam,
    /// Radial ray count (0 = none) — the focal spokes (flat projection).
    #[id = "kfry"] pub kf_rays: IntParam,
    /// Concentric-ring gain (flat projection).
    #[id = "kfrg"] pub kf_ring: FloatParam,
    /// Overall output gain (drives bloom).
    #[id = "kfgl"] pub kf_glow: FloatParam,
    /// Palette phase (a static hue offset into the selected scheme).
    #[id = "kfhu"] pub kf_hue: FloatParam,
    /// Fractal engine — the core fold that builds the pattern. Each is a
    /// fundamentally different self-similar structure (inversion, mandelbox,
    /// sierpinski, log-spiral, kleinian).
    #[id = "kfpt"] pub kf_pattern: EnumParam<KifsPattern>,
    /// Colour scheme (a bank of cosine palettes; `Spectral` = the original look).
    #[id = "kfpl"] pub kf_palette: EnumParam<KifsPalette>,
    /// Colour-cycle rate — how fast the palette sweeps, independent of the spin.
    #[id = "kfcs"] pub kf_color_speed: FloatParam,
    /// Churn rate — the fractal's intrinsic self-animation (per-iteration tumbling +
    /// ring/ray breathing), separate from spin / colour speed / tunnel flow. 1 =
    /// the natural rate, 0 = frozen (the field only moves under spin/flow then),
    /// negative = reverse.
    #[id = "kfch"] pub kf_churn: FloatParam,
    /// E8 8-D rotation rate (E8 space only): tumbles the viewing 2-plane through
    /// 8-space so all 240 roots flow between configurations (rings split/merge — the
    /// real rotating-E8 morph). 0 = the static 8-ring projection (with shell churn);
    /// higher = faster tumble (and the heavier 240-point render path).
    #[id = "kfe8"] pub kf_e8_flow: FloatParam,
    /// Domain warp — a low-frequency swirl layered on the fractal for organic,
    /// non-self-similar variation (0 = off, the clean fractal).
    #[id = "kfwp"] pub kf_warp: FloatParam,
    /// Tunnel forward-motion speed (only in tunnel mode) — flies the camera down
    /// the bore; the receding rings stream past at this rate.
    #[id = "kffl"] pub kf_flow: FloatParam,
    /// Petal-motif count — the rhodonea curve's lobe number (shape variety).
    #[id = "kfpc"] pub kf_petals: IntParam,
    /// Emission contrast — >1 punches the bright filaments and deepens the gaps,
    /// <1 lifts the mids into a softer glow.
    #[id = "kfct"] pub kf_contrast: FloatParam,
    /// Motif crispness — 0 = soft glowing filaments, 1 = thin razor lines.
    #[id = "kfsh"] pub kf_sharp: FloatParam,
    /// Invert (figure ↔ ground): 0 = bright structure on a dark field, 1 = dark
    /// bands carved out of a bright field. Blendable.
    #[id = "kfiv"] pub kf_invert: FloatParam,
    /// Chromatic dispersion — samples the field at slightly different scales per
    /// RGB channel so bright edges split into a prism rim (light through cut glass).
    /// 0 = off. Higher = more rainbow fringe (and more shader cost; off by default).
    #[id = "kfds"] pub kf_dispersion: FloatParam,
    /// Render dimensionality: Field (2-D) / Relief (3-D heightfield) / Conformal
    /// (3-D sphere-inversion solid). See `KifsView`.
    #[id = "kfvw"] pub kf_view: EnumParam<KifsView>,
    /// Relief height — how far the field's brightness lifts off the base (Relief mode).
    #[id = "kfrl"] pub kf_relief: FloatParam,
    /// 3-D camera elevation (radians): low = grazing/oblique, high = top-down/front.
    /// Shared by Relief and Conformal modes.
    #[id = "kfre"] pub kf_relief_elev: FloatParam,
    /// 3-D raymarch step budget — perf vs. crispness (drop on a projector). Shared.
    #[id = "kfrt"] pub kf_relief_steps: IntParam,
    /// 3-D rim/specular shine on the lit surface (0 = matte emission only). Shared.
    #[id = "kfrn"] pub kf_relief_shine: FloatParam,

    // --- Scene Kaleidoscope (#361 Tier 1): a post-stage kaleidoscopic fold of the
    //     resolved HDR scene — folds the live PBR render of ANY generator + surface,
    //     run before bloom/composite. Off by default (byte-identical). A captured Look.
    #[id = "kalo"] pub kal_on: BoolParam,
    #[id = "kals"] pub kal_sectors: FloatParam,
    #[id = "kalm"] pub kal_mode: EnumParam<KaleidoMode>,
    #[id = "kalp"] pub kal_spin: FloatParam,
    #[id = "kalr"] pub kal_roll: FloatParam,
    #[id = "kalz"] pub kal_zoom: FloatParam,
    #[id = "kalx"] pub kal_center_x: FloatParam,
    #[id = "kaly"] pub kal_center_y: FloatParam,
    #[id = "kalb"] pub kal_mix: FloatParam,
    #[id = "kalt"] pub kal_twist: FloatParam,
    #[id = "kalh"] pub kal_tint_hue: FloatParam,
    #[id = "kala"] pub kal_tint_amt: FloatParam,
    #[id = "kalw"] pub kal_seam: FloatParam,

    // --- Quantitative instrumentation (#391 Tier 1): placeable field probes + an
    //     energy ledger + a Poynting-flux surface, read from the same kernels the
    //     visual draws (Maxwell/Acoustic/Cavity generators). A captured Look; the HUD
    //     is off by default (byte-identical), so these are inert until enabled. ---
    /// Draw the instrumentation HUD (probe / ledger / flux read-outs). Master gate.
    #[id = "insh"] pub instr_hud: BoolParam,
    /// Read the point probe (E/B or pressure/velocity + energy + Poynting at a point).
    #[id = "insp"] pub instr_probe_on: BoolParam,
    #[id = "inpx"] pub instr_probe_x: FloatParam,
    #[id = "inpy"] pub instr_probe_y: FloatParam,
    #[id = "inpz"] pub instr_probe_z: FloatParam,
    /// Integrate the energy ledger (E↔B / compression↔kinetic trade + total) over a box.
    #[id = "insl"] pub instr_ledger_on: BoolParam,
    /// Ledger box half-extent (world units).
    #[id = "inlh"] pub instr_ledger_half: FloatParam,
    /// Ledger sample resolution (n per axis).
    #[id = "inlr"] pub instr_ledger_res: FloatParam,
    /// Integrate the Poynting flux through a placeable square patch.
    #[id = "insf"] pub instr_flux_on: BoolParam,
    #[id = "infx"] pub instr_flux_x: FloatParam,
    #[id = "infy"] pub instr_flux_y: FloatParam,
    #[id = "infz"] pub instr_flux_z: FloatParam,
    /// Flux patch half-size (world units).
    #[id = "infs"] pub instr_flux_size: FloatParam,
    /// Flux patch orientation.
    #[id = "infa"] pub instr_flux_axis: EnumParam<FluxAxis>,
    /// Flux patch sample resolution (n per side).
    #[id = "infr"] pub instr_flux_res: FloatParam,
    /// Append a probe-trace CSV row each frame (to `ipc::probe_csv_path()`).
    #[id = "incl"] pub instr_csv_log: BoolParam,
    // HUD presentation: a rounded backing panel (contrast over the render), overall
    // size, and which corner it docks to.
    /// Backing-panel opacity (0 = no panel, 1 = solid).
    #[id = "inpo"] pub instr_panel_opacity: FloatParam,
    /// Panel corner rounding (0 = square, 1 = pill).
    #[id = "inpb"] pub instr_panel_bevel: FloatParam,
    /// Overall HUD size — scales the font + panel together.
    #[id = "inhz"] pub instr_hud_scale: FloatParam,
    /// Which corner the HUD panel docks to.
    #[id = "inhd"] pub instr_hud_dock: EnumParam<HudDock>,

    // --- Surface (how nodes become geometry) ---
    #[id = "surf"] pub surface_mode: EnumParam<SurfaceMode>,
    // Origin mode for the Original cube-field: Corner (grid corner at the origin,
    // the historical look) vs Centered (grid symmetric about the origin).
    #[id = "orig"] pub origin_mode: EnumParam<OriginMode>,
    /// Node bevel: rounds the cube geometry (Original + Flow-Aligned) from a sharp
    /// cube (0) through a wide rounded cube to a full sphere (1). Drives the vertex
    /// shader's rounded-box morph (`Uniforms.shape.x`); 0 = today's sharp cube
    /// (byte-identical). No effect on Swept-Tubes / raymarch surfaces.
    #[id = "bevl"] pub bevel: FloatParam,

    // --- Procedural / texture-mapped materials (#472 Tier 1) ---
    /// Master switch for the PBR **material texture set**: when on, the generator
    /// cubes are shaded from real albedo / normal / roughness / metallic / AO maps
    /// (loaded via "Load Material…") instead of the scalar-uniform PBR path. Off (the
    /// default) = byte-identical to today. Captured **Look**.
    #[id = "maten"] pub mat_enable: BoolParam,
    /// How the 2-D maps project onto the (UV-less) geometry — Triplanar (world),
    /// Planar world-XZ, or Planar object-XY. Captured **Look**.
    #[id = "matpj"] pub mat_projection: EnumParam<MatProjection>,
    /// Texture scale — world units per map tile (higher = finer tiling). Captured **Look**.
    /// (#472: the maps feed the unified material pipeline directly — the roughness /
    /// metallic / AO / normal channels drive the ONE material system's roughness /
    /// metallic / ambient / normal, so there are deliberately NO per-map quality knobs.)
    #[id = "matsc"] pub mat_scale: FloatParam,

    // --- Procedural material — single-layer noise graph (#472 Tier 2) ---
    /// Master switch: bake the procedural noise layer into its routed channel
    /// texture (a compute pass) instead of loading it from PNG. Off (default) →
    /// the Tier-1 / scalar path is untouched (byte-identical). Captured **Look**.
    #[id = "mpon"] pub mp_enable: BoolParam,
    /// Which noise/pattern generator this layer bakes. Captured **Look**.
    #[id = "mpno"] pub mp_noise: EnumParam<MatNoise>,
    /// Which PBR channel the layer writes (albedo maps through the gradient; the
    /// rest write a scalar). Captured **Look**.
    #[id = "mpch"] pub mp_channel: EnumParam<MatChannel>,
    /// Noise scale — tiles across the bake (higher = finer). Captured **Look**.
    #[id = "mpsc"] pub mp_scale: FloatParam,
    /// Rotation of the noise field (radians). Captured **Look**.
    #[id = "mpro"] pub mp_rotation: FloatParam,
    /// Noise field offset X (pans the pattern). Captured **Look**.
    #[id = "mpox"] pub mp_offset_x: FloatParam,
    /// Noise field offset Y. Captured **Look**.
    #[id = "mpoy"] pub mp_offset_y: FloatParam,
    /// Fractal octaves (FBM / Turbulence / Ridged; ignored by the single-scale
    /// noises). Captured **Look**.
    #[id = "mpoc"] pub mp_octaves: IntParam,
    /// Fractal lacunarity — frequency multiplier per octave. Captured **Look**.
    #[id = "mpla"] pub mp_lacunarity: FloatParam,
    /// Fractal gain — amplitude multiplier per octave. Captured **Look**.
    #[id = "mpga"] pub mp_gain: FloatParam,
    /// Domain-warp amount — perturbs the sample position by a second noise (0 = off).
    /// Captured **Look**.
    #[id = "mpwa"] pub mp_warp: FloatParam,
    /// Output contrast around 0.5 (1 = linear). Captured **Look**.
    #[id = "mpco"] pub mp_contrast: FloatParam,
    /// Output gamma (1 = linear). Captured **Look**.
    #[id = "mpgm"] pub mp_gamma: FloatParam,
    /// Input remap low — noise values ≤ this map to 0. Captured **Look**.
    #[id = "mprl"] pub mp_remap_lo: FloatParam,
    /// Input remap high — noise values ≥ this map to 1. Captured **Look**.
    #[id = "mprh"] pub mp_remap_hi: FloatParam,
    /// Invert the baked field. Captured **Look**.
    #[id = "mpiv"] pub mp_invert: BoolParam,
    /// Noise seed (shifts the hash lattice). Captured **Look**.
    #[id = "mpsd"] pub mp_seed: IntParam,
    /// Bake resolution (per-quality, but captured as a Look for reproducibility).
    #[id = "mprs"] pub mp_res: EnumParam<BakeRes>,
    /// Albedo gradient — low colour (linear RGB), the noise-0 end. Captured **Look**.
    #[id = "mplr"] pub mp_lo_r: FloatParam,
    #[id = "mplg"] pub mp_lo_g: FloatParam,
    #[id = "mplb"] pub mp_lo_b: FloatParam,
    /// Albedo gradient — high colour (linear RGB), the noise-1 end. Captured **Look**.
    #[id = "mphr"] pub mp_hi_r: FloatParam,
    #[id = "mphg"] pub mp_hi_g: FloatParam,
    #[id = "mphb"] pub mp_hi_b: FloatParam,

    // --- Procedural material — overlay layer 2 (#472 Tier 3) ---
    /// Enable overlay layer 2 (composites onto layer 1's output for the same
    /// channel). Off (default) → the Tier-2 single-layer path. Captured **Look**.
    #[id = "m2on"] pub mp2_enable: BoolParam,
    /// How overlay layer 2 blends onto the layer below it. Captured **Look**.
    #[id = "m2bl"] pub mp2_blend: EnumParam<BlendMode>,
    /// Overlay layer 2 noise/pattern generator. Captured **Look**.
    #[id = "m2no"] pub mp2_noise: EnumParam<MatNoise>,
    /// Overlay layer 2 target channel. Captured **Look**.
    #[id = "m2ch"] pub mp2_channel: EnumParam<MatChannel>,
    #[id = "m2sc"] pub mp2_scale: FloatParam,
    #[id = "m2ro"] pub mp2_rotation: FloatParam,
    #[id = "m2ox"] pub mp2_offset_x: FloatParam,
    #[id = "m2oy"] pub mp2_offset_y: FloatParam,
    #[id = "m2oc"] pub mp2_octaves: IntParam,
    #[id = "m2la"] pub mp2_lacunarity: FloatParam,
    #[id = "m2ga"] pub mp2_gain: FloatParam,
    #[id = "m2wa"] pub mp2_warp: FloatParam,
    #[id = "m2co"] pub mp2_contrast: FloatParam,
    #[id = "m2gm"] pub mp2_gamma: FloatParam,
    #[id = "m2rl"] pub mp2_remap_lo: FloatParam,
    #[id = "m2rh"] pub mp2_remap_hi: FloatParam,
    #[id = "m2iv"] pub mp2_invert: BoolParam,
    #[id = "m2sd"] pub mp2_seed: IntParam,
    /// Overlay layer 2 albedo gradient (linear RGB). Captured **Look**.
    #[id = "m2lr"] pub mp2_lo_r: FloatParam,
    #[id = "m2lg"] pub mp2_lo_g: FloatParam,
    #[id = "m2lb"] pub mp2_lo_b: FloatParam,
    #[id = "m2hr"] pub mp2_hi_r: FloatParam,
    #[id = "m2hg"] pub mp2_hi_g: FloatParam,
    #[id = "m2hb"] pub mp2_hi_b: FloatParam,

    // --- Procedural material — derived maps (#472 Tier 3, the correlation principle) ---
    /// Derive a **normal map** from the height field (or albedo luminance) so it
    /// agrees with the surface. Off (default) → no derived normal. Captured **Look**.
    #[id = "mdno"] pub mat_derive_normal: BoolParam,
    /// Derive an **AO map** (cavity of the height field). Captured **Look**.
    #[id = "mdao"] pub mat_derive_ao: BoolParam,
    /// Pseudo-height source for the derived normal: off = the baked height channel,
    /// on = albedo luminance (the "normal from albedo" path). Captured **Look**.
    #[id = "mdns"] pub mat_normal_source_albedo: BoolParam,
    /// Derived-normal bump strength. Captured **Look**.
    #[id = "mdst"] pub mat_derive_normal_strength: FloatParam,
    /// Derived-AO darkening amount. Captured **Look**.
    #[id = "mdas"] pub mat_derive_ao_strength: FloatParam,
    /// Derived-AO cavity radius (texels). Captured **Look**.
    #[id = "mdar"] pub mat_derive_ao_radius: FloatParam,

    // --- Procedural material — live: animation + displacement (#472 Tier 5) ---
    /// Animate the material — the visual injects a time term into the baked layers so
    /// the noise flows / evolves (throttled re-bake). Off (default) → static.
    /// Captured **Look**.
    #[id = "mano"] pub mat_anim_enable: BoolParam,
    /// Animation speed (cycles/second of the injected time term). Captured **Look**.
    #[id = "mans"] pub mat_anim_speed: FloatParam,
    /// Animation mode — Drift (pan along flow) / Evolve (circular churn) / Rotate
    /// (spin the field). Captured **Look**.
    #[id = "manm"] pub mat_anim_mode: EnumParam<AnimMode>,
    /// Flow direction X (Drift). Captured **Look**.
    #[id = "manx"] pub mat_flow_x: FloatParam,
    /// Flow direction Y (Drift). Captured **Look**.
    #[id = "many"] pub mat_flow_y: FloatParam,
    /// Height → vertex **displacement** amount on the generator cubes (0 = off; the
    /// baked/loaded height offsets each vertex along its normal). Captured **Look**.
    #[id = "mdsp"] pub mat_displace: FloatParam,

    // --- Plexus surface mode (proximity web; all × node spacing, scale-invariant) ---
    /// Link radius as a multiple of the field's node spacing — how far a node
    /// reaches to find neighbours. Higher = denser web. Shared.
    #[id = "plxr"] pub plexus_radius: FloatParam,
    /// Max neighbours wired per node (caps clutter in dense clusters). Shared.
    #[id = "plxk"] pub plexus_links: FloatParam,
    /// Strut thickness as a multiple of node spacing (thin filaments). Shared.
    #[id = "plxs"] pub plexus_strut: FloatParam,
    /// Node marker size as a multiple of node spacing. Shared.
    #[id = "plxm"] pub plexus_marker: FloatParam,

    // --- Plexus Tier 2: impostor rendering + independent node/edge materials ---
    /// Render the web as GPU impostors (sphere nodes + capsule-tube edges) instead
    /// of Tier-1 instanced cubes. Shared.
    #[id = "plxi"] pub plexus_impostor: BoolParam,
    /// Draw the edges (capsule impostors). Off = nodes only. Shared.
    #[id = "plxe"] pub plexus_edges: BoolParam,
    /// Node sphere-impostor radius (× node spacing). Shared.
    #[id = "plnr"] pub plexus_node_radius: FloatParam,
    /// Edge tube-impostor radius (× node spacing). Shared.
    #[id = "pler"] pub plexus_edge_radius: FloatParam,
    /// Node impostor material — full independent control.
    #[id = "plnt"] pub plexus_node_type: EnumParam<MaterialType>,
    #[id = "plnm"] pub plexus_node_metallic: FloatParam,
    #[id = "plng"] pub plexus_node_rough: FloatParam,
    #[id = "plno"] pub plexus_node_ior: FloatParam,
    #[id = "plnh"] pub plexus_node_hue: FloatParam,
    #[id = "plns"] pub plexus_node_sat: FloatParam,
    #[id = "plnv"] pub plexus_node_val: FloatParam,
    #[id = "plne"] pub plexus_node_emissive: FloatParam,
    /// Edge impostor material — independent of the node material.
    #[id = "plet"] pub plexus_edge_type: EnumParam<MaterialType>,
    #[id = "plem"] pub plexus_edge_metallic: FloatParam,
    #[id = "pleg"] pub plexus_edge_rough: FloatParam,
    #[id = "pleo"] pub plexus_edge_ior: FloatParam,
    #[id = "pleh"] pub plexus_edge_hue: FloatParam,
    #[id = "ples"] pub plexus_edge_sat: FloatParam,
    #[id = "plev"] pub plexus_edge_val: FloatParam,
    #[id = "plee"] pub plexus_edge_emissive: FloatParam,

    // --- Plexus Tier 3: beat-driven signal propagation (rides the impostor path) ---
    /// A bright activation shell radiates from the web centre on the beat, firing the
    /// node/edge impostors it crosses. Shared.
    #[id = "plsg"] pub plexus_signal: BoolParam,
    #[id = "plzs"] pub plexus_signal_speed: FloatParam,
    #[id = "plsn"] pub plexus_signal_gain: FloatParam,
    #[id = "plzw"] pub plexus_signal_width: FloatParam,

    // --- Plexus Tier-1 shape morph (node markers + connecting struts) ---
    /// Node marker shape: 0 = sharp cube → rounded cube → 1 = sphere. Shared.
    #[id = "pnsh"] pub plexus_node_shape: FloatParam,
    /// Strut cross-section: 0 = sharp square → 1 = circle. Shared.
    #[id = "pesh"] pub plexus_edge_shape: FloatParam,

    // --- Plexus overlay: wrap the web as an outer shell around ANOTHER surface ---
    /// Enable the plexus as an OVERLAY (like the Particle Aura / Water). Reads the
    /// active generator's node cloud non-destructively and draws only its outer
    /// shell as a plexus web around whatever surface is on. Reuses every Plexus look
    /// param above. Off → byte-identical.
    #[id = "plxov"] pub plexus_overlay_on: BoolParam,
    /// Grows the shell outward from the cloud centroid (1 = hug the surface).
    #[id = "plssc"] pub plexus_shell_scale: FloatParam,
    /// Shell depth: keep the outer `depth` fraction of each direction's radial extent
    /// (a stable band — bigger = a thicker, steadier rind). Replaces the old node-count.
    #[id = "plsth"] pub plexus_shell_depth: FloatParam,
    /// Directional-bin resolution (higher = a finer, denser shell outline).
    #[id = "plsbn"] pub plexus_shell_bins: FloatParam,

    // --- Animation (per-axis speed lives in `rot_mod_*`; `inc_scale` is global) ---
    // Effective global speed = `inc_scale` (0..1 dial) × 10^`speed_exp` (the decade).
    // Splitting them lets the 0..1 dial stay usable while reaching very slow speeds.
    #[id = "anim"] pub animate: BoolParam,
    #[id = "incs"] pub inc_scale: FloatParam,
    #[id = "incp"] pub speed_exp: IntParam,

    // --- Direct lighting (analytic key + fill; `ambient` scales the IBL term) ---
    #[id = "amb"] pub ambient: FloatParam,
    #[id = "key"] pub key_intensity: FloatParam,
    #[id = "fill"] pub fill_intensity: FloatParam,
    #[id = "elev"] pub elevation: FloatParam,
    #[id = "azim"] pub azimuth: FloatParam,
    #[id = "glow"] pub glow: FloatParam,
    /// Material emissive: emit the surface's own colour (its Hue/Saturation/Value)
    /// into HDR — a dedicated, high-range self-emission on top of `glow`, so the
    /// geometry glows in its OWN colour (not washed to white) and blooms. 0 = off.
    #[id = "emis"] pub mat_emissive: FloatParam,
    #[id = "opac"] pub opacity: FloatParam,

    // --- Pulse ---
    #[id = "puls"] pub pulse: BoolParam,
    #[id = "tsyn"] pub tempo_sync: BoolParam,
    #[id = "tmpo"] pub tempo: FloatParam,
    /// What feeds the pulse envelope: the synthetic beat clock or the audio bass.
    #[id = "psrc"] pub pulse_source: EnumParam<PulseSource>,

    // --- Audio reactive (input analysis on the audio thread) ---
    /// Master enable: run the FFT band analysis on the track's input. Off = no
    /// analysis (zero CPU), and the audio bands stay 0.
    #[id = "arac"] pub audio_react: BoolParam,
    /// Input sensitivity for the analysis (linear). Turn up for quiet room sound.
    #[id = "agan"] pub audio_gain: FloatParam,
    /// Envelope follower attack (ms): how fast a band level rises on a transient.
    #[id = "aatk"] pub audio_attack: FloatParam,
    /// Envelope follower release (ms): how slowly it falls between hits — the
    /// "breathing" knob (long = liquid swells, short = snappy strobe).
    #[id = "arel"] pub audio_release: FloatParam,

    // --- Calibrated / analytical metering (#333 Tiers 1–2) ---
    /// RTA fractional-octave resolution for the calibrated spectrum.
    #[id = "mres"] pub meter_res: EnumParam<SpectrumMode>,
    /// Frequency weighting (Z/A/C) applied on the calibrated dB axis.
    #[id = "mwgt"] pub meter_weight: EnumParam<MeterWeighting>,
    /// RTA time-averaging (fast/slow/peak-hold/Leq).
    #[id = "mavg"] pub meter_averaging: EnumParam<MeterAveraging>,
    /// Draw the calibrated numeric HUD (LUFS / dBTP / correlation) in the visual.
    #[id = "mhud"] pub meter_hud: BoolParam,

    // --- Analyzer / Calibrated instrument mode (#333 Tier 3) ---
    /// Duo-Field drive mode: Expressive (gain·RMS, today) or Calibrated (a
    /// reproducible law of the measured LUFS). Expressive → byte-identical.
    #[id = "anmo"] pub analytical_mode: EnumParam<AnalyticalMode>,
    /// Delivery loudness target (LUFS): the reference the field reaches full drive at,
    /// and the "over/under target" horizon in the instrument HUD (−14 streaming,
    /// −23 EBU). Calibrated mode only.
    #[id = "antl"] pub an_target_lufs: FloatParam,
    /// Calibrated drive floor (LUFS): the loudness at which the field goes silent
    /// (the bottom of the linear-in-dB drive curve). Calibrated mode only.
    #[id = "anfl"] pub an_floor_lufs: FloatParam,
    /// True-peak alarm ceiling (dBTP): dBTP above this flashes an over-alarm (−1
    /// streaming, −2 broadcast).
    #[id = "antp"] pub an_tp_ceiling: FloatParam,
    /// Correlation alarm threshold: stereo correlation below this flags a phase
    /// warning (anti-phase / mono-cancelling).
    #[id = "anco"] pub an_corr_alarm: FloatParam,
    /// Show the calibrated **instrument** HUD (delivery-target over/under + true-peak
    /// and phase alarms) in the visual.
    #[id = "anrf"] pub an_reference_hud: BoolParam,

    // --- Field Volume (#348): density-cloud render for SurfaceMode::Volume ---
    /// Density source: Legacy (today's node metaball, byte-identical) / Auto (field
    /// generators → analytic energy, others → smoothed nodes) / Field-baked /
    /// Smoothed node.
    #[id = "fvsr"] pub fv_source: EnumParam<FieldVolSource>,
    /// Smoothing-kernel width scale for the node bake (× the metaball radius). 1 =
    /// neutral; higher = softer, cloudier density.
    #[id = "fvsm"] pub fv_smooth: FloatParam,
    /// Tier-2 exposure in dB added to the cloud brightness (0 = neutral).
    #[id = "fvex"] pub fv_exposure_db: FloatParam,
    /// Key the cloud density/emission to the **calibrated** loudness
    /// (`calibrated_drive(LUFS)²`) instead of the plain `volume[]` dials. Off =
    /// audio-independent (the plain dials).
    #[id = "fvca"] pub fv_calibrate: BoolParam,
    /// Extra density/emission multiplier (1 = neutral).
    #[id = "fvga"] pub fv_gain: FloatParam,
    /// Render the field generator as **volumetric field-lines** (a dense cloud of thin
    /// glowing streamlines of both channels — the tube-mode flow, without chunky tubes)
    /// instead of the energy cloud. Acoustic only (for now). Off = the cloud/lattice.
    #[id = "fvln"] pub fv_lines: BoolParam,
    /// Streamlines traced per channel (line density). Higher = denser, more volumetric.
    #[id = "fvld"] pub fv_line_density: FloatParam,
    /// Field-line filament thickness (thin = wispy filaments, thick = ribbons).
    #[id = "fvlt"] pub fv_line_thickness: FloatParam,

    // --- Calibrated colour (#349): a cross-cutting colour-means-a-level tint ---
    /// Colour mode: Aesthetic (today's tint, byte-identical) or Calibrated (colour =
    /// a measured dB level via a legend-backed perceptual LUT).
    #[id = "clmo"] pub col_mode: EnumParam<ColourMode>,
    /// Low end of the dB window mapped to the LUT (e.g. −60 dBFS → LUT 0).
    #[id = "cllo"] pub col_lo_db: FloatParam,
    /// High end of the dB window mapped to the LUT (e.g. 0 dBFS → LUT 1).
    #[id = "clhi"] pub col_hi_db: FloatParam,
    /// Perceptually-uniform LUT: Turbo / Viridis / Inferno / Magma.
    #[id = "cllt"] pub col_lut: EnumParam<CalLut>,
    /// What "measured level" drives the colour: Auto (field → band dBFS, else LUFS) /
    /// Band / Loudness.
    #[id = "clsc"] pub col_source: EnumParam<CalColourSource>,
    /// Blend of the calibrated tint over the aesthetic tint (0..1).
    #[id = "clam"] pub col_amount: FloatParam,

    // --- Camera (auto-orbit) ---
    #[id = "cpth"] pub cam_path: EnumParam<CamPath>,
    #[id = "cspd"] pub cam_speed: FloatParam,
    #[id = "ckik"] pub cam_kick: FloatParam,
    #[id = "cdmp"] pub cam_damping: FloatParam,
    #[id = "camt"] pub cam_amount: FloatParam,
    /// Ride the per-beat momentum kick (today's lurch-on-the-beat) or glide
    /// smoothly on the bar clock. Off = cinematic (no wiggle with the audio).
    #[id = "cmom"] pub cam_beat_momentum: BoolParam,

    // --- Camera shot sequencer (#307 Tier 1): progress through moves on bar marks ---
    /// Master: cycle through camera moves on musical boundaries instead of one fixed path.
    #[id = "cseq"] pub cam_seq_enabled: BoolParam,
    /// How long each shot holds before changing.
    #[id = "csbp"] pub cam_bars_per_shot: EnumParam<BarPeriod>,
    /// Order the shots are chosen in (Series / Random).
    #[id = "csor"] pub cam_seq_order: EnumParam<CamOrder>,
    /// Hand-off style: Glide (ease between shots) or Cut (snap on the downbeat).
    #[id = "cstr"] pub cam_transition: EnumParam<CamTransition>,
    /// Glide length in bars (ignored for Cut).
    #[id = "cstb"] pub cam_transition_bars: FloatParam,

    // --- Decoupled dolly (#307 Tier 1): in/out breath at its own musical rate ---
    /// Dolly period in bars (one full in-and-out breath).
    #[id = "cdpb"] pub cam_dolly_period: FloatParam,
    /// Dolly depth: fraction of the radius the breath swings (0 = inert).
    #[id = "cddp"] pub cam_dolly_depth: FloatParam,
    /// Dolly waveform over its period.
    #[id = "cdwv"] pub cam_dolly_wave: EnumParam<DollyWave>,

    // --- Beat/bar clock source (#307 Tier 1) ---
    /// Where the beat clock gets its BPM (Host transport / detect from Audio / Manual dial).
    #[id = "tsrc"] pub tempo_source: EnumParam<TempoSource>,
    /// Beats per bar (assumed 4/4 by default) — the bar clock the sequencer counts against.
    #[id = "bpbr"] pub beats_per_bar: IntParam,

    // --- Preset-recall timing (#354) — meta controls, NOT preset-captured ---
    /// Beat division a **Scene** recall snaps to (Instant = immediate).
    #[id = "pstm"] pub scene_preset_timing: EnumParam<PresetDivision>,
    /// Beat division an individual Scene-component (Generator/Motion/Environment/
    /// Look) recall snaps to. Audio/Synth/Settings always recall instantly.
    #[id = "pctm"] pub component_preset_timing: EnumParam<PresetDivision>,
    /// #356: enable the Four-Quadrant Performance Controller (a Launchpad-style
    /// pad surface drives beat-quantized component recalls). Off = the pad
    /// surface is ignored by the controller layer, so Key Map / synth-note
    /// behaviour is untouched. A meta control, NOT preset-captured.
    #[id = "pfen"] pub perf_enable: BoolParam,

    // --- Camera framing axes (#307 Tier 2): roll + FOV ---
    /// Camera roll / dutch tilt in degrees (rotates the up-vector about the view axis).
    #[id = "crol"] pub cam_roll: FloatParam,
    /// Base vertical field of view in degrees (45 = today's look).
    #[id = "cfov"] pub cam_fov: FloatParam,
    /// Dolly-zoom (Hitchcock): couple FOV to the dolly breath so pushing in widens the
    /// lens. 0 = fixed FOV; up = more vertigo warp.
    #[id = "cfvz"] pub cam_fov_dolly: FloatParam,

    // --- Sequencer Tier 2 richness (#307) ---
    /// Chance (0..1) the sequencer holds the current shot for another period instead
    /// of changing — so the choreography isn't perfectly predictable.
    #[id = "chld"] pub cam_hold_prob: FloatParam,
    /// Phrase-locked facing: snap the move phase to a canonical start at each shot
    /// boundary so the camera faces consistently on the downbeat.
    #[id = "cphl"] pub cam_phrase_lock: BoolParam,
    /// Sequencer blend (0..1): how much the sequencer contributes on top of the
    /// always-on orbit-cam (`cam_path`). 0 = fully organic-math (the plain orbit
    /// path), 1 = fully sequencer. Lets the base orbit keep running while the
    /// sequencer layers its changing moves in.
    #[id = "csmx"] pub cam_seq_mix: FloatParam,

    // --- Camera storyboard (#307 Tier 3): an authored, saveable playlist of shots ---
    /// Master: play the authored storyboard (overrides the auto sequencer).
    #[id = "cstl"] pub cam_story_enabled: BoolParam,
    /// How many of the 4 shot slots are in the playlist.
    #[id = "cscn"] pub cam_story_count: IntParam,
    /// Playback order across the storyboard slots.
    #[id = "csmd"] pub cam_story_mode: EnumParam<CamOrder>,
    /// RNG seed so a Random/Shuffle/Weighted storyboard replays identically.
    #[id = "cssd"] pub cam_story_seed: IntParam,
    // Shot 0
    #[id = "cs0p"] pub cam_shot0_path: EnumParam<CamPath>,
    #[id = "cs0b"] pub cam_shot0_bars: EnumParam<BarPeriod>,
    #[id = "cs0r"] pub cam_shot0_radius: FloatParam,
    // Shot 1
    #[id = "cs1p"] pub cam_shot1_path: EnumParam<CamPath>,
    #[id = "cs1b"] pub cam_shot1_bars: EnumParam<BarPeriod>,
    #[id = "cs1r"] pub cam_shot1_radius: FloatParam,
    // Shot 2
    #[id = "cs2p"] pub cam_shot2_path: EnumParam<CamPath>,
    #[id = "cs2b"] pub cam_shot2_bars: EnumParam<BarPeriod>,
    #[id = "cs2r"] pub cam_shot2_radius: FloatParam,
    // Shot 3
    #[id = "cs3p"] pub cam_shot3_path: EnumParam<CamPath>,
    #[id = "cs3b"] pub cam_shot3_bars: EnumParam<BarPeriod>,
    #[id = "cs3r"] pub cam_shot3_radius: FloatParam,

    // --- Pulse routing (two slots: pulse env → any param) ---
    #[id = "mat"] pub mod_a_target: EnumParam<ModTarget>,
    #[id = "mad"] pub mod_a_depth: FloatParam,
    #[id = "mbt"] pub mod_b_target: EnumParam<ModTarget>,
    #[id = "mbd"] pub mod_b_depth: FloatParam,

    // --- Speed Pulse (logarithmic rotation-speed kick with its own AD envelope) ---
    // The generic routing adds *linearly* to the per-axis speed, which is invisible
    // once multiplied by the tiny global decade (10^speed_exp). This instead
    // multiplies the global speed by 10^(env·amount), so a hit "knocks" the spin up
    // by whole powers of 10 and falls back at a manually-set rate.
    /// Kick size in decades: a full pulse multiplies the global rotation speed by
    /// 10^amount (1 = ×10, e.g. 10⁻³ → 10⁻²). 0 = inert.
    #[id = "spamt"] pub speed_pulse_amount: FloatParam,
    /// How fast the kick rises to the peak (ms).
    #[id = "spatk"] pub speed_pulse_attack: FloatParam,
    /// How slowly it falls back afterwards (ms) — the bounce's fall-off, set
    /// independently of the audio band's release.
    #[id = "spdec"] pub speed_pulse_decay: FloatParam,

    // --- Breath: universal pulse-driven scene scale ---
    // Scales the whole scene about its centre at the camera/view level, so it
    // breathes for every generator + surface mode against a fixed sky. Driven by
    // the same pulse envelope, smoothed with its own attack/decay.
    /// Scale depth at a full pulse (0 = inert). A full pulse swells the whole scene
    /// by × (1 + amount), e.g. 0.1 ≈ +10%, 3 ≈ ×4.
    #[id = "brama"] pub breath_amount: FloatParam,
    /// How fast the swell rises to the peak (ms).
    #[id = "bratk"] pub breath_attack: FloatParam,
    /// How slowly it relaxes back (ms).
    #[id = "brdec"] pub breath_decay: FloatParam,

    // --- Environment / PBR (IBL) ---
    #[id = "mtyp"] pub mat_type: EnumParam<MaterialType>,
    #[id = "ior"]  pub ior: FloatParam, // glass index of refraction
    /// Beer–Lambert absorption strength for the `Refractive` material (σ scale;
    /// the node's albedo is what survives, mirroring the liquid's convention).
    /// Also drives the refraction overlay's murk; inert otherwise.
    #[id = "mabs"] pub mat_absorb: FloatParam,
    /// Refraction overlay: the Refractive optics as an OPTION on the other
    /// material types — Standard opens its diffuse body into (roughness-
    /// frosted) refraction, Chrome keeps its mirror at grazing angles and
    /// opens into a refracted core face-on (Fresnel), Glass gains the
    /// measured-chord murk. `ior` + `mat_absorb` drive it; redundant when
    /// the type is already Refractive (it IS the effect).
    #[id = "rovr"] pub refr_overlay: BoolParam,
    /// Overlay strength: how much of the material's body opens into the
    /// refracted transmission (0 = the plain material, 1 = full).
    #[id = "rblf"] pub refr_blend: FloatParam,
    /// Screen-space refraction (#214 T5 pt 2): a post pass so the Refractive
    /// material shows the ACTUAL scene behind it (neighbours / the world),
    /// displaced — not just the environment. 0 = off (env-only, as today).
    /// Refractive material + instanced cube/tube modes only.
    #[id = "srfs"] pub refract_ss: FloatParam,
    /// Screen-space refraction displacement: the world-space step along the
    /// refracted ray before re-sampling the scene (bigger = more bend).
    #[id = "srfd"] pub refract_dist: FloatParam,
    /// Anisotropy (#214 T1): elliptical GGX highlight. −1..1 = the streak
    /// direction/strength (0 = isotropic); drives the `Anisotropic` material and
    /// the overlay. Inert at 0.
    #[id = "anis"] pub anisotropy: FloatParam,
    /// Brush rotation around the surface normal (degrees) — re-aims the streak
    /// (the default follows the instance's long axis, ideal for tubes/rods).
    #[id = "anir"] pub aniso_rotation: FloatParam,
    /// Anisotropy overlay: apply the elliptical lobe to Standard/Chrome (brushed
    /// chrome / satin) instead of only the dedicated `Anisotropic` material.
    #[id = "anov"] pub aniso_overlay: BoolParam,
    /// Overlay strength (0 = isotropic, 1 = full anisotropy).
    #[id = "anbl"] pub aniso_blend: FloatParam,
    /// Clearcoat (#214 T2): strength of the thin smooth coat lobe (car paint /
    /// lacquer / wet). Drives the `Clearcoat` material + the overlay.
    #[id = "ccst"] pub clearcoat: FloatParam,
    /// Clearcoat roughness (0 = a sharp glossy coat, up = a satin coat).
    #[id = "ccrf"] pub clearcoat_rough: FloatParam,
    /// Clearcoat overlay: lacquer any Standard/Chrome material (not only the
    /// dedicated `Clearcoat` type).
    #[id = "cclo"] pub clearcoat_overlay: BoolParam,
    /// Sheen / velvet (#214 T2): amount of the grazing-angle fuzz lobe. Drives the
    /// `Velvet` material + the overlay.
    #[id = "shst"] pub sheen: FloatParam,
    /// Sheen roughness (softness of the fuzz).
    #[id = "shrf"] pub sheen_rough: FloatParam,
    /// Sheen tint: 0 = white fuzz, 1 = the fuzz picks up the node's own colour.
    #[id = "shtn"] pub sheen_tint: FloatParam,
    /// Sheen overlay: dust any Standard/Chrome material with the fuzz.
    #[id = "shov"] pub sheen_overlay: BoolParam,
    /// Body optics (#214 T3): drive the translucency back-glow by the MEASURED body
    /// thickness (Beer–Lambert) instead of the distortion hack. 0 = today's
    /// translucency; 1 = fully thickness-driven (honest wax/jade). Works with the
    /// Surface-FX translucency amount + the `Subsurface` material.
    #[id = "ssth"] pub sss_thickness: FloatParam,
    /// Translucency penetration radius (how deep light travels before the node's
    /// colour absorbs it) — larger = deeper glow.
    #[id = "ssrd"] pub sss_radius: FloatParam,
    /// Interior in-scatter (#214 T3): where a Glass/Refractive body absorbs the
    /// transmission, re-add a scattered glow (ambient + bounced GI at the body
    /// midpoint, tinted by the node colour) — opal / nebula-in-a-cube. 0 = off;
    /// crystal-clear glass stays clear.
    #[id = "insc"] pub interior_scatter: FloatParam,
    /// Glitter (#214 T4): sparse per-facet sparkle flakes (metallic flake / frost).
    /// Amount 0 = off. Twinkles per frame (TAA/blue-noise resolve it).
    #[id = "gltr"] pub glitter: FloatParam,
    /// Glitter density: the world-space facet grid scale (bigger = finer flakes).
    #[id = "gltd"] pub glitter_density: FloatParam,
    /// Glitter sharpness: how tight each flake's glint is (0 = broad, 1 = pinpoint).
    #[id = "glts"] pub glitter_sharpness: FloatParam,
    /// Diffraction (#214 T4): a grating rainbow on the reflection (CD / holo-foil,
    /// strongest over Chrome). Amount 0 = off.
    #[id = "difr"] pub diffraction: FloatParam,
    /// Diffraction frequency: how many rainbow orders across the angle sweep.
    #[id = "diff"] pub diffraction_freq: FloatParam,
    /// Retroreflection (#214 T4): a glow back toward the light (road-sign / cat's-eye),
    /// brightest when the camera looks along the light. Amount 0 = off.
    #[id = "retr"] pub retro: FloatParam,
    /// Fluorescence (#214 T5): absorb the environment's short-wavelength (blue) light
    /// and re-emit it at `fluor_hue` — a blacklight-poster glow under a blue/bright
    /// env. Amount 0 = off.
    #[id = "flur"] pub fluorescence: FloatParam,
    /// Fluorescence emit hue (0..1 around the wheel).
    #[id = "fluh"] pub fluor_hue: FloatParam,
    /// Incandescence (#214 T5): a blackbody glow by `temperature` (embers → white-hot)
    /// on top of the material. Amount 0 = off.
    #[id = "inca"] pub incandescence: FloatParam,
    /// Blackbody temperature in Kelvin (≈1000 deep red → 6500 white → 12000 blue).
    #[id = "temp"] pub temperature: FloatParam,
    #[id = "metl"] pub metallic: FloatParam,
    #[id = "rough"] pub roughness: FloatParam,
    #[id = "expo"] pub exposure: FloatParam,
    #[id = "envi"] pub env_intensity: FloatParam,
    #[id = "envr"] pub env_rotation: FloatParam,
    #[id = "blmi"] pub bloom_intensity: FloatParam,
    #[id = "blmt"] pub bloom_threshold: FloatParam,

    // --- Surface FX (additive modifiers layered on any material type) ---
    // Translucency: a Barré-Brisebois back-scatter lobe — the surface glows
    // through when backlit (wax / jellyfish flesh). Iridescence: a view-angle
    // thin-film spectral tint that shimmers as nodes rotate. Both inert at 0.
    #[id = "sss"]  pub subsurface: FloatParam,
    #[id = "sssd"] pub sss_distortion: FloatParam,
    #[id = "sssp"] pub sss_power: FloatParam,
    #[id = "irid"] pub iridescence: FloatParam,
    #[id = "irds"] pub irid_scale: FloatParam,
    #[id = "irdh"] pub irid_shift: FloatParam,

    /// Colour palette (1-D LUT) for the strand/field sweep. Native = current look.
    #[id = "pal"] pub palette: EnumParam<Palette>,

    // --- Metaball mode (only used when Surface Mode = Metaball) ---
    /// Per-node influence radius (world units). Must exceed the node spacing
    /// (≈1 for the unit grid) for neighbouring blobs to fuse into a skin.
    #[id = "mbrad"] pub metaball_radius: FloatParam,
    /// Iso level the surface sits at — higher pulls the skin tighter to the cores
    /// (thinner, more separated blobs); lower swells and merges them.
    #[id = "mbthr"] pub metaball_threshold: FloatParam,
    /// Falloff-edge sharpness: 0 = soft/round, higher = tighter, more defined rims.
    #[id = "mbsmt"] pub metaball_smooth: FloatParam,

    // --- Gaussian Splatting surface (only used when Surface Mode = Splat) ---
    /// Splat radius: world-space multiplier on each node's covariance axes. The
    /// cube-of-cubes default packs unit nodes ≈1 apart, so ≈0.5+ overlaps them into a
    /// continuous soft cloud; smaller separates the Gaussians into discrete blobs.
    #[id = "splrd"] pub splat_radius: FloatParam,
    /// Per-splat opacity (peak weight at the Gaussian centre, 0..1). Tier 1 scales the
    /// additive brightness; Tier 2 scales the alpha the disks composite with.
    #[id = "splop"] pub splat_opacity: FloatParam,
    /// Falloff — the Gaussian exponent scale. Higher = a tighter, more defined core
    /// (crisper splats); lower = a softer, foggier spread.
    #[id = "splfo"] pub splat_falloff: FloatParam,
    /// Tier: Additive (unlit, blooms through HDR — the robust look) or Lit (sorted-
    /// alpha 2DGS oriented disks, shaded by the IBL + key/fill).
    #[id = "splmd"] pub splat_mode: EnumParam<SplatMode>,
    /// Coverage cutoff: discard fragments whose Gaussian weight falls below this
    /// (trims the transparent skirt; larger = smaller, harder splats).
    #[id = "splct"] pub splat_cutoff: FloatParam,
    /// Anisotropy: extra stretch of each node's non-uniform axes. 1 = the node's own
    /// scale as-is; >1 exaggerates elongation (streaky splats), 0 forces round blobs.
    #[id = "splan"] pub splat_aniso: FloatParam,
    /// Scatter (Tier 3): sub-splats sprayed per node. 1 = one splat per node (as
    /// before); higher densifies the cloud toward the soft photoreal-splat look
    /// (each sub-splat's weight is divided by the count, preserving total brightness).
    #[id = "splsc"] pub splat_scatter: IntParam,
    /// Scatter jitter (Tier 3): how far the sub-splats spread from the node centre, as
    /// a fraction of the node's own size. 0 disables scatter (one splat per node — a
    /// stacked cloud would skew the lit tier's alpha), so scatter needs jitter > 0.
    #[id = "spljt"] pub splat_jitter: FloatParam,
    /// Solidity (Tier 3): remaps each splat from a soft Gaussian (0) toward a flat-topped,
    /// sharp-edged **opaque disc** (1). This is the dial that turns the cloud from soft,
    /// out-of-focus bokeh into a compact opaque **surface** — raise it (with Opacity ~1 and
    /// enough Radius for the discs to meet) and overlapping splats occlude instead of
    /// blurring. 0 = the original soft look.
    #[id = "splsd"] pub splat_solid: FloatParam,

    // --- Contiguous (welded) Swept Tubes (only used when Surface Mode = Swept Tubes) ---
    /// Weld each strand's segments into ONE smooth swept tube (closing the gaps at
    /// bends), instead of the default per-segment open cylinders.
    #[id = "twld"] pub tube_weld: BoolParam,
    /// Close the welded tube's two ends with a cap (else they stay open holes).
    #[id = "tcap"] pub tube_end_cap: BoolParam,
    /// End-cap height: 0 = flat disc flush with the end, 1 = full-height dome.
    #[id = "tcrd"] pub tube_cap_round: FloatParam,
    /// End-cap profile: 0 = rounded (dome), 1 = straight chamfer (bevel).
    #[id = "tcbv"] pub tube_cap_bevel: FloatParam,
    /// Cross-section shape: 1 = circle (round tube), 0 = sharp square (welded
    /// flow-aligned cubes), between = rounded square (corner-bevel radius).
    #[id = "tprf"] pub tube_profile: FloatParam,

    // --- Voxel mode (only used when Surface Mode = Voxel) ---
    /// Grid resolution (cubic). The projector perf dial — higher = finer voxels but
    /// a costlier per-frame splat.
    #[id = "vxres"] pub voxel_res: FloatParam,
    /// Fill threshold: the splat density a cell must reach to become solid. Higher
    /// thins the structure (only the cores survive); lower swells + fuses it.
    #[id = "vxthr"] pub voxel_threshold: FloatParam,
    /// Per-node splat radius (world units): how thick each strand voxelizes. ≈1 (the
    /// unit node spacing) gives lines 1–2 voxels thick; raise to merge into walls.
    #[id = "vxrad"] pub voxel_radius: FloatParam,
    /// Emissive gain: hot voxels exceed 1.0 and bloom in the HDR composite. 0 =
    /// purely lit blocks.
    #[id = "vxemi"] pub voxel_emission: FloatParam,
    /// Ambient-occlusion strength (0..1): the soft darkening in seams + crevices.
    #[id = "vxao"] pub voxel_ao: FloatParam,
    /// Soft-shadow strength (0..1): how dark a voxel goes when the key light is
    /// blocked by another voxel.
    #[id = "vxshd"] pub voxel_shadow: FloatParam,
    /// Palette posterize levels (0 = off): quantize the voxel colour to N steps per
    /// channel for the blocky-colour MagicaVoxel charm.
    #[id = "vxqnt"] pub voxel_quantize: FloatParam,
    /// Beat → fill threshold amount (bipolar): the pulse envelope pushes the
    /// threshold each beat, so the block-world swells / dissolves on tempo. Active
    /// only while Pulse is on; 0 = inert.
    #[id = "vxbt"] pub voxel_beat: FloatParam,
    /// Voxel GI (#89): cone-trace the field's mip pyramid for bounced colour —
    /// emissive voxels bleed onto their neighbours + the world. Off by default →
    /// the base voxel look is unchanged.
    #[id = "vxgi"] pub voxel_gi: BoolParam,
    /// Voxel GI bounce intensity.
    #[id = "vxgis"] pub voxel_gi_strength: FloatParam,
    /// Voxel GI cone reach, as a fraction (0..1) of the structure's size (so it
    /// reads the same across generators of different scale).
    #[id = "vxgid"] pub voxel_gi_distance: FloatParam,
    /// Voxel GI sky/ambient fill: light added in the unoccluded remainder of each
    /// cone (a soft fill from "open sky").
    #[id = "vxgik"] pub voxel_gi_sky: FloatParam,

    // --- Bioluminescence (colour cycling + travelling emissive ripple) ---
    /// Palette colour cycling (cycles/sec, signed). Advances a phase added to the
    /// palette/HSV sweep so colour flows along the strand/field. 0 = off; no effect
    /// with the Native palette.
    #[id = "cyc"] pub color_cycle: FloatParam,
    /// Emissive ripple peak (linear HDR units, 0 = off). A travelling band of
    /// emission pulses through the field; push past ~1 so it blooms / exceeds SDR
    /// white on EDR for the bioluminescent glow.
    #[id = "rpli"] pub ripple_intensity: FloatParam,
    /// Ripple travel speed (signed).
    #[id = "rpls"] pub ripple_speed: FloatParam,
    /// Ripple frequency — number of simultaneous bands (1 = one fat pulse, higher =
    /// many thin ctenophore-comb ripples).
    #[id = "rplf"] pub ripple_freq: FloatParam,
    /// Ripple band sharpness (higher = tighter, more defined bands).
    #[id = "rplsh"] pub ripple_sharp: FloatParam,
    /// Ripple geometry: radial shells from the centre, or an axial wavefront.
    #[id = "rplg"] pub ripple_geom: EnumParam<RippleGeom>,

    // --- Membrane mode (only used when Surface Mode = Membrane) ---
    /// Which grid axis/axes to loft sheets across.
    #[id = "mweav"] pub membrane_weave: EnumParam<MembraneWeave>,
    /// Also draw the boundary strands (as swept tubes) under the membrane.
    #[id = "mstr"] pub membrane_show_strands: BoolParam,
    /// Skin-Arms mode: instead of one continuous shell that fuses the strands,
    /// skin **each strand (arm) as its own closed, capped finger** with open
    /// gaps between arms — the hull of the volume-render form. Off = the shell.
    #[id = "marm"] pub membrane_arms: BoolParam,
    /// How the per-arm fingers are built when Skin-Arms is on: cheap capsule
    /// **Impostors** (no per-frame mesh) or a real welded **Mesh** (seamless).
    #[id = "marmb"] pub membrane_arm_build: EnumParam<MembraneArmBuild>,
    /// Close the loft seam when the strand grid wraps a full 360° (bridge the
    /// last row of strands back to the first) so the shell has no gap.
    #[id = "mcls"] pub membrane_close: BoolParam,
    /// Skin-Arms capsule radius. 0 = auto (per-node thickness for field generators,
    /// a unit-grid default for the Original cube-field); > 0 overrides it with a
    /// uniform radius — fatten to close the side gaps between capsule segments.
    #[id = "marmr"] pub membrane_arm_radius: FloatParam,

    // --- Reaction–diffusion skin (Turing patterns; any surface mode) ---
    /// HDR emissive gain of the pattern (0 = off). Push > 1 to bloom / glow.
    #[id = "rdi"] pub rd_intensity: FloatParam,
    /// Feed rate — with kill, selects spots ↔ stripes ↔ maze.
    #[id = "rdf"] pub rd_feed: FloatParam,
    /// Kill rate.
    #[id = "rdk"] pub rd_kill: FloatParam,
    /// Pattern scale (world→pattern frequency): higher = finer/denser.
    #[id = "rdsc"] pub rd_scale: FloatParam,
    /// Pigment mix: 0 = pure emissive glow; up = the pattern also carves albedo.
    #[id = "rdmix"] pub rd_albedo_mix: FloatParam,

    // --- Ambient occlusion (depth-prepass SSAO; off by default) ---
    /// Master toggle. Off = no depth prepass / AO passes run and the composite
    /// uses AO=1, so the default look is byte-identical.
    #[id = "ao"]   pub ssao: BoolParam,
    /// Sample radius in world units — how far contact shadowing reaches.
    #[id = "aorad"] pub ssao_radius: FloatParam,
    /// Strength of the darkening (0 = none).
    #[id = "aoint"] pub ssao_intensity: FloatParam,
    /// Depth bias to suppress self-occlusion on flat faces.
    #[id = "aobia"] pub ssao_bias: FloatParam,

    // --- "Jewel Box" (#80): surface-to-surface light transport ---
    // Part A — Inter-cube reflections (SSR). Off by default → no prepass/SSR pass.
    /// Master toggle. Off = the SSR pass is skipped; cubes reflect only the env.
    #[id = "ssr"]    pub ssr: BoolParam,
    /// Reflection strength (multiplies the marched neighbour radiance).
    #[id = "ssrint"] pub ssr_intensity: FloatParam,
    /// Roughness cutoff: SSR is skipped above this (keeps it off diffuse cubes).
    #[id = "ssrrou"] pub ssr_max_roughness: FloatParam,
    /// Thickness band (world units) for accepting a depth-march hit.
    #[id = "ssrthk"] pub ssr_thickness: FloatParam,
    /// March step budget (perf dial).
    #[id = "ssrstp"] pub ssr_steps: IntParam,
    // Part B — Bounced GI (irradiance probe volume). Off by default (intensity 0).
    /// Master toggle. Off = the probe grid is uploaded "off" and contributes nothing.
    #[id = "gi"]     pub gi: BoolParam,
    /// Bounce strength (how strongly neighbour colour bleeds into the ambient term).
    #[id = "giint"]  pub gi_intensity: FloatParam,
    /// Bleed reach: widens/narrows how far a node's colour reaches probe cells.
    #[id = "gifal"]  pub gi_falloff: FloatParam,
    // Part C — Spectral glass. dispersion 0 → today's single-IOR glass exactly.
    /// Chromatic dispersion: splits the Glass refraction into RGB (rainbow fringing).
    #[id = "gdisp"]  pub glass_dispersion: FloatParam,
    /// Caustic boost: brightens focused light seen through the glass body.
    #[id = "gcaus"]  pub glass_caustic: FloatParam,
    /// Thin-film interference tint (oil-slick) on the Glass reflection at grazing.
    #[id = "gfilm"]  pub glass_thin_film: FloatParam,

    // --- Reflection look (#163 Tier 1): pure chrome / clear glass + palette dial ---
    // All default 0 → today's chrome/glass/standard look byte-identical. See #163.
    /// Palette influence on the reflection: 0 = each material's existing behaviour
    /// (reflection uncoloured by the RGB-cube palette beyond what it does today); at 1
    /// the reflection is fully tinted by the surface albedo, and >1 pushes past it
    /// (saturate / override). Works with Standard, Chrome and Glass.
    #[id = "rfltnt"] pub reflect_tint: FloatParam,
    /// Chrome purity: 0 = today's chrome; 1 = a pure NEUTRAL mirror (sharp, untinted,
    /// high reflectance) regardless of palette/metallic. Restores the "pure chrome" look.
    #[id = "chrpur"] pub chrome_purity: FloatParam,
    /// Glass clarity: 0 = today's tinted glass; 1 = colourless CLEAR glass (crisp
    /// Fresnel rim, minimal body tint, sharper refraction). Restores the "clear glass" look.
    #[id = "glsclr"] pub glass_clarity: FloatParam,
    /// Standard reflectivity override: 0 = metallic-driven F0 (today); up lifts the
    /// dielectric reflectance toward a mirror WITHOUT forcing metallic = 1 (keeps albedo).
    #[id = "f0ovr"]  pub f0_override: FloatParam,

    // --- Temporal pass (#152 Tier 2): TAA + motion blur + stochastic glass ---
    /// Temporal anti-aliasing (reproject + neighbourhood-clamp history). Per-display.
    #[id = "taaen"]  pub taa_enabled: BoolParam,
    /// TAA current-frame weight (0.05..1; lower = more history = smoother but more ghosting).
    #[id = "taabl"]  pub taa_blend: FloatParam,
    /// Post-blend sharpen to counter TAA softening.
    #[id = "taash"]  pub taa_sharpen: FloatParam,
    /// Per-object/camera motion blur (uses the reconstructed camera velocity).
    #[id = "mblur"]  pub motion_blur: BoolParam,
    /// Motion-blur shutter strength.
    #[id = "mbamt"]  pub mb_amount: FloatParam,
    /// Motion-blur taps (perf/quality).
    #[id = "mbsmp"]  pub mb_samples: IntParam,
    /// Stochastic (dither-discard) transparency for Glass — order-independent; needs TAA.
    #[id = "stoch"]  pub stochastic_glass: BoolParam,

    // --- Screen-space GI (#152 Tier 2) ---
    /// One-bounce screen-space GI (bright cubes bleed colour onto neighbours).
    #[id = "ssgi"]   pub ssgi: BoolParam,
    /// GI strength.
    #[id = "ssgiin"] pub ssgi_intensity: FloatParam,
    /// View-space gather radius.
    #[id = "ssgird"] pub ssgi_radius: FloatParam,
    /// Rays per pixel (perf/noise; TAA denoises).
    #[id = "ssgiry"] pub ssgi_rays: IntParam,

    // --- Cast shadows (#152 Tier 3) ---
    /// Key-light cast shadows (a world-space depth map, PCF). Instanced path only.
    #[id = "shdw"]   pub shadow_enabled: BoolParam,
    /// Depth bias — raise to kill shadow acne, lower if shadows detach (peter-panning).
    #[id = "shdwb"]  pub shadow_bias: FloatParam,
    /// Shadow strength (0 = none, 1 = full darkness in the key term).
    #[id = "shdws"]  pub shadow_strength: FloatParam,

    // --- Voxel GI (#152 Tier 3, #10) ---
    /// World-space bounce GI marched from the voxelized node field (sees off-screen
    /// emitters, unlike SSGI). Instanced path only; pairs with TAA to denoise.
    #[id = "wvgion"] pub vxgi_enabled: BoolParam,
    /// Bounce strength.
    #[id = "wvgiin"] pub vxgi_intensity: FloatParam,
    /// Hemisphere rays per pixel (perf/noise).
    #[id = "wvgiry"] pub vxgi_rays: IntParam,
    /// March steps per ray (reach/quality).
    #[id = "wvgist"] pub vxgi_steps: IntParam,

    // --- Reflection probe / parallax (#163 Tier 2) ---
    // EnvOnly (default) → today's look. Parallax box-projects the env reflection
    // against the field's AABB so reflections shift with position, not just orientation.
    /// Reflection source: Env Only (today) or Parallax Box (position-aware).
    #[id = "rflsrc"] pub refl_source: EnumParam<ReflectionSource>,
    /// Parallax box XZ half-extent as a multiple of the field's AABB (1 = the field itself).
    #[id = "rflbxs"] pub refl_box_scale: FloatParam,
    /// Parallax box Y (height) half-extent multiplier (fields that are tall/flat).
    #[id = "rflbxh"] pub refl_box_height: FloatParam,
    /// Parallax blend: 0 = infinite (today), 1 = fully box-projected reflection direction.
    #[id = "rflbld"] pub refl_blend: FloatParam,

    // --- VXGI specular cone tracing (#163 Tier 3) ---
    // Reflections marched through the SAME voxel volume: cubes reflect the actual scene
    // (other cubes, off-screen emitters — no screen-edge dropout). Requires VXGI on.
    /// Reflection strength (0 = off → today's look). Adds a specular cone to the VXGI pass.
    #[id = "vxsstr"] pub vxgi_spec_strength: FloatParam,
    /// Cone aperture (0 = sharp mirror, 1 = wide/glossy blur).
    #[id = "vxsapr"] pub vxgi_spec_aperture: FloatParam,
    /// Reach: reflection march distance as a fraction of the scene diagonal.
    #[id = "vxsrch"] pub vxgi_spec_reach: FloatParam,
    /// Cone march steps (perf/quality).
    #[id = "vxsstp"] pub vxgi_spec_steps: IntParam,

    // --- Membrane screen-space FX ---
    /// Draw the Membrane surface into the depth prepass so screen-space effects (VXGI
    /// diffuse + reflections, SSAO, SSR, SSGI, DoF, TAA) apply to it. Off = today's look
    /// (membrane skips the prepass); on = an extra depth draw so those effects light up.
    /// A perf escape hatch — leave off to keep membrane exactly as it is now.
    #[id = "memfx"]  pub membrane_fx: BoolParam,

    // --- Cinematic finishing (#167 Tier 1) — post-composite, inside the FX pass ---
    // Halation + lens flares. Both amounts 0 → today's look; only act when Post FX is on.
    /// Halation strength — the warm chromatic bleed around bright highlights (0 = off).
    #[id = "halamt"] pub hal_amount: FloatParam,
    /// Halation bright-pass threshold (which highlights bleed).
    #[id = "halthr"] pub hal_threshold: FloatParam,
    /// Halation halo radius (screen fraction).
    #[id = "halwid"] pub hal_width: FloatParam,
    /// Halation warmth — how red-weighted the bleed tint is (0 = neutral, 1 = deep red).
    #[id = "halwrm"] pub hal_warmth: FloatParam,
    /// Lens-flare strength — screen-space ghosts + halo + streak from bright points (0 = off).
    #[id = "lfamt"]  pub lf_amount: FloatParam,
    /// Lens-flare ghost intensity (the mirrored blobs across centre).
    #[id = "lfgho"]  pub lf_ghosts: FloatParam,
    /// Lens-flare halo-ring intensity.
    #[id = "lfhalo"] pub lf_halo: FloatParam,
    /// Lens-flare anamorphic streak intensity (horizontal blue smear).
    #[id = "lfstrk"] pub lf_streak: FloatParam,

    // --- Emissive cubes as real lights (#167 Tier 3) ---
    // The brightest cubes become real point lights that illuminate their neighbours.
    /// Turn the brightest cubes into real point lights (crisp glints + coloured pools). Off = today.
    #[id = "mlen"]   pub ml_enabled: BoolParam,
    /// Emitted-radiance scale (how bright the cube-lights are).
    #[id = "mlint"]  pub ml_intensity: FloatParam,
    /// Falloff radius as a fraction of the scene diagonal.
    #[id = "mlrad"]  pub ml_radius: FloatParam,
    /// How many of the brightest cubes to use as lights (perf).
    #[id = "mlcnt"]  pub ml_count: IntParam,
    /// ReSTIR many-lights (#200 Tier 5d): pick the light set by weighted reservoir
    /// sampling instead of a hard brightest-`count` cap, so every glowing cube gets
    /// a luminance-proportional chance to become a light over time (dim/distant
    /// emitters rotate in; the fade envelope + TAA integrate it). Off = brightest-N.
    #[id = "mlrst"]  pub ml_restir: BoolParam,

    // --- Renderer (output) ---
    /// True-HDR (macOS EDR) output. The visual swaps to a float surface + an
    /// extended-linear colorspace so highlights exceed SDR white. Not captured by
    /// presets (it's a per-display capability, not a look).
    #[id = "hdro"] pub hdr_output: BoolParam,
    /// HDR highlight roll-off knee (0..1): where highlights start compressing
    /// toward the display headroom. Lower = softer, higher = punchier.
    #[id = "hdrk"] pub hdr_knee: FloatParam,
    /// Wide-gamut HDR output: tag the EDR surface as Rec.2020 (extendedLinearITUR_2020)
    /// instead of Rec.709 (…SRGB), so saturated colours can reach a wide-gamut display's
    /// real primaries. Pairs with `hdr_vivid`. (Field renamed from `hdr_p3`, same id.)
    #[id = "hdrp"] pub hdr_wide: BoolParam,
    /// HDR gamut-expansion / vividness (0..1, #119). Only active with `hdr_wide` + EDR.
    /// 0 = colour-accurate (Rec.709→Rec.2020, unchanged); 1 = full stretch (spectrum
    /// pushed to the Rec.2020 primaries → much more vivid). Per-display, not a preset.
    #[id = "hdrv"] pub hdr_vivid: FloatParam,
    /// SDR tone-mapping operator (HDR mode uses the headroom shoulder instead).
    #[id = "tmop"] pub tonemap: EnumParam<ToneMap>,
    /// MSAA sample count for the scene pass.
    #[id = "msaa"] pub msaa: EnumParam<Msaa>,
    /// Global render-resolution scale (0.25..1.0): the scene + post render at this
    /// fraction of the output and upscale to native (e.g. 0.5 = 1080p on a 4K
    /// projector). The manual target; ignored while Auto Resolution is on.
    #[id = "rscl"] pub render_scale: FloatParam,
    /// Auto resolution (dynamic): the visual lowers/raises the render scale to hold
    /// ~60 FPS. The live resolution shows in the editor + the visual window title.
    #[id = "raut"] pub render_auto: BoolParam,

    // --- Capture / production frame (#135 Phase 1) ---
    /// Fixed output aspect ratio. `Native` = render straight to the window (today's
    /// behaviour); any other preset renders into a fixed-resolution offscreen target
    /// and letterboxes it into the window so an OBS capture is pixel-exact. Per-display
    /// (not preset-captured).
    #[id = "capa"] pub aspect_preset: EnumParam<AspectPreset>,
    /// Output long-edge resolution in px (short edge derived from the aspect, e.g.
    /// 1920 → 1080×1920 for 9:16). **0 = match the display** (the visual uses the
    /// window's longest side), so picking an aspect reframes at full native
    /// resolution instead of downscaling to a fixed signal — a 4K display stays 4K.
    /// Ignored for `Native`/`Custom`.
    #[id = "cape"] pub out_long_edge: IntParam,
    /// Custom output width/height (px) — used only when aspect = `Custom`.
    #[id = "capw"] pub out_custom_w: IntParam,
    #[id = "caph"] pub out_custom_h: IntParam,
    /// Letterbox/pillarbox bar colour (linear 0..1), shown around the production frame.
    #[id = "capr"] pub letterbox_r: FloatParam,
    #[id = "capg"] pub letterbox_g: FloatParam,
    #[id = "capb"] pub letterbox_b: FloatParam,
    /// Draw a thin safe-area border at the production-frame edge (line up an OBS crop).
    #[id = "capfg"] pub frame_guide: BoolParam,
    /// Resize the window's inner size to exactly the output size (pixel-perfect
    /// window-capture in OBS, no crop needed).
    #[id = "caplk"] pub lock_window: BoolParam,

    // --- Capture overlay (#135 Phase 2) ---
    /// Master on/off for the text overlay (title / description / formula / live readouts /
    /// handle), composited on top of the production frame. Per-display (not preset-saved).
    #[id = "oven"] pub overlay_enabled: BoolParam,
    /// Whole-overlay opacity.
    #[id = "ovop"] pub overlay_opacity: FloatParam,
    /// Overlay font / zone scale (1.0 = default sizing).
    #[id = "ovsc"] pub overlay_scale: FloatParam,
    /// Per-zone toggles.
    #[id = "ovtt"] pub overlay_title: BoolParam,
    #[id = "ovds"] pub overlay_desc: BoolParam,
    #[id = "ovfm"] pub overlay_formula: BoolParam,
    #[id = "ovrd"] pub overlay_readouts: BoolParam,
    #[id = "ovhd"] pub overlay_handle: BoolParam,
    /// Readout-panel fill colour (linear) + opacity.
    #[id = "ovpr"] pub overlay_panel_r: FloatParam,
    #[id = "ovpg"] pub overlay_panel_g: FloatParam,
    #[id = "ovpb"] pub overlay_panel_b: FloatParam,
    #[id = "ovpo"] pub overlay_panel_opacity: FloatParam,
    /// Default (non-symbol) text colour (linear).
    #[id = "ovtr"] pub overlay_text_r: FloatParam,
    #[id = "ovtg"] pub overlay_text_g: FloatParam,
    #[id = "ovtb"] pub overlay_text_b: FloatParam,

    // --- Capture decoration: 3-D axes + wireframe volume (#135 Phase 5) ---
    /// XYZ reference axes through the origin (X red / Y green / Z blue). World-space lines
    /// in the scene pass; per-display (not preset-captured).
    #[id = "axon"] pub axes_on: BoolParam,
    /// Axis length (world units), tube thickness (radius), opacity; ticks + X/Y/Z labels.
    #[id = "axln"] pub axes_len: FloatParam,
    #[id = "axth"] pub axes_thick: FloatParam,
    #[id = "axop"] pub axes_opacity: FloatParam,
    #[id = "axtk"] pub axes_ticks: BoolParam,
    #[id = "axlb"] pub axes_labels: BoolParam,
    /// Wireframe bounding box / grid volume around the origin.
    #[id = "boxon"] pub box_on: BoolParam,
    #[id = "boxex"] pub box_extent: FloatParam,
    #[id = "boxsd"] pub box_subdiv: IntParam,
    #[id = "boxr"] pub box_r: FloatParam,
    #[id = "boxg"] pub box_g: FloatParam,
    #[id = "boxb"] pub box_b: FloatParam,
    #[id = "boxop"] pub box_opacity: FloatParam,

    // --- Field Chamber (#346): analyzer panels on the box back walls ---
    /// Master on/off. Off → nothing drawn (byte-identical). Captured **Look**.
    #[id = "chon"] pub panels_on: BoolParam,
    /// Flat 2-D composite vs the #298 impostor rounded-line + material.
    #[id = "chsty"] pub panel_style: EnumParam<PanelStyle>,
    /// Rear −Z wall = oscilloscope (time); right +X wall = spectrum (frequency).
    #[id = "chrear"] pub panel_rear: BoolParam,
    #[id = "chrite"] pub panel_right: BoolParam,
    /// Whole-panel alpha and wall inset (0..1 of the wall face).
    #[id = "chopa"] pub panel_opacity: FloatParam,
    #[id = "chfill"] pub panel_fill: FloatParam,
    /// Wall-scope vertical gain; spectrum dBFS window (floor..top).
    #[id = "champ"] pub panel_scope_amp: FloatParam,
    #[id = "chdbf"] pub panel_db_floor: FloatParam,
    #[id = "chdbt"] pub panel_db_top: FloatParam,
    /// Impostor material (Tier 2) + metallic/roughness + emissive glow + line radius.
    #[id = "chmat"] pub panel_material: EnumParam<MaterialType>,
    #[id = "chmet"] pub panel_metallic: FloatParam,
    #[id = "chrgh"] pub panel_roughness: FloatParam,
    #[id = "chemi"] pub panel_emissive: FloatParam,
    #[id = "chthk"] pub panel_thickness: FloatParam,
    /// 0 = fixed world axes (rear −Z / right +X, shown only when back-facing);
    /// 1 = camera-relative (time/freq always ride the back-facing walls).
    #[id = "chwrel"] pub panel_wall_rel: BoolParam,
    /// Wall-scope publish controls (plugin-side; drive `scopewave`). Captured **Look**.
    /// `panel_scope_time_ms` = display-window length; `panel_scope_trigger`
    /// (0 Free / 1 Rising / 2 Falling); `panel_scope_channel` (0 L / 1 R / 2 Mid).
    #[id = "chtms"] pub panel_scope_time_ms: FloatParam,
    #[id = "chtrg"] pub panel_scope_trigger: IntParam,
    #[id = "chchn"] pub panel_scope_channel: IntParam,

    // --- Post-composite creative FX (#152, Tier 1) ---
    /// Master enable: off → the whole FX pass is skipped, the image is byte-identical.
    #[id = "fxen"] pub fx_enabled: BoolParam,
    /// NPR style applied on the composited image (None / Toon / Outline / Halftone /
    /// Dither / Pixelate).
    #[id = "fxst"] pub fx_style: EnumParam<RenderStyle>,
    /// Style strength — toon bands / outline edge / halftone cell / pixel size.
    #[id = "fxsa"] pub fx_style_amt: FloatParam,
    /// Depth-of-field amount (0 = off). Uses the scene depth on the instanced path.
    #[id = "fxdof"] pub fx_dof: FloatParam,
    /// DoF focus plane (0..1 raw depth — near→far).
    #[id = "fxdff"] pub fx_dof_focus: FloatParam,
    /// DoF in-focus band width.
    #[id = "fxdfr"] pub fx_dof_range: FloatParam,
    /// Chromatic aberration (radial RGB split).
    #[id = "fxca"] pub fx_chroma: FloatParam,
    /// Vignette darkening at the frame edges.
    #[id = "fxvg"] pub fx_vignette: FloatParam,
    /// Film grain.
    #[id = "fxgr"] pub fx_grain: FloatParam,
    /// Colour grade — saturation (1 = neutral).
    #[id = "fxgs"] pub fx_grade_sat: FloatParam,
    /// Colour grade — contrast about mid-grey (1 = neutral).
    #[id = "fxgc"] pub fx_grade_contrast: FloatParam,
    /// Colour grade — temperature (0 = neutral; + warm / − cool).
    #[id = "fxgt"] pub fx_grade_temp: FloatParam,
    /// Colour grade — gain multiplier (1 = neutral).
    #[id = "fxgg"] pub fx_grade_gain: FloatParam,
    /// Echo-trail persistence (frame feedback; 0 = off).
    #[id = "fxfb"] pub fx_feedback: FloatParam,
    /// Outline edge threshold (Outline style only).
    #[id = "fxol"] pub fx_outline: FloatParam,

    // --- Emissive volume surface mode (#152, Tier 1) ---
    /// Field blob radius for the volume bake (reuses the metaball field).
    #[id = "vlrd"] pub volume_radius: FloatParam,
    /// Volume density multiplier.
    #[id = "vldn"] pub volume_density: FloatParam,
    /// Volume emissive glow strength (HDR — blooms).
    #[id = "vlem"] pub volume_emission: FloatParam,
    /// Volume extinction / absorption (Beer–Lambert).
    #[id = "vlab"] pub volume_absorption: FloatParam,
    /// Volume raymarch step budget (perf dial).
    #[id = "vlst"] pub volume_steps: IntParam,

    // --- Environment backdrop + tint ---
    /// Tone-map operator for the environment backdrop only (geometry uses
    /// `tonemap`, or the EDR shoulder in HDR mode). HDR panoramas (e.g. Blockade
    /// Labs Skybox) get crushed by a contrasty filmic curve like ACES; AgX keeps
    /// them natural. Applied in both SDR and HDR output, so the backdrop looks the
    /// same across displays.
    #[id = "bgtm"] pub bg_tonemap: EnumParam<ToneMap>,
    /// Draw the environment as the background (off = cubes float on black; they
    /// stay lit by the IBL either way).
    #[id = "bgsh"] pub bg_visible: BoolParam,
    /// Background brightness multiplier (skybox only; does not change IBL lighting).
    #[id = "bgin"] pub bg_intensity: FloatParam,
    /// Environment tint hue (degrees). Tints both the IBL lighting and the
    /// background — e.g. warm the "sun" from white toward orange.
    #[id = "etnh"] pub env_tint_hue: FloatParam,
    /// Environment tint amount (0 = untouched white, 1 = full hue saturation).
    #[id = "etna"] pub env_tint_amt: FloatParam,

    // --- Per-material Hue / Saturation / Value (#305 Tier 1). Defaults are the
    // identity (hue 0, cycle 0, sat 1, value 1) → byte-identical; every knob only
    // recolours when moved. `hue` rotates the palette-derived colour (cycles the
    // palette); `hue_cycle` auto-advances the hue on the beat clock; `saturation` /
    // `value` only lower from 1. ---
    /// Generator material hue (turns): rotates/cycles the palette-derived albedo.
    #[id = "mhue"] pub mat_hue: FloatParam,
    /// Generator material hue-cycle rate (turns per beat): auto-advances the hue.
    #[id = "mhcy"] pub mat_hue_cycle: FloatParam,
    /// Generator material saturation (1 = unchanged, 0 = greyscale).
    #[id = "msat"] pub mat_saturation: FloatParam,
    /// Generator material value/brightness (1 = unchanged, 0 = black).
    #[id = "mval"] pub mat_value: FloatParam,
    /// Scenery / environment material hue (turns).
    #[id = "shue"] pub scen_hue: FloatParam,
    /// Scenery / environment material hue-cycle rate (turns per beat).
    #[id = "shcy"] pub scen_hue_cycle: FloatParam,
    /// Scenery / environment material saturation.
    #[id = "ssat"] pub scen_saturation: FloatParam,
    /// Scenery / environment material value.
    #[id = "sval"] pub scen_value: FloatParam,

    // --- Live-sky cloud reflections (#305 Tier 2). Off by default → byte-identical. ---
    /// Drifting clouds on the sharp environment reflection (chrome/glass/beads show
    /// moving clouds instead of a frozen sky). Off = today's reflection.
    #[id = "srcl"] pub sky_reflect_clouds: BoolParam,
    /// Cloud cover on the reflected sky dome (0 = clear, 1 = overcast).
    #[id = "srcv"] pub sky_cloud_cover: FloatParam,
    /// Cloud drift speed (turns per beat) — the phase is accumulated on the beat clock.
    #[id = "srsp"] pub sky_cloud_speed: FloatParam,
    /// Cloud reflection strength (the brightness swing between sunlit cloud and gap).
    #[id = "srsh"] pub sky_cloud_strength: FloatParam,

    // --- Terrain backdrop (an infinite raymarched landscape behind any
    // generator; not a generator itself — a toggleable world layer) ---
    /// Draw the terrain backdrop (replaces the skybox as the background; the IBL
    /// still lights the generator geometry from the environment map).
    #[id = "tren"] pub terrain_enabled: BoolParam,
    /// Vertical scale of the mountains (world units of peak height).
    #[id = "trht"] pub terrain_height: FloatParam,
    /// Snow line as a fraction of peak height (lower = more snow).
    #[id = "trsn"] pub terrain_snow: FloatParam,
    /// Distance-fog density (haze that the land fades into at the horizon).
    #[id = "trfg"] pub terrain_fog: FloatParam,
    /// Sun elevation above the horizon (degrees).
    #[id = "trse"] pub terrain_sun_elev: FloatParam,
    /// Sun azimuth (degrees).
    #[id = "trsa"] pub terrain_sun_azim: FloatParam,
    /// Sun intensity (key-light strength on the land).
    #[id = "trsi"] pub terrain_sun_int: FloatParam,
    /// Fly speed — how fast the synthetic camera drifts over the landscape.
    #[id = "trsc"] pub terrain_scroll: FloatParam,
    /// Ride height — how far the fly-camera floats above the local terrain.
    #[id = "trrh"] pub terrain_ride: FloatParam,
    /// Which synthesized noise tile drives the landscape's character.
    #[id = "trnt"] pub terrain_noise: EnumParam<TerrainNoise>,
    /// Colour palette (rock / vegetation / snow albedo).
    #[id = "trpl"] pub terrain_palette: EnumParam<TerrainPalette>,
    /// Emissive glow strength — HDR light the terrain itself emits (lava veins,
    /// bioluminescent crevices, ice glints; colour set per palette). 0 = none.
    #[id = "trem"] pub terrain_emissive: FloatParam,
    /// Day-cycle speed: when > 0 the sun rises/sets over time (the elevation
    /// oscillates), driving a moving time-of-day. 0 = static (use sun elevation).
    #[id = "trdy"] pub terrain_day_speed: FloatParam,
    /// The terrain sun also lights the floating generator (key light follows it).
    #[id = "trsl"] pub terrain_sun_scene: BoolParam,
    /// Atmospheric scattering: colored aerial perspective (distant ridges fade to
    /// sky). 0 = the plain distance fog.
    #[id = "trsk"] pub terrain_scatter: FloatParam,
    /// Volumetric god-rays: in-scattered sunlight glowing through the haze toward
    /// the sun. 0 = off.
    #[id = "trgr"] pub terrain_godray: FloatParam,
    /// Sea level: add a reflective water plane flooding the valleys.
    #[id = "trwa"] pub terrain_water: BoolParam,
    /// Water level as a fraction (0..1) of terrain height.
    #[id = "trwl"] pub terrain_water_level: FloatParam,
    /// Water colour (hue, 0..1).
    #[id = "trwh"] pub terrain_water_hue: FloatParam,
    /// Water ripple strength (the surface ripples animate off the clock).
    #[id = "trwr"] pub terrain_water_ripple: FloatParam,
    /// Noise seed (re-roll for a different landscape of the same character).
    #[id = "trsd"] pub terrain_seed: IntParam,
    /// Ridged noise — fold each octave into sharp alpine spines.
    #[id = "trrg"] pub terrain_ridged: BoolParam,
    /// Overall backdrop brightness.
    #[id = "trbr"] pub terrain_brightness: FloatParam,
    /// Horizon haze tint strength.
    #[id = "trhz"] pub terrain_haze: FloatParam,
    /// PERF: raymarch step budget (the dominant cost). Lower for a projector.
    #[id = "trms"] pub terrain_steps: IntParam,
    /// PERF: fBm octaves while marching (detail vs. cost; normal/shadow derive from it).
    #[id = "trmo"] pub terrain_octaves: IntParam,
    /// PERF: render the terrain pass at half resolution, then upscale (≈4× cheaper).
    #[id = "trhr"] pub terrain_res: EnumParam<TerrainRes>,

    // --- Starfield (Yale Bright Star Catalog) — a global sky layer ---
    /// Draw the HDR starfield (9110 real stars, fading in as the day-cycle sun sets).
    #[id = "sten"] pub stars_enabled: BoolParam,
    /// Overall star brightness (linear HDR gain; bright stars bloom past 1).
    #[id = "stbr"] pub stars_brightness: FloatParam,
    /// Twinkle amount (per-star scintillation depth, 0 = steady).
    #[id = "sttw"] pub stars_twinkle: FloatParam,
    /// Twinkle speed (scintillation rate).
    #[id = "stts"] pub stars_twinkle_speed: FloatParam,
    /// Star sprite size in pixels.
    #[id = "stsz"] pub stars_size: FloatParam,
    /// Observer latitude (°) — sets the celestial-pole height over the horizon.
    #[id = "stla"] pub stars_latitude: FloatParam,
    /// Sky-rotation speed — wheels the whole sky over time (sidereal drift).
    #[id = "stsr"] pub stars_sky_speed: FloatParam,
    /// Magnitude limit (density): only stars brighter than this are drawn.
    #[id = "stml"] pub stars_mag_limit: FloatParam,
    /// Colour saturation of the spectral-type tints (0 = white stars).
    #[id = "stsa"] pub stars_saturation: FloatParam,
    /// Draw the HDR sun disc (rides the terrain sun elevation/azimuth + day cycle).
    #[id = "stsu"] pub stars_sun: BoolParam,
    /// Sun brightness (linear HDR — push it up to bloom hard).
    #[id = "stsb"] pub stars_sun_bright: FloatParam,
    /// Sun angular size (° radius).
    #[id = "stsd"] pub stars_sun_size: FloatParam,
    /// Sun warmth — tint from white (0) to deep sunset orange (1).
    #[id = "stsw"] pub stars_sun_warmth: FloatParam,

    // --- Physically based atmosphere (#100): a derived single-scattering sky baked
    // into the IBL + the terrain aerial perspective. A global world layer (not
    // preset-captured); the sun rides the terrain sun elevation/azimuth. ---
    /// Enable the physically based atmosphere (replaces the env/skybox + terrain
    /// sky with derived single scattering; bakes it into the IBL lighting).
    #[id = "atmen"] pub atmos_enabled: BoolParam,
    /// Turbidity — aerosol (Mie) density: low = clear deep-blue sky, high = hazy
    /// with a broad bright halo around the sun.
    #[id = "atmtb"] pub atmos_turbidity: FloatParam,
    /// Mie anisotropy g — how forward-peaked the aerosol scatter (sun aureole) is.
    #[id = "atmmg"] pub atmos_mie_g: FloatParam,
    /// Sun intensity — the radiance feeding the scattering (HDR).
    #[id = "atmsi"] pub atmos_sun_int: FloatParam,
    /// Ground albedo — a cheap multiple-scatter ambient lift from the lit ground.
    #[id = "atmga"] pub atmos_ground_albedo: FloatParam,
    /// Exposure — overall HDR gain on the baked sky.
    #[id = "atmex"] pub atmos_exposure: FloatParam,
    /// Aerial perspective — how strongly distant terrain fades into the sky colour.
    #[id = "atmar"] pub atmos_aerial: FloatParam,
    /// Rayleigh strength — scales the blue (1 = Earth-like).
    #[id = "atmry"] pub atmos_rayleigh: FloatParam,

    // --- Volumetric clouds (#102, Part A): a raymarched cloud layer in the terrain
    // sky. A global world layer (not preset-captured); only drawn while terrain is on. ---
    /// Enable volumetric clouds (replaces the flat value-noise cloud sheet).
    #[id = "clen"] pub clouds_enabled: BoolParam,
    /// Cloud coverage — how much of the sky fills with cloud (0 = clear, 1 = overcast).
    #[id = "ccov"] pub clouds_coverage: FloatParam,
    /// Cloud density — optical thickness (darker, more opaque cores).
    #[id = "cden"] pub clouds_density: FloatParam,
    /// Cloud base altitude (world units; the bottom of the cloud slab).
    #[id = "cbas"] pub clouds_base: FloatParam,
    /// Cloud layer thickness (world units; the slab height).
    #[id = "cthk"] pub clouds_thickness: FloatParam,
    /// PERF: cloud raymarch steps (the primary perf dial).
    #[id = "cstp"] pub clouds_steps: IntParam,
    /// Cloud detail — high-frequency erosion carving the billows/wisps.
    #[id = "cdet"] pub clouds_detail: FloatParam,
    /// Cloud drift speed (rides the terrain fly clock).
    #[id = "cdft"] pub clouds_drift: FloatParam,
    /// Forward scatter (Mie g) — the silver-lining / sun-behind glow anisotropy.
    #[id = "clhg"] pub clouds_hg: FloatParam,
    /// Light absorption — how fast the sun is extinguished inside the cloud.
    #[id = "cabs"] pub clouds_absorption: FloatParam,
    /// Cloud shadow strength — how much clouds darken the terrain beneath them.
    #[id = "cshd"] pub clouds_shadow: FloatParam,
    /// Ambient fill — sky light scattered into the shadowed cloud bottoms.
    #[id = "camb"] pub clouds_ambient: FloatParam,

    // --- FFT (Tessendorf) ocean (#102, Part B): a statistical wind-wave ocean. A
    // global world layer (not preset-captured). Enable with terrain OFF for an
    // infinite ocean-only world. ---
    /// Enable the FFT ocean (replaces the pooled reflective water).
    #[id = "ocen"] pub ocean_enabled: BoolParam,
    /// Sea level (world y). Used as the eye reference in ocean-only mode.
    #[id = "oclv"] pub ocean_level: FloatParam,
    /// Wind speed — drives the wave spectrum (bigger = longer swell, rougher sea).
    #[id = "ocws"] pub ocean_wind_speed: FloatParam,
    /// Wind direction (degrees) — the dominant wave heading.
    #[id = "ocwd"] pub ocean_wind_dir: FloatParam,
    /// Wave amplitude — overall height scale.
    #[id = "ocam"] pub ocean_amplitude: FloatParam,
    /// Choppiness — lateral displacement: sharper crests + more foam.
    #[id = "occh"] pub ocean_choppiness: FloatParam,
    /// Tile size (world units of one FFT tile) — the wave scale on screen.
    #[id = "octs"] pub ocean_tile_size: FloatParam,
    /// Foam strength on the wave crests + folds.
    #[id = "ocfm"] pub ocean_foam: FloatParam,
    /// Sun glitter — the sparkle of the sun off the wave slopes.
    #[id = "ocgl"] pub ocean_glitter: FloatParam,
    /// Water hue (0..1) — the deep-water colour.
    #[id = "ochu"] pub ocean_hue: FloatParam,
    /// Depth absorption — how the deep teal brightens toward the shallows/steep view.
    #[id = "ocdp"] pub ocean_depth: FloatParam,

    // --- Particle Aura (#81): a GPU cloud of luminous motes advected through the
    // active generator's velocity field. Off by default → image identical. ---
    /// Tier: Off, or Lite (advection — a drifting halo that hugs the structure).
    /// (Fluid / Navier–Stokes is a planned follow-up; not in this build.)
    #[id = "pator"] pub particles_tier: EnumParam<ParticleTier>,
    /// PERF: particle count, in thousands of motes (particles ≫ grid cells, so it
    /// looks far more detailed than the coarse velocity grid).
    #[id = "patct"] pub particles_count_k: IntParam,
    /// PERF: coarse velocity-grid resolution per axis (the field the motes sample).
    #[id = "patgr"] pub particles_grid_res: IntParam,
    /// Flow speed — how fast the motes ride the field.
    #[id = "patsp"] pub particles_speed: FloatParam,
    /// Lifetime (seconds) before a mote ages out and respawns near the geometry.
    #[id = "patlf"] pub particles_lifetime: FloatParam,
    /// Spawn radius — how far motes scatter around the structure's nodes (world).
    #[id = "patsr"] pub particles_spawn_radius: FloatParam,
    /// Mote size (world units of the additive billboard).
    #[id = "patsz"] pub particles_size: FloatParam,
    /// Emissive brightness (HDR; > 1 blooms in the post chain).
    #[id = "patem"] pub particles_emissive: FloatParam,
    /// Render motes as motion-blurred ribbons (stretched along velocity) vs points.
    #[id = "patrb"] pub particles_ribbon: BoolParam,
    /// Ribbon stretch — how far a mote elongates along its velocity.
    #[id = "patrs"] pub particles_ribbon_stretch: FloatParam,
    /// Palette hue rotation (0..1 turns): rotates the whole ember palette coherently
    /// (0 = warm orange ember → cyan / green / violet auras). The look is a speed→heat
    /// ramp (slow = deep ember, fast = white-hot spark), not a per-mote rainbow.
    #[id = "paths"] pub particles_hue_shift: FloatParam,
    /// Beat burst — how much the PLL beat pulse brightens/kicks the aura (0 = off).
    #[id = "patbb"] pub particles_beat_burst: FloatParam,
    /// Drag — velocity damping toward the field (0 = pure advection, up = laminar).
    #[id = "patdg"] pub particles_drag: FloatParam,
    /// Turbulence — extra curl jitter on top of the field (life for splatted fields).
    #[id = "pattb"] pub particles_turbulence: FloatParam,
    /// Overall opacity of the aura.
    #[id = "patal"] pub particles_alpha: FloatParam,
    /// Hide the generator geometry — it still stirs the motes, but only the
    /// particles render (the structure becomes an invisible force).
    #[id = "pathg"] pub particles_hide_generator: BoolParam,
    /// Shaded beads (#298 Tier 1): draw each mote as a **sphere-impostor droplet**
    /// that bears the shared IBL + key/fill lighting (chrome / pearl / glass beads
    /// reflecting the HDR environment) instead of the additive spark billboard. The
    /// energization glow + hue cycle survive as the beads' emissive. Off = the sparks,
    /// byte-identical.
    #[id = "patbd"] pub particles_beads: BoolParam,
    /// Bead metallic (#298 Tier 1): the droplets' PBR metalness (1 = chrome beads,
    /// 0 = dielectric pearls). Only read when `beads` is on.
    #[id = "patmt"] pub particles_metallic: FloatParam,
    /// Bead roughness (#298 Tier 1): the droplets' PBR roughness (0 = mirror-sharp
    /// reflection, 1 = soft matte). Only read when `beads` is on.
    #[id = "patrg"] pub particles_roughness: FloatParam,
    /// Bead material (#298 Tier 2): Standard (pearl) / Chrome (mirror) / Glass
    /// (Fresnel env reflect+refract) / Refractive (glass + murk). Only read when
    /// `beads` is on.
    #[id = "patma"] pub particles_material: EnumParam<ParticleMaterial>,
    /// Bead shape (#298 Tier 2): the impostor SDF — Sphere (analytic) / Ellipsoid /
    /// Teardrop / Rounded Box / Dice. Non-sphere shapes orient along the mote's
    /// velocity (teardrops streak the vortex). Only read when `beads` is on.
    #[id = "patsh"] pub particles_shape: EnumParam<ParticleShape>,
    /// Bead index of refraction (#298 Tier 2): Glass/Refractive bending strength
    /// (1 = none, ~1.33 water, ~1.5 glass). Only read for Glass/Refractive beads.
    #[id = "patio"] pub particles_ior: FloatParam,
    /// Bead shape parameter (#298 Tier 2): stretch/roundness of the non-sphere SDF
    /// (0 = compact, 1 = elongated/soft). Only read for non-sphere bead shapes.
    #[id = "patsa"] pub particles_shape_param: FloatParam,
    /// Beads in hardware RT (#298 Tier 4): put a curated subset of the largest
    /// droplets into the ray-tracing TLAS (a unit-sphere BLAS per bead) so they join
    /// RT reflections / GI / shadows + the path tracer. Needs the RT master toggle on.
    /// Off by default (the screen-space Tier-3 participation stands on its own).
    #[id = "patbr"] pub particles_beads_rt: BoolParam,
    /// Bead material hue (#305 Tier 1, turns): rotates/cycles the bead albedo colour.
    #[id = "pbhu"] pub particles_bead_hue: FloatParam,
    /// Bead material hue-cycle rate (turns per beat): auto-advances the bead hue.
    #[id = "pbhc"] pub particles_bead_hue_cycle: FloatParam,
    /// Bead material saturation (1 = unchanged, 0 = greyscale).
    #[id = "pbsa"] pub particles_bead_sat: FloatParam,
    /// Bead material value/brightness (1 = unchanged, 0 = black).
    #[id = "pbva"] pub particles_bead_val: FloatParam,
    /// Bead material emissive: emit the solid bead impostor's own colour (its
    /// Hue/Saturation/Value) into HDR — a self-emission that blooms. 0 = off.
    #[id = "pbem"] pub particles_bead_emissive: FloatParam,

    // --- Aura-Fluid (#81 showpiece): a GPU Navier–Stokes solver the motes ride;
    // only active at tier = Fluid. ---
    /// Stir force — how hard the generator's node motion drives the fluid.
    #[id = "fldfo"] pub fluid_force: FloatParam,
    /// Vorticity confinement — re-injects small eddies (the "more curl/whorls" dial).
    #[id = "fldvo"] pub fluid_vorticity: FloatParam,
    /// Dissipation — velocity damping per second (viscosity-ish; stabilizes + fades).
    #[id = "flddi"] pub fluid_dissipation: FloatParam,
    /// PERF: pressure-solve (Jacobi) iterations — higher = cleaner swirl, costlier.
    #[id = "fldit"] pub fluid_iters: IntParam,
    /// Inflow decay — how fast the injected source is forgotten so the wake lingers.
    #[id = "fldid"] pub fluid_inflow_decay: FloatParam,

    // --- Fluid Ink (#182 Tier 1): render the dye the generator stirs into the
    // fluid medium. Enabling it runs the fluid solver even without the Fluid
    // particle tier; `hide_generator` gives the pure-medium view. ---
    /// Master toggle: inject an RGB dye at the nodes + raymarch it as a lit volume.
    #[id = "inkon"] pub ink_enabled: BoolParam,
    /// Dye injection rate — how much ink each node sheds per second.
    #[id = "inkra"] pub ink_rate: FloatParam,
    /// Dye injection radius — the splat ball around each node, in grid cells.
    #[id = "inkrd"] pub ink_radius: FloatParam,
    /// Extinction (Beer–Lambert σ) — how optically thick the ink is.
    #[id = "inkex"] pub ink_extinction: FloatParam,
    /// Scatter — key-light in-scatter (+ IBL ambient) strength; 0 = unlit ink.
    #[id = "inksc"] pub ink_scatter: FloatParam,
    /// Emissive — the dye glows on its own (bioluminescent ink; > 1 blooms).
    #[id = "inkem"] pub ink_emissive: FloatParam,
    /// Anisotropy g — Henyey–Greenstein lobe (0 = isotropic, + = forward scatter
    /// / silver linings toward the key light, − = back scatter).
    #[id = "inkan"] pub ink_anisotropy: FloatParam,
    /// Dye dissipation — how fast the ink fades (its own clock, not the fluid's).
    #[id = "inkdi"] pub ink_dissipation: FloatParam,
    /// PERF: raymarch step budget across the fluid volume.
    #[id = "inkst"] pub ink_steps: FloatParam,
    /// MacCormack advection — error-corrected sharp filaments vs soft SL mush.
    #[id = "inkmc"] pub ink_maccormack: BoolParam,
    /// PERF: march at half resolution + depth-aware upsample (big win, soft edges).
    #[id = "inkhr"] pub ink_half_res: BoolParam,
    /// Reveal — a soft density threshold the march culls below (like the
    /// vector-field reveal): chips away the dilute haze crust so the dense
    /// vortex filaments inside show through. 0 = show everything.
    #[id = "inkrv"] pub ink_reveal: FloatParam,

    // --- Fluid medium, Tier 2 (#182): boundaries, buoyancy, micro-detail,
    // beat coupling, and the honest perf dials. All inert at defaults. ---
    /// Solid boundaries: node occupancy becomes moving no-slip walls — wakes
    /// shed off the structure, flow channels through lattices.
    #[id = "fl2bd"] pub fl2_boundaries: BoolParam,
    /// Buoyancy — vertical force per unit heat (+ smoke rises / − ink sinks).
    #[id = "fl2bu"] pub fl2_buoyancy: FloatParam,
    /// Heat decay — how fast fresh (hot) ink cools and stops rising (1/s).
    #[id = "fl2hd"] pub fl2_heat_decay: FloatParam,
    /// Render micro-detail — curl-noise swirl on the ink march, scaled by local
    /// vorticity (a coarse grid reads finer than it is).
    #[id = "fl2dt"] pub fl2_detail: FloatParam,
    /// Beat splash — radial momentum impulse on each beat (Pulse must be on).
    #[id = "fl2sp"] pub fl2_splash: FloatParam,
    /// Beat dye gate — fades dye injection toward beat-gated (1 = ink puffs
    /// only on the pulse).
    #[id = "fl2dg"] pub fl2_dye_gate: FloatParam,
    /// PERF: sim grid resolution override (0 = follow the aura's grid dial;
    /// 128³ is heavy — watch the frame time).
    #[id = "fl2rs"] pub fl2_res: IntParam,
    /// PERF: sim substeps per frame — stability for fast stirs (full solver
    /// cost per substep).
    #[id = "fl2ss"] pub fl2_substeps: IntParam,

    // --- MLS-MPM liquid (#182 Tier 3a): a free-surface liquid the generator
    // churns, rendered through the metaball isosurface path (Glass = water). ---
    /// Master toggle: simulate + draw the liquid pool.
    #[id = "liqon"] pub liq_enabled: BoolParam,
    /// PERF: particle count in thousands (reseeds on change).
    #[id = "liqct"] pub liq_count: IntParam,
    /// PERF: sim grid resolution per axis (reseeds on change).
    #[id = "liqgr"] pub liq_res: IntParam,
    /// Gravity (world units/s²).
    #[id = "liqgv"] pub liq_gravity: FloatParam,
    /// Stiffness — the equation of state; higher = less compressible, splashier.
    #[id = "liqst"] pub liq_stiffness: FloatParam,
    /// Viscosity — 0 = water, up = honey (APIC affine damping).
    #[id = "liqvi"] pub liq_viscosity: FloatParam,
    /// Container half-extent (world units; an invisible tank centred on the field).
    #[id = "liqcn"] pub liq_container: FloatParam,
    /// Open top — no ceiling: splashes go ballistic and fall back.
    #[id = "liqot"] pub liq_open_top: BoolParam,
    /// Generator collides: the nodes become moving no-slip obstacles that churn
    /// the pool.
    #[id = "liqcl"] pub liq_collide: BoolParam,
    /// Stir gain — scales the velocity the colliding nodes impose.
    #[id = "liqsr"] pub liq_stir: FloatParam,
    /// Surface density — scales the splatted iso field (thicker/fuller liquid).
    #[id = "liqde"] pub liq_density: FloatParam,
    /// Surface threshold — the iso level the liquid surface sits at.
    #[id = "liqth"] pub liq_threshold: FloatParam,
    /// Liquid hue (0..1 around the wheel).
    #[id = "liqhu"] pub liq_hue: FloatParam,
    /// Liquid saturation (0 = clear/grey, 1 = deeply tinted).
    #[id = "liqsa"] pub liq_sat: FloatParam,
    /// PERF: sim substeps per frame (stability for fast stirs, full cost each).
    #[id = "liqss"] pub liq_substeps: IntParam,
    /// Tank vertical offset (world units) — slides the whole simulation volume
    /// up/down off the smoothed field centre (0 = centred on the generator).
    #[id = "liqoy"] pub liq_offset_y: FloatParam,
    /// Container shape: box / sphere / cylinder / boundless (soft shell).
    #[id = "liqsh"] pub liq_shape: EnumParam<LiqShape>,
    /// Render reveal 0..1 — a soft spherical window on the liquid's density,
    /// so the surface trails off into space instead of showing tank walls.
    #[id = "liqrv"] pub liq_reveal: FloatParam,

    // --- Fluid light coupling (#182 Tier 4): the medium joins the light
    // transport — GI, shadows both ways, caustics, and the fluid pushing
    // back on the generator. All inert at 0. ---
    /// Fluid → GI: inject ink radiance + liquid occupancy into the VXGI
    /// bounce volume (glowing ink tints the bounce light; liquid occludes).
    #[id = "fgigi"] pub fgi_gi: FloatParam,
    /// Fluid shadows the scene: dye density attenuates the key light on
    /// geometry via a light-space transmittance LUT.
    #[id = "fgish"] pub fgi_shadow: FloatParam,
    /// Fluid receives shadows: the ink march samples the scene shadow map,
    /// so geometry shades the smoke.
    #[id = "fgirc"] pub fgi_receive: BoolParam,
    /// Two-way coupling: fluid velocity sampled at the generator's nodes
    /// drives a per-node sway spring — the structure moves with the water.
    #[id = "fgisw"] pub fgi_sway: FloatParam,
    /// Caustics amount: key light refracted through the liquid surface,
    /// splatted and projected onto geometry beneath.
    #[id = "causa"] pub ca_amount: FloatParam,
    /// Caustic sharpness — focuses the refraction pattern.
    #[id = "causs"] pub ca_sharpness: FloatParam,
    /// The liquid's own material (Use Scene = follow the Material selector).
    #[id = "liqmm"] pub liq_material: EnumParam<LiqMaterial>,
    /// Liquid metallic (used when the liquid material ≠ Use Scene).
    #[id = "liqme"] pub liq_metallic: FloatParam,
    /// Liquid roughness (ditto).
    #[id = "liqro"] pub liq_roughness: FloatParam,
    /// Liquid IOR (ditto; water = 1.33).
    #[id = "liqio"] pub liq_ior: FloatParam,
    /// Ghost light (#182 T4): a hidden generator keeps feeding probe GI,
    /// VXGI and the emissive-cube point lights — a pure GI/light emitter.
    #[id = "ghost"] pub ghost_light: BoolParam,
    /// Liquid render mode: isosurface (default) or refractive see-through.
    #[id = "liqrn"] pub liq_render: EnumParam<LiqRender>,
    /// Beer–Lambert absorption strength (refractive mode).
    #[id = "liqab"] pub liq_absorb: FloatParam,
    /// The liquid's own emissive glow (used when material ≠ Use Scene).
    #[id = "liqgl"] pub liq_glow: FloatParam,
    /// Liquid chrome purity (material block 2 — the scene dial, liquid-local).
    #[id = "liqcp"] pub liq_chrome_purity: FloatParam,
    /// Liquid glass clarity.
    #[id = "liqgc"] pub liq_glass_clarity: FloatParam,
    /// Liquid F0 override (Standard reflectance lift).
    #[id = "liqf0"] pub liq_f0: FloatParam,
    /// Liquid glass dispersion (spectral fringing).
    #[id = "liqdi"] pub liq_dispersion: FloatParam,
    /// Liquid glass caustic boost (through-body focus).
    #[id = "liqdc"] pub liq_gcaustic: FloatParam,
    /// Liquid thin-film interference.
    #[id = "liqtf"] pub liq_thin_film: FloatParam,
    /// Hardware ray tracing master (#195 Tier 0): build the BLAS/TLAS over the
    /// instanced field each frame. Draws nothing yet (Tier 0 is the foundation
    /// + its cost measurement); later tiers' RT effects hang off it. Silently
    /// inert on machines without ray-query support. A captured Look.
    #[id = "rton"] pub rt_enable: BoolParam,
    /// Hardware-RT debug view (per-display, NOT preset-captured — like HDR/MSAA).
    #[id = "rtdb"] pub rt_debug: EnumParam<RtDebugView>,
    /// Path tracer (#200 Tier 4): the ground-truth progressive path-tracer toggle.
    /// **Per-display, NOT preset-captured** (like HDR/MSAA/rt_debug) — a heavy
    /// reference mode, not a saved look. Drives `Shared.pathtrace_on`; the visual's
    /// **P** key toggles the same state (last-touched-wins).
    #[id = "pton"] pub pathtrace_enable: BoolParam,
    /// Path-tracer dielectric BTDF (#258 Tier 2): when on, the ground-truth path
    /// tracer grows from diffuse-only to a real two-interface dielectric for the
    /// Glass/Refractive materials (Fresnel reflect/transmit split, refract on entry
    /// AND exit, total-internal-reflection, Beer–Lambert body absorption) and a
    /// perfect mirror for Chrome. Off → the tracer stays diffuse-only, byte-identical
    /// to before. A captured Look.
    #[id = "ptdi"] pub pt_dielectric: BoolParam,
    /// Path-tracer Beer–Lambert absorption strength — the σ scale for rays travelling
    /// INSIDE the dielectric body (σ per channel = (1 − albedo) × this). 0 = clear
    /// glass (no absorption). Only affects the dielectric path (needs `pt_dielectric`).
    #[id = "ptab"] pub pt_absorb: FloatParam,
    /// Path-tracer composite mode: Replace (overwrite the frame — ground truth),
    /// Blend (cross-blend the trace over the raster PBR image by `pt_augment`), or
    /// GI add (add the tracer's INDIRECT light onto the raster — no double-count).
    /// Default Replace = the original behaviour, byte-identical. A captured Look.
    #[id = "ptcm"] pub pt_composite: EnumParam<PtComposite>,
    /// Path-tracer augment amount (0..1) for the Blend / GI-add composite modes:
    /// Blend = the trace's opacity over the raster; GI add = the indirect light's
    /// gain. 0 = the raster is untouched. Ignored in Replace mode.
    #[id = "ptau"] pub pt_augment: FloatParam,
    /// Spectral dispersion (#258 Tier 4): each traced path carries one wavelength and
    /// glass / the lens refracts at a per-λ Cauchy IOR — a prism / dispersive lens
    /// throws a real spectrum. Off → the RGB tracer, byte-identical. A captured Look.
    #[id = "spec"] pub spectral_enable: BoolParam,
    /// Abbe number (Vd) — dispersion strength. LOW (~25) = strong (dense flint, wide
    /// rainbow); HIGH (~80) = weak; large collapses to no dispersion.
    #[id = "sabb"] pub spectral_abbe: FloatParam,
    /// Extra stratified wavelengths per pixel (0..8) beyond the hero one — higher =
    /// less colour noise, more cost. Per-quality (not a saved Look).
    #[id = "ssec"] pub spectral_secondaries: IntParam,
    /// Photon-mapped caustics (#258 Tier 5): with the path tracer ON, a light-tracing
    /// pass fires photons from the key light through the specular chain (glass /
    /// chrome / the Lens, dispersed per-wavelength when spectral is on) and splats
    /// where they land — so the focused caustic a lens or prism casts ON a surface
    /// resolves in about a frame instead of thousands. Off → the tracer is
    /// byte-identical. A captured Look.
    #[id = "ptca"] pub pt_caustics: BoolParam,
    /// Photon budget per frame, in thousands (16k–1024k). More = smoother caustics,
    /// more GPU cost. Per-quality (not a saved Look).
    #[id = "ptcp"] pub pt_caustic_photons: IntParam,
    /// Caustic map gain (0 = invisible, 1 = physical). A captured Look.
    #[id = "ptci"] pub pt_caustic_intensity: FloatParam,
    /// Screen-space photon gather radius in pixels (the KDE blur) — larger = smoother
    /// but softer caustics; 0 = raw single-pixel splats. A captured Look.
    #[id = "ptcr"] pub pt_caustic_radius: FloatParam,

    // --- Neural radiance cache — live (#256 Tier 0). Off by default → byte-identical. ---
    /// Live neural radiance cache: with the path tracer ON, the visual trains the
    /// #200 Tier-6 SIREN `(pos, dir) → radiance` each frame and short paths
    /// **terminate into a cache query** at `terminate_bounce` instead of tracing on —
    /// infinite-bounce GI at short-path cost, blended by `confidence` so a cold cache
    /// can only lose GI, never corrupt the image. Off → the tracer is byte-identical.
    /// A captured Look.
    #[id = "nrce"] pub nrc_enable: BoolParam,
    /// How much of the cached radiance is trusted at path termination (the confidence
    /// blend). 0 = ignore the cache (raw trace), 1 = fully trust it.
    #[id = "nrcc"] pub nrc_confidence: FloatParam,
    /// Online SGD learning rate for the per-frame training (higher = tracks the light
    /// field faster but noisier). A captured Look.
    #[id = "nrcl"] pub nrc_learn_rate: FloatParam,
    /// SIREN feature frequency (ω) of the cache network — higher captures sharper
    /// spatial variation in the light field, lower is smoother. A captured Look.
    #[id = "nrco"] pub nrc_omega: FloatParam,
    /// Bounce depth at which a short path terminates into a cache query (1 = after the
    /// first bounce → cheapest / most cache-reliant; higher = trace more, cache less).
    #[id = "nrct"] pub nrc_terminate: IntParam,
    /// Training samples per frame — how many `(pos, dir) → radiance` pairs the cache
    /// fits each frame (more = faster convergence, more CPU). Per-quality feel, but
    /// captured with the Look so a preset restores the same training rate.
    #[id = "nrcn"] pub nrc_train_samples: IntParam,
    /// Cache init seed — reseeding restarts training from a fresh random network.
    #[id = "nrcd"] pub nrc_seed: IntParam,

    // --- Neural radiance cache — RT-stack synergies (#256 Tier 1). Off = byte-identical. ---
    /// NRC-guided importance sampling: choose each diffuse bounce direction by
    /// resampled importance sampling over cosine candidates weighted by the cache's
    /// predicted radiance — paths stop wasting themselves on dark directions, so the
    /// path-traced image converges faster at equal quality. Unbiased (the RIS reweight
    /// folds into throughput). Needs the radiance cache ON. A captured Look.
    #[id = "nrcg"] pub nrc_guide: BoolParam,
    /// Guiding candidate count — how many cosine directions the cache scores per
    /// bounce before resampling one (more = better guiding, more cache queries).
    #[id = "nrck"] pub nrc_guide_candidates: IntParam,
    /// Firefly suppression at the source: clamp each stochastic sample toward the
    /// cache mean (the cache IS the expected value), killing bright single-sample
    /// outliers before the denoiser has to. Needs the radiance cache ON. A captured Look.
    #[id = "nrcf"] pub nrc_firefly: BoolParam,
    /// Firefly clamp strength — a sample is capped at this multiple of the cache's
    /// expected radiance (lower = more aggressive de-noising, higher = only the
    /// worst outliers). A captured Look.
    #[id = "nrcy"] pub nrc_firefly_clamp: FloatParam,

    // --- Neural radiance cache — light-field uses (#256 Tier 2). Off = byte-identical. ---
    /// Cache GI: fill the bounced-GI probe volume from the continuous radiance cache
    /// instead of the discrete node integration — a learned, continuous bounce field
    /// that also lights the ink / fluid (they share the probe buffer). Supersedes the
    /// DDGI probe grid. Needs the radiance cache ON. A captured Look.
    #[id = "nrgi"] pub nrc_gi: BoolParam,
    /// Cache-GI strength — how strongly the cache's bounce lights surfaces (used in
    /// place of the Bounced-GI card's intensity while cache GI is on). A captured Look.
    #[id = "nrgs"] pub nrc_gi_strength: FloatParam,
    /// Cache-lit reflections: a Chrome/Glass secondary ray in the path tracer
    /// terminates into a cache query, so reflections show the LIT neighbours + off-
    /// screen light, not just the environment map. Needs the path tracer + the cache
    /// ON. A captured Look.
    #[id = "nrrf"] pub nrc_reflect: BoolParam,

    // --- Neural radiance cache — hard transport + volumetrics (#256 Tier 3). Off = byte-identical. ---
    /// Cache volumetrics: the path tracer marches the camera ray through a
    /// participating medium and queries the cache for the in-scattered radiance at
    /// each step → god-rays / haze that glow with the scene's light. Needs the path
    /// tracer + the cache ON. A captured Look.
    #[id = "nrvo"] pub nrc_volume: BoolParam,
    /// Volumetric medium density (extinction) — higher = thicker haze, more glow
    /// (saturating toward the in-scattered radiance). A captured Look.
    #[id = "nrvd"] pub nrc_volume_density: FloatParam,
    /// Volumetric march steps — more = smoother shafts, more cache queries. A captured Look.
    #[id = "nrvn"] pub nrc_volume_steps: IntParam,
    /// Volumetric glow strength (a final scale on the in-scattered light). A captured Look.
    #[id = "nrvs"] pub nrc_volume_strength: FloatParam,
    /// Cached caustics: at the primary hit the path tracer adds the cache's radiance
    /// along the mirror direction — the focused high-energy light a camera-first path
    /// can't find through glass — so it blooms. Needs the path tracer + the cache ON.
    /// A captured Look.
    #[id = "nrca"] pub nrc_caustic: BoolParam,
    /// Cached-caustic gain (0 = off, higher = brighter focused highlights). A captured Look.
    #[id = "nrcx"] pub nrc_caustic_gain: FloatParam,

    /// RT shadows (#195 Tier 1): one traced ray per pixel toward the key light
    /// supersedes the PCF shadow map — ground-truth occlusion, no bias/frustum
    /// tuning. Implies the TLAS build. A captured Look.
    #[id = "rtsh"] pub rt_shadows: BoolParam,
    /// RT shadow softness — the light's angular size (0 = hard edges). One
    /// jittered ray per pixel; TAA integrates it into a smooth penumbra.
    #[id = "rtss"] pub rt_shadow_soft: FloatParam,
    /// RT shadow strength (how dark full occlusion gets; the mix into the key).
    #[id = "rtst"] pub rt_shadow_strength: FloatParam,
    /// Also trace a second ray toward the FILL light (the PCF map never did).
    #[id = "rtsf"] pub rt_shadow_fill: BoolParam,
    /// RT reflections (#195 Tier 2): trace the reflected view ray against the
    /// TLAS — cubes reflect the ACTUAL scene (neighbours, off-screen emitters,
    /// behind-camera), with no screen-edge dropout. Supersedes SSR while on;
    /// a miss falls back to the env reflection with no seam. Implies the TLAS
    /// build. A captured Look.
    #[id = "rtre"] pub rt_reflect: BoolParam,
    /// RT reflection intensity (the confidence-weight scale, like SSR's).
    #[id = "rtri"] pub rt_reflect_intensity: FloatParam,
    /// Roughness cutoff — above this the env/IBL reflection stands (SSR's dial).
    #[id = "rtrr"] pub rt_reflect_rough: FloatParam,
    /// Reflection ray reach, as a multiple of the scene diagonal.
    #[id = "rtrd"] pub rt_reflect_reach: FloatParam,
    /// Trace a key-light shadow ray at each reflection hit (reflections
    /// contain shadows — one extra ray per reflective pixel).
    #[id = "rtrs"] pub rt_reflect_shadows: BoolParam,
    #[id = "rtry"] pub rt_reflect_rays: IntParam,
    /// AO source (#195 Tier 3): GTAO (screen-space, default) or short traced
    /// hemisphere rays against the TLAS. Lives under the Ambient Occlusion
    /// enable + radius dials; implies the TLAS build while RT. A captured Look.
    #[id = "aosrc"] pub ao_source: EnumParam<AoSource>,
    /// RT AO rays per pixel (1–4; TAA integrates the noise).
    #[id = "rtar"] pub rt_ao_rays: IntParam,
    /// RT diffuse GI (#195 Tier 4): gather one indirect bounce against the
    /// TLAS — real inter-cube colour bleed incl. off-screen emitters.
    /// Supersedes the SSGI march while on; implies the TLAS build. A Look.
    #[id = "rtgi"] pub rt_gi: BoolParam,
    /// RT GI intensity (the gathered-radiance scale).
    #[id = "rtgn"] pub rt_gi_intensity: FloatParam,
    /// RT GI rays per pixel (1–4; TAA integrates).
    #[id = "rtgr"] pub rt_gi_rays: IntParam,
    /// RT GI gather reach, as a multiple of the scene diagonal (how far
    /// indirect light travels).
    #[id = "rtgd"] pub rt_gi_reach: FloatParam,
    /// Trace a key-light shadow ray at each GI hit, so bounced light is
    /// itself shadowed (one extra ray per gather hit).
    #[id = "rtgs"] pub rt_gi_shadows: BoolParam,
    /// RT temporal accumulator (#200 Tier 4½ part 3): reproject + accumulate the
    /// RT reflection + GI buffers across frames — the temporal half of the
    /// denoiser, complementing the spatial filter. Off = today's raw jitter.
    /// A captured Look.
    #[id = "rttm"] pub rt_temporal: BoolParam,
    /// History feedback (how much of the accumulation to keep each frame; higher
    /// = smoother but more lag/ghosting).
    #[id = "rttf"] pub rt_temporal_feedback: FloatParam,
    /// Beat relax: how much a PLL beat kick drops the history weight (so it
    /// doesn't smear across the kick's fast camera motion). 0 = ignore the beat.
    #[id = "rttb"] pub rt_temporal_beat: FloatParam,
    /// Part 4 (variance-guided SVGF): swap the fixed feedback + raw box clamp for
    /// history-length-adaptive blending + a luminance σ-clamp — fresh pixels
    /// converge faster and fireflies stop swelling the clamp. Off = part 3.
    #[id = "rttv"] pub rt_temporal_variance: BoolParam,
    /// Max accumulated-sample count the adaptive blend ramps to (higher = the
    /// history weight climbs toward `feedback` more slowly / more smoothing).
    #[id = "rtta"] pub rt_temporal_accum: FloatParam,
    /// σ-clamp width γ: history luma is clamped to μ ± γ·σ (lower = tighter,
    /// less ghosting but more noise; higher = softer, more temporal smoothing).
    #[id = "rttc"] pub rt_temporal_clamp: FloatParam,
    /// RT denoise (#200 Tier 4½ part 2): edge-aware à-trous over the RT
    /// reflection + GI buffers — cleans the 1–4-spp grain without crossing
    /// depth/highlight edges. Reflections roughness-adaptive (sharp mirrors
    /// untouched). Off = today's raw jitter. A captured Look.
    #[id = "rtdn"] pub rt_denoise: BoolParam,
    /// RT denoise amount (blend toward the filtered result; 0..1).
    #[id = "rtda"] pub rt_denoise_amount: FloatParam,
    /// Neural denoiser (#200 Tier 5a): when on, the RT denoise step routes through
    /// a kernel-predicting neural filter (a seeded MLP modulates the classical
    /// bilateral kernel) instead of the plain à-trous. Off = classical (Tier 4½).
    /// Uses `rt_denoise_amount` as the overall blend. A captured Look.
    #[id = "ndde"] pub nd_enable: BoolParam,
    /// Network influence (0..1): how strongly the MLP reshapes the classical
    /// kernel. 0 = the classical filter, byte-for-byte.
    #[id = "ndst"] pub nd_strength: FloatParam,
    /// Filter-network seed: the whole weight set regenerates from this integer,
    /// so stepping it swaps the learned kernel behaviour.
    #[id = "ndsd"] pub nd_seed: IntParam,
    /// SIREN feature scale (ω): the network's first-layer frequency — higher =
    /// the kernel keys off finer feature differences.
    #[id = "ndom"] pub nd_omega: FloatParam,
    /// Learned upscaler (#200 Tier 5c): when on AND the composite is upscaling
    /// (render scale < 1), the bilinear DRS upscale becomes an HDR-safe content-
    /// adaptive sharpen reconstruction (a seeded-MLP-modulated gain). Off, or at
    /// full render scale, = plain bilinear (byte-identical). A captured Look.
    #[id = "upen"] pub up_enable: BoolParam,
    /// Sharpen strength (also the network influence): how hard edges are
    /// reconstructed. 0 = bilinear.
    #[id = "upsh"] pub up_sharpen: FloatParam,
    /// Upscaler-network seed: regenerates the per-pixel gain network.
    #[id = "upsd"] pub up_seed: IntParam,
    /// Neural field foundation (#200 Tier 0). Ships dark — nothing samples the
    /// MLP yet (Tier 1's generator is the first consumer); these are the compact
    /// control surface, host-automatable + preset-captured now so Tier 1 is a
    /// pure consumer. A captured Look.
    #[id = "nnen"] pub neural_enable: BoolParam,
    /// Network seed A: the whole weight set is regenerated from this integer, so
    /// stepping it swaps to an entirely different organism.
    #[id = "nnsa"] pub neural_seed_a: IntParam,
    /// Network seed B: the latent-walk destination (weights lerp A→B by `walk`).
    #[id = "nnsb"] pub neural_seed_b: IntParam,
    /// Latent walk: interpolates the weights from seed A (0) to seed B (1) for a
    /// continuous organism-to-organism morph.
    #[id = "nnwk"] pub neural_walk: FloatParam,
    /// Feature scale (SIREN ω): the first layer's frequency — higher = finer,
    /// busier detail in the neural field.
    #[id = "nnom"] pub neural_omega: FloatParam,
    /// Neural field generator (#200 Tier 1) — the raymarched isosurface controls
    /// (active when the generator is Neural field). All captured Looks.
    /// World radius the unit field is blown up to (the organism's size).
    #[id = "nnsc"] pub neural_scale: FloatParam,
    /// Spatial feature scale: multiplies the sample coords into the network, so
    /// higher = more, smaller features packed into the same volume.
    #[id = "nncd"] pub neural_coord: FloatParam,
    /// Isosurface threshold: the field level counted as the surface (shifts how
    /// much of the organism is solid).
    #[id = "nnis"] pub neural_iso: FloatParam,
    /// Raymarch step budget (perf ↔ thin-feature accuracy).
    #[id = "nnst"] pub neural_steps: IntParam,
    /// March step scale (sphere-trace relaxation): the field is not a true SDF,
    /// so lower = safer/slower (fewer overshoots), higher = faster/riskier.
    #[id = "nnmr"] pub neural_march: FloatParam,
    /// Colour intensity: 0 = near-white shading, 1 = the network's saturated
    /// per-point colour output.
    #[id = "nncl"] pub neural_color: FloatParam,
    /// Latent-walk rate in walk-cycles per beat: the PLL beat clock drives a
    /// triangle-wave morph between seed A and seed B. 0 = static (manual walk).
    #[id = "nnwr"] pub neural_walk_rate: FloatParam,
    /// Neural field **strand form** (#200 Tier 1b): sample the MLP on a grid and
    /// DISPLACE nodes instead of raymarching — the neural organism becomes a node
    /// field every Surface mode + Material + membrane skins. Off = raymarch (T1).
    #[id = "nnsm"] pub neural_strands_mode: BoolParam,
    /// Strand grid columns (the network's spatial resolution across the sheet).
    #[id = "nnsx"] pub neural_strands_cols: IntParam,
    /// Strand grid rows (nodes per strand).
    #[id = "nnsy"] pub neural_strands_rows: IntParam,
    /// Base-plane half-size the grid spans (world units; Breath scales it).
    #[id = "nnse"] pub neural_strands_extent: FloatParam,
    /// Displacement amplitude — how far the network's density pushes each node
    /// out of the base plane (0 = a flat sheet).
    #[id = "nnsd"] pub neural_strands_displace: FloatParam,
}

fn flin(name: &str, default: f32, min: f32, max: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min, max })
        .with_value_to_string(v2s_va())
}

/// Value-aware float formatter: decimals scale down as the magnitude grows
/// (≥1000 → none, ≥100 → 1, ≥10 → 2, else 3), trailing zeros trimmed. That caps
/// every readout at ~6 characters ("2160", "-999.9", "-99.99", "-9.999",
/// "0.005"), so a slider's value box always fits the fixed `VALUE_W` reserve in
/// the editor and never shows more than three decimals. Applied to every `flin`
/// float. Typing a value back still works: `FloatParam` falls back to its
/// default float parser when only `value_to_string` is set.
fn v2s_va() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| {
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
    })
}
fn ilin(name: &str, default: i32, min: i32, max: i32) -> IntParam {
    IntParam::new(name, default, IntRange::Linear { min, max })
}

/// The editor's default size in **logical points**, before any HiDPI scale factor.
///
/// Split out of the `Default` impl below because `src/standalone.rs` needs the same two
/// numbers: on Windows it works out how far it can scale the UI before the window would
/// stop fitting on the display, and "how big is the window" has to mean the same thing in
/// both places or the fit calculation is answering about a different window than the one
/// that opens.
pub const EDITOR_DEFAULT_W: u32 = 1280;
pub const EDITOR_DEFAULT_H: u32 = 860;

impl Default for OrganicMathParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(EDITOR_DEFAULT_W, EDITOR_DEFAULT_H),

            generator: EnumParam::new("Generator", GeneratorMode::Original),

            // Defaults are the "no modifiers" state: equal counts + every
            // deformation amp at 0 → reset renders a clean cube of cubes on a
            // unit grid. Dialing any amp up deforms it from there.
            loop_count_x: ilin("Loop Count X", 20, 1, 128),
            loop_count_y: ilin("Loop Count Y", 20, 1, 128),
            loop_count_z: ilin("Loop Count Z", 20, 1, 128),
            loop_count_q: ilin("Loop Count Q (strand)", 0, 0, 256),

            rot_func: EnumParam::new("Rotation Func", HostFuncName::Sin),
            rot_amp_x: flin("Rotation Amp X", 0.0, 0.0, 2160.0),
            rot_amp_y: flin("Rotation Amp Y", 0.0, 0.0, 2160.0),
            rot_amp_z: flin("Rotation Amp Z", 0.0, 0.0, 2160.0),
            // Per-axis rotation speed (× inc_scale per frame). Distinct defaults
            // desync the axes for organic motion; negative reverses.
            rot_mod_x: flin("Rotation Speed X", 0.6, -2.0, 2.0),
            rot_mod_y: flin("Rotation Speed Y", 0.8, -2.0, 2.0),
            rot_mod_z: flin("Rotation Speed Z", 1.0, -2.0, 2.0),
            continuous: BoolParam::new("Continuous Rotation", false),
            // 0 = constant spin (current look); up shapes the winding velocity.
            cont_shape: flin("Continuous Wave Depth", 0.0, 0.0, 1.0),

            trans_func: EnumParam::new("Translation Func", HostFuncName::Sin),
            trans_amp_x: flin("Translation Amp X", 0.0, 0.0, 20.0),
            trans_amp_y: flin("Translation Amp Y", 0.0, 0.0, 20.0),
            trans_amp_z: flin("Translation Amp Z", 0.0, 0.0, 20.0),
            trans_mod_x: flin("Translation Mod X", 0.0, -200.0, 200.0),
            trans_mod_y: flin("Translation Mod Y", 0.0, -200.0, 200.0),
            trans_mod_z: flin("Translation Mod Z", 0.0, -200.0, 200.0),

            scale_func: EnumParam::new("Scaling Func", HostFuncName::Log),
            scale_amp: flin("Scale Amp", 0.0, 0.0, 0.5),

            // Frenet defaults: constant κ + τ = a clean helix bundle (amps 0, so
            // no winding modulation until dialed up). 24 strands × 200 nodes.
            frenet_strands: ilin("Frenet Strands", 24, 1, 128),
            frenet_nodes: ilin("Frenet Nodes", 200, 2, 1024),
            frenet_step: flin("Frenet Step (ds)", 0.12, 0.001, 1.0),
            frenet_func: EnumParam::new("Frenet Func", HostFuncName::Sin),
            frenet_kappa: flin("Frenet Curvature", 0.35, 0.0, 3.0),
            frenet_kappa_amp: flin("Frenet Curvature Amp", 0.0, 0.0, 50.0),
            frenet_kappa_freq: flin("Frenet Curvature Freq", 1.0, 0.0, 8.0),
            frenet_tau: flin("Frenet Torsion", 0.12, -10.0, 10.0),
            frenet_tau_amp: flin("Frenet Torsion Amp", 0.0, 0.0, 2.0),
            frenet_tau_freq: flin("Frenet Torsion Freq", 1.0, 0.0, 8.0),
            frenet_spread: flin("Frenet Spread", 0.15, 0.0, 10.0),
            frenet_thickness: flin("Frenet Thickness", 0.25, 0.01, 2.0),

            // DNA defaults: relaxed B-DNA (σ=0 → straight spine), 48 bp, seq seed 1.
            dna_form: EnumParam::new("DNA Form", DnaForm::B),
            dna_bp: ilin("DNA Base Pairs", 48, 4, 600),
            dna_bp_per_turn: flin("DNA bp/turn", 10.5, 4.0, 16.0),
            dna_rise: flin("DNA Rise (Å)", 3.32, 1.5, 5.0),
            dna_radius: flin("DNA Radius (Å)", 10.0, 5.0, 16.0),
            dna_groove: flin("DNA Groove Δ (°)", 144.0, 60.0, 180.0),
            dna_left: BoolParam::new("DNA Left-handed", false),
            dna_sigma: flin("DNA Supercoil σ", 0.0, -0.1, 0.1),
            dna_super_radius: flin("DNA Superhelix Radius (Å)", 24.0, 0.0, 80.0),
            dna_seed: ilin("DNA Sequence Seed", 1, 0, 65535),
            dna_thickness: flin("DNA Thickness", 0.16, 0.01, 1.0),
            dna_twist_breathe: flin("DNA Twist Breathe", 0.0, 0.0, 4.0),

            // Attractor defaults: Lorenz, 6 seeds, a 300-frame flowing trail.
            attr_field: EnumParam::new("Attractor Field", AttractorField::Lorenz),
            attr_seeds: ilin("Attractor Seeds", 6, 1, 64),
            attr_seed: ilin("Attractor Seed Value", 1, 0, 65535),
            attr_spread: flin("Attractor Spread", 0.6, 0.0, 5.0),
            attr_dt: flin("Attractor Step ×dt", 1.0, 0.1, 4.0),
            attr_trail: ilin("Attractor Trail", 300, 4, 1024),
            attr_speed: flin("Attractor Head Speed", 1.0, 0.0, 8.0),
            attr_scale: flin("Attractor Scale", 1.0, 0.1, 4.0),
            attr_thickness: flin("Attractor Thickness", 0.12, 0.01, 1.0),

            // Boids defaults (#52): 120 agents flocking in a radius-6 cage, a
            // 64-frame trail, gentle beat-pulsed gather. First-guess feel — all
            // tunable on the Mac.
            boids_count: ilin("Boids Count", 120, 1, 2000),
            boids_perception: flin("Boids Perception", 3.0, 0.1, 20.0),
            boids_separation: flin("Boids Separation", 1.2, 0.05, 10.0),
            boids_sep: flin("Boids Separation Weight", 1.5, 0.0, 4.0),
            boids_align: flin("Boids Alignment Weight", 1.0, 0.0, 4.0),
            boids_cohere: flin("Boids Cohesion Weight", 1.0, 0.0, 4.0),
            boids_max_speed: flin("Boids Max Speed", 3.0, 0.1, 20.0),
            boids_max_force: flin("Boids Max Force", 4.0, 0.1, 20.0),
            boids_trail: ilin("Boids Trail", 64, 2, 512),
            boids_bounds: flin("Boids Bounds", 6.0, 1.0, 40.0),
            boids_goal: flin("Boids Goal Pull", 0.3, 0.0, 4.0),
            boids_thickness: flin("Boids Thickness", 1.5, 0.01, 8.0),
            boids_seed: ilin("Boids Seed", 1, 0, 65535),
            boids_speed: flin("Boids Sim Speed", 1.0, 0.0, 8.0),
            boids_scale: flin("Boids Scale", 30.0, 1.0, 200.0),
            boids_form: EnumParam::new("Boids Form", BoidsForm::Fish),
            boids_size: flin("Boids Creature Size", 14.0, 1.0, 80.0),
            boids_bank: flin("Boids Banking", 0.6, 0.0, 3.0),

            // Harmonic defaults: a Y₂₀ + Y₃₀ pulsing bell on a 48×64 grid.
            harm_mode0: ilin("Harm Mode 0", 4, 0, 15), // Y(2,0)
            harm_amp0: flin("Harm Amp 0", 0.5, -2.0, 2.0),
            harm_freq0: flin("Harm Freq 0", 1.0, 0.0, 8.0),
            harm_mode1: ilin("Harm Mode 1", 8, 0, 15), // Y(3,0)
            harm_amp1: flin("Harm Amp 1", 0.25, -2.0, 2.0),
            harm_freq1: flin("Harm Freq 1", 0.5, 0.0, 8.0),
            harm_mode2: ilin("Harm Mode 2", 6, 0, 15), // Y(2,2)
            harm_amp2: flin("Harm Amp 2", 0.0, -2.0, 2.0),
            harm_freq2: flin("Harm Freq 2", 0.75, 0.0, 8.0),
            harm_radius: flin("Harm Radius", 6.0, 0.5, 20.0),
            harm_theta: ilin("Harm θ Resolution", 48, 2, 256),
            harm_phi: ilin("Harm φ Resolution", 64, 3, 256),
            harm_thickness: flin("Harm Thickness", 0.12, 0.01, 1.0),
            bell_physical: BoolParam::new("Bell Physical", false),
            bell_stroke_depth: flin("Bell Stroke Depth", 0.5, 0.0, 0.95),
            bell_stiffness: ilin("Bell Stiffness", 8, 1, 40),
            bell_damping: flin("Bell Damping", 0.99, 0.8, 1.0),
            bell_open: flin("Bell Openness", 1.7, 0.3, 3.0),
            bell_speed: flin("Bell Stroke Rate", 0.1, 0.0, 1.0),

            // L-system defaults: a depth-4 fern, 25° turns, fully grown, no sway.
            ls_system: EnumParam::new("L-system", LSystem::Fern),
            ls_depth: ilin("L-system Depth", 4, 0, 7),
            ls_angle: flin("L-system Angle (°)", 25.0, 0.0, 90.0),
            ls_step: flin("L-system Step", 0.5, 0.02, 4.0),
            ls_sway_amp: flin("L-system Sway Amp (°)", 0.0, 0.0, 30.0),
            ls_sway_freq: flin("L-system Sway Freq", 1.0, 0.0, 8.0),
            ls_grow: flin("L-system Growth", 1.0, 0.0, 1.0),
            ls_thickness: flin("L-system Thickness", 0.08, 0.01, 1.0),

            // Curl-noise defaults: 12 particles, 200-step streamlines, free flow.
            cn_seeds: ilin("Curl Seeds", 12, 1, 256),
            cn_seed: ilin("Curl Seed Value", 1, 0, 65535),
            cn_spread: flin("Curl Spread", 4.0, 0.0, 20.0),
            cn_scale: flin("Curl Field Scale", 0.3, 0.02, 2.0),
            cn_steps: ilin("Curl Steps", 200, 2, 1024),
            cn_dt: flin("Curl Step dt", 0.08, 0.005, 0.5),
            cn_flow: flin("Curl Flow Speed", 1.0, 0.0, 8.0),
            cn_bound: flin("Curl Containment", 0.0, 0.0, 2.0),
            cn_thickness: flin("Curl Thickness", 0.1, 0.01, 1.0),

            // Polarization: a warm 4×20 radiating bloom of 64-node E helices over a
            // 150° cone — immediately reads as the "eye"; toggle B for the duplex.
            pol_rings: ilin("Pol Rings (θ)", 4, 1, 64),
            pol_spokes: ilin("Pol Spokes (φ)", 20, 1, 256),
            pol_samples: ilin("Pol Samples/Ray", 64, 2, 512),
            pol_len: flin("Pol Ray Length", 14.0, 1.0, 60.0),
            pol_k: flin("Pol Wavenumber k", 1.6, 0.0, 12.0),
            pol_amp: flin("Pol Amplitude", 2.2, 0.0, 12.0),
            pol_falloff: flin("Pol Falloff (1/r)", 0.0, 0.0, 1.0),
            pol_handed: BoolParam::new("Pol Left-handed", false),
            pol_spread: flin("Pol Spread (°)", 150.0, 0.0, 180.0),
            pol_swirl: flin("Pol Swirl", 0.0, -4.0, 4.0),
            pol_show_b: BoolParam::new("Pol Show B Helix", false),
            pol_thickness: flin("Pol Thickness", 0.12, 0.01, 1.0),

            // Maxwell: one oscillating dipole on a 5×24 lattice over the full sphere
            // → the rotating radiation lobe. Switch to field lines + 2 charges for
            // the dipole rose.
            mx_lines: BoolParam::new("Mx Field Lines", false),
            mx_gen_blend: flin("Generator E↔B", 0.0, 0.0, 1.0),
            mx_dipoles: BoolParam::new("Mx Dipoles", true),
            mx_sources: ilin("Mx Sources", 1, 1, 8),
            mx_separation: flin("Mx Separation", 6.0, 0.0, 30.0),
            mx_phase: flin("Mx Phase Offset", 1.57, 0.0, 6.28),
            mx_swirl: flin("Mx Swirl", 0.0, -4.0, 4.0),
            mx_near: flin("Mx Near-field", 0.0, 0.0, 1.0),
            mx_k: flin("Mx Wavenumber k", 1.2, 0.0, 12.0),
            mx_amp: flin("Mx Amplitude", 3.0, 0.0, 20.0),
            mx_rmin: flin("Mx Source Clamp", 0.6, 0.05, 5.0),
            mx_thickness: flin("Mx Thickness", 0.12, 0.01, 1.0),
            mx_rings: ilin("Mx Rings (θ)", 5, 1, 64),
            mx_spokes: ilin("Mx Spokes (φ)", 24, 1, 256),
            mx_samples: ilin("Mx Samples/Ray", 48, 2, 512),
            mx_raylen: flin("Mx Ray Length", 12.0, 1.0, 60.0),
            mx_spread: flin("Mx Spread (°)", 180.0, 0.0, 360.0),
            mx_norm_field: BoolParam::new("Unit-Field Spokes", false),
            mx_seeds: ilin("Mx Seeds/Source", 8, 1, 128),
            mx_steps: ilin("Mx Line Steps", 200, 2, 2048),
            mx_ds: flin("Mx Line Step ds", 0.15, 0.01, 1.0),
            mx_bound: flin("Mx Line Bound", 40.0, 2.0, 200.0),
            // Tempo sync off by default (free-running Speed clock = the historical look);
            // when on, one full oscillation per quarter note.
            mx_osc_sync: BoolParam::new("Mx Osc Tempo Sync", false),
            mx_osc_div: EnumParam::new("Mx Osc Division", OscDivision::Quarter),
            // E↔B phase: 0° = far-field / in-phase (today's lock); dial toward 90° for
            // the near-field induction look (B swirl in quadrature with the E wave).
            mx_eb_phase: flin("Mx E↔B Phase (°)", 0.0, 0.0, 90.0),

            // #412 Phase 0 FDTD. Off (byte-identical) but sane defaults so it runs on
            // toggle. Match ipc::Shared::default().fdtd exactly (the Default→Shared golden).
            fdtd_on: BoolParam::new("FDTD Solver", false),
            fdtd_res: flin("FDTD Resolution", 64.0, 16.0, 96.0),
            fdtd_source: EnumParam::new("FDTD Source", FdtdSource::Pulse),
            fdtd_freq: flin("FDTD Frequency", 8.0, 0.5, 40.0),
            fdtd_drive: flin("FDTD Drive", 1.0, 0.0, 20.0),
            fdtd_substeps: flin("FDTD Substeps", 4.0, 1.0, 16.0),
            fdtd_boundary: flin("FDTD Sponge Cells", 8.0, 0.0, 24.0),
            fdtd_extent: flin("FDTD Extent", 12.0, 2.0, 40.0),

            // Acoustic (#325): a dipole on a 5×24 lattice → the figure-8 pressure
            // shell (geometry blend 0 = pressure), motes advecting along the
            // velocity (aura blend 1 = velocity). Beat pump off → byte-identical
            // audio behaviour until turned up.
            ac_source: EnumParam::new("Ac Source", AcousticSource::Dipole),
            ac_k: flin("Ac Wavenumber k", 1.5, 0.0, 12.0),
            ac_near: flin("Ac Circulation", 0.5, 0.0, 1.0),
            ac_amp: flin("Ac Amplitude", 1.5, 0.0, 20.0),
            ac_separation: flin("Ac Separation", 1.5, 0.0, 20.0),
            ac_rmin: flin("Ac Source Clamp", 0.3, 0.05, 5.0),
            ac_blend: flin("Ac Geometry Compress↔Transverse", 0.0, 0.0, 1.0),
            ac_norm_field: BoolParam::new("Ac Unit-Field Spokes", false),
            ac_rings: ilin("Ac Rings (θ)", 5, 1, 64),
            ac_spokes: ilin("Ac Spokes (φ)", 24, 1, 256),
            ac_samples: ilin("Ac Samples/Ray", 48, 2, 512),
            ac_raylen: flin("Ac Ray Length", 8.0, 1.0, 60.0),
            ac_spread: flin("Ac Spread (°)", 180.0, 0.0, 360.0),
            ac_thickness: flin("Ac Thickness", 0.1, 0.01, 1.0),
            ac_aura_blend: flin("Ac Aura Compress↔Transverse", 1.0, 0.0, 1.0),
            ac_beat_pump: flin("Ac Beat Pump", 0.0, 0.0, 8.0),

            // #380 Density-Map Attractor (Tier 1): static a/b, ~60K points.
            ma_kind: EnumParam::new("Ma Map", MapKindParam::Complexus),
            ma_a: flin("Ma a", 1.5, -6.283, 6.283),
            ma_b: flin("Ma b", 1.5, -6.283, 6.283),
            ma_c: flin("Ma c", 1.5, -6.283, 6.283),
            ma_d: flin("Ma d", 1.5, -6.283, 6.283),
            ma_color: EnumParam::new("Ma Color", MapColorParam::StepSpeed),
            ma_points_k: ilin("Ma Points (K)", 60, 1, 400),
            ma_warmup: ilin("Ma Warm-up", 50, 0, 2000),
            ma_scale: flin("Ma Scale", 12.0, 0.5, 60.0),
            ma_size: flin("Ma Point Size", 0.08, 0.005, 1.0),
            ma_intensity: flin("Ma Intensity", 1.0, 0.0, 8.0),
            ma_a_drive: flin("Ma Anim>A", 0.0, 0.0, 1.0),
            ma_b_drive: flin("Ma Anim>B", 0.0, 0.0, 1.0),
            // #380 Tier 2 parameter orbit — default Linear (Tier-1-compatible; with the
            // drives at 0 the field is byte-identical). Lissajous defaults: 16-beat loop,
            // radii 1.5, fa=1/fb=2 figure-8, ψ=π/2, slow free-run.
            ma_orbit: EnumParam::new("Ma Orbit", MapOrbitModeParam::Linear),
            ma_loop_beats: flin("Ma Loop (beats)", 16.0, 0.25, 128.0),
            ma_orbit_ra: flin("Ma Orbit Ra", 1.5, 0.0, 6.283),
            ma_orbit_rb: flin("Ma Orbit Rb", 1.5, 0.0, 6.283),
            ma_orbit_fa: ilin("Ma Orbit fa", 1, 1, 8),
            ma_orbit_fb: ilin("Ma Orbit fb", 2, 1, 8),
            ma_orbit_psi: flin("Ma Orbit ψ", std::f32::consts::FRAC_PI_2, 0.0, 6.283),
            ma_orbit_free: flin("Ma Orbit Free Rate", 0.05, 0.0, 2.0),
            // #381 Tier 1 Field Engine — defaults render the Coulomb gallery preset.
            field_kind: EnumParam::new("Fe Kind", FieldKind::Auto),
            field_preset: EnumParam::new("Fe Phenomenon", FieldPreset::Coulomb),
            field_scale: flin("Fe Domain Scale k", 1.0, 0.05, 8.0),
            field_extent: flin("Fe Box Extent", 6.0, 0.5, 40.0),
            field_a: flin("Fe Coeff a", 1.0, -8.0, 8.0),
            field_b: flin("Fe Coeff b", 1.0, -8.0, 8.0),
            field_density: ilin("Fe Seeds / Res", 12, 1, 64),
            field_gain: flin("Fe Gain", 1.0, 0.0, 8.0),
            field_thickness: flin("Fe Thickness", 0.12, 0.01, 1.0),
            // #381 Tier 3 PDE dynamics — Off by default (static field byte-identical).
            pde_preset: EnumParam::new("Fe PDE", PdePreset::Off),
            sim_diffusion: flin("Fe Sim D", 1.0, 0.0, 8.0),
            sim_time_scale: flin("Fe Sim Speed", 1.0, 0.0, 16.0),
            sim_feed: flin("Fe Sim Feed", 0.037, 0.0, 0.12),
            sim_kill: flin("Fe Sim Kill", 0.06, 0.0, 0.12),
            sim_potential: flin("Fe Sim Potential", 1.0, 0.0, 8.0),
            sim_forcing: flin("Fe Sim Forcing", 0.0, 0.0, 4.0),
            sim_res: ilin("Fe Sim Res", 64, 16, 128),

            // #339 Duo-Field synthesis: off by default (byte-identical passthrough).
            sn_on: BoolParam::new("Sn Synth On", false),
            sn_play_mode: EnumParam::new("Sn Play Mode", SynthPlayMode::Generative),
            sn_gain: flin("Sn Gain", 0.6, 0.0, 2.0),
            sn_mix: flin("Sn Wet", 1.0, 0.0, 1.0),
            sn_tuning: flin("Sn k→Pitch (Hz/k)", 110.0, 10.0, 800.0),
            sn_gen_amp: flin("Sn Gen Amp", 1.0, 0.0, 4.0),
            sn_mode: EnumParam::new("Sn Engine Mode", SynthMode::Probes),
            sn_bank: ilin("Sn Lattice Bank", 32, 1, 64),
            sn_tuning_layout: EnumParam::new("Sn Lattice Tuning", TuningLayout::Octaves),
            sn_tune_spread: flin("Sn Lattice Spread", 0.25, 0.0, 2.0),
            sn_tune_stretch: flin("Sn Lattice Stretch", 0.0, 0.0, 0.2),
            sn_shell_r: flin("Sn Lattice Shell R", 2.5, 0.2, 20.0),
            sn_shell_rate: flin("Sn Lattice Breathe (Hz)", 1.0, 0.0, 8.0),
            sn_t60: flin("Sn Modal Decay (s)", 1.5, 0.02, 12.0),
            sn_bright: flin("Sn Modal Brightness", 0.5, 0.0, 1.0),
            sn_grain_size: flin("Sn Grain Size (s)", 0.06, 0.005, 0.4),
            sn_grain_density: flin("Sn Grain Density", 0.5, 0.0, 1.0),
            sn_attack: flin("Sn Attack", 0.01, 0.0, 4.0),
            sn_decay: flin("Sn Decay", 0.2, 0.0, 4.0),
            sn_sustain: flin("Sn Sustain", 0.7, 0.0, 1.0),
            sn_release: flin("Sn Release", 0.4, 0.0, 8.0),
            sn_glide: flin("Sn Glide", 0.0, 0.0, 2.0),
            sn_bend_range: flin("Sn Bend Range", 2.0, 0.0, 24.0),
            sn_place_spread: flin("Sn Keyboard Spread", 0.0, 0.0, 2.0),
            sn_a4: flin("Sn Concert A", 440.0, 400.0, 480.0),
            sn_probe_lx: flin("Sn Probe L X", -0.09, -8.0, 8.0),
            sn_probe_ly: flin("Sn Probe L Y", 0.0, -8.0, 8.0),
            sn_probe_lz: flin("Sn Probe L Z", 1.2, -8.0, 8.0),
            sn_probe_rx: flin("Sn Probe R X", 0.09, -8.0, 8.0),
            sn_probe_ry: flin("Sn Probe R Y", 0.0, -8.0, 8.0),
            sn_probe_rz: flin("Sn Probe R Z", 1.2, -8.0, 8.0),
            sn_probe_cam: BoolParam::new("Sn Probe Rides Camera", false),
            sn_vis_pivot: flin("Sn Lens Pivot", 110.0, 20.0, 2000.0),
            sn_vis_anchor: flin("Sn Lens Rate", 0.5, 0.0, 4.0),
            sn_vis_slope: flin("Sn Lens Slope", 0.34, 0.0, 1.0),
            sn_vis_k_anchor: flin("Sn Lens k", 1.5, 0.0, 12.0),
            sn_vis_k_slope: flin("Sn Lens k Slope", 0.34, 0.0, 1.0),
            sn_vis_quantize: EnumParam::new("Sn Lens Quantize", SynthQuantize::Free),
            // Acoustic Tier 4 (#325): Radiating model + intensity 0 → identical to
            // Tiers 1–3; a (2,2,1) cavity mode in a box of half-extent 8 when selected.
            ac2_model: EnumParam::new("Ac Model", AcousticModel::Radiating),
            ac2_nx: ilin("Ac Cavity nx", 2, 0, 8),
            ac2_ny: ilin("Ac Cavity ny", 2, 0, 8),
            ac2_nz: ilin("Ac Cavity nz", 1, 0, 8),
            ac2_morph: flin("Ac Cavity Beat Morph", 0.0, 0.0, 1.0),
            ac2_cav_scale: flin("Ac Cavity Scale", 8.0, 2.0, 40.0),
            ac2_intensity: flin("Ac Intensity Flux", 0.0, 0.0, 4.0),
            // Tier 5: soft mode-glide (0.6) + per-axis audio breathe off by default.
            ac2_tween: flin("Ac Cavity Tween", 0.6, 0.0, 1.0),
            ac2_audio_x: flin("Ac Cavity Audio→X", 0.0, 0.0, 8.0),
            ac2_audio_y: flin("Ac Cavity Audio→Y", 0.0, 0.0, 8.0),
            ac2_audio_z: flin("Ac Cavity Audio→Z", 0.0, 0.0, 8.0),

            // Maxwell field energization (#247 Tier 1): off; gain 1, knee 4, ember hue.
            mn_energize: BoolParam::new("Mx Energize Aura", false),
            mn_gain: flin("Mx Energy Gain", 1.0, 0.0, 8.0),
            mn_knee: flin("Mx Energy Knee", 4.0, 0.1, 20.0),
            mn_hue: flin("Mx Energy Hue", 0.08, 0.0, 1.0),
            // Finite antenna (#247 Tier 2): off; a half-wave-ish rod, L 6 along Z.
            mn_antenna: BoolParam::new("Mx Antenna (finite rod)", false),
            mn_antenna_len: flin("Mx Antenna Length", 6.0, 0.5, 40.0),
            // Fluid dye injection (#247 Tier 3): off.
            mn_dye_inject: flin("Mx Energy → Dye", 0.0, 0.0, 4.0),
            mx_aura_blend: flin("Aura E↔B", 0.0, 0.0, 1.0),
            // Field-force drive (#248): off; gain 1, core contrast 1 (= current look).
            mn_force: BoolParam::new("Mx Force Drive", false),
            mn_force_gain: flin("Mx Force Strength", 1.0, 0.0, 100.0),
            mn_energy_contrast: flin("Mx Energy Contrast", 1.0, 0.25, 4.0),
            mn_stir_rate: flin("Mx Stir Rate", 0.3, 0.0, 4.0),
            mn_pump: flin("Mx Acoustic Pump", 0.0, 0.0, 30.0),
            mn_swirl_beat: flin("Mx Beat Spin Force", 0.0, 0.0, 4.0),
            mn_pump_scale: flin("Mx Pump Size", 3.0, 0.5, 20.0),
            mn_swirl_decay: flin("Mx Spin Slowdown", 1.5, 0.1, 8.0),
            mn_mode_mix: flin("Mx Beat Mode", -1.0, -1.0, 1.0),
            mn_ring_freq: flin("Mx Ring Frequency", 2.0, 0.1, 20.0),
            mn_hue_cycle: flin("Mx Hue Cycle", 0.0, 0.0, 2.0),
            // Audio-driven dipole (#248 Tier 1): off; unity RMS→drive, floor 0.1.
            ad_drive: BoolParam::new("Audio Drive Dipole", false),
            ad_amount: flin("Audio Drive Amount", 1.0, 0.0, 4.0),
            ad_floor: flin("Audio Drive Floor", 0.1, 0.0, 1.0),
            // Spectrum → multipole (#248 Tier 2): off; gentle wavelength spread,
            // colour-by-band mostly on (inert until multipole mode engages).
            ad_multipole: BoolParam::new("Audio Spectrum Multipole", false),
            ad_spread: flin("Audio Band Spread", 0.25, 0.0, 1.0),
            ad_band_hue: flin("Audio Band Hue", 0.7, 0.0, 1.0),
            ad_stereo: flin("Audio Stereo Lean", 0.5, 0.0, 1.0),
            ad_pitch: flin("Audio Pitch Rate", 0.5, 0.0, 2.0),
            ad_wave: flin("Audio Waveform Shells", 0.0, 0.0, 1.0),
            // Axon Waveguide (#218 Tier 1): a 24-fibre bundle, 24 long, radius 4,
            // Ranvier nodes every 3 with a moderate pinch, a slow staggered pulse.
            ax_count: ilin("Ax Fibres", 24, 1, 256),
            ax_length: flin("Ax Length", 24.0, 2.0, 80.0),
            ax_bundle: flin("Ax Bundle Radius", 4.0, 0.0, 30.0),
            ax_samples: ilin("Ax Samples/Fibre", 64, 2, 512),
            ax_thickness: flin("Ax Thickness", 0.16, 0.01, 1.0),
            ax_node_spacing: flin("Ax Node Spacing", 3.0, 0.2, 20.0),
            ax_node_dip: flin("Ax Node Pinch", 0.55, 0.0, 1.0),
            ax_pulse_speed: flin("Ax Pulse Speed", 0.6, 0.0, 6.0),
            ax_pulse_width: flin("Ax Pulse Width", 0.10, 0.01, 1.0),
            ax_stagger: flin("Ax Stagger", 1.0, 0.0, 1.0),
            ax_splay: flin("Ax Splay", 0.0, -1.0, 1.0),
            ax_seed: ilin("Ax Seed", 1, 0, 999),
            ax_mode: EnumParam::new("Ax Mode", AxonMode::Lp01),
            ax_mode_amount: flin("Ax Mode Amount", 0.0, 0.0, 1.0),
            // Brain-like out of the box: a gentle C-arc + a touch of tortuosity,
            // and a modest bend so the outer fibres leak/flare (the combined look).
            ax_bend: flin("Ax Bend", 0.35, 0.0, 1.0),
            ax_curve: flin("Ax Curve", 0.6, 0.0, 1.0),
            ax_tortuosity: flin("Ax Tortuosity", 0.3, 0.0, 1.0),
            ax_dti: flin("Ax DTI Colour", 0.0, 0.0, 1.0),
            ax_dispersion: flin("Ax Dispersion", 0.0, 0.0, 1.0),
            ax_polarization: flin("Ax Polarization", 0.0, 0.0, 1.0),

            // Neural Network (#226 Tier 1): a small-world ring of 48 nodes, k = 6,
            // 15% rewired — the canonical Watts–Strogatz picture — with slim glowing
            // tracts and a slow travelling pulse. Best in Swept Tubes + Glass.
            nw_topology: EnumParam::new("NN Topology", NeuralTopology::SmallWorld),
            nw_nodes: ilin("NN Nodes", 48, 2, 1024),
            nw_connectivity: ilin("NN Connectivity", 6, 1, 64),
            nw_rewire: flin("NN Rewire / Radius", 0.15, 0.0, 1.0),
            nw_layers: ilin("NN Layers", 4, 2, 64),
            nw_seed: ilin("NN Seed", 1, 0, 999),
            nw_extent: flin("NN Extent", 12.0, 1.0, 60.0),
            nw_node_size: flin("NN Node Size", 0.5, 0.05, 4.0),
            nw_node_glow: flin("NN Node Glow", 1.2, 0.0, 6.0),
            nw_edge_thickness: flin("NN Edge Thickness", 0.14, 0.01, 1.0),
            nw_edge_bow: flin("NN Edge Bow", 0.25, 0.0, 1.5),
            nw_edge_samples: ilin("NN Samples/Edge", 16, 2, 128),
            nw_pulse_speed: flin("NN Pulse Speed", 0.5, 0.0, 6.0),
            nw_pulse_width: flin("NN Pulse Width", 0.12, 0.01, 1.0),
            // Tier 1.5 — axon-bundle edges + dendritic somas. Defaults render the
            // Tier-1 geometry (fibres 1, dendrite 0); the rest are inert until raised.
            nw_edge_fibres: ilin("NN Edge Fibres", 1, 1, 32),
            nw_bundle_radius: flin("NN Bundle Radius", 0.4, 0.02, 3.0),
            nw_edge_node_dip: flin("NN Ranvier Dip", 0.6, 0.0, 1.0),
            nw_ranvier: ilin("NN Ranvier Nodes", 5, 0, 40),
            nw_dendrite: flin("NN Dendrite", 0.0, 0.0, 3.0),
            nw_dendrite_count: ilin("NN Dendrite Count", 6, 1, 24),
            // Tier 2 signal propagation — Off by default (Tier-1 look). Sensible
            // cascade dials: fire at 0.5, ~8 units/beat conduction, 1-beat
            // refractory, gentle leak, a supra-threshold deposit, ~2 seeds/beat.
            nw_fire_mode: EnumParam::new("NN Firing", NeuralFireMode::Off),
            nw_threshold: flin("NN Threshold", 0.5, 0.05, 4.0),
            nw_conduction: flin("NN Conduction", 8.0, 0.5, 60.0),
            nw_refractory: flin("NN Refractory", 1.0, 0.0, 8.0),
            nw_decay: flin("NN Decay", 0.6, 0.0, 8.0),
            nw_deposit: flin("NN Deposit", 0.6, 0.0, 4.0),
            nw_stim_rate: flin("NN Stimulus Rate", 2.0, 0.0, 16.0),
            nw_motes: flin("NN Signal Motes", 0.0, 0.0, 3.0),
            // Tier 4 MLP: sign colour on, mild sparsify, comfortable layer spacing,
            // a static loaded input by default (drive 0).
            nw_sign_colour: flin("NN Sign Colour", 0.8, 0.0, 1.0),
            nw_sparsify: flin("NN Sparsify", 0.05, 0.0, 1.0),
            nw_layer_gap: flin("NN Layer Gap", 1.0, 0.1, 4.0),
            nw_mlp_drive: flin("NN Input Drive", 0.0, 0.0, 2.0),
            // Tier 5 Attention: layer/head 0, mild edge threshold, 24 synthesized
            // tokens, a gentle token-by-token reveal, sweep off, row layout.
            nw_attn_layer: flin("NN Attn Layer", 0.0, 0.0, 48.0),
            nw_attn_head: flin("NN Attn Head", 0.0, 0.0, 32.0),
            nw_attn_threshold: flin("NN Attn Threshold", 0.05, 0.0, 1.0),
            nw_attn_tokens: flin("NN Attn Tokens", 24.0, 2.0, 128.0),
            nw_attn_reveal: flin("NN Attn Reveal", 0.5, 0.0, 8.0),
            nw_attn_sweep: flin("NN Attn Sweep", 0.0, 0.0, 4.0),
            nw_attn_ring: flin("NN Attn Ring", 0.0, 0.0, 1.0),

            // Neural Tissue (#260 Tier 1): soma size 1.0, round (no anisotropy),
            // bouton 0.35× the edge thickness, membrane SSS/iridescence inert (0).
            nt_soma_size: flin("Tissue Soma Size", 1.0, 0.1, 4.0),
            nt_soma_shape: flin("Tissue Soma Shape", 0.0, 0.0, 1.0),
            nt_bouton_size: flin("Tissue Bouton Size", 0.35, 0.0, 2.0),
            nt_membrane_sss: flin("Tissue Membrane SSS", 0.0, 0.0, 1.0),
            nt_membrane_irid: flin("Tissue Membrane Iridescence", 0.0, 0.0, 1.0),

            // Neural Tissue morphology (#260 Tier 2): density 0 = inert (no arbor,
            // byte-identical to Tier 1); reach 1.0 soma-radii, Rall taper 0.62,
            // pyramidal by default, spines off.
            nt_dendrite_density: flin("Tissue Dendrite Density", 0.0, 0.0, 1.0),
            nt_dendrite_length: flin("Tissue Dendrite Length", 1.0, 0.2, 4.0),
            nt_dendrite_taper: flin("Tissue Dendrite Taper", 0.62, 0.3, 0.95),
            nt_neuron_type: EnumParam::new("Tissue Neuron Type", NeuronType::Pyramidal),
            nt_spines: flin("Tissue Dendritic Spines", 0.0, 0.0, 1.0),
            nt_myelin_amount: flin("Tissue Myelin", 0.0, 0.0, 1.0),
            nt_ranvier_spacing: flin("Tissue Ranvier Spacing", 0.6, 0.1, 3.0),
            nt_sheath_scale: flin("Tissue Sheath Scale", 0.5, 0.0, 2.0),
            nt_synapse_cleft: flin("Tissue Synaptic Cleft", 0.0, 0.0, 1.0),
            nt_synapse_glow: flin("Tissue Cytoplasm Glow", 0.0, 0.0, 1.0),
            nt_synapse_vesicles: flin("Tissue Vesicles", 0.0, 0.0, 1.0),
            nt_glia: flin("Tissue Glia", 0.0, 0.0, 1.0),
            nt_capillary: flin("Tissue Capillaries", 0.0, 0.0, 1.0),
            // Brain model (#275 T1): folded cerebrum by default, modest fissure, a
            // cerebellum on. Only active when NN topology = Brain model.
            br_fold_depth: flin("Brain Fold Depth", 0.12, 0.0, 1.0),
            br_fold_freq: flin("Brain Fold Freq", 5.0, 0.0, 16.0),
            br_hemi_gap: flin("Brain Fissure", 0.1, 0.0, 0.5),
            br_local_k: ilin("Brain Local k", 8, 1, 24),
            br_cerebellum: flin("Brain Cerebellum", 0.14, 0.0, 0.6),
            // T2 white matter: a proper brain gets association tracts, a corpus
            // callosum joining the hemispheres, and subcortical nuclei by default.
            br_assoc: flin("Brain Assoc Tracts", 0.25, 0.0, 1.0),
            br_callosum: flin("Brain Corpus Callosum", 0.4, 0.0, 1.0),
            br_subcortical: flin("Brain Subcortical", 0.2, 0.0, 1.0),
            // T3 parcellation: target highlight off by default (a Look, opt-in).
            br_region_hi: flin("Brain Target Highlight", 0.0, 0.0, 2.0),
            br_target: ilin("Brain Target Region", 0, 0, 7),
            // T4 stimulation: off by default (amount 0); a sensible ~2 pulses/beat rate.
            br_stim_amount: flin("Brain Stim Strength", 0.0, 0.0, 4.0),
            br_stim_rate: flin("Brain Stim Rate", 2.0, 0.0, 16.0),
            br_signal_swell: flin("Brain Signal Swell", 0.0, 0.0, 0.5),

            // Demo scene bench (#288): Cornell box by default, unit scale, hero
            // objects on, fixed reference framing (auto-orbit gated off so the
            // canonical view holds), a bright key/emitter, smooth demo metal,
            // 4 rows/side, still turntable.
            demo_scene: EnumParam::new("Demo Scene", DemoScene::CornellBox),
            demo_size: flin("Demo Scale", 1.0, 0.3, 3.0),
            demo_objects: BoolParam::new("Demo Inner Objects", true),
            demo_static_cam: BoolParam::new("Demo Fixed Camera", true),
            demo_light: flin("Demo Light", 1.0, 0.0, 8.0),
            demo_roughness: flin("Demo Roughness", 0.15, 0.0, 1.0),
            demo_count: ilin("Demo Count", 4, 1, 8),
            demo_spin: flin("Demo Spin", 0.0, 0.0, 2.0),

            // Physical thin-film (#258 T1): thickness 0 → OFF (byte-identical
            // default). Non-inert ranges for the demo: 0–1200 nm base thickness,
            // marbling 0–1, film IOR ~1.2–1.6 (soap 1.33), drainage 0–1.
            film_thickness: flin("Film Thickness (nm)", 0.0, 0.0, 1200.0),
            film_thickness_var: flin("Film Marbling", 0.3, 0.0, 1.0),
            film_ior: flin("Film IOR", 1.33, 1.0, 2.0),
            film_drainage: flin("Film Drainage", 0.5, 0.0, 1.0),

            // Synchrotron: one charge, β = 0.5, sampled on a 28² orbit-plane grid.
            sy_radius: flin("Sy Orbit Radius", 4.0, 0.5, 30.0),
            sy_beta: flin("Sy Beta (v/c)", 0.5, 0.0, 0.99),
            sy_charges: ilin("Sy Charges", 1, 1, 16),
            sy_grid: ilin("Sy Grid", 28, 4, 96),
            sy_extent: flin("Sy Plane Extent", 14.0, 2.0, 60.0),
            sy_near: flin("Sy Near-field", 1.0, 0.0, 1.0),
            sy_amp: flin("Sy Arrow Gain", 1.0, 0.0, 10.0),
            sy_thickness: flin("Sy Thickness", 0.10, 0.01, 1.0),
            sy_rmin: flin("Sy Source Clamp", 0.5, 0.05, 5.0),
            sy_perp: BoolParam::new("Sy Perp Plane", false),
            sy_view: EnumParam::new("Sy View", SyncView::Arrows),
            sy_line_seeds: ilin("Sy Line Seeds", 64, 1, 1024),
            sy_line_steps: ilin("Sy Line Steps", 220, 2, 2048),
            sy_line_ds: flin("Sy Line Step ds", 0.12, 0.01, 1.0),
            sy_line_bound: flin("Sy Line Bound", 40.0, 2.0, 200.0),
            sy_vol_layers: ilin("Sy Volume Layers", 7, 1, 64),
            sy_reveal: flin("Sy Reveal", 0.0, 0.0, 0.95),
            sy_invert: BoolParam::new("Sy Invert", false),
            sy_invert_radius: flin("Sy Invert Radius", 8.0, 1.0, 60.0),
            sy_tilt: flin("Sy Orbit Tilt", 0.0, 0.0, 90.0),
            sy_precess: flin("Sy Precession", 0.0, -1.0, 1.0),

            // Vector field (#173): the reel's parabolic swirl on a 12³ lattice,
            // gently lifted into 3-D and slowly rotating. grid_z = 1 → the flat
            // Instagram plot. (Defaults are first guesses — tune on the Mac.)
            vf_preset: EnumParam::new("VF Function", VecFieldPreset::ParabolicSwirl),
            vf_grid_x: ilin("VF Grid X", 12, 1, 32),
            vf_grid_y: ilin("VF Grid Y", 12, 1, 32),
            vf_grid_z: ilin("VF Grid Z", 12, 1, 32),
            vf_extent: flin("VF Extent", 10.0, 2.0, 60.0),
            vf_field_scale: flin("VF Field Scale", 0.5, 0.02, 4.0),
            vf_amp: flin("VF Arrow Gain", 1.0, 0.0, 10.0),
            vf_thickness: flin("VF Thickness", 0.10, 0.01, 1.0),
            vf_mag_map: EnumParam::new("VF Length Map", VecMagMap::Soft),
            vf_tint_mode: EnumParam::new("VF Tint", VecTint::Magnitude),
            vf_evolve: flin("VF Evolve", 0.3, -4.0, 4.0),
            vf_z_lift: flin("VF Z Lift", 0.6, -2.0, 2.0),
            vf_reveal: flin("VF Reveal", 0.0, 0.0, 0.95),
            // Tier 2 (#173): field lines. Defaults trace nothing until the view
            // switches off Arrows, so the Tier-1 default look is unchanged.
            vf_view: EnumParam::new("VF View", VecFieldView::Arrows),
            vf_seed_mode: EnumParam::new("VF Seeding", VecSeedMode::Lattice),
            vf_line_seeds: ilin("VF Line Seeds", 96, 4, 512),
            vf_line_steps: ilin("VF Line Steps", 160, 8, 1024),
            vf_line_ds: flin("VF Line Step ds", 0.15, 0.02, 1.0),
            vf_bidir: BoolParam::new("VF Bidirectional", true),
            vf_line_color: EnumParam::new("VF Line Colour", VecLineColor::Magnitude),
            vf_flow: flin("VF Flow Pulse", 0.0, 0.0, 1.0),
            vf_flow_speed: flin("VF Flow Speed", 1.0, -8.0, 8.0),
            vf_line_thickness: flin("VF Line Thickness", 0.06, 0.01, 1.0),

            // Builder (#173 T3): defaults reproduce the flagship parabolic
            // swirl + z-lift, so switching the bank to Custom starts at the
            // reel's field. Off terms keep gain 1 + their own axis as the
            // argument, so enabling a func immediately does something.
            vb_x1_func: EnumParam::new("VB Fx T1 Func", VecTermFunc::Square),
            vb_x1_gain: flin("VB Fx T1 Gain", 1.0, -4.0, 4.0),
            vb_x1_a: flin("VB Fx T1 Arg X", 0.0, -4.0, 4.0),
            vb_x1_b: flin("VB Fx T1 Arg Y", 1.0, -4.0, 4.0),
            vb_x1_c: flin("VB Fx T1 Arg Z", 0.0, -4.0, 4.0),
            vb_x1_phase: flin("VB Fx T1 Phase", 0.0, -6.2832, 6.2832),
            vb_x2_func: EnumParam::new("VB Fx T2 Func", VecTermFunc::Off),
            vb_x2_gain: flin("VB Fx T2 Gain", 1.0, -4.0, 4.0),
            vb_x2_a: flin("VB Fx T2 Arg X", 1.0, -4.0, 4.0),
            vb_x2_b: flin("VB Fx T2 Arg Y", 0.0, -4.0, 4.0),
            vb_x2_c: flin("VB Fx T2 Arg Z", 0.0, -4.0, 4.0),
            vb_x2_phase: flin("VB Fx T2 Phase", 0.0, -6.2832, 6.2832),
            vb_x3_func: EnumParam::new("VB Fx T3 Func", VecTermFunc::Off),
            vb_x3_gain: flin("VB Fx T3 Gain", 1.0, -4.0, 4.0),
            vb_x3_a: flin("VB Fx T3 Arg X", 1.0, -4.0, 4.0),
            vb_x3_b: flin("VB Fx T3 Arg Y", 0.0, -4.0, 4.0),
            vb_x3_c: flin("VB Fx T3 Arg Z", 0.0, -4.0, 4.0),
            vb_x3_phase: flin("VB Fx T3 Phase", 0.0, -6.2832, 6.2832),
            vb_y1_func: EnumParam::new("VB Fy T1 Func", VecTermFunc::Square),
            vb_y1_gain: flin("VB Fy T1 Gain", -1.0, -4.0, 4.0),
            vb_y1_a: flin("VB Fy T1 Arg X", 1.0, -4.0, 4.0),
            vb_y1_b: flin("VB Fy T1 Arg Y", 0.0, -4.0, 4.0),
            vb_y1_c: flin("VB Fy T1 Arg Z", 0.0, -4.0, 4.0),
            vb_y1_phase: flin("VB Fy T1 Phase", 0.0, -6.2832, 6.2832),
            vb_y2_func: EnumParam::new("VB Fy T2 Func", VecTermFunc::Off),
            vb_y2_gain: flin("VB Fy T2 Gain", 1.0, -4.0, 4.0),
            vb_y2_a: flin("VB Fy T2 Arg X", 0.0, -4.0, 4.0),
            vb_y2_b: flin("VB Fy T2 Arg Y", 1.0, -4.0, 4.0),
            vb_y2_c: flin("VB Fy T2 Arg Z", 0.0, -4.0, 4.0),
            vb_y2_phase: flin("VB Fy T2 Phase", 0.0, -6.2832, 6.2832),
            vb_y3_func: EnumParam::new("VB Fy T3 Func", VecTermFunc::Off),
            vb_y3_gain: flin("VB Fy T3 Gain", 1.0, -4.0, 4.0),
            vb_y3_a: flin("VB Fy T3 Arg X", 0.0, -4.0, 4.0),
            vb_y3_b: flin("VB Fy T3 Arg Y", 1.0, -4.0, 4.0),
            vb_y3_c: flin("VB Fy T3 Arg Z", 0.0, -4.0, 4.0),
            vb_y3_phase: flin("VB Fy T3 Phase", 0.0, -6.2832, 6.2832),
            vb_z1_func: EnumParam::new("VB Fz T1 Func", VecTermFunc::Sin),
            vb_z1_gain: flin("VB Fz T1 Gain", 0.5, -4.0, 4.0),
            vb_z1_a: flin("VB Fz T1 Arg X", 0.0, -4.0, 4.0),
            vb_z1_b: flin("VB Fz T1 Arg Y", 0.0, -4.0, 4.0),
            vb_z1_c: flin("VB Fz T1 Arg Z", 1.0, -4.0, 4.0),
            vb_z1_phase: flin("VB Fz T1 Phase", 0.0, -6.2832, 6.2832),
            vb_z2_func: EnumParam::new("VB Fz T2 Func", VecTermFunc::Off),
            vb_z2_gain: flin("VB Fz T2 Gain", 1.0, -4.0, 4.0),
            vb_z2_a: flin("VB Fz T2 Arg X", 0.0, -4.0, 4.0),
            vb_z2_b: flin("VB Fz T2 Arg Y", 0.0, -4.0, 4.0),
            vb_z2_c: flin("VB Fz T2 Arg Z", 1.0, -4.0, 4.0),
            vb_z2_phase: flin("VB Fz T2 Phase", 0.0, -6.2832, 6.2832),
            vb_z3_func: EnumParam::new("VB Fz T3 Func", VecTermFunc::Off),
            vb_z3_gain: flin("VB Fz T3 Gain", 1.0, -4.0, 4.0),
            vb_z3_a: flin("VB Fz T3 Arg X", 0.0, -4.0, 4.0),
            vb_z3_b: flin("VB Fz T3 Arg Y", 0.0, -4.0, 4.0),
            vb_z3_c: flin("VB Fz T3 Arg Z", 1.0, -4.0, 4.0),
            vb_z3_phase: flin("VB Fz T3 Phase", 0.0, -6.2832, 6.2832),
            vb_op: EnumParam::new("VB Operator", VecFieldOp::Direct),
            vb_mix: flin("VB Helmholtz Mix", 0.5, 0.0, 1.0),

            // Z0NE rails (#187 T1): defaults must be demo-ready untouched — a
            // 6-radius throat morphing every 2 bars, ticking beat ribs, colour
            // swept down the corridor. Keep in sync with `Shared::default`.
            rl_speed: flin("Rail Speed (units/beat)", 8.0, 0.5, 40.0),
            rl_bore: flin("Rail Bore", 6.0, 1.0, 30.0),
            rl_cell_len: EnumParam::new("Rail Cell Length", RailCellLen::TwoBars),
            rl_variance: flin("Rail Variance", 0.5, 0.0, 1.0),
            rl_seed: ilin("Rail Seed", 0, 0, 9999),
            rl_ring_n: ilin("Rail Ring Count", 36, 6, 128),
            rl_rows_beat: ilin("Rail Rows/Beat", 4, 1, 16),
            rl_horizon: flin("Rail Horizon (beats)", 24.0, 4.0, 64.0),
            rl_rib_gain: flin("Rail Beat Ribs", 0.6, 0.0, 2.0),
            rl_thickness: flin("Rail Thickness", 0.5, 0.05, 3.0),
            rl_lobes: ilin("Rail Max Lobes", 8, 2, 16),
            rl_spike: flin("Rail Spikiness", 0.5, 0.0, 1.0),
            rl_twist: flin("Rail Twist (turns/beat)", 0.1, -1.0, 1.0),
            rl_swell: flin("Rail Swell", 0.3, 0.0, 1.0),
            rl_fade: flin("Rail Fade-In (beats)", 6.0, 0.0, 16.0),
            rl_color_flow: flin("Rail Colour Flow", 0.05, 0.0, 0.5),
            rl_archetype: EnumParam::new("Rail Archetype", RailArchetype::Throat),
            rl_diverge: flin("Rail Divergence (°)", 137.50776, 90.0, 180.0),
            rl_shells: ilin("Rail Shells", 2, 1, 4),
            rl_parastichy: ilin("Rail Parastichy", 13, 1, 55),
            rl_change_every: EnumParam::new("Rail Change Every", RailChangeEvery::EightBars),
            rl_evolve: flin("Rail Evolve", 0.0, 0.0, 1.0),
            sc_mode: EnumParam::new("Scenery", SceneryMode::None),
            sc_surface: EnumParam::new("Scenery Surface", ScenerySurface::Cubes),
            sc_mat: EnumParam::new("Scenery Material", MaterialType::Standard),
            sc_metallic: flin("Scenery Metallic", 0.0, 0.0, 1.0),
            sc_roughness: flin("Scenery Roughness", 0.35, 0.0, 1.0),
            sc_glow: flin("Scenery Glow", 0.2, 0.0, 2.0),
            sc_emissive: flin("Scenery Emissive", 0.0, 0.0, 16.0),
            sc_opacity: flin("Scenery Opacity", 1.0, 0.0, 1.0),
            sc_ior: flin("Scenery Glass IOR", 1.45, 1.0, 2.5),
            sc_palette: EnumParam::new("Scenery Palette", Palette::Native),
            sc_sss: flin("Scenery Translucency", 0.0, 0.0, 1.0),
            sc_sss_dist: flin("Scenery SSS Distortion", 0.3, 0.0, 1.0),
            sc_sss_pow: flin("Scenery SSS Power", 4.0, 1.0, 16.0),
            sc_irid: flin("Scenery Iridescence", 0.0, 0.0, 1.0),
            sc_irid_scale: flin("Scenery Irid Scale", 2.0, 0.1, 6.0),
            sc_irid_shift: flin("Scenery Irid Hue", 0.0, 0.0, 1.0),

            // Terra (#206 Tier 2): a fjord — tall narrow steep walls, high water,
            // gentle meander. Keep in sync with Shared::default's terra block.
            terra_form: EnumParam::new("Terra Form", TerraForm::Fjord),
            terra_ridge: flin("Terra Ridge", 2.0, 0.2, 6.0),
            terra_channel: flin("Terra Channel", 1.0, 0.2, 4.0),
            terra_width: flin("Terra Valley Width", 3.0, 1.2, 8.0),
            terra_steep: flin("Terra Steepness", 0.7, 0.0, 1.0),
            terra_terrace: flin("Terra Terracing", 0.0, 0.0, 1.0),
            terra_rough: flin("Terra Roughness", 0.4, 0.0, 2.0),
            terra_meander: flin("Terra Meander", 0.6, 0.0, 3.0),
            terra_water_level: flin("Terra Water Level", -1.0, -6.0, 2.0),
            terra_water_on: BoolParam::new("Terra Water", true),
            terra_clearance: flin("Terra Clearance", 1.5, 0.2, 5.0),
            terra_noise_freq: flin("Terra Detail Freq", 0.15, 0.01, 1.0),
            // Terra water (#206 Tier 3): calm see-through glass water.
            wt_mat: EnumParam::new("Water Material", MaterialType::Glass),
            wt_roughness: flin("Water Roughness", 0.06, 0.0, 1.0),
            wt_ior: flin("Water IOR", 1.33, 1.0, 2.5),
            wt_opacity: flin("Water Opacity", 0.7, 0.0, 1.0),
            wt_glow: flin("Water Glow", 0.0, 0.0, 2.0),
            wt_ripple: flin("Water Ripple", 0.15, 0.0, 1.0),
            wt_ripple_freq: flin("Water Ripple Freq", 0.6, 0.05, 3.0),
            wt_absorb: flin("Water Absorption", 1.4, 0.0, 4.0),
            wt_glitter: flin("Water Glitter", 0.6, 0.0, 3.0),
            wt_reflect: flin("Water Reflectivity", 0.15, 0.0, 1.0),

            // Phyllotaxis: a sunflower disk, 21 parastichy spirals, golden angle.
            phyl_surface: EnumParam::new("Phyl Surface", PhylSurface::Disk),
            phyl_count: ilin("Phyl Count", 1500, 10, 20000),
            phyl_divergence: flin("Phyl Divergence (°)", 137.50776, 90.0, 180.0),
            phyl_radius: flin("Phyl Radius", 1.0, 0.05, 10.0),
            phyl_parastichy: ilin("Phyl Parastichy", 21, 1, 89),
            phyl_height: flin("Phyl Height", 8.0, 0.0, 40.0),
            phyl_growth: flin("Phyl Shell Growth", 2.0, 0.0, 6.0),
            phyl_breathe_amp: flin("Phyl Breathe Amp", 0.0, 0.0, 0.5),
            phyl_breathe_freq: flin("Phyl Breathe Freq", 0.5, 0.0, 4.0),
            phyl_rot: flin("Phyl Rotation", 0.3, -8.0, 8.0),
            phyl_thickness: flin("Phyl Thickness", 0.1, 0.01, 1.0),

            // Tessellation: a Penrose P3 tiling at depth 4 (~470 tiles), filling a
            // ~radius-8 plane with slim glowing edge rods.
            tess_family: EnumParam::new("Tiling Family", TilingFamily::PenroseP3),
            tess_depth: ilin("Tess Depth", 4, 0, 8),
            tess_scale: flin("Tess Scale", 8.0, 0.5, 40.0),
            tess_thickness: flin("Tess Thickness", 0.06, 0.01, 1.0),
            // Phase 2 default = extruded prisms (the headline "cityscape"); a
            // moderate uniform height looks good against the depth-4 tile size.
            tess_view: EnumParam::new("Tess View", TessView::Extruded),
            tess_height: flin("Tess Height", 0.25, 0.0, 1.0),
            tess_height_mode: EnumParam::new("Tess Height Mode", TessHeightMode::Uniform),
            // Phase 3 beat motion off by default (amounts 0) → look unchanged.
            tess_beat_infl: flin("Tess Beat Inflate", 0.0, 0.0, 1.0),
            tess_ripple_amt: flin("Tess Beat Ripple", 0.0, 0.0, 1.0),
            tess_ripple_freq: flin("Tess Ripple Freq", 2.0, 0.0, 12.0),
            // Phase 4: default to inflation (Penrose, unchanged); cut-and-project +
            // phason are opt-in. Grid range 6 ≈ a few hundred tiles.
            tess_construct: EnumParam::new("Tess Construct", TilingConstruct::Inflation),
            tess_phason: flin("Tess Phason", 0.0, 0.0, 1.0),
            tess_grid_n: ilin("Tess Grid Range", 6, 1, 24),
            tess_ammann: flin("Tess Ammann Bars", 0.0, 0.0, 1.0),
            tess_hyp_p: ilin("Tess Hyperbolic p", 7, 3, 12),
            tess_hyp_q: ilin("Tess Hyperbolic q", 3, 3, 12),

            // Mandelbulb: the classic power-8 set at cube-field size, gently
            // spinning + morphing off the global Speed; orbit-trap colour on.
            mb_power: flin("MB Power", 8.0, 2.0, 16.0),
            mb_iter: ilin("MB Iterations", 8, 1, 24),
            mb_scale: flin("MB Scale", 150.0, 10.0, 600.0),
            mb_detail: ilin("MB Detail (steps)", 96, 24, 400),
            mb_spin: flin("MB Spin", 1.0, -4.0, 4.0),
            mb_morph: flin("MB Morph", 0.0, -2.0, 2.0),
            mb_color: flin("MB Colour", 1.0, 0.0, 1.0),
            mb_bailout: flin("MB Bailout", 2.0, 1.5, 8.0),

            // Creature Engine (#476 Tier 1): the bell jelly at cube-field size, a
            // gentle beat-paced swim, a soft rim glow + bright organs.
            cr_form: ilin("Creature Form", 0, 0, 2),
            cr_scale: flin("Creature Scale", 120.0, 10.0, 600.0),
            cr_detail: ilin("Creature Detail (steps)", 128, 24, 400),
            cr_swim: flin("Creature Swim", 1.0, 0.0, 8.0),
            cr_warp_amp: flin("Creature Swim Amp", 0.06, 0.0, 0.4),
            cr_warp_freq: flin("Creature Swim Freq", 4.0, 0.0, 16.0),
            cr_rim: flin("Creature Rim Glow", 0.6, 0.0, 3.0),
            cr_glow: flin("Creature Bioluminescence", 1.0, 0.0, 4.0),
            // Tier 2a metachronal wave: 1 pulse/beat, 3 bands, sharpness 2.5,
            // amount 0 (off → the Tier-1 steady glow).
            cr_wave_speed: flin("Creature Band Speed", 1.0, 0.0, 8.0),
            cr_wave_freq: flin("Creature Band Count", 3.0, 0.0, 16.0),
            cr_wave_sharp: flin("Creature Band Sharpness", 2.5, 1.0, 12.0),
            cr_wave_amt: flin("Creature Band Amount", 0.0, 0.0, 4.0),
            // Tier 2c anatomy overlay: off by default (byte-identical).
            cr_overlay: BoolParam::new("Creature Anatomy Overlay", false),
            cr_overlay_opacity: flin("Creature Overlay Opacity", 1.0, 0.0, 1.0),
            cr_overlay_bright: flin("Creature Overlay Brightness", 1.0, 0.0, 4.0),

            // Minimal surfaces (#127): a balanced gyroid at cube-field size, six
            // channels across, a thin soap-film wall, mild channel colour.
            ms_family: EnumParam::new("MS Family", MinimalFamily::Gyroid),
            ms_scale: flin("MS Scale", 150.0, 10.0, 600.0),
            ms_cells: flin("MS Cells", 6.0, 0.5, 24.0),
            ms_iso: flin("MS Isolevel", 0.0, -1.2, 1.2),
            ms_thickness: flin("MS Thickness", 0.06, 0.0, 0.6),
            ms_twist: flin("MS Twist", 0.0, -4.0, 4.0),
            ms_detail: ilin("MS Detail (steps)", 160, 24, 768),
            ms_color: flin("MS Colour", 1.0, 0.0, 1.0),
            ms_beat_iso: flin("MS Beat Isolevel", 0.0, 0.0, 1.0),
            ms_bend: flin("MS Bend Speed", 0.0, 0.0, 2.0),
            ms_uv_res: ilin("MS UV Resolution", 96, 16, 256),
            ms_extent: flin("MS Extent", 2.0, 0.5, 4.0),
            ms_bend_phase: flin("MS Bend Phase", 0.0, 0.0, 1.0),
            ms_turns: flin("MS Turns", 1.0, 0.25, 6.0),
            ms_form_res: flin("MS Form Resolution", 0.5, 0.25, 1.0),

            // Lens (#258 T3): a biconvex lens at cube-field size — moderate curvature,
            // 0.6-aperture, a chunky centre, 128 sphere-trace steps.
            lens_focal: flin("Lens Focal / Curvature", 1.0, 0.2, 4.0),
            lens_aperture: flin("Lens Aperture", 0.6, 0.05, 1.2),
            lens_thickness: flin("Lens Thickness", 0.25, 0.02, 0.9),
            lens_plano: BoolParam::new("Lens Plano-Convex", false),
            lens_scale: flin("Lens Scale", 150.0, 10.0, 600.0),
            lens_detail: ilin("Lens Detail (steps)", 128, 16, 256),

            kf_space: EnumParam::new("KF Space", KifsSpace::Euclidean),
            kf_sectors: flin("KF Sectors", 12.0, 2.0, 24.0),
            kf_fold: flin("KF Fold", 0.65, 0.2, 1.2),
            kf_iter: ilin("KF Iterations", 6, 1, 10),
            kf_iter_rot: flin("KF Iter Rotation", 0.0, -1.0, 1.0),
            kf_spin: flin("KF Spin", 1.0, -4.0, 4.0),
            kf_breathe: flin("KF Breathe", 0.25, 0.0, 1.0),
            kf_zoom: flin("KF Zoom", 1.2, 0.4, 3.0),
            kf_tunnel: BoolParam::new("KF Tunnel", false),
            kf_rays: ilin("KF Rays", 18, 0, 36),
            kf_ring: flin("KF Ring", 1.0, 0.0, 2.0),
            kf_glow: flin("KF Glow", 1.0, 0.0, 3.0),
            kf_hue: flin("KF Hue", 0.0, 0.0, 1.0),
            kf_pattern: EnumParam::new("KF Pattern", KifsPattern::Inversion),
            kf_palette: EnumParam::new("KF Palette", KifsPalette::Spectral),
            kf_color_speed: flin("KF Colour Speed", 0.08, -1.0, 1.0),
            kf_churn: flin("KF Churn", 1.0, -2.0, 4.0),
            kf_e8_flow: flin("KF E8 8-D Rotation", 0.0, 0.0, 2.0),
            kf_warp: flin("KF Warp", 0.0, 0.0, 1.0),
            kf_flow: flin("KF Tunnel Flow", 0.004, -0.01, 0.01),
            kf_petals: ilin("KF Petals", 6, 2, 16),
            kf_contrast: flin("KF Contrast", 1.0, 0.3, 3.0),
            kf_sharp: flin("KF Sharpness", 0.5, 0.0, 1.0),
            kf_invert: flin("KF Invert", 0.0, 0.0, 1.0),
            kf_dispersion: flin("KF Dispersion", 0.0, 0.0, 1.0),
            kf_view: EnumParam::new("KF 3D Mode", KifsView::Field),
            kf_relief: flin("KF Relief Height", 0.5, 0.0, 1.0),
            kf_relief_elev: flin("KF 3D Elevation", 1.25, 0.05, 1.4),
            kf_relief_steps: ilin("KF 3D Steps", 96, 32, 256),
            kf_relief_shine: flin("KF 3D Shine", 0.5, 0.0, 2.0),

            // Scene Kaleidoscope (#361 Tier 1). Off by default; sane look defaults.
            kal_on: BoolParam::new("Kaleidoscope", false),
            kal_sectors: flin("Kaleido Sectors", 6.0, 1.0, 48.0),
            kal_mode: EnumParam::new("Kaleido Mode", KaleidoMode::FullFrame),
            kal_spin: flin("Kaleido Spin", 0.1, -2.0, 2.0),
            kal_roll: flin("Kaleido Roll", 0.0, 0.0, 1.0),
            kal_zoom: flin("Kaleido Zoom", 1.0, 0.2, 4.0),
            kal_center_x: flin("Kaleido Center X", 0.0, -1.0, 1.0),
            kal_center_y: flin("Kaleido Center Y", 0.0, -1.0, 1.0),
            kal_mix: flin("Kaleido Mix", 1.0, 0.0, 1.0),
            kal_twist: flin("Kaleido Twist", 0.0, -2.0, 2.0),
            kal_tint_hue: flin("Kaleido Tint Hue", 0.0, 0.0, 360.0),
            kal_tint_amt: flin("Kaleido Tint Amount", 0.0, 0.0, 1.0),
            kal_seam: flin("Kaleido Seam Soften", 0.5, 0.0, 1.0),

            // #391 Tier 1 instrumentation. HUD off (byte-identical) but a sensible
            // measurement rig so toggling the HUD on reads immediately. Match
            // ipc::Shared::default().instrument exactly (the Default→Shared golden).
            instr_hud: BoolParam::new("Instrument HUD", false),
            instr_probe_on: BoolParam::new("Field Probe", true),
            instr_probe_x: flin("Probe X", 2.0, -20.0, 20.0),
            instr_probe_y: flin("Probe Y", 0.0, -20.0, 20.0),
            instr_probe_z: flin("Probe Z", 0.0, -20.0, 20.0),
            instr_ledger_on: BoolParam::new("Energy Ledger", true),
            instr_ledger_half: flin("Ledger Half-Extent", 4.0, 0.5, 20.0),
            instr_ledger_res: flin("Ledger Samples", 12.0, 2.0, 48.0),
            instr_flux_on: BoolParam::new("Poynting Flux", true),
            instr_flux_x: flin("Flux X", 2.0, -20.0, 20.0),
            instr_flux_y: flin("Flux Y", 0.0, -20.0, 20.0),
            instr_flux_z: flin("Flux Z", 0.0, -20.0, 20.0),
            instr_flux_size: flin("Flux Patch Size", 2.0, 0.25, 20.0),
            instr_flux_axis: EnumParam::new("Flux Axis", FluxAxis::X),
            instr_flux_res: flin("Flux Samples", 16.0, 2.0, 64.0),
            instr_csv_log: BoolParam::new("Probe CSV Log", false),
            // HUD panel presentation. Match ipc::Shared::default().instrument2 exactly.
            instr_panel_opacity: flin("HUD Panel Opacity", 0.55, 0.0, 1.0),
            instr_panel_bevel: flin("HUD Panel Bevel", 0.35, 0.0, 1.0),
            instr_hud_scale: flin("HUD Size", 1.0, 0.4, 3.0),
            instr_hud_dock: EnumParam::new("HUD Dock", HudDock::TopLeft),

            surface_mode: EnumParam::new("Surface Mode", SurfaceMode::Original),
            origin_mode: EnumParam::new("Origin Mode", OriginMode::Corner),
            // 0 = sharp cube (today's look); 0.5 = wide rounded cube; 1 = sphere.
            bevel: flin("Bevel", 0.0, 0.0, 1.0),

            // #472 Tier 1 materials: off by default (byte-identical). Defaults must
            // match ipc::Shared::default().material = [0,0,1,0,0,0,0,0] (indices 3–6
            // are reserved now that the maps feed the unified pipeline directly).
            mat_enable: BoolParam::new("Material Maps", false),
            mat_projection: EnumParam::new("Material Projection", MatProjection::Triplanar),
            mat_scale: flin("Material Scale", 1.0, 0.02, 16.0),

            // #472 Tier 2 procedural layer: OFF by default (byte-identical). Defaults
            // must match ipc::Shared::default().material_layer / material_grad.
            mp_enable: BoolParam::new("Procedural Material", false),
            mp_noise: EnumParam::new("Noise", MatNoise::Fbm),
            mp_channel: EnumParam::new("Bake Channel", MatChannel::Albedo),
            mp_scale: flin("Noise Scale", 4.0, 0.25, 64.0),
            mp_rotation: flin("Noise Rotation", 0.0, 0.0, std::f32::consts::TAU),
            mp_offset_x: flin("Noise Offset X", 0.0, -8.0, 8.0),
            mp_offset_y: flin("Noise Offset Y", 0.0, -8.0, 8.0),
            mp_octaves: IntParam::new("Octaves", 5, IntRange::Linear { min: 1, max: 8 }),
            mp_lacunarity: flin("Lacunarity", 2.0, 1.2, 4.0),
            mp_gain: flin("Gain", 0.5, 0.1, 0.9),
            mp_warp: flin("Domain Warp", 0.0, 0.0, 2.0),
            mp_contrast: flin("Contrast", 1.0, 0.1, 4.0),
            mp_gamma: flin("Gamma", 1.0, 0.2, 4.0),
            mp_remap_lo: flin("Remap Low", 0.0, 0.0, 1.0),
            mp_remap_hi: flin("Remap High", 1.0, 0.0, 1.0),
            mp_invert: BoolParam::new("Invert", false),
            mp_seed: IntParam::new("Noise Seed", 0, IntRange::Linear { min: 0, max: 64 }),
            mp_res: EnumParam::new("Bake Resolution", BakeRes::R512),
            mp_lo_r: flin("Gradient Low R", 0.04, 0.0, 1.0),
            mp_lo_g: flin("Gradient Low G", 0.04, 0.0, 1.0),
            mp_lo_b: flin("Gradient Low B", 0.05, 0.0, 1.0),
            mp_hi_r: flin("Gradient High R", 0.80, 0.0, 1.0),
            mp_hi_g: flin("Gradient High G", 0.76, 0.0, 1.0),
            mp_hi_b: flin("Gradient High B", 0.70, 0.0, 1.0),

            // #472 Tier 3 overlay layer 2: disabled by default (must match
            // ipc::Shared::default().material_layer2 / material_grad2).
            mp2_enable: BoolParam::new("Layer 2", false),
            mp2_blend: EnumParam::new("Layer 2 Blend", BlendMode::Normal),
            mp2_noise: EnumParam::new("Layer 2 Noise", MatNoise::Fbm),
            mp2_channel: EnumParam::new("Layer 2 Channel", MatChannel::Roughness),
            mp2_scale: flin("Layer 2 Scale", 6.0, 0.25, 64.0),
            mp2_rotation: flin("Layer 2 Rotation", 0.0, 0.0, std::f32::consts::TAU),
            mp2_offset_x: flin("Layer 2 Offset X", 0.0, -8.0, 8.0),
            mp2_offset_y: flin("Layer 2 Offset Y", 0.0, -8.0, 8.0),
            mp2_octaves: IntParam::new("Layer 2 Octaves", 4, IntRange::Linear { min: 1, max: 8 }),
            mp2_lacunarity: flin("Layer 2 Lacunarity", 2.0, 1.2, 4.0),
            mp2_gain: flin("Layer 2 Gain", 0.5, 0.1, 0.9),
            mp2_warp: flin("Layer 2 Warp", 0.0, 0.0, 2.0),
            mp2_contrast: flin("Layer 2 Contrast", 1.0, 0.1, 4.0),
            mp2_gamma: flin("Layer 2 Gamma", 1.0, 0.2, 4.0),
            mp2_remap_lo: flin("Layer 2 Remap Low", 0.0, 0.0, 1.0),
            mp2_remap_hi: flin("Layer 2 Remap High", 1.0, 0.0, 1.0),
            mp2_invert: BoolParam::new("Layer 2 Invert", false),
            mp2_seed: IntParam::new("Layer 2 Seed", 1, IntRange::Linear { min: 0, max: 64 }),
            mp2_lo_r: flin("Layer 2 Grad Low R", 0.04, 0.0, 1.0),
            mp2_lo_g: flin("Layer 2 Grad Low G", 0.04, 0.0, 1.0),
            mp2_lo_b: flin("Layer 2 Grad Low B", 0.05, 0.0, 1.0),
            mp2_hi_r: flin("Layer 2 Grad High R", 0.80, 0.0, 1.0),
            mp2_hi_g: flin("Layer 2 Grad High G", 0.76, 0.0, 1.0),
            mp2_hi_b: flin("Layer 2 Grad High B", 0.70, 0.0, 1.0),

            // #472 Tier 3 derived maps: off by default (match ipc::Shared::default()).
            mat_derive_normal: BoolParam::new("Derive Normal", false),
            mat_derive_ao: BoolParam::new("Derive AO", false),
            mat_normal_source_albedo: BoolParam::new("Normal from Albedo", false),
            mat_derive_normal_strength: flin("Derived Normal Strength", 1.0, 0.0, 4.0),
            mat_derive_ao_strength: flin("Derived AO Strength", 1.0, 0.0, 1.0),
            mat_derive_ao_radius: flin("Derived AO Radius", 2.0, 1.0, 8.0),

            // #472 Tier 5 live: animation off + no displacement (match
            // ipc::Shared::default().material_live = [0, 0.1, 0, 1, 0, 0, 0, 0]).
            mat_anim_enable: BoolParam::new("Animate Material", false),
            mat_anim_speed: flin("Animation Speed", 0.1, 0.0, 4.0),
            mat_anim_mode: EnumParam::new("Animation Mode", AnimMode::Drift),
            mat_flow_x: flin("Flow X", 1.0, -1.0, 1.0),
            mat_flow_y: flin("Flow Y", 0.0, -1.0, 1.0),
            mat_displace: flin("Height Displace", 0.0, 0.0, 2.0),

            // Plexus defaults: radius 1.6× spacing, up to 8 links, thin struts, small
            // node markers. Match ipc::Shared::default().plexus (byte-identical).
            plexus_radius: flin("Plexus Link Radius", 1.6, 0.3, 20.0),
            plexus_links: flin("Plexus Max Links", 8.0, 1.0, 16.0),
            plexus_strut: flin("Plexus Strut", 0.07, 0.01, 0.5),
            plexus_marker: flin("Plexus Node Size", 0.24, 0.0, 1.0),

            // Tier 2 impostors (off by default; match ipc::Shared::default()).
            plexus_impostor: BoolParam::new("Plexus Impostors", false),
            plexus_edges: BoolParam::new("Plexus Edges", true),
            plexus_node_radius: flin("Plexus Node Radius", 0.35, 0.02, 1.5),
            plexus_edge_radius: flin("Plexus Edge Radius", 0.09, 0.01, 0.8),
            plexus_node_type: EnumParam::new("Plexus Node Material", MaterialType::Standard),
            plexus_node_metallic: flin("Plexus Node Metallic", 0.1, 0.0, 1.0),
            plexus_node_rough: flin("Plexus Node Roughness", 0.4, 0.0, 1.0),
            plexus_node_ior: flin("Plexus Node IOR", 1.45, 1.0, 2.5),
            plexus_node_hue: flin("Plexus Node Hue", 0.0, 0.0, 1.0),
            plexus_node_sat: flin("Plexus Node Saturation", 0.0, 0.0, 1.0),
            plexus_node_val: flin("Plexus Node Value", 1.0, 0.0, 2.0),
            plexus_node_emissive: flin("Plexus Node Emissive", 0.6, 0.0, 16.0),
            plexus_edge_type: EnumParam::new("Plexus Edge Material", MaterialType::Standard),
            plexus_edge_metallic: flin("Plexus Edge Metallic", 0.0, 0.0, 1.0),
            plexus_edge_rough: flin("Plexus Edge Roughness", 0.6, 0.0, 1.0),
            plexus_edge_ior: flin("Plexus Edge IOR", 1.45, 1.0, 2.5),
            plexus_edge_hue: flin("Plexus Edge Hue", 0.58, 0.0, 1.0),
            plexus_edge_sat: flin("Plexus Edge Saturation", 0.4, 0.0, 1.0),
            plexus_edge_val: flin("Plexus Edge Value", 1.0, 0.0, 2.0),
            plexus_edge_emissive: flin("Plexus Edge Emissive", 0.3, 0.0, 16.0),

            // Tier 3 signal propagation (off by default).
            plexus_signal: BoolParam::new("Plexus Signal", false),
            plexus_signal_speed: flin("Plexus Signal Speed", 1.0, 0.0, 8.0),
            plexus_signal_gain: flin("Plexus Signal Gain", 1.5, 0.0, 8.0),
            plexus_signal_width: flin("Plexus Signal Width", 0.18, 0.02, 1.0),

            // Shape morph (default 1 = sphere nodes / circular struts; 0 recovers
            // the old sharp cube / square-strut look).
            plexus_node_shape: flin("Plexus Node Shape", 1.0, 0.0, 1.0),
            plexus_edge_shape: flin("Plexus Edge Shape", 1.0, 0.0, 1.0),
            // Plexus overlay defaults: off; shell grows 1.15×, 2 nodes per
            // directional cell over a 12×12 grid. Match ipc::Shared::default().plexus_overlay.
            plexus_overlay_on: BoolParam::new("Plexus Overlay", false),
            plexus_shell_scale: flin("Plexus Shell Scale", 1.15, 1.0, 2.5),
            plexus_shell_depth: flin("Plexus Shell Depth", 0.2, 0.02, 1.0),
            plexus_shell_bins: flin("Plexus Shell Resolution", 12.0, 4.0, 32.0),

            animate: BoolParam::new("Animate", true),
            // Speed dial 0..1; the decade is set by `speed_exp`. Default 1.0 × 10⁻²
            // = 0.01 effective (matches the old default), so the dial spans 0..0.01.
            inc_scale: flin("Speed (global)", 1.0, 0.0, 1.0),
            speed_exp: ilin("Speed Power (10ⁿ)", -2, -6, 0),

            ambient: flin("Ambient (IBL)", 1.0, 0.0, 3.0),
            key_intensity: flin("Key Light", 2.2, 0.0, 6.0),
            fill_intensity: flin("Fill Light", 0.6, 0.0, 3.0),
            elevation: flin("Key Elevation", 35.0, -90.0, 90.0),
            azimuth: flin("Key Azimuth", 40.0, -180.0, 180.0),
            glow: flin("Glow", 0.2, 0.0, 2.0),
            mat_emissive: flin("Emissive", 0.0, 0.0, 16.0),
            opacity: flin("Opacity", 1.0, 0.0, 1.0),

            pulse: BoolParam::new("Pulse To Tempo", false),
            tempo_sync: BoolParam::new("Tempo Sync (Host)", true),
            tempo: flin("Tempo (BPM)", 120.0, 40.0, 240.0),
            // Default Beat = the original behaviour; switch to Audio to react.
            pulse_source: EnumParam::new("Pulse Source", PulseSource::Beat),

            // Audio-reactive — off by default (no analysis, no behaviour change).
            audio_react: BoolParam::new("Audio Reactive", false),
            audio_gain: flin("Audio Gain", 1.0, 0.0, 20.0),
            audio_attack: flin("Audio Attack (ms)", 10.0, 1.0, 200.0),
            audio_release: flin("Audio Release (ms)", 200.0, 20.0, 2000.0),

            // #333: calibrated metering — 1/3-octave, Z-weight, fast, HUD off.
            meter_res: EnumParam::new("Meter RTA Resolution", SpectrumMode::Oct3),
            meter_weight: EnumParam::new("Meter Weighting", MeterWeighting::Z),
            meter_averaging: EnumParam::new("Meter Averaging", MeterAveraging::Fast),
            meter_hud: BoolParam::new("Meter HUD (visual)", false),

            // #333 Tier 3: Expressive by default → byte-identical; streaming targets.
            analytical_mode: EnumParam::new("Duo-Field Drive", AnalyticalMode::Expressive),
            an_target_lufs: flin("Loudness Target (LUFS)", -14.0, -40.0, 0.0),
            an_floor_lufs: flin("Drive Floor (LUFS)", -50.0, -70.0, -10.0),
            an_tp_ceiling: flin("True-Peak Ceiling (dBTP)", -1.0, -6.0, 0.0),
            an_corr_alarm: flin("Correlation Alarm", 0.0, -1.0, 1.0),
            an_reference_hud: BoolParam::new("Instrument HUD (visual)", false),

            // #348 Field Volume: Legacy source → today's Volume byte-identical.
            fv_source: EnumParam::new("Field Volume Source", FieldVolSource::Legacy),
            fv_smooth: flin("Volume Smoothing", 1.0, 0.25, 4.0),
            fv_exposure_db: flin("Volume Exposure (dB)", 0.0, -24.0, 24.0),
            fv_calibrate: BoolParam::new("Volume Calibrated Brightness", false),
            fv_gain: flin("Volume Gain", 1.0, 0.0, 4.0),
            fv_lines: BoolParam::new("Field Lines (flow)", false),
            fv_line_density: flin("Line Density", 160.0, 40.0, 4000.0),
            fv_line_thickness: flin("Line Thickness", 0.09, 0.01, 0.5),

            // #349 Calibrated colour: Aesthetic → today's tint byte-identical.
            col_mode: EnumParam::new("Colour Mode", ColourMode::Aesthetic),
            col_lo_db: flin("Colour Low (dB)", -60.0, -120.0, 0.0),
            col_hi_db: flin("Colour High (dB)", 0.0, -60.0, 12.0),
            col_lut: EnumParam::new("Colour LUT", CalLut::Turbo),
            col_source: EnumParam::new("Colour Source", CalColourSource::Auto),
            col_amount: flin("Colour Amount", 1.0, 0.0, 1.0),

            cam_path: EnumParam::new("Camera Path", CamPath::Off),
            // Flow speed (cycles/beat): the master orbit rate in BOTH plain orbit-cam
            // and sequencer modes. Widened from the old 0..0.05 (which capped at the
            // default, so the slider could only slow down) to a usable 0..1 range.
            // 0.05 keeps the original gentle drift; 0.25 ≈ one orbit per bar; 1.0 fast.
            cam_speed: flin("Camera Flow Speed", 0.05, 0.0, 1.0),
            cam_kick: flin("Camera Kick", 0.08, 0.0, 1.0),
            cam_damping: flin("Camera Damping", 0.4, 0.01, 0.99),
            cam_amount: flin("Camera Motion Amount", 1.0, 0.0, 1.0),
            // Momentum ON by default = today's lurch-on-the-beat is preserved.
            cam_beat_momentum: BoolParam::new("Camera Beat Momentum", true),

            // Sequencer OFF by default → the single `cam_path` behaves exactly as today.
            cam_seq_enabled: BoolParam::new("Camera Sequencer", false),
            cam_bars_per_shot: EnumParam::new("Bars Per Shot", BarPeriod::B8),
            cam_seq_order: EnumParam::new("Shot Order", CamOrder::Series),
            cam_transition: EnumParam::new("Shot Transition", CamTransition::Glide),
            cam_transition_bars: flin("Glide Bars", 1.0, 0.0, 4.0),

            // Dolly depth 0 by default → inert (no in/out breath), today's look unchanged.
            cam_dolly_period: flin("Dolly Period (bars)", 4.0, 0.25, 32.0),
            cam_dolly_depth: flin("Dolly Depth", 0.0, 0.0, 0.9),
            cam_dolly_wave: EnumParam::new("Dolly Wave", DollyWave::Sine),

            // Host by default → today's PLL-locked behaviour unchanged.
            tempo_source: EnumParam::new("Tempo Source", TempoSource::Host),
            beats_per_bar: ilin("Beats Per Bar", 4, 1, 16),
            scene_preset_timing: EnumParam::new("Scene Preset Timing", PresetDivision::Instant),
            component_preset_timing: EnumParam::new(
                "Component Preset Timing",
                PresetDivision::Instant,
            ),
            perf_enable: BoolParam::new("Performance Controller", false),

            // Tier 2 framing — inert by default (roll 0, FOV 45, no dolly-zoom).
            cam_roll: flin("Camera Roll (deg)", 0.0, -45.0, 45.0),
            cam_fov: flin("Field of View (deg)", 45.0, 20.0, 90.0),
            cam_fov_dolly: flin("Dolly Zoom", 0.0, 0.0, 1.0),
            cam_hold_prob: flin("Shot Hold Chance", 0.0, 0.0, 1.0),
            cam_phrase_lock: BoolParam::new("Phrase-Locked Facing", false),
            // 1.0 = fully sequencer (the original Tier-2 behaviour when the
            // sequencer is on); dial down to blend the base orbit-cam back in.
            cam_seq_mix: flin("Sequencer Blend", 1.0, 0.0, 1.0),

            // Storyboard — off by default; a sensible 4-shot demo playlist when enabled.
            cam_story_enabled: BoolParam::new("Camera Storyboard", false),
            cam_story_count: ilin("Storyboard Shots", 4, 1, 4),
            cam_story_mode: EnumParam::new("Storyboard Order", CamOrder::Series),
            cam_story_seed: ilin("Storyboard Seed", 1, 0, 9999),
            cam_shot0_path: EnumParam::new("Shot 1 Move", CamPath::HCircle),
            cam_shot0_bars: EnumParam::new("Shot 1 Bars", BarPeriod::B8),
            cam_shot0_radius: flin("Shot 1 Radius", 1.0, 0.3, 3.0),
            cam_shot1_path: EnumParam::new("Shot 2 Move", CamPath::Spiral),
            cam_shot1_bars: EnumParam::new("Shot 2 Bars", BarPeriod::B8),
            cam_shot1_radius: flin("Shot 2 Radius", 1.3, 0.3, 3.0),
            cam_shot2_path: EnumParam::new("Shot 3 Move", CamPath::Figure8),
            cam_shot2_bars: EnumParam::new("Shot 3 Bars", BarPeriod::B4),
            cam_shot2_radius: flin("Shot 3 Radius", 0.8, 0.3, 3.0),
            cam_shot3_path: EnumParam::new("Shot 4 Move", CamPath::Boom),
            cam_shot3_bars: EnumParam::new("Shot 4 Bars", BarPeriod::B8),
            cam_shot3_radius: flin("Shot 4 Radius", 1.5, 0.3, 3.0),

            mod_a_target: EnumParam::new("Mod A Target", ModTarget::None),
            mod_a_depth: flin("Mod A Depth", 0.0, -1.0, 1.0),
            mod_b_target: EnumParam::new("Mod B Target", ModTarget::None),
            mod_b_depth: flin("Mod B Depth", 0.0, -1.0, 1.0),

            // Speed Pulse — inert by default (amount 0); dial up for the log bounce.
            speed_pulse_amount: flin("Speed Pulse Amount (10ⁿ)", 0.0, 0.0, 3.0),
            speed_pulse_attack: flin("Speed Pulse Attack (ms)", 5.0, 1.0, 200.0),
            speed_pulse_decay: flin("Speed Pulse Decay (ms)", 350.0, 20.0, 3000.0),

            // Breath — inert by default (amount 0); snappy attack, slow release.
            breath_amount: flin("Breath Amount", 0.0, 0.0, 3.0),
            breath_attack: flin("Breath Attack (ms)", 8.0, 1.0, 200.0),
            breath_decay: flin("Breath Decay (ms)", 400.0, 20.0, 3000.0),

            mat_type: EnumParam::new("Material Type", MaterialType::Standard),
            ior: flin("Glass IOR", 1.45, 1.0, 2.5),
            mat_absorb: flin("Absorption", 1.0, 0.0, 8.0),
            refr_overlay: BoolParam::new("Refraction Overlay", false),
            refr_blend: flin("Refraction Blend", 1.0, 0.0, 1.0),
            // Screen-space refraction off by default (strength 0); a modest default
            // displacement so raising strength reads immediately.
            refract_ss: flin("Screen Refraction", 0.0, 0.0, 1.0),
            refract_dist: flin("Refraction Displace", 0.5, 0.0, 3.0),
            anisotropy: flin("Anisotropy", 0.0, -1.0, 1.0),
            aniso_rotation: flin("Anisotropy Rotation", 0.0, 0.0, 360.0),
            aniso_overlay: BoolParam::new("Anisotropy Overlay", false),
            aniso_blend: flin("Anisotropy Blend", 1.0, 0.0, 1.0),
            // Surface lobes default to their natural full strength but stay inert
            // until the pure type or the overlay is selected (like the aniso/refr
            // overlay-blend convention) — so Standard stays byte-identical.
            clearcoat: flin("Clearcoat", 1.0, 0.0, 1.0),
            clearcoat_rough: flin("Clearcoat Roughness", 0.1, 0.0, 1.0),
            clearcoat_overlay: BoolParam::new("Clearcoat Overlay", false),
            sheen: flin("Sheen", 1.0, 0.0, 1.0),
            sheen_rough: flin("Sheen Roughness", 0.3, 0.0, 1.0),
            sheen_tint: flin("Sheen Tint", 0.0, 0.0, 1.0),
            sheen_overlay: BoolParam::new("Sheen Overlay", false),
            // Body optics default inert (thickness 0 = today's translucency, interior
            // scatter 0 = clear glass) — Standard/Glass stay byte-identical.
            sss_thickness: flin("Translucency Thickness", 0.0, 0.0, 1.0),
            sss_radius: flin("Translucency Radius", 1.0, 0.05, 8.0),
            interior_scatter: flin("Interior Scatter", 0.0, 0.0, 1.0),
            // Microstructure (#214 T4) — all amounts default 0 (byte-identical);
            // density/sharpness/freq are sensible starting points.
            glitter: flin("Glitter", 0.0, 0.0, 1.0),
            glitter_density: flin("Glitter Density", 12.0, 1.0, 60.0),
            glitter_sharpness: flin("Glitter Sharpness", 0.6, 0.0, 1.0),
            diffraction: flin("Diffraction", 0.0, 0.0, 1.0),
            diffraction_freq: flin("Diffraction Frequency", 8.0, 1.0, 30.0),
            retro: flin("Retroreflection", 0.0, 0.0, 1.0),
            // Spectral emission (#214 T5) — amounts default 0 (byte-identical); hue
            // 0.33 (green blacklight) + 3000K (warm ember) are sensible starting points.
            fluorescence: flin("Fluorescence", 0.0, 0.0, 1.0),
            fluor_hue: flin("Fluorescence Hue", 0.33, 0.0, 1.0),
            incandescence: flin("Incandescence", 0.0, 0.0, 1.0),
            temperature: flin("Temperature (K)", 3000.0, 1000.0, 12000.0),
            metallic: flin("Metallic", 0.0, 0.0, 1.0),
            roughness: flin("Roughness", 0.35, 0.0, 1.0),
            exposure: flin("Exposure (EV)", 0.0, -8.0, 4.0),
            env_intensity: flin("Env Intensity", 1.0, 0.0, 4.0),
            env_rotation: flin("Env Rotation", 0.0, 0.0, 360.0),
            bloom_intensity: flin("Bloom", 0.08, 0.0, 1.0),
            bloom_threshold: flin("Bloom Threshold", 1.0, 0.0, 4.0),

            // Surface FX — amounts default 0 (no change to the current look).
            subsurface: flin("Translucency", 0.0, 0.0, 1.0),
            sss_distortion: flin("Translucency Distortion", 0.3, 0.0, 1.0),
            sss_power: flin("Translucency Power", 4.0, 1.0, 16.0),
            iridescence: flin("Iridescence", 0.0, 0.0, 1.0),
            irid_scale: flin("Iridescence Scale", 2.0, 0.1, 6.0),
            irid_shift: flin("Iridescence Hue", 0.0, 0.0, 1.0),
            palette: EnumParam::new("Palette", Palette::Native),

            // Metaball — radius > unit node spacing so blobs fuse into a skin.
            metaball_radius: flin("Metaball Radius", 1.3, 1.0, 100.0),
            metaball_threshold: flin("Metaball Threshold", 0.6, 0.05, 3.0),
            metaball_smooth: flin("Metaball Smoothness", 1.0, 0.0, 4.0),

            splat_radius: flin("Splat Radius", 0.55, 0.0, 8.0),
            splat_opacity: flin("Splat Opacity", 0.85, 0.0, 1.0),
            splat_falloff: flin("Splat Falloff", 1.0, 0.2, 6.0),
            splat_mode: EnumParam::new("Splat Tier", SplatMode::Lit),
            splat_cutoff: flin("Splat Cutoff", 0.003, 0.0, 0.2),
            splat_aniso: flin("Splat Anisotropy", 1.0, 0.0, 3.0),
            splat_scatter: IntParam::new("Splat Scatter", 1, IntRange::Linear { min: 1, max: 16 }),
            splat_jitter: flin("Splat Jitter", 0.35, 0.0, 1.0),
            splat_solid: flin("Splat Solidity", 0.0, 0.0, 1.0),

            // Contiguous tubes — welding off by default (segmented Swept Tubes
            // unchanged); caps on with a half-dome so enabling weld reads well.
            tube_weld: BoolParam::new("Contiguous Tube", false),
            tube_end_cap: BoolParam::new("Tube End Caps", true),
            tube_cap_round: flin("Cap Rounding", 0.5, 0.0, 1.0),
            tube_cap_bevel: flin("Cap Bevel", 0.0, 0.0, 1.0),
            // 1 = circle (the current welded look); dial toward 0 to square it off.
            tube_profile: flin("Tube Profile", 1.0, 0.0, 1.0),

            // Voxel mode defaults: a 96³ grid, mid threshold, ~unit-thick strands,
            // no glow, full AO, gentle shadows, no posterize, no beat pump.
            voxel_res: flin("Voxel Grid", 96.0, 16.0, 256.0),
            voxel_threshold: flin("Voxel Threshold", 0.5, 0.05, 3.0),
            voxel_radius: flin("Voxel Radius", 1.0, 0.3, 8.0),
            voxel_emission: flin("Voxel Emission", 0.0, 0.0, 8.0),
            voxel_ao: flin("Voxel AO", 1.0, 0.0, 1.0),
            voxel_shadow: flin("Voxel Shadow", 0.6, 0.0, 1.0),
            voxel_quantize: flin("Voxel Quantize", 0.0, 0.0, 16.0),
            voxel_beat: flin("Voxel Beat→Threshold", 0.0, -1.0, 1.0),
            voxel_gi: BoolParam::new("Voxel GI", false),
            voxel_gi_strength: flin("Voxel GI Strength", 1.0, 0.0, 4.0),
            voxel_gi_distance: flin("Voxel GI Distance", 0.5, 0.05, 1.0),
            voxel_gi_sky: flin("Voxel GI Sky", 0.2, 0.0, 2.0),

            // Bioluminescence — all inert at defaults (cycle 0, ripple intensity 0).
            color_cycle: flin("Colour Cycle", 0.0, -2.0, 2.0),
            ripple_intensity: flin("Ripple Intensity", 0.0, 0.0, 10.0),
            ripple_speed: flin("Ripple Speed", 0.3, -4.0, 4.0),
            ripple_freq: flin("Ripple Frequency", 1.0, 0.25, 8.0),
            ripple_sharp: flin("Ripple Sharpness", 2.0, 1.0, 16.0),
            ripple_geom: EnumParam::new("Ripple Geometry", RippleGeom::Radial),

            membrane_weave: EnumParam::new("Membrane Weave", MembraneWeave::Auto),
            membrane_show_strands: BoolParam::new("Show Strands", false),
            membrane_arms: BoolParam::new("Skin Arms", false),
            membrane_arm_build: EnumParam::new("Arm Build", MembraneArmBuild::Impostor),
            membrane_close: BoolParam::new("Close Seam (360°)", false),
            membrane_arm_radius: flin("Arm Radius", 0.0, 0.0, 3.0),

            // Reaction–diffusion — off by default (intensity 0); spot-forming rates.
            rd_intensity: flin("RD Intensity", 0.0, 0.0, 8.0),
            rd_feed: flin("RD Feed", 0.037, 0.01, 0.1),
            rd_kill: flin("RD Kill", 0.06, 0.03, 0.08),
            rd_scale: flin("RD Scale", 0.02, 0.0, 0.1),
            rd_albedo_mix: flin("RD Pigment", 0.0, 0.0, 1.0),

            // Ambient occlusion — off by default (no prepass, default look intact).
            ssao: BoolParam::new("Ambient Occlusion", false),
            ssao_radius: flin("AO Radius", 1.5, 0.1, 8.0),
            ssao_intensity: flin("AO Intensity", 1.0, 0.0, 2.0),
            ssao_bias: flin("AO Bias", 0.025, 0.0, 0.2),

            // Jewel Box (#80) — all off / neutral by default (look unchanged).
            ssr: BoolParam::new("Reflections (SSR)", false),
            ssr_intensity: flin("SSR Intensity", 1.0, 0.0, 2.0),
            ssr_max_roughness: flin("SSR Max Roughness", 0.4, 0.0, 1.0),
            ssr_thickness: flin("SSR Thickness", 0.5, 0.05, 4.0),
            ssr_steps: ilin("SSR Steps", 48, 8, 160),
            gi: BoolParam::new("Bounced GI", false),
            gi_intensity: flin("GI Intensity", 1.0, 0.0, 4.0),
            gi_falloff: flin("GI Reach", 1.0, 0.0, 4.0),
            glass_dispersion: flin("Glass Dispersion", 0.0, 0.0, 1.0),
            glass_caustic: flin("Glass Caustic", 0.0, 0.0, 2.0),
            glass_thin_film: flin("Glass Thin-Film", 0.0, 0.0, 1.0),

            // Reflection look (#163 Tier 1) — all 0 → today's look unchanged.
            reflect_tint: flin("Reflect Palette", 0.0, 0.0, 2.0),
            chrome_purity: flin("Chrome Purity", 0.0, 0.0, 1.0),
            glass_clarity: flin("Glass Clarity", 0.0, 0.0, 1.0),
            f0_override: flin("Reflectivity (Std)", 0.0, 0.0, 1.0),

            // Temporal pass (#152 Tier 2) — off by default (image unchanged).
            taa_enabled: BoolParam::new("TAA", false),
            taa_blend: flin("TAA Blend", 0.1, 0.05, 1.0),
            taa_sharpen: flin("TAA Sharpen", 0.2, 0.0, 1.0),
            motion_blur: BoolParam::new("Motion Blur", false),
            mb_amount: flin("Motion Blur Amount", 0.5, 0.0, 2.0),
            mb_samples: ilin("Motion Blur Samples", 8, 2, 32),
            stochastic_glass: BoolParam::new("Stochastic Glass", false),

            // Screen-space GI (#152 Tier 2) — off by default.
            ssgi: BoolParam::new("Screen-Space GI", false),
            ssgi_intensity: flin("SSGI Intensity", 1.0, 0.0, 4.0),
            ssgi_radius: flin("SSGI Radius", 2.0, 0.1, 16.0),
            ssgi_rays: ilin("SSGI Rays", 4, 1, 16),

            // Cast shadows (#152 Tier 3) — off by default.
            shadow_enabled: BoolParam::new("Cast Shadows", false),
            shadow_bias: flin("Shadow Bias", 0.0015, 0.0, 0.02),
            shadow_strength: flin("Shadow Strength", 1.0, 0.0, 1.0),

            // Voxel GI (#152 Tier 3, #10) — off by default.
            vxgi_enabled: BoolParam::new("Voxel GI", false),
            vxgi_intensity: flin("Voxel GI Intensity", 1.0, 0.0, 4.0),
            vxgi_rays: ilin("Voxel GI Rays", 4, 1, 8),
            vxgi_steps: ilin("Voxel GI Steps", 12, 2, 32),
            // Reflection probe / parallax (#163 Tier 2) — EnvOnly = today's look.
            refl_source: EnumParam::new("Reflection Source", ReflectionSource::EnvOnly),
            refl_box_scale: flin("Reflect Box Scale", 1.0, 0.1, 8.0),
            refl_box_height: flin("Reflect Box Height", 1.0, 0.1, 8.0),
            refl_blend: flin("Reflect Parallax Blend", 1.0, 0.0, 1.0),

            // VXGI specular reflections (#163 Tier 3) — off by default (strength 0).
            vxgi_spec_strength: flin("VXGI Reflection", 0.0, 0.0, 2.0),
            vxgi_spec_aperture: flin("VXGI Refl Aperture", 0.2, 0.0, 1.0),
            vxgi_spec_reach: flin("VXGI Refl Reach", 1.0, 0.1, 2.0),
            vxgi_spec_steps: ilin("VXGI Refl Steps", 24, 4, 64),
            // Membrane screen-space FX on by default (turn off if the extra depth pass costs perf).
            membrane_fx: BoolParam::new("Membrane Screen-Space FX", true),

            // Cinematic finishing (#167 Tier 1) — halation + lens flares off by default.
            hal_amount: flin("Halation", 0.0, 0.0, 2.0),
            hal_threshold: flin("Halation Threshold", 0.6, 0.0, 1.0),
            hal_width: flin("Halation Width", 1.0, 0.1, 3.0),
            hal_warmth: flin("Halation Warmth", 0.6, 0.0, 1.0),
            lf_amount: flin("Lens Flare", 0.0, 0.0, 2.0),
            lf_ghosts: flin("Flare Ghosts", 0.5, 0.0, 1.0),
            lf_halo: flin("Flare Halo", 0.4, 0.0, 1.0),
            lf_streak: flin("Flare Streak", 0.3, 0.0, 1.0),
            // Emissive cubes as real lights (#167 Tier 3) — off by default.
            ml_enabled: BoolParam::new("Cubes as Lights", false),
            ml_intensity: flin("Cube Light Intensity", 1.0, 0.0, 8.0),
            ml_radius: flin("Cube Light Radius", 0.5, 0.05, 2.0),
            ml_count: ilin("Cube Light Count", 24, 1, 64),
            ml_restir: BoolParam::new("Cube Lights ReSTIR", false),

            // On by default, so the window opens in HDR straight away (falls back
            // to SDR automatically if the display/surface can't do EDR).
            hdr_output: BoolParam::new("HDR Output (EDR)", true),
            hdr_knee: flin("HDR Roll-off", 0.8, 0.5, 1.0),
            hdr_wide: BoolParam::new("HDR Wide Gamut (Rec.2020)", true),
            hdr_vivid: flin("HDR Vividness", 1.0, 0.0, 1.0),
            tonemap: EnumParam::new("Tone Map", ToneMap::Aces),
            msaa: EnumParam::new("MSAA", Msaa::X4),

            bg_tonemap: EnumParam::new("Env Tone Map", ToneMap::Agx),
            bg_visible: BoolParam::new("Background Visible", true),
            bg_intensity: flin("Background Brightness", 1.0, 0.0, 2.0),
            mat_hue: flin("Material Hue", 0.0, 0.0, 1.0),
            mat_hue_cycle: flin("Material Hue Cycle", 0.0, -2.0, 2.0),
            mat_saturation: flin("Material Saturation", 1.0, 0.0, 1.0),
            mat_value: flin("Material Value", 1.0, 0.0, 1.0),
            scen_hue: flin("Scenery Hue", 0.0, 0.0, 1.0),
            scen_hue_cycle: flin("Scenery Hue Cycle", 0.0, -2.0, 2.0),
            scen_saturation: flin("Scenery Saturation", 1.0, 0.0, 1.0),
            scen_value: flin("Scenery Value", 1.0, 0.0, 1.0),
            sky_reflect_clouds: BoolParam::new("Reflect Sky Clouds", false),
            sky_cloud_cover: flin("Reflection Cloud Cover", 0.55, 0.0, 1.0),
            sky_cloud_speed: flin("Reflection Cloud Speed", 0.08, 0.0, 1.0),
            sky_cloud_strength: flin("Reflection Cloud Strength", 0.7, 0.0, 1.0),
            env_tint_hue: flin("Env Tint Hue", 40.0, 0.0, 360.0),
            env_tint_amt: flin("Env Tint Amount", 0.0, 0.0, 1.0),

            // Terrain backdrop (off by default).
            terrain_enabled: BoolParam::new("Terrain Backdrop", false),
            terrain_height: flin("Terrain Height", 140.0, 20.0, 400.0),
            terrain_snow: flin("Terrain Snow Line", 0.62, 0.0, 1.0),
            terrain_fog: flin("Terrain Fog", 1.0, 0.0, 4.0),
            terrain_sun_elev: flin("Terrain Sun Elevation", 14.0, -90.0, 90.0),
            terrain_sun_azim: flin("Terrain Sun Azimuth", 145.0, 0.0, 360.0),
            terrain_sun_int: flin("Terrain Sun Intensity", 1.0, 0.0, 3.0),
            terrain_scroll: flin("Terrain Fly Speed", 1.0, 0.0, 1.0),
            terrain_ride: flin("Terrain Ride Height", 55.0, 5.0, 300.0),
            terrain_noise: EnumParam::new("Terrain Noise", TerrainNoise::White),
            terrain_palette: EnumParam::new("Terrain Palette", TerrainPalette::Alpine),
            terrain_emissive: flin("Terrain Emissive", 0.0, 0.0, 4.0),
            terrain_day_speed: flin("Terrain Day Speed", 0.0, 0.0, 2.0),
            terrain_sun_scene: BoolParam::new("Terrain Sun Lights Scene", true),
            terrain_scatter: flin("Terrain Scattering", 0.0, 0.0, 2.0),
            terrain_godray: flin("Terrain God Rays", 0.0, 0.0, 2.0),
            terrain_water: BoolParam::new("Terrain Water", false),
            terrain_water_level: flin("Terrain Water Level", 0.25, 0.0, 1.0),
            terrain_water_hue: flin("Terrain Water Hue", 0.55, 0.0, 1.0),
            terrain_water_ripple: flin("Terrain Water Ripple", 0.4, 0.0, 1.0),
            render_scale: flin("Render Scale", 0.5, 0.25, 1.0),
            render_auto: BoolParam::new("Auto Resolution (60 FPS)", false),

            // Capture / production frame (#135) — Native by default (no change).
            aspect_preset: EnumParam::new("Output Aspect", AspectPreset::Native),
            out_long_edge: ilin("Output Long Edge (0 = match display)", 0, 0, 7680),
            out_custom_w: ilin("Custom Width (px)", 1920, 64, 7680),
            out_custom_h: ilin("Custom Height (px)", 1080, 64, 7680),
            letterbox_r: flin("Letterbox R", 0.0, 0.0, 1.0),
            letterbox_g: flin("Letterbox G", 0.0, 0.0, 1.0),
            letterbox_b: flin("Letterbox B", 0.0, 0.0, 1.0),
            frame_guide: BoolParam::new("Frame Guide", false),
            lock_window: BoolParam::new("Lock Window to Output", false),

            // Capture overlay (#135 Phase 2) — off by default; on = the maths-account card.
            overlay_enabled: BoolParam::new("Overlay", false),
            overlay_opacity: flin("Overlay Opacity", 0.9, 0.0, 1.0),
            overlay_scale: flin("Overlay Scale", 1.0, 0.4, 2.5),
            overlay_title: BoolParam::new("Overlay Title", true),
            overlay_desc: BoolParam::new("Overlay Description", true),
            overlay_formula: BoolParam::new("Overlay Formula", true),
            overlay_readouts: BoolParam::new("Overlay Readouts", true),
            overlay_handle: BoolParam::new("Overlay Handle", true),
            overlay_panel_r: flin("Overlay Panel R", 0.04, 0.0, 1.0),
            overlay_panel_g: flin("Overlay Panel G", 0.05, 0.0, 1.0),
            overlay_panel_b: flin("Overlay Panel B", 0.08, 0.0, 1.0),
            overlay_panel_opacity: flin("Overlay Panel Opacity", 0.55, 0.0, 1.0),
            overlay_text_r: flin("Overlay Text R", 0.95, 0.0, 1.0),
            overlay_text_g: flin("Overlay Text G", 0.95, 0.0, 1.0),
            overlay_text_b: flin("Overlay Text B", 0.97, 0.0, 1.0),

            // Capture decoration: axes + wireframe (#135 P5) — off by default.
            axes_on: BoolParam::new("Axes", false),
            axes_len: flin("Axis Length", 4.0, 0.5, 40.0),
            axes_thick: flin("Axis Thickness", 0.12, 0.01, 2.0),
            axes_opacity: flin("Axis Opacity", 1.0, 0.0, 1.0),
            axes_ticks: BoolParam::new("Axis Ticks", false),
            axes_labels: BoolParam::new("Axis Labels", true),
            box_on: BoolParam::new("Wireframe Box", false),
            box_extent: flin("Box Extent", 4.0, 0.5, 40.0),
            box_subdiv: ilin("Box Subdivisions", 1, 1, 16),
            box_r: flin("Box R", 0.5, 0.0, 1.0),
            box_g: flin("Box G", 0.55, 0.0, 1.0),
            box_b: flin("Box B", 0.7, 0.0, 1.0),
            box_opacity: flin("Box Opacity", 0.5, 0.0, 1.0),

            // --- Field Chamber (#346) — off by default → byte-identical ---
            panels_on: BoolParam::new("Field Chamber", false),
            panel_style: EnumParam::new("Chamber Style", PanelStyle::Flat),
            panel_rear: BoolParam::new("Chamber Rear Scope", true),
            panel_right: BoolParam::new("Chamber Right Spectrum", true),
            panel_opacity: flin("Chamber Opacity", 0.85, 0.0, 1.0),
            panel_fill: flin("Chamber Wall Fill", 0.9, 0.1, 1.0),
            panel_scope_amp: flin("Scope Amp", 1.0, 0.05, 8.0),
            // Deprecated (#346): the chamber spectrum now uses the Audio-tab RTA's fixed
            // dB window (−72..0); these are no longer read but kept for preset compat.
            panel_db_floor: flin("Scope dB Floor (unused)", -60.0, -120.0, -6.0),
            panel_db_top: flin("Scope dB Top (unused)", 0.0, -60.0, 12.0),
            panel_material: EnumParam::new("Chamber Material", MaterialType::Standard),
            panel_metallic: flin("Chamber Metallic", 0.8, 0.0, 1.0),
            panel_roughness: flin("Chamber Roughness", 0.25, 0.0, 1.0),
            panel_emissive: flin("Chamber Emissive", 0.0, 0.0, 8.0),
            panel_thickness: flin("Chamber Line Thickness", 0.03, 0.005, 0.3),
            panel_wall_rel: BoolParam::new("Chamber Camera-Relative Walls", false),
            panel_scope_time_ms: flin("Scope Time (ms)", 20.0, 2.0, 200.0),
            panel_scope_trigger: ilin("Scope Trigger", 1, 0, 2),
            panel_scope_channel: ilin("Scope Channel", 2, 0, 2),

            // Post-composite creative FX (#152) — off by default (image unchanged).
            fx_enabled: BoolParam::new("Post FX", false),
            fx_style: EnumParam::new("Style", RenderStyle::None),
            fx_style_amt: flin("Style Amount", 4.0, 1.0, 32.0),
            fx_dof: flin("Depth of Field", 0.0, 0.0, 1.0),
            fx_dof_focus: flin("Focus", 0.5, 0.0, 1.0),
            fx_dof_range: flin("Focus Range", 0.25, 0.01, 1.0),
            fx_chroma: flin("Chromatic Aberration", 0.0, 0.0, 1.0),
            fx_vignette: flin("Vignette", 0.0, 0.0, 1.0),
            fx_grain: flin("Film Grain", 0.0, 0.0, 0.5),
            fx_grade_sat: flin("Saturation", 1.0, 0.0, 2.0),
            fx_grade_contrast: flin("Contrast", 1.0, 0.0, 2.0),
            fx_grade_temp: flin("Temperature", 0.0, -1.0, 1.0),
            fx_grade_gain: flin("Gain", 1.0, 0.0, 2.0),
            fx_feedback: flin("Feedback Trails", 0.0, 0.0, 0.97),
            fx_outline: flin("Outline Threshold", 0.15, 0.01, 1.0),

            // Emissive volume surface mode (#152).
            volume_radius: flin("Volume Radius", 1.5, 0.5, 50.0),
            volume_density: flin("Volume Density", 1.0, 0.0, 8.0),
            volume_emission: flin("Volume Emission", 1.5, 0.0, 16.0),
            volume_absorption: flin("Volume Absorption", 0.6, 0.0, 8.0),
            volume_steps: ilin("Volume Steps", 96, 16, 256),

            terrain_seed: ilin("Terrain Seed", 1, 1, 999),
            terrain_ridged: BoolParam::new("Terrain Ridged", false),
            terrain_brightness: flin("Terrain Brightness", 1.0, 0.0, 3.0),
            terrain_haze: flin("Terrain Haze", 0.0, 0.0, 1.0),
            terrain_steps: ilin("Terrain March Steps", 220, 32, 400),
            terrain_octaves: ilin("Terrain March Octaves", 7, 2, 9),
            terrain_res: EnumParam::new("Terrain Resolution", TerrainRes::Full),
            stars_enabled: BoolParam::new("Starfield", false),
            stars_brightness: flin("Star Brightness", 1.0, 0.0, 5.0),
            stars_twinkle: flin("Star Twinkle", 0.35, 0.0, 1.0),
            stars_twinkle_speed: flin("Star Twinkle Speed", 1.5, 0.0, 8.0),
            stars_size: flin("Star Size", 1.6, 0.5, 6.0),
            stars_latitude: flin("Star Latitude", 35.0, -90.0, 90.0),
            stars_sky_speed: flin("Star Sky Rotation", 0.02, 0.0, 1.0),
            stars_mag_limit: flin("Star Magnitude Limit", 6.5, 1.0, 8.0),
            stars_saturation: flin("Star Saturation", 0.55, 0.0, 1.0),
            stars_sun: BoolParam::new("Sun Disc", true),
            stars_sun_bright: flin("Sun Brightness", 6.0, 0.0, 40.0),
            stars_sun_size: flin("Sun Size", 0.8, 0.1, 8.0),
            stars_sun_warmth: flin("Sun Warmth", 0.5, 0.0, 1.0),
            atmos_enabled: BoolParam::new("Atmosphere", true),
            atmos_turbidity: flin("Atmosphere Turbidity", 2.0, 0.5, 10.0),
            atmos_mie_g: flin("Atmosphere Mie g", 0.76, 0.0, 0.95),
            atmos_sun_int: flin("Atmosphere Sun Intensity", 22.0, 0.0, 60.0),
            atmos_ground_albedo: flin("Atmosphere Ground Albedo", 0.3, 0.0, 1.0),
            atmos_exposure: flin("Atmosphere Exposure", 1.0, 0.1, 4.0),
            atmos_aerial: flin("Atmosphere Aerial Perspective", 1.0, 0.0, 2.0),
            atmos_rayleigh: flin("Atmosphere Rayleigh", 1.0, 0.2, 3.0),
            clouds_enabled: BoolParam::new("Volumetric Clouds", false),
            clouds_coverage: flin("Cloud Coverage", 0.5, 0.0, 1.0),
            clouds_density: flin("Cloud Density", 1.0, 0.1, 4.0),
            clouds_base: flin("Cloud Base Altitude", 800.0, 200.0, 3000.0),
            clouds_thickness: flin("Cloud Thickness", 500.0, 100.0, 2000.0),
            clouds_steps: ilin("Cloud March Steps", 48, 8, 128),
            clouds_detail: flin("Cloud Detail", 0.5, 0.0, 1.0),
            clouds_drift: flin("Cloud Drift Speed", 1.0, 0.0, 4.0),
            clouds_hg: flin("Cloud Forward Scatter", 0.55, 0.0, 0.95),
            clouds_absorption: flin("Cloud Absorption", 1.0, 0.1, 4.0),
            clouds_shadow: flin("Cloud Shadow Strength", 0.7, 0.0, 1.0),
            clouds_ambient: flin("Cloud Ambient Fill", 0.5, 0.0, 2.0),
            ocean_enabled: BoolParam::new("FFT Ocean", false),
            ocean_level: flin("Ocean Level", 0.0, -200.0, 400.0),
            ocean_wind_speed: flin("Ocean Wind Speed", 14.0, 2.0, 40.0),
            ocean_wind_dir: flin("Ocean Wind Direction", 45.0, 0.0, 360.0),
            ocean_amplitude: flin("Ocean Amplitude", 1.0, 0.0, 4.0),
            ocean_choppiness: flin("Ocean Choppiness", 1.0, 0.0, 3.0),
            ocean_tile_size: flin("Ocean Tile Size", 600.0, 100.0, 2000.0),
            ocean_foam: flin("Ocean Foam", 1.0, 0.0, 3.0),
            ocean_glitter: flin("Ocean Glitter", 1.0, 0.0, 4.0),
            ocean_hue: flin("Ocean Hue", 0.54, 0.0, 1.0),
            ocean_depth: flin("Ocean Depth Absorption", 0.6, 0.0, 1.0),

            // Particle Aura off by default; the rest carry sane defaults so
            // turning it on reads well (200k motes on a 32³ grid, a tight halo).
            particles_tier: EnumParam::new("Particle Aura", ParticleTier::Off),
            particles_count_k: ilin("Particles Count (k)", 200, 1, 4000),
            particles_grid_res: ilin("Particles Grid Res", 32, 8, 96),
            particles_speed: flin("Particles Speed", 1.0, 0.0, 8.0),
            particles_lifetime: flin("Particles Lifetime", 3.0, 0.2, 20.0),
            particles_spawn_radius: flin("Particles Spawn Radius", 0.6, 0.0, 8.0),
            particles_size: flin("Particles Size", 0.06, 0.005, 1.0),
            particles_emissive: flin("Particles Emissive", 1.0, 0.0, 8.0),
            particles_ribbon: BoolParam::new("Particles Ribbons", false),
            particles_ribbon_stretch: flin("Particles Ribbon Stretch", 0.5, 0.0, 4.0),
            particles_hue_shift: flin("Particles Hue Shift", 0.0, 0.0, 1.0),
            particles_beat_burst: flin("Particles Beat Burst", 0.3, 0.0, 1.0),
            particles_drag: flin("Particles Drag", 0.1, 0.0, 1.0),
            particles_turbulence: flin("Particles Turbulence", 0.2, 0.0, 1.0),
            particles_alpha: flin("Particles Opacity", 1.0, 0.0, 1.0),
            particles_hide_generator: BoolParam::new("Hide Generator", false),
            particles_beads: BoolParam::new("Particles Shaded Beads", false),
            particles_metallic: flin("Particles Bead Metallic", 0.9, 0.0, 1.0),
            particles_roughness: flin("Particles Bead Roughness", 0.2, 0.0, 1.0),
            particles_material: EnumParam::new("Particles Bead Material", ParticleMaterial::Standard),
            particles_shape: EnumParam::new("Particles Bead Shape", ParticleShape::Sphere),
            particles_ior: flin("Particles Bead IOR", 1.45, 1.0, 2.5),
            particles_shape_param: flin("Particles Bead Shape Amount", 0.5, 0.0, 1.0),
            particles_beads_rt: BoolParam::new("Particles Beads in RT", false),
            particles_bead_hue: flin("Particles Bead Hue", 0.0, 0.0, 1.0),
            particles_bead_hue_cycle: flin("Particles Bead Hue Cycle", 0.0, -2.0, 2.0),
            particles_bead_sat: flin("Particles Bead Saturation", 1.0, 0.0, 1.0),
            particles_bead_val: flin("Particles Bead Value", 1.0, 0.0, 1.0),
            particles_bead_emissive: flin("Particles Bead Emissive", 0.0, 0.0, 16.0),
            fluid_force: flin("Fluid Stir Force", 12.0, 0.0, 60.0),
            fluid_vorticity: flin("Fluid Vorticity", 6.0, 0.0, 30.0),
            fluid_dissipation: flin("Fluid Dissipation", 0.4, 0.0, 4.0),
            fluid_iters: ilin("Fluid Pressure Iters", 24, 4, 80),
            fluid_inflow_decay: flin("Fluid Inflow Decay", 1.5, 0.0, 8.0),
            // Fluid Ink (#182 Tier 1) off by default; defaults tuned so turning
            // it on reads as luminous ink immediately (match ipc::Shared).
            ink_enabled: BoolParam::new("Fluid Ink", false),
            ink_rate: flin("Ink Injection Rate", 2.0, 0.0, 10.0),
            ink_radius: flin("Ink Injection Radius", 1.5, 0.5, 4.0),
            ink_extinction: flin("Ink Extinction", 4.0, 0.0, 20.0),
            ink_scatter: flin("Ink Scatter", 1.0, 0.0, 4.0),
            ink_emissive: flin("Ink Emissive", 0.6, 0.0, 8.0),
            ink_anisotropy: flin("Ink Anisotropy", 0.45, -0.9, 0.9),
            ink_dissipation: flin("Ink Dissipation", 0.15, 0.0, 2.0),
            ink_steps: flin("Ink March Steps", 96.0, 16.0, 256.0),
            ink_maccormack: BoolParam::new("Ink Sharp Advection", true),
            ink_half_res: BoolParam::new("Ink Half-Res March", true),
            ink_reveal: flin("Ink Reveal", 0.3, 0.0, 10.0),
            // Fluid medium Tier 2 (#182): all inert (match ipc::Shared).
            fl2_boundaries: BoolParam::new("Fluid Boundaries", false),
            fl2_buoyancy: flin("Fluid Buoyancy", 0.0, -2.0, 2.0),
            fl2_heat_decay: flin("Fluid Heat Decay", 0.3, 0.0, 2.0),
            fl2_detail: flin("Ink Micro-Detail", 0.0, 0.0, 1.0),
            fl2_splash: flin("Fluid Beat Splash", 0.0, 0.0, 8.0),
            fl2_dye_gate: flin("Ink Beat Gate", 0.0, 0.0, 1.0),
            fl2_res: ilin("Fluid Sim Res Override", 0, 0, 128),
            fl2_substeps: ilin("Fluid Substeps", 1, 1, 4),
            // MLS-MPM liquid (#182 Tier 3a) off by default (match ipc::Shared).
            liq_enabled: BoolParam::new("Liquid", false),
            liq_count: ilin("Liquid Particles (k)", 100, 10, 300),
            liq_res: ilin("Liquid Grid Res", 64, 16, 96),
            liq_gravity: flin("Liquid Gravity", 0.0, 0.0, 30.0),
            liq_stiffness: flin("Liquid Stiffness", 3.0, 0.5, 20.0),
            liq_viscosity: flin("Liquid Viscosity", 0.05, 0.0, 1.0),
            liq_container: flin("Liquid Container Size", 10.0, 2.0, 40.0),
            liq_open_top: BoolParam::new("Liquid Open Top", false),
            liq_collide: BoolParam::new("Liquid Generator Collides", true),
            liq_stir: flin("Liquid Stir Gain", 1.0, 0.0, 3.0),
            liq_density: flin("Liquid Surface Density", 1.0, 0.2, 4.0),
            liq_threshold: flin("Liquid Surface Threshold", 0.35, 0.05, 1.0),
            liq_hue: flin("Liquid Hue", 0.55, 0.0, 1.0),
            liq_sat: flin("Liquid Saturation", 0.35, 0.0, 1.0),
            liq_substeps: ilin("Liquid Substeps", 2, 1, 4),
            liq_offset_y: flin("Liquid Vertical Offset", 0.0, -10.0, 10.0),
            liq_shape: EnumParam::new("Liquid Container Shape", LiqShape::Box),
            liq_reveal: flin("Liquid Reveal", 0.0, 0.0, 1.0),
            // #182 Tier 4 coupling (match ipc::Shared: all off, sharpness 1).
            fgi_gi: flin("Fluid To GI", 0.0, 0.0, 2.0),
            fgi_shadow: flin("Fluid Shadows Scene", 0.0, 0.0, 1.0),
            fgi_receive: BoolParam::new("Fluid Receives Shadows", false),
            fgi_sway: flin("Fluid Sways Generator", 0.0, 0.0, 1.0),
            ca_amount: flin("Caustics", 0.0, 0.0, 2.0),
            ca_sharpness: flin("Caustic Sharpness", 1.0, 0.1, 4.0),
            liq_material: EnumParam::new("Liquid Material", LiqMaterial::UseScene),
            liq_metallic: flin("Liquid Metallic", 0.0, 0.0, 1.0),
            liq_roughness: flin("Liquid Roughness", 0.05, 0.0, 1.0),
            liq_ior: flin("Liquid IOR", 1.33, 1.0, 2.5),
            ghost_light: BoolParam::new("Hidden Generator Keeps Lighting", false),
            liq_render: EnumParam::new("Liquid Render", LiqRender::Isosurface),
            liq_absorb: flin("Liquid Absorption", 0.5, 0.0, 4.0),
            liq_glow: flin("Liquid Glow", 0.0, 0.0, 4.0),
            liq_chrome_purity: flin("Liquid Chrome Purity", 0.0, 0.0, 1.0),
            liq_glass_clarity: flin("Liquid Glass Clarity", 0.0, 0.0, 1.0),
            liq_f0: flin("Liquid F0 Override", 0.0, 0.0, 1.0),
            liq_dispersion: flin("Liquid Dispersion", 0.0, 0.0, 1.0),
            liq_gcaustic: flin("Liquid Glass Caustic", 0.0, 0.0, 2.0),
            liq_thin_film: flin("Liquid Thin Film", 0.0, 0.0, 1.0),
            rt_enable: BoolParam::new("Ray Tracing (Hardware)", false),
            rt_debug: EnumParam::new("RT Debug View", RtDebugView::Off),
            pathtrace_enable: BoolParam::new("Path Tracer (P)", false),
            pt_dielectric: BoolParam::new("PT Dielectric", false),
            pt_absorb: flin("PT Absorption", 0.0, 0.0, 4.0),
            pt_composite: EnumParam::new("PT Composite", PtComposite::Replace),
            pt_augment: flin("PT Augment", 0.0, 0.0, 1.0),
            spectral_enable: BoolParam::new("Spectral Dispersion", false),
            spectral_abbe: flin("Abbe Number", 40.0, 15.0, 90.0),
            spectral_secondaries: IntParam::new("Spectral Samples", 3, IntRange::Linear { min: 0, max: 8 }),
            pt_caustics: BoolParam::new("PT Caustics", false),
            pt_caustic_photons: IntParam::new("Caustic Photons (k)", 128, IntRange::Linear { min: 16, max: 1024 }),
            pt_caustic_intensity: flin("Caustic Intensity", 1.0, 0.0, 4.0),
            pt_caustic_radius: flin("Caustic Radius", 2.0, 0.0, 8.0),
            // Neural radiance cache — live (#256 Tier 0). Off = byte-identical.
            nrc_enable: BoolParam::new("Radiance Cache", false),
            nrc_confidence: flin("Cache Confidence", 0.5, 0.0, 1.0),
            nrc_learn_rate: flin("Cache Learn Rate", 0.02, 0.0, 0.2),
            nrc_omega: flin("Cache Frequency", 4.0, 0.5, 12.0),
            nrc_terminate: IntParam::new("Cache Terminate Bounce", 2, IntRange::Linear { min: 1, max: 8 }),
            nrc_train_samples: IntParam::new("Cache Train Samples", 8, IntRange::Linear { min: 0, max: 64 }),
            nrc_seed: IntParam::new("Cache Seed", 1, IntRange::Linear { min: 1, max: 9999 }),
            // Cache RT-stack synergies (#256 Tier 1). Off = byte-identical.
            nrc_guide: BoolParam::new("Cache-Guided Sampling", false),
            nrc_guide_candidates: IntParam::new("Guide Candidates", 4, IntRange::Linear { min: 1, max: 8 }),
            nrc_firefly: BoolParam::new("Cache Firefly Clamp", false),
            nrc_firefly_clamp: flin("Firefly Clamp Strength", 8.0, 1.0, 64.0),
            // Cache light-field uses (#256 Tier 2). Off = byte-identical.
            nrc_gi: BoolParam::new("Cache GI (supersede DDGI)", false),
            nrc_gi_strength: flin("Cache GI Strength", 1.0, 0.0, 4.0),
            nrc_reflect: BoolParam::new("Cache-Lit Reflections", false),
            // Cache hard transport + volumetrics (#256 Tier 3). Off = byte-identical.
            nrc_volume: BoolParam::new("Cache Volumetrics", false),
            nrc_volume_density: flin("Volumetric Density", 0.15, 0.0, 2.0),
            nrc_volume_steps: IntParam::new("Volumetric Steps", 16, IntRange::Linear { min: 1, max: 64 }),
            nrc_volume_strength: flin("Volumetric Strength", 1.0, 0.0, 4.0),
            nrc_caustic: BoolParam::new("Cached Caustics", false),
            nrc_caustic_gain: flin("Cached Caustic Gain", 1.0, 0.0, 4.0),
            rt_shadows: BoolParam::new("RT Shadows", false),
            rt_shadow_soft: flin("RT Shadow Softness", 0.15, 0.0, 1.0),
            rt_shadow_strength: flin("RT Shadow Strength", 1.0, 0.0, 1.0),
            rt_shadow_fill: BoolParam::new("RT Fill Shadow", false),
            rt_reflect: BoolParam::new("RT Reflections", false),
            rt_reflect_intensity: flin("RT Reflection Intensity", 1.0, 0.0, 2.0),
            rt_reflect_rough: flin("RT Reflection Max Roughness", 0.4, 0.0, 1.0),
            rt_reflect_reach: flin("RT Reflection Reach", 2.0, 0.25, 4.0),
            rt_reflect_shadows: BoolParam::new("RT Reflection Hit Shadows", true),
            rt_reflect_rays: ilin("RT Reflection Rays", 16, 1, 16),
            ao_source: EnumParam::new("AO Source", AoSource::Gtao),
            rt_ao_rays: ilin("RT AO Rays", 16, 1, 16),
            rt_gi: BoolParam::new("RT Global Illumination", false),
            rt_gi_intensity: flin("RT GI Intensity", 1.0, 0.0, 4.0),
            rt_gi_rays: ilin("RT GI Rays", 16, 1, 16),
            rt_gi_reach: flin("RT GI Reach", 2.0, 0.25, 4.0),
            rt_gi_shadows: BoolParam::new("RT GI Hit Shadows", true),
            rt_temporal: BoolParam::new("RT Temporal Accumulate", false),
            rt_temporal_feedback: flin("RT Temporal Feedback", 0.9, 0.0, 0.98),
            rt_temporal_beat: flin("RT Temporal Beat Relax", 0.7, 0.0, 1.0),
            rt_temporal_variance: BoolParam::new("RT Temporal Variance (SVGF)", false),
            rt_temporal_accum: flin("RT Temporal Max Samples", 32.0, 1.0, 256.0),
            rt_temporal_clamp: flin("RT Temporal Clamp Width", 3.0, 0.5, 8.0),
            rt_denoise: BoolParam::new("RT Denoise", false),
            rt_denoise_amount: flin("RT Denoise Amount", 1.0, 0.0, 1.0),
            nd_enable: BoolParam::new("Neural Denoise", false),
            nd_strength: flin("Neural Denoise Strength", 0.5, 0.0, 1.0),
            nd_seed: ilin("Neural Denoise Seed", 1, 0, 65535),
            nd_omega: flin("Neural Denoise Feature Scale", 4.0, 0.5, 16.0),
            up_enable: BoolParam::new("Learned Upscale", false),
            up_sharpen: flin("Upscale Sharpen", 0.5, 0.0, 1.5),
            up_seed: ilin("Upscale Seed", 1, 0, 65535),
            neural_enable: BoolParam::new("Neural Field", false),
            neural_seed_a: ilin("Neural Seed A", 1, 0, 65535),
            neural_seed_b: ilin("Neural Seed B", 2, 0, 65535),
            neural_walk: flin("Neural Latent Walk", 0.0, 0.0, 1.0),
            neural_omega: flin("Neural Feature Scale", 4.0, 0.5, 16.0),
            neural_scale: flin("Neural Field Size", 120.0, 10.0, 400.0),
            neural_coord: flin("Neural Detail", 1.5, 0.25, 6.0),
            neural_iso: flin("Neural Iso", 0.0, -1.0, 1.0),
            neural_steps: ilin("Neural Steps", 96, 16, 400),
            neural_march: flin("Neural March Scale", 0.6, 0.1, 1.0),
            neural_color: flin("Neural Colour", 0.8, 0.0, 1.0),
            neural_walk_rate: flin("Neural Walk Rate", 0.0, 0.0, 2.0),
            neural_strands_mode: BoolParam::new("Neural Strand Form", false),
            neural_strands_cols: ilin("Neural Strand Columns", 48, 2, 400),
            neural_strands_rows: ilin("Neural Strand Rows", 48, 2, 400),
            neural_strands_extent: flin("Neural Strand Extent", 2.5, 0.25, 20.0),
            neural_strands_displace: flin("Neural Strand Displace", 1.0, 0.0, 8.0),
        }
    }
}

impl OrganicMathParams {
    /// Snapshot the live parameter values for the matrix builder.
    #[allow(dead_code)]
    pub fn values(&self) -> ParamValues {
        ParamValues {
            loop_count: Vec3::new(
                self.loop_count_x.value() as f32,
                self.loop_count_y.value() as f32,
                self.loop_count_z.value() as f32,
            ),
            loop_count_q: self.loop_count_q.value() as f32,
            rot_amp: Vec3::new(self.rot_amp_x.value(), self.rot_amp_y.value(), self.rot_amp_z.value()),
            trans_amp: Vec3::new(self.trans_amp_x.value(), self.trans_amp_y.value(), self.trans_amp_z.value()),
            trans_mod: Vec3::new(self.trans_mod_x.value(), self.trans_mod_y.value(), self.trans_mod_z.value()),
            scale_amp: self.scale_amp.value(),
        }
    }

    /// Serialize the live params into the shared-memory snapshot for the visual.
    pub fn to_shared(&self) -> crate::ipc::Shared {
        crate::ipc::Shared {
            seq: 0,
            layout_version: crate::ipc::LAYOUT_VERSION,
            loop_count: crate::param_table::pack_loop_count(self),
            rot_amp: crate::param_table::pack_rot_amp(self),
            rot_mod: crate::param_table::pack_rot_mod(self),
            trans_amp: crate::param_table::pack_trans_amp(self),
            trans_mod: crate::param_table::pack_trans_mod(self),
            lighting: crate::param_table::pack_lighting(self),
            scale_amp: self.scale_amp.value(),
            rot_func: self.rot_func.value().to_u32(),
            trans_func: self.trans_func.value().to_u32(),
            scale_func: self.scale_func.value().to_u32(),
            animate: self.animate.value() as u32,
            pulse: self.pulse.value() as u32,
            tempo: self.tempo.value(),
            pulse_depth: 0.0, // reserved (the Pulse Depth knob was removed)
            pbr: crate::param_table::pack_pbr(self),
            hdr_gen: 0, // overwritten by lib.rs process() from the Arc<AtomicU32>
            // transport is filled by lib.rs process() from context.transport()
            transport: [0.0, 0.0, self.tempo.value(), 0.0],
            tempo_sync: self.tempo_sync.value() as u32,
            camera: crate::param_table::pack_camera(self),
            cam_amount: self.cam_amount.value(),
            cam_seq: crate::param_table::pack_cam_seq(self),
            cam_dolly: crate::param_table::pack_cam_dolly(self),
            cam_clock: crate::param_table::pack_cam_clock(self),
            // cam_audio (detected BPM + confidence) is filled by lib.rs process()
            // from the live analyzer; here it's the inert default.
            cam_audio: [0.0; 4],
            cam_frame: crate::param_table::pack_cam_frame(self),
            // cam_story[4] (next-shot trigger) is filled by lib.rs process() from the
            // editor button's atomic; the packer writes 0 there.
            cam_story: crate::param_table::pack_cam_story(self),
            surface_mode: self.surface_mode.value().to_u32(),
            origin_mode: self.origin_mode.value().to_u32(),
            bevel: self.bevel.value(),
            creature: crate::param_table::pack_creature(self),
            creature2: crate::param_table::pack_creature2(self),
            creature3: crate::param_table::pack_creature3(self),
            material: crate::param_table::pack_material(self),
            // Runtime-stamped by the plugin's "Load Material…" button (the hdr_gen
            // pattern); the packer writes 0 (no folder loaded → neutral set).
            material_gen: 0,
            material_layer: crate::param_table::pack_material_layer(self),
            material_grad: crate::param_table::pack_material_grad(self),
            material_layer2: crate::param_table::pack_material_layer2(self),
            material_grad2: crate::param_table::pack_material_grad2(self),
            material_derive: crate::param_table::pack_material_derive(self),
            material_live: crate::param_table::pack_material_live(self),
            maporbit: crate::param_table::pack_maporbit(self),
            // AI-Performer runtime block (#317 T1): runtime-stamped in process() from
            // editor-thread atomics; the packer writes zeros (inert).
            agent: [0.0; 8],
            fieldsim: crate::param_table::pack_fieldsim(self),
            routing: crate::param_table::pack_routing(self),
            surface_fx: crate::param_table::pack_surface_fx(self),
            hdr_output: self.hdr_output.value() as u32,
            hdr_knee: self.hdr_knee.value(),
            hdr_wide: self.hdr_wide.value() as u32,
            tonemap: self.tonemap.value().to_u32(),
            bg_tonemap: self.bg_tonemap.value().to_u32(),
            msaa: self.msaa.value().samples(),
            bg_visible: self.bg_visible.value() as u32,
            bg_intensity: self.bg_intensity.value(),
            env_tint_hue: self.env_tint_hue.value(),
            env_tint_amt: self.env_tint_amt.value(),
            ssao: crate::param_table::pack_ssao(self),
            // audio[] is filled by lib.rs process() from the live band analysis;
            // here it's just the inert default.
            audio: [0.0; 8],
            pulse_source: self.pulse_source.value().to_u32(),
            speed_pulse: crate::param_table::pack_speed_pulse(self),
            cont_shape: self.cont_shape.value(),
            metaball: crate::param_table::pack_metaball(self),
            voxel: crate::param_table::pack_voxel(self),
            voxel_gi: crate::param_table::pack_voxel_gi(self),
            bio: crate::param_table::pack_bio(self),
            membrane: crate::param_table::pack_membrane(self),
            rd: crate::param_table::pack_rd(self),
            generator: self.generator.value().to_u32(),
            frenet: crate::param_table::pack_frenet(self),
            dna: crate::param_table::pack_dna(self),
            attr: crate::param_table::pack_attr(self),
            boids: crate::param_table::pack_boids(self),
            bell: crate::param_table::pack_bell(self),
            atmosphere: crate::param_table::pack_atmosphere(self),
            clouds: crate::param_table::pack_clouds(self),
            ocean: crate::param_table::pack_ocean(self),
            harm: crate::param_table::pack_harm(self),
            ls: crate::param_table::pack_ls(self),
            cn: crate::param_table::pack_cn(self),
            breath: crate::param_table::pack_breath(self),
            pol: crate::param_table::pack_pol(self),
            maxwell: crate::param_table::pack_maxwell(self),
            acoustic: crate::param_table::pack_acoustic(self),
            acoustic2: crate::param_table::pack_acoustic2(self),
            acoustic3: crate::param_table::pack_acoustic3(self),
            analytical: crate::param_table::pack_analytical(self),
            fieldvol: crate::param_table::pack_fieldvol(self),
            colour: crate::param_table::pack_colour(self),
            sonify: crate::param_table::pack_sonify(self),
            voices: [0.0; 64], // runtime-written by process() each block
            // #333: measured each block by the plugin; the param default is silence.
            audiometer: [0.0; 16],
            audiospectrum: [-120.0; 128],
            // #346 Field Chamber: scope frame is runtime-written by process(); the
            // panel look packs from the params.
            scopewave: [0.0; 260],
            chamber: crate::param_table::pack_chamber(self),
            emissive: crate::param_table::pack_emissive(self),
            splat: crate::param_table::pack_splat(self),
            plexus: crate::param_table::pack_plexus(self),
            plexus2: crate::param_table::pack_plexus2(self),
            plexus_node_mat: crate::param_table::pack_plexus_node_mat(self),
            plexus_edge_mat: crate::param_table::pack_plexus_edge_mat(self),
            plexus3: crate::param_table::pack_plexus3(self),
            plexus4: crate::param_table::pack_plexus4(self),
            splat2: crate::param_table::pack_splat2(self),
            mx_eb: crate::param_table::pack_mx_eb(self),
            plexus_overlay: crate::param_table::pack_plexus_overlay(self),
            // Visible-Mind specimen (#367 T1): runtime-stamped in process(); 0 here.
            mind: [0.0; 8],
            // #381 Field Engine live coefficients; `field_gen` is bumped by the
            // editor's sidecar write (overwritten by process() from the atomic).
            field: crate::param_table::pack_field(self),
            field_gen: 0,
            mapattractor: crate::param_table::pack_mapattractor(self),
            mapattractor2: crate::param_table::pack_mapattractor2(self),
            axon: crate::param_table::pack_axon(self),
            phyl: crate::param_table::pack_phyl(self),
            tessellation: crate::param_table::pack_tessellation(self),
            mandelbulb: crate::param_table::pack_mandelbulb(self),
            minimal_surface: crate::param_table::pack_minimal_surface(self),
            synchrotron: crate::param_table::pack_synchrotron(self),
            kifs: crate::param_table::pack_kifs(self),
            kaleido: crate::param_table::pack_kaleido(self),
            instrument: crate::param_table::pack_instrument(self),
            instrument2: crate::param_table::pack_instrument2(self),
            // #423 atlas: runtime-stamped in lib.rs process(); packer writes zeros.
            atlas: [0.0; 8],
            // #407 Tier A: runtime-stamped in lib.rs process() (the field_gen pattern);
            // to_shared leaves it 0.
            fieldclip_gen: 0,
            // #407 Tier B: runtime-stamped by process() (the field_gen/nn_gen pattern);
            // to_shared leaves it 0.
            nca_gen: 0,
            fdtd: crate::param_table::pack_fdtd(self),
            terrain: crate::param_table::pack_terrain(self),
            render_scale: self.render_scale.value(),
            render_auto: self.render_auto.value() as u32,
            stars: crate::param_table::pack_stars(self),
            particles: crate::param_table::pack_particles(self),
            fluid: crate::param_table::pack_fluid(self),
            // Jewel Box (#80).
            ssr: crate::param_table::pack_ssr(self),
            gi: crate::param_table::pack_gi(self),
            glass_spec: crate::param_table::pack_glass_spec(self),
            hdr_vivid: self.hdr_vivid.value(),
            capture: crate::param_table::pack_capture(self),
            overlay: crate::param_table::pack_overlay(self),
            // Runtime-written (process() stamps the live atomic), like hdr_gen.
            overlay_gen: 0,
            axes: crate::param_table::pack_axes(self),
            fx: crate::param_table::pack_fx(self),
            volume: crate::param_table::pack_volume(self),
            temporal: crate::param_table::pack_temporal(self),
            ssgi: crate::param_table::pack_ssgi(self),
            shadow: crate::param_table::pack_shadow(self),
            vxgi: crate::param_table::pack_vxgi(self),
            reflect: crate::param_table::pack_reflect(self),
            refl_probe: crate::param_table::pack_refl_probe(self),
            vxgi_spec: crate::param_table::pack_vxgi_spec(self),
            membrane_fx: crate::param_table::pack_membrane_fx(self),
            finishing: crate::param_table::pack_finishing(self),
            manylight: crate::param_table::pack_manylight(self),
            vecfield: crate::param_table::pack_vecfield(self),
            vecbuild: crate::param_table::pack_vecbuild(self),
            fluidvis: crate::param_table::pack_fluidvis(self),
            fluid2: crate::param_table::pack_fluid2(self),
            liquid: crate::param_table::pack_liquid(self),
            liquid2: crate::param_table::pack_liquid2(self),
            fluidgi: crate::param_table::pack_fluidgi(self),
            caustic: crate::param_table::pack_caustic(self),
            liqmat: crate::param_table::pack_liqmat(self),
            liqmat2: crate::param_table::pack_liqmat2(self),
            rails: crate::param_table::pack_rails(self),
            scenery: crate::param_table::pack_scenery(self),
            terra: crate::param_table::pack_terra(self),
            water: crate::param_table::pack_water(self),
            water2: crate::param_table::pack_water2(self),
            rt: crate::param_table::pack_rt(self),
            refrmat: crate::param_table::pack_refrmat(self),
            rt2: crate::param_table::pack_rt2(self),
            rt3: crate::param_table::pack_rt3(self),
            rt4: crate::param_table::pack_rt4(self),
            neural: crate::param_table::pack_neural(self),
            aniso: crate::param_table::pack_aniso(self),
            coat: crate::param_table::pack_coat(self),
            neural2: crate::param_table::pack_neural2(self),
            neural3: crate::param_table::pack_neural3(self),
            body: crate::param_table::pack_body(self),
            micro: crate::param_table::pack_micro(self),
            pathtrace_on: self.pathtrace_enable.value() as u32,
            ndenoise: crate::param_table::pack_ndenoise(self),
            emit: crate::param_table::pack_emit(self),
            ssrefr: crate::param_table::pack_ssrefr(self),
            upscale: crate::param_table::pack_upscale(self),
            restir: crate::param_table::pack_restir(self),
            neural_net: crate::param_table::pack_neural_net(self),
            neural_edge: crate::param_table::pack_neural_edge(self),
            maxenergy: crate::param_table::pack_maxenergy(self),
            neural_net2: crate::param_table::pack_neural_net2(self),
            nn_gen: 0, // overwritten by lib.rs process() from the Arc<AtomicU32>
            creature_gen: 0, // #476 T2b: overwritten by process() from the Arc<AtomicU32>
            neural_mlp: crate::param_table::pack_neural_mlp(self),
            neural_attn: crate::param_table::pack_neural_attn(self),
            tube: crate::param_table::pack_tube(self),
            neural_surface: crate::param_table::pack_neural_surface(self),
            neural_surface2: crate::param_table::pack_neural_surface2(self),
            brain: crate::param_table::pack_brain(self),
            thinfilm: crate::param_table::pack_thinfilm(self),
            // Path-tracer dielectric BTDF (#258 T2): [enable, absorption, _, _].
            ptglass: [
                self.pt_dielectric.value() as u32 as f32,
                self.pt_absorb.value(),
                self.pt_composite.value().to_u32() as f32, // [2] composite mode (0 = Replace)
                self.pt_augment.value(),                   // [3] augment amount (0 = raster untouched)
            ],
            tube_profile: self.tube_profile.value(),
            lens: crate::param_table::pack_lens(self),
            spectral: [
                self.spectral_enable.value() as u32 as f32,
                self.spectral_abbe.value(),
                self.spectral_secondaries.value() as f32,
                0.0,
            ],
            demo: crate::param_table::pack_demo(self),
            audiodip: crate::param_table::pack_audiodip(self),
            audiodip2: crate::param_table::pack_audiodip2(self),
            mxforce: crate::param_table::pack_mxforce(self),
            mxforce2: crate::param_table::pack_mxforce2(self),
            mxforce3: crate::param_table::pack_mxforce3(self),
            pbeads: crate::param_table::pack_pbeads(self),
            matcol: crate::param_table::pack_matcol(self),
            pbeads2: crate::param_table::pack_pbeads2(self),
            // Photon-mapped caustics (#258 T5): [enable, photons_k, intensity, radius].
            ptcaustic: [
                self.pt_caustics.value() as u32 as f32,
                self.pt_caustic_photons.value() as f32,
                self.pt_caustic_intensity.value(),
                self.pt_caustic_radius.value(),
            ],
            skyrefl: crate::param_table::pack_skyrefl(self),
            nrc: crate::param_table::pack_nrc(self),
            nrc2: crate::param_table::pack_nrc2(self),
            nrc3: crate::param_table::pack_nrc3(self),
            nrc4: crate::param_table::pack_nrc4(self),
            // #541 S2 T1 mindview spine: a RESERVATION. There is no compositor to
            // write it and no param behind it, so the packer emits zeros — Single
            // grid, pane 0 showing the scene the visual already draws. When WS-A
            // lands, the pane arrangement is runtime-stamped in `lib.rs process()`
            // (the `mind[8]`/`atlas[8]` pattern), not packed from params.
            mindview: [0.0; 8],
            mindview_pane: [0.0; crate::ipc::MINDVIEW_PANES * crate::ipc::MINDVIEW_PANE_SLOTS],
            mindview_gen: 0,
        }
    }
}

#[cfg(test)]
mod host_mirror_tests {
    use super::*;

    /// Relocated from `math.rs` by #626 T3 PR B. It tests `GeneratorMode`'s
    /// `from_u32`/`to_u32` — i.e. THIS file's `enum_u32_via_index!` machinery — and had
    /// been sitting in `math.rs`'s test module, which is the only reason `math.rs`
    /// appeared to depend on `GeneratorMode` at all. Moving it here is what let the
    /// 27-variant enum stay put instead of being duplicated into `organon-core`.
    #[test]
    fn generator_mode_roundtrips() {
        // Every known generator id survives a to_u32 → from_u32 round-trip, and
        // unknown ids fall back to Original (so a stale preset/IPC never panics).
        for g in [GeneratorMode::Original] {
            assert_eq!(GeneratorMode::from_u32(g.to_u32()), g);
        }
        assert_eq!(GeneratorMode::from_u32(9999), GeneratorMode::Original);
    }

    /// **The pin.** `HostFuncName` (nih-plug's, for `EnumParam`) and
    /// `organon_core::params::FuncName` (the algorithm's) are two declarations of one
    /// list, and the index is the wire format shared by `Shared`, presets and
    /// automation. If they drift, nothing fails loudly — a preset simply recalls the
    /// wrong waveform.
    ///
    /// Compared **element-wise by name, in both directions**, deliberately. A length
    /// check would pass a same-length *reordering*, which is the failure that actually
    /// corrupts saved state; and comparing only one direction would miss a variant
    /// added to core alone.
    #[test]
    fn host_func_name_mirrors_core() {
        let host = HostFuncName::variants();
        let core = FuncName::ALL;

        assert_eq!(
            host.len(),
            core.len(),
            "HostFuncName has {} variants, core::FuncName has {} — add to BOTH, at the tail",
            host.len(),
            core.len(),
        );

        for (i, (h, c)) in host.iter().zip(core.iter()).enumerate() {
            assert_eq!(
                *h,
                c.as_str(),
                "index {i}: host says {h:?}, core says {:?} — the lists have DRIFTED, \
                 and index is the wire format",
                c.as_str(),
            );
        }

        // Both directions: every core variant round-trips through the shared index.
        for (i, c) in core.iter().enumerate() {
            assert_eq!(c.to_u32(), i as u32, "core {c:?} is not at its declared index");
            assert_eq!(
                HostFuncName::from_u32(i as u32).to_u32(),
                i as u32,
                "host index {i} does not round-trip",
            );
        }
    }

    /// The host adapter and core must agree on the u32 wire value for every variant —
    /// this is what `to_shared` writes and what the visual reads back.
    #[test]
    fn host_and_core_agree_on_wire_values() {
        for (i, c) in FuncName::ALL.iter().enumerate() {
            let h = HostFuncName::from_u32(i as u32);
            assert_eq!(h.to_u32(), c.to_u32(), "wire value differs at index {i}");
        }
    }
}
