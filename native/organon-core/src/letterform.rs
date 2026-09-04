//! # letterform — a real glyph outline as an extruded, bevelled mesh
//!
//! **organon#217 T7, phase one: the geometry only.** `ab_glyph` outlines → flattened
//! contours → a tessellated cap → a bevel band → an extruded side wall → a cached mesh
//! atlas. Plain `Vec`s of vertices and indices. **No `wgpu`, no buffers, no draw** — and
//! nothing in the renderer changes because of this module. `doc/pbr_text_engine.md` §14
//! says in as many words that T7 is not a prerequisite for anything before it; this file
//! is the geometry that a later tier may adopt, not the adoption.
//!
//! ## Why it is in `organon-core`, and behind a feature
//!
//! Core's acceptance test is `cargo tree -p organon-core` — no `nih_plug`, no `wgpu`, no
//! `egui`. A glyph outline is file parsing plus arithmetic, exactly the shape of [`gguf`]
//! and [`lora`], so it belongs here on the merits. But `ab_glyph` is a *seventh*
//! dependency for a crate whose whole point is a short dependency list, so the module is
//! behind the **optional `letterform` feature** and the default build does not compile
//! it. ⚠️ That cuts both ways: a green `cargo test -p organon-core` says **nothing** about
//! this file. Run it with `--features letterform` as well.
//!
//! [`gguf`]: crate::gguf
//! [`lora`]: crate::lora
//!
//! ## The units, once, so nothing downstream has to guess
//!
//! **Everything in this module is in em units.** A font's outline arrives in *font units*
//! (0..`units_per_em`, typically 1000 for CFF and 2048 for TrueType); [`glyph_contours`]
//! divides by `units_per_em` at the boundary and nothing below it ever sees a font unit
//! again. So [`LetterformParams::tolerance`], [`LetterformParams::depth`] and
//! [`LetterformParams::bevel`] are all fractions of an em, and a parameter set that looks
//! right for one font looks right for the next. y is **up**, matching the font's own
//! convention; the extrusion runs along **+z toward the viewer**.
//!
//! ## The six things that are easy to get wrong here
//!
//! 1. **Contours and holes.** `ab_glyph` hands back a *flat list* of curves with no
//!    contour delimiter — see [`glyph_contours`] for how a contour is recovered from it.
//!    The counter of an `o` is a second contour wound the other way, and it stays a hole
//!    because the cap is filled under the **non-zero winding rule** (§ *The cap*).
//!    🚨 **And which side of a contour the material is on cannot be read off that
//!    contour's winding** — the trap that cost the first draft of this module, and the one
//!    worth carrying away. See [`Ring`].
//! 2. **Self-intersection and degenerate contours.** Real fonts are full of both, and an
//!    ear-clipping triangulator either refuses them or folds them silently. This module
//!    does not triangulate a boundary at all: it *samples the fill rule* (§ *The cap*),
//!    which cannot refuse an input and has no notion of a "valid" polygon.
//! 3. **The bevel is the point.** A flat letter has one normal and therefore one specular
//!    answer. The bevel band's normals are **generated from the bevel's own geometry** —
//!    the outward miter direction blended with the chamfer's rise — never copied from the
//!    cap. See [`build_mesh`].
//! 4. **Flattening tolerance** is in **em units** (above), and it is a real distance
//!    bound: `flatten_*` subdivides until the polyline is within `tolerance` of the true
//!    curve, and `flattening_stays_within_tolerance` measures that against a dense
//!    sampling of the curve rather than trusting the subdivision predicate.
//! 5. **The cache key** carries every parameter that changes the mesh
//!    ([`MeshKey::new`]). A parameter left out of it is not a slow cache, it is a *wrong*
//!    mesh served silently; `a_key_missing_the_bevel_serves_the_wrong_mesh` proves that
//!    negative by building the broken key and watching it collide.
//! 6. **§9 law 1 — the cell's energy stays in the cell.** A block tile is clipped to its
//!    cell by construction; a letterform is not. In **x/y** this module can only ever
//!    shrink the silhouette — the bevel *insets*, so the mesh never exceeds the glyph's
//!    own outline bounds — but *those bounds are the font's business and routinely leave
//!    the em square*: descenders, overshoot, negative side bearings, italic overhang. In
//!    **z** the mesh spans exactly `±depth/2`, which is ours to bound. So the honest
//!    statement is: **z is bounded by a parameter, x/y is bounded by the font**, and a
//!    consumer that needs the cell law must measure [`GlyphMesh::bounds`] against its cell
//!    and either scale or accept the overhang. `glyph_bounds_can_leave_the_em_square`
//!    measures it on a real font rather than asserting it.
//!
//! ## The cap: a scanline, not a triangulator
//!
//! [`tessellate_fill`] cuts the plane into horizontal bands at every contour vertex's `y`
//! **and at every self-intersection's `y`**, then evaluates the non-zero winding rule
//! once per band at the band's midpoint. Inside each band no vertex and no crossing
//! exists, so every edge that enters the band spans it completely and each filled span is
//! an exact trapezoid — the tessellation reproduces the polygon exactly rather than
//! approximating it.
//!
//! Non-zero rather than even-odd is deliberate and it is not a taste call: composite
//! glyphs (an accented letter is usually two overlapping components) fill solid under
//! non-zero and punch a hole through themselves under even-odd.
//!
//! ⚠️ **The wall follows the source contours, and that is where self-intersection still
//! shows.** The cap is exact under any input, but a contour that crosses itself produces
//! a wall segment *inside* the solid. For an opaque material it is invisible; under glass
//! or with a normal-driven effect it is not. Resolving it needs a real boolean on the
//! filled region, which is phase two.
//!
//! ## What phase one deliberately does not do
//!
//! Vertices are **not welded** — every triangle carries its own three, so the vertex
//! count is exactly `3 × triangles`. Welding needs a spatial hash keyed on position *and*
//! normal, and getting it wrong smooths a corner that should be sharp. The counts below
//! are reported honestly ([`MeshStats`]) so a later tier can see what welding would buy.

use std::collections::HashMap;
use std::collections::hash_map::Entry as MapEntry;

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

/// Default curve-flattening tolerance, **in em units**.
///
/// At a 200x80 cell grid on a 2160-line display a cell is ~27 px tall, so one em is at
/// most ~27 px and `0.005` em is ~0.14 px — already below what any anti-aliaser can see.
/// It is the default because it is cheap (see the tolerance table in
/// `doc/pbr_text_engine.md` §16) and because the grid case is not the only case: a single
/// letter across the screen is the other, and there the same number is ~5 px, which is
/// still fine for a bevel highlight.
pub const DEFAULT_TOLERANCE: f32 = 0.005;

/// The tightest tolerance accepted. Below this, subdivision cost grows without changing
/// anything a renderer can show, so the value is clamped rather than honoured.
pub const MIN_TOLERANCE: f32 = 1.0e-5;

/// How far a miter may run past the offset distance at a sharp corner before it is
/// clamped. Unclamped, `1/sin(theta/2)` goes to infinity as a corner closes, and a
/// letterform is full of near-closed corners (the apex of an `A`, the spur of a `G`).
pub const MITER_LIMIT: f32 = 4.0;

/// Above this edge count the pairwise self-intersection scan (`O(E^2)`) is skipped and
/// only vertex `y` values become scanlines. See [`MeshStats::intersection_scan`].
const INTERSECTION_SCAN_MAX_EDGES: usize = 2000;

/// The shape of one extruded letter, in em units.
///
/// Every field here changes the mesh, so every field here is in [`MeshKey`]. That is not
/// a coincidence to be maintained by hand — `mesh_key_covers_every_shape_parameter`
/// fails if the two ever disagree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LetterformParams {
    /// Total front-to-back extrusion, em units. `0.0` gives a flat, double-sided cap.
    pub depth: f32,
    /// Chamfer size, em units. The bevel is 45 degrees: it insets the face by `bevel`
    /// and drops it by `bevel`. Clamped to `depth / 2` so the two bevels cannot cross.
    pub bevel: f32,
    /// Maximum distance a flattened polyline may deviate from the true curve, em units.
    pub tolerance: f32,
}

impl Default for LetterformParams {
    fn default() -> Self {
        Self { depth: 0.15, bevel: 0.02, tolerance: DEFAULT_TOLERANCE }
    }
}

impl LetterformParams {
    /// Clamp into the range the builder can actually honour.
    ///
    /// Non-finite input is not an error the caller has to handle — it becomes the
    /// default for that field, because a NaN reaching the scanline turns into a mesh of
    /// NaN vertices that looks like a renderer bug three layers away.
    pub fn sanitised(self) -> Self {
        let d = if self.depth.is_finite() && self.depth >= 0.0 { self.depth } else { 0.0 };
        let b = if self.bevel.is_finite() && self.bevel >= 0.0 { self.bevel } else { 0.0 };
        let t = if self.tolerance.is_finite() && self.tolerance > 0.0 {
            self.tolerance
        } else {
            DEFAULT_TOLERANCE
        };
        Self { depth: d, bevel: b.min(d * 0.5), tolerance: t.max(MIN_TOLERANCE) }
    }
}

// ---------------------------------------------------------------------------
// Mesh
// ---------------------------------------------------------------------------

/// One mesh vertex: position and normal, both in em units / unit length.
///
/// No UVs and no tangent. A PBR shader that needs a tangent frame for a normal map can
/// derive one from the cap plane, and phase one has no texture to map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

/// Axis-aligned bounds of a mesh, em units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds3 {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds3 {
    /// The empty bounds — `min` above `max`, so the first `expand` replaces both.
    pub fn empty() -> Self {
        Self { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3] }
    }

    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0]
    }

    fn expand(&mut self, p: [f32; 3]) {
        for i in 0..3 {
            if p[i] < self.min[i] {
                self.min[i] = p[i];
            }
            if p[i] > self.max[i] {
                self.max[i] = p[i];
            }
        }
    }

    /// Does this mesh fit inside a cell of the given half-extents, centred on the origin?
    ///
    /// The §9 law-1 question, asked in the only form that has an answer: a *number*
    /// compared against a cell. An empty mesh fits anything.
    pub fn fits_cell(&self, half_w: f32, half_h: f32, half_d: f32) -> bool {
        if self.is_empty() {
            return true;
        }
        self.min[0] >= -half_w
            && self.max[0] <= half_w
            && self.min[1] >= -half_h
            && self.max[1] <= half_h
            && self.min[2] >= -half_d
            && self.max[2] <= half_d
    }
}

