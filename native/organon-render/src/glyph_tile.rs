//! PBR text T9 (organon#217) — the CPU twin of `cube.wgsl`'s tile shading arithmetic,
//! kept the way this tree keeps a Rust mirror beside a shader (T6's `capsule_core` in
//! `particles.rs`) so the invariants can be pinned without a GPU: the **emission
//! profile** across a tile face (`tile_profile`) and the **face UV** it is keyed on
//! (`face_uv`, the dominant-axis rule the T3 crown also uses).
//!
//! ⚠️ Mirror, not import: the WGSL is the shipped code, and a test here proves the
//! *arithmetic*, not the picture. What a GPU session must look at is named in
//! `doc/arch/render.md` under "The tile". The `shader_still_carries_the_twin` test is
//! the one link between the two: it fails if the shader loses or renames either
//! function, so the twin cannot outlive its subject unnoticed.
//!
//! Production reads none of this (the world writes the knob, the GPU does the
//! arithmetic), hence the `dead_code` allow outside tests.

#![cfg_attr(not(test), allow(dead_code))]

/// Mirrors `face_uv`: the two coordinates of a mesh-local point across the face of the
/// unit cube it sits on — the face whose axis dominates `|p|`, `x` winning a tie with
/// `y` and `y` with `z` (the same rule as `face_axis`, which the T3 crown uses). The
/// result is in `[-0.5, 0.5]²` for a point on the cube's surface.
pub fn face_uv(p: [f32; 3]) -> [f32; 2] {
    let ap = [p[0].abs(), p[1].abs(), p[2].abs()];
    if ap[0] >= ap[1] && ap[0] >= ap[2] {
        [p[1], p[2]]
    } else if ap[1] >= ap[2] {
        [p[0], p[2]]
    } else {
        [p[0], p[1]]
    }
}

/// Mirrors `face_axis`: the signed dominant axis of a mesh-local point, same tie rule.
pub fn face_axis(p: [f32; 3]) -> [f32; 3] {
    let ap = [p[0].abs(), p[1].abs(), p[2].abs()];
    if ap[0] >= ap[1] && ap[0] >= ap[2] {
        [p[0].signum(), 0.0, 0.0]
    } else if ap[1] >= ap[2] {
        [0.0, p[1].signum(), 0.0]
    } else {
        [0.0, 0.0, p[2].signum()]
    }
}

