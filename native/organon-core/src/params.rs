//! The two param types the **algorithm** needs — and nothing else from `params.rs`.
//!
//! #626 Tier 3 PR B, resolving #536 T4 **reference #2**: `math.rs` →
//! `crate::params::{FuncName, ParamValues}`, which #536 called *"the one real design
//! decision"* in the whole crate split.
//!
//! ## Why this module is named `params`
//!
//! So that `math.rs`'s existing `use crate::params::{FuncName, ParamValues};` resolves
//! **verbatim** once `math.rs` lives here. A 31k-line file moving between crates is
//! already the riskiest edit in the tier; not also rewriting its imports is worth more
//! than a tidier module name.
//!
//! ## The decision, and the rule that forced it
//!
//! #536 recommended core own these enums behind an optional `host-params` feature that
//! adds nih-plug's `#[derive(Enum)]`, with a caveat about `wasm32` resolution. **That
//! caveat is dead** — #418 is parked, there is no wasm32 target — so the choice is made
//! on native merits, and the merits favour the other option:
//!
//! > **nih-plug must stay out of core *unconditionally*.** With `host-params`,
//! > `cargo tree -p organon-core` would be clean only for the default feature set —
//! > the tier's acceptance test would become a statement about one configuration
//! > rather than about the crate. An optional dependency is still a dependency.
//!
//! So [`FuncName`] is declared **plain** here, and `params.rs` keeps a host-facing
//! mirror carrying the derive. **The orphan rule is what makes this unavoidable rather
//! than merely preferable:** `organic-math-native` cannot
//! `impl nih_plug::Enum for organon_core::FuncName`, because both the trait and the
//! type would be foreign to it. There is no third option where one type serves both
//! sides — either core takes the derive, or the host side owns a mirror.
//!
//! **Core owns the semantic type; the host owns the adapter.** That is the right way
//! round: the duplication lives on the side that has the host concern.
//!
//! ⚠️ The mirror is pinned by a test asserting the two variant lists agree
//! **element-wise, by name, in both directions** — see `params.rs`. A length check
//! would pass a same-length *reordering*, which would silently misindex every saved
//! preset and every automation lane, because the index **is** the wire format.
//!
//! ## What moved later, and why the earlier decision was right at the time
//!
//! ⚠️ **`GeneratorMode`, `BoidsForm` and `OscDivision` live here now** (organon#49 Tier 1).
//! This section used to say `GeneratorMode` deliberately stayed in `params.rs`, and that
//! was correct *on the reason it gave*: `math.rs`'s only non-comment use of it was a test
//! of `from_u32`/`to_u32` round-tripping, so relocating the test resolved the reference and
//! the enum never had to move.
//!
//! **A different consumer changed the answer.** `world.rs` imports
//! `params::{BoidsForm, FuncName, GeneratorMode, OscDivision, ParamValues}`, and `world.rs`
//! has to become reachable from a crate that does not carry `nih_plug` — otherwise
//! `shell_main.rs` (and therefore the whole Organon Console binary) stays inside
//! `organic-math-native` and inherits GPL from a VST3 binding it never calls. Two of those
//! five already resolved here; these three are the remainder. The reason the old note gave
//! was about `math.rs`, and it is still true about `math.rs` — it simply was never the only
//! caller that mattered.
//!
//! Each keeps its host-facing mirror in `params.rs` (`HostGeneratorMode`, `HostBoidsForm`,
//! `HostOscDivision`) for exactly the orphan-rule reason `HostFuncName` exists, and each
//! pair is pinned element-wise by a test there.
//!
//! **The same shape of correction already applies to `ipc.rs`, one section up.** This
//! note used to add that `ipc.rs` likewise stays, because #536 T4 reference #3
//! (`math.rs` → `crate::ipc::Shared`) was *also* test-only and co-location bought
//! nothing — one relocated test resolved it. That reasoning was sound and `ipc.rs` moved
//! anyway, in Tier 4, when `organon-render` needed `Shared` from a crate it could depend
//! on. Twice now, "no caller needs this yet" has been true and then stopped being true;
//! read these notes as *records of a moment*, not as standing prohibitions.

