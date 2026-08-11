//! **The substrate camera rig** — Console Spike Tier 1, Leaf A (as-built brief R2/R5).
//!
//! The console's backdrop is a flat lit plane in the world x–z plane. This module answers
//! one question about it, in arithmetic: *where does the camera go, and how narrow is the
//! lens, so that the plane exactly fills the console window?* It computes; it renders
//! nothing and touches no engine state. glam + std only — no wgpu, no egui, no nih_plug —
//! so the whole thing is testable headless, which is the point of a leaf module.
//!
//! ## What the engine will do with this
//!
//! The engine assembles its projection in exactly **one** place (brief R2):
//! `build_uniforms` (`world.rs:10551-10598`) builds
//! `Mat4::perspective_rh(fov_deg.to_radians(), aspect, CAM_NEAR, CAM_FAR)` over
//! `Mat4::look_at_rh(eye, center, up)`, with `eye = center + distance · dir` and
//! `dir = (cos·pitch·sin·yaw, sin·pitch, cos·pitch·cos·yaw)` (`:10579-10580`). There is no
//! `Camera` type to configure; the inputs arrive as a six-element tuple finalized at
//! `world.rs:6480-6524`, and the **rails** branch (`:6497-6511`) is the precedent for
//! overriding all six at once. A substrate rig is a third arm on that same `if`.
//! [`SubstrateRig::camera_arm`] returns exactly that tuple's shape.
//!
//! Everything here mirrors the engine's conventions deliberately: **glam, f32, right-handed,
//! and the FOV is VERTICAL** (`world.rs:10564-10567` says so in as many words). Getting the
//! axis wrong is silent — the picture is merely wrong by `aspect`.
//!
//! ## Why perspective and not orthographic
//!
//! Issue #3 asks for a near-orthographic look; brief R2 answers that this stays **narrow-FOV
//! perspective**. A true `orthographic_rh` breaks the DoF depth remap, SSAO/SSR/SSGI/VXGI
//! position reconstruction and TAA reprojection — every path that reconstructs a world
//! position from `inv_vp`. Long lens, far back, still perspective.
//!
//! ## Why the deviation bound is the deliverable and not a diagnostic
//!
//! Brief R5's finding that decides the Tier 1 beat: a perfectly flat plane, a uniform
//! material and no FOV shade to **one constant colour** — N, V and L are identical at every
//! fragment, so Fresnel, the specular lobe and the environment lookup all collapse. The FOV
//! is therefore not a framing detail, it *is* the shading gradient, and
//! [`max_view_deviation_deg`] is the size of it. A number quoted without its aspect ratio
//! would be wrong by nearly 2× between a portrait and a landscape window, which is why that
//! function takes aspect and no caller may substitute a constant.
//!
//! ## What this module does not do
//!
//! It does not widen the engine's FOV clamp (10°–120° today, at **two** sites —
//! `world.rs:6489-6490` and `:10597`; moving one is a silent no-op), latch off the camera's
//! auto-follow (`world.rs:5270` lerps `cam_center` toward the generator field's AABB centre
//! every frame), or choose the near/far planes. Those are `world.rs` edits and `world.rs`
//! belongs to the Tier 1 integrator. This module is correct over **4°–120°** precisely so it
//! is already right when the floor moves.

use glam::{Mat4, Vec3};
use std::f32::consts::FRAC_PI_2;

// ── The engine's constants, mirrored so tests can assert against them ────────────────

/// `CAM_NEAR` / `CAM_FAR`, baked into `build_uniforms` (`world.rs:10521-10522`). Mirrored,
/// not re-derived: if the integrator moves them (brief R2's edit 3 — at a long framing
/// distance a 0.1 near plane wastes most of a non-reversed `Depth32Float` buffer), these
/// move with them.
pub const ENGINE_NEAR: f32 = 0.1;
/// See [`ENGINE_NEAR`]. Note the consequence for framing: `distance` must land inside
/// `(ENGINE_NEAR, ENGINE_FAR)` or the engine draws nothing at all. See
/// [`SubstrateRig::frame_plane`] for the extent cap that implies.
pub const ENGINE_FAR: f32 = 5000.0;

/// What `world.rs` clamps the vertical FOV to **today**, at both sites. Recorded so the
/// integrator can assert the clamp actually moved; this module does not apply it.
pub const ENGINE_FOV_CLAMP_DEG: (f32, f32) = (10.0, 120.0);

/// The vertical-FOV band this module is **specified and tested** over. It is documentation,
/// not a clamp: 2° in gets you a correct 2° rig, never a silent 4°. The floor is 4° because
/// that is where the integrator is expected to take the engine (brief R2); the module must
/// not assume 10° or 4°.
pub const FOV_MIN_DEG: f32 = 4.0;
/// See [`FOV_MIN_DEG`].
pub const FOV_MAX_DEG: f32 = 120.0;

/// The aspect band this module is specified and tested over (0.1 = a tall strip,
/// 10.0 = an ultrawide). Also documentation, not a clamp.
pub const ASPECT_MIN: f32 = 0.1;
/// See [`ASPECT_MIN`].
pub const ASPECT_MAX: f32 = 10.0;

// ── The pitch epsilon: the sharpest edge in the rig ──────────────────────────────────