/// What the builder had to do to the input, reported rather than hidden.
///
/// Every field here is something a real font caused at least once. A silent drop is the
/// failure mode this struct exists to prevent: a glyph that comes out subtly wrong with
/// a green build and no line anywhere saying which contour went missing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MeshStats {
    /// Contours that survived cleaning and contributed geometry.
    pub contours: usize,
    /// Contours dropped as degenerate: fewer than three distinct points, or an absolute
    /// signed area below the area epsilon. Both occur in shipped fonts.
    pub dropped_contours: usize,
    /// Contours whose bevel had to be reduced (possibly to zero) because the inset
    /// over-ran the contour and turned it inside out. A thin stem is the usual cause.
    pub bevel_reduced_contours: usize,
    /// Points after flattening and cleaning, summed over contours.
    pub points: usize,
    /// Self-intersections found among the inset contour edges. Non-zero is normal.
    pub self_intersections: usize,
    /// Whether the pairwise intersection scan ran. `false` means the glyph exceeded
    /// [`INTERSECTION_SCAN_MAX_EDGES`] and the cap may be approximate where edges cross
    /// inside a band. Reported because "approximate" must never be indistinguishable
    /// from "exact".
    pub intersection_scan: bool,
    pub cap_triangles: usize,
    pub bevel_triangles: usize,
    pub wall_triangles: usize,
}

impl MeshStats {
    pub fn triangles(&self) -> usize {
        self.cap_triangles + self.bevel_triangles + self.wall_triangles
    }
}

/// A finished letterform: vertices, indices, bounds, and what it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub bounds: Bounds3,
    pub stats: MeshStats,
}

impl GlyphMesh {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    fn empty(stats: MeshStats) -> Self {
        Self { vertices: Vec::new(), indices: Vec::new(), bounds: Bounds3::empty(), stats }
    }
}

// ---------------------------------------------------------------------------
// Contour cleaning and geometry helpers
// ---------------------------------------------------------------------------

/// Two points closer than this (em units) are the same point.
const POINT_EPS: f32 = 1.0e-6;

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn len2(v: [f32; 2]) -> f32 {
    v[0] * v[0] + v[1] * v[1]
}

fn normalise2(v: [f32; 2]) -> Option<[f32; 2]> {
    let l = len2(v).sqrt();
    if l > 1.0e-12 { Some([v[0] / l, v[1] / l]) } else { None }
}

fn normalise3(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1.0e-12 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 0.0, 1.0] }
}

/// Twice the signed area, positive for counter-clockwise in a y-up frame.
pub fn signed_area(pts: &[[f32; 2]]) -> f32 {
    if pts.len() < 3 {
        return 0.0;
    }
    let mut a = 0.0f64;
    for i in 0..pts.len() {
        let p = pts[i];
        let q = pts[(i + 1) % pts.len()];
        a += p[0] as f64 * q[1] as f64 - q[0] as f64 * p[1] as f64;
    }
    (a * 0.5) as f32
}

/// One contour that survived cleaning, plus which side of it the material is on.
///
/// The winding is carried rather than normalised away. Normalising every contour to
/// counter-clockwise would make the outer ring and the counter of an `o` wind the same
/// way, whereupon the non-zero rule fills the counter — the exact bug this module has to
/// not have.
///
/// 🚨 **`material_left` cannot be derived from this contour's own winding**, which is the
/// trap that cost the first draft of this module. The obvious rule — "the interior is
/// left of travel for a counter-clockwise ring, right for a clockwise one" — is right for
/// an *isolated* ring and wrong for a nested one, and the two cases are the same
/// clockwise ring: an isolated clockwise square has its material on the **right**, while
/// a clockwise counter inside a counter-clockwise `O` has material on its **left**. What
/// decides it is the winding number of the whole ring *set* on either side, so that is
/// what [`material_is_left`] measures.
#[derive(Debug, Clone)]
struct Ring {
    pts: Vec<[f32; 2]>,
    /// `+1.0` when the filled region lies to the left of travel, `-1.0` when to the right.
    s: f32,
}

/// The non-zero winding number of a whole ring set at a point.
fn winding_at(rings: &[Vec<[f32; 2]>], p: [f32; 2]) -> i32 {
    let mut w = 0i32;
    for r in rings {
        let n = r.len();
        for i in 0..n {
            let a = r[i];
            let b = r[(i + 1) % n];
            let is_left =
                (b[0] - a[0]) as f64 * (p[1] - a[1]) as f64 - (p[0] - a[0]) as f64 * (b[1] - a[1]) as f64;
            if a[1] <= p[1] {
                if b[1] > p[1] && is_left > 0.0 {
                    w += 1;
                }
            } else if b[1] <= p[1] && is_left < 0.0 {
                w -= 1;
            }
        }
    }
    w
}

/// Which side of `ring` the filled region is on, measured against every ring at once.
///
/// Samples a point a hair to either side of the middle of the ring's longest edge and
/// asks the non-zero rule. The probe distance shrinks on a tie, because a thin stem can
/// be narrower than a fixed epsilon and would then report the same answer on both sides.
fn material_is_left(ring: &[[f32; 2]], all: &[Vec<[f32; 2]>], diag: f32) -> Option<bool> {
    let n = ring.len();
    let (mut best_i, mut best_l) = (0usize, -1.0f32);
    for i in 0..n {
        let l = len2(sub(ring[(i + 1) % n], ring[i]));
        if l > best_l {
            best_l = l;
            best_i = i;
        }
    }
    let a = ring[best_i];
    let b = ring[(best_i + 1) % n];
    let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let ln = normalise2([-(b[1] - a[1]), b[0] - a[0]])?;
    let mut delta = (diag * 1.0e-4).max(1.0e-9);
    for _ in 0..12 {
        let left = winding_at(all, [mid[0] + ln[0] * delta, mid[1] + ln[1] * delta]);
        let right = winding_at(all, [mid[0] - ln[0] * delta, mid[1] - ln[1] * delta]);
        if left != right {
            return Some(left != 0);
        }
        delta *= 0.5;
    }
    None
}

/// Drop repeated points and reject a contour that cannot bound an area.
///
/// ⚠️ **The closing point matters.** `ab_glyph`'s `close()` emits a `Line(last, first)`
/// even when `last == first`, so a zero-length final edge arrives on nearly every glyph.
/// Left in, the edge direction is `(0,0)`, the miter normal is a NaN, and the NaN
/// propagates through the whole ring.
///
/// ⚠️ **A contour is NOT rejected for having zero signed area**, which the first draft did
/// and which is wrong in a way that only a self-intersecting input reveals: a
/// figure-eight with equal lobes has a signed area of exactly zero and a *filled* area of
/// twice one lobe, because under the non-zero rule both lobes have winding `±1`. Rejecting
/// it would silently delete a contour that a rasteriser draws. Degeneracy is therefore
/// tested as "no perpendicular extent at all" — every point on one line — which is what
/// actually cannot bound anything.
fn clean_contour(raw: &[[f32; 2]]) -> Option<Vec<[f32; 2]>> {
    let mut pts: Vec<[f32; 2]> = Vec::with_capacity(raw.len());
    for &p in raw {
        if !p[0].is_finite() || !p[1].is_finite() {
            continue;
        }
        if let Some(&last) = pts.last() {
            if len2(sub(p, last)) <= POINT_EPS * POINT_EPS {
                continue;
            }
        }
        pts.push(p);
    }
    // The ring is implicitly closed, so a final point equal to the first is a duplicate.
    while pts.len() >= 2 {
        let first = pts[0];
        let last = *pts.last().unwrap();
        if len2(sub(last, first)) <= POINT_EPS * POINT_EPS {
            pts.pop();
        } else {
            break;
        }
    }
    if pts.len() < 3 {
        return None;
    }
    if is_collinear(&pts) {
        return None;
    }
    Some(pts)
}

/// Every point on one line, to within [`POINT_EPS`] — a contour with no area to fill and
/// no side to have a normal on, whatever its bounding box looks like.
fn is_collinear(pts: &[[f32; 2]]) -> bool {
    let a = pts[0];
    let (mut far, mut far_l) = (a, 0.0f32);
    for &p in pts {
        let l = len2(sub(p, a));
        if l > far_l {
            far_l = l;
            far = p;
        }
    }
    if far_l <= POINT_EPS * POINT_EPS {
        return true;
    }
    pts.iter().all(|&p| dist_to_chord(p, a, far) <= POINT_EPS)
}

/// Move every vertex `distance` toward the filled side, mitred so the offset ring stays
/// parallel to the original; also returns the unit **outward** direction at each vertex.
///
/// The direction is `s * left_normal`, and `s` — measured by [`material_is_left`], not
/// inferred from this ring's winding — is what makes one rule serve both outline
/// conventions and both nesting depths. TrueType winds its outer contours one way and CFF
/// the other, and a counter winds opposite to whichever its outer ring uses; a rule
/// phrased as "move left" shrinks some of those four cases and grows the others.
fn inset_ring(ring: &Ring, distance: f32) -> (Vec<[f32; 2]>, Vec<[f32; 2]>) {
    let n = ring.pts.len();
    let mut out = Vec::with_capacity(n);
    let mut outward = Vec::with_capacity(n);
    // Per-edge unit left normals, edge i running pts[i] -> pts[i+1].
    let mut edge_n: Vec<[f32; 2]> = Vec::with_capacity(n);
    for i in 0..n {
        let d = sub(ring.pts[(i + 1) % n], ring.pts[i]);
        let ln = normalise2([-d[1], d[0]]).unwrap_or([0.0, 0.0]);
        edge_n.push(ln);
    }
    for i in 0..n {
        let prev = edge_n[(i + n - 1) % n];
        let next = edge_n[i];
        // Bisector of the two edge normals, in the "left" frame.
        let bis = normalise2([prev[0] + next[0], prev[1] + next[1]]).unwrap_or(next);
        // 1/cos(half-angle) between the bisector and either edge normal.
        let c = bis[0] * next[0] + bis[1] * next[1];
        let scale = if c > 1.0 / MITER_LIMIT { 1.0 / c } else { MITER_LIMIT };
        let dir = [ring.s * bis[0], ring.s * bis[1]];
        out.push([
            ring.pts[i][0] + dir[0] * distance * scale,
            ring.pts[i][1] + dir[1] * distance * scale,
        ]);
        outward.push([-dir[0], -dir[1]]);
    }
    (out, outward)
}