use glam::Vec3;

/// A waveshaper applied to a phasor — a synth oscillator's wave family.
///
/// `Sin`/`Cos` are the smooth defaults; `Tan`/`Log` are exotic curves;
/// `Triangle`/`Square`/`Saw` are the classic synth shapes. All map phase → value and
/// slot into [`crate::math::apply_func`] wherever a function is used (translation
/// deformation + pendulum rotation).
///
/// ⚠️ **Variants are append-only and their ORDER IS THE WIRE FORMAT.** The index is
/// what `to_u32`/`from_u32` write into `Shared` and what presets store, so inserting or
/// reordering a variant silently repoints every saved preset and automation lane at a
/// different waveform. Append at the tail.
///
/// #626 T3: declared plain here (no `#[derive(Enum)]`) so that nih-plug stays out of
/// `organon-core` unconditionally. `params.rs`'s `HostFuncName` is the host-facing
/// mirror; a test there pins the two lists together.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum FuncName {
    Sin,
    Cos,
    Tan,
    Log,
    Triangle,
    Square,
    Saw,
}

impl FuncName {
    /// Every variant, **in declaration order** — which is index order, which is the
    /// wire format. This is the list `params.rs`'s pinning test compares against.
    pub const ALL: [FuncName; 7] = [
        FuncName::Sin,
        FuncName::Cos,
        FuncName::Tan,
        FuncName::Log,
        FuncName::Triangle,
        FuncName::Square,
        FuncName::Saw,
    ];

    /// The stable name for this variant. Matches the `#[name = "…"]` the host mirror
    /// carries, and the pinning test asserts they agree — so this is not a second
    /// source of truth, it is the thing the two are checked against.
    pub fn as_str(self) -> &'static str {
        match self {
            FuncName::Sin => "sin",
            FuncName::Cos => "cos",
            FuncName::Tan => "tan",
            FuncName::Log => "log",
            FuncName::Triangle => "triangle",
            FuncName::Square => "square",
            FuncName::Saw => "saw",
        }
    }

    pub fn to_u32(self) -> u32 {
        match self {
            FuncName::Sin => 0,
            FuncName::Cos => 1,
            FuncName::Tan => 2,
            FuncName::Log => 3,
            FuncName::Triangle => 4,
            FuncName::Square => 5,
            FuncName::Saw => 6,
        }
    }

    /// Unknown values fall back to `Sin` rather than panicking — the visual reads these
    /// out of a shared-memory block a differently-versioned writer may have filled.
    pub fn from_u32(v: u32) -> FuncName {
        match v {
            1 => FuncName::Cos,
            2 => FuncName::Tan,
            3 => FuncName::Log,
            4 => FuncName::Triangle,
            5 => FuncName::Square,
            6 => FuncName::Saw,
            _ => FuncName::Sin,
        }
    }
}

/// Which generative algorithm builds the node field.
///
/// The chosen generator only changes *which controls* live in the editor's generator
/// column and *how the node positions are produced*; everything downstream (surface mode,
/// materials, lighting, post) is generator-agnostic.
///
/// ⚠️ **Variants are append-only and their ORDER IS THE WIRE FORMAT** — the same rule as
/// [`FuncName`], and with more saved state behind it. The trailing comments on
/// [`GeneratorMode::from_u32`] record ordinals that were re-seated once and must not move
/// again: `None` holds the retired `Rails` ordinal 17, and `AxonWaveguide` took 18 after
/// it, which is why `NeuralField` sits at 19 rather than 18.
///
/// organon#49 T1: declared plain here (no `#[derive(Enum)]`) so nih-plug stays out of
/// `organon-core` unconditionally. `params.rs`'s `HostGeneratorMode` is the host-facing
/// mirror; a test there pins the two lists together.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum GeneratorMode {
    Original,
    Frenet,
    Dna,
    Attractor,
    Harmonic,
    LSystem,
    CurlNoise,
    Polarization,
    MaxwellField,
    Phyllotaxis,
    Mandelbulb,
    Kaleidoscope,
    Boids,
    Tessellation,
    MinimalSurface,
    Synchrotron,
    VectorField,
    None,
    AxonWaveguide,
    NeuralField,
    NeuralNetwork,
    Lens,
    Demo,
    Acoustic,
    FieldEngine,
    MapAttractor,
    Creature,
}

