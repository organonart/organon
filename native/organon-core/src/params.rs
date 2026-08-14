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
//! ⚠️ **Tier 2 added four more, and for a different consumer** (organon#49 T2):
//! `SurfaceMode`, `MaterialType`, `CamPath` and `Palette`. Not because `world.rs` names
//! them, but because `cli.rs` and `agent.rs` do — and those two are *also* on the path
//! (`world.rs` imports `agent`; `shell_main.rs` imports both), so they have to lose
//! nih-plug too. Tier 2's scope was set by that transitive fact rather than by the
//! original list: the CLI's three selectors are generator/surface/material, and the
//! agent's feature fingerprint adds palette and camera path. That is the whole set.
//!
//! Eight semantic enums now live here, and [`IndexedEnum`] is the vocabulary that lets a
//! caller list, name and index them without a plugin host.
//!
//! **The same shape of correction already applies to `ipc.rs`, one section up.** This
//! note used to add that `ipc.rs` likewise stays, because #536 T4 reference #3
//! (`math.rs` → `crate::ipc::Shared`) was *also* test-only and co-location bought
//! nothing — one relocated test resolved it. That reasoning was sound and `ipc.rs` moved
//! anyway, in Tier 4, when `organon-render` needed `Shared` from a crate it could depend
//! on. Twice now, "no caller needs this yet" has been true and then stopped being true;
//! read these notes as *records of a moment*, not as standing prohibitions.

use glam::Vec3;

/// **Core's counterpart to nih-plug's `Enum`** (organon#49 Tier 2).
///
/// Tier 1 moved the semantic enums here but left `cli.rs` and `agent.rs` reaching for
/// `nih_plug::prelude::Enum` to do three things: list the variant names, look one up by
/// index, and count them. None of that is a plugin-host concern — it is *"this enum has
/// an ordered set of variants with display names"*, which is exactly what the wire format
/// already requires of these types. The nih-plug trait was standing in for a vocabulary
/// core is better placed to own.
///
/// That matters because `cli.rs` and `agent.rs` are both on `world.rs`'s dependency path
/// (`world.rs` imports `agent`, and `shell_main.rs` imports both), so they have to become
/// nih-plug-free for Tier 4 to move `world.rs` below the plugin crate.
///
/// ⚠️ **The index is the wire format** — see [`FuncName`]. `all()` is in declaration
/// order, and `index()`/`from_index()` agree with each type's inherent `to_u32`/`from_u32`
/// by construction. The `Host*` mirrors in `params.rs` are pinned to these lists
/// element-wise by name; that pinning is what lets a caller use either side and get the
/// same answer.
///
/// **Method names deliberately do not collide with the inherent ones.** Each type already
/// has `as_str`/`to_u32`/`from_u32`, and a trait method of the same name would be shadowed
/// by the inherent one at every call site — silently, and differently depending on whether
/// the trait was in scope. `all`/`label`/`index`/`from_index` cannot be confused for them.
pub trait IndexedEnum: Copy + PartialEq + 'static {
    /// Every variant, in declaration order = index order = the wire format.
    fn all() -> &'static [Self];

    /// This variant's display name — equal to the host mirror's `#[name = "…"]`.
    fn label(self) -> &'static str;

    /// The variant at `i`, falling back to the type's documented default when out of
    /// range (never a panic: these indices arrive from `Shared`, from presets, and from
    /// a human typing at a CLI).
    fn from_index(i: u32) -> Self;

    /// Position in [`IndexedEnum::all`]. Defaulted rather than required — it is a search
    /// of a list that is at most 27 long, in CLI and doc paths only, and a hand-written
    /// override would be a second place for the index to drift.
    fn index(self) -> u32 {
        Self::all().iter().position(|v| *v == self).unwrap_or(0) as u32
    }

    /// Display names in index order — the shape nih-plug's `variants()` returned, which
    /// is what the CLI's resolver, the `organon docs` generator and the agent's
    /// capability catalog all consume.
    ///
    /// Allocates, and that is fine: every caller is parsing a command line, building a
    /// prompt, or writing a markdown file. Deriving it from `all()` keeps one source of
    /// truth instead of a `NAMES` constant that could drift from `label()`.
    fn labels() -> Vec<&'static str> {
        Self::all().iter().map(|v| v.label()).collect()
    }
}

