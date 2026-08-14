//! `substrate_scene`'s and `substrate_materials`'s tests, relocated (organon#49 Tier 3).
//!
//! # Why they are not beside the code they test
//!
//! Both modules moved to `organon-scene`, which carries **no `nih_plug`** — that is the
//! crate's whole acceptance test. These tests cannot follow them, because their baseline
//! is the plugin's own default parameter set:
//!
//! ```ignore
//! OrganicMathParams::default().to_shared()
//! ```
//!
//! ⚠️ **`Shared::default()` is not a substitute, and the difference is silent.** Core
//! documents its `Default` as *"the web app's helix defaults, so the visual shows
//! something sensible before the plugin has written anything"*; these fixtures document
//! theirs as *"the real host parameter set"*. Swapping one for the other changes what
//! every assertion below is measured against **without changing whether it passes** —
//! which is the failure mode this repo keeps writing down rather than discovering.
//!
//! So the tests move up to where `OrganicMathParams` still is. That is the same answer
//! #626 Tier 3 reached when `math.rs` went down and one test needed `Shared`
//! (`native/tests/vecbuild_ipc.rs`), and `organon-core/src/lib.rs` records it as the
//! precedent.
//!
//! **The bodies are byte-for-byte what they were** — only the `use` headers changed, to
//! name the crates the types live in now. If you are reading this because an assertion
//! failed, the assertion is unchanged since it last passed in `substrate_scene.rs`.

mod scene {
    use glam::{DVec3, Vec3};
    use organic_math_native::params::{FuncName, OrganicMathParams, ParamValues};
    use organon_core::ipc::Shared;
    use organon_core::math;
    use organon_scene::substrate_scene::*;

    /// The baseline every test starts from: the real host parameter set, with no host, no
    /// audio thread and no GPU (`params.rs:3982-3983`, `:9281`).
    fn default_shared() -> Shared {
        OrganicMathParams::default().to_shared()
    }

    fn applied() -> Shared {
        let mut s = default_shared();
        apply_substrate_look(&mut s);
        s
    }