impl GeneratorMode {
    /// Every variant, **in declaration order** — which is index order, which is the wire
    /// format. This is the list `params.rs`'s pinning test compares against.
    pub const ALL: [GeneratorMode; 27] = [
        GeneratorMode::Original,
        GeneratorMode::Frenet,
        GeneratorMode::Dna,
        GeneratorMode::Attractor,
        GeneratorMode::Harmonic,
        GeneratorMode::LSystem,
        GeneratorMode::CurlNoise,
        GeneratorMode::Polarization,
        GeneratorMode::MaxwellField,
        GeneratorMode::Phyllotaxis,
        GeneratorMode::Mandelbulb,
        GeneratorMode::Kaleidoscope,
        GeneratorMode::Boids,
        GeneratorMode::Tessellation,
        GeneratorMode::MinimalSurface,
        GeneratorMode::Synchrotron,
        GeneratorMode::VectorField,
        GeneratorMode::None,
        GeneratorMode::AxonWaveguide,
        GeneratorMode::NeuralField,
        GeneratorMode::NeuralNetwork,
        GeneratorMode::Lens,
        GeneratorMode::Demo,
        GeneratorMode::Acoustic,
        GeneratorMode::FieldEngine,
        GeneratorMode::MapAttractor,
        GeneratorMode::Creature,
    ];

    /// The stable display name for this variant. Matches the `#[name = "…"]` the host
    /// mirror carries, and the pinning test asserts they agree — so this is not a second
    /// source of truth, it is the thing the two are checked against.
    ///
    /// organon#49 T1: this is the former `params.rs::GeneratorMode::to_label`, renamed to
    /// match [`FuncName::as_str`]. The old name had **zero call sites** — it was a synonym
    /// nobody used — so this is a rename of dead API, not a break.
    ///
    /// ⚠️ `Frenet` carries an EN DASH (U+2013) in "Frenet–Serret", not a hyphen. The
    /// mirror test compares these strings byte-for-byte against the host `#[name]`s.
    pub fn as_str(self) -> &'static str {
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

    pub fn to_u32(self) -> u32 {
        self as u32
    }

    /// Unknown values fall back to `Original` rather than panicking — the visual reads
    /// these out of a shared-memory block a differently-versioned writer may have filled.
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
            21 => GeneratorMode::Lens,        // #258 Tier 3 (raymarched analytic lens SDF)
            22 => GeneratorMode::Demo,        // #288 (scene bench for the RT stack)
            23 => GeneratorMode::Acoustic, // #325 (acoustic Duo-Field: pressure + velocity)
            24 => GeneratorMode::FieldEngine, // #381 Tier 1 (arbitrary closed-form field equations)
            25 => GeneratorMode::MapAttractor, // #380 Tier 1 (discrete density-map attractor)
            26 => GeneratorMode::Creature, // #476 Tier 1 (raymarched SDF sea creatures)
            // Unknown ids fall back to Original.
            _ => GeneratorMode::Original,
        }
    }
}

/// Boids creature form (#52): how each flocking agent is drawn.
///
/// `Surface` keeps the normal surface mode (cubes / tubes / metaball …); every other
/// variant overrides it with a per-agent creature mesh oriented by velocity. The
/// non-`Surface` indices map to `render::creature_mesh` kinds (Fish = 0, …).
///
/// ⚠️ Append-only; the index is the wire format. organon#49 T1: plain here,
/// `HostBoidsForm` mirrors it in `params.rs`.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum BoidsForm {
    Surface,
    Fish,
    Bird,
    Manta,
    Dart,
}