// ---------------------------------------------------------------------------
// Curve flattening
// ---------------------------------------------------------------------------

/// Deepest subdivision. `2^14` segments per curve is far past any font's needs and is
/// here only so a pathological control polygon cannot recurse forever.
const MAX_FLATTEN_DEPTH: u32 = 14;

fn dist_to_chord(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = sub(b, a);
    let ap = sub(p, a);
    let l = len2(ab).sqrt();
    if l <= 1.0e-12 {
        return len2(ap).sqrt();
    }
    ((ab[0] * ap[1] - ab[1] * ap[0]) / l).abs()
}

/// Append the flattening of a quadratic Bezier, excluding `p0` and including `p2`.
///
/// The flatness test is the exact one for a quadratic: the curve's greatest deviation
/// from its chord is a quarter of the control point's deviation from it.
pub fn flatten_quad(p0: [f32; 2], c: [f32; 2], p2: [f32; 2], tol: f32, out: &mut Vec<[f32; 2]>) {
    flatten_quad_at(p0, c, p2, tol, 0, out);
}

fn flatten_quad_at(
    p0: [f32; 2],
    c: [f32; 2],
    p2: [f32; 2],
    tol: f32,
    depth: u32,
    out: &mut Vec<[f32; 2]>,
) {
    if depth >= MAX_FLATTEN_DEPTH || dist_to_chord(c, p0, p2) * 0.25 <= tol {
        out.push(p2);
        return;
    }
    let m = |a: [f32; 2], b: [f32; 2]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let a = m(p0, c);
    let b = m(c, p2);
    let mid = m(a, b);
    flatten_quad_at(p0, a, mid, tol, depth + 1, out);
    flatten_quad_at(mid, b, p2, tol, depth + 1, out);
}

/// Append the flattening of a cubic Bezier, excluding `p0` and including `p3`.
///
/// The bound is `3/4` of the larger control-point deviation, which is the standard
/// conservative bound for a cubic — the curve stays inside the convex hull of the control
/// polygon and the hull's own deviation is at most that.
pub fn flatten_cubic(
    p0: [f32; 2],
    c1: [f32; 2],
    c2: [f32; 2],
    p3: [f32; 2],
    tol: f32,
    out: &mut Vec<[f32; 2]>,
) {
    flatten_cubic_at(p0, c1, c2, p3, tol, 0, out);
}

fn flatten_cubic_at(
    p0: [f32; 2],
    c1: [f32; 2],
    c2: [f32; 2],
    p3: [f32; 2],
    tol: f32,
    depth: u32,
    out: &mut Vec<[f32; 2]>,
) {
    let d = dist_to_chord(c1, p0, p3).max(dist_to_chord(c2, p0, p3));
    if depth >= MAX_FLATTEN_DEPTH || d * 0.75 <= tol {
        out.push(p3);
        return;
    }
    let m = |a: [f32; 2], b: [f32; 2]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let a = m(p0, c1);
    let b = m(c1, c2);
    let c = m(c2, p3);
    let ab = m(a, b);
    let bc = m(b, c);
    let mid = m(ab, bc);
    flatten_cubic_at(p0, a, ab, mid, tol, depth + 1, out);
    flatten_cubic_at(mid, bc, c, p3, tol, depth + 1, out);
}

// ---------------------------------------------------------------------------
// The cap: non-zero scanline trapezoidation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Edge {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Edge {
    fn x_at(&self, y: f64) -> f64 {
        let dy = self.y1 - self.y0;
        if dy.abs() < 1.0e-18 { self.x0 } else { self.x0 + (y - self.y0) * (self.x1 - self.x0) / dy }
    }
}

/// Fill a set of closed rings under the **non-zero winding rule**, returning triangles in
/// the xy plane wound counter-clockwise when seen from `+z`.
///
/// This is a *sampler*, not a triangulator: it never asks whether the input is a valid
/// polygon, so a self-intersecting contour, a doubled contour and a contour with a
/// zero-area spur all produce the filled region the rasteriser would have produced. The
/// price is triangle count — a band per distinct `y` rather than one triangle per ear.
///
/// Returns `(triangles, self_intersections, scan_ran)`.
fn tessellate_fill(rings: &[Vec<[f32; 2]>]) -> (Vec<[[f32; 2]; 3]>, usize, bool) {
    let mut edges: Vec<Edge> = Vec::new();
    for r in rings {
        let n = r.len();
        for i in 0..n {
            let a = r[i];
            let b = r[(i + 1) % n];
            edges.push(Edge { x0: a[0] as f64, y0: a[1] as f64, x1: b[0] as f64, y1: b[1] as f64 });
        }
    }
    if edges.is_empty() {
        return (Vec::new(), 0, true);
    }

    let mut ys: Vec<f64> = Vec::with_capacity(edges.len() * 2);
    for e in &edges {
        ys.push(e.y0);
        ys.push(e.y1);
    }

    // Every self-intersection is a `y` at which the span structure changes without any
    // vertex saying so. Missing one does not make the cap fail; it makes it quietly wrong
    // inside one band, which is worse.
    let mut crossings = 0usize;
    let scan_ran = edges.len() <= INTERSECTION_SCAN_MAX_EDGES;
    if scan_ran {
        for i in 0..edges.len() {
            for j in (i + 1)..edges.len() {
                if let Some(y) = segment_cross_y(&edges[i], &edges[j]) {
                    crossings += 1;
                    ys.push(y);
                }
            }
        }
    }

    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let band_eps = 1.0e-9f64;
    let mut bands: Vec<f64> = Vec::with_capacity(ys.len());
    for y in ys {
        if bands.last().map_or(true, |&l| y - l > band_eps) {
            bands.push(y);
        }
    }

    let mut tris: Vec<[[f32; 2]; 3]> = Vec::new();
    let mut xs: Vec<(f64, f64, f64, i32)> = Vec::new();
    for w in bands.windows(2) {
        let (y0, y1) = (w[0], w[1]);
        if y1 - y0 <= band_eps {
            continue;
        }
        let ym = 0.5 * (y0 + y1);
        xs.clear();
        for e in &edges {
            // Half-open in y so a vertex is counted by exactly one of its two edges.
            let below0 = e.y0 <= ym;
            let below1 = e.y1 <= ym;
            if below0 == below1 {
                continue;
            }
            let dir = if e.y1 > e.y0 { 1 } else { -1 };
            xs.push((e.x_at(ym), e.x_at(y0), e.x_at(y1), dir));
        }
        if xs.len() < 2 {
            continue;
        }
        xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut winding = 0i32;
        for k in 0..xs.len() - 1 {
            winding += xs[k].3;
            if winding == 0 {
                continue;
            }
            let (_, a0, a1, _) = xs[k];
            let (_, b0, b1, _) = xs[k + 1];
            push_trapezoid(&mut tris, a0, b0, y0, a1, b1, y1);
        }
    }
    (tris, crossings, scan_ran)
}

/// The `y` at which two segments cross, strictly inside both, if they do.
fn segment_cross_y(a: &Edge, b: &Edge) -> Option<f64> {
    let r = (a.x1 - a.x0, a.y1 - a.y0);
    let s = (b.x1 - b.x0, b.y1 - b.y0);
    let denom = r.0 * s.1 - r.1 * s.0;
    if denom.abs() < 1.0e-18 {
        return None;
    }
    let qp = (b.x0 - a.x0, b.y0 - a.y0);
    let t = (qp.0 * s.1 - qp.1 * s.0) / denom;
    let u = (qp.0 * r.1 - qp.1 * r.0) / denom;
    let eps = 1.0e-9;
    if t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps {
        Some(a.y0 + t * r.1)
    } else {
        None
    }
}

fn push_trapezoid(
    tris: &mut Vec<[[f32; 2]; 3]>,
    a0: f64,
    b0: f64,
    y0: f64,
    a1: f64,
    b1: f64,
    y1: f64,
) {
    let w_eps = 1.0e-9;
    if (b0 - a0) <= w_eps && (b1 - a1) <= w_eps {
        return;
    }
    let p = |x: f64, y: f64| [x as f32, y as f32];
    let (aa0, bb0, aa1, bb1) = (p(a0, y0), p(b0, y0), p(a1, y1), p(b1, y1));
    if (b0 - a0) > w_eps {
        tris.push([aa0, bb0, bb1]);
    }
    if (b1 - a1) > w_eps {
        tris.push([aa0, bb1, aa1]);
    }
}

// ---------------------------------------------------------------------------
// The builder
// ---------------------------------------------------------------------------

/// How many times the bevel may be halved before it is abandoned for a contour.
const BEVEL_RETRIES: u32 = 4;
/// The inset must keep at least this fraction of the contour's area, or it has over-run.
const BEVEL_AREA_FLOOR: f32 = 0.05;

struct Emit {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    bounds: Bounds3,
}

impl Emit {
    fn new() -> Self {
        Self { vertices: Vec::new(), indices: Vec::new(), bounds: Bounds3::empty() }
    }

    fn tri(&mut self, a: ([f32; 3], [f32; 3]), b: ([f32; 3], [f32; 3]), c: ([f32; 3], [f32; 3])) {
        let base = self.vertices.len() as u32;
        for (pos, normal) in [a, b, c] {
            self.bounds.expand(pos);
            self.vertices.push(Vertex { pos, normal });
        }
        self.indices.push(base);
        self.indices.push(base + 1);
        self.indices.push(base + 2);
    }
}