    // -----------------------------------------------------------------------------
    // (a) every field this file claims to set, at its exact value
    // -----------------------------------------------------------------------------
    #[test]
    fn apply_sets_every_declared_field() {
        let s = applied();

        // geometry — the membrane path
        assert_eq!(s.generator, 0, "generator must be Original");
        assert_eq!(s.surface_mode, 4, "surface_mode must be Membrane");
        assert_eq!(s.origin_mode, 1, "origin_mode must be Centered");
        assert_eq!(s.loop_count, [128.0, 1.0, 128.0, 0.0]);
        assert_eq!(s.membrane, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(s.rot_amp, [0.0; 4]);
        assert_eq!(s.trans_amp, [0.0; 4]);
        assert_eq!(s.trans_mod, [0.0; 4]);

        // stillness
        assert_eq!(s.animate, 0, "the animation clock must not advance");
        assert_eq!(s.pulse, 0, "the pulse rewrites `s` itself — it must be off");
        assert_eq!(s.routing[1], 0.0);
        assert_eq!(s.routing[3], 0.0);
        assert_eq!(s.breath[0], 0.0, "Breath scene scale");
        assert_eq!(s.camera[0], 0.0, "auto-orbit path must be Off");
        assert_eq!(s.bio[0], 0.0, "colour-cycle clock");
        assert_eq!(s.bio[1], 0.0, "emissive ripple intensity");
        assert_eq!(s.bio[2], 0.0, "ripple-phase clock");
        assert_eq!(s.rd[3], 0.0);
        assert_eq!(s.rd[4], 0.0);

        // albedo
        assert_eq!(s.surface_fx[6], 0.0, "palette must be Native");
        assert_eq!(s.matcol[0], 0.0, "hue");
        assert_eq!(s.matcol[1], 0.0, "hue cycle — the beat-driven one");
        assert_eq!(s.matcol[2], 0.0, "saturation 0 = neutral slate");
        assert_eq!(s.matcol[3], 1.0, "value");

        // the lighting rig
        assert_eq!(s.lighting, [1.0, 2.6, 0.35, 42.0, -10.0, 0.0, 1.0, 0.0]);
        assert_eq!(s.pbr[0], 0.0, "metallic");
        assert_eq!(s.pbr[1], 0.42, "roughness");
        assert_eq!(s.pbr[2], 0.0, "exposure EV");
        assert_eq!(s.pbr[3], 0.65, "env intensity");
        assert_eq!(s.pbr[4], 0.0, "env rotation");
        assert_eq!(s.pbr[5], 0.0, "bloom");
        assert_eq!(s.bg_visible, 0);

        // never ours to touch
        let d = default_shared();
        assert_eq!(s.seq, d.seq, "`seq` belongs to the publisher");
        assert_eq!(s.layout_version, d.layout_version, "layout is append-only");
    }

    // -----------------------------------------------------------------------------
    // (b) the manifest: nothing outside the declared field list changes
    // -----------------------------------------------------------------------------
    #[test]
    fn apply_touches_only_the_manifest_fields() {
        let d = default_shared();
        let mut restored = applied();
        for (_, restore) in substrate_field_manifest() {
            restore(&mut restored, &d);
        }
        assert_eq!(
            bytemuck::bytes_of(&restored),
            bytemuck::bytes_of(&d),
            "apply_substrate_look wrote a field that substrate_field_manifest does not declare"
        );
    }

    /// The other direction: every declared field must actually be reachable, i.e. the
    /// manifest may not accumulate names for fields nobody writes any more. A manifest
    /// entry earns its place by being either a change or a deliberate pin, and both are
    /// covered by the value assertions above — so all this asserts is that the list is
    /// non-empty and names are unique (a duplicate would silently mask a stray write).
    #[test]
    fn manifest_names_are_unique() {
        let m = substrate_field_manifest();
        assert!(!m.is_empty());
        for (i, (a, _)) in m.iter().enumerate() {
            for (b, _) in m.iter().skip(i + 1) {
                assert_ne!(a, b, "duplicate manifest entry `{a}`");
            }
        }
    }

    /// Which manifest fields are real *changes* against today's defaults, and which are
    /// deliberate pins. Not a correctness claim — a tripwire: if a default moves so that a
    /// pin becomes a change (or vice versa), the reasoning in this file's doc comments
    /// needs re-reading, and this is where that shows up.
    #[test]
    fn changes_versus_pins_are_as_documented() {
        let d = default_shared();
        let s = applied();
        // Real changes against `OrganicMathParams::default()`.
        assert_ne!(s.surface_mode, d.surface_mode);
        assert_ne!(s.origin_mode, d.origin_mode);
        assert_ne!(s.loop_count, d.loop_count);
        assert_ne!(s.membrane, d.membrane);
        assert_ne!(s.animate, d.animate);
        assert_ne!(s.lighting, d.lighting);
        assert_ne!(s.pbr, d.pbr);
        assert_ne!(s.matcol, d.matcol);
        assert_ne!(s.bg_visible, d.bg_visible);
        // `bio` is a CHANGE, not a pin, and the reason is worth keeping in a test:
        // `ripple_speed` (`bio[2]`) defaults to 0.3 (`params.rs:8781`) and `world.rs:2330`
        // advances the ripple phase by it every frame, ungated. See SUBSTRATE_BIO_CLOCK.
        assert_ne!(s.bio, d.bio);
        // Deliberately an inequality, not `== 0.3`: the default round-trips through
        // nih-plug's normalization, so the exact bits are not ours to predict.
        assert!(d.bio[2] > 0.0, "the stock ripple-speed clock is running");
        // Deliberate pins: already correct at the default, written anyway.
        assert_eq!(s.generator, d.generator);
        assert_eq!(s.rot_amp, d.rot_amp);
        assert_eq!(s.trans_amp, d.trans_amp);
        assert_eq!(s.trans_mod, d.trans_mod);
        assert_eq!(s.pulse, d.pulse);
        assert_eq!(s.routing, d.routing);
        assert_eq!(s.breath, d.breath);
        assert_eq!(s.camera, d.camera, "R2's claim: auto-orbit already Off");
        assert_eq!(s.rd, d.rd);
        assert_eq!(s.surface_fx, d.surface_fx);
    }

    // -----------------------------------------------------------------------------
    // (c) idempotence
    // -----------------------------------------------------------------------------
    #[test]
    fn apply_is_idempotent() {
        let once = applied();
        let mut twice = applied();
        apply_substrate_look(&mut twice);
        assert_eq!(bytemuck::bytes_of(&twice), bytemuck::bytes_of(&once));
    }

    /// Stronger than idempotence: applying over an arbitrary *dirtied* snapshot lands on
    /// the same look, so the console can switch to the substrate from whatever the World
    /// happened to be showing. Only fields `apply` writes **whole** are compared — the
    /// partially-written blocks (`routing`, `breath`, `camera`, `bio`, `rd`,
    /// `surface_fx`, `matcol`, `pbr`) keep their other lanes on purpose, which is the
    /// point of writing lanes rather than blocks there.
    #[test]
    fn apply_overrides_a_dirty_snapshot() {
        let mut dirty = default_shared();
        dirty.generator = 2; // DNA
        dirty.surface_mode = 3; // Metaball
        dirty.origin_mode = 0; // Corner
        dirty.animate = 1;
        dirty.pulse = 1;
        dirty.bg_visible = 1;
        dirty.loop_count = [20.0, 20.0, 20.0, 6.0];
        dirty.membrane = [4.0, 1.0, 1.0, 1.0]; // Web weave, strands + arms + seam on
        dirty.rot_amp = [90.0, 45.0, 10.0, 1.0];
        dirty.trans_amp = [7.0, 7.0, 7.0, 0.0];
        dirty.trans_mod = [5.0, -5.0, 5.0, 0.0];
        dirty.lighting = [3.0; 8];
        apply_substrate_look(&mut dirty);

        let clean = applied();
        assert_eq!(dirty.generator, clean.generator);
        assert_eq!(dirty.surface_mode, clean.surface_mode);
        assert_eq!(dirty.origin_mode, clean.origin_mode);
        assert_eq!(dirty.animate, clean.animate);
        assert_eq!(dirty.pulse, clean.pulse);
        assert_eq!(dirty.bg_visible, clean.bg_visible);
        assert_eq!(dirty.loop_count, clean.loop_count);
        assert_eq!(dirty.membrane, clean.membrane);
        assert_eq!(dirty.rot_amp, clean.rot_amp);
        assert_eq!(dirty.trans_amp, clean.trans_amp);
        assert_eq!(dirty.trans_mod, clean.trans_mod);
        assert_eq!(dirty.lighting, clean.lighting);

        // And the geometry that comes out the far end is the same mesh.
        assert_eq!(
            build_sheet(&dirty, DVec3::ZERO).0,
            build_sheet(&clean, DVec3::ZERO).0,
            "a dirty snapshot must converge to the same sheet"
        );
    }

    // -----------------------------------------------------------------------------
    // (d) the membrane gate pair still holds after apply
    // -----------------------------------------------------------------------------
    /// `world.rs:2334` + `:2656-2657`: the lofted sheet is built only when
    /// `surface_mode == 4` **and** `generator == Original` **and** Skin-Arms is off. All
    /// three, restated exactly as the World evaluates them.
    #[test]
    fn membrane_gate_holds() {
        let s = applied();
        let membrane_mode = s.surface_mode == 4; // world.rs:2334
        let membrane_arms = membrane_mode && s.membrane[2] > 0.5; // world.rs:2341
        let draw_membrane_mesh = membrane_mode && s.generator == 0 && !membrane_arms;
        assert!(draw_membrane_mesh, "the membrane mesh gate must be open");

        // The competing render paths ahead of Membrane in `world.rs:7993-8026` are all
        // generator- or surface-mode-driven; at Original + Membrane none can win.
        assert_ne!(s.surface_mode, 3, "Metaball would take the path");
        assert_ne!(s.surface_mode, 5, "Voxel would take the path");
        assert_ne!(s.surface_mode, 6, "Volume would take the path");
        // Show Strands would also draw the boundary strands as swept tubes (world.rs:2337).
        assert!(s.membrane[1] <= 0.5, "Show Strands must be off");
    }

    // -----------------------------------------------------------------------------
    // The geometry itself — built headless through the same call the World makes
    // -----------------------------------------------------------------------------

    /// Build the membrane mesh exactly as `world.rs:2815-2831` does, with the caller
    /// supplying the animation clock so a test can vary it. `ParamValues` is assembled
    /// field-for-field as at `world.rs:2225-2232`.
    fn build_sheet(s: &Shared, angle: DVec3) -> (Vec<Vec3>, Vec<Vec3>, Vec<glam::Vec4>, Vec<u32>) {
        let pv = ParamValues {
            loop_count: Vec3::new(s.loop_count[0], s.loop_count[1], s.loop_count[2]),
            loop_count_q: s.loop_count[3],
            rot_amp: Vec3::new(s.rot_amp[0], s.rot_amp[1], s.rot_amp[2]),
            trans_amp: Vec3::new(s.trans_amp[0], s.trans_amp[1], s.trans_amp[2]),
            trans_mod: Vec3::new(s.trans_mod[0], s.trans_mod[1], s.trans_mod[2]),
            scale_amp: s.scale_amp, // unread by `draw_membrane`; carried for fidelity
        };
        let (mut pos, mut norm, mut col, mut idx) = (vec![], vec![], vec![], vec![]);
        math::draw_membrane(
            &pv,
            FuncName::from_u32(s.rot_func),
            FuncName::from_u32(s.trans_func),
            angle,
            angle,                  // rot_phase: pendulum mode reads `angle` (world.rs:2324)
            s.rot_amp[3] != 0.0,    // continuous (world.rs:2323)
            s.origin_mode != 0,     // centered (world.rs:2754)
            s.surface_fx[6] as u32, // palette (world.rs:2345)
            0.0,                    // color_phase — frozen by `bio[0] = 0`
            s.membrane[0] as u32,   // weave id
            s.membrane[3] > 0.5,    // close the 360° seam
            &mut pos,
            &mut norm,
            &mut col,
            &mut idx,
        );
        (pos, norm, col, idx)
    }

    /// ONE sheet, flat on world y = 0, centred on the origin, with the vertex and triangle
    /// counts the constants promise.
    #[test]
    fn the_sheet_is_one_flat_plane_on_the_world_xz_plane() {
        let s = applied();
        let (pos, norm, col, idx) = build_sheet(&s, DVec3::ZERO);

        let (nx, nz) = (SUBSTRATE_GRID_X as usize, SUBSTRATE_GRID_Z as usize);
        assert_eq!(pos.len(), nx * nz, "exactly one sheet of nx × nz vertices");
        assert_eq!(idx.len(), 6 * (nx - 1) * (nz - 1), "two triangles per quad");
        assert_eq!(norm.len(), pos.len());
        assert_eq!(col.len(), pos.len());

        // Flat: every vertex on y = 0.
        for p in &pos {
            assert_eq!(p.y, 0.0, "the sheet must lie exactly on world y = 0");
        }

        // Extent + centring: a unit-pitch lattice symmetric about the origin.
        let half_x = (nx as f32 - 1.0) * 0.5;
        let half_z = (nz as f32 - 1.0) * 0.5;
        let (mut mnx, mut mxx, mut mnz, mut mxz) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in &pos {
            mnx = mnx.min(p.x);
            mxx = mxx.max(p.x);
            mnz = mnz.min(p.z);
            mxz = mxz.max(p.z);
        }
        assert_eq!((mnx, mxx), (-half_x, half_x), "x extent, centred");
        assert_eq!((mnz, mxz), (-half_z, half_z), "z extent, centred");
        assert_eq!(mxx - mnx, 127.0, "127 world units across — see KNOWN_UNSTILLABLE #4");

        // One constant normal. `push_sheet` winds x × z, so the geometric normal is −Y;
        // `cube.wgsl:1382-1385` flips it toward the viewer, which is why a sheet lights
        // correctly from either side. What matters here is that it is CONSTANT.
        for n in &norm {
            assert_eq!(*n, Vec3::NEG_Y, "a flat sheet has one normal everywhere");
        }

        // Native palette → per-vertex colour is the normalized position in the bounds
        // (math.rs:12257-12262). Flat in y ⇒ the green channel is identically 0, and the
        // red/blue ramps span the full 0..1 — the albedo variation R5 asks for.
        let (mut mnr, mut mxr) = (f32::MAX, f32::MIN);
        for c in &col {
            assert_eq!(c.y, 0.0, "flat in y ⇒ no green component");
            mnr = mnr.min(c.x);
            mxr = mxr.max(c.x);
        }
        assert_eq!((mnr, mxr), (0.0, 1.0), "the albedo ramp must span the sheet");
    }

    /// Stillness, mechanically: the mesh is **byte-identical** at wildly different
    /// animation-clock phases. This is the property `SUBSTRATE_TRANS_AMP` and
    /// `SUBSTRATE_ROT_AMP` exist for — with either non-zero, `angle` reaches the vertex
    /// positions through `math.rs:11820-11824` and this fails.
    #[test]
    fn the_sheet_does_not_move_with_the_animation_clock() {
        let s = applied();
        let a = build_sheet(&s, DVec3::ZERO);
        let b = build_sheet(&s, DVec3::new(123.456, -78.9, 4242.0));
        assert_eq!(a.0, b.0, "positions moved with the animation clock");
        assert_eq!(a.1, b.1, "normals moved with the animation clock");
        assert_eq!(a.3, b.3, "topology changed with the animation clock");

        // And the guard is real, not vacuous: put the clock back into the expression and
        // the same comparison must fail.
        let mut breathing = s;
        breathing.trans_amp[0] = 1.0;
        let c = build_sheet(&breathing, DVec3::ZERO);
        let d = build_sheet(&breathing, DVec3::new(1.0, 0.0, 0.0));
        assert_ne!(c.0, d.0, "control: non-zero trans_amp must make the lattice breathe");
    }
}


mod materials {
    use organic_math_native::params::OrganicMathParams;
    use organon_core::ipc::Shared;
    use organon_scene::substrate_materials::*;
    use organon_scene::substrate_scene::{self, apply_substrate_look};

