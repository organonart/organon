//! Per-generator overlay metadata (#135 Phase 2) — pure, unit-tested, no GPU.
//!
//! Each generator carries a display **title**, a one-line **description**, an optional
//! pre-rendered **formula** image id, the formula's **symbols** (variable → colour, shared
//! with the formula image), and a **readout** layout: the generator's live **key parameter
//! values** (loop counts, amplifications, curvature, …), read straight from the `Shared`
//! snapshot each frame by a per-generator `eval`. So the panel shows what's actually driving
//! the geometry, not abstract input/output coordinates.
//!
//! All overlay TEXT is ASCII — the glyph atlas (`overlay.rs`) covers printable ASCII only,
//! so Greek/em-dashes/super-scripts would render as `?`. The pretty maths lives in the
//! bundled formula image (rendered by MathJax), not in this drawn text.
//!
//! All 17 generators carry a title, description, a bundled formula image, and a readout
//! card; the flagships (Organic Math, Frenet, DNA, Harmonic, Minimal-surface, Synchrotron)
//! carry a bespoke live eval, the others a key-param readout (#135 P2/P4, #150 P2).

use crate::ipc::Shared;
use crate::params::GeneratorMode;

/// A formula image bundled by the visual (`overlay.rs` maps this to the PNG bytes).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FormulaId {
    Original,
    Frenet,
    Dna,
    Harmonic,
    Minimal,
    Attractor,
    LSystem,
    CurlNoise,
    Polarization,
    Maxwell,
    Phyllotaxis,
    Mandelbulb,
    Kifs,
    Boids,
    Tessellation,
    Synchrotron,
    VectorField,
    Rails,
    Axon,
}

/// A formula variable and its colour, shared with the formula image (informational; the
/// drawn readouts are key params, not formula variables).
#[derive(Clone, Copy, Debug)]
pub struct Symbol {
    pub name: &'static str,
    pub color: [f32; 3],
}

// The shared palette (must match `assets/overlay/gen.mjs`'s `C`).
pub const PINK: [f32; 3] = [0.910, 0.475, 0.753]; // #e879c0  u
pub const PURPLE: [f32; 3] = [0.690, 0.486, 0.941]; // #b07cf0  v
pub const CYAN: [f32; 3] = [0.298, 0.788, 0.941]; // #4cc9f0  theta
pub const GREEN: [f32; 3] = [0.404, 0.910, 0.639]; // #67e8a3  phi
pub const ORANGE: [f32; 3] = [0.965, 0.651, 0.298]; // #f6a64c  kappa
pub const ROSE: [f32; 3] = [0.941, 0.408, 0.561]; // #f0688f  tau

/// Number formatting for a readout (signed, fixed decimals, optional unit).
#[derive(Clone, Copy, Debug)]
pub struct Fmt {
    pub decimals: u8,
    pub signed: bool,
    pub unit: &'static str,
}

impl Fmt {
    pub const fn signed2() -> Fmt {
        Fmt { decimals: 2, signed: true, unit: "" }
    }
    /// Unsigned, 2 decimals — the default for parameter values.
    pub const fn f2() -> Fmt {
        Fmt { decimals: 2, signed: false, unit: "" }
    }
    /// Integer (counts).
    pub const fn int() -> Fmt {
        Fmt { decimals: 0, signed: false, unit: "" }
    }
    /// Format a value: optional leading `+`, fixed decimals, optional unit suffix.
    pub fn apply(&self, v: f32) -> String {
        let v = if v == 0.0 { 0.0 } else { v }; // normalize -0.0 → 0.0
        let mut s = format!("{:.*}", self.decimals as usize, v);
        if self.signed && v >= 0.0 && !s.starts_with('+') {
            s.insert(0, '+');
        }
        if !self.unit.is_empty() {
            s.push(' ');
            s.push_str(self.unit);
        }
        s
    }
}

/// One readout row: a label, the first value slot it shows, how many slots (`span` > 1 →
/// a bracketed vector `[a, b, c]`), an optional symbol-colour index, and its number format.
#[derive(Clone, Copy, Debug)]
pub struct Readout {
    pub label: &'static str,
    pub slot: usize,
    pub span: usize, // 1 = scalar; 3/4 = a [x, y, z(, w)] vector
    pub color: Option<usize>, // index into OverlayMeta.symbols (usually None for params)
    pub fmt: Fmt,
}

/// A grouped column of readouts in the panel.
#[derive(Clone, Copy, Debug)]
pub struct ReadoutGroup {
    pub title: &'static str,
    pub rows: &'static [Readout],
}

/// Live inputs for `eval`: the visual's owned clocks + the full `Shared` snapshot (so a
/// generator can read its real parameter values).
#[derive(Clone, Copy)]
pub struct OverlayCtx<'a> {
    pub gen_phase: f32,
    /// The full (unwrapped) animation clock in **f64** — same value as `gen_phase` but
    /// without the f32 narrowing, for evals that free-run a phase off it and must match
    /// the renderer's f64 math (e.g. the #380 orbit's stopped free-run `φ = gen_phase·rate`).
    pub gen_phase_hi: f64,
    /// The audio-dipole oscillation clock (#248/#325) — advances like `gen_phase`
    /// but pitch-scaled when the audio drive is on. Readouts for fields whose
    /// oscillation rides this clock (Maxwell, Acoustic) must use it, not `gen_phase`.
    pub maxdip_phase: f32,
    pub angle: f32,
    pub beat: f32,
    /// The full (unwrapped) PLL beat-clock accumulator — for evals that lock a
    /// phase to it (e.g. the #380 parameter orbit `phi = beat_pos / loop_beats`).
    /// `beat` above is only its fractional part. Kept in **f64** — it's an unbounded
    /// accumulator, so f32 would quantize the fractional phase away after long playback.
    pub beat_pos: f64,
    pub bpm: f32,
    pub s: &'a Shared,
}

/// The evaluated readout values; `Readout.slot` indexes this.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Values {
    pub v: [f32; 16],
}

impl Values {
    pub fn get(&self, slot: usize) -> f32 {
        self.v.get(slot).copied().unwrap_or(0.0)
    }
}

/// Slot every `eval` fills with the **live animation phase** — a value that ticks at the
/// rate the visualization is actually moving (`gen_phase`, or the spin `angle`), wrapped
/// to `[0, 2π)`. The overlay draws it as a prominent live ticker labelled `live_label`.
pub const LIVE_SLOT: usize = 15;

/// Wrap an accumulating animation phase into `[0, 2π)` for a readable, cycling readout.
pub fn wrap_tau(x: f32) -> f32 {
    x.rem_euclid(std::f32::consts::TAU)
}