/// Build the extruded, bevelled mesh for a set of closed contours, in em units.
///
/// The contours are the *glyph's own*: [`glyph_contours`] produces them from a font, and
/// a test or a caller with its own outline source may hand them in directly. That split
/// is deliberate — everything difficult in this module is downstream of the font, so it
/// is testable without one.
///
/// The mesh is centred on `z = 0` and spans `±depth/2`. In `x`/`y` it occupies exactly
/// the contours' own extent (see the module doc's point 6).
pub fn build_mesh(contours: &[Vec<[f32; 2]>], params: &LetterformParams) -> GlyphMesh {
    let p = params.sanitised();
    let mut stats = MeshStats { intersection_scan: true, ..MeshStats::default() };

    let mut cleaned: Vec<Vec<[f32; 2]>> = Vec::with_capacity(contours.len());
    for c in contours {
        match clean_contour(c) {
            Some(r) => {
                stats.points += r.len();
                cleaned.push(r);
            }
            None => stats.dropped_contours += 1,
        }
    }
    stats.contours = cleaned.len();
    if cleaned.is_empty() {
        return GlyphMesh::empty(stats);
    }

    // Which side is filled has to be asked of the whole set at once — see [`Ring`].
    let mut bb = Bounds3::empty();
    for c in &cleaned {
        for p in c {
            bb.expand([p[0], p[1], 0.0]);
        }
    }
    let diag = ((bb.max[0] - bb.min[0]).powi(2) + (bb.max[1] - bb.min[1]).powi(2)).sqrt();
    let rings: Vec<Ring> = cleaned
        .iter()
        .map(|c| {
            // The fallback is only reached when a probe cannot separate the two sides at
            // any scale — a ring lying exactly on another, or a zero-area figure-eight.
            // Its own winding is then the best guess available, and it is a guess.
            let left = material_is_left(c, &cleaned, diag)
                .unwrap_or_else(|| signed_area(c) >= 0.0);
            Ring { pts: c.clone(), s: if left { 1.0 } else { -1.0 } }
        })
        .collect();

    // --- per-contour bevel, reduced where the inset would turn the ring inside out ---
    struct Beveled {
        ring: Ring,
        inset: Vec<[f32; 2]>,
        outward: Vec<[f32; 2]>,
        bevel: f32,
    }
    let mut beveled: Vec<Beveled> = Vec::with_capacity(rings.len());
    for ring in rings {
        let area0 = signed_area(&ring.pts);
        let mut b = p.bevel;
        let mut chosen: Option<(Vec<[f32; 2]>, Vec<[f32; 2]>, f32)> = None;
        if b > 0.0 {
            for _ in 0..=BEVEL_RETRIES {
                let (inset, outward) = inset_ring(&ring, b);
                let area1 = signed_area(&inset);
                let ok = area1.signum() == area0.signum()
                    && area1.abs() >= area0.abs() * BEVEL_AREA_FLOOR;
                if ok {
                    chosen = Some((inset, outward, b));
                    break;
                }
                b *= 0.5;
            }
        }
        match chosen {
            Some((inset, outward, b_used)) => {
                if b_used < p.bevel {
                    stats.bevel_reduced_contours += 1;
                }
                beveled.push(Beveled { ring, inset, outward, bevel: b_used });
            }
            None => {
                if p.bevel > 0.0 {
                    stats.bevel_reduced_contours += 1;
                }
                let inset = ring.pts.clone();
                let (_, outward) = inset_ring(&ring, 0.0);
                beveled.push(Beveled { ring, inset, outward, bevel: 0.0 });
            }
        }
    }

    let half_d = p.depth * 0.5;
    let mut emit = Emit::new();

    // --- the cap, front and back, from the INSET rings ---
    let inset_rings: Vec<Vec<[f32; 2]>> = beveled.iter().map(|b| b.inset.clone()).collect();
    let (cap, crossings, scan_ran) = tessellate_fill(&inset_rings);
    stats.self_intersections = crossings;
    stats.intersection_scan = scan_ran;
    for t in &cap {
        let n_f = [0.0, 0.0, 1.0];
        emit.tri(
            ([t[0][0], t[0][1], half_d], n_f),
            ([t[1][0], t[1][1], half_d], n_f),
            ([t[2][0], t[2][1], half_d], n_f),
        );
        let n_b = [0.0, 0.0, -1.0];
        emit.tri(
            ([t[0][0], t[0][1], -half_d], n_b),
            ([t[2][0], t[2][1], -half_d], n_b),
            ([t[1][0], t[1][1], -half_d], n_b),
        );
    }
    stats.cap_triangles = cap.len() * 2;

    // --- the bevel bands and the side wall, per contour ---
    for b in &beveled {
        let n = b.ring.pts.len();
        let flip = b.ring.s < 0.0;
        let z_face = half_d;
        let z_shoulder = half_d - b.bevel;

        if b.bevel > 0.0 {
            for i in 0..n {
                let j = (i + 1) % n;
                // Normals are generated from the chamfer's own geometry: a 45 degree
                // bevel rises as fast as it runs, so the outward miter direction and the
                // cap axis contribute equally. Nothing here reads the cap's normal.
                let ni = normalise3([b.outward[i][0], b.outward[i][1], 1.0]);
                let nj = normalise3([b.outward[j][0], b.outward[j][1], 1.0]);
                let a_hi = ([b.inset[i][0], b.inset[i][1], z_face], ni);
                let b_hi = ([b.inset[j][0], b.inset[j][1], z_face], nj);
                let a_lo = ([b.ring.pts[i][0], b.ring.pts[i][1], z_shoulder], ni);
                let b_lo = ([b.ring.pts[j][0], b.ring.pts[j][1], z_shoulder], nj);
                if flip {
                    emit.tri(a_hi, b_lo, a_lo);
                    emit.tri(a_hi, b_hi, b_lo);
                } else {
                    emit.tri(a_hi, a_lo, b_lo);
                    emit.tri(a_hi, b_lo, b_hi);
                }
                stats.bevel_triangles += 2;

                // The mirrored band on the back face.
                let mi = normalise3([b.outward[i][0], b.outward[i][1], -1.0]);
                let mj = normalise3([b.outward[j][0], b.outward[j][1], -1.0]);
                let a_hi = ([b.inset[i][0], b.inset[i][1], -z_face], mi);
                let b_hi = ([b.inset[j][0], b.inset[j][1], -z_face], mj);
                let a_lo = ([b.ring.pts[i][0], b.ring.pts[i][1], -z_shoulder], mi);
                let b_lo = ([b.ring.pts[j][0], b.ring.pts[j][1], -z_shoulder], mj);
                if flip {
                    emit.tri(a_hi, a_lo, b_lo);
                    emit.tri(a_hi, b_lo, b_hi);
                } else {
                    emit.tri(a_hi, b_lo, a_lo);
                    emit.tri(a_hi, b_hi, b_lo);
                }
                stats.bevel_triangles += 2;
            }
        }

        if z_shoulder > -z_shoulder + 1.0e-9 {
            for i in 0..n {
                let j = (i + 1) % n;
                let pi = b.ring.pts[i];
                let pj = b.ring.pts[j];
                let d = sub(pj, pi);
                // Per-face wall normal: outward is away from the filled side, which is
                // the same measured `s` the inset direction rests on and NOT this ring's
                // own winding.
                let out = match normalise2([d[1] * b.ring.s, -d[0] * b.ring.s]) {
                    Some(v) => [v[0], v[1], 0.0],
                    None => continue,
                };
                let a_hi = ([pi[0], pi[1], z_shoulder], out);
                let b_hi = ([pj[0], pj[1], z_shoulder], out);
                let a_lo = ([pi[0], pi[1], -z_shoulder], out);
                let b_lo = ([pj[0], pj[1], -z_shoulder], out);
                if flip {
                    emit.tri(a_hi, b_lo, a_lo);
                    emit.tri(a_hi, b_hi, b_lo);
                } else {
                    emit.tri(a_hi, a_lo, b_lo);
                    emit.tri(a_hi, b_lo, b_hi);
                }
                stats.wall_triangles += 2;
            }
        }
    }

    GlyphMesh { vertices: emit.vertices, indices: emit.indices, bounds: emit.bounds, stats }
}

// ---------------------------------------------------------------------------
// The font bridge
// ---------------------------------------------------------------------------

/// A font's identity for cache purposes: a 64-bit FNV-1a of its bytes.
///
/// 🚨 **Derive it from the bytes, never from a name.** Two fonts sharing a key serve each
/// other's glyphs, and the symptom is a single wrong letter in a string that is otherwise
/// perfect — which reads as a shaping bug, not a cache bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub u64);

impl FontId {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self(h)
    }
}

/// Recover a glyph's closed contours from `ab_glyph`, in **em units**.
///
/// ⚠️ **`ab_glyph::Outline::curves` is a flat list with no contour delimiter.** Its
/// builder emits `move_to` as nothing at all and `close()` as an explicit
/// `Line(last, first)`, so the only record of where one contour ends is that a curve's
/// end point returns to the contour's start. That is the rule used here.
///
/// A contour that passes exactly through its own start point mid-way would be split into
/// two closed loops — which under the non-zero rule fills identically, so the split is
/// invisible in the result. That is why this rule is safe and the alternative
/// ("start a new contour when the next curve does not continue the last") is not: two
/// consecutive contours beginning at the same point would be merged into one.
#[cfg(feature = "letterform")]
pub fn glyph_contours<F: ab_glyph::Font>(
    font: &F,
    id: ab_glyph::GlyphId,
    tolerance: f32,
) -> Vec<Vec<[f32; 2]>> {
    use ab_glyph::OutlineCurve;

    let upem = match font.units_per_em() {
        Some(u) if u > 0.0 => u,
        _ => return Vec::new(),
    };
    let outline = match font.outline(id) {
        Some(o) => o,
        None => return Vec::new(),
    };
    // The tolerance the caller stated is in em units; flattening happens in font units,
    // so it is scaled here and only here.
    let tol_font = (tolerance.max(MIN_TOLERANCE)) * upem;

    let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut cur: Vec<[f32; 2]> = Vec::new();
    let mut start: Option<[f32; 2]> = None;
    let close_eps = upem * 1.0e-5;

    for c in &outline.curves {
        let (p0, end) = match *c {
            OutlineCurve::Line(a, b) => ([a.x, a.y], [b.x, b.y]),
            OutlineCurve::Quad(a, _, b) => ([a.x, a.y], [b.x, b.y]),
            OutlineCurve::Cubic(a, _, _, b) => ([a.x, a.y], [b.x, b.y]),
        };
        if cur.is_empty() {
            start = Some(p0);
            cur.push(p0);
        }
        match *c {
            OutlineCurve::Line(_, b) => cur.push([b.x, b.y]),
            OutlineCurve::Quad(a, k, b) => {
                flatten_quad([a.x, a.y], [k.x, k.y], [b.x, b.y], tol_font, &mut cur)
            }
            OutlineCurve::Cubic(a, k1, k2, b) => flatten_cubic(
                [a.x, a.y],
                [k1.x, k1.y],
                [k2.x, k2.y],
                [b.x, b.y],
                tol_font,
                &mut cur,
            ),
        }
        if let Some(s) = start {
            if len2(sub(end, s)) <= close_eps * close_eps {
                contours.push(std::mem::take(&mut cur));
                start = None;
            }
        }
    }
    if !cur.is_empty() {
        contours.push(cur);
    }

    for c in &mut contours {
        for p in c.iter_mut() {
            p[0] /= upem;
            p[1] /= upem;
        }
    }
    contours
}