/// How far short of straight-down the rig stops, in radians.
///
/// A backdrop plane in x–z wants the camera directly above it looking straight down —
/// `pitch = π/2`. That is exactly the degenerate case: `build_uniforms` hard-codes
/// `up = Vec3::Y` when roll is 0 (`world.rs:10584-10591`), and glam's `look_at_rh` builds its
/// basis from `normalize(forward × up)`. Forward parallel to up makes that cross product
/// zero, `normalize` divides by zero, and **every** entry of the view matrix becomes NaN.
///
/// 1e-3 rad (0.057°) is the smallest tilt that leaves the cross product ~1e-3 in magnitude —
/// far enough above f32's 1.2e-7 epsilon that the camera's basis (and so which way "up" is on
/// screen) is stable rather than decided by rounding noise. The tilt it costs is folded
/// exactly into the framing distance, so it buys stability without costing coverage.
pub const PITCH_EPS_RAD: f32 = 1.0e-3;

/// The most vertical pitch the rig will emit: `π/2 − ` [`PITCH_EPS_RAD`].
pub const MAX_PITCH_RAD: f32 = FRAC_PI_2 - PITCH_EPS_RAD;

// ── Hard clamps: what a degenerate input becomes. Documented, never a panic ──────────

/// Below this the projection's `cot(fov/2)` stops being finite in f32.
const FOV_HARD_MIN_DEG: f32 = 0.01;
/// At 180° the projection is undefined (`sin_fov` → 0 in the other direction).
const FOV_HARD_MAX_DEG: f32 = 179.0;
/// The engine's own aspect floor (`world.rs:10593`: `.max(0.01)`), mirrored so the rig and
/// the render never disagree about what a degenerate window means.
const ASPECT_HARD_MIN: f32 = 0.01;
/// Not a real window; present only so the arithmetic stays finite.
const ASPECT_HARD_MAX: f32 = 1.0e4;
/// A plane smaller than this is not a backdrop.
const EXTENT_HARD_MIN: f32 = 1.0e-4;
/// Far past any world the engine draws; present only so `extent/2` stays finite.
const EXTENT_HARD_MAX: f32 = 1.0e18;
/// Keeps `eye != center`, without which `look_at_rh` normalizes a zero vector. Only bites
/// for planes small enough that the framing distance is sub-millimetre, where the coverage
/// guarantee is void — documented on [`frame_distance`].
const DISTANCE_MIN: f32 = 1.0e-4;
/// The ceiling, and it is not cosmetic: `look_at_rh` normalizes `center − eye`, and
/// `Vec3::length` **squares** the components first. A coordinate past ~1.8e19 squares to
/// +inf, `length_recip` becomes 0, `normalize` returns the zero vector, the basis cross
/// product is then zero too — and the entire view matrix is NaN. 1e18 squares to 1e36, two
/// orders inside f32's 3.4e38 ceiling. Reachable in practice: a 1e18 plane at the narrowest
/// lens asks for 5.7e21. Found by the degenerate-input test, not by inspection.
const DISTANCE_MAX: f32 = 1.0e18;

/// The engine's own fallback FOV (`world.rs:6488`), reused for a non-finite input.
const FOV_FALLBACK_DEG: f32 = 45.0;

// ── Sanitizers ───────────────────────────────────────────────────────────────────────

#[inline]
fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

/// NaN/±inf → 45° (the engine's own default); otherwise clamped to the band where
/// `tan(fov/2)` is finite and positive. Note this is **not** the engine's 10°–120° clamp:
/// applying that here would defeat the 4° rig this module exists to compute.
pub fn sanitize_fov_deg(fov_deg: f32) -> f32 {
    finite_or(fov_deg, FOV_FALLBACK_DEG).clamp(FOV_HARD_MIN_DEG, FOV_HARD_MAX_DEG)
}

/// NaN/±inf → 1.0; otherwise clamped to `[0.01, 1e4]`. The floor matches the engine's
/// (`world.rs:10593`) so a zero-height render target means the same thing on both sides.
pub fn sanitize_aspect(aspect: f32) -> f32 {
    finite_or(aspect, 1.0).clamp(ASPECT_HARD_MIN, ASPECT_HARD_MAX)
}

/// NaN/±inf → 1.0; a negative extent is read as a sign slip and its magnitude used.
fn sanitize_extent(extent: f32) -> f32 {
    finite_or(extent, 1.0).abs().clamp(EXTENT_HARD_MIN, EXTENT_HARD_MAX)
}

// ── Which axis governs the framing ───────────────────────────────────────────────────

/// Which viewport axis the framing distance is pinned by — the one the plane exactly fills.
/// The other overhangs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoverningAxis {
    /// Landscape: the view is wider than it is tall, so width runs out first.
    Horizontal,
    /// Portrait (and square): height runs out first.
    Vertical,
}

/// The axis that governs [`frame_distance`] at this aspect.
///
/// Ideally this is just "landscape → horizontal", because the horizontal half-angle is the
/// vertical one scaled by `aspect`. The rig's [`PITCH_EPS_RAD`] tilt shifts the threshold to
/// `aspect > sec(ε)`, which differs from 1 by 5e-7 — invisible for any real window, and
/// carried exactly anyway so this function never disagrees with the distance it explains.
pub fn governing_axis(aspect: f32) -> GoverningAxis {
    let a = sanitize_aspect(aspect);
    if 1.0 / a < PITCH_EPS_RAD.cos() {
        GoverningAxis::Horizontal
    } else {
        GoverningAxis::Vertical
    }
}

// ── The framing formula ──────────────────────────────────────────────────────────────