/// The full overlay record for a generator.
#[derive(Clone, Copy)]
pub struct OverlayMeta {
    pub title: &'static str,
    pub description: &'static str,
    pub formula: Option<FormulaId>,
    pub symbols: &'static [Symbol],
    pub groups: &'static [ReadoutGroup],
    /// Name of the live animation value (slot `LIVE_SLOT`) — e.g. "spin", "phase", "t".
    pub live_label: &'static str,
    pub eval: for<'a> fn(&OverlayCtx<'a>) -> Values,
}

// ----------------------------------------------------------------------------
// Flagship evaluators — read the generator's real key params from `Shared`.
// ----------------------------------------------------------------------------

fn eval_original(c: &OverlayCtx) -> Values {
    let s = c.s;
    let mut v = [0.0f32; 16];
    // loop counts
    v[0] = s.loop_count[0];
    v[1] = s.loop_count[1];
    v[2] = s.loop_count[2];
    v[3] = s.loop_count[3];
    // rotation amplification (x,y,z)
    v[4] = s.rot_amp[0];
    v[5] = s.rot_amp[1];
    v[6] = s.rot_amp[2];
    // translation amplification (x,y,z)
    v[7] = s.trans_amp[0];
    v[8] = s.trans_amp[1];
    v[9] = s.trans_amp[2];
    // per-axis rotation rate (rot_mod[0..2]) — the speed of each dimension. (The 4th
    // rot_mod slot is the global multiplier inc_scale·10^speed_exp, usually < 0.01, so
    // it reads 0.00 at 2 dp — show the per-axis rates instead.)
    v[10] = s.rot_mod[0];
    v[11] = s.rot_mod[1];
    v[12] = s.rot_mod[2];
    v[LIVE_SLOT] = wrap_tau(c.angle); // live spin (rotation phase)
    Values { v }
}

fn eval_frenet(c: &OverlayCtx) -> Values {
    let f = c.s.frenet;
    let mut v = [0.0f32; 16];
    v[0] = f[3]; // kappa
    v[1] = f[6]; // tau
    v[2] = f[2]; // step
    v[3] = f[0]; // strands
    v[4] = f[1]; // nodes
    v[5] = f[9]; // spread
    v[LIVE_SLOT] = wrap_tau(c.gen_phase);
    Values { v }
}

fn eval_dna(c: &OverlayCtx) -> Values {
    let d = c.s.dna;
    let mut v = [0.0f32; 16];
    v[0] = d[1]; // base pairs
    v[1] = d[2]; // bp per turn
    v[2] = d[3]; // rise
    v[3] = d[4]; // radius
    v[4] = d[5]; // groove
    v[5] = d[11]; // twist breathe
    v[LIVE_SLOT] = wrap_tau(c.angle); // live twist (the helix winds with rotation)
    Values { v }
}

fn eval_harmonic(c: &OverlayCtx) -> Values {
    let h = c.s.harm;
    let mut v = [0.0f32; 16];
    v[0] = h[0]; // mode 0 (l)
    v[1] = h[3]; // mode 1
    v[2] = h[6]; // mode 2
    v[3] = h[1]; // amp 0
    v[4] = h[4]; // amp 1
    v[5] = h[7]; // amp 2
    v[6] = h[9]; // radius
    v[LIVE_SLOT] = wrap_tau(c.gen_phase);
    Values { v }
}

fn eval_minimal(c: &OverlayCtx) -> Values {
    let m = c.s.minimal_surface;
    let mut v = [0.0f32; 16];
    v[0] = m[1]; // scale
    v[1] = m[2]; // cells
    v[2] = m[3]; // iso
    v[3] = m[5]; // twist
    v[4] = m[9]; // bend speed
    v[5] = m[13]; // turns
    v[6] = c.gen_phase; // live bend phase
    v[LIVE_SLOT] = wrap_tau(c.gen_phase); // live bend
    Values { v }
}

/// Copy a single `Shared` block's slots into contiguous readout values, in display order.
/// (Single-block generators; the flagships that mix blocks keep bespoke evals above.)
macro_rules! eval_pick {
    ($name:ident, $block:ident, [$($idx:expr),* $(,)?]) => {
        fn $name(c: &OverlayCtx) -> Values {
            let b = c.s.$block;
            let mut v = [0.0f32; 16];
            let mut i = 0usize;
            $( v[i] = b[$idx]; i += 1; )*
            let _ = i;
            v[LIVE_SLOT] = wrap_tau(c.gen_phase); // live animation clock
            Values { v }
        }
    };
}

eval_pick!(eval_attr, attr, [4, 6, 7, 1, 5, 3]); // dt, speed, scale | seeds, trail, spread
eval_pick!(eval_ls, ls, [1, 2, 3, 4, 5, 6]); // depth, angle, step | sway_amp, sway_freq, grow
eval_pick!(eval_cn, cn, [3, 6, 5, 0, 4, 2]); // scale, flow, dt | seeds, steps, spread
eval_pick!(eval_pol, pol, [4, 5, 9, 0, 1, 3]); // k, amp, swirl | rings, spokes, len
eval_pick!(eval_maxwell, maxwell, [2, 4, 8, 9, 6, 5]); // sources, sep, k | amp, swirl, phase
eval_pick!(eval_axon, axon, [0, 1, 2, 5, 7, 4]); // fibres, length, bundle | node-spacing, pulse-speed, thickness
eval_pick!(eval_neural_net, neural_net, [1, 2, 4, 6, 12, 9]); // nodes, conn, layers | extent, pulse-speed, thickness
eval_pick!(eval_phyl, phyl, [1, 2, 4, 3, 5, 6]); // count, divergence, parastichy | radius, height, growth
eval_pick!(eval_mandel, mandelbulb, [0, 1, 7, 2, 3, 5]); // power, iter, bailout | scale, detail, morph
eval_pick!(eval_lens, lens, [0, 1, 2, 4, 5, 3]); // focal, aperture, thickness | scale, steps, plano
eval_pick!(eval_creature, creature, [0, 1, 2, 3, 4, 7]); // form, scale, detail | swim, warp, glow
eval_pick!(eval_kifs, kifs, [0, 1, 2, 6, 10, 11]); // sectors, fold, iter | zoom, glow, hue
eval_pick!(eval_boids, boids, [0, 1, 13, 3, 4, 5, 10]); // count, perception, speed, [sep,align,cohere] weights, goal
eval_pick!(eval_tess, tessellation, [1, 2, 5, 7, 11, 13]); // depth, scale, height | beat, phason, ammann
eval_pick!(eval_vecfield, vecfield, [0, 1, 2, 3, 4, 5]); // preset, [gx, gy, gz], extent, field scale
// Neural field (#224 review): the readout slots come from `neural2` like the
// macro, but the live ticker is the latent WALK — the triangle-wave morph seed
// A→B (`math::visual` uses the continuous beat position; here the ctx's beat
// fraction reproduces it within the bar) — not the generic `gen_phase`.
fn eval_neural(c: &OverlayCtx) -> Values {
    let b = c.s.neural2;
    let mut v = [0.0f32; 16];
    v[0] = b[0]; // size
    v[1] = b[1]; // detail
    v[2] = b[2]; // iso
    v[3] = b[3]; // steps
    v[4] = b[5]; // colour
    v[5] = b[6]; // walk-rate
    // Resolved walk = triangle(manual walk `neural[3]` + beat · walk-rate `neural2[6]`).
    let ph = c.s.neural[3] + c.beat * b[6];
    let frac = ph.rem_euclid(2.0);
    v[LIVE_SLOT] = if frac <= 1.0 { frac } else { 2.0 - frac };
    Values { v }
}