/// Mirrors `tile_profile`: the emission profile at face UV `(u, v)` (each in
/// `[-0.5, 0.5]`) for strength `k` (clamped to `[0, 1]`).
///
/// `s = (2u)⁴ + (2v)⁴` clamped to 1 (a p=4 squircle radius⁴), `w = (1 − s)²`, and the
/// profile is `mix(1, w, k)`. So: exactly `1.0` everywhere at `k = 0`; `1.0` at the
/// centre for any `k`; `1 − k` on the edge midlines and in the corners; flat-topped,
/// soft-landing, and monotone from the centre outward along every ray.
pub fn tile_profile(u: f32, v: f32, k: f32) -> f32 {
    let (a, b) = (4.0 * u * u, 4.0 * v * v);
    let s = (a * a + b * b).min(1.0);
    let w = (1.0 - s) * (1.0 - s);
    let k = k.clamp(0.0, 1.0);
    1.0 * (1.0 - k) + w * k
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lattice over the face, corners and edges included, plus a few points past the
    /// edge (the bevel band can hand the profile a coordinate the flat face never would).
    fn lattice() -> Vec<(f32, f32)> {
        let mut pts = Vec::new();
        let n = 21;
        for i in 0..=n {
            for j in 0..=n {
                let u = -0.5 + i as f32 / n as f32;
                let v = -0.5 + j as f32 / n as f32;
                pts.push((u, v));
            }
        }
        pts.extend([(0.6, 0.0), (0.0, -0.7), (0.55, 0.55), (-0.9, 0.3)]);
        pts
    }

    /// Invariant #4 in one line: zero strength is EXACTLY 1.0 at every point, not close
    /// to it — the emit term must reduce to the pre-T9 expression bit for bit.
    #[test]
    fn zero_strength_is_exactly_one_everywhere() {
        for (u, v) in lattice() {
            let p = tile_profile(u, v, 0.0);
            assert!(p.to_bits() == 1.0f32.to_bits(), "profile({u}, {v}, 0) = {p}, must be exactly 1.0");
        }
        // And a negative strength is treated as zero, never as a brightening.
        assert_eq!(tile_profile(0.4, 0.1, -3.0).to_bits(), 1.0f32.to_bits());
    }

    /// The core is at the centre: `1.0` exactly at `(0, 0)` for any strength, and the
    /// edge is `1 − k` — on the midlines AND in the corners (the squircle clamps there),
    /// so the darkest a tile face can be is the strength's complement.
    #[test]
    fn the_centre_is_one_and_the_edge_is_one_minus_k() {
        for &k in &[0.0, 0.25, 0.5, 0.9, 1.0] {
            assert_eq!(tile_profile(0.0, 0.0, k), 1.0, "centre at k={k}");
            for (u, v) in [(0.5, 0.0), (0.0, 0.5), (-0.5, 0.0), (0.0, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
                let p = tile_profile(u, v, k);
                assert!((p - (1.0 - k)).abs() < 1e-6, "edge ({u}, {v}) at k={k}: {p}");
            }
            for (u, v) in lattice() {
                let p = tile_profile(u, v, k);
                assert!(p >= 1.0 - k - 1e-6 && p <= 1.0 + 1e-6, "range at ({u}, {v}), k={k}: {p}");
            }
        }
        // Past 1 the strength clamps: the edge cannot go negative and eat the neighbour's
        // light through additive bloom.
        assert!((tile_profile(0.5, 0.0, 7.0) - 0.0).abs() < 1e-6);
    }

    /// Symmetric under every sign flip and under swapping the two face axes — a tile's
    /// core sits in the middle of the tile whichever way the mesh's UV happens to run.
    #[test]
    fn the_profile_is_symmetric_under_sign_and_axis_swap() {
        let k = 0.7;
        for (u, v) in lattice() {
            let p = tile_profile(u, v, k);
            for (name, (a, b)) in [
                ("(-u, v)", (-u, v)),
                ("(u, -v)", (u, -v)),
                ("(-u, -v)", (-u, -v)),
                ("(v, u)", (v, u)),
                ("(-v, u)", (-v, u)),
                ("(v, -u)", (v, -u)),
                ("(-v, -u)", (-v, -u)),
            ] {
                let q = tile_profile(a, b, k);
                assert!(
                    (p - q).abs() < 1e-6,
                    "profile is not symmetric: profile({u}, {v}) = {p} but profile{name} = {q}"
                );
            }
        }
    }

    /// Monotone from the centre outward: along every ray from the centre the profile
    /// never rises, and along each axis it never rises with |u| or |v|. Flat-topped, so
    /// consecutive samples may tie; they may never invert.
    #[test]
    fn the_profile_is_monotone_from_centre_to_edge() {
        let k = 1.0;
        let rays = 32;
        for r in 0..rays {
            let ang = r as f32 / rays as f32 * std::f32::consts::TAU;
            let (du, dv) = (ang.cos(), ang.sin());
            let mut last = f32::INFINITY;
            for i in 0..=60 {
                let t = i as f32 / 60.0 * 0.75; // out past the corner (0.707)
                let p = tile_profile(t * du, t * dv, k);
                assert!(p <= last + 1e-6, "ray {r} rises at t={t}: {last} -> {p}");
                last = p;
            }
        }
        // And the falloff is genuinely soft rather than a step: strictly between the
        // centre and the edge at half radius, on both axes.
        let mid = tile_profile(0.25, 0.0, k);
        assert!(mid < 1.0 && mid > 0.0, "half-radius: {mid}");
        assert_eq!(mid, tile_profile(0.0, 0.25, k));
    }

    /// Fixed values, so a change to the CURVE (as opposed to a bug in its invariants) is
    /// a named diff: the plateau holds ~88 % at half radius and is 12 % at 0.9 of it.
    #[test]
    fn the_curve_is_the_one_the_shader_comment_describes() {
        let at = |q: f32| tile_profile(q * 0.5, 0.0, 1.0);
        assert!((at(0.5) - 0.87890625).abs() < 1e-6, "{}", at(0.5));
        assert!((at(0.8) - 0.348).abs() < 1e-3, "{}", at(0.8));
        assert!((at(0.9) - 0.119).abs() < 1e-3, "{}", at(0.9));
    }

    /// `face_uv` picks the dominant axis with the crown's tie rule (`x` ≥ `y` ≥ `z`), and
    /// returns the OTHER two coordinates in the shader's order (`p.yz`, `p.xz`, `p.xy`).
    /// `face_axis` agrees with it about the face on every point tested.
    #[test]
    fn face_uv_picks_the_dominant_face_with_the_crown_tie_rule() {
        // A glyph tile's front face is local +z: the UV is the (x, y) across it.
        assert_eq!(face_uv([0.1, -0.3, 0.5]), [0.1, -0.3]);
        assert_eq!(face_axis([0.1, -0.3, 0.5]), [0.0, 0.0, 1.0]);
        // Back face, same UV convention.
        assert_eq!(face_uv([0.1, -0.3, -0.5]), [0.1, -0.3]);
        assert_eq!(face_axis([0.1, -0.3, -0.5]), [0.0, 0.0, -1.0]);
        // Side walls.
        assert_eq!(face_uv([0.5, 0.2, 0.1]), [0.2, 0.1]);
        assert_eq!(face_uv([-0.2, 0.5, 0.1]), [-0.2, 0.1]);
        // Ties: x beats y beats z, exactly as the crown decided before T9.
        assert_eq!(face_axis([0.5, 0.5, 0.5]), [1.0, 0.0, 0.0]);
        assert_eq!(face_uv([0.5, 0.5, 0.5]), [0.5, 0.5]);
        assert_eq!(face_axis([0.1, 0.5, 0.5]), [0.0, 1.0, 0.0]);
        assert_eq!(face_uv([0.1, 0.5, 0.5]), [0.1, 0.5]);
        // The two agree about the face everywhere: the axis is the one coordinate the
        // UV dropped.
        for p in [[0.5, 0.1, 0.2], [0.1, -0.5, 0.2], [0.3, 0.2, 0.5], [-0.4, 0.4, 0.4], [0.0, 0.0, 0.5]] {
            let ax = face_axis(p);
            let uv = face_uv(p);
            let dropped = if ax[0] != 0.0 { p[0] } else if ax[1] != 0.0 { p[1] } else { p[2] };
            let kept: Vec<f32> = p.iter().copied().filter(|&c| c != dropped).collect();
            assert_eq!(kept.len(), 2, "{p:?}");
            assert!(uv.contains(&kept[0]) && uv.contains(&kept[1]), "{p:?}: axis {ax:?}, uv {uv:?}");
        }
    }

    /// The one link between twin and subject: the shader must still define both
    /// functions with the signatures mirrored here, and `fs_main` must apply the profile
    /// to the per-instance term keyed on `u.shape.z`. A rename or a removal on either
    /// side fails here by name rather than leaving a twin that tests nothing.
    #[test]
    fn shader_still_carries_the_twin() {
        let src = include_str!("cube.wgsl");
        for needle in [
            "fn tile_profile(uv: vec2<f32>, k: f32) -> f32",
            "fn face_uv(p: vec3<f32>) -> vec2<f32>",
            "fn face_axis(p: vec3<f32>) -> vec3<f32>",
            "tile_profile(face_uv(in.local_pos), u.shape.z)",
            "let s = min(a.x * a.x + a.y * a.y, 1.0);",
            "let w = (1.0 - s) * (1.0 - s);",
        ] {
            assert!(src.contains(needle), "cube.wgsl no longer carries `{needle}` — update glyph_tile.rs with it");
        }
    }
}