/// Implements [`IndexedEnum`] by delegating to the inherent `ALL` / `as_str` / `from_u32`
/// each of these types already has. Six near-identical impls is exactly the kind of
/// repetition that rots one line at a time.
macro_rules! impl_indexed_enum {
    ($($t:ty),* $(,)?) => { $(
        impl IndexedEnum for $t {
            fn all() -> &'static [Self] { &Self::ALL }
            fn label(self) -> &'static str { self.as_str() }
            fn from_index(i: u32) -> Self { Self::from_u32(i) }
        }
    )* };
}

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

/// How each node is turned into geometry (organon#49 T2).
///
/// `Original` = an independent cube per node. `FlowAligned` orients each cube toward its
/// successor so consecutive nodes connect into ribbons; `SweptTubes` renders those
/// bridges as cylinders. `Metaball`/`Volume` bake the node set into one scalar field and
/// raymarch it as a skin or a participating medium; `Membrane` lofts a sheet between
/// strands; `Voxel` is the grid-snapped DDA path; `NeuralTissue` builds closed anatomical
/// primitives; `Splat` draws anisotropic 3-D Gaussians; `Plexus` wires nearest neighbours
/// into a field web.
///
/// The per-variant prose stays on `params.rs`'s `HostSurfaceMode` — those strings are the
/// DAW's dropdown, which is a host concern.
///
/// ⚠️ Append-only; the index is the wire format. `Plexus` took ordinal 9 because `Splat`
/// took 8 on the main merge.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum SurfaceMode {
    Original,
    FlowAligned,
    SweptTubes,
    Metaball,
    Membrane,
    Voxel,
    Volume,
    NeuralTissue,
    Splat,
    Plexus,
}

impl SurfaceMode {
    pub const ALL: [SurfaceMode; 10] = [
        SurfaceMode::Original,
        SurfaceMode::FlowAligned,
        SurfaceMode::SweptTubes,
        SurfaceMode::Metaball,
        SurfaceMode::Membrane,
        SurfaceMode::Voxel,
        SurfaceMode::Volume,
        SurfaceMode::NeuralTissue,
        SurfaceMode::Splat,
        SurfaceMode::Plexus,
    ];

    /// Matches the host mirror's `#[name = "…"]`; pinned by a test in `params.rs`.
    pub fn as_str(self) -> &'static str {
        match self {
            SurfaceMode::Original => "Original",
            SurfaceMode::FlowAligned => "Flow-Aligned",
            SurfaceMode::SweptTubes => "Swept Tubes",
            SurfaceMode::Metaball => "Metaball",
            SurfaceMode::Membrane => "Membrane",
            SurfaceMode::Voxel => "Voxel",
            SurfaceMode::Volume => "Volume",
            SurfaceMode::NeuralTissue => "Neural Tissue",
            SurfaceMode::Splat => "Splat",
            SurfaceMode::Plexus => "Plexus",
        }
    }

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

/// How a surface shades (organon#49 T2) — the PBR lobe selector.
///
/// ⚠️ Append-only; the index is the wire format, and `cube.wgsl` branches on it. The
/// per-variant prose stays on `params.rs`'s `HostMaterialType`.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum MaterialType {
    Standard,
    Chrome,
    Glass,
    Refractive,
    Anisotropic,
    Clearcoat,
    Velvet,
    Subsurface,
}