/// Rails (#187): decode the cell-length ordinal into beats so the card shows a
/// musical value, and tick the live slot with the beat fraction (the rail
/// coordinate IS the beat clock, so this is literally the ride position).
fn eval_rails(c: &OverlayCtx) -> Values {
    let r = c.s.rails;
    let mut v = [0.0f32; 16];
    let cell_tab = [1.0, 2.0, 4.0, 8.0, 16.0];
    v[0] = r[0]; // speed (units/beat)
    v[1] = r[1]; // bore
    v[2] = cell_tab[(r[2] as usize).min(cell_tab.len() - 1)]; // cell beats
    v[3] = r[4]; // variance
    v[4] = r[11]; // max lobes
    v[5] = r[13]; // twist (turns/beat)
    v[LIVE_SLOT] = c.beat; // live beat fraction = ride position within the beat
    Values { v }
}

/// Demo scene bench (#288) eval — reports the active sub-scene id and its key dials.
fn eval_demo(c: &OverlayCtx) -> Values {
    let d = c.s.demo;
    let mut v = [0.0f32; 16];
    v[0] = d[0]; // scene id
    v[1] = d[1]; // scale
    v[2] = d[4]; // light
    v[3] = d[6]; // count
    v[LIVE_SLOT] = c.beat; // live beat fraction (turntable phase)
    Values { v }
}

/// Acoustic (#325) eval — the live acoustic-field values: source kind, wavenumber
/// k, near-field weight, the pressure↔velocity blend, and the live oscillation
/// phase (ωt).
fn eval_acoustic(c: &OverlayCtx) -> Values {
    let a = c.s.acoustic;
    let mut v = [0.0f32; 16];
    v[0] = a[0]; // source multipole
    v[1] = a[1]; // wavenumber k
    v[2] = a[2]; // near-field weight
    v[3] = a[6]; // compression↔circulation blend
    v[LIVE_SLOT] = wrap_tau(c.maxdip_phase); // live oscillation phase ωt (the pitch-scaled clock the field rides)
    Values { v }
}

/// Field Engine (#381) eval — the live coefficients: kind, gallery preset, domain
/// scale k, and the two host-mappable coefficients a/b, plus the live time clock.
fn eval_field(c: &OverlayCtx) -> Values {
    let f = c.s.field;
    let mut v = [0.0f32; 16];
    v[0] = f[0]; // FieldKind
    v[1] = f[1]; // FieldPreset (gallery)
    v[2] = f[2]; // domain scale k
    v[3] = f[4]; // coefficient a
    v[4] = f[5]; // coefficient b
    v[LIVE_SLOT] = c.gen_phase; // live time t
    Values { v }
}

/// Density-Map Attractor (#380) eval — the live map values: kind, the EFFECTIVE
/// parameters a / b from the Tier-2 **parameter orbit** ("you are here in chaos-
/// space"), the point count (K), and the orbit's loop length + mode. The live ticker
/// is the loop phase φ. Uses the SAME `map_attractor_effective_ab` the renderer uses,
/// so the readout (and the inset dot) always match the field on screen.
fn eval_map_attractor(c: &OverlayCtx) -> Values {
    let m = c.s.mapattractor;
    let o = c.s.maporbit;
    let playing = c.s.transport[0] > 0.5;
    let ab = crate::math::map_attractor_effective_ab(&m, &o, c.beat_pos, c.gen_phase_hi, playing);
    let loop_beats = o[1].max(0.25);
    // The live φ ticker is a *closed-loop* phase — only meaningful in Lissajous mode.
    // Off has no motion and Linear rides an unbounded `a_drive`/`b_drive` ramp (the
    // a/b readouts already show that motion), so a moving loop-phase ticker there
    // would misrepresent the field. Derive it in f64 then narrow (same as the
    // renderer), so it doesn't quantize away once `beat_pos` grows large.
    let is_lissajous =
        crate::math::MapOrbitMode::from_u32(o[0] as u32) == crate::math::MapOrbitMode::Lissajous;
    let phi = if !is_lissajous {
        0.0
    } else if playing {
        (c.beat_pos / loop_beats as f64).rem_euclid(1.0) as f32
    } else {
        (c.gen_phase_hi * o[7] as f64).rem_euclid(1.0) as f32
    };
    let mut v = [0.0f32; 16];
    v[0] = m[0]; // map kind
    v[1] = ab.x; // effective a (orbit)
    v[2] = ab.y; // effective b (orbit)
    v[3] = m[3]; // points (K)
    v[4] = o[0]; // orbit mode (0 Off / 1 Linear / 2 Lissajous)
    v[5] = loop_beats; // loop length (beats)
    v[LIVE_SLOT] = wrap_tau(phi * std::f32::consts::TAU); // live loop phase φ (Lissajous only)
    Values { v }
}

/// Synchrotron (#150) flagship eval — the live values plugged into the Lienard-Wiechert
/// formula: orbit R, speed beta, the Lorentz factor gamma = 1/sqrt(1-beta^2), the tightest
/// (forward) Doppler factor kappa_min = 1-beta, the relativistic beam half-angle ~1/gamma
/// (deg), and the live orbital phase theta(t) = omega*t (omega = beta/R).
fn eval_synchrotron(c: &OverlayCtx) -> Values {
    let y = c.s.synchrotron;
    let mut v = [0.0f32; 16];
    let radius = y[0].max(1.0e-3);
    let beta = y[1].clamp(0.0, 0.999);
    let gamma = 1.0 / (1.0 - beta * beta).max(1.0e-6).sqrt();
    v[0] = radius; // R
    v[1] = beta; // beta
    v[2] = y[2]; // charges
    v[3] = gamma; // Lorentz factor
    v[4] = 1.0 - beta; // kappa_min = 1 - beta (forward beaming)
    v[5] = (1.0 / gamma).to_degrees(); // beam half-angle ~ 1/gamma
    v[LIVE_SLOT] = wrap_tau((beta / radius) * c.gen_phase); // live orbital phase
    Values { v }
}