    /// The baseline every test starts from: Tier 1's substrate snapshot, which is what these
    /// functions are specified as deltas on.
    fn substrate() -> Shared {
        let mut s = OrganicMathParams::default().to_shared();
        apply_substrate_look(&mut s);
        s
    }

    fn with_material(name: &str) -> Shared {
        let mut s = substrate();
        assert!(apply_material(&mut s, name), "`{name}` must be a known material");
        s
    }

    fn with_rig(name: &str) -> Shared {
        let mut s = substrate();
        assert!(apply_rig(&mut s, name), "`{name}` must be a known rig");
        s
    }

    // -----------------------------------------------------------------------------
    // (a) every value, exactly
    // -----------------------------------------------------------------------------

    /// The shared map-stack preamble, identical for all four.
    #[test]
    fn every_material_writes_the_same_projection_and_bake_size() {
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            assert_eq!(s.material[1], 1.0, "{name}: projection must be world-planar XZ");
            assert_eq!(s.material_layer[16], 1.0, "{name}: procedural must be ON");
            assert_eq!(s.material_layer[17], 512.0, "{name}: bake size in PIXELS, not the enum");
            // The stillness pair — a material must not animate or displace.
            assert_eq!(s.material_live[0], 0.0, "{name}: the material bake must not animate");
            assert_eq!(s.material_live[5], 0.0, "{name}: height→vertex displace must stay off");
        }
    }