/// The distance from the plane's centre at which a square plane of side `extent`, lying in
/// the world x–z plane and centred at the origin, exactly fills the viewport.
///
/// # The formula
///
/// `Mat4::perspective_rh(fov_y, aspect, near, far)` puts the view half-height at distance
/// `d` at `d·tan(fov/2)`, and the half-width at `aspect · d·tan(fov/2)` — **vertical** FOV
/// is what the engine takes (`world.rs:10564-10567`). The substrate is a backdrop, so this
/// is *cover* framing, not *fit* framing: a plane that merely fits leaves a hole around it.
/// With `h = extent/2` and `t = tan(fov/2)`, covering both axes means
///
/// ```text
///     h ≥ d·t·aspect      (horizontal)
///     h ≥ d·t             (vertical)
/// ```
///
/// Each is an upper bound on `d`, so the **governing** axis is whichever gives the *smaller*
/// bound — the wider one on screen. Landscape is governed horizontally, portrait vertically,
/// and the plane exactly fills the governing axis while overhanging the other:
///
/// ```text
///     d = h / (t · max(1, aspect))            (ideal: view axis ⟂ plane)
/// ```
///
/// # The tilt term, and why it is not optional
///
/// The rig cannot look straight down — see [`PITCH_EPS_RAD`] — so the view axis is `ε` off
/// the plane's normal and the plane is `ε` out of the image plane. With `yaw = 0` the tilt
/// lies purely in the image's vertical direction, and for a ground point `(x, 0, z)`:
///
/// ```text
///     depth = d − z·sin ε        right = x        up = −z·cos ε
/// ```
///
/// so on **both** axes the binding point is the far corner (`z = −h`, the one pushed away),
/// and the two bounds become `d ≤ h·cos ε/t − h·sin ε` and `d ≤ h/(a·t) − h·sin ε`:
///
/// ```text
///     d = (h/t)·min(cos ε, 1/aspect) − h·sin ε
/// ```
///
/// which collapses to the ideal formula at `ε = 0`. The correction is `t·sin ε` relative
/// (plus a negligible `1 − cos ε`): 8.7e-5 at a 10° lens, 1.7e-3 at 120° — well below
/// anything an eye could see either way. It is folded in anyway because "the backdrop covers
/// the viewport" is a guarantee this module either makes or does not.
///
/// # Domain
///
/// Every input is sanitized (see [`sanitize_fov_deg`], [`sanitize_aspect`]); nothing panics
/// and the result is always finite and positive. The coverage guarantee holds wherever
/// neither distance clamp bites — a plane between roughly 1e-3 and 1e14 world units, which
/// contains the specified band with room to spare. Outside that the answer is merely finite
/// and usable, which is the honest promise for an input nothing sane produces.
pub fn frame_distance(extent: f32, fov_deg: f32, aspect: f32) -> f32 {
    let h = 0.5 * sanitize_extent(extent);
    let t = (0.5 * sanitize_fov_deg(fov_deg).to_radians()).tan();
    let a = sanitize_aspect(aspect);
    let (sin_e, cos_e) = PITCH_EPS_RAD.sin_cos();
    // min(cos ε, 1/aspect) — the governing axis, in one term. `governing_axis` names it.
    let governing = cos_e.min(1.0 / a);
    // Cannot go negative: that would need t·tan ε > 1, i.e. a FOV past 179.88°, which
    // `sanitize_fov_deg` has already excluded. Both ends of the clamp are about keeping the
    // *matrices* usable, not about taste — see the two constants.
    (h * governing / t - h * sin_e).clamp(DISTANCE_MIN, DISTANCE_MAX)
}

/// The largest angle between any view ray inside the frustum and the view axis, in degrees —
/// the frustum's **diagonal half-angle**.
///
/// The NDC corner `(±1, ±1)` leaves the eye along camera-space `(±aspect·t, ±t, −1)` with
/// `t = tan(fov/2)`, so
///
/// ```text
///     max deviation = atan( tan(fov/2) · sqrt(1 + aspect²) )
/// ```
///
/// equivalently `atan(sqrt(tan²(fov_v/2) + tan²(fov_h/2)))`.
///
/// **It is a function of both inputs and a constant is always wrong.** A 10° vertical FOV
/// gives ≈10.117° at 16:9, ≈8.295° at 4:3 and ≈5.732° at 9:16 — the same lens, nearly 2×
/// the gradient, decided entirely by the shape of the window.
///
/// Brief R5: on a flat plane with a uniform material this angle is the *entire* budget the
/// shading has to vary over. Quote it with its aspect or do not quote it.
pub fn max_view_deviation_deg(fov_deg: f32, aspect: f32) -> f32 {
    let t = (0.5 * sanitize_fov_deg(fov_deg).to_radians()).tan();
    let a = sanitize_aspect(aspect);
    (t * (1.0 + a * a).sqrt()).atan().to_degrees()
}

/// The **horizontal** field of view implied by a vertical one at this aspect:
/// `2·atan(aspect · tan(fov_v/2))`.
///
/// Companion to [`max_view_deviation_deg`]: it says how much the view vector swings
/// left-to-right, which on a 16:9 console is roughly twice the top-to-bottom swing. Not the
/// same as `fov_v · aspect` — that approximation is only good for narrow lenses, and stops
/// being good exactly where someone would reach for a wide one.
pub fn horizontal_fov_deg(fov_deg: f32, aspect: f32) -> f32 {
    let t = (0.5 * sanitize_fov_deg(fov_deg).to_radians()).tan();
    (2.0 * (sanitize_aspect(aspect) * t).atan()).to_degrees()
}