impl MaterialType {
    pub const ALL: [MaterialType; 8] = [
        MaterialType::Standard,
        MaterialType::Chrome,
        MaterialType::Glass,
        MaterialType::Refractive,
        MaterialType::Anisotropic,
        MaterialType::Clearcoat,
        MaterialType::Velvet,
        MaterialType::Subsurface,
    ];

    /// Matches the host mirror's `#[name = "…"]`; pinned by a test in `params.rs`.
    pub fn as_str(self) -> &'static str {
        match self {
            MaterialType::Standard => "Standard",
            MaterialType::Chrome => "Chrome",
            MaterialType::Glass => "Glass",
            MaterialType::Refractive => "Refractive",
            MaterialType::Anisotropic => "Anisotropic",
            MaterialType::Clearcoat => "Clearcoat",
            MaterialType::Velvet => "Velvet",
            MaterialType::Subsurface => "Subsurface",
        }
    }

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

/// Auto-orbit camera path presets (organon#49 T2). `Off` leaves the camera fully manual.
///
/// ⚠️ Append-only; the index is the wire format. Ordinals 5–10 are #307 Tier 2's
/// cinematic moves, appended after the original five.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum CamPath {
    Off,
    HCircle,
    VCircle,
    Figure8,
    Spiral,
    Boom,
    Pendulum,
    Truck,
    PushPull,
    PolarOver,
    Drift,
}

impl CamPath {
    pub const ALL: [CamPath; 11] = [
        CamPath::Off,
        CamPath::HCircle,
        CamPath::VCircle,
        CamPath::Figure8,
        CamPath::Spiral,
        CamPath::Boom,
        CamPath::Pendulum,
        CamPath::Truck,
        CamPath::PushPull,
        CamPath::PolarOver,
        CamPath::Drift,
    ];

    /// Matches the host mirror's `#[name = "…"]`; pinned by a test in `params.rs`.
    pub fn as_str(self) -> &'static str {
        match self {
            CamPath::Off => "Off",
            CamPath::HCircle => "Horizontal Circle",
            CamPath::VCircle => "Vertical Circle",
            CamPath::Figure8 => "Figure Eight",
            CamPath::Spiral => "Spiral",
            CamPath::Boom => "Boom (Crane)",
            CamPath::Pendulum => "Pendulum",
            CamPath::Truck => "Truck (Lateral)",
            CamPath::PushPull => "Push / Pull",
            CamPath::PolarOver => "Over the Top",
            CamPath::Drift => "Handheld Drift",
        }
    }

    pub fn to_u32(self) -> u32 {
        self as u32
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

/// The colour LUT applied across every surface mode (organon#49 T2).
///
/// `Native` keeps the RGB-cube / HSV colouring; every other variant replaces it. The
/// non-`Native`, non-`Spectrum` palettes are Inigo-Quilez cosine gradients; `Spectrum`
/// keeps the exact HSV wheel.
///
/// ⚠️ Append-only; the index is the wire format. `Native` is ordinal 0 **and** the
/// out-of-range fallback, which is what makes an unknown palette degrade to today's look
/// rather than to an arbitrary gradient.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Palette {
    Native,
    Spectrum,
    CoralReef,
    DeepSea,
    Anemone,
    Jellyfish,
    Nautilus,
    Kelp,
    Bioluminescence,
    Flesh,
    Candy,
    Plasma,
    Neon,
}

impl Palette {
    pub const ALL: [Palette; 13] = [
        Palette::Native,
        Palette::Spectrum,
        Palette::CoralReef,
        Palette::DeepSea,
        Palette::Anemone,
        Palette::Jellyfish,
        Palette::Nautilus,
        Palette::Kelp,
        Palette::Bioluminescence,
        Palette::Flesh,
        Palette::Candy,
        Palette::Plasma,
        Palette::Neon,
    ];