// ----------------------------------------------------------------------------
// Static metadata tables
// ----------------------------------------------------------------------------

const SYM_TH: Symbol = Symbol { name: "theta", color: CYAN };
const SYM_PH: Symbol = Symbol { name: "phi", color: GREEN };
const SYM_KA: Symbol = Symbol { name: "kappa", color: ORANGE };
const SYM_TA: Symbol = Symbol { name: "tau", color: ROSE };
const SYM_U: Symbol = Symbol { name: "u", color: PINK };
const SYM_V: Symbol = Symbol { name: "v", color: PURPLE };
// Synchrotron (#150): beta shares u's pink, gamma shares phi's green (matching gen.mjs).
const SYM_BE: Symbol = Symbol { name: "beta", color: PINK };
const SYM_GA: Symbol = Symbol { name: "gamma", color: GREEN };

macro_rules! ro {
    ($label:expr, $slot:expr, $fmt:expr) => {
        Readout { label: $label, slot: $slot, span: 1, color: None, fmt: $fmt }
    };
}
// A bracketed vector readout: `label [s0, s0+1, …]`.
macro_rules! rov {
    ($label:expr, $slot:expr, $span:expr, $fmt:expr) => {
        Readout { label: $label, slot: $slot, span: $span, color: None, fmt: $fmt }
    };
}

// Organic Math (Original cube field): the actual cube-field parameters, vectors
// compressed into bracketed [x, y, z] arrays.
static ORIGINAL_SYMS: &[Symbol] = &[SYM_TH];
static ORIGINAL_GEOM: &[Readout] = &[
    rov!("loops", 0, 4, Fmt::int()),   // [Lx, Ly, Lz, Lq]
    rov!("rot amp", 4, 3, Fmt::f2()),  // [x, y, z]
    rov!("tr amp", 7, 3, Fmt::f2()),   // [x, y, z]
];
static ORIGINAL_MOTION: &[Readout] = &[
    rov!("rate", 10, 3, Fmt::f2()), // [x, y, z] per-axis rotation speed
];
static ORIGINAL_GROUPS: &[ReadoutGroup] = &[
    ReadoutGroup { title: "geometry", rows: ORIGINAL_GEOM },
    ReadoutGroup { title: "motion", rows: ORIGINAL_MOTION },
];

// Density-Map Attractor (#380): the live "you are here in chaos-space" readout —
// the effective (a,b) from the parameter orbit + the orbit's mode/loop length.
static MAP_ATTRACTOR_ROWS: &[Readout] = &[
    ro!("a", 1, Fmt::f2()),
    ro!("b", 2, Fmt::f2()),
    ro!("points K", 3, Fmt::int()),
    ro!("orbit", 4, Fmt::int()),      // 0 Off / 1 Linear / 2 Lissajous
    ro!("loop beats", 5, Fmt::f2()),
];
static MAP_ATTRACTOR_GROUPS: &[ReadoutGroup] = &[
    ReadoutGroup { title: "chaos-space", rows: MAP_ATTRACTOR_ROWS },
];

// Frenet
static FRENET_SYMS: &[Symbol] = &[SYM_KA, SYM_TA];
static FRENET_CURVE: &[Readout] = &[
    ro!("kappa", 0, Fmt::f2()),
    ro!("tau", 1, Fmt::f2()),
    ro!("step", 2, Fmt::f2()),
];
static FRENET_SHAPE: &[Readout] = &[
    ro!("strands", 3, Fmt::int()),
    ro!("nodes", 4, Fmt::int()),
    ro!("spread", 5, Fmt::f2()),
];
static FRENET_GROUPS: &[ReadoutGroup] = &[
    ReadoutGroup { title: "curve", rows: FRENET_CURVE },
    ReadoutGroup { title: "shape", rows: FRENET_SHAPE },
];

// DNA
static DNA_SYMS: &[Symbol] = &[SYM_TH];
static DNA_HELIX: &[Readout] = &[
    ro!("bp", 0, Fmt::int()),
    ro!("bp/turn", 1, Fmt::f2()),
    ro!("rise", 2, Fmt::f2()),
];
static DNA_SHAPE: &[Readout] = &[
    ro!("radius", 3, Fmt::f2()),
    ro!("groove", 4, Fmt::f2()),
    ro!("twist", 5, Fmt::f2()),
];
static DNA_GROUPS: &[ReadoutGroup] = &[
    ReadoutGroup { title: "helix", rows: DNA_HELIX },
    ReadoutGroup { title: "shape", rows: DNA_SHAPE },
];

// Harmonic
static HARM_SYMS: &[Symbol] = &[SYM_TH, SYM_PH];
static HARM_MODES: &[Readout] = &[
    rov!("modes", 0, 3, Fmt::int()), // [l0, l1, l2]
    rov!("amps", 3, 3, Fmt::f2()),   // [a0, a1, a2]
    ro!("radius", 6, Fmt::f2()),
];
static HARM_GROUPS: &[ReadoutGroup] = &[ReadoutGroup { title: "harmonics", rows: HARM_MODES }];

// Minimal
static MIN_SYMS: &[Symbol] = &[SYM_U, SYM_V];
static MIN_SURFACE: &[Readout] = &[
    ro!("scale", 0, Fmt::f2()),
    ro!("cells", 1, Fmt::f2()),
    ro!("iso", 2, Fmt::signed2()),
];
static MIN_FORM: &[Readout] = &[
    ro!("twist", 3, Fmt::f2()),
    ro!("bend", 4, Fmt::f2()),
    ro!("turns", 5, Fmt::f2()),
];
static MIN_GROUPS: &[ReadoutGroup] = &[
    ReadoutGroup { title: "surface", rows: MIN_SURFACE },
    ReadoutGroup { title: "form", rows: MIN_FORM },
];

// --- Non-flagship generators (no formula image yet; meaningful readouts) ---

static ATTR_A: &[Readout] = &[ro!("dt", 0, Fmt::f2()), ro!("speed", 1, Fmt::f2()), ro!("scale", 2, Fmt::f2())];
static ATTR_B: &[Readout] = &[ro!("seeds", 3, Fmt::int()), ro!("trail", 4, Fmt::int()), ro!("spread", 5, Fmt::f2())];
static ATTR_G: &[ReadoutGroup] = &[ReadoutGroup { title: "flow", rows: ATTR_A }, ReadoutGroup { title: "seed", rows: ATTR_B }];