/// The projection the engine will build, built here — `world.rs:10597`'s call with the same
/// arguments in the same order, degrees in.
///
/// This exists so a test can assert the rig's geometry against **the same matrix the engine
/// makes**, rather than against a re-derivation of it that could be wrong in the same way
/// twice. `near`/`far` are parameters rather than baked so the integrator can preview brief
/// R2's edit 3 (a near plane that follows the rig) without touching this module; pass
/// [`ENGINE_NEAR`] / [`ENGINE_FAR`] for today's engine, as [`SubstrateRig::projection`] does.
pub fn perspective(fov_deg: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    // Both ends clamped finite before the ordering fix-up: `near * 1.000_001` on an
    // unclamped f32::MAX overflows to +inf, and `far / (near - far)` with an infinity in it
    // is NaN — a whole matrix of it, from one bad argument.
    let near = finite_or(near, ENGINE_NEAR).clamp(1.0e-6, 1.0e12);
    let far = finite_or(far, ENGINE_FAR).clamp(1.0e-6, 1.0e18).max(near * 1.000_001);
    Mat4::perspective_rh(
        sanitize_fov_deg(fov_deg).to_radians(),
        sanitize_aspect(aspect),
        near,
        far,
    )
}

// ── The rig ──────────────────────────────────────────────────────────────────────────

/// What the integrator feeds the engine's camera arm.
///
/// Exactly the inputs `build_uniforms` consumes (`world.rs:6480-6524`, `:10551-10598`) and
/// nothing else — no matrices, no eye position, because the engine derives those itself and
/// a second copy would be a second thing to keep in step. Roll is not carried: the substrate
/// is roll-free by construction and [`camera_arm`](Self::camera_arm) supplies the 0.
///
/// Fields are public because the integrator has to read all five to fill the arm. Writing
/// them is allowed and occasionally right (nudging `center` to follow a plane that is not at
/// the origin), but two of them carry guarantees that a hand-edit voids — see
/// [`Self::frame_plane`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubstrateRig {
    /// The look-at target. `Vec3::ZERO` from [`Self::frame_plane`], because the substrate
    /// plane is centred at the origin. Note the engine will fight this if left to itself:
    /// `world.rs:5270` lerps `cam_center` 5%/frame toward the generator field's AABB centre,
    /// so the integrator's camera arm must latch that off (brief R2). A plane at the origin
    /// makes the auto-follow *happen* to converge to the same point — a coincidence worth
    /// naming and not worth leaning on.
    pub center: Vec3,
    /// Radians. Always 0 from [`Self::frame_plane`] — see there for why changing it breaks
    /// coverage.
    pub yaw: f32,
    /// Radians, never ±π/2 — see [`PITCH_EPS_RAD`] for the NaN that guards against.
    pub pitch: f32,
    /// World units from [`Self::center`] to the eye.
    pub distance: f32,
    /// **Vertical** FOV in degrees, as the engine means it (`world.rs:10564-10567`).
    pub fov_deg: f32,
}

impl SubstrateRig {
    /// Frame a square plane of side `extent`, lying in the world x–z plane and centred at
    /// the origin, so that it fills a viewport of `aspect` at vertical FOV `fov_deg`.
    ///
    /// The distance derivation — including which axis governs — is on [`frame_distance`].
    ///
    /// **`yaw` is 0 and that is part of the guarantee.** Yaw spins the plane on screen; a
    /// square rotated inside a rectangle no longer covers it, so a non-zero yaw needs a
    /// bigger plane, not just a different angle. Rotating the *substrate* instead (in Leaf
    /// B's state builder) costs nothing and keeps this exact.
    ///
    /// **Re-frame on resize.** The rig is computed for one aspect; the engine reads its
    /// aspect from the render target every frame (`world.rs:10593`). Call this again rather
    /// than letting the two drift — a stale aspect is exactly the defect brief R1/R4 found
    /// already sitting in the backdrop seam (a window-sized texture painted into a shorter
    /// panel), and on a flat plane it is glaring rather than invisible.
    ///
    /// **The engine's far plane caps the extent.** `distance` must land inside
    /// (`ENGINE_NEAR`, `ENGINE_FAR`) or the plane is framed perfectly and then clipped away.
    /// `d ≤ far` means `extent ≤ 2·far·tan(fov/2)·max(1, aspect)` — about **621** world units
    /// at 4°/16:9 and **1555** at 10°/16:9. Bigger plane, narrower lens, or move `CAM_FAR`;
    /// pick knowingly. Pinned by a test.
    pub fn frame_plane(extent: f32, fov_deg: f32, aspect: f32) -> Self {
        Self {
            center: Vec3::ZERO,
            yaw: 0.0,
            pitch: MAX_PITCH_RAD,
            distance: frame_distance(extent, fov_deg, aspect),
            fov_deg: sanitize_fov_deg(fov_deg),
        }
    }

    /// Where the camera actually is.
    ///
    /// `world.rs:10579-10580`'s formula verbatim (and again at `:6530` for the decoration
    /// eye). Copied rather than approximated: if this and the engine ever disagree, the rig
    /// frames one picture and the engine renders another, with nothing to say so.
    pub fn eye(&self) -> Vec3 {
        let dir = Vec3::new(
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
            self.pitch.cos() * self.yaw.cos(),
        );
        self.center + self.distance * dir
    }