    /// Matches the host mirror's `#[name = "…"]`; pinned by a test in `params.rs`.
    pub fn as_str(self) -> &'static str {
        match self {
            Palette::Native => "Native (HSV / RGB)",
            Palette::Spectrum => "Spectrum (HSV)",
            Palette::CoralReef => "Coral Reef",
            Palette::DeepSea => "Deep Sea",
            Palette::Anemone => "Anemone",
            Palette::Jellyfish => "Jellyfish",
            Palette::Nautilus => "Nautilus",
            Palette::Kelp => "Kelp",
            Palette::Bioluminescence => "Bioluminescence",
            Palette::Flesh => "Flesh",
            Palette::Candy => "Candy",
            Palette::Plasma => "Plasma",
            Palette::Neon => "Neon",
        }
    }

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

// ── organon#49 T4a: the last six enums `world.rs` reaches for through `params` ──────
//
// With these here, nothing `world.rs` names in `crate::params` requires nih-plug — which
// is the precondition for `world.rs` itself moving below the plugin crate. Same split as
// T1/T2, same `Host*` mirrors, same element-wise pins. Every one is small (2–4 variants);
// the whole wave is 18 variants against `GeneratorMode`'s 27 alone.

/// FDTD Maxwell solver (#412 T3) source waveform. `Pulse` is a one-shot Gaussian wavelet
/// (the "watch it launch and travel" transient); `Cw` is a continuous sinusoid at the
/// source frequency (steady radiation / resonance).
///
/// ⚠️ Append-only; the index is the wire format.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum FdtdSource {
    Pulse,
    Cw,
}

impl FdtdSource {
    pub const ALL: [FdtdSource; 2] = [FdtdSource::Pulse, FdtdSource::Cw];
    pub fn as_str(self) -> &'static str {
        match self {
            FdtdSource::Pulse => "Pulse",
            FdtdSource::Cw => "CW (continuous)",
        }
    }
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

/// Field Volume density source (#348) — how `SurfaceMode::Volume` bakes its density
/// field. `Legacy` is the node point-set metaball bake and the default, so it stays
/// byte-identical; the others render density instead of nodes.
///
/// ⚠️ Append-only; packed into `Shared.fieldvol[0]`.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum FieldVolSource {
    Legacy,
    Auto,
    FieldBaked,
    SmoothedNode,
}

impl FieldVolSource {
    pub const ALL: [FieldVolSource; 4] = [
        FieldVolSource::Legacy,
        FieldVolSource::Auto,
        FieldVolSource::FieldBaked,
        FieldVolSource::SmoothedNode,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            FieldVolSource::Legacy => "Legacy (node metaball)",
            FieldVolSource::Auto => "Auto (field / smoothed)",
            FieldVolSource::FieldBaked => "Field-baked (energy)",
            FieldVolSource::SmoothedNode => "Smoothed node",
        }
    }
    pub fn to_u32(self) -> u32 {
        self as u32
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

/// Calibrated-colour mode (#349). `Aesthetic` is today's tint (HSV / palette / RGB-cube)
/// and the default, so it stays byte-identical; `Calibrated` is colour that means a
/// measured dB level, sampled from a legend-backed perceptual LUT.
///
/// ⚠️ Append-only; packed into `Shared.colour[0]`.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum ColourMode {
    Aesthetic,
    Calibrated,
}

impl ColourMode {
    pub const ALL: [ColourMode; 2] = [ColourMode::Aesthetic, ColourMode::Calibrated];
    pub fn as_str(self) -> &'static str {
        match self {
            ColourMode::Aesthetic => "Aesthetic",
            ColourMode::Calibrated => "Calibrated (dB)",
        }
    }
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> ColourMode {
        match v {
            1 => ColourMode::Calibrated,
            _ => ColourMode::Aesthetic,
        }
    }
}