/// The full path from a font glyph to a mesh, in em units.
#[cfg(feature = "letterform")]
pub fn glyph_mesh<F: ab_glyph::Font>(
    font: &F,
    id: ab_glyph::GlyphId,
    params: &LetterformParams,
) -> GlyphMesh {
    let p = params.sanitised();
    let contours = glyph_contours(font, id, p.tolerance);
    build_mesh(&contours, &p)
}

// ---------------------------------------------------------------------------
// The atlas
// ---------------------------------------------------------------------------

/// Fold `-0.0` onto `+0.0` so the two do not key different entries for one shape.
fn key_bits(v: f32) -> u32 {
    if v == 0.0 { 0f32.to_bits() } else { v.to_bits() }
}

/// Everything that changes the mesh, and nothing that does not.
///
/// 🚨 **A shape parameter missing from this key is not a slow cache, it is a wrong mesh.**
/// The atlas hits, returns the entry built under the *old* value, and every downstream
/// number — triangle count, bounds, the legibility gate's per-cell luma — is measured on
/// geometry nobody asked for. `mesh_key_covers_every_shape_parameter` enumerates
/// [`LetterformParams`]'s fields against this struct so the two cannot drift apart, and
/// `a_key_missing_the_bevel_serves_the_wrong_mesh` builds the broken key and watches it
/// collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshKey {
    pub font: FontId,
    pub glyph: u16,
    depth: u32,
    bevel: u32,
    tolerance: u32,
}

impl MeshKey {
    pub fn new(font: FontId, glyph: u16, params: &LetterformParams) -> Self {
        let p = params.sanitised();
        Self {
            font,
            glyph,
            depth: key_bits(p.depth),
            bevel: key_bits(p.bevel),
            tolerance: key_bits(p.tolerance),
        }
    }
}

struct AtlasEntry {
    mesh: GlyphMesh,
    used: u64,
}

