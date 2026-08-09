//! The `math` ↔ `ipc` boundary: does `VecBuildSpec::from_slots` read `Shared`'s
//! default `vecbuild` block as the flagship field it is meant to encode?
//!
//! # Why this file exists (#626 Tier 3 PR B)
//!
//! These tests lived in `math.rs`'s test module and were the **only** thing there
//! reaching `crate::ipc::Shared` — the single reference #536 T4 lists as "reference #3,
//! resolves by co-location". Reading it showed co-location was never required: the
//! coupling is test-only, so `ipc.rs` did not have to follow `math.rs` into
//! `organon-core` at all. Moving these four items here resolved reference #3 outright
//! and left `ipc.rs` at a **zero diff**, which is a far stronger guarantee about the
//! `Shared` layout than any amount of re-reading it would have been.
//!
//! They belong on this side of the boundary anyway: they assert that two crates agree
//! about a wire format, so they cannot live inside either one.
//!
//! An integration test rather than a unit test because it needs `organon_core::math`
//! and `organic_math_native::ipc` simultaneously — everything it touches is public API.
//!
//! ⚠️ **These bodies are taken from `main`, not from the pre-move copy.** #641 (#618 T1a)
//! refactored them from `Vec<Strand>` to the recycling `Strands` bundle *while this PR
//! had them in flight*. Git conflicted on the `math.rs` deletion — which is the lucky
//! half — but it cannot know that a relocated copy in another file also needed the
//! refactor. Resolving the conflict alone would have left this file compiling against a
//! signature that no longer exists. Taking `main`'s bodies wholesale is what keeps the
//! relocation faithful rather than a fork.

use organic_math_native::ipc::Shared;
use organic_math_native::math::{
    vecfield_eval, vecfield_field, vecfield_lines_strands, vecfield_strands, Strands,
    VecBuildSpec, VECFIELD_CUSTOM,
};
use glam::Vec3;

mod boundary {
    use super::*;

    /// The default builder spec exactly as `ipc::Shared::default().vecbuild`
    /// packs it (the flagship: Fx = y², Fy = −x², Fz = 0.5·sin z, direct op).
    fn default_build_spec() -> VecBuildSpec {
        VecBuildSpec::from_slots(&Shared::default().vecbuild)
    }

    #[test]
    fn vecbuild_default_spec_reproduces_the_flagship() {
        let spec = default_build_spec();
        for &p in &[Vec3::new(1.0, 2.0, 0.4), Vec3::new(-1.3, 0.7, -2.0), Vec3::ZERO] {
            let f = spec.eval(p);
            let want = Vec3::new(p.y * p.y, -(p.x * p.x), 0.5 * p.z.sin());
            assert!((f - want).length() < 1.0e-5, "builder default != flagship at {p}: {f}");
        }
        // And it rides the generator entry point: preset 12 + spec == the spec;
        // preset 12 with NO spec falls back to the bank's parabolic swirl.
        let p = Vec3::new(0.8, -1.1, 0.5);
        let via = vecfield_field(VECFIELD_CUSTOM, Some(&spec), p, 1.0, 0.0, 0.0);
        assert!((via - spec.eval(p)).length() < 1.0e-6);
        let fallback = vecfield_field(VECFIELD_CUSTOM, None, p, 1.0, 0.0, 0.0);
        assert!((fallback - vecfield_eval(0, p, 0.0)).length() < 1.0e-6, "spec-less Custom falls back");
    }

    #[test]
    fn vecbuild_helmholtz_blend_interpolates_gradient_to_curl() {
        // One spec carrying both a potential row (Fx) and a vector potential:
        // mix 0 must equal the gradient op, mix 1 the curl op.
        let mut spec = default_build_spec();
        spec.op = 3;
        let p = Vec3::new(0.9, -0.6, 1.2);
        let grad = VecBuildSpec { op: 1, ..spec }.eval(p);
        let curl = VecBuildSpec { op: 2, ..spec }.eval(p);
        spec.mix = 0.0;
        assert!((spec.eval(p) - grad).length() < 1.0e-6, "mix 0 = gradient");
        spec.mix = 1.0;
        assert!((spec.eval(p) - curl).length() < 1.0e-6, "mix 1 = curl");
        spec.mix = 0.5;
        let mid = spec.eval(p);
        assert!((mid - (grad + curl) * 0.5).length() < 1.0e-6, "mix 0.5 = the average");
    }

    #[test]
    fn vecbuild_custom_rides_arrows_and_field_lines() {
        // The Custom entry must flow through both Tier-1 arrows and Tier-2
        // lines: non-empty, finite, and the arrow directions match the spec.
        let spec = default_build_spec();
        let mut arrows = Strands::new();
        vecfield_strands(
            VECFIELD_CUSTOM, Some(&spec), 4, 4, 2, 8.0, 1.0, 1.0, 0.1, 0, 0, 0.0, 0.0, 0.0, 0.0,
            0, 0.0, &mut arrows,
        );
        assert!(!arrows.is_empty());
        for strand in &arrows {
            let mid = (strand[0].position + strand[1].position) * 0.5;
            let f = spec.eval(mid);
            if f.length() > 1.0e-4 {
                let dir = strand[0].tangent.unwrap();
                assert!(dir.dot(f.normalize()) > 0.99, "arrow direction != spec field");
            }
        }
        let mut lines = Strands::new();
        vecfield_lines_strands(
            VECFIELD_CUSTOM, Some(&spec), 0, 12, 80, 0.1, true, 8.0, 1.0, 0, 0.06, 0.0, 1.0, 0.0,
            0.0, 0.4, 0, 0.0, &mut lines,
        );
        assert!(!lines.is_empty(), "Custom must trace field lines too");
        for strand in &lines {
            for fr in strand {
                assert!(fr.position.is_finite() && fr.tint.is_finite());
            }
        }
    }

}
