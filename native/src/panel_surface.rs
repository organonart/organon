//! **Look ▸ Surface** — the first of Organon's editor panels to be drawn in Organon Console as
//! well as in Organon's own editor, from one body (Console #7).
//!
//! # What this file is
//!
//! [`surface_card`] *is* the Surface card. `lib.rs`'s Look tab calls it, and so does
//! `console_main.rs` when a `/organon look surface` element comes up in a conversation. There
//! is no second rendering to keep in step, which is the point: the claim `/organon` makes is
//! that a Console panel is *the same instrument*, and a copied body would make that claim
//! false within a week.
//!
//! It moved here rather than staying inline for a reason beyond tidiness. `editor_ui` is one
//! ~4,700-line pass over 105 cards, and a card in the middle of it cannot be called from
//! anywhere else. Lifting one out is what "transplanting a panel" actually consists of; the
//! parameter plumbing ([`crate::param_sink`]) is the part that looks hard and the extraction is
//! the part that takes the time.
//!
//! # 🚨 Why it lives in the root crate and not in `organon-console`
//!
//! `organon-console` depends on `organon-core` and egui, **not** on this crate — and it cannot,
//! because this crate depends on *it*. It has no way to see `OrganicMathParams`. So the panel
//! body is drawn from the root crate and handed to the compositor through
//! `conversation_view::OrganonDraw`, a callback rather than a render list. A callback because
//! a dropdown has to open where it is clicked: unlike a `SurfaceRequest`, which can defer to a
//! texture rendered next frame, a control has to be *in* the frame that laid it out.
//!
//! # ⚠️ What the Console's copy is, and is not, faithful about
//!
//! Faithful: every range, unit, value string, enum variant name, help text and grid line,
//! because they are read from a real `OrganicMathParams` the Console holds as **metadata** and
//! never writes. Also the disclosure logic — which rows appear under which surface mode — since
//! that travels through [`crate::param_sink::read`] rather than through `params.…value()`.
//!
//! Not faithful: the slider fill (see [`crate::param_sink`] — `ParamSlider` takes a
//! `ParamSetter` in its constructor, so no mirror can drive one), and the values the panel
//! *opens* on, which are Organon's defaults rather than whatever look the Console's backdrop
//! is currently dressed in. `CONSOLE_ARCHITECTURE.md` §1.11 carries what follows from that.