/// An LRU cache of built letterforms, budgeted in entries **and** in triangles.
///
/// Two budgets rather than one because the two failure modes are different: a thousand
/// tiny glyphs exhausts an entry budget while costing nothing, and one enormous glyph at
/// a fine tolerance exhausts a triangle budget while occupying one entry.
///
/// ⚠️ **The triangle budget is a target, not a wall.** A single mesh larger than the whole
/// budget is stored anyway — refusing it would mean the atlas never caches the one glyph
/// most expensive to rebuild, which is the opposite of what a cache is for. The overrun
/// is visible in [`MeshAtlas::triangles`], which is how you find it.
pub struct MeshAtlas {
    entries: HashMap<MeshKey, AtlasEntry>,
    max_entries: usize,
    max_triangles: usize,
    triangles: usize,
    clock: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl MeshAtlas {
    pub fn new(max_entries: usize, max_triangles: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries: max_entries.max(1),
            max_triangles,
            triangles: 0,
            clock: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }
    pub fn misses(&self) -> u64 {
        self.misses
    }
    pub fn evictions(&self) -> u64 {
        self.evictions
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Triangles currently held. Compare against the budget given to [`MeshAtlas::new`].
    pub fn triangles(&self) -> usize {
        self.triangles
    }

    pub fn contains(&self, key: &MeshKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Return the cached mesh, building it with `build` on a miss.
    pub fn get_or_insert_with<F: FnOnce() -> GlyphMesh>(
        &mut self,
        key: MeshKey,
        build: F,
    ) -> &GlyphMesh {
        self.clock += 1;
        let now = self.clock;
        if self.entries.contains_key(&key) {
            self.hits += 1;
            let e = self.entries.get_mut(&key).unwrap();
            e.used = now;
            return &self.entries[&key].mesh;
        }
        self.misses += 1;
        let mesh = build();
        let tris = mesh.triangles();
        self.make_room(tris);
        self.triangles += tris;
        match self.entries.entry(key) {
            MapEntry::Occupied(mut o) => {
                o.insert(AtlasEntry { mesh, used: now });
            }
            MapEntry::Vacant(v) => {
                v.insert(AtlasEntry { mesh, used: now });
            }
        }
        &self.entries[&key].mesh
    }

    /// Evict least-recently-used entries until one more of `incoming` triangles fits.
    fn make_room(&mut self, incoming: usize) {
        while !self.entries.is_empty()
            && (self.entries.len() + 1 > self.max_entries
                || self.triangles + incoming > self.max_triangles)
        {
            let victim = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| *k);
            match victim {
                Some(k) => {
                    if let Some(e) = self.entries.remove(&k) {
                        self.triangles -= e.mesh.triangles();
                        self.evictions += 1;
                    }
                }
                None => break,
            }
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.triangles = 0;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit square, counter-clockwise (positive area).
    fn ccw_square(s: f32) -> Vec<[f32; 2]> {
        vec![[0.0, 0.0], [s, 0.0], [s, s], [0.0, s]]
    }

    /// The same square, clockwise. Both conventions are real: TrueType and CFF disagree
    /// about which way an outer contour runs.
    fn cw_square(s: f32) -> Vec<[f32; 2]> {
        let mut v = ccw_square(s);
        v.reverse();
        v
    }

    fn flat() -> LetterformParams {
        LetterformParams { depth: 0.0, bevel: 0.0, tolerance: DEFAULT_TOLERANCE }
    }

    /// The filled area of the front cap.
    ///
    /// ⚠️ Selected by **normal**, not by z-plane. The first version of this helper
    /// selected `z == plane` and reported exactly zero for every `depth: 0.0` mesh: the
    /// front and back caps are then coplanar and their signed areas cancel. That is a
    /// property of the helper, not of the mesh, and it failed four tests at once in a way
    /// that read like a broken tessellator.
    fn front_cap_area(mesh: &GlyphMesh) -> f32 {
        let mut a = 0.0f64;
        for t in mesh.indices.chunks(3) {
            let v: Vec<&Vertex> = t.iter().map(|&i| &mesh.vertices[i as usize]).collect();
            if v.iter().any(|q| q.normal[2] < 0.999) {
                continue;
            }
            let p: Vec<[f32; 3]> = v.iter().map(|q| q.pos).collect();
            a += 0.5
                * ((p[1][0] - p[0][0]) as f64 * (p[2][1] - p[0][1]) as f64
                    - (p[2][0] - p[0][0]) as f64 * (p[1][1] - p[0][1]) as f64);
        }
        a as f32
    }

    /// Is `p` covered by any front-cap triangle?
    ///
    /// 🚨 **Area is not shape, and a fill test that only checks area can be fooled.** The
    /// mutation harness proved it: removing the self-intersection scanlines makes a
    /// bowtie tessellate as ONE triangle of area exactly 4 where the true fill is two
    /// lobes of area 2 — the same number, a completely different picture, and
    /// `front_cap_area` alone reported success. Every fill assertion below therefore
    /// pins points as well as totals.
    fn covered(mesh: &GlyphMesh, p: [f32; 2]) -> bool {
        for t in mesh.indices.chunks(3) {
            let v: Vec<&Vertex> = t.iter().map(|&i| &mesh.vertices[i as usize]).collect();
            if v.iter().any(|q| q.normal[2] < 0.999) {
                continue;
            }
            let a = v[0].pos;
            let b = v[1].pos;
            let c = v[2].pos;
            let s = |u: [f32; 3], w: [f32; 3]| {
                (w[0] - u[0]) * (p[1] - u[1]) - (p[0] - u[0]) * (w[1] - u[1])
            };
            let (d1, d2, d3) = (s(a, b), s(b, c), s(c, a));
            let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            if !(neg && pos) {
                return true;
            }
        }
        false
    }

    // -- winding, holes, and the fill rule ---------------------------------

    #[test]
    fn a_square_fills_to_its_own_area_either_winding() {
        for c in [ccw_square(1.0), cw_square(1.0)] {
            let m = build_mesh(&[c], &flat());
            let a = front_cap_area(&m);
            assert!(
                (a - 1.0).abs() < 1.0e-4,
                "cap area {a} should be 1.0 for a unit square"
            );
        }
    }

    #[test]
    fn a_counter_stays_a_hole_under_the_nonzero_rule() {
        // The `o` case: an outer ring and an inner ring wound the other way.
        let outer = ccw_square(4.0);
        let inner: Vec<[f32; 2]> =
            vec![[1.0, 1.0], [1.0, 3.0], [3.0, 3.0], [3.0, 1.0]]; // clockwise
        let m = build_mesh(&[outer, inner], &flat());
        let a = front_cap_area(&m);
        assert!(
            (a - 12.0).abs() < 1.0e-3,
            "cap area {a} should be 16 - 4 = 12; a filled counter would give 16"
        );
        assert!(!covered(&m, [2.0, 2.0]), "the middle of the counter must be empty");
        assert!(covered(&m, [0.5, 2.0]), "the left stem must be filled");
        assert!(covered(&m, [2.0, 0.5]), "the bottom bar must be filled");
    }

    #[test]
    fn overlapping_components_fill_solid_rather_than_punching_a_hole() {
        // Two overlapping same-wound squares: the composite-glyph case. Even-odd would
        // subtract the overlap; non-zero must not.
        let a = ccw_square(2.0);
        let b: Vec<[f32; 2]> = vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
        let m = build_mesh(&[a, b], &flat());
        let area = front_cap_area(&m);
        assert!(
            (area - 7.0).abs() < 1.0e-3,
            "cap area {area} should be 4 + 4 - 1 = 7; even-odd would give 6"
        );
        assert!(covered(&m, [1.5, 1.5]), "the overlap must be filled, not punched out");
    }

    #[test]
    fn a_self_intersecting_contour_is_filled_not_rejected() {
        // A bowtie whose lobes have EQUAL area, so its signed area is exactly zero while
        // its filled area is 4 — under non-zero each lobe has winding +/-1 and both are
        // filled. This is the case a "reject zero-area contours" cleaning rule deletes
        // silently, and this crossing is at (2, 1): no vertex has that y, so the fill is
        // only correct because the intersection scan added it as a scanline.
        let bowtie: Vec<[f32; 2]> = vec![[0.0, 0.0], [4.0, 0.0], [0.0, 2.0], [4.0, 2.0]];
        assert!(signed_area(&bowtie).abs() < 1.0e-6, "the fixture must have zero signed area");
        let m = build_mesh(&[bowtie], &flat());
        assert_eq!(m.stats.contours, 1, "a zero-signed-area contour must NOT be dropped");
        assert!(m.stats.self_intersections >= 1, "the crossing should be found");
        assert!(m.stats.intersection_scan, "the scan should have run at this size");
        let a = front_cap_area(&m).abs();
        assert!((a - 4.0).abs() < 1.0e-3, "filled area {a} should be 2 + 2, not 0");
        // And the fill must be in the right PLACE, which the area alone does not say.
        for p in [[2.0f32, 0.2], [1.0, 0.2], [2.0, 1.8], [3.0, 1.8]] {
            assert!(covered(&m, p), "{p:?} is inside a lobe and must be filled");
        }
        for p in [[0.2f32, 1.0], [3.8, 1.0], [1.0, 1.0], [3.0, 1.0]] {
            assert!(!covered(&m, p), "{p:?} is between the lobes and must be empty");
        }
    }

    #[test]
    fn degenerate_contours_are_dropped_and_counted() {
        let dup: Vec<[f32; 2]> = vec![[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]];
        let line: Vec<[f32; 2]> = vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]];
        let m = build_mesh(&[dup, line, ccw_square(1.0)], &flat());
        assert_eq!(m.stats.dropped_contours, 2, "both degenerates should be dropped");
        assert_eq!(m.stats.contours, 1, "only the square should survive");
    }

    #[test]
    fn a_trailing_duplicate_of_the_first_point_is_not_a_fourth_edge() {
        // ab_glyph's close() emits Line(last, first) even when last == first, so nearly
        // every real glyph arrives with this. Left in, the edge direction is (0,0) and
        // the miter normal becomes NaN.
        let mut sq = ccw_square(1.0);
        sq.push([0.0, 0.0]);
        let m = build_mesh(&[sq], &LetterformParams { depth: 0.2, bevel: 0.05, tolerance: 0.005 });
        assert!(
            m.vertices.iter().all(|v| v.pos.iter().all(|c| c.is_finite())
                && v.normal.iter().all(|c| c.is_finite())),
            "a duplicated closing point must not produce NaN vertices or normals"
        );
    }

    // -- the bevel ---------------------------------------------------------

    #[test]
    fn the_bevel_generates_normals_the_cap_does_not_have() {
        let p = LetterformParams { depth: 0.4, bevel: 0.1, tolerance: 0.005 };
        let m = build_mesh(&[ccw_square(1.0)], &p);
        assert!(m.stats.bevel_triangles > 0, "a bevel was asked for and must be emitted");
        // Every bevel vertex must be tilted: neither the flat cap normal (|z| = 1) nor a
        // wall normal (|z| = 0). Nothing else in the mesh is, so the tilted vertices are
        // exactly the bevel band's — three per triangle, since nothing is welded.
        //
        // ⚠️ The count must be EXACT. An earlier version asserted
        // `tilted >= bevel_triangles` and **passed against deliberately broken code**:
        // replacing one of the two per-edge miter normals with the cap's own `(0,0,1)`
        // still leaves the other one tilted, and a lower bound cannot see half a band go
        // flat. That is #133's failure mode, found by the mutation harness rather than by
        // reading. An equality is what kills it.
        let tilted: Vec<&Vertex> = m
            .vertices
            .iter()
            .filter(|v| v.normal[2].abs() > 0.05 && v.normal[2].abs() < 0.95)
            .collect();
        assert_eq!(
            tilted.len(),
            m.stats.bevel_triangles * 3,
            "every bevel vertex must carry a generated normal, and only bevel vertices may"
        );
        for v in &tilted {
            assert!(
                (v.normal[2].abs() - std::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-3,
                "a 45 degree chamfer's normal must have |z| = 1/sqrt(2), got {}",
                v.normal[2]
            );
        }
    }

    #[test]
    fn the_bevel_insets_the_face_it_never_grows_the_silhouette() {
        let p = LetterformParams { depth: 0.4, bevel: 0.1, tolerance: 0.005 };
        let m = build_mesh(&[ccw_square(1.0)], &p);
        assert!(
            m.bounds.min[0] >= -1.0e-5 && m.bounds.max[0] <= 1.0 + 1.0e-5,
            "x bounds {:?} left the source contour",
            (m.bounds.min[0], m.bounds.max[0])
        );
        assert!(
            m.bounds.min[1] >= -1.0e-5 && m.bounds.max[1] <= 1.0 + 1.0e-5,
            "y bounds left the source contour"
        );
        // The face is the inset ring, so the cap area is the shrunken square.
        let a = front_cap_area(&m);
        assert!((a - 0.8 * 0.8).abs() < 1.0e-3, "front cap area {a} should be (1 - 2*0.1)^2");
    }

    #[test]
    fn the_bevel_shrinks_the_material_for_both_windings_and_grows_a_counter() {
        let p = LetterformParams { depth: 0.4, bevel: 0.1, tolerance: 0.005 };
        for outer in [ccw_square(4.0), cw_square(4.0)] {
            // Whatever the outer winding, the counter runs the other way.
            let ccw_outer = signed_area(&outer) > 0.0;
            let mut inner: Vec<[f32; 2]> =
                vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
            if ccw_outer {
                inner.reverse();
            }
            let m = build_mesh(&[outer, inner], &p);
            let a = front_cap_area(&m);
            // Outer 4 -> 3.8, counter 2 -> 2.2: material shrinks from both sides.
            let want = 3.8 * 3.8 - 2.2 * 2.2;
            assert!(
                (a - want).abs() < 1.0e-2,
                "front cap area {a} should be {want}: outer inset AND counter grown"
            );
        }
    }

    #[test]
    fn a_bevel_too_large_for_a_thin_stem_is_reduced_not_inverted() {
        // A 0.02-wide stem cannot carry a 0.05 bevel: the inset crosses itself.
        let stem: Vec<[f32; 2]> = vec![[0.0, 0.0], [0.02, 0.0], [0.02, 1.0], [0.0, 1.0]];
        let p = LetterformParams { depth: 0.4, bevel: 0.05, tolerance: 0.005 };
        let m = build_mesh(&[stem], &p);
        assert_eq!(m.stats.bevel_reduced_contours, 1, "the over-run must be reported");
        let a = front_cap_area(&m);
        assert!(a > 0.0, "front cap area {a} must stay positive, not invert");
        assert!(a <= 0.02 * 1.0 + 1.0e-6, "front cap area {a} must not exceed the stem");
    }

    #[test]
    fn zero_bevel_emits_no_bevel_band() {
        let p = LetterformParams { depth: 0.4, bevel: 0.0, tolerance: 0.005 };
        let m = build_mesh(&[ccw_square(1.0)], &p);
        assert_eq!(m.stats.bevel_triangles, 0);
        assert!(m.stats.wall_triangles > 0);
    }

    #[test]
    fn bevel_is_clamped_to_half_the_depth_so_the_two_faces_cannot_cross() {
        let p = LetterformParams { depth: 0.10, bevel: 0.40, tolerance: 0.005 }.sanitised();
        assert!((p.bevel - 0.05).abs() < 1.0e-7, "bevel {} should clamp to depth/2", p.bevel);
        // With bevel exactly depth/2 the shoulder is at z = 0 and there is no wall left.
        let m = build_mesh(&[ccw_square(1.0)], &p);
        assert_eq!(m.stats.wall_triangles, 0, "a full-depth chamfer leaves no side wall");
        assert!(m.stats.bevel_triangles > 0);
    }

    // -- extrusion ---------------------------------------------------------

    #[test]
    fn the_mesh_spans_exactly_plus_and_minus_half_the_depth() {
        let p = LetterformParams { depth: 0.3, bevel: 0.05, tolerance: 0.005 };
        let m = build_mesh(&[ccw_square(1.0)], &p);
        assert!((m.bounds.min[2] + 0.15).abs() < 1.0e-6, "min z {}", m.bounds.min[2]);
        assert!((m.bounds.max[2] - 0.15).abs() < 1.0e-6, "max z {}", m.bounds.max[2]);
    }

    #[test]
    fn zero_depth_gives_a_flat_double_sided_cap_and_no_wall() {
        let m = build_mesh(&[ccw_square(1.0)], &flat());
        assert_eq!(m.stats.wall_triangles, 0);
        assert_eq!(m.stats.bevel_triangles, 0);
        assert!(m.stats.cap_triangles > 0);
        assert!((m.bounds.max[2] - m.bounds.min[2]).abs() < 1.0e-6);
    }

    #[test]
    fn every_wall_normal_points_out_of_the_solid_for_either_winding() {
        for c in [ccw_square(1.0), cw_square(1.0)] {
            let p = LetterformParams { depth: 0.4, bevel: 0.0, tolerance: 0.005 };
            let m = build_mesh(&[c], &p);
            let centre = [0.5f32, 0.5f32];
            for v in &m.vertices {
                if v.normal[2].abs() > 1.0e-3 {
                    continue; // a cap vertex
                }
                let away = [v.pos[0] - centre[0], v.pos[1] - centre[1]];
                let d = away[0] * v.normal[0] + away[1] * v.normal[1];
                assert!(
                    d > 0.0,
                    "wall normal {:?} at {:?} points back into the square",
                    v.normal,
                    v.pos
                );
            }
        }
    }

    #[test]
    fn every_triangle_faces_outward_for_either_winding() {
        // The divergence test: for a closed mesh, sum over triangles of
        // dot(centroid - interior_point, area_normal) must be positive if every face is
        // wound outward. A single flipped face shows as a smaller sum; a wholesale
        // reversal flips the sign.
        for c in [ccw_square(1.0), cw_square(1.0)] {
            let p = LetterformParams { depth: 0.4, bevel: 0.08, tolerance: 0.005 };
            let m = build_mesh(&[c], &p);
            let inside = [0.5f64, 0.5f64, 0.0f64];
            let mut flux = 0.0f64;
            for t in m.indices.chunks(3) {
                let q: Vec<[f64; 3]> = t
                    .iter()
                    .map(|&i| {
                        let v = m.vertices[i as usize].pos;
                        [v[0] as f64, v[1] as f64, v[2] as f64]
                    })
                    .collect();
                let u = [q[1][0] - q[0][0], q[1][1] - q[0][1], q[1][2] - q[0][2]];
                let w = [q[2][0] - q[0][0], q[2][1] - q[0][1], q[2][2] - q[0][2]];
                let n = [
                    u[1] * w[2] - u[2] * w[1],
                    u[2] * w[0] - u[0] * w[2],
                    u[0] * w[1] - u[1] * w[0],
                ];
                let cen = [
                    (q[0][0] + q[1][0] + q[2][0]) / 3.0 - inside[0],
                    (q[0][1] + q[1][1] + q[2][1]) / 3.0 - inside[1],
                    (q[0][2] + q[1][2] + q[2][2]) / 3.0 - inside[2],
                ];
                flux += cen[0] * n[0] + cen[1] * n[1] + cen[2] * n[2];
            }
            // 6 * volume for a closed, outward-wound mesh.
            assert!(flux > 0.0, "outward flux {flux} is not positive: faces are inverted");
        }
    }

    // -- flattening --------------------------------------------------------

    #[test]
    fn flattening_stays_within_tolerance() {
        // Measured against a dense sampling of the true curve, not against the
        // subdivision predicate that produced the polyline.
        let tol = 0.01f32;
        let p0 = [0.0f32, 0.0];
        let c1 = [0.0f32, 1.0];
        let c2 = [1.0f32, 1.0];
        let p3 = [1.0f32, 0.0];
        let mut poly = vec![p0];
        flatten_cubic(p0, c1, c2, p3, tol, &mut poly);
        let mut worst = 0.0f32;
        for i in 0..=2000 {
            let t = i as f32 / 2000.0;
            let mt = 1.0 - t;
            let b = [
                mt * mt * mt * p0[0]
                    + 3.0 * mt * mt * t * c1[0]
                    + 3.0 * mt * t * t * c2[0]
                    + t * t * t * p3[0],
                mt * mt * mt * p0[1]
                    + 3.0 * mt * mt * t * c1[1]
                    + 3.0 * mt * t * t * c2[1]
                    + t * t * t * p3[1],
            ];
            let mut best = f32::INFINITY;
            for s in poly.windows(2) {
                best = best.min(dist_to_segment(b, s[0], s[1]));
            }
            worst = worst.max(best);
        }
        assert!(worst <= tol, "flattened polyline deviates {worst} > tolerance {tol}");
    }

    #[test]
    fn a_tighter_tolerance_produces_more_points() {
        let p0 = [0.0f32, 0.0];
        let c = [0.5f32, 1.0];
        let p2 = [1.0f32, 0.0];
        let mut coarse = vec![p0];
        flatten_quad(p0, c, p2, 0.05, &mut coarse);
        let mut fine = vec![p0];
        flatten_quad(p0, c, p2, 0.0005, &mut fine);
        assert!(
            fine.len() > coarse.len(),
            "fine {} should exceed coarse {}",
            fine.len(),
            coarse.len()
        );
    }

    fn dist_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
        let ab = sub(b, a);
        let l2 = len2(ab);
        if l2 <= 1.0e-20 {
            return len2(sub(p, a)).sqrt();
        }
        let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0);
        let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
        len2(sub(p, q)).sqrt()
    }

    // -- parameter sanitising ---------------------------------------------

    #[test]
    fn non_finite_parameters_become_something_buildable() {
        let p = LetterformParams { depth: f32::NAN, bevel: f32::INFINITY, tolerance: -1.0 }
            .sanitised();
        assert_eq!(p.depth, 0.0);
        assert_eq!(p.bevel, 0.0);
        assert_eq!(p.tolerance, DEFAULT_TOLERANCE);
        let m = build_mesh(&[ccw_square(1.0)], &p);
        assert!(m.vertices.iter().all(|v| v.pos.iter().all(|c| c.is_finite())));
    }

    // -- the cache key -----------------------------------------------------

    #[test]
    fn mesh_key_covers_every_shape_parameter() {
        // Vary one field at a time; each must change the key. If LetterformParams grows a
        // field and MeshKey does not, add the case here and this test is what catches it.
        let base = LetterformParams { depth: 0.2, bevel: 0.03, tolerance: 0.004 };
        let f = FontId(7);
        let k = MeshKey::new(f, 42, &base);
        let cases: [(&str, LetterformParams); 3] = [
            ("depth", LetterformParams { depth: 0.25, ..base }),
            ("bevel", LetterformParams { bevel: 0.04, ..base }),
            ("tolerance", LetterformParams { tolerance: 0.002, ..base }),
        ];
        for (name, p) in cases {
            assert_ne!(k, MeshKey::new(f, 42, &p), "{name} is missing from MeshKey");
        }
        assert_ne!(k, MeshKey::new(FontId(8), 42, &base), "font is missing from MeshKey");
        assert_ne!(k, MeshKey::new(f, 43, &base), "glyph is missing from MeshKey");
    }

    #[test]
    fn a_key_missing_the_bevel_serves_the_wrong_mesh() {
        // Prove the negative the module doc claims. This is the key MeshKey would be if
        // someone dropped `bevel` from it; the atlas then hits on a mesh built with a
        // different bevel and hands it back with no error anywhere.
        #[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
        struct BrokenKey {
            font: FontId,
            glyph: u16,
            depth: u32,
            tolerance: u32,
        }
        let thin = LetterformParams { depth: 0.3, bevel: 0.01, tolerance: 0.005 };
        let fat = LetterformParams { depth: 0.3, bevel: 0.10, tolerance: 0.005 };
        let broken = |p: &LetterformParams| BrokenKey {
            font: FontId(1),
            glyph: 9,
            depth: key_bits(p.depth),
            tolerance: key_bits(p.tolerance),
        };
        assert_eq!(broken(&thin), broken(&fat), "the broken key must collide, or the point is lost");

        let mut cache: HashMap<BrokenKey, GlyphMesh> = HashMap::new();
        let square = ccw_square(1.0);
        cache.insert(broken(&thin), build_mesh(&[square.clone()], &thin));
        let served = cache
            .entry(broken(&fat))
            .or_insert_with(|| build_mesh(&[square.clone()], &fat))
            .clone();
        let honest = build_mesh(&[square.clone()], &fat);
        let served_face = front_cap_area(&served);
        let honest_face = front_cap_area(&honest);
        assert!(
            (served_face - honest_face).abs() > 1.0e-3,
            "the broken key served face area {served_face} where {honest_face} was asked for; \
             if these agree the negative is not proven"
        );

        // And the real key does not collide.
        assert_ne!(
            MeshKey::new(FontId(1), 9, &thin),
            MeshKey::new(FontId(1), 9, &fat),
            "MeshKey must separate two bevels"
        );
    }

    #[test]
    fn minus_zero_and_zero_key_the_same_entry() {
        let a = LetterformParams { depth: 0.0, bevel: 0.0, tolerance: 0.005 };
        let b = LetterformParams { depth: -0.0, bevel: -0.0, tolerance: 0.005 };
        assert_eq!(MeshKey::new(FontId(1), 1, &a), MeshKey::new(FontId(1), 1, &b));
    }

    #[test]
    fn font_id_follows_the_bytes_not_the_name() {
        assert_ne!(FontId::from_bytes(b"OTTO-a"), FontId::from_bytes(b"OTTO-b"));
        assert_eq!(FontId::from_bytes(b"same"), FontId::from_bytes(b"same"));
    }

    // -- the atlas ---------------------------------------------------------

    fn square_mesh() -> GlyphMesh {
        build_mesh(&[ccw_square(1.0)], &LetterformParams::default())
    }

    #[test]
    fn the_atlas_hits_on_the_second_ask() {
        let mut a = MeshAtlas::new(8, 1_000_000);
        let k = MeshKey::new(FontId(1), 1, &LetterformParams::default());
        let mut built = 0;
        a.get_or_insert_with(k, || {
            built += 1;
            square_mesh()
        });
        a.get_or_insert_with(k, || {
            built += 1;
            square_mesh()
        });
        assert_eq!(built, 1, "the second ask must not rebuild");
        assert_eq!(a.hits(), 1);
        assert_eq!(a.misses(), 1);
    }

    #[test]
    fn the_atlas_evicts_least_recently_used_by_entry_count() {
        let mut a = MeshAtlas::new(2, 100_000_000);
        let p = LetterformParams::default();
        let k1 = MeshKey::new(FontId(1), 1, &p);
        let k2 = MeshKey::new(FontId(1), 2, &p);
        let k3 = MeshKey::new(FontId(1), 3, &p);
        a.get_or_insert_with(k1, square_mesh);
        a.get_or_insert_with(k2, square_mesh);
        a.get_or_insert_with(k1, square_mesh); // k1 is now the most recent
        a.get_or_insert_with(k3, square_mesh);
        assert_eq!(a.len(), 2);
        assert!(a.contains(&k1), "k1 was touched most recently and must survive");
        assert!(a.contains(&k3));
        assert!(!a.contains(&k2), "k2 was least recently used and must be the victim");
        assert_eq!(a.evictions(), 1);
    }

    #[test]
    fn the_atlas_evicts_on_the_triangle_budget_too() {
        let one = square_mesh().triangles();
        let mut a = MeshAtlas::new(1000, one * 2);
        let p = LetterformParams::default();
        for g in 0..4u16 {
            a.get_or_insert_with(MeshKey::new(FontId(1), g, &p), square_mesh);
        }
        assert!(a.len() <= 2, "entries {} exceed the triangle budget", a.len());
        assert!(a.triangles() <= one * 2, "triangles {} exceed the budget", a.triangles());
        assert!(a.evictions() >= 2);
    }

    #[test]
    fn a_single_mesh_larger_than_the_whole_budget_is_still_cached() {
        // Refusing it would mean never caching the one glyph most expensive to rebuild.
        // The overrun is visible in triangles(), which is how it is found.
        let one = square_mesh().triangles();
        let mut a = MeshAtlas::new(8, one / 2);
        let k = MeshKey::new(FontId(1), 1, &LetterformParams::default());
        a.get_or_insert_with(k, square_mesh);
        assert!(a.contains(&k));
        assert!(a.triangles() > a.max_triangles, "the overrun must be visible");
    }

    // -- the cell law (section 9) -----------------------------------------

    #[test]
    fn bounds_answer_the_cell_question_in_both_directions() {
        let p = LetterformParams { depth: 0.3, bevel: 0.05, tolerance: 0.005 };
        // A contour that leaves the em square in y, as a descender does.
        let tall: Vec<[f32; 2]> = vec![[0.0, -0.3], [0.5, -0.3], [0.5, 0.9], [0.0, 0.9]];
        let m = build_mesh(&[tall], &p);
        assert!(!m.bounds.fits_cell(0.5, 0.5, 0.5), "a descender must NOT fit a half-em cell");
        assert!(m.bounds.fits_cell(0.5, 1.0, 0.5), "and must fit a taller one");
    }
}

// ---------------------------------------------------------------------------
// Font-backed tests: the bridge, and the measurements
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "letterform"))]
mod font_tests {
    use super::*;
    use ab_glyph::{Font, FontRef};