/// What "measured level" the calibrated colour reads (#349). `Auto` sends field
/// generators (Maxwell / Acoustic) to their band's dBFS and everything else to momentary
/// LUFS.
///
/// ⚠️ Append-only; packed into `Shared.colour[4]`.
///
/// 📌 `Auto` carries **no `#[name]`** on the host mirror, so nih-plug derives its display
/// string rather than reading one. That is the one variant in this wave whose name is not
/// written down anywhere, and `host_cal_colour_source_mirrors_core` is what establishes
/// what it actually is — if this string is wrong, that test fails rather than the UI
/// quietly disagreeing with the CLI.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum CalColourSource {
    Auto,
    Band,
    Lufs,
}

impl CalColourSource {
    pub const ALL: [CalColourSource; 3] = [
        CalColourSource::Auto,
        CalColourSource::Band,
        CalColourSource::Lufs,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            CalColourSource::Auto => "Auto",
            CalColourSource::Band => "Band (dBFS)",
            CalColourSource::Lufs => "Loudness (LUFS)",
        }
    }
    pub fn to_u32(self) -> u32 {
        self as u32
    }
    pub fn from_u32(v: u32) -> CalColourSource {
        match v {
            1 => CalColourSource::Band,
            2 => CalColourSource::Lufs,
            _ => CalColourSource::Auto,
        }
    }
}

/// Field Engine (#381 T1) render-kind selector. `Auto` infers the kind by probing the
/// compiled program; the explicit variants override it.
///
/// ⚠️ Append-only; rides `Shared.field[0]`.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum FieldKind {
    Auto,
    Scalar,
    Vector,
    Complex,
}

impl FieldKind {
    pub const ALL: [FieldKind; 4] = [
        FieldKind::Auto,
        FieldKind::Scalar,
        FieldKind::Vector,
        FieldKind::Complex,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            FieldKind::Auto => "Auto (infer)",
            FieldKind::Scalar => "Scalar (density)",
            FieldKind::Vector => "Vector (lines + aura)",
            FieldKind::Complex => "Complex (|psi|^2 + phase)",
        }
    }
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

/// Orientation of the #391 T1 Poynting-flux measurement patch — the surface the
/// instrument integrates energy flux through.
///
/// ⚠️ Append-only; packs into `Shared.instrument[13]`.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum FluxAxis {
    X,
    Y,
    Z,
    Radial,
}

impl FluxAxis {
    pub const ALL: [FluxAxis; 4] = [FluxAxis::X, FluxAxis::Y, FluxAxis::Z, FluxAxis::Radial];
    pub fn as_str(self) -> &'static str {
        match self {
            FluxAxis::X => "X",
            FluxAxis::Y => "Y",
            FluxAxis::Z => "Z",
            FluxAxis::Radial => "Radial",
        }
    }
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
    ///
    /// ⚠️ This travels with the semantic type rather than staying on the host mirror,
    /// because `world.rs` calls it (`axis.normal(center)`, twice) and `world.rs` is what
    /// organon#49 Tier 4 moves below the plugin crate. It is pure glam — no host in it.
    pub fn normal(self, center: Vec3) -> Vec3 {
        match self {
            FluxAxis::X => Vec3::X,
            FluxAxis::Y => Vec3::Y,
            FluxAxis::Z => Vec3::Z,
            FluxAxis::Radial => {
                let n = center.normalize_or_zero();
                if n == Vec3::ZERO {
                    Vec3::X
                } else {
                    n
                }
            }
        }
    }
}