use crate::material_graph::MaterialGraph;
use crate::param_sink::{combo, crow, rd, srow, Sink};
use crate::params::{AnimMode, HostSurfaceMode, MatChannel, OrganicMathParams};
use crate::preset;
use crate::theme::COMBO_W;
use nih_plug_egui::egui;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// **The Look ▸ Surface card's body.** `p` supplies every control's metadata; `sink` decides
/// where the writes land. See the module doc.
///
/// ⚠️ The caller owns the card frame and the heading — `lib.rs` through `card()`, the Console
/// through `conversation_view::organon_element` — because the two are deliberately different
/// objects (a column card; an element in a conversation flow) drawn to the same spec.
///
/// ⚠️ **`material_gen` is a `Shared` lane, not a param**, so it does not ride the mirror: a
/// preset stores the material's *path*, not a counter. "Load Material…" bumps it on a
/// background thread and something has to fold it into the published snapshot —
/// `to_shared` does that in the editor, [`OrganonPanels::overlay`] in the Console. Both ends
/// are needed or the folder picker writes a sidecar nothing ever reads.
pub(crate) fn surface_card(
    ui: &mut egui::Ui,
    w2: f32,
    p: &OrganicMathParams,
    sink: &mut Sink,
    material_gen: &Arc<AtomicU32>,
) {
    combo!(ui, w2, "mode", sink, p, surface_mode, 2.0 * COMBO_W);
    combo!(ui, w2, "palette", sink, p, palette, 2.0 * COMBO_W);
    crate::help(
        ui,
        "Palette = colour LUT for the sweep. Native keeps the current \
         look; any other applies across all surface modes.",
    );
    let mode = rd!(sink, p, surface_mode);
    // Node bevel: rounds the cube geometry (Original + Flow-Aligned) from a sharp
    // cube through a rounded cube to a full sphere. Only these two modes draw cubes.
    if matches!(mode, HostSurfaceMode::Original | HostSurfaceMode::FlowAligned) {
        srow!(ui, w2, "node bevel", sink, p, bevel);
        crate::help(
            ui,
            "Rounds the cubes: 0 = sharp cube, 0.5 = wide \
             rounded cube, 1 = sphere. In Flow-Aligned the \
             rods round into smooth capsules.",
        );
        // #472 Tier 1: procedural / texture-mapped PBR materials. A folder of PNGs
        // (albedo/normal/roughness/metallic/ao/height) sampled onto the generator
        // cubes; off = today.
        ui.separator();
        crow!(ui, "material maps", sink, p, mat_enable);
        if rd!(sink, p, mat_enable) {
            // Only texture-MAPPING controls live here (how the maps project + tile).
            // The material qualities themselves — roughness / metallic / normal / AO —
            // come straight from the loaded maps into the ONE material system (the main
            // Material card's roughness/metallic drive any channel a set doesn't
            // provide), so there are no per-map quality knobs.
            combo!(ui, w2, "projection", sink, p, mat_projection, 2.0 * COMBO_W);
            srow!(ui, w2, "mat scale", sink, p, mat_scale);
            if ui
                .button("Load Material…")
                .on_hover_text(
                    "Pick a folder of PBR PNGs \
                     (albedo/normal/roughness/metallic/ao/height)",
                )
                .clicked()
            {
                crate::pick_material_async(material_gen.clone());
            }
            crate::help(
                ui,
                "Loads a folder of PBR maps as the surface's albedo / \
                 normal / roughness / metallic / AO — feeding the full \
                 material system, so the Material type (Chrome / Glass / \
                 Subsurface / …), GI, and reflections all apply on top. \
                 Triplanar needs no UVs; an absent channel falls back to \
                 the scalar Material sliders. Off = today's look.",
            );
        }
        // #472 Tier 2: procedural noise layer — baked into the routed channel by a
        // compute pass (supersedes the PNG for that channel). Off = today's look.
        ui.separator();
        crow!(ui, "procedural (noise)", sink, p, mp_enable);
        // #472 Tier 4: the declarative material.json graph — human/agent-authorable,
        // shareable, gallery-installed.
        ui.horizontal(|ui| {
            if ui
                .button("Load Material Graph…")
                .on_hover_text(
                    "Load a material.json (procedural noise graph) — \
                     turns materials + procedural on and applies it.",
                )
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Material Graph", &["json"])
                    .set_directory(preset::material_graphs_dir())
                    .pick_file()
                {
                    if let Ok(txt) = std::fs::read_to_string(&path) {
                        if let Ok(g) = MaterialGraph::from_json(&txt) {
                            // Through the sink, so a graph loaded in the Console lands in
                            // the mirror the same way the knobs above it do.
                            g.apply(p, sink);
                        }
                    }
                }
            }
            if ui
                .button("Save Material Graph…")
                .on_hover_text("Write the current material as a material.json graph")
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Material Graph", &["json"])
                    .set_directory(preset::material_graphs_dir())
                    .set_file_name("material.json")
                    .save_file()
                {
                    let g = MaterialGraph::from_sink(p, sink);
                    let _ = std::fs::write(&path, g.to_json());
                }
            }
        });
        if rd!(sink, p, mp_enable) {
            combo!(ui, w2, "noise", sink, p, mp_noise, 2.0 * COMBO_W);
            combo!(ui, w2, "→ channel", sink, p, mp_channel, 2.0 * COMBO_W);
            srow!(ui, w2, "noise scale", sink, p, mp_scale);
            srow!(ui, w2, "rotation", sink, p, mp_rotation);
            srow!(ui, w2, "offset X", sink, p, mp_offset_x);
            srow!(ui, w2, "offset Y", sink, p, mp_offset_y);
            srow!(ui, w2, "octaves", sink, p, mp_octaves);
            srow!(ui, w2, "lacunarity", sink, p, mp_lacunarity);
            srow!(ui, w2, "gain", sink, p, mp_gain);
            srow!(ui, w2, "domain warp", sink, p, mp_warp);
            srow!(ui, w2, "contrast", sink, p, mp_contrast);
            srow!(ui, w2, "gamma", sink, p, mp_gamma);
            srow!(ui, w2, "remap low", sink, p, mp_remap_lo);
            srow!(ui, w2, "remap high", sink, p, mp_remap_hi);
            crow!(ui, "invert", sink, p, mp_invert);
            srow!(ui, w2, "seed", sink, p, mp_seed);
            combo!(ui, w2, "bake res", sink, p, mp_res, 2.0 * COMBO_W);
            if rd!(sink, p, mp_channel) == MatChannel::Albedo {
                srow!(ui, w2, "grad low R", sink, p, mp_lo_r);
                srow!(ui, w2, "grad low G", sink, p, mp_lo_g);
                srow!(ui, w2, "grad low B", sink, p, mp_lo_b);
                srow!(ui, w2, "grad high R", sink, p, mp_hi_r);
                srow!(ui, w2, "grad high G", sink, p, mp_hi_g);
                srow!(ui, w2, "grad high B", sink, p, mp_hi_b);
            }
            crate::help(
                ui,
                "Bakes a noise field into the chosen channel \
                 (albedo maps through the gradient; others write a \
                 scalar). FBM/Turbulence/Ridged use the octave/\
                 lacunarity/gain dials; scale snaps to whole tiles \
                 so an un-rotated bake tiles seamlessly.",
            );
            // #472 Tier 3: overlay layer 2 (composites onto layer 1's output for the
            // same channel).
            ui.separator();
            crow!(ui, "layer 2", sink, p, mp2_enable);
            if rd!(sink, p, mp2_enable) {
                combo!(ui, w2, "L2 noise", sink, p, mp2_noise, 2.0 * COMBO_W);
                combo!(ui, w2, "L2 → channel", sink, p, mp2_channel, 2.0 * COMBO_W);
                combo!(ui, w2, "L2 blend", sink, p, mp2_blend, 2.0 * COMBO_W);
                srow!(ui, w2, "L2 scale", sink, p, mp2_scale);
                srow!(ui, w2, "L2 rotation", sink, p, mp2_rotation);
                srow!(ui, w2, "L2 offset X", sink, p, mp2_offset_x);
                srow!(ui, w2, "L2 offset Y", sink, p, mp2_offset_y);
                srow!(ui, w2, "L2 octaves", sink, p, mp2_octaves);
                srow!(ui, w2, "L2 lacunarity", sink, p, mp2_lacunarity);
                srow!(ui, w2, "L2 gain", sink, p, mp2_gain);
                srow!(ui, w2, "L2 warp", sink, p, mp2_warp);
                srow!(ui, w2, "L2 contrast", sink, p, mp2_contrast);
                srow!(ui, w2, "L2 gamma", sink, p, mp2_gamma);
                srow!(ui, w2, "L2 remap low", sink, p, mp2_remap_lo);
                srow!(ui, w2, "L2 remap high", sink, p, mp2_remap_hi);
                crow!(ui, "L2 invert", sink, p, mp2_invert);
                srow!(ui, w2, "L2 seed", sink, p, mp2_seed);
                if rd!(sink, p, mp2_channel) == MatChannel::Albedo {
                    srow!(ui, w2, "L2 grad low R", sink, p, mp2_lo_r);
                    srow!(ui, w2, "L2 grad low G", sink, p, mp2_lo_g);
                    srow!(ui, w2, "L2 grad low B", sink, p, mp2_lo_b);
                    srow!(ui, w2, "L2 grad high R", sink, p, mp2_hi_r);
                    srow!(ui, w2, "L2 grad high G", sink, p, mp2_hi_g);
                    srow!(ui, w2, "L2 grad high B", sink, p, mp2_hi_b);
                }
            }
            // #472 Tier 3: derived maps (the correlation principle — normal + AO agree
            // with the surface).
            ui.separator();
            crow!(ui, "derive normal", sink, p, mat_derive_normal);
            crow!(ui, "derive AO", sink, p, mat_derive_ao);
            if rd!(sink, p, mat_derive_normal) || rd!(sink, p, mat_derive_ao) {
                crow!(ui, "normal from albedo", sink, p, mat_normal_source_albedo);
                srow!(ui, w2, "normal strength", sink, p, mat_derive_normal_strength);
                srow!(ui, w2, "AO strength", sink, p, mat_derive_ao_strength);
                srow!(ui, w2, "AO radius", sink, p, mat_derive_ao_radius);
                crate::help(
                    ui,
                    "Derives a normal (Sobel of the height, or albedo \
                     luminance) and/or AO (cavity of the height) so the \
                     maps agree — bake a Height layer, then derive.",
                );
            }
            // #472 Tier 5: temporal animation (re-bakes the procedural layers each
            // frame at a throttled rate) + height→vertex displacement (geometry, not
            // just shading). Both inert at their defaults.
            ui.separator();
            crow!(ui, "animate", sink, p, mat_anim_enable);
            if rd!(sink, p, mat_anim_enable) {
                combo!(ui, w2, "motion", sink, p, mat_anim_mode, 2.0 * COMBO_W);
                srow!(ui, w2, "anim speed", sink, p, mat_anim_speed);
                if rd!(sink, p, mat_anim_mode) == AnimMode::Drift {
                    srow!(ui, w2, "flow X", sink, p, mat_flow_x);
                    srow!(ui, w2, "flow Y", sink, p, mat_flow_y);
                }
                crate::help(
                    ui,
                    "Drift pans the noise (flow X/Y), Evolve morphs \
                     it in place, Rotate spins each layer. Re-bakes at \
                     ~30 Hz — cost scales with layer count.",
                );
            }
            srow!(ui, w2, "height displace", sink, p, mat_displace);
            crate::help(
                ui,
                "Pushes vertices along the surface normal by the baked \
                 Height field (needs a Height layer). 0 = flat (shading \
                 only). Real geometry — casts shadows, catches silhouettes.",
            );
        }
    }
    // Metaball-only: wrap the node set in one smooth skin.
    if mode == HostSurfaceMode::Metaball {
        srow!(ui, w2, "blob radius", sink, p, metaball_radius);
        srow!(ui, w2, "blob threshold", sink, p, metaball_threshold);
        srow!(ui, w2, "blob smoothness", sink, p, metaball_smooth);
        crate::help(
            ui,
            "Radius must exceed node spacing for blobs to fuse \
             into a contiguous skin.",
        );
    }
    // Splat-only: render the node set as anisotropic 3-D Gaussians (the 3DGS
    // primitive, synthesized directly).
    if mode == HostSurfaceMode::Splat {
        combo!(ui, w2, "tier", sink, p, splat_mode, 2.0 * COMBO_W);
        srow!(ui, w2, "splat radius", sink, p, splat_radius);
        srow!(ui, w2, "splat opacity", sink, p, splat_opacity);
        srow!(ui, w2, "splat falloff", sink, p, splat_falloff);
        srow!(ui, w2, "splat cutoff", sink, p, splat_cutoff);
        srow!(ui, w2, "splat anisotropy", sink, p, splat_aniso);
        srow!(ui, w2, "splat scatter", sink, p, splat_scatter);
        srow!(ui, w2, "splat jitter", sink, p, splat_jitter);
        srow!(ui, w2, "splat solidity", sink, p, splat_solid);
        crate::help(
            ui,
            "Each node becomes an anisotropic Gaussian (no \
             photogrammetry — synthesized from the node's matrix). \
             Additive = unlit glow that blooms through HDR; Lit = \
             sorted 2DGS disks shaded by the IBL + the Material card \
             (Chrome/Glass reflect + refract the environment). \
             Solidity 0 = soft Gaussian bokeh → 1 = opaque discs: \
             raise it (with Opacity ~1 + enough Radius for the discs \
             to meet) for a compact opaque SURFACE instead of a blur. \
             Scatter sprays jittered sub-splats per node for density.",
        );
    }
    // Swept-Tubes-only: weld the per-segment cylinders into one smooth continuous
    // tube per strand, with a shaped cap.
    if mode == HostSurfaceMode::SweptTubes {
        crow!(ui, "contiguous tube", sink, p, tube_weld);
        srow!(ui, w2, "tube profile", sink, p, tube_profile);
        crow!(ui, "end caps", sink, p, tube_end_cap);
        srow!(ui, w2, "cap rounding", sink, p, tube_cap_round);
        srow!(ui, w2, "cap bevel", sink, p, tube_cap_bevel);
        crate::help(
            ui,
            "'Contiguous' welds the segments into one smooth \
             tube per strand (closing the gaps at bends). Tube \
             profile 1 = round → 0 = sharp square (welded cubes). \
             Cap rounding 0 = flat → 1 = dome; cap bevel 0 = \
             rounded → 1 = chamfer.",
        );
    }
    // Voxel-only: splat the node field into a lattice and DDA-raymarch crisp
    // grid-snapped cubes.
    if mode == HostSurfaceMode::Voxel {
        srow!(ui, w2, "voxel grid", sink, p, voxel_res);
        srow!(ui, w2, "voxel threshold", sink, p, voxel_threshold);
        srow!(ui, w2, "voxel radius", sink, p, voxel_radius);
        srow!(ui, w2, "voxel emission", sink, p, voxel_emission);
        srow!(ui, w2, "voxel AO", sink, p, voxel_ao);
        srow!(ui, w2, "voxel shadow", sink, p, voxel_shadow);
        srow!(ui, w2, "voxel quantize", sink, p, voxel_quantize);
        srow!(ui, w2, "voxel beat→threshold", sink, p, voxel_beat);
        crate::help(
            ui,
            "Crisp grid-snapped cubes (flat faces, voxel AO, soft \
             shadows). Grid = perf dial; radius = strand thickness. \
             Quantize posterizes colour; emission blooms.",
        );
        // Voxel GI (#89): cone-traced bounced colour.
        crow!(ui, "voxel GI", sink, p, voxel_gi);
        if rd!(sink, p, voxel_gi) {
            srow!(ui, w2, "GI strength", sink, p, voxel_gi_strength);
            srow!(ui, w2, "GI distance", sink, p, voxel_gi_distance);
            srow!(ui, w2, "GI sky", sink, p, voxel_gi_sky);
            crate::help(
                ui,
                "Cone-traces the voxel field so emissive voxels bleed \
                 coloured light onto their neighbours + the world \
                 (raise emission for stronger bleed). A per-frame mip \
                 build — drop the grid on a projector.",
            );
        }
    }
    // Volume-only (#152): raymarch the metaball field as a glowing participating
    // medium (nebula/fog).
    if mode == HostSurfaceMode::Volume {
        srow!(ui, w2, "volume radius", sink, p, volume_radius);
        srow!(ui, w2, "density", sink, p, volume_density);
        srow!(ui, w2, "emission", sink, p, volume_emission);
        srow!(ui, w2, "absorption", sink, p, volume_absorption);
        srow!(ui, w2, "steps", sink, p, volume_steps);
        crate::help(
            ui,
            "Emissive fog: density × emission glows (HDR → blooms), \
             absorption thins it. Radius must exceed node spacing for \
             a continuous cloud; steps = perf dial.",
        );
        // Field Volume (#348): choose what the cloud bakes.
        ui.separator();
        combo!(ui, w2, "field source", sink, p, fv_source, 2.0 * COMBO_W);
        srow!(ui, w2, "smoothing", sink, p, fv_smooth);
        srow!(ui, w2, "exposure dB", sink, p, fv_exposure_db);
        crow!(ui, "calibrated brightness", sink, p, fv_calibrate);
        srow!(ui, w2, "gain", sink, p, fv_gain);
        ui.separator();
        crow!(ui, "field lines (flow)", sink, p, fv_lines);
        srow!(ui, w2, "line density", sink, p, fv_line_density);
        srow!(ui, w2, "line thickness", sink, p, fv_line_thickness);
        crate::help(
            ui,
            "Field lines (Acoustic / Maxwell): render the field as a dense cloud of \
             thin glowing streamlines of both channels (pressure/velocity or E/B) \
             — the tube-mode flow, without chunky tubes. Density = how many \
             lines; raise glow (Lighting) for luminous filaments. \n\
             Field source: Legacy = today's node metaball (byte-identical). \
             Auto = Maxwell/Acoustic bake the analytic field ENERGY (a smooth \
             cloud, no scraggly far-node spikes), every other generator bakes \
             a smoothed node kernel. Smoothing widens the node kernel; \
             calibrated brightness keys the glow to the measured loudness.",
        );
    }
    // Membrane-only: loft sheets between adjacent strands.
    if mode == HostSurfaceMode::Membrane {
        combo!(ui, w2, "membrane weave", sink, p, membrane_weave, 2.0 * COMBO_W);
        crow!(ui, "membrane: show strands", sink, p, membrane_show_strands);
        crow!(ui, "membrane: close seam (360°)", sink, p, membrane_close);
        crow!(ui, "membrane: skin arms", sink, p, membrane_arms);
        if rd!(sink, p, membrane_arms) {
            combo!(ui, w2, "arm build", sink, p, membrane_arm_build, 2.0 * COMBO_W);
            srow!(ui, w2, "arm radius (0=auto)", sink, p, membrane_arm_radius);
        }
        crow!(ui, "membrane: screen-space FX", sink, p, membrane_fx);
        crate::help(
            ui,
            "Skins a sheet between neighbouring strands \
             (sail/bell/web). Close seam bridges the last row of strands \
             back to the first when the form wraps a full 360°. Skin arms \
             skins each strand (arm) as its own closed capped finger with \
             gaps between arms (the volume-render hull) — built as cheap \
             capsule Impostors or a welded Mesh. Screen-space FX draws the \
             membrane into the depth prepass so VXGI (diffuse + reflections), \
             SSAO, SSR, SSGI, DoF and TAA apply to it — on by default; turn \
             off to skip the extra depth pass and keep membrane as a flat-lit sheet.",
        );
    }
    // Neural Tissue (#260): living neural tissue built from closed anatomical
    // primitives. Best paired with the Neural Network generator. Grouped by tier —
    // soma/membrane, dendritic arbor, myelin, synapse/context.
    if mode == HostSurfaceMode::NeuralTissue {
        crate::help(
            ui,
            "Living neural tissue: soma cell bodies, capped-capsule \
             tracts + synaptic boutons. Pair with Generator = Neural \
             Network. All morphology dials below are inert at 0 (a bare \
             soma field) — raise them to grow the anatomy.",
        );
        // Tier 1 — soma + membrane.
        srow!(ui, w2, "soma size", sink, p, nt_soma_size);
        srow!(ui, w2, "soma shape", sink, p, nt_soma_shape);
        srow!(ui, w2, "bouton size", sink, p, nt_bouton_size);
        srow!(ui, w2, "membrane SSS", sink, p, nt_membrane_sss);
        srow!(ui, w2, "membrane iridescence", sink, p, nt_membrane_irid);
        // Tier 2 — dendritic arbor + axon morphology.
        combo!(ui, w2, "neuron type", sink, p, nt_neuron_type, 2.0 * COMBO_W);
        srow!(ui, w2, "dendrite density", sink, p, nt_dendrite_density);
        srow!(ui, w2, "dendrite length", sink, p, nt_dendrite_length);
        srow!(ui, w2, "dendrite taper", sink, p, nt_dendrite_taper);
        srow!(ui, w2, "dendritic spines", sink, p, nt_spines);
        crate::help(
            ui,
            "Dendrite density 0 = a bare soma; raise it to grow \
             branching arbors (type sets the class). Spines sprinkle \
             tiny stubs — higher detail, off by default.",
        );
        // Tier 3 — myelinated axons (saltatory conduction).
        srow!(ui, w2, "myelin amount", sink, p, nt_myelin_amount);
        srow!(ui, w2, "Ranvier spacing", sink, p, nt_ranvier_spacing);
        srow!(ui, w2, "sheath scale", sink, p, nt_sheath_scale);
        crate::help(
            ui,
            "Myelin 0 = plain capped tracts; raise it to sheathe edges \
             as nerve fibres (fatty internodes + Ranvier constrictions). \
             The pulse jumps node-to-node — turn the firing sim on \
             (Neural Network → Signal) to see saltatory conduction.",
        );
        // Tier 4 — living synapse + tissue context.
        srow!(ui, w2, "synapse cleft", sink, p, nt_synapse_cleft);
        srow!(ui, w2, "cytoplasm glow", sink, p, nt_synapse_glow);
        srow!(ui, w2, "vesicles", sink, p, nt_synapse_vesicles);
        srow!(ui, w2, "glia", sink, p, nt_glia);
        srow!(ui, w2, "capillaries", sink, p, nt_capillary);
        crate::help(
            ui,
            "Cleft opens a gap at each terminal; cytoplasm glow lights \
             somata from within (tied to activation). Vesicles burst on \
             each spike arrival (needs the firing sim). Glia + capillaries \
             sprout faint scaffolding so the network sits in tissue.",
        );
    }
    // Plexus (#8): the node cloud rebuilt as a proximity web — struts between near
    // neighbours + a marker per node. Works on ANY node-emitting generator, either as
    // the surface itself OR as an OVERLAY (outer shell) on top of another surface.
    crow!(ui, "plexus overlay (outer shell)", sink, p, plexus_overlay_on);
    let overlay_on = rd!(sink, p, plexus_overlay_on);
    if overlay_on {
        srow!(ui, w2, "shell scale", sink, p, plexus_shell_scale);
        srow!(ui, w2, "shell depth", sink, p, plexus_shell_depth);
        srow!(ui, w2, "shell resolution", sink, p, plexus_shell_bins);
        crate::help(
            ui,
            "Wraps the plexus web as an outer SHELL() around the CURRENT surface \
             (Metaball, etc.) — like the Particle Aura, it reads the node cloud \
             without replacing it, so the base surface still renders. Shell scale \
             grows the cage outward; shell depth keeps the outer band of each \
             direction (bigger = a thicker, steadier rind — raise it if nodes \
             flicker); resolution sets the outline detail. It uses all the Plexus \
             look controls below (impostors, materials, shape morph, signal).",
        );
        ui.separator();
    }
    if mode == HostSurfaceMode::Plexus || overlay_on {
        srow!(ui, w2, "link radius", sink, p, plexus_radius);
        srow!(ui, w2, "max links / node", sink, p, plexus_links);
        srow!(ui, w2, "strut thickness", sink, p, plexus_strut);
        srow!(ui, w2, "node size", sink, p, plexus_marker);
        srow!(ui, w2, "node shape (cube→sphere)", sink, p, plexus_node_shape);
        srow!(ui, w2, "edge shape (square→circle)", sink, p, plexus_edge_shape);
        crate::help(
            ui,
            "Wires each node to its nearest neighbours. All sizes are × the \
             field's node spacing (auto-scaled per generator), so the same \
             settings read consistently everywhere. Raise link radius for a \
             denser web. Node shape morphs the markers cube → rounded → sphere; \
             edge shape morphs the strut cross-section square → circle.",
        );
        ui.separator();
        // Tier 2 — GPU impostors + independent materials.
        crow!(ui, "impostors (spheres + tubes)", sink, p, plexus_impostor);
        if rd!(sink, p, plexus_impostor) {
            crow!(ui, "draw edges", sink, p, plexus_edges);
            srow!(ui, w2, "node radius", sink, p, plexus_node_radius);
            srow!(ui, w2, "edge radius", sink, p, plexus_edge_radius);
            ui.label("Node material");
            combo!(ui, w2, "node type", sink, p, plexus_node_type, 2.0 * COMBO_W);
            srow!(ui, w2, "node metallic", sink, p, plexus_node_metallic);
            srow!(ui, w2, "node roughness", sink, p, plexus_node_rough);
            srow!(ui, w2, "node IOR", sink, p, plexus_node_ior);
            srow!(ui, w2, "node hue", sink, p, plexus_node_hue);
            srow!(ui, w2, "node saturation", sink, p, plexus_node_sat);
            srow!(ui, w2, "node value", sink, p, plexus_node_val);
            srow!(ui, w2, "node emissive", sink, p, plexus_node_emissive);
            ui.label("Edge material");
            combo!(ui, w2, "edge type", sink, p, plexus_edge_type, 2.0 * COMBO_W);
            srow!(ui, w2, "edge metallic", sink, p, plexus_edge_metallic);
            srow!(ui, w2, "edge roughness", sink, p, plexus_edge_rough);
            srow!(ui, w2, "edge IOR", sink, p, plexus_edge_ior);
            srow!(ui, w2, "edge hue", sink, p, plexus_edge_hue);
            srow!(ui, w2, "edge saturation", sink, p, plexus_edge_sat);
            srow!(ui, w2, "edge value", sink, p, plexus_edge_val);
            srow!(ui, w2, "edge emissive", sink, p, plexus_edge_emissive);
            crate::help(
                ui,
                "Nodes as sphere impostors, edges as capsule-tube impostors \
                 — each with its OWN full material (chrome nodes on glass \
                 filaments, emissive nodes on matte struts, whatever).",
            );
        }
        ui.separator();
        // Tier 3 — beat-driven signal propagation.
        crow!(ui, "signal propagation", sink, p, plexus_signal);
        if rd!(sink, p, plexus_signal) {
            srow!(ui, w2, "signal speed (/beat)", sink, p, plexus_signal_speed);
            srow!(ui, w2, "signal gain", sink, p, plexus_signal_gain);
            srow!(ui, w2, "signal width", sink, p, plexus_signal_width);
            crate::help(
                ui,
                "A bright activation shell radiates from the web centre on \
                 the beat, firing the impostors it crosses (needs impostors \
                 on). Speed = shells per beat.",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The Console's side of it
// ---------------------------------------------------------------------------

/// **The state behind a `/organon` panel element in Organon Console**: Organon's parameter set
/// as *metadata*, and a freely writable mirror of it that the panel's controls actually move.
///
/// 🚨 **The mirror exists because a parameter cannot be written from outside `nih_plug`.**
/// [`crate::param_sink`]'s module doc owns that account; the short version is that `ParamMut`
/// and every setter on it are `pub(crate)` in an upstream git dependency, so `PresetValues` —
/// a plain struct whose fields share their identifiers with `OrganicMathParams`, and which has
/// its own `to_shared()` — is the way through.
///
/// ⚠️ **One per console, not one per element.** Two `/organon look surface` cards in a
/// transcript are two views of one instrument; reading different values off each would make
/// the claim `/organon` exists to make — *this is the same panel* — false on sight.
///
/// ⚠️ **`params` is never written and must never be.** It is a `Default` instance held purely
/// so the rows can read ranges, units, value strings and enum variant names off the real
/// thing. Its staying at the factory defaults for the life of the process is exactly what
/// makes it a usable baseline in [`Self::overlay`].
pub struct OrganonPanels {
    params: OrganicMathParams,
    /// What the panel has asked for. Written by the rows, read by [`Self::overlay`].
    mirror: preset::PresetValues,
    /// The mirror's snapshot before anyone touched it — kept rather than recomputed, because
    /// keeping it is what makes "has this panel an opinion about this lane?" a comparison
    /// instead of a hand-written list of lanes.
    base: Box<crate::ipc::Shared>,
    /// The folder-picker's generation counter, matching the editor's. See [`surface_card`].
    material_gen: Arc<AtomicU32>,
    /// `mirror.to_shared()`, recomputed only after a row moved. A `Shared` is kilobytes and
    /// this is consulted every frame.
    cached: Box<crate::ipc::Shared>,
    dirty: bool,
}

impl Default for OrganonPanels {
    fn default() -> Self {
        Self::new()
    }
}

impl OrganonPanels {
    pub fn new() -> Self {
        let params = OrganicMathParams::default();
        let mirror = preset::PresetValues::capture_params_only(&params);
        let base = Box::new(mirror.to_shared());
        let cached = base.clone();
        Self { params, mirror, base, material_gen: Arc::new(AtomicU32::new(0)), cached, dirty: false }
    }

    /// Put whatever the panel has asked for on top of `dst`.
    ///
    /// 🚨 **Only the lanes that moved, and that is what keeps this safe.** The panel opens on
    /// Organon's defaults rather than on whatever the console's backdrop is currently dressed
    /// in — so if it asserted its whole snapshot, opening a Surface card over a dressed
    /// substrate would wipe the dressing. Comparing against [`Self::base`] means an untouched
    /// control has *no opinion*: the substrate shows through, and a console in which nobody
    /// ever opens a panel publishes bytes identical to before this existed.
    /// [`organon_core::ipc::overlay_changed`] carries the mechanism and the guard on it.
    ///
    /// ⚠️ **Against `BackdropSource::World` the panel is exactly faithful**, because the
    /// console's snapshot there *is* the params' own defaults, which is what the mirror was
    /// captured from. Against a dressed substrate it is faithful only about what it has been
    /// told: a row still reading `0.35` while the substrate renders something else is saying
    /// "I have not asked", not "the world is at 0.35". `CONSOLE_ARCHITECTURE.md` §1.11 records
    /// what follows from that.
    pub fn overlay(&mut self, dst: &mut crate::ipc::Shared) {
        if self.dirty {
            *self.cached = self.mirror.to_shared();
            self.dirty = false;
        }
        crate::ipc::overlay_changed(dst, &self.base, &self.cached);
        // Not a param, so not in the diff — see [`surface_card`].
        dst.material_gen = self.material_gen.load(Ordering::Relaxed);
    }

    /// **Draw one of Organon's editor cards into a console panel column — the same `card()` the
    /// Look tab draws, around the same body.**
    ///
    /// 🚨 **This function exists so that "it should use the same exact code" is a fact rather
    /// than a resemblance.** James, 2026-08-20, holding the Console's panel column up against
    /// Organon's own stack: *"I want us to adopt the styling … so that it looks just like it
    /// does here in terms of the padding and the fact that it just has the one word for the
    /// panel … In fact, it should use the same exact code somehow."* The chrome is
    /// [`crate::card`] — `theme::framed`, the silver header band, an `egui::CollapsingHeader` —
    /// and `organon-console` cannot call it, because that crate is *beneath* this one in the
    /// graph. So the Console asks: `panel_stack::OrganonDraw` hands this the panel and takes a
    /// card back. There is one card function in the tree and both surfaces call it.
    ///
    /// ⚠️ **`absent` is the Console's sentence, carried rather than written.** Twenty-four of the
    /// twenty-five panels have no body here, and what they say instead
    /// (`panel_stack::NOT_TRANSPLANTED`) belongs to the crate that decides what a Console panel
    /// is. Placing it *inside* the card is the point — a declared panel that skipped the chrome
    /// would be the one card in a column of twenty-five that looked like something else.
    ///
    /// ⚠️ The heading is `panel.title` — `panels::Panel`'s own field, which is the string
    /// Organon's editor already draws over this same card. Nothing is composed here, and
    /// `panel_stack::heading` is the Console's one reader of the same field.
    pub fn card(
        &mut self,
        ui: &mut egui::Ui,
        panel: &'static organon_core::panels::Panel,
        absent: Option<&str>,
    ) {
        crate::card(ui, panel.title, |ui| match absent {
            None => self.draw(ui, panel),
            Some(sentence) => {
                ui.label(egui::RichText::new(sentence).italics().weak());
            }
        });
    }

    /// Draw one panel's body into `ui`. The frame and heading belong to the caller — [`Self::card`]
    /// for the Console, `lib.rs`'s Look tab for the editor; this supplies only what goes inside.
    ///
    /// ⚠️ **`dirty` is set unconditionally rather than from a `changed()` response.** Every row
    /// writes through a `Sink`, and threading a "did anything move" answer back out of five
    /// widget kinds would be a second thing to keep true; re-deriving one `Shared` on a frame
    /// where the pointer merely hovered a slider costs a memcpy nobody can perceive.
    pub fn draw(&mut self, ui: &mut egui::Ui, panel: &'static organon_core::panels::Panel) {
        let w2 = ui.available_width().max(150.0);
        if panel.slug == organon_core::panels::LOOK_SURFACE.slug {
            let mut sink = Sink::Mirror(&mut self.mirror);
            surface_card(ui, w2, &self.params, &mut sink, &self.material_gen);
            self.dirty = true;
            return;
        }
        // Every other transplanted panel is *declared* rather than written
        // (`crate::panel_table`), so the Console reaches it the same way Organon's own editor
        // does: one generated body, two sinks. Surface stays above because its body is
        // hand-written — it is the one Look panel with disclosure logic, file dialogs and a
        // material-graph loader in it.
        if let Some(body) = crate::panel_table::body_for(panel.slug) {
            let mut sink = Sink::Mirror(&mut self.mirror);
            body(ui, w2, &self.params, &mut sink);
            self.dirty = true;
            return;
        }
        // 🚨 Unreachable for a `panels::Status::Live` panel — [`Self::card`] is handed `absent`
        // for every `Declared` one and never gets here — so arriving here means
        // `panels::Status` and `panel_table::DECLARED` have drifted. It says so rather than
        // drawing an empty box, for the reason `panel_stack::absent_body` gives about a
        // `Declared` panel: a card that opens to nothing is indistinguishable from one that
        // failed. `panel_table::a_declared_panel_is_exactly_a_live_one` is what stops that
        // drift reaching a build.
        ui.label(
            egui::RichText::new(format!(
                "`{}` is Live in the panel table but this console has no body for it",
                panel.slug
            ))
            .italics(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::HostSurfaceMode;

    /// 🚨 **The end-to-end inertness guarantee.** A console in which nobody has touched a
    /// panel publishes exactly the bytes it published before this module existed — not because
    /// the overlay was ordered late or gated on a flag, but because there is no difference for
    /// it to carry. This is the composed version of `ipc::overlay_changed`'s own test: it
    /// additionally proves `PresetValues::capture_params_only` → `to_shared()` is stable, which
    /// that one cannot see.
    #[test]
    fn an_untouched_panel_changes_nothing_it_is_laid_over() {
        let mut panels = OrganonPanels::new();
        let mut dst = OrganicMathParams::default().to_shared();
        dst.bevel = 0.75; // somebody else's opinion, on a lane the panel owns
        dst.lighting[2] = 9.0;
        let before = dst;
        panels.overlay(&mut dst);
        // `material_gen` is folded in unconditionally — see `surface_card` — so compare
        // everything else.
        dst.material_gen = before.material_gen;
        assert_eq!(bytemuck::bytes_of(&dst), bytemuck::bytes_of(&before));
    }

    /// A mirror write reaches `Shared`. The one-liner the whole transplant is for.
    #[test]
    fn a_mirror_write_reaches_the_snapshot() {
        let mut panels = OrganonPanels::new();
        panels.mirror.bevel = 0.5;
        panels.dirty = true;
        let mut dst = OrganicMathParams::default().to_shared();
        panels.overlay(&mut dst);
        assert_eq!(dst.bevel, 0.5);
    }

    /// ⚠️ **`dirty` is what makes the cache honest, and a stale cache is silent** — the panel
    /// would keep publishing the value from before the drag and read as a knob that moves
    /// nothing, which is precisely the failure `/panel` was retired for. Setting the mirror
    /// *without* the flag must therefore change nothing, and setting it *with* the flag must.
    #[test]
    fn the_cache_is_only_as_fresh_as_the_flag_says() {
        let mut panels = OrganonPanels::new();
        panels.mirror.bevel = 0.5; // no flag: the panel has not admitted to moving
        let mut dst = OrganicMathParams::default().to_shared();
        panels.overlay(&mut dst);
        assert_eq!(dst.bevel, OrganicMathParams::default().to_shared().bevel);
        panels.dirty = true;
        panels.overlay(&mut dst);
        assert_eq!(dst.bevel, 0.5);
    }

    /// 🚨 **One field from every disclosure branch of the card reaches `Shared`.**
    ///
    /// The Surface card shows a different set of rows under each surface mode, and each set is
    /// a different region of `PresetValues`. A field with no lane in `PresetValues::to_shared`
    /// would draw, drag, and move nothing — the failure this transplant most needed to rule
    /// out, and one that only shows up in the *one* mode that owns the field.
    ///
    /// ⚠️ **A spot-check per branch, not per field.** Rust cannot enumerate a struct's fields,
    /// so full per-field coverage would mean a 167-name list beside the panel body — the kind
    /// of second copy this repo keeps learning to avoid. One field per region catches a whole
    /// region falling out of the packers, which is how this actually breaks.
    #[test]
    fn every_branch_of_the_card_reaches_the_snapshot() {
        let base = OrganicMathParams::default().to_shared();
        // (what to move, how, where it should show up) — one per `if` in `surface_card`.
        //
        // ⚠️ **A delta, never a literal.** Every one of these fields is a plain `(f32, …)` slot
        // in `param_table!`, so `+= 1.0` is *guaranteed* to move its lane; a chosen constant
        // silently tests nothing on the day it happens to equal the field's default.
        // ⚠️ The lane indices below were checked against the packers by hand, because the
        // local verification bar only type-checks this file — an in-bounds index into the
        // wrong lane compiles and then asserts against something nobody moved.
        let cases: Vec<(&str, fn(&mut preset::PresetValues), fn(&crate::ipc::Shared) -> f32)> = vec![
            ("always: bevel", |m| m.bevel += 1.0, |s| s.bevel),
            ("material maps: mat_scale", |m| m.mat_scale += 1.0, |s| s.material[2]),
            ("procedural: mp_scale", |m| m.mp_scale += 1.0, |s| s.material_layer[2]),
            ("metaball: radius", |m| m.metaball_radius += 1.0, |s| s.metaball[0]),
            ("splat: radius", |m| m.splat_radius += 1.0, |s| s.splat[0]),
            ("swept tubes: profile", |m| m.tube_profile += 1.0, |s| s.tube_profile),
            ("voxel: grid", |m| m.voxel_res += 1.0, |s| s.voxel[0]),
            ("volume: density", |m| m.volume_density += 1.0, |s| s.volume[1]),
            ("membrane: arm radius", |m| m.membrane_arm_radius += 1.0, |s| s.membrane_fx[2]),
            ("neural tissue: soma size", |m| m.nt_soma_size += 1.0, |s| s.neural_surface[0]),
            ("plexus: link radius", |m| m.plexus_radius += 1.0, |s| s.plexus[0]),
        ];
        for (what, move_it, read_it) in cases {
            let mut panels = OrganonPanels::new();
            move_it(&mut panels.mirror);
            panels.dirty = true;
            let mut dst = base;
            panels.overlay(&mut dst);
            assert_ne!(
                read_it(&dst),
                read_it(&base),
                "`{what}` moved in the mirror and nothing moved in `Shared` — the panel would \
                 draw a knob that does nothing"
            );
        }
    }

    /// The enum route, separately: a dropdown stores a variant *index*, and the index has to
    /// survive `PresetValues` → `Shared` as the same variant the row displayed.
    #[test]
    fn a_dropdown_choice_reaches_the_snapshot_as_the_same_variant() {
        let mut panels = OrganonPanels::new();
        panels.mirror.surface_mode = HostSurfaceMode::Voxel.to_u32();
        panels.dirty = true;
        let mut dst = OrganicMathParams::default().to_shared();
        panels.overlay(&mut dst);
        assert_eq!(dst.surface_mode, HostSurfaceMode::Voxel.to_u32());
    }

    /// "Load Material…" is not a param, so it does not ride the diff — it is folded in by
    /// hand, and this is what stops that fold being deleted as redundant. Without it the
    /// folder picker writes a sidecar the renderer never re-reads.
    #[test]
    fn the_material_generation_reaches_the_snapshot() {
        let mut panels = OrganonPanels::new();
        panels.material_gen.fetch_add(1, Ordering::Relaxed);
        let mut dst = OrganicMathParams::default().to_shared();
        panels.overlay(&mut dst);
        assert_eq!(dst.material_gen, 1);
    }
}