    /// The repository's own monospace face, read at run time rather than embedded.
    ///
    /// `include_bytes!` would bake 274 KB into every build of this crate for the sake of
    /// tests, and `organon-core`'s existing `include_str!` sites reaching out of the
    /// package are already the reason `cargo package` fails (see `Cargo.toml`'s header) —
    /// so this reads the file instead, and fails naming the path if it is gone.
    ///
    /// It is a **CFF/OpenType** face on purpose: the other candidate in the tree is
    /// TrueType, and the two disagree about which way an outer contour winds, which is
    /// exactly the assumption this module must not make.
    fn font_bytes() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("site")
            .join("fonts")
            .join("CommitMono-400-Regular.otf");
        std::fs::read(&path)
            .unwrap_or_else(|e| panic!("cannot read the test face at {}: {e}", path.display()))
    }

    #[test]
    fn a_real_cff_glyph_produces_a_closed_bevelled_mesh() {
        let bytes = font_bytes();
        let font = FontRef::try_from_slice(&bytes).expect("CommitMono should parse");
        let m = glyph_mesh(&font, font.glyph_id('O'), &LetterformParams::default());
        assert!(m.stats.contours >= 2, "an O has an outer contour and a counter");
        assert!(m.stats.cap_triangles > 0 && m.stats.wall_triangles > 0);
        assert!(m.stats.bevel_triangles > 0, "the default parameters ask for a bevel");
        assert!(
            m.vertices.iter().all(|v| v.pos.iter().all(|c| c.is_finite())),
            "a real glyph must not produce NaN positions"
        );
    }

    #[test]
    fn the_counter_of_a_real_o_is_not_filled() {
        let bytes = font_bytes();
        let font = FontRef::try_from_slice(&bytes).expect("font");
        let p = LetterformParams { depth: 0.0, bevel: 0.0, tolerance: 0.002 };
        let o = glyph_mesh(&font, font.glyph_id('O'), &p);
        let ring = super::tests_front_cap_area(&o);
        // A solid rectangle of the same bounds would be far larger; the counter is most
        // of an O's box. Compare against the bounding box: a filled counter would push
        // the ratio well above 0.75.
        let bw = o.bounds.max[0] - o.bounds.min[0];
        let bh = o.bounds.max[1] - o.bounds.min[1];
        let ratio = ring / (bw * bh);
        assert!(
            ratio > 0.1 && ratio < 0.75,
            "cap area / bbox = {ratio}; above 0.75 means the counter filled in"
        );
    }

    #[test]
    fn glyph_bounds_can_leave_the_em_square() {
        // Section 9 law 1, measured rather than asserted. A descender is the everyday
        // case: 'g' reaches below the baseline by more than any cell centred on the em.
        let bytes = font_bytes();
        let font = FontRef::try_from_slice(&bytes).expect("font");
        let p = LetterformParams::default();
        let g = glyph_mesh(&font, font.glyph_id('g'), &p);
        assert!(
            g.bounds.min[1] < 0.0,
            "a descender must reach below the baseline; got {}",
            g.bounds.min[1]
        );
        // And it is the FONT, not this module, that decides: the mesh never exceeds the
        // contours it was given, which the bevel-inset test pins separately.
        println!(
            "[cell-law] 'g' bounds x [{:.4}, {:.4}] y [{:.4}, {:.4}] z [{:.4}, {:.4}] em",
            g.bounds.min[0],
            g.bounds.max[0],
            g.bounds.min[1],
            g.bounds.max[1],
            g.bounds.min[2],
            g.bounds.max[2]
        );
    }

    #[test]
    fn organon_triangle_counts_by_tolerance() {
        // The measurement the brief asks for. Printed, not asserted: a triangle count is
        // a property of the font and would make this a change-detector test.
        let bytes = font_bytes();
        let font = FontRef::try_from_slice(&bytes).expect("font");
        println!("[budget] ORGANON, depth 0.15 em, bevel 0.02 em, CommitMono 400");
        for tol in [0.02f32, 0.01, 0.005, 0.002, 0.001] {
            let p = LetterformParams { depth: 0.15, bevel: 0.02, tolerance: tol };
            let mut tris = 0usize;
            let mut verts = 0usize;
            let mut pts = 0usize;
            for ch in "ORGANON".chars() {
                let m = glyph_mesh(&font, font.glyph_id(ch), &p);
                tris += m.triangles();
                verts += m.vertices.len();
                pts += m.stats.points;
            }
            println!(
                "  tol {tol:>7.4} em -> {tris:>7} tris, {verts:>7} verts, {pts:>5} flattened pts"
            );
        }
        let p = LetterformParams::default();
        let total: usize =
            "ORGANON".chars().map(|c| glyph_mesh(&font, font.glyph_id(c), &p).triangles()).sum();
        assert!(total > 0);
    }

    #[test]
    fn atlas_cold_and_warm_over_the_printable_ascii_range() {
        // Timing is reported, never asserted: under CARGO_PROFILE_TEST_OPT_LEVEL=0 the
        // absolute numbers are meaningless, so the figures quoted anywhere else come from
        // a --release run and say so.
        let bytes = font_bytes();
        let font = FontRef::try_from_slice(&bytes).expect("font");
        let fid = FontId::from_bytes(&bytes);
        let p = LetterformParams::default();
        let mut atlas = MeshAtlas::new(256, 4_000_000);

        let chars: Vec<char> = (0x20u8..0x7fu8).map(|b| b as char).collect();
        let t0 = std::time::Instant::now();
        for &c in &chars {
            let id = font.glyph_id(c);
            let key = MeshKey::new(fid, id.0, &p);
            atlas.get_or_insert_with(key, || glyph_mesh(&font, id, &p));
        }
        let cold = t0.elapsed();
        let t1 = std::time::Instant::now();
        for &c in &chars {
            let id = font.glyph_id(c);
            let key = MeshKey::new(fid, id.0, &p);
            atlas.get_or_insert_with(key, || glyph_mesh(&font, id, &p));
        }
        let warm = t1.elapsed();
        println!(
            "[atlas] {} glyphs: cold {:?}, warm {:?}, hits {}, misses {}, evictions {}, {} tris held",
            chars.len(),
            cold,
            warm,
            atlas.hits(),
            atlas.misses(),
            atlas.evictions(),
            atlas.triangles()
        );
        assert_eq!(atlas.misses(), chars.len() as u64, "the cold pass must miss everything");
        assert_eq!(atlas.hits(), chars.len() as u64, "the warm pass must hit everything");
        assert_eq!(atlas.evictions(), 0, "256 entries is enough for printable ASCII");
    }

    #[test]
    fn every_printable_ascii_glyph_builds_without_a_nan_or_an_inversion() {
        // The sweep that finds what a hand-picked letter does not: real fonts carry
        // near-degenerate contours, and one NaN normal is enough to blacken a whole
        // draw call.
        let bytes = font_bytes();
        let font = FontRef::try_from_slice(&bytes).expect("font");
        let p = LetterformParams::default();
        let mut reduced = 0usize;
        let mut crossings = 0usize;
        for b in 0x20u8..0x7fu8 {
            let c = b as char;
            let m = glyph_mesh(&font, font.glyph_id(c), &p);
            reduced += m.stats.bevel_reduced_contours;
            crossings += m.stats.self_intersections;
            for v in &m.vertices {
                assert!(
                    v.pos.iter().all(|x| x.is_finite()) && v.normal.iter().all(|x| x.is_finite()),
                    "glyph {c:?} produced a non-finite vertex"
                );
                let l = (v.normal[0] * v.normal[0]
                    + v.normal[1] * v.normal[1]
                    + v.normal[2] * v.normal[2])
                    .sqrt();
                assert!((l - 1.0).abs() < 1.0e-3, "glyph {c:?} normal is not unit: {l}");
            }
            assert!(m.stats.intersection_scan, "glyph {c:?} exceeded the intersection scan cap");
        }
        println!(
            "[sweep] printable ASCII: {reduced} contours had their bevel reduced, \
             {crossings} self-intersections found among inset edges"
        );
    }
}

/// Front-cap area of a mesh, for the font tests. Kept out of `mod tests` so the font
/// module can use it without reaching into a `#[cfg(test)]` sibling.
///
/// ⚠️ Selects by **normal**, not by z-plane: at `depth: 0.0` the two caps are coplanar
/// and their signed areas cancel to exactly zero.
#[cfg(all(test, feature = "letterform"))]
fn tests_front_cap_area(m: &GlyphMesh) -> f32 {
    let mut a = 0.0f64;
    for t in m.indices.chunks(3) {
        let v: Vec<&Vertex> = t.iter().map(|&i| &m.vertices[i as usize]).collect();
        if v.iter().any(|q| q.normal[2] < 0.999) {
            continue;
        }
        let p: Vec<[f32; 3]> = v.iter().map(|q| q.pos).collect();
        a += 0.5
            * ((p[1][0] - p[0][0]) as f64 * (p[2][1] - p[0][1]) as f64
                - (p[2][0] - p[0][0]) as f64 * (p[1][1] - p[0][1]) as f64);
    }
    a as f32
}