    /// `Mat4::look_at_rh(eye, center, Vec3::Y)` — `build_uniforms` at roll 0
    /// (`world.rs:10584-10592`).
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.center, Vec3::Y)
    }

    /// This rig's projection at `aspect`, over today's engine near/far.
    ///
    /// `aspect` is a parameter, not a field, because the render target owns it. Pass the
    /// same value the rig was framed with or the coverage guarantee is void.
    pub fn projection(&self, aspect: f32) -> Mat4 {
        perspective(self.fov_deg, aspect, ENGINE_NEAR, ENGINE_FAR)
    }

    /// `projection · view` — the composite `build_uniforms` calls `view_proj`
    /// (`world.rs:10598`). Anything injected downstream of that point fights TAA's jitter,
    /// which post-multiplies it (`world.rs:7924-7942`); this is the matrix to reason about.
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view_matrix()
    }

    /// The six-element tuple `world.rs:6497-6524` selects between —
    /// `(cam_center, yaw, pitch, distance, cam_roll, fov_deg)`.
    ///
    /// Shaped for the substrate arm to return directly, so the integrator does not re-order
    /// five floats by hand. Roll is 0: a dutch-tilted backdrop behind a monospace grid is a
    /// different feature, and an accidental one would look like a bug.
    pub fn camera_arm(&self) -> (Vec3, f32, f32, f32, f32, f32) {
        (self.center, self.yaw, self.pitch, self.distance, 0.0, self.fov_deg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEG_TOL: f32 = 0.01;

    fn approx(got: f32, want: f32, tol: f32, what: &str) {
        assert!(
            (got - want).abs() <= tol,
            "{what}: got {got}, want {want} (±{tol})"
        );
    }

    /// NDC (x, y) of a world point through `vp`. The perspective divide is the engine's.
    fn ndc(vp: &Mat4, p: Vec3) -> (f32, f32) {
        let c = *vp * p.extend(1.0);
        assert!(c.w > 0.0, "point behind the eye: w = {}", c.w);
        (c.x / c.w, c.y / c.w)
    }

    /// The tightest corner of the plane on each axis: `(min |ndc.x|, min |ndc.y|)` over the
    /// four corners. ≥ 1 on both axes means the plane covers the viewport; = 1 means it is
    /// tight (no wasted plane) on that axis.
    fn min_abs_ndc(vp: &Mat4, extent: f32) -> (f32, f32) {
        let h = 0.5 * extent;
        let mut mx = f32::INFINITY;
        let mut my = f32::INFINITY;
        for &(sx, sz) in &[(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let (x, y) = ndc(vp, Vec3::new(sx * h, 0.0, sz * h));
            mx = mx.min(x.abs());
            my = my.min(y.abs());
        }
        (mx, my)
    }

    // ── (a) the framing formula ──────────────────────────────────────────────────────

    /// Landscape: width runs out first, so the plane's left/right edges land exactly on the
    /// viewport edge and the top/bottom overhang.
    #[test]
    fn plane_exactly_fills_the_governing_axis_landscape() {
        let aspect = 16.0 / 9.0;
        let extent = 100.0;
        assert_eq!(governing_axis(aspect), GoverningAxis::Horizontal);

        let rig = SubstrateRig::frame_plane(extent, 10.0, aspect);
        let (mx, my) = min_abs_ndc(&rig.view_projection(aspect), extent);
        approx(mx, 1.0, 5.0e-4, "landscape: governing (horizontal) axis is exactly filled");
        assert!(my > 1.0, "landscape: vertical must overhang, got {my}");
        // The overhang is the aspect ratio itself — the plane is square, the window is not.
        approx(my, aspect, 1.0e-3, "landscape: vertical overhang == aspect");
    }

    /// Portrait: the same argument with the axes swapped. This is the case a constant
    /// "landscape wins" rule would silently get wrong.
    #[test]
    fn plane_exactly_fills_the_governing_axis_portrait() {
        let aspect = 9.0 / 16.0;
        let extent = 100.0;
        assert_eq!(governing_axis(aspect), GoverningAxis::Vertical);

        let rig = SubstrateRig::frame_plane(extent, 10.0, aspect);
        let (mx, my) = min_abs_ndc(&rig.view_projection(aspect), extent);
        approx(my, 1.0, 5.0e-4, "portrait: governing (vertical) axis is exactly filled");
        assert!(mx > 1.0, "portrait: horizontal must overhang, got {mx}");
        approx(mx, 1.0 / aspect, 1.0e-3, "portrait: horizontal overhang == 1/aspect");
    }

    /// The guarantee itself, over the whole specified band: the plane always covers the
    /// viewport, and never by more than it has to on the governing axis.
    #[test]
    fn the_plane_covers_the_viewport_across_the_specified_band() {
        let fovs = [4.0f32, 6.0, 10.0, 24.0, 45.0, 90.0, 120.0];
        // 9:16, 3:4, 1:1, 4:3, 16:9, 21:9, plus both ends of the specified aspect band.
        let aspects = [0.1f32, 0.5625, 0.75, 1.0, 1.3333334, 1.7777778, 2.3703704, 10.0];
        let extents = [1.0f32, 100.0, 4096.0];
        for &fov in &fovs {
            for &aspect in &aspects {
                for &extent in &extents {
                    let rig = SubstrateRig::frame_plane(extent, fov, aspect);
                    let (mx, my) = min_abs_ndc(&rig.view_projection(aspect), extent);
                    let tol = 1.0e-3;
                    assert!(
                        mx >= 1.0 - tol && my >= 1.0 - tol,
                        "gap in the backdrop at fov {fov}, aspect {aspect}, extent {extent}: \
                         ndc ({mx}, {my})"
                    );
                    // Tight on the governing axis: no wasted plane, and the proof that
                    // `governing_axis` names the same axis the distance was pinned by.
                    match governing_axis(aspect) {
                        GoverningAxis::Horizontal => assert!(
                            mx <= 1.0 + tol,
                            "horizontal should govern at aspect {aspect} but ndc.x = {mx}"
                        ),
                        GoverningAxis::Vertical => assert!(
                            my <= 1.0 + tol,
                            "vertical should govern at aspect {aspect} but ndc.y = {my}"
                        ),
                    }
                }
            }
        }
    }

    /// The closed form in the doc comment is the closed form in the code, and the tilt it
    /// carries really is the small correction the doc calls it — not something larger hiding
    /// behind a round number.
    #[test]
    fn distance_matches_the_documented_formula() {
        for &(fov, aspect, extent) in &[
            (10.0f32, 16.0 / 9.0f32, 100.0f32),
            (4.0, 16.0 / 9.0, 800.0),
            (45.0, 1.0, 10.0),
            (120.0, 0.5625, 2.0),
        ] {
            let h = 0.5 * extent;
            let t = (0.5 * fov.to_radians()).tan();
            let got = frame_distance(extent, fov, aspect);

            // `d = (h/t)·min(cos ε, 1/aspect) − h·sin ε`, transcribed from the doc comment.
            let (sin_e, cos_e) = PITCH_EPS_RAD.sin_cos();
            let exact = h * cos_e.min(1.0 / aspect) / t - h * sin_e;
            approx(got, exact, exact * 1.0e-6, "the documented closed form");

            // And it is within a fraction of a percent of the ideal `h/(t·max(1, aspect))`
            // the derivation starts from. The gap is `t·max(1,aspect)·sin ε` when the
            // horizontal governs and `(1−cos ε) + t·sin ε` when the vertical does — both
            // sub-1e-3 across the specified band, which is why the tilt is a footnote and
            // not a design constraint.
            let ideal = h / (t * aspect.max(1.0));
            let rel = (got - ideal).abs() / ideal;
            assert!(rel < 2.0e-3, "fov {fov} aspect {aspect}: {got} vs ideal {ideal} ({rel})");
            // The tilt always pulls the camera closer — never further, or the far edge of
            // the plane would uncover.
            assert!(got < ideal, "the tilt term must shorten the distance, not lengthen it");
        }
    }

    /// `d ≤ CAM_FAR` caps the extent a given lens can frame. A plane past the cap is framed
    /// correctly and then clipped away by the engine — a failure that looks like "the
    /// backdrop is black", not like a camera bug.
    #[test]
    fn the_engine_depth_range_caps_the_extent_this_rig_can_frame() {
        let aspect = 16.0f32 / 9.0;
        for &(fov, cap) in &[(4.0f32, 620.8f32), (10.0, 1555.4)] {
            let t = (0.5 * fov.to_radians()).tan();
            let predicted = 2.0 * ENGINE_FAR * t * aspect.max(1.0);
            approx(predicted, cap, 0.5, "documented extent cap");
            assert!(frame_distance(cap * 0.9, fov, aspect) < ENGINE_FAR);
            assert!(frame_distance(cap * 1.1, fov, aspect) > ENGINE_FAR);
            // And the near plane is never the binding end for a substrate worth looking at.
            assert!(frame_distance(cap * 0.9, fov, aspect) > ENGINE_NEAR);
        }
    }

    // ── (b) the deviation bound, as a function of aspect ─────────────────────────────

    /// The figures the brief quotes, and the reason a constant cannot stand in for them.
    #[test]
    fn deviation_bound_is_a_function_of_aspect() {
        // Brief R2 correction #2: ≈10.1° at 10° vertical / 16:9.
        approx(max_view_deviation_deg(10.0, 16.0 / 9.0), 10.1177, DEG_TOL, "10° @ 16:9");
        // A second aspect, so the test cannot pass with the aspect ignored.
        approx(max_view_deviation_deg(10.0, 4.0 / 3.0), 8.2955, DEG_TOL, "10° @ 4:3");
        approx(max_view_deviation_deg(10.0, 9.0 / 16.0), 5.7321, DEG_TOL, "10° @ 9:16");
        // Same lens, 1.76× the gradient, decided entirely by the window's shape.
        assert!(
            max_view_deviation_deg(10.0, 16.0 / 9.0)
                > 1.7 * max_view_deviation_deg(10.0, 9.0 / 16.0)
        );
        // Monotone in aspect, and never below the vertical half-FOV (the square case is
        // still a diagonal).
        let mut prev = 0.0f32;
        for &a in &[0.1f32, 0.5, 1.0, 1.7777778, 4.0, 10.0] {
            let d = max_view_deviation_deg(10.0, a);
            assert!(d > prev, "not monotone in aspect at {a}");
            assert!(d >= 5.0, "the diagonal is never tighter than the vertical half-FOV");
            prev = d;
        }
        // Monotone in FOV too, across the specified band.
        let mut prev = 0.0f32;
        for &f in &[FOV_MIN_DEG, 6.0, 10.0, 45.0, 90.0, FOV_MAX_DEG] {
            let d = max_view_deviation_deg(f, 16.0 / 9.0);
            assert!(d > prev, "not monotone in fov at {f}");
            prev = d;
        }
    }

    /// The bound describes *the matrix the engine builds*, not a parallel derivation of it:
    /// read the projection's own `w`/`h` entries back out, rebuild the corner ray from them,
    /// and measure its angle from the view axis.
    #[test]
    fn deviation_bound_matches_the_projection_it_describes() {
        for &(fov, aspect) in &[
            (4.0f32, 16.0 / 9.0f32),
            (10.0, 16.0 / 9.0),
            (10.0, 9.0 / 16.0),
            (45.0, 1.0),
            (120.0, 2.3703704),
        ] {
            let proj = perspective(fov, aspect, ENGINE_NEAR, ENGINE_FAR);
            // glam's perspective_rh: col0.x = cot(fov/2)/aspect, col1.y = cot(fov/2), and
            // camera space is right-handed with the axis down −Z.
            let w = proj.x_axis.x;
            let h = proj.y_axis.y;
            let corner_ray = Vec3::new(1.0 / w, 1.0 / h, -1.0).normalize();
            let from_matrix = corner_ray.dot(Vec3::NEG_Z).clamp(-1.0, 1.0).acos().to_degrees();
            approx(
                max_view_deviation_deg(fov, aspect),
                from_matrix,
                1.0e-3,
                "analytic bound vs the projection matrix",
            );
        }
    }

    /// The horizontal companion: it is the tangent law, not `fov · aspect`.
    #[test]
    fn horizontal_fov_follows_from_aspect() {
        // 2·atan(16/9 · tan 5°) — a 10° lens is 17.7° wide on a 16:9 console.
        approx(horizontal_fov_deg(10.0, 16.0 / 9.0), 17.6819, DEG_TOL, "10° @ 16:9");
        approx(horizontal_fov_deg(10.0, 1.0), 10.0, DEG_TOL, "square aspect is identity");
        // The naive `fov · aspect` is close for a narrow lens and badly wrong for a wide
        // one — which is exactly where someone would reach for it.
        let naive = 90.0 * 16.0 / 9.0;
        assert!((horizontal_fov_deg(90.0, 16.0 / 9.0) - naive).abs() > 15.0);
        // The corner deviation always exceeds both half-FOVs it is built from.
        for &a in &[0.5625f32, 1.0, 1.7777778] {
            let dev = max_view_deviation_deg(10.0, a);
            assert!(dev > 0.5 * horizontal_fov_deg(10.0, a));
            assert!(dev > 5.0);
        }
    }

    // ── (c) the projection preview ───────────────────────────────────────────────────

    /// Bit-identical to the engine's own call for every in-band input — same function, same
    /// arguments. If this drifts, every geometric assertion above is measuring the wrong
    /// matrix.
    #[test]
    fn projection_preview_matches_glam_perspective_rh() {
        for &(fov, aspect) in &[
            (4.0f32, 16.0 / 9.0f32),
            (10.0, 16.0 / 9.0),
            (45.0, 1.0),
            (120.0, 0.5625),
            (10.0, ASPECT_MIN),
            (10.0, ASPECT_MAX),
            (FOV_MIN_DEG, 1.0),
            (FOV_MAX_DEG, 1.0),
        ] {
            let ours = perspective(fov, aspect, ENGINE_NEAR, ENGINE_FAR);
            let engine = Mat4::perspective_rh(fov.to_radians(), aspect, ENGINE_NEAR, ENGINE_FAR);
            assert_eq!(ours, engine, "fov {fov}, aspect {aspect}");
        }

        let aspect = 16.0 / 9.0;
        let rig = SubstrateRig::frame_plane(100.0, 10.0, aspect);
        assert_eq!(
            rig.projection(aspect),
            Mat4::perspective_rh(10.0f32.to_radians(), aspect, ENGINE_NEAR, ENGINE_FAR)
        );
        // And the view half-height law the framing formula rests on, read off the matrix.
        let t = (0.5f32 * 10.0f32.to_radians()).tan();
        approx(1.0 / rig.projection(aspect).y_axis.y, t, 1.0e-6, "cot(fov/2) inverted");
    }

    /// The engine clamps the FOV to 10°–120° at two sites today. This module must not, or
    /// the 4° rig the integrator is widening the engine *for* would be quietly framed at 10°
    /// and nobody would see a wrong number — only a wrong picture.
    #[test]
    fn the_engine_fov_floor_is_not_baked_in_here() {
        let rig = SubstrateRig::frame_plane(100.0, 4.0, 16.0 / 9.0);
        assert_eq!(rig.fov_deg, 4.0);
        assert!(rig.fov_deg < ENGINE_FOV_CLAMP_DEG.0);
        // A narrower lens frames from further away — monotone, no clamp plateau anywhere in
        // the specified band.
        let mut prev = f32::INFINITY;
        for &fov in &[FOV_MIN_DEG, 6.0, 10.0, 20.0, 45.0, 90.0, FOV_MAX_DEG] {
            let d = frame_distance(100.0, fov, 16.0 / 9.0);
            assert!(d < prev, "distance must fall as the lens widens (fov {fov})");
            prev = d;
        }
    }

    // ── (d) degeneracy: clamps, not panics ───────────────────────────────────────────

    /// The NaN this rig exists to avoid, demonstrated rather than asserted on faith.
    #[test]
    fn the_rig_never_emits_the_degenerate_top_down_pitch() {
        let rig = SubstrateRig::frame_plane(100.0, 10.0, 16.0 / 9.0);
        assert!(rig.pitch < FRAC_PI_2 && rig.pitch > 0.0);
        assert_eq!(rig.pitch, MAX_PITCH_RAD);
        assert!(rig.view_matrix().is_finite());
        assert!(rig.view_projection(16.0 / 9.0).is_finite());

        // Straight down, with the plane centred away from the origin: the eye's horizontal
        // offset from the centre is d·cos(π/2) ≈ −1.4e-5, which rounds clean away against a
        // coordinate of 1000 (f32 spacing there is 6.1e-5). eye and center then differ by a
        // vector exactly parallel to Vec3::Y, look_at_rh normalizes a zero cross product,
        // and the entire view matrix is NaN. Not theoretical — one `cam_center` away.
        let degenerate = SubstrateRig {
            center: Vec3::new(0.0, 0.0, 1000.0),
            pitch: FRAC_PI_2,
            ..rig
        };
        assert!(
            !degenerate.view_matrix().is_finite(),
            "the degenerate case must really be degenerate, or this guard is cargo cult"
        );
    }

    /// Every combination of hostile input produces a usable rig and finite matrices.
    #[test]
    fn degenerate_inputs_clamp_rather_than_panic() {
        let hostile = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.0,
            -0.0,
            -1.0,
            -1.0e30,
            1.0e30,
            f32::MIN_POSITIVE,
            f32::MAX,
        ];
        for &extent in &hostile {
            for &fov in &hostile {
                for &aspect in &hostile {
                    let rig = SubstrateRig::frame_plane(extent, fov, aspect);
                    let what = format!("extent {extent}, fov {fov}, aspect {aspect}");
                    assert!(rig.distance.is_finite() && rig.distance > 0.0, "{what}: distance");
                    assert!(rig.distance <= DISTANCE_MAX, "{what}: distance cap");
                    assert!(rig.fov_deg.is_finite() && rig.fov_deg > 0.0, "{what}: fov");
                    assert!(rig.pitch.abs() < FRAC_PI_2, "{what}: pitch");
                    assert!(rig.center.is_finite() && rig.eye().is_finite(), "{what}: eye");
                    assert!(rig.view_matrix().is_finite(), "{what}: view");
                    assert!(rig.view_projection(aspect).is_finite(), "{what}: view_proj");
                    assert!(max_view_deviation_deg(fov, aspect).is_finite(), "{what}: deviation");
                    assert!(horizontal_fov_deg(fov, aspect).is_finite(), "{what}: h-fov");
                    assert!(perspective(fov, aspect, extent, fov).is_finite(), "{what}: proj");
                }
            }
        }
    }

    /// The extremes of the *specified* band — where the answer must be right, not merely
    /// finite.
    #[test]
    fn extremes_of_the_specified_band_stay_finite_and_correct() {
        for &aspect in &[ASPECT_MIN, 1.0, ASPECT_MAX] {
            for &fov in &[FOV_MIN_DEG, 45.0, FOV_MAX_DEG] {
                for &extent in &[1.0e-3f32, 1.0, 1.0e6, 1.0e12] {
                    let rig = SubstrateRig::frame_plane(extent, fov, aspect);
                    assert!(rig.distance.is_finite() && rig.distance > 0.0);
                    assert!(rig.eye().is_finite());
                    assert!(rig.view_projection(aspect).is_finite());
                    // Coverage holds wherever the DISTANCE_MIN floor did not bite — a
                    // sub-millimetre plane at a 120° lens is pushed back to a distance it
                    // cannot fill, and the doc says so rather than the test pretending
                    // otherwise. (Only one combination here reaches it: 1e-3 at 120°/10:1.)
                    if rig.distance > DISTANCE_MIN && rig.distance < DISTANCE_MAX {
                        let (mx, my) = min_abs_ndc(&rig.view_projection(aspect), extent);
                        assert!(
                            mx >= 1.0 - 1.0e-2 && my >= 1.0 - 1.0e-2,
                            "fov {fov}, aspect {aspect}, extent {extent}: ndc ({mx}, {my})"
                        );
                    }
                }
            }
        }
    }

    /// The sanitizers' documented behaviour, stated once so the doc comments are checkable.
    #[test]
    fn sanitizers_do_what_their_docs_say() {
        assert_eq!(sanitize_fov_deg(f32::NAN), FOV_FALLBACK_DEG);
        assert_eq!(sanitize_fov_deg(10.0), 10.0);
        assert_eq!(sanitize_fov_deg(4.0), 4.0);
        assert_eq!(sanitize_fov_deg(1000.0), FOV_HARD_MAX_DEG);
        assert_eq!(sanitize_fov_deg(-5.0), FOV_HARD_MIN_DEG);
        assert_eq!(sanitize_aspect(f32::NAN), 1.0);
        assert_eq!(sanitize_aspect(0.0), ASPECT_HARD_MIN, "matches world.rs:10593's floor");
        assert_eq!(sanitize_aspect(1.7777778), 1.7777778);
        assert_eq!(sanitize_extent(-100.0), 100.0, "a negative extent is a sign slip");
        assert_eq!(sanitize_extent(f32::INFINITY), 1.0);
    }

    // ── the integration point ────────────────────────────────────────────────────────

    /// `camera_arm` hands back the six the engine's arm selects, in the engine's order, with
    /// roll zeroed.
    #[test]
    fn camera_arm_carries_the_engine_tuple() {
        let rig = SubstrateRig::frame_plane(100.0, 10.0, 16.0 / 9.0);
        let (center, yaw, pitch, distance, roll, fov) = rig.camera_arm();
        assert_eq!(center, rig.center);
        assert_eq!(yaw, rig.yaw);
        assert_eq!(pitch, rig.pitch);
        assert_eq!(distance, rig.distance);
        assert_eq!(roll, 0.0);
        assert_eq!(fov, rig.fov_deg);
        // The eye the engine will compute from that tuple is above the plane, looking down.
        let eye = rig.eye();
        assert!(eye.y > 0.0 && eye.y > 100.0);
        assert!(eye.x.abs() < 1.0e-3 && eye.z.abs() < 1.0, "essentially straight above");
    }
}