static LS_A: &[Readout] = &[ro!("depth", 0, Fmt::int()), ro!("angle", 1, Fmt::f2()), ro!("step", 2, Fmt::f2())];
static LS_B: &[Readout] = &[ro!("sway amp", 3, Fmt::f2()), ro!("sway freq", 4, Fmt::f2()), ro!("grow", 5, Fmt::f2())];
static LS_G: &[ReadoutGroup] = &[ReadoutGroup { title: "growth", rows: LS_A }, ReadoutGroup { title: "sway", rows: LS_B }];

static CN_A: &[Readout] = &[ro!("scale", 0, Fmt::f2()), ro!("flow", 1, Fmt::f2()), ro!("dt", 2, Fmt::f2())];
static CN_B: &[Readout] = &[ro!("seeds", 3, Fmt::int()), ro!("steps", 4, Fmt::int()), ro!("spread", 5, Fmt::f2())];
static CN_G: &[ReadoutGroup] = &[ReadoutGroup { title: "flow", rows: CN_A }, ReadoutGroup { title: "field", rows: CN_B }];

static POL_A: &[Readout] = &[ro!("k", 0, Fmt::f2()), ro!("amp", 1, Fmt::f2()), ro!("swirl", 2, Fmt::f2())];
static POL_B: &[Readout] = &[ro!("rings", 3, Fmt::int()), ro!("spokes", 4, Fmt::int()), ro!("len", 5, Fmt::f2())];
static POL_G: &[ReadoutGroup] = &[ReadoutGroup { title: "field", rows: POL_A }, ReadoutGroup { title: "fan", rows: POL_B }];

static MX_A: &[Readout] = &[ro!("sources", 0, Fmt::int()), ro!("sep", 1, Fmt::f2()), ro!("k", 2, Fmt::f2())];
static MX_B: &[Readout] = &[ro!("amp", 3, Fmt::f2()), ro!("swirl", 4, Fmt::f2()), ro!("phase", 5, Fmt::f2())];
static MX_G: &[ReadoutGroup] = &[ReadoutGroup { title: "charges", rows: MX_A }, ReadoutGroup { title: "field", rows: MX_B }];

static AX_A: &[Readout] = &[ro!("fibres", 0, Fmt::int()), ro!("length", 1, Fmt::f2()), ro!("bundle", 2, Fmt::f2())];
static AX_B: &[Readout] = &[ro!("nodes", 3, Fmt::f2()), ro!("pulse", 4, Fmt::f2()), ro!("thick", 5, Fmt::f2())];
static AX_G: &[ReadoutGroup] = &[ReadoutGroup { title: "bundle", rows: AX_A }, ReadoutGroup { title: "signal", rows: AX_B }];

static NW_A: &[Readout] = &[ro!("nodes", 0, Fmt::int()), ro!("conn", 1, Fmt::int()), ro!("layers", 2, Fmt::int())];
static NW_B: &[Readout] = &[ro!("extent", 3, Fmt::f2()), ro!("pulse", 4, Fmt::f2()), ro!("thick", 5, Fmt::f2())];
static NW_G: &[ReadoutGroup] = &[ReadoutGroup { title: "graph", rows: NW_A }, ReadoutGroup { title: "look", rows: NW_B }];

static PHYL_A: &[Readout] = &[ro!("count", 0, Fmt::int()), ro!("diverge", 1, Fmt::f2()), ro!("parastichy", 2, Fmt::int())];
static PHYL_B: &[Readout] = &[ro!("radius", 3, Fmt::f2()), ro!("height", 4, Fmt::f2()), ro!("growth", 5, Fmt::f2())];
static PHYL_G: &[ReadoutGroup] = &[ReadoutGroup { title: "spiral", rows: PHYL_A }, ReadoutGroup { title: "shape", rows: PHYL_B }];

static MB_A: &[Readout] = &[ro!("power", 0, Fmt::f2()), ro!("iter", 1, Fmt::int()), ro!("bailout", 2, Fmt::f2())];
static MB_B: &[Readout] = &[ro!("scale", 3, Fmt::f2()), ro!("detail", 4, Fmt::int()), ro!("morph", 5, Fmt::f2())];
static MB_G: &[ReadoutGroup] = &[ReadoutGroup { title: "fractal", rows: MB_A }, ReadoutGroup { title: "render", rows: MB_B }];

static NN_A: &[Readout] = &[ro!("size", 0, Fmt::f2()), ro!("detail", 1, Fmt::f2()), ro!("iso", 2, Fmt::f2())];
static NN_B: &[Readout] = &[ro!("steps", 3, Fmt::int()), ro!("colour", 4, Fmt::f2()), ro!("walk/beat", 5, Fmt::f2())];
static NN_G: &[ReadoutGroup] = &[ReadoutGroup { title: "field", rows: NN_A }, ReadoutGroup { title: "render", rows: NN_B }];

// Lens (#258 Tier 3): the lens shape + its raymarch budget.
static LENS_A: &[Readout] = &[ro!("focal", 0, Fmt::f2()), ro!("aperture", 1, Fmt::f2()), ro!("thickness", 2, Fmt::f2())];
static LENS_B: &[Readout] = &[ro!("scale", 3, Fmt::f2()), ro!("steps", 4, Fmt::int()), ro!("plano", 5, Fmt::f2())];
static LENS_G: &[ReadoutGroup] = &[ReadoutGroup { title: "lens", rows: LENS_A }, ReadoutGroup { title: "render", rows: LENS_B }];

// Creature Engine (#476 Tier 1): the body plan + its swim.
static CR_A: &[Readout] = &[ro!("form", 0, Fmt::int()), ro!("scale", 1, Fmt::f2()), ro!("detail", 2, Fmt::int())];
static CR_B: &[Readout] = &[ro!("swim", 3, Fmt::f2()), ro!("warp", 4, Fmt::f2()), ro!("glow", 5, Fmt::f2())];
static CR_G: &[ReadoutGroup] = &[ReadoutGroup { title: "body", rows: CR_A }, ReadoutGroup { title: "swim", rows: CR_B }];

static KF_A: &[Readout] = &[ro!("sectors", 0, Fmt::f2()), ro!("fold", 1, Fmt::f2()), ro!("iter", 2, Fmt::int())];
static KF_B: &[Readout] = &[ro!("zoom", 3, Fmt::f2()), ro!("glow", 4, Fmt::f2()), ro!("hue", 5, Fmt::f2())];
static KF_G: &[ReadoutGroup] = &[ReadoutGroup { title: "fold", rows: KF_A }, ReadoutGroup { title: "look", rows: KF_B }];