    /// `uv_scale` is per-material, but only one material may depart from the floor — and only
    /// because `mp_scale` caps at 64. A departure costs sampling headroom (32.5·s texels/px),
    /// which is affordable for bandlimited content and not for a Sobel-derived normal, so the
    /// rule is pinned rather than left as prose: **any material that derives a normal stays at
    /// [`MATERIAL_UV_SCALE`].** `paper` is the case this protects.
    #[test]
    fn only_bandlimited_materials_leave_the_uv_scale_floor() {
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            let derives_normal = s.material_derive[0] > 0.5;
            if derives_normal {
                assert_eq!(
                    s.material[2], MATERIAL_UV_SCALE,
                    "{name} derives a normal, so it must sample at the floor"
                );
            }
            // Whatever it chose, the sampling rate must stay inside the range where bilinear
            // without mips is honest (see the module doc's table).
            let texels_per_px = 32.5 * s.material[2];
            assert!(
                texels_per_px <= 2.0,
                "{name}: {texels_per_px} texels/px on a 2000-px pane is past the no-mip budget"
            );
        }
        assert_eq!(GRAPHITE.uv_scale, 0.055, "graphite is the one departure");
        for m in [&SLATE, &PAPER, &METAL] {
            assert_eq!(m.uv_scale, MATERIAL_UV_SCALE, "`{}` must stay at the floor", m.name);
        }
    }

    #[test]
    fn slate_sets_every_declared_field() {
        let s = with_material("slate");
        assert_eq!(
            s.material_layer,
            [3.0, 1.0, 6.0, 0.0, 0.0, 0.0, 4.0, 2.0, 0.5, 0.25, 0.20, 1.30, 0.0, 1.0, 0.0, 0.0, 1.0, 512.0]
        );
        assert_eq!(s.material_layer2[16], 0.0, "slate has no overlay");
        assert_eq!(s.material_derive, [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 0.0, 0.0]);
        assert_eq!(s.lighting[7], 0.0, "Standard");
        assert_eq!(s.pbr[0], 0.0, "metallic");
        assert_eq!(s.pbr[1], 0.42, "roughness = the map's mean");
        assert_eq!(s.aniso, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!([s.matcol[0], s.matcol[1], s.matcol[2], s.matcol[3]], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn graphite_sets_every_declared_field() {
        let s = with_material("graphite");
        assert_eq!(s.material[2], 0.055, "the one material off the uv_scale floor");
        assert_eq!(
            s.material_layer,
            [3.0, 0.0, 8.0, 0.0, 0.0, 0.0, 3.0, 2.0, 0.55, 0.30, 1.10, 1.0, 0.15, 0.90, 0.0, 3.0, 1.0, 512.0]
        );
        assert_eq!(s.material_grad, [0.012, 0.012, 0.015, 0.0, 0.055, 0.055, 0.062, 0.0]);
        assert_eq!(
            s.material_layer2,
            [12.0, 1.0, 64.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.5, 0.10, 0.30, 2.40, 0.0, 1.0, 0.0, 5.0, 1.0, 0.0]
        );
        // No derived maps: an albedo-sourced normal is inert on a material this dark (the
        // Sobel scales with the albedo's luminance range — see the doc comment).
        assert_eq!(s.material_derive, DERIVE_NONE);
        assert_eq!(s.lighting[7], 0.0, "Standard — the anisotropy is an OVERLAY");
        assert_eq!(s.pbr[0], 0.0, "metallic 0: a near-black metal has no specular colour");
        assert_eq!(s.pbr[1], 0.19, "= the roughness map's centre; see the band test below");
        assert_eq!(s.aniso, [0.45, 0.0, 1.0, 0.65]);
        assert_eq!(s.matcol[2], 0.35, "saturation");
    }

    /// The beat-check regression, and it guards the **opposite** of what its predecessor did.
    ///
    /// That test floored graphite's roughness — `assert!(lo > 0.40, "still a mirror")` — on the
    /// reasoning that a dielectric's specular is albedo-independent, so a *glossy* near-black
    /// plane is a dark mirror of a bright sky and therefore bright. The first half is true and
    /// the conclusion is not, and believing it drove `gamma` 1.55 → 0.83 → 0.20, each step
    /// making the sheet **paler**. Measured on the pane (2000 px, scrim 96, studio):
    ///
    /// ```text
    ///     band centre   0.189   0.250   0.563   0.871
    ///     graphite       99.7   114.9   159.2   176.2     slate 96.3, paper 155.2
    /// ```
    ///
    /// Rougher is brighter here. The substrate camera is top-down, so the mirror direction is
    /// the **zenith** — the dark part of the baked Nishita sky — and roughness raises the
    /// prefiltered LOD (`cube.wgsl:1071-1074`) until the sample is the bright hemispherical
    /// mean instead. [`METAL`] is the standing proof: glossiest material in the file, and it
    /// measures 29.6 where slate measures 96.3.
    ///
    /// So the guard is a **ceiling** on the band, never a floor. Let graphite drift matte and
    /// it goes pale again — which is the bug this file has now had twice.
    #[test]
    fn graphite_band_stays_glossy_enough_to_read_dark() {
        // The bake's own arithmetic (material_bake.wgsl:315-322), restated.
        let band = |contrast: f32, gamma: f32, raw: f32| {
            ((raw - 0.5) * contrast + 0.5).clamp(0.0, 1.0).powf(gamma.max(1e-3))
        };
        let l = GRAPHITE.overlay.expect("graphite bakes a roughness overlay");
        assert_eq!(l.channel, 1.0, "…to the Roughness channel");
        let (lo, hi) = (band(l.contrast, l.gamma, 0.0), band(l.contrast, l.gamma, 1.0));

        // The band the doc comment quotes, recomputed from the constants rather than copied
        // beside them — so the prose cannot drift away from the numbers it describes.
        assert!((lo - 0.081).abs() < 1e-3, "glossiest stripe {lo}, doc quotes 0.081");
        assert!((hi - 0.356).abs() < 1e-3, "mattest stripe {hi}, doc quotes 0.356");

        // The regression itself: at a centre of 0.563 graphite measured 159.2, *brighter than
        // paper*. Nothing may drift back toward that.
        assert!(hi < 0.40, "mattest stripe {hi} is into the pale regime (0.563 read 159.2)");
        // The lamination is the band's own width, so it may not be bought back by flattening.
        // Iteration 1's gamma 0.20 collapsed this to 0.107 and the stripes vanished.
        assert!(hi - lo > 0.20, "band span {} is too narrow to read as lamination", hi - lo);

        // The map-absent scalar tracks the map's centre — `0.5^gamma`, the same definition
        // slate (1.30 → 0.406, declares 0.42) and metal (2.20 → 0.218, declares 0.22) use — or
        // a dropped roughness bit silently restores the failure above.
        let centre = band(l.contrast, l.gamma, 0.5);
        assert!(
            (GRAPHITE.roughness - centre).abs() < 0.02,
            "scalar roughness {} disagrees with the map's centre {centre}",
            GRAPHITE.roughness
        );
    }

    #[test]
    fn paper_sets_every_declared_field() {
        let s = with_material("paper");
        assert_eq!(
            s.material_layer,
            [3.0, 0.0, 10.0, 0.0, 0.0, 0.0, 4.0, 2.2, 0.45, 0.60, 0.45, 1.0, 0.20, 0.85, 0.0, 7.0, 1.0, 512.0]
        );
        assert_eq!(s.material_grad, [0.300, 0.290, 0.265, 0.0, 0.440, 0.430, 0.395, 0.0]);
        assert_eq!(
            s.material_layer2,
            [3.0, 3.0, 40.0, 0.0, 0.0, 0.0, 2.0, 2.2, 0.5, 0.15, 0.55, 1.0, 0.0, 1.0, 0.0, 11.0, 1.0, 0.0]
        );
        assert_eq!(s.material_derive, [1.0, 0.0, 0.0, 0.22, 1.0, 2.0, 0.0, 0.0]);
        assert_eq!(s.pbr[1], 0.78, "matte, and unopposed: no roughness channel is baked");
        assert_eq!(s.aniso, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(s.matcol[3], 1.0, "value stays identity — the gradient is the one level dial");
    }

    /// The beat-check regression: **nothing may modulate the ambient term.** Derived AO is
    /// applied as `ambient_mul *= m_ao` at full strength (`cube.wgsl:1419`) after
    /// `material_bake.wgsl:437` has already multiplied the occlusion by **4**, so on a rig
    /// whose environment is most of the light it stamps dark holes into the backdrop — the
    /// "clots that fight the glyphs". No material here derives AO any more.
    ///
    /// The second half is the legibility floor the scrim implies, checked at the **dark** end,
    /// because that is where glyph contrast is actually lost.
    #[test]
    fn paper_cannot_clot_behind_the_glyphs() {
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            assert_eq!(s.material_derive[1], 0.0, "{name}: derived AO modulates the ambient");
        }
        // The albedo the bake can actually produce, restated from material_bake.wgsl:315-322
        // + :338-340 — remap expands, contrast compresses, then the gradient maps it.
        let l = PAPER.base;
        let shaped = |raw: f32| {
            let v = ((raw - l.remap[0]) / (l.remap[1] - l.remap[0])).clamp(0.0, 1.0);
            ((v - 0.5) * l.contrast + 0.5).clamp(0.0, 1.0)
        };
        let albedo = |raw: f32| PAPER.base_grad[0] + (PAPER.base_grad[4] - PAPER.base_grad[0]) * shaped(raw);
        let (lo, hi) = (albedo(0.0), albedo(1.0));
        assert!(lo > 0.30, "paper's darkest albedo is {lo} — the clot end");
        assert!(hi < 0.46, "paper's brightest albedo is {hi} — too hot behind a 96 scrim");
        assert!(hi - lo < 0.10, "a {} albedo span reads as pattern, not formation", hi - lo);
    }

    #[test]
    fn metal_sets_every_declared_field() {
        let s = with_material("metal");
        assert_eq!(
            s.material_layer,
            [12.0, 0.0, 40.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.5, 0.12, 0.50, 1.0, 0.0, 1.0, 0.0, 2.0, 1.0, 512.0]
        );
        assert_eq!(s.material_grad, [0.520, 0.530, 0.550, 0.0, 0.680, 0.690, 0.720, 0.0]);
        assert_eq!(
            s.material_layer2,
            [12.0, 1.0, 64.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.5, 0.15, 0.25, 2.20, 0.0, 1.0, 0.0, 9.0, 1.0, 0.0]
        );
        // No derived normal: the anisotropic lobe IS the brushed cue, and a Sobel normal off
        // the same stripes would double it at texel scale.
        assert_eq!(s.material_derive, DERIVE_NONE);
        assert_eq!(s.lighting[7], 4.0, "MaterialType::Anisotropic — params.rs:3710-3711");
        assert_eq!(s.pbr[0], 1.0, "a real metal: albedo IS the specular colour");
        assert_eq!(s.pbr[1], 0.22);
        assert_eq!(s.aniso[0], 0.75, "raw amount — overlay enable/blend are unread at type 4");
        assert_eq!(s.matcol[2], 0.50);
    }

    #[test]
    fn rigs_set_every_declared_field() {
        let studio = with_rig("studio");
        assert_eq!(studio.lighting[0], 1.0, "ambient pinned at unity");
        assert_eq!(studio.lighting[1], 2.6, "key");
        assert_eq!(studio.lighting[2], 0.35, "fill");
        assert_eq!(studio.pbr[2], 0.0, "exposure EV");
        assert_eq!(studio.pbr[3], 0.65, "env intensity");

        let daylight = with_rig("daylight");
        assert_eq!(daylight.lighting[0], 1.0, "ambient pinned at unity in BOTH rigs");
        assert_eq!(daylight.lighting[1], 1.1);
        assert_eq!(daylight.lighting[2], 0.9);
        assert_eq!(daylight.pbr[2], -0.8);
        assert_eq!(daylight.pbr[3], 1.9);
    }

    /// The property the `studio` doc comment claims: it is Tier 1's rig verbatim, so applying
    /// it over a fresh substrate snapshot changes **nothing**. If Tier 1's constants ever
    /// move, this is where the two stop agreeing.
    #[test]
    fn studio_is_exactly_tier_ones_shipped_rig() {
        let before = substrate();
        let after = with_rig("studio");
        assert_eq!(
            bytemuck::bytes_of(&after),
            bytemuck::bytes_of(&before),
            "`studio` must reproduce substrate_scene's rig byte for byte"
        );
        // And say it in Tier 1's own vocabulary, so the failure names the drifted constant.
        assert_eq!(STUDIO.ambient, substrate_scene::SUBSTRATE_AMBIENT);
        assert_eq!(STUDIO.key, substrate_scene::SUBSTRATE_KEY_INTENSITY);
        assert_eq!(STUDIO.fill, substrate_scene::SUBSTRATE_FILL_INTENSITY);
        assert_eq!(STUDIO.env, substrate_scene::SUBSTRATE_ENV_INTENSITY);
        assert_eq!(STUDIO.exposure_ev, substrate_scene::SUBSTRATE_EXPOSURE_EV);
    }

    /// `slate` likewise formalizes Tier 1's *surface*: the two scalars Tier 1 set are the
    /// ones it keeps, and it adds no albedo map, so Tier 1's per-vertex ramp survives.
    #[test]
    fn slate_keeps_tier_ones_surface_scalars_and_its_per_vertex_ramp() {
        let s = with_material("slate");
        assert_eq!(s.pbr[0], substrate_scene::SUBSTRATE_METALLIC);
        assert_eq!(s.pbr[1], substrate_scene::SUBSTRATE_ROUGHNESS);
        assert_eq!(s.lighting[7], substrate_scene::SUBSTRATE_MATERIAL_TYPE);
        assert_eq!(s.matcol[2], substrate_scene::SUBSTRATE_MATCOL[2]);
        // The claim that matters: no layer routes Albedo (channel 0), so `has_alb` stays 0
        // and `cube.wgsl:1477`'s `mix(in.color, m_albedo.rgb, has_alb)` returns the mesh's
        // per-vertex colour exactly.
        assert_ne!(s.material_layer[1], 0.0, "slate's base layer must not route Albedo");
        assert_eq!(s.material_layer2[16], 0.0, "…and it has no overlay to route it either");
    }

    // -----------------------------------------------------------------------------
    // (b) the manifests
    // -----------------------------------------------------------------------------

    #[test]
    fn apply_material_touches_only_the_manifest_lanes() {
        for name in MATERIAL_NAMES {
            let before = substrate();
            let mut restored = with_material(name);
            for (_, restore) in material_field_manifest() {
                restore(&mut restored, &before);
            }
            assert_eq!(
                bytemuck::bytes_of(&restored),
                bytemuck::bytes_of(&before),
                "`{name}` wrote a lane material_field_manifest does not declare"
            );
        }
    }

    #[test]
    fn apply_rig_touches_only_the_manifest_lanes() {
        for name in RIG_NAMES {
            let before = substrate();
            let mut restored = with_rig(name);
            for (_, restore) in rig_field_manifest() {
                restore(&mut restored, &before);
            }
            assert_eq!(
                bytemuck::bytes_of(&restored),
                bytemuck::bytes_of(&before),
                "`{name}` wrote a lane rig_field_manifest does not declare"
            );
        }
    }

    #[test]
    fn manifest_names_are_unique_and_the_two_manifests_are_disjoint() {
        let all: Vec<&str> = material_field_manifest()
            .iter()
            .chain(rig_field_manifest())
            .map(|(n, _)| *n)
            .collect();
        assert!(!all.is_empty());
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a, b, "duplicate/overlapping manifest entry `{a}`");
            }
        }
    }

    /// Disjointness proved by behaviour rather than by name: applying a material and a rig in
    /// either order must give the same bytes. This is what lets the console change one without
    /// re-deriving the other.
    #[test]
    fn material_and_rig_commute() {
        for m in MATERIAL_NAMES {
            for r in RIG_NAMES {
                let mut a = substrate();
                apply_material(&mut a, m);
                apply_rig(&mut a, r);

                let mut b = substrate();
                apply_rig(&mut b, r);
                apply_material(&mut b, m);

                assert_eq!(
                    bytemuck::bytes_of(&a),
                    bytemuck::bytes_of(&b),
                    "`{m}` + `{r}` must not depend on the order they were applied"
                );
            }
        }
    }

    // -----------------------------------------------------------------------------
    // (c) idempotence, totality, and the unknown-name contract
    // -----------------------------------------------------------------------------

    #[test]
    fn apply_is_idempotent() {
        for name in MATERIAL_NAMES {
            let once = with_material(name);
            let mut twice = with_material(name);
            apply_material(&mut twice, name);
            assert_eq!(bytemuck::bytes_of(&twice), bytemuck::bytes_of(&once), "material `{name}`");
        }
        for name in RIG_NAMES {
            let once = with_rig(name);
            let mut twice = with_rig(name);
            apply_rig(&mut twice, name);
            assert_eq!(bytemuck::bytes_of(&twice), bytemuck::bytes_of(&once), "rig `{name}`");
        }
    }

    /// The property the "total over the block" paragraph claims: **every ordered pair** of
    /// materials converges. Switching from a material with an overlay to one without must
    /// clear the overlay, not inherit it — `graphite → slate` is the concrete case, and this
    /// covers all 16.
    #[test]
    fn switching_between_any_two_materials_converges() {
        for from in MATERIAL_NAMES {
            for to in MATERIAL_NAMES {
                let mut s = with_material(from);
                apply_material(&mut s, to);
                assert_eq!(
                    bytemuck::bytes_of(&s),
                    bytemuck::bytes_of(&with_material(to)),
                    "`{from}` → `{to}` did not converge: a lane was inherited"
                );
            }
        }
        for from in RIG_NAMES {
            for to in RIG_NAMES {
                let mut s = with_rig(from);
                apply_rig(&mut s, to);
                assert_eq!(
                    bytemuck::bytes_of(&s),
                    bytemuck::bytes_of(&with_rig(to)),
                    "rig `{from}` → `{to}` did not converge"
                );
            }
        }
    }

    #[test]
    fn an_unknown_name_touches_nothing() {
        for bad in ["", " ", "slat", "graphit3", "chrome", "none", "off", "studio"] {
            let before = substrate();
            let mut s = substrate();
            assert!(!apply_material(&mut s, bad), "`{bad}` must not be a material");
            assert_eq!(bytemuck::bytes_of(&s), bytemuck::bytes_of(&before), "material `{bad}`");
        }
        for bad in ["", "studi0", "sunset", "graphite"] {
            let before = substrate();
            let mut s = substrate();
            assert!(!apply_rig(&mut s, bad), "`{bad}` must not be a rig");
            assert_eq!(bytemuck::bytes_of(&s), bytemuck::bytes_of(&before), "rig `{bad}`");
        }
    }

    /// Case-insensitive, per `shell_main.rs:96`'s precedent for the backdrop selector.
    #[test]
    fn names_match_case_insensitively() {
        for name in MATERIAL_NAMES {
            let mut s = substrate();
            assert!(apply_material(&mut s, &name.to_uppercase()));
            assert_eq!(bytemuck::bytes_of(&s), bytemuck::bytes_of(&with_material(name)));
        }
        for name in RIG_NAMES {
            let mut s = substrate();
            assert!(apply_rig(&mut s, &name.to_uppercase()));
            assert_eq!(bytemuck::bytes_of(&s), bytemuck::bytes_of(&with_rig(name)));
        }
    }

    // -----------------------------------------------------------------------------
    // (d) the name tables, and that no two names are the same look
    // -----------------------------------------------------------------------------

    /// Every advertised name applies, **and no two names are the same picture.** That
    /// second clause is the one that earns its place: a copy-paste leaving two materials
    /// identical would ship four names and three looks, and nothing else here would notice.
    ///
    /// ⚠️ The other half of this test — that `MATERIAL_NAMES` is in `MATERIALS` order —
    /// stayed in `substrate_materials.rs` (organon#49 T3). `MATERIALS` is a private static
    /// and an integration test cannot see it; making it `pub` to satisfy a test would widen
    /// the crate's API for the convenience of its own suite. The two halves also fail for
    /// unrelated reasons, so splitting them is an improvement rather than a concession.
    #[test]
    fn every_material_name_applies_and_is_a_distinct_look() {
        for (i, a) in MATERIAL_NAMES.iter().enumerate() {
            let sa = with_material(a);
            for b in MATERIAL_NAMES.iter().skip(i + 1) {
                assert_ne!(a, b, "duplicate material name `{a}`");
                assert_ne!(
                    bytemuck::bytes_of(&sa),
                    bytemuck::bytes_of(&with_material(b)),
                    "`{a}` and `{b}` are byte-identical — two names, one look"
                );
            }
        }
    }

    /// The rig half of the same split — the `RIG_NAMES`/`RIGS` order assertion is in
    /// `substrate_materials.rs`, for the reason above.
    #[test]
    fn every_rig_name_applies_and_is_a_distinct_look() {
        for (i, a) in RIG_NAMES.iter().enumerate() {
            let sa = with_rig(a);
            for b in RIG_NAMES.iter().skip(i + 1) {
                assert_ne!(a, b, "duplicate rig name `{a}`");
                assert_ne!(
                    bytemuck::bytes_of(&sa),
                    bytemuck::bytes_of(&with_rig(b)),
                    "`{a}` and `{b}` are byte-identical — two names, one rig"
                );
            }
        }
    }

    // -----------------------------------------------------------------------------
    // (e) the ranges — R6's "fourth way to lie", closed
    // -----------------------------------------------------------------------------

    /// Every value every material writes, against its `params.rs` range. R6 found three
    /// hand-maintained range tables already disagreeing with `params.rs` on 9 of 45 ids; this
    /// is the assertion that stops a scene builder becoming the fourth.
    #[test]
    fn every_material_value_is_inside_its_declared_range() {
        // (name, lo, hi) per material_layer slot — param_table.rs:1220-1241 order,
        // params.rs:8430-8446 ranges. `[16]`/`[17]` are checked separately.
        let layer_ranges: [(&str, f32, f32); 16] = [
            ("noise", 0.0, 15.0),        // MatNoise, 16 variants (params.rs:1494-1511)
            ("channel", 0.0, 4.0),       // MatChannel 0..5; 5 (Emissive) has no slot
            ("scale", 0.25, 64.0),       // params.rs:8432
            ("rotation", 0.0, std::f32::consts::TAU), // :8433
            ("offset_x", -8.0, 8.0),     // :8434
            ("offset_y", -8.0, 8.0),     // :8435
            ("octaves", 1.0, 8.0),       // :8436
            ("lacunarity", 1.2, 4.0),    // :8437
            ("gain", 0.1, 0.9),          // :8438
            ("warp", 0.0, 2.0),          // :8439
            ("contrast", 0.1, 4.0),      // :8440
            ("gamma", 0.2, 4.0),         // :8441
            ("remap_lo", 0.0, 1.0),      // :8442
            ("remap_hi", 0.0, 1.0),      // :8443
            ("invert", 0.0, 1.0),        // :8444
            ("seed", 0.0, 64.0),         // :8445
        ];
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            let check = |what: &str, v: f32, lo: f32, hi: f32| {
                assert!(v >= lo && v <= hi, "{name}: {what} = {v} outside {lo}..{hi}");
            };
            check("material[1] projection", s.material[1], 0.0, 2.0); // params.rs:1450-1457
            check("material[2] scale", s.material[2], 0.02, 16.0); // :8425
            for (i, (what, lo, hi)) in layer_ranges.iter().enumerate() {
                check(&format!("material_layer[{i}] {what}"), s.material_layer[i], *lo, *hi);
                // The overlay's slots share the base's ranges (params.rs:8458-8473); its
                // values are inert while disabled but must still be legal.
                check(&format!("material_layer2[{i}] {what}"), s.material_layer2[i], *lo, *hi);
            }
            check("material_layer[17] bake px", s.material_layer[17], 64.0, 2048.0); // render.rs clamp
            check("material_layer2[17] blend", s.material_layer2[17], 0.0, 7.0); // params.rs:1626-1635
            for i in 0..8 {
                check(&format!("material_grad[{i}]"), s.material_grad[i], 0.0, 1.0); // :8447-8452
                check(&format!("material_grad2[{i}]"), s.material_grad2[i], 0.0, 1.0); // :8474-8479
            }
            check("derive[3] normal strength", s.material_derive[3], 0.0, 4.0); // :8485
            check("derive[4] ao strength", s.material_derive[4], 0.0, 1.0); // :8486
            check("derive[5] ao radius", s.material_derive[5], 1.0, 8.0); // :8487
            check("material_live[1] anim speed", s.material_live[1], 0.0, 4.0); // :8492
            check("material_live[3] flow x", s.material_live[3], -1.0, 1.0); // :8494
            check("material_live[4] flow y", s.material_live[4], -1.0, 1.0); // :8495
            check("material_live[5] displace", s.material_live[5], 0.0, 2.0); // :8496
            check("lighting[7] material type", s.lighting[7], 0.0, 7.0); // params.rs:3701-3718
            check("pbr[0] metallic", s.pbr[0], 0.0, 1.0); // :8722
            check("pbr[1] roughness", s.pbr[1], 0.0, 1.0); // :8723
            check("aniso[0] amount", s.aniso[0], -1.0, 1.0); // :8689
            check("aniso[1] rotation", s.aniso[1], 0.0, 360.0); // :8690
            check("aniso[3] blend", s.aniso[3], 0.0, 1.0); // :8692
            check("matcol[0] hue", s.matcol[0], 0.0, 1.0); // :8892
            check("matcol[1] hue cycle", s.matcol[1], -2.0, 2.0); // :8893
            check("matcol[2] saturation", s.matcol[2], 0.0, 1.0); // :8894
            check("matcol[3] value", s.matcol[3], 0.0, 1.0); // :8895
        }
    }

    #[test]
    fn every_rig_value_is_inside_its_declared_range() {
        for name in RIG_NAMES {
            let s = with_rig(name);
            let check = |what: &str, v: f32, lo: f32, hi: f32| {
                assert!(v >= lo && v <= hi, "{name}: {what} = {v} outside {lo}..{hi}");
            };
            check("lighting[0] ambient", s.lighting[0], 0.0, 3.0); // params.rs:8550
            check("lighting[1] key", s.lighting[1], 0.0, 6.0); // :8551
            check("lighting[2] fill", s.lighting[2], 0.0, 3.0); // :8552
            check("pbr[2] exposure EV", s.pbr[2], -8.0, 4.0); // :8724
            check("pbr[3] env intensity", s.pbr[3], 0.0, 4.0); // :8725
        }
    }

    // -----------------------------------------------------------------------------
    // (f) the claims the doc comments make about the engine, restated as the engine
    //     evaluates them
    // -----------------------------------------------------------------------------

    /// `world.rs:10850-10852`: the material uniform turns on from `material_layer[16]` alone,
    /// so no material needs to touch `material[0]` — and none does. If that OR ever becomes
    /// an AND, four materials go inert at once and this is the only place that says so.
    #[test]
    fn the_material_uniform_turns_on_from_the_procedural_flag_alone() {
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            let mtl_x = s.material[0] > 0.5 || s.material_layer[16] > 0.5; // world.rs:10852
            assert!(mtl_x, "{name}: Uniforms.mtl.x must be on");
            assert_eq!(s.material[0], 0.0, "{name}: the PNG master must stay untouched");
        }
    }

    /// `render.rs`'s `present_mask` + `material_bake.wgsl:382`, restated: which channel bits
    /// each material actually lands, and — the part worth pinning — that **derived AO is only
    /// ever asked for over a baked Height layer**.
    #[test]
    fn the_present_mask_each_material_lands_is_what_it_claims() {
        // MatChannel → present bit (render.rs's `channel_slot`).
        let bit = |ch: f32| -> u32 {
            match ch as u32 {
                0 => 1,  // albedo
                1 => 4,  // roughness
                2 => 8,  // metallic
                3 => 32, // height
                4 => 16, // AO
                _ => 0,  // emissive: no bound slot
            }
        };
        let mask_of = |s: &Shared| -> u32 {
            let mut m = bit(s.material_layer[1]);
            if s.material_layer2[16] > 0.5 {
                m |= bit(s.material_layer2[1]);
            }
            if s.material_derive[0] > 0.5 {
                m |= 2; // derived normal
            }
            if s.material_derive[1] > 0.5 {
                m |= 16; // derived AO
            }
            m
        };
        assert_eq!(mask_of(&with_material("slate")), 4, "slate: roughness only");
        assert_eq!(mask_of(&with_material("graphite")), 1 | 4, "graphite: albedo + roughness");
        assert_eq!(mask_of(&with_material("paper")), 1 | 32 | 2, "paper: albedo + height + derived normal");
        assert_eq!(mask_of(&with_material("metal")), 1 | 4, "metal: albedo + roughness");

        // The pairing nothing in `Shared` enforces (see `derive`'s warning): the AO derive
        // pass reads the HEIGHT slot unconditionally, so asking for AO without baking height
        // derives cavities from a texture this material never wrote.
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            if s.material_derive[1] > 0.5 {
                let bakes_height = s.material_layer[1] == 3.0
                    || (s.material_layer2[16] > 0.5 && s.material_layer2[1] == 3.0);
                assert!(bakes_height, "{name}: derived AO without a baked Height layer");
            }
        }
    }

    /// `cube.wgsl:1491-1497`: the two routes into the anisotropic lobe are exclusive, and each
    /// material takes exactly one. Restated because "set both and hope" is the silent failure.
    #[test]
    fn each_material_takes_exactly_one_route_into_the_anisotropic_lobe() {
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            let is_type = s.lighting[7] == 4.0;
            let overlay_on = s.aniso[2] > 0.5;
            assert!(!(is_type && overlay_on), "{name}: type 4 ignores the overlay — do not set both");
            // The effective amount, exactly as the shader resolves it.
            let amt = if is_type {
                s.aniso[0].clamp(-1.0, 1.0)
            } else if overlay_on {
                s.aniso[0].clamp(-1.0, 1.0) * s.aniso[3].clamp(0.0, 1.0)
            } else {
                0.0
            };
            match name {
                "metal" => assert_eq!(amt, 0.75, "metal: the raw amount, via MaterialType"),
                "graphite" => assert!((amt - 0.2925).abs() < 1e-6, "graphite: amount × blend, via the overlay"),
                _ => assert_eq!(amt, 0.0, "{name} must be isotropic"),
            }
        }
    }
}