impl_indexed_enum!(
    FuncName,
    GeneratorMode,
    BoidsForm,
    OscDivision,
    SurfaceMode,
    MaterialType,
    CamPath,
    Palette,
    FdtdSource,
    FieldVolSource,
    ColourMode,
    CalColourSource,
    FieldKind,
    FluxAxis,
);

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

    // ── organon#49 T2 ────────────────────────────────────────────────────────────

    #[test]
    fn surface_and_material_indices_round_trip() {
        for (i, s) in SurfaceMode::ALL.iter().enumerate() {
            assert_eq!(s.to_u32(), i as u32, "{s:?} is not at its declared index");
            assert_eq!(SurfaceMode::from_u32(i as u32), *s);
        }
        for (i, m) in MaterialType::ALL.iter().enumerate() {
            assert_eq!(m.to_u32(), i as u32, "{m:?} is not at its declared index");
            assert_eq!(MaterialType::from_u32(i as u32), *m);
        }
        assert_eq!(SurfaceMode::from_u32(9999), SurfaceMode::Original);
        assert_eq!(MaterialType::from_u32(9999), MaterialType::Standard);
    }

    /// `Splat` = 8 and `Plexus` = 9 by merge order, asserted by name so a "tidier"
    /// reordering of `ALL` still fails.
    #[test]
    fn surface_mode_merge_order_ordinals_are_pinned() {
        assert_eq!(SurfaceMode::Splat.to_u32(), 8);
        assert_eq!(SurfaceMode::Plexus.to_u32(), 9);
    }

    /// The trait must agree with the inherent methods for **every** implementor —
    /// `index()` is a default that searches `all()`, so this is what catches an `ALL`
    /// that disagrees with `to_u32`.
    #[test]
    fn indexed_enum_agrees_with_the_inherent_methods() {
        fn check<T: IndexedEnum + std::fmt::Debug>(inherent: &[(T, u32, &'static str)]) {
            assert_eq!(T::all().len(), inherent.len(), "all() length");
            assert_eq!(T::labels().len(), inherent.len(), "labels() length");
            for (v, wire, name) in inherent {
                assert_eq!(v.index(), *wire, "{v:?}: index() vs to_u32()");
                assert_eq!(v.label(), *name, "{v:?}: label() vs as_str()");
                assert_eq!(T::from_index(*wire), *v, "{v:?}: from_index() vs from_u32()");
            }
        }
        check(&FuncName::ALL.map(|v| (v, v.to_u32(), v.as_str())));
        check(&GeneratorMode::ALL.map(|v| (v, v.to_u32(), v.as_str())));
        check(&BoidsForm::ALL.map(|v| (v, v.to_u32(), v.as_str())));
        check(&OscDivision::ALL.map(|v| (v, v.to_u32(), v.as_str())));
        check(&SurfaceMode::ALL.map(|v| (v, v.to_u32(), v.as_str())));
        check(&MaterialType::ALL.map(|v| (v, v.to_u32(), v.as_str())));
        check(&CamPath::ALL.map(|v| (v, v.to_u32(), v.as_str())));
        check(&Palette::ALL.map(|v| (v, v.to_u32(), v.as_str())));
    }

    /// Display names must be unique per type, because the CLI resolves a selector by
    /// **unambiguous case-insensitive substring** — two equal names would make one
    /// unreachable, and the ambiguity error would name the same string twice.
    #[test]
    fn labels_are_unique_within_each_type() {
        fn check<T: IndexedEnum>(what: &str) {
            let names = T::labels();
            for (i, a) in names.iter().enumerate() {
                for b in names.iter().skip(i + 1) {
                    assert_ne!(
                        a.to_lowercase(),
                        b.to_lowercase(),
                        "{what}: duplicate display name {a:?}",
                    );
                }
            }
        }
        check::<FuncName>("FuncName");
        check::<GeneratorMode>("GeneratorMode");
        check::<BoidsForm>("BoidsForm");
        check::<OscDivision>("OscDivision");
        check::<SurfaceMode>("SurfaceMode");
        check::<MaterialType>("MaterialType");
        check::<CamPath>("CamPath");
        check::<Palette>("Palette");
    }

    #[test]
    fn osc_division_beats_scale_with_the_bar() {
        assert_eq!(OscDivision::Quarter.beats(4.0), 1.0);
        assert_eq!(OscDivision::Bar.beats(4.0), 4.0);
        assert_eq!(OscDivision::Bar.beats(3.0), 3.0, "3/4 time");
        assert_eq!(OscDivision::TwoBar.beats(3.0), 6.0);
    }
}