impl BoidsForm {
    /// Declaration order = index order = the wire format.
    pub const ALL: [BoidsForm; 5] = [
        BoidsForm::Surface,
        BoidsForm::Fish,
        BoidsForm::Bird,
        BoidsForm::Manta,
        BoidsForm::Dart,
    ];

    /// Matches the host mirror's `#[name = "…"]`; pinned by a test in `params.rs`.
    pub fn as_str(self) -> &'static str {
        match self {
            BoidsForm::Surface => "Surface (normal)",
            BoidsForm::Fish => "Fish",
            BoidsForm::Bird => "Bird",
            BoidsForm::Manta => "Manta ray",
            BoidsForm::Dart => "Dart",
        }
    }

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
            BoidsForm::Surface => Option::None,
            BoidsForm::Fish => Some(0),
            BoidsForm::Bird => Some(1),
            BoidsForm::Manta => Some(2),
            BoidsForm::Dart => Some(3),
        }
    }
}

/// Musical note-division for the Maxwell dipole's tempo-synced oscillation.
///
/// The field's E vectors swing out and back once per selected division (an LFO period),
/// phase-locked to the beat clock. Sub-beat divisions are fixed fractions of a beat;
/// Bar / 2-Bar scale with the session's beats-per-bar.
///
/// ⚠️ Append-only; the index is the wire format. organon#49 T1: plain here,
/// `HostOscDivision` mirrors it in `params.rs`.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum OscDivision {
    Sixteenth,
    Eighth,
    Quarter,
    Half,
    Bar,
    TwoBar,
}

impl OscDivision {
    /// Declaration order = index order = the wire format.
    pub const ALL: [OscDivision; 6] = [
        OscDivision::Sixteenth,
        OscDivision::Eighth,
        OscDivision::Quarter,
        OscDivision::Half,
        OscDivision::Bar,
        OscDivision::TwoBar,
    ];

    /// Matches the host mirror's `#[name = "…"]`; pinned by a test in `params.rs`.
    pub fn as_str(self) -> &'static str {
        match self {
            OscDivision::Sixteenth => "1/16",
            OscDivision::Eighth => "1/8",
            OscDivision::Quarter => "1/4",
            OscDivision::Half => "1/2",
            OscDivision::Bar => "Bar",
            OscDivision::TwoBar => "2 Bars",
        }
    }

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

    /// ⚠️ Transcribed verbatim from `params.rs`, **including the quirk**: index `2`
    /// (`Quarter`) has no arm of its own and is served by the fallback. That is
    /// behaviour-preserving — `2` and every unknown value both yield `Quarter` — and it
    /// is left as-is rather than "fixed", because the fallback target is the observable
    /// contract and rewriting it would change what an out-of-range index returns.
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