static BO_A: &[Readout] = &[ro!("count", 0, Fmt::int()), ro!("perception", 1, Fmt::f2()), ro!("speed", 2, Fmt::f2())];
static BO_B: &[Readout] = &[rov!("rules", 3, 3, Fmt::f2()), ro!("goal", 6, Fmt::f2())]; // [sep, align, cohere]
static BO_G: &[ReadoutGroup] = &[ReadoutGroup { title: "flock", rows: BO_A }, ReadoutGroup { title: "rules", rows: BO_B }];

static TS_A: &[Readout] = &[ro!("depth", 0, Fmt::int()), ro!("scale", 1, Fmt::f2()), ro!("height", 2, Fmt::f2())];
static TS_B: &[Readout] = &[ro!("beat", 3, Fmt::f2()), ro!("phason", 4, Fmt::f2()), ro!("ammann", 5, Fmt::f2())];
static TS_G: &[ReadoutGroup] = &[ReadoutGroup { title: "tiling", rows: TS_A }, ReadoutGroup { title: "detail", rows: TS_B }];

// Vector field (#173): the sampled function + the lattice it's plotted on.
static VF_A: &[Readout] = &[ro!("function", 0, Fmt::int()), rov!("grid", 1, 3, Fmt::int())];
static VF_B: &[Readout] = &[ro!("extent", 4, Fmt::f2()), ro!("field scale", 5, Fmt::f2())];
static VF_G: &[ReadoutGroup] = &[ReadoutGroup { title: "field", rows: VF_A }, ReadoutGroup { title: "domain", rows: VF_B }];

// Rails (#187): the ride + the morphing profile.
static RL_A: &[Readout] = &[ro!("speed", 0, Fmt::f2()), ro!("bore", 1, Fmt::f2()), ro!("cell beats", 2, Fmt::int())];
static RL_B: &[Readout] = &[ro!("variance", 3, Fmt::f2()), ro!("lobes", 4, Fmt::int()), ro!("twist", 5, Fmt::f2())];
static RL_G: &[ReadoutGroup] = &[ReadoutGroup { title: "rail", rows: RL_A }, ReadoutGroup { title: "profile", rows: RL_B }];

// Synchrotron (#150) flagship readout. Symbol order: [beta, gamma, kappa, theta] — the
// coloured `color: Some(i)` readouts link to the same hues as the formula image.
static SYN_SYMS: &[Symbol] = &[SYM_BE, SYM_GA, SYM_KA, SYM_TH];
static SYN_CHARGE: &[Readout] = &[
    ro!("radius", 0, Fmt::f2()),
    Readout { label: "beta", slot: 1, span: 1, color: Some(0), fmt: Fmt::f2() }, // beta (pink)
    ro!("charges", 2, Fmt::int()),
];
static SYN_BEAM: &[Readout] = &[
    Readout { label: "gamma", slot: 3, span: 1, color: Some(1), fmt: Fmt::f2() }, // gamma (green)
    Readout { label: "kappa min", slot: 4, span: 1, color: Some(2), fmt: Fmt::f2() }, // kappa (orange)
    ro!("beam deg", 5, Fmt::f2()),
];
static SYN_G: &[ReadoutGroup] = &[
    ReadoutGroup { title: "charge", rows: SYN_CHARGE },
    ReadoutGroup { title: "beaming", rows: SYN_BEAM },
];
static DEMO_ROWS: &[Readout] = &[
    ro!("scene", 0, Fmt::int()),
    ro!("scale", 1, Fmt::f2()),
    ro!("light", 2, Fmt::f2()),
    ro!("count", 3, Fmt::int()),
];
static DEMO_G: &[ReadoutGroup] = &[ReadoutGroup { title: "scene", rows: DEMO_ROWS }];

/// The overlay record for a generator. The 5 flagships carry a formula image; the rest
/// show their key parameters with no formula (a title-only fallback covers any future
/// generator).
pub fn overlay_meta(g: GeneratorMode) -> OverlayMeta {
    match g {
        GeneratorMode::Original => OverlayMeta {
            title: "Organic Math",
            description: "The original cube field: rotate-then-translate, compounded across nested loops into organic strands.",
            formula: Some(FormulaId::Original),
            symbols: ORIGINAL_SYMS,
            groups: ORIGINAL_GROUPS,
            live_label: "spin",
            eval: eval_original,
        },
        GeneratorMode::Frenet => OverlayMeta {
            title: "Frenet-Serret",
            description: "A space curve grown from its own curvature and torsion.",
            formula: Some(FormulaId::Frenet),
            symbols: FRENET_SYMS,
            groups: FRENET_GROUPS,
            live_label: "trace",
            eval: eval_frenet,
        },
        GeneratorMode::Dna => OverlayMeta {
            title: "DNA Double Helix",
            description: "Two antiparallel helices wound at a constant pitch.",
            formula: Some(FormulaId::Dna),
            symbols: DNA_SYMS,
            groups: DNA_GROUPS,
            live_label: "twist",
            eval: eval_dna,
        },
        GeneratorMode::Harmonic => OverlayMeta {
            title: "Spherical Harmonics",
            description: "A sphere whose radius is a sum of spherical-harmonic modes.",
            formula: Some(FormulaId::Harmonic),
            symbols: HARM_SYMS,
            groups: HARM_GROUPS,
            live_label: "pulse",
            eval: eval_harmonic,
        },
        GeneratorMode::MinimalSurface => OverlayMeta {
            title: "Minimal Surface",
            description: "A zero-mean-curvature soap film: the catenoid-helicoid family.",
            formula: Some(FormulaId::Minimal),
            symbols: MIN_SYMS,
            groups: MIN_GROUPS,
            live_label: "bend",
            eval: eval_minimal,
        },
        GeneratorMode::Attractor => OverlayMeta {
            title: "Strange Attractor",
            description: "A chaotic flow traced by integrating a strange-attractor field.",
            formula: Some(FormulaId::Attractor),
            symbols: &[],
            groups: ATTR_G,
            live_label: "t",
            eval: eval_attr,
        },
        GeneratorMode::LSystem => OverlayMeta {
            title: "L-System",
            description: "A grammar-grown plant: rewrite rules unfolded into branches.",
            formula: Some(FormulaId::LSystem),
            symbols: &[],
            groups: LS_G,
            live_label: "grow",
            eval: eval_ls,
        },
        GeneratorMode::CurlNoise => OverlayMeta {
            title: "Curl-Noise Flow",
            description: "Divergence-free curl noise advecting ink through space.",
            formula: Some(FormulaId::CurlNoise),
            symbols: &[],
            groups: CN_G,
            live_label: "flow",
            eval: eval_cn,
        },
        GeneratorMode::Polarization => OverlayMeta {
            title: "Circular Polarization",
            description: "An E/B helix fan: rotating polarization vectors around a ring.",
            formula: Some(FormulaId::Polarization),
            symbols: &[],
            groups: POL_G,
            live_label: "phase",
            eval: eval_pol,
        },
        GeneratorMode::MaxwellField => OverlayMeta {
            title: "Maxwell Field",
            description: "Real E/B fields from charges and dipoles, with retarded time.",
            formula: Some(FormulaId::Maxwell),
            symbols: &[],
            groups: MX_G,
            live_label: "phase",
            eval: eval_maxwell,
        },
        GeneratorMode::Phyllotaxis => OverlayMeta {
            title: "Phyllotaxis",
            description: "Golden-angle packing: florets spiralling by the divergence angle.",
            formula: Some(FormulaId::Phyllotaxis),
            symbols: &[],
            groups: PHYL_G,
            live_label: "grow",
            eval: eval_phyl,
        },
        GeneratorMode::Mandelbulb => OverlayMeta {
            title: "Mandelbulb",
            description: "A 3-D escape-time fractal raymarched by distance estimation.",
            formula: Some(FormulaId::Mandelbulb),
            symbols: &[],
            groups: MB_G,
            live_label: "spin",
            eval: eval_mandel,
        },
        GeneratorMode::Kaleidoscope => OverlayMeta {
            title: "Kaleidoscopic Fractal",
            description: "Kaleidoscope folds and circle-inversion IFS raymarched per pixel.",
            formula: Some(FormulaId::Kifs),
            symbols: &[],
            groups: KF_G,
            live_label: "churn",
            eval: eval_kifs,
        },
        GeneratorMode::Boids => OverlayMeta {
            title: "Boids",
            description: "Reynolds flocking: separation, alignment and cohesion in a swarm.",
            formula: Some(FormulaId::Boids),
            symbols: &[],
            groups: BO_G,
            live_label: "t",
            eval: eval_boids,
        },
        GeneratorMode::Tessellation => OverlayMeta {
            title: "Tessellation",
            description: "Aperiodic tilings grown by inflation or cut-and-project.",
            formula: Some(FormulaId::Tessellation),
            symbols: &[],
            groups: TS_G,
            live_label: "phase",
            eval: eval_tess,
        },
        GeneratorMode::Synchrotron => OverlayMeta {
            title: "Synchrotron Radiation",
            description: "Lienard-Wiechert field of a relativistic charge orbiting a circle (retarded time).",
            formula: Some(FormulaId::Synchrotron),
            symbols: SYN_SYMS,
            groups: SYN_G,
            live_label: "orbit",
            eval: eval_synchrotron,
        },
        GeneratorMode::VectorField => OverlayMeta {
            title: "Vector Field",
            description: "A function F(x, y, z) plotted as a lattice of arrows - the vector-field plot in 3-D.",
            formula: Some(FormulaId::VectorField),
            symbols: &[],
            groups: VF_G,
            live_label: "evolve",
            eval: eval_vecfield,
        },
        GeneratorMode::AxonWaveguide => OverlayMeta {
            title: "Axon Waveguide",
            description: "Myelinated axons as optical fibres: guided light, Ranvier nodes, a travelling pulse.",
            formula: Some(FormulaId::Axon),
            symbols: &[],
            groups: AX_G,
            live_label: "pulse",
            eval: eval_axon,
        },
        GeneratorMode::NeuralField => OverlayMeta {
            title: "Neural Field",
            description: "A tiny SIREN network (x,y,z,t) raymarched as an implicit isosurface; the organism is a seed.",
            formula: None,
            symbols: &[],
            groups: NN_G,
            live_label: "walk",
            eval: eval_neural,
        },
        GeneratorMode::NeuralNetwork => OverlayMeta {
            title: "Neural Network",
            description: "A graph of neuron nodes wired by fibre-tract edges: connectivity + geometry, not a neural sim.",
            formula: None,
            symbols: &[],
            groups: NW_G,
            live_label: "pulse",
            eval: eval_neural_net,
        },
        GeneratorMode::Lens => OverlayMeta {
            title: "Lens",
            description: "An analytic double-convex / plano-convex lens raymarched as an SDF; shade it as Glass to refract.",
            formula: None,
            symbols: &[],
            groups: LENS_G,
            live_label: "orbit",
            eval: eval_lens,
        },
        GeneratorMode::Creature => OverlayMeta {
            title: "Creature Engine",
            description: "A synthetic sea creature: a union of SDF primitives (ellipsoids/capsules/paddles) along a spine, raymarched; a travelling peristaltic warp is the swim.",
            formula: None,
            symbols: &[],
            groups: CR_G,
            live_label: "swim",
            eval: eval_creature,
        },
        GeneratorMode::None => OverlayMeta {
            // The generator is off — the Scenery layer (usually the Zone
            // corridor) carries the scene, so its card shows the ride.
            title: "Scenery Ride",
            description: "Primary generator off: the Scenery layer carries the scene - a beat-parametrized corridor crossed exactly on the beat.",
            formula: Some(FormulaId::Rails),
            symbols: &[],
            groups: RL_G,
            live_label: "beat",
            eval: eval_rails,
        },
        GeneratorMode::Demo => OverlayMeta {
            title: "Demo (scene bench)",
            description: "A hand-authored reference scene for the ray-tracing stack: Cornell box, sphere pyramids, a glass menagerie, a light stage.",
            formula: None,
            symbols: &[],
            groups: DEMO_G,
            live_label: "spin",
            eval: eval_demo,
        },
        GeneratorMode::Acoustic => OverlayMeta {
            title: "Acoustic Field",
            description: "A radiating sound source: compression (longitudinal) and transverse flow (particle-velocity + circulation), the acoustic analog of Maxwell's E and B — each crossfadeable independently.",
            formula: None,
            symbols: &[],
            groups: &[],
            live_label: "phase",
            eval: eval_acoustic,
        },
        GeneratorMode::FieldEngine => OverlayMeta {
            title: "Field Engine",
            description: "An arbitrary closed-form field equation over (x,y,z,t): scalar, vector, or complex, rendered through the shared field-line / aura / density machinery.",
            formula: None,
            symbols: &[],
            groups: &[],
            live_label: "t",
            eval: eval_field,
        },
        GeneratorMode::MapAttractor => OverlayMeta {
            title: "Density-Map Attractor",
            description: "A discrete complex-holomorphic map x'=sin(x^2-y^2+a), y'=cos(2xy+b) iterated for many points; the visited-set density is an additive glow (best in Splat + bloom). Tier 2: (a,b) walk a closed, beat-locked parameter orbit (Lissajous) so the field morphs seamlessly - one loop per 'loop beats' of the host clock.",
            formula: None,
            symbols: &[],
            groups: MAP_ATTRACTOR_GROUPS,
            live_label: "phi",
            eval: eval_map_attractor,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLAGSHIPS: [GeneratorMode; 6] = [
        GeneratorMode::Original,
        GeneratorMode::Frenet,
        GeneratorMode::Dna,
        GeneratorMode::Harmonic,
        GeneratorMode::MinimalSurface,
        GeneratorMode::Synchrotron,
    ];

    // Generators with a full formula/readout card (Demo + Acoustic carry a title +
    // live value but no formula groups, like the raymarch siblings, so they're not
    // in this curated set the card-completeness tests iterate).
    const ALL: [GeneratorMode; 22] = [
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
    ];

    fn ctx(s: &Shared) -> OverlayCtx<'_> {
        OverlayCtx { gen_phase: 0.8, gen_phase_hi: 0.8, maxdip_phase: 0.8, angle: 1.3, beat: 0.25, beat_pos: 4.25, bpm: 120.0, s }
    }

    #[test]
    fn flagships_have_formula_and_groups() {
        for g in FLAGSHIPS {
            let m = overlay_meta(g);
            assert!(m.formula.is_some(), "{g:?} missing formula");
            assert!(!m.title.is_empty());
            assert!(!m.groups.is_empty(), "{g:?} should have readout groups");
            for grp in m.groups {
                for r in grp.rows {
                    assert!(r.span >= 1 && r.slot + r.span <= 16, "{g:?} slot/span out of range");
                    if let Some(i) = r.color {
                        assert!(i < m.symbols.len(), "{g:?} bad symbol index {i}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_generator_has_meaningful_card() {
        // All 18 generators get a title, a description, and at least one readout group;
        // all drawn text is ASCII (the atlas is ASCII-only → non-ASCII renders '?'); and
        // every readout slot/span is in range with valid symbol-colour links.
        let s = Shared::default();
        for g in ALL {
            let m = overlay_meta(g);
            assert!(!m.title.is_empty() && m.title.is_ascii(), "{g:?} title");
            assert!(!m.description.is_empty() && m.description.is_ascii(), "{g:?} description");
            assert!(!m.groups.is_empty(), "{g:?} has no readout groups");
            // Every generator shows a live animation value (the ticker), named + finite.
            assert!(!m.live_label.is_empty() && m.live_label.is_ascii(), "{g:?} live_label");
            let v = (m.eval)(&ctx(&s));
            assert!(v.get(LIVE_SLOT).is_finite(), "{g:?} live value not finite");
            for grp in m.groups {
                assert!(grp.title.is_ascii(), "{g:?} group title not ASCII");
                for r in grp.rows {
                    assert!(r.label.is_ascii(), "{g:?} label not ASCII: {:?}", r.label);
                    assert!(r.span >= 1 && r.slot + r.span <= 16, "{g:?} slot/span out of range");
                    for k in 0..r.span {
                        assert!(v.get(r.slot + k).is_finite(), "{g:?} slot {} not finite", r.slot + k);
                    }
                    if let Some(i) = r.color {
                        assert!(i < m.symbols.len(), "{g:?} bad symbol index {i}");
                    }
                }
            }
        }
    }

    #[test]
    fn original_reads_real_params() {
        let mut s = Shared::default();
        s.loop_count = [2.0, 3.0, 4.0, 5.0];
        s.rot_amp = [0.1, 0.2, 0.3, 0.0];
        s.trans_amp = [0.4, 0.5, 0.6, 0.0];
        s.rot_mod = [0.6, 0.8, 1.0, 0.01];
        let v = eval_original(&ctx(&s));
        assert_eq!(v.get(0), 2.0); // Lx
        assert_eq!(v.get(3), 5.0); // Lq
        assert_eq!(v.get(4), 0.1); // rot amp x
        assert_eq!(v.get(9), 0.6); // tr amp z
        assert_eq!(v.get(10), 0.6); // rate x (rot_mod[0])
        assert_eq!(v.get(12), 1.0); // rate z (rot_mod[2])
    }

    #[test]
    fn synchrotron_reads_relativistic_kinematics() {
        let mut s = Shared::default();
        // R = 5, beta = 0.6, 3 charges → gamma = 1.25, kappa_min = 0.4.
        s.synchrotron = [
            5.0, 0.6, 3.0, 28.0, 14.0, 1.0, 1.0, 0.1, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];
        let v = eval_synchrotron(&ctx(&s));
        assert_eq!(v.get(0), 5.0); // R
        assert!((v.get(1) - 0.6).abs() < 1e-6); // beta
        assert_eq!(v.get(2), 3.0); // charges
        assert!((v.get(3) - 1.25).abs() < 1e-4, "gamma = 1/sqrt(1-beta^2)");
        assert!(v.get(3) >= 1.0, "gamma >= 1");
        assert!((v.get(4) - 0.4).abs() < 1e-6, "kappa_min = 1 - beta");
        assert!(v.get(5).is_finite() && v.get(5) > 0.0, "beam half-angle");
        assert!(v.get(LIVE_SLOT).is_finite());
    }

    #[test]
    fn flagship_values_are_finite() {
        let s = Shared::default();
        for g in FLAGSHIPS {
            let m = overlay_meta(g);
            let v = (m.eval)(&ctx(&s));
            for grp in m.groups {
                for r in grp.rows {
                    assert!(v.get(r.slot).is_finite(), "{g:?} slot {} not finite", r.slot);
                }
            }
        }
    }

    #[test]
    fn fmt_variants() {
        assert_eq!(Fmt::signed2().apply(0.5), "+0.50");
        assert_eq!(Fmt::signed2().apply(-0.46), "-0.46");
        assert_eq!(Fmt::f2().apply(0.5), "0.50");
        assert_eq!(Fmt::int().apply(5.0), "5");
        assert_eq!(Fmt::f2().apply(0.0), "0.00");
    }

    #[test]
    fn every_generator_has_a_formula() {
        // #135 P4 / #150 P2 / #173: the established generators carry a bundled
        // formula image. The Neural field (#200 Tier 1), Neural Network (#226
        // Tier 1) and Lens (#258 Tier 3) ship without one yet — a formula card is a
        // follow-up — so they're the documented exceptions.
        for g in ALL {
            if g == GeneratorMode::NeuralField
                || g == GeneratorMode::NeuralNetwork
                || g == GeneratorMode::Lens
            {
                continue;
            }
            assert!(overlay_meta(g).formula.is_some(), "{g:?} missing formula");
        }
    }
}