/// The vector-shaped numeric params consumed by [`crate::math::draw_tissue`].
///
/// #626 T3 PR B: moved here from `params.rs` with `FuncName`. It moves *free* — six
/// fields, all `glam::Vec3` or `f32`, with no reference to either param enum — which is
/// why #536 T4 called it out as the easy half of reference #2.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct ParamValues {
    pub loop_count: Vec3,
    pub loop_count_q: f32,
    pub rot_amp: Vec3,
    pub trans_amp: Vec3,
    pub trans_mod: Vec3,
    pub scale_amp: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn func_name_indices_round_trip() {
        for (i, f) in FuncName::ALL.iter().enumerate() {
            assert_eq!(f.to_u32(), i as u32, "{f:?} is not at its declared index");
            assert_eq!(FuncName::from_u32(i as u32), *f);
        }
    }

    #[test]
    fn unknown_func_index_falls_back_to_sin() {
        // The visual may read a Shared written by a differently-versioned plugin.
        assert_eq!(FuncName::from_u32(9999), FuncName::Sin);
        assert_eq!(FuncName::from_u32(7), FuncName::Sin);
    }

    #[test]
    fn func_name_all_is_complete_and_ordered() {
        assert_eq!(FuncName::ALL.len(), 7);
        let names: Vec<_> = FuncName::ALL.iter().map(|f| f.as_str()).collect();
        assert_eq!(names, ["sin", "cos", "tan", "log", "triangle", "square", "saw"]);
    }

    // ── organon#49 T1: the three enums `world.rs` needs ──────────────────────────
    //
    // Each gets the same shape of check `FuncName` has: declared index == wire value,
    // round-trip, and an out-of-range fallback. The cross-crate pin against the host
    // mirror lives in `params.rs`, because that is the side that has nih-plug.

    #[test]
    fn generator_mode_indices_round_trip() {
        for (i, g) in GeneratorMode::ALL.iter().enumerate() {
            assert_eq!(g.to_u32(), i as u32, "{g:?} is not at its declared index");
            assert_eq!(GeneratorMode::from_u32(i as u32), *g);
        }
    }

    /// The ordinals the history re-seated once, asserted by name rather than by
    /// position in `ALL` — so this test still fails if someone reorders the list to
    /// make it "read better". `#187` retired `Rails` and gave 17 to `None`; `Axon`
    /// took 18 on main, which is why `NeuralField` is 19.
    #[test]
    fn generator_mode_reseated_ordinals_are_pinned() {
        assert_eq!(GeneratorMode::None.to_u32(), 17);
        assert_eq!(GeneratorMode::AxonWaveguide.to_u32(), 18);
        assert_eq!(GeneratorMode::NeuralField.to_u32(), 19);
        assert_eq!(GeneratorMode::Creature.to_u32(), 26);
    }

    #[test]
    fn unknown_generator_index_falls_back_to_original() {
        assert_eq!(GeneratorMode::from_u32(9999), GeneratorMode::Original);
        assert_eq!(GeneratorMode::from_u32(27), GeneratorMode::Original);
    }

    #[test]
    fn boids_form_indices_round_trip() {
        for (i, b) in BoidsForm::ALL.iter().enumerate() {
            assert_eq!(b.to_u32(), i as u32, "{b:?} is not at its declared index");
            assert_eq!(BoidsForm::from_u32(i as u32), *b);
        }
        assert_eq!(BoidsForm::from_u32(9999), BoidsForm::Surface);
    }

    /// `creature_kind` is `BoidsForm` − 1, and `Surface` has none. `render.rs`'s
    /// creature-mesh table is indexed by exactly this.
    #[test]
    fn boids_creature_kinds_are_index_minus_one() {
        assert_eq!(BoidsForm::Surface.creature_kind(), Option::None);
        for b in BoidsForm::ALL.iter().skip(1) {
            assert_eq!(b.creature_kind(), Some(b.to_u32() - 1), "{b:?}");
        }
    }

    #[test]
    fn osc_division_indices_round_trip() {
        for (i, d) in OscDivision::ALL.iter().enumerate() {
            assert_eq!(d.to_u32(), i as u32, "{d:?} is not at its declared index");
            assert_eq!(OscDivision::from_u32(i as u32), *d);
        }
    }

    /// The transcribed quirk, asserted so it stays deliberate: index 2 has no arm of
    /// its own, so `Quarter` is both the value at 2 and the out-of-range fallback.
    #[test]
    fn unknown_osc_division_falls_back_to_quarter() {
        assert_eq!(OscDivision::from_u32(2), OscDivision::Quarter);
        assert_eq!(OscDivision::from_u32(9999), OscDivision::Quarter);
    }

    #[test]
    fn osc_division_beats_scale_with_the_bar() {
        assert_eq!(OscDivision::Quarter.beats(4.0), 1.0);
        assert_eq!(OscDivision::Bar.beats(4.0), 4.0);
        assert_eq!(OscDivision::Bar.beats(3.0), 3.0, "3/4 time");
        assert_eq!(OscDivision::TwoBar.beats(3.0), 6.0);
    }
}
