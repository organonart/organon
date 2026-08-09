//! AI Performer — the internal agent that *plays* Organon (#317 Tier 1).
//!
//! A thin, headless-verifiable first tier. The agent RUNTIME lives in the **visual**
//! process (it owns the frame + the look-application machinery); the **plugin** only
//! contributes the Mind card (chat text → `organic-math-chat.txt` + `chat_gen`) and
//! stamps the `Shared.agent[8]` runtime block in `process()`.
//!
//! Pieces:
//! - [`AgentAction`] — the action set v1 + a single [`dispatch`] entry point onto the
//!   override lane. Kept **decoupled from where the action came from** (a localhost
//!   model now, the in-process character of #367 later) so the convergence guard costs
//!   nothing: #367 Tier 5 can feed the same enum.
//! - [`core_catalog`] — the prompt-side vocabulary, generated mechanically from the
//!   `param_block!` slot lists in `param_table.rs`. A param added to a core block
//!   appears automatically; there is no second hand-maintained list.
//! - [`AgentLane`] — the **agent override lane**: last-touched-wins holds applied in the
//!   visual at the same site as the pulse-routing / CC override. Lives in the visual's
//!   `Shared` working copy (the pulse-routing precedent) — documented choice below.
//! - [`ChatClient`] — an OpenAI-compatible **localhost** client (Ollama / LM Studio /
//!   llama.cpp / MLX are interchangeable; endpoint + model are config read from the
//!   `organic-math-agent.txt` sidecar, not constants). Mockable — the tool-call parse +
//!   dispatch are unit-tested against canned JSON; real network calls are the Mac step.
//! - [`PhrasePlan`] — the debug executor path: a hand-written phrase-plan JSON in
//!   `organic-math-plan.txt` a visual reads and applies with no model at all.
//!
//! ## Where the override lives (documented design choice)
//! The override lives in the **visual's `Shared` working copy**, applied each frame
//! before `ParamValues` is built (so geometry holds flow into `draw_tissue`) and before
//! `build_uniforms` (so look holds reach the shader) — exactly the pulse-routing
//! precedent (`apply_mod`). It is **not** in the plugin's param layer: the agent runs in
//! the visual, a plugin can't set its own params off the GUI thread, and this keeps the
//! actuation lock-free and self-contained. Last-touched-wins vs sliders/CC is enforced
//! by watching the incoming (slider-driven) `Shared` value each frame: if it moved from
//! the baseline observed when the hold was created, the physical control was touched and
//! the agent hold is released.

use crate::ipc::Shared;
use crate::params::{GeneratorMode, MaterialType, SurfaceMode};
use serde::{Deserialize, Serialize};

// ===========================================================================
// Action set v1 (convergence guard §5 — decoupled from the caller)
// ===========================================================================

/// The Tier-1 action vocabulary. One [`dispatch`] entry point applies any of these onto
/// an [`AgentLane`]; the caller (localhost model now, in-process character later) is
/// irrelevant to dispatch.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentAction {
    /// Set one or more named params to absolute values (raw units).
    SetParams(Vec<(String, f32)>),
    /// Apply a saved preset by name (plugin-side; recorded as intent in Tier 1).
    ApplyPreset(String),
    /// Save the current look as a preset (plugin-side; recorded as intent in Tier 1).
    SavePreset(String),
    /// Switch the active generator (`GeneratorMode` ordinal).
    SelectGenerator(u32),
    /// Switch the surface mode (`SurfaceMode` ordinal).
    SelectSurface(u32),
    /// Switch the material type (`MaterialType` ordinal).
    SelectMaterial(u32),
    /// Read back the live look state (feeds the model's next turn).
    ReadState,
    /// Read back render/perf feedback (feeds the model's next turn).
    ReadFeedback,
    /// Free-text narration the agent emits about what it's doing.
    Describe(String),
}

/// The outcome of dispatching one action, for the conversation + the mind-log.
#[derive(Debug, Clone, PartialEq)]
pub enum Dispatched {
    /// Applied, with a short human summary.
    Applied(String),
    /// Rejected, with a reason (advertised-but-not-actuatable, unknown id, …).
    Rejected(String),
}

impl Dispatched {
    pub fn summary(&self) -> &str {
        match self {
            Dispatched::Applied(s) | Dispatched::Rejected(s) => s,
        }
    }
    pub fn is_applied(&self) -> bool {
        matches!(self, Dispatched::Applied(_))
    }
}

/// The single dispatch entry point (convergence guard §5). Decoupled from the source of
/// the action — a localhost model, the phrase-plan executor, or #367's character all call
/// this. Returns one [`Dispatched`] per logical outcome for logging.
pub fn dispatch(lane: &mut AgentLane, action: AgentAction) -> Vec<Dispatched> {
    match action {
        AgentAction::SetParams(pairs) => {
            let mut out = Vec::with_capacity(pairs.len());
            for (id, v) in pairs {
                if is_actuatable(&id) {
                    lane.set(&id, v);
                    out.push(Dispatched::Applied(format!("set {id} = {v}")));
                } else {
                    out.push(Dispatched::Rejected(format!(
                        "no Tier-1 actuation route for '{id}'"
                    )));
                }
            }
            if out.is_empty() {
                out.push(Dispatched::Rejected("set_params: no params".into()));
            }
            out
        }
        AgentAction::SelectGenerator(g) => {
            lane.generator = Some(g);
            vec![Dispatched::Applied(format!("generator = {g}"))]
        }
        AgentAction::SelectSurface(s) => {
            lane.surface = Some(s);
            vec![Dispatched::Applied(format!("surface = {s}"))]
        }
        AgentAction::SelectMaterial(m) => {
            lane.material = Some(m);
            vec![Dispatched::Applied(format!("material = {m}"))]
        }
        AgentAction::ApplyPreset(name) => {
            // Presets live in the plugin's store; the visual can't recall them in Tier 1.
            // Recorded as intent so the convergence guard + mind-log corpus capture it.
            vec![Dispatched::Rejected(format!(
                "apply_preset '{name}' is plugin-side (Tier 1 records intent only)"
            ))]
        }
        AgentAction::SavePreset(name) => vec![Dispatched::Rejected(format!(
            "save_preset '{name}' is plugin-side (Tier 1 records intent only)"
        ))],
        AgentAction::ReadState => vec![Dispatched::Applied("read_state".into())],
        AgentAction::ReadFeedback => vec![Dispatched::Applied("read_feedback".into())],
        AgentAction::Describe(text) => vec![Dispatched::Applied(format!("describe: {text}"))],
    }
}

// ===========================================================================
// Action catalog — generated from param_table.rs (convergence guard §5)
// ===========================================================================

/// A slot's kind in the generated vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Num,
    Int,
    Flag,
    Enum,
}

/// One entry in the agent's action catalog: a param id (the `param_block!` field name),
/// its kind, and — when it is Tier-1 actuatable — its `(min, max)` range.
#[derive(Debug, Clone, PartialEq)]
pub struct CatSlot {
    pub id: &'static str,
    pub kind: SlotKind,
}

impl CatSlot {
    pub fn num(id: &'static str) -> Self {
        CatSlot { id, kind: SlotKind::Num }
    }
    pub fn int(id: &'static str) -> Self {
        CatSlot { id, kind: SlotKind::Int }
    }
    pub fn flag(id: &'static str) -> Self {
        CatSlot { id, kind: SlotKind::Flag }
    }
    pub fn enm(id: &'static str) -> Self {
        CatSlot { id, kind: SlotKind::Enum }
    }
}

/// The curated core vocabulary for the prompt, generated from the `param_block!` slot
/// lists (see the `@catalog_struct` arm in `param_table.rs`). New params added to any of
/// these blocks appear automatically — there is no second hand-maintained list. Kept
/// focused (motion + look + camera) so the system prompt stays small; more blocks can be
/// added here without touching the params.
pub fn core_catalog() -> Vec<CatSlot> {
    let mut out = Vec::new();
    crate::param_table::pack_loop_count::catalog(&mut out);
    crate::param_table::pack_rot_amp::catalog(&mut out);
    crate::param_table::pack_rot_mod::catalog(&mut out);
    crate::param_table::pack_trans_amp::catalog(&mut out);
    crate::param_table::pack_trans_mod::catalog(&mut out);
    crate::param_table::pack_lighting::catalog(&mut out);
    crate::param_table::pack_pbr::catalog(&mut out);
    crate::param_table::pack_surface_fx::catalog(&mut out);
    crate::param_table::pack_camera::catalog(&mut out);
    out
}

/// Render the catalog as compact prompt text: one line per actuatable param with its
/// range, plus a note of the advertised-but-not-yet-actuatable ones.
pub fn catalog_prompt(catalog: &[CatSlot]) -> String {
    let mut s = String::from("PARAMS you can set with set_params (id: range):\n");
    let mut other = Vec::new();
    for c in catalog {
        if let Some((lo, hi)) = id_range(c.id) {
            s.push_str(&format!("  {} : {} .. {}\n", c.id, lo, hi));
        } else {
            other.push(c.id);
        }
    }
    if !other.is_empty() {
        s.push_str("Known but not directly settable in Tier 1 (use select_* / presets): ");
        s.push_str(&other.join(", "));
        s.push('\n');
    }
    s
}

/// A short, condensed architecture brief for the system prompt — enough for the model to
/// reason about what it's "playing" without shipping the whole `ARCHITECTURE.md`.
pub fn architecture_brief() -> &'static str {
    "You are the Performer inside Organon, a parametric 3D generative visualizer driven \
     by an audio host. You play the instrument by adjusting numeric parameters and \
     switching the generator (the geometry engine), the surface mode (how nodes are \
     drawn), and the material (shading). Geometry comes from a rotate-then-translate \
     transform stack; motion is beat-synced. Prefer small, musical moves. Use the tools; \
     do not invent parameter names outside the catalog. When unsure, read_state first."
}

// ===========================================================================
// Capability catalog — what each generator / surface / material *is*, so the model
// can choose a look holistically (not just twiddle the params it happens to know).
// The descriptions live in three EXHAUSTIVE match functions: add a `GeneratorMode`/
// `SurfaceMode`/`MaterialType` variant and it won't compile until it's described here
// (the same source-of-truth discipline as the generated param catalog). Prompt indices
// + names come from the derived `Enum` at runtime, so they can never drift.
// ===========================================================================

/// One-paragraph, artist-facing description of each generator (the geometry engine).
pub fn generator_desc(g: GeneratorMode) -> &'static str {
    match g {
        GeneratorMode::Original => "The seed look — a rotating field of RGB colour cubes that sweep into spirals, helices and jellyfish tentacles. At zero amplifiers it's a clean cube-of-cubes lattice; push the per-axis rotation speeds (rot_mod_x/y/z) and translation amplifiers to bloom it into organic arcs. Try origin_mode Centered for a symmetric fan.",
        GeneratorMode::Frenet => "Ribbons and coils grown by integrating a moving frame from curvature and torsion along a path — helices, springs, winding strands. A phase-offset bundle where every surface mode works; reach for rope, coil, and flowing helical motion, ideally in Swept Tubes.",
        GeneratorMode::Dna => "Two antiparallel backbones spiralling around a spine with base-pair rungs — a literal, optionally supercoiling double helix. Reach for it for biology, genetics, and molecular themes; it pairs beautifully with Swept Tubes plus a Glass or Chrome material.",
        GeneratorMode::Attractor => "Chaotic flowing streamlines forward-integrated from a strange-attractor field (Lorenz, Aizawa, Thomas, Halvorsen) — smoky, orbiting, never-repeating filaments. Suits turbulence, chaos, cosmic and smoke themes; best in Swept Tubes so the strands read as continuous flow.",
        GeneratorMode::Harmonic => "A pulsing sphere deformed by spherical-harmonic modes, each breathing on its own sine — the classic pulsing medusa bell. Reach for it for organic bloom, sea-creature, and radial-symmetry looks; let the beat drive the pulse for a living heartbeat.",
        GeneratorMode::LSystem => "A branching turtle walks a rewritten grammar into ferns, bushes, trees and seaweed. Reach for it whenever the subject is botanical or plant-like; Swept Tubes gives woody branches, and it grows organically frond by frond.",
        GeneratorMode::CurlNoise => "Particles advected through divergence-free curl noise into smooth swirling streamlines — ink, smoke, wind. Suits flowing, atmospheric, fluid motion; Swept Tubes turns each streamline into an elegant ribbon of flow.",
        GeneratorMode::Polarization => "The rotating corkscrew E-field of a circularly polarized wave traced along a fan of rays from a point source — one ray a helix, a dense fan a radiating eye. A grid lattice that Membrane can loft into a rippling shell; suits radiant, physics-flavoured bursts.",
        GeneratorMode::MaxwellField => "The real E/B fields of point charges and oscillating dipoles, superposed with retarded time — the textbook dipole rose as field-line streamlines or a lattice of field vectors. Suits electromagnetism, energy and aura themes; the audio-dipole drive can make the cloud breathe with the music.",
        GeneratorMode::Phyllotaxis => "Golden-angle plant packing — Vogel sunflower disk, Fibonacci sphere, cone or log-spiral shell — with parastichy spiral arms as the strands. Reach for it for seedheads, shells, sacred-geometry and natural-spiral looks; Membrane skins the spiral into a ribbon.",
        GeneratorMode::Mandelbulb => "A raymarched power-8 3-D fractal — bulbous, infinitely detailed alien coral. It has no surface modes of its own but takes the full material, HDR and beat stack; reach for it for psychedelic, otherworldly, deep-detail centrepieces.",
        GeneratorMode::Creature => "A synthetic deep-sea creature raymarched from a union of SDF primitives (ellipsoids, tapered capsules, paddles) along a spine — bell jellies, ribbon-swimmers, paddle-finned predators. No surface modes; a travelling peristaltic warp is the swim (beat-driven), with a Fresnel rim and bright bioluminescent organs. Reach for it for organic, bioluminescent sea-creature centrepieces.",
        GeneratorMode::Kaleidoscope => "A fullscreen N-fold kaleidoscopic fractal with a folding symmetry, drawn flat or as a receding tunnel. Suits hypnotic, symmetric, VJ-tunnel visuals; surface modes don't apply (it's a per-pixel field), so lean on the beat and tone-map choice.",
        GeneratorMode::Boids => "A live swarm obeying separation, alignment and cohesion around a beat-pulsed goal, each agent trailing a strand so the flock reads as murmuration filaments. Suits organic swarm, school and flock motion; works in tubes, cubes or metaball.",
        GeneratorMode::Tessellation => "Aperiodic tilings — Penrose rhombi and kin — emitted as tile-edge geometry. Suits geometric, crystalline, architectural and quasicrystal looks; every surface mode and material apply, so the tile edges can become glowing tubes or brushed metal.",
        GeneratorMode::MinimalSurface => "Raymarched triply-periodic minimal surfaces (gyroid, Schwarz P and D) — smooth least-area lattice skins. No surface modes; pair with Glass plus thin-film iridescence for soap-bubble rainbows. Suits sculptural, mathematical, architectural centrepieces.",
        GeneratorMode::Synchrotron => "The relativistic radiation field of charges orbiting a ring — oriented arrows on a plane whose radiation lobe sweeps a searchlight spiral as the charge circles. Suits high-energy physics, particle-accelerator and beamed-radiation themes with a moving searchlight feel.",
        GeneratorMode::VectorField => "Arrows plotted from a chosen function F(x,y,z) on a lattice, length and colour keyed to magnitude — the maths-Instagram vector field lifted into 3-D. Suits scientific and diagrammatic looks; collapse one grid axis to 1 for the flat 2-D plot.",
        GeneratorMode::None => "The primary generator switched off — the scene is carried by the Scenery layer and world layers alone. Reach for it when you want only a backdrop, environment or scenery with no generated node field on top.",
        GeneratorMode::AxonWaveguide => "A bundle of myelinated nerve fibres as optical waveguides, with periodic Ranvier constrictions and a travelling action-potential pulse. Best viewed in Swept Tubes plus Glass or Refractive; suits neuroscience, signal-propagation and glowing-conduit themes.",
        GeneratorMode::NeuralField => "A tiny neural network raymarched into a soft implicit blob that morphs between latent seeds on the beat. No surface modes; suits abstract, continuously-melting AI-organism centrepieces where the whole form flows between shapes rather than holding still.",
        GeneratorMode::NeuralNetwork => "A graph of soma-blob nodes wired by routed fibre tracts — random-geometric, layered feed-forward, ring or small-world topologies. Best in Swept Tubes plus Glass, or the Neural Tissue surface, for the glowing-tract look; suits AI, brain and connectome themes, and can load a real network JSON (or a GGUF model's architecture via topology = Connectome).",
        GeneratorMode::Lens => "A single raymarched glass lens body, double- or plano-convex. Pair with the Glass or Refractive material so it actually refracts and focuses. Suits optics, eye, and focus or clarity motifs — a clean, minimal hero object.",
        GeneratorMode::Demo => "A hand-built showcase scene — Cornell box, sphere pyramids, glass menagerie, light stage — for showing off reflections and the ray tracer. Reach for it to demo materials and lighting rather than generative motion; the DemoScene setting picks the sub-scene.",
        GeneratorMode::Acoustic => "A radiating sound source (monopole, dipole or quadrupole) whose pressure breathes a multipole shell while particle-velocity drives a glowing aura. The most on-theme generator — the field IS sound; reach for it for audio-reactive and sonification looks, and switch its Cavity model on for 3-D Chladni standing-wave figures.",
        GeneratorMode::FieldEngine => "Renders any closed-form field equation over (x,y,z,t) — pick from a Phenomenon Gallery (Coulomb, dipole, ABC flow, hydrogen orbital, vortex, Gaussian) or type your own expression. Vector programs draw field-lines and aura, scalar a density lattice, complex a phase-tinted cloud. Reach for bespoke physics or maths visuals.",
        GeneratorMode::MapAttractor => "Iterates a chaotic complex map into a glowing additive density fire of visited points. Best in the Splat surface with bloom, or emissive cubes for structure; suits nebula, flame and smoky point-cloud looks. Its parameter orbit can morph the whole shape over a bar.",
    }
}

/// One-paragraph, artist-facing description of each surface mode (how nodes are drawn).
pub fn surface_desc(s: SurfaceMode) -> &'static str {
    match s {
        SurfaceMode::Original => "Solid RGB colour cubes, one per node — the classic Organic Math signature. The default surface; reach for it for crisp, geometric, blocky looks and whenever you want the literal colour-cube identity of the original algorithm.",
        SurfaceMode::FlowAligned => "Each node becomes an oriented rod bridged toward its neighbour, so the geometry follows the direction of flow. Suits streamlines, hair, and fibrous or directional motion — a middle ground between hard cubes and fully continuous tubes.",
        SurfaceMode::SweptTubes => "Nodes sweep into continuous smooth tubes along each strand — pipes, cables, tentacles, vines. The go-to surface for helices, DNA, flow and neural tracts; pairs strongly with Glass or Chrome and with the HSV colour sweep along each tube.",
        SurfaceMode::Metaball => "The node set fuses into one smooth contiguous skin wherever nodes are near — molten blobs, mercury, organic membrane. Reach for it for gooey, liquid, fused-organic looks; it uses its own radius, threshold and smoothness dials rather than box geometry.",
        SurfaceMode::Membrane => "Lofts a continuous sheet through the negative space between strands — a sail, jellyfish bell or web. Suits draped, translucent, membranous forms; it runs the full material stack, so Glass turns it into a shimmering canopy.",
        SurfaceMode::Voxel => "The node set snapped into crisp grid-aligned cubes with flat-face shading and voxel ambient occlusion — a Teardown or MagicaVoxel look. Reach for it for retro, blocky, game-art aesthetics, deliberately distinct from the smooth PBR cubes.",
        SurfaceMode::Volume => "Bakes the nodes into a glowing participating medium — emission with fog-like extinction — for a nebula or luminous fog. Suits smoke, cloud and ethereal energy; it uses density, emission and absorption dials, and shines on the Density-Map Attractor and the Maxwell or Acoustic fields.",
        SurfaceMode::NeuralTissue => "A living-tissue surface built from anatomical primitives — soma cell bodies, capped capsule dendrites, synaptic boutons — wrapped in a waxy translucent membrane. Best on the Neural Network generator; suits brains, neurons and biological wetware.",
        SurfaceMode::Splat => "The node cloud rendered as soft anisotropic Gaussian blobs — additive glowing motes, or lit disc-splats. A dreamy volumetric look sitting between hard cubes and raymarched fog; reach for it for point-cloud, dust and fire (with bloom), and it is ideal for the Density-Map Attractor.",
        SurfaceMode::Plexus => "Wires each node to its nearest neighbours with thin struts plus a marker per node — a breathing field web. Suits network, constellation and data-graph aesthetics; it is generator-agnostic but a no-op on the raymarched generators, which emit no nodes.",
    }
}

/// One-paragraph, artist-facing description of each material (shading).
pub fn material_desc(m: MaterialType) -> &'static str {
    match m {
        MaterialType::Standard => "Metallic-roughness PBR lit by the environment — the versatile default. Sweep metallic and roughness to move from matte plastic to polished metal; reach for it whenever you want a straightforward, physically-based surface without a special optical trick.",
        MaterialType::Chrome => "A polished neutral mirror that reflects the environment sharply. Reach for it for liquid-metal, mirror-ball and high-gloss looks; pair it with a rich HDR environment, since it reflects the surroundings rather than showing a diffuse colour of its own.",
        MaterialType::Glass => "A translucent surface that Fresnel-blends reflection and refraction and lets the scene show through. Suits crystal, ice, water and jewel looks; set ior for the strength of the bend, and add thin-film for iridescent soap-bubble rainbows.",
        MaterialType::Refractive => "Glass plus Beer-Lambert absorption through each body — thin edges stay clear while thick centres go murky in the node's own colour, like coloured water or a deep gemstone. Reach for it for tinted glass, liquid and dense-jewel looks; mat_absorb sets the density.",
        MaterialType::Anisotropic => "An elliptical specular highlight stretched along the surface grain — brushed metal, satin, hair. Reach for it for spun or brushed surfaces; anisotropy sets the strength and aniso_rotation the brush angle. It shines on rods and tubes, where the grain follows the axis.",
        MaterialType::Clearcoat => "A thin glossy dielectric coat over the base colour — car paint, lacquer, ceramic, wet surfaces. Reach for it to add a layered wet high-gloss sheen on top of any colour; it is also available as an overlay on Standard or Chrome.",
        MaterialType::Velvet => "A sheen lobe that blooms at grazing angles — velvet, dust, moss, peach-fuzz. Suits soft, fabric and powdery-organic looks, with the silhouette glowing gently. Also usable as an overlay to dust any other surface.",
        MaterialType::Subsurface => "Translucency driven by real body thickness — thin edges glow while thick centres go deep, like wax, jade, marble or skin. Reach for it for soft organic, candle and flesh looks; sss_thickness drives the glow and sss_radius its penetration depth.",
    }
}

/// One-line, artist-facing gloss of what a settable param *does* (Layer 2 of the #452
/// "describe surface"): the vocabulary the CLI/agent queries to choose values holistically
/// instead of blindly twiddling a range. Curated for every `ACTUATABLE_IDS` entry — a test
/// (`every_actuatable_id_has_a_gloss`) fails if a new actuatable id lands without a gloss,
/// the same source-of-truth discipline as the generator/surface/material descs. Ranges live
/// in `id_range`; this is the *meaning*, not the bounds.
pub fn param_desc(id: &str) -> Option<&'static str> {
    Some(match id {
        // ---- Geometry (Original cube-field generator only) ----
        "loop_count_x" | "loop_count_y" | "loop_count_z" => {
            "How many nodes along this axis of the base grid (Original generator). Together \
             the three set the cube-of-cubes size — e.g. 5×5×5 = 125 cubes; higher = denser \
             and heavier."
        }
        "loop_count_q" => {
            "Length of the accumulating 4th 'q' strand that compounds transforms with no reset \
             — the tentacle / DNA-helix filament. 0 = off; raise it to grow one long winding \
             strand through the field. (Original generator.)"
        }
        "rot_amp_x" | "rot_amp_y" | "rot_amp_z" => {
            "Per-axis rotation AMPLITUDE (degrees) that grows with node index — the arc each \
             arm sweeps. 0 = a straight grid; raise to bloom the lattice into spirals and \
             helices. (Original generator only.)"
        }
        "rot_mod_x" | "rot_mod_y" | "rot_mod_z" => {
            "Per-axis rotation SPEED the beat clock integrates — how fast this axis winds over \
             time. Negative reverses; 0 = a static orientation. (Original generator only.)"
        }
        "trans_amp_x" | "trans_amp_y" | "trans_amp_z" => {
            "Per-axis translation amplifier — how far func(angle) pushes each node outward, \
             scaled by its index. 0 = the clean unit grid; raise to splay the field open. \
             (Original generator only.)"
        }
        "trans_mod_x" | "trans_mod_y" | "trans_mod_z" => {
            "Per-axis translation offset (bipolar) added to every node — a rigid shift of the \
             whole field along this axis. (Original generator only.)"
        }
        "scale_amp" => {
            "How much each node grows with its index — 0 = uniform cubes, higher tapers the \
             field larger toward its far corners. (Original generator only.)"
        }
        // ---- Lighting ----
        "ambient" => {
            "Strength of the environment (IBL) ambient term — the soft fill from the whole \
             surrounding map. Raise for a brighter, flatter look."
        }
        "key_intensity" => {
            "Brightness of the analytic KEY light — the main directional highlight. This is \
             what makes specular hotspots pop on metal and glass."
        }
        "fill_intensity" => {
            "Brightness of the analytic FILL light — the softer opposite-side light that opens \
             up the shadows the key leaves."
        }
        "elevation" => {
            "Key-light elevation angle (degrees) — up/down position of the main light. High = \
             top-lit; negative = dramatic under-lighting."
        }
        "azimuth" => "Key-light azimuth angle (degrees) — the main light's direction around the scene.",
        "glow" => {
            "Emissive self-glow of each surface in its OWN colour — a gentle bloom-feeding \
             halo. For a hot, saturated glow prefer the Emissive material dial instead."
        }
        "opacity" => {
            "Surface opacity. NOTE: true translucency needs a Glass / Refractive / Subsurface \
             material — lowering opacity alone just fades the surface, it doesn't refract."
        }
        // ---- PBR / environment ----
        "metallic" => {
            "Metalness — 0 = dielectric/plastic (has its own diffuse colour), 1 = raw metal \
             (colour comes from what it reflects)."
        }
        "roughness" => {
            "Microsurface roughness — 0 = mirror-sharp reflections, 1 = fully matte/diffuse. \
             The single biggest lever on how polished a material reads."
        }
        "exposure" => {
            "Overall exposure in EV STOPS — 0 is neutral, +3 is roughly 8× brighter and usually \
             blows the scene to white. Move it in small steps."
        }
        "env_intensity" => {
            "Brightness of the environment map / skybox lighting — scales the whole image-based \
             lighting contribution up or down."
        }
        "env_rotation" => {
            "Spin the environment map around (degrees) — moves the reflections and the 'sun' \
             direction without touching the analytic lights."
        }
        "bloom_intensity" => {
            "Bloom (glow bleed) strength. Keep it modest (~0.2–0.4) unless you deliberately want \
             a hazy, blown-out glow."
        }
        "bloom_threshold" => {
            "Brightness above which bloom kicks in — higher = only the hottest highlights bloom, \
             lower = the whole image starts to glow."
        }
        "ior" => {
            "Index of refraction for the Glass / Refractive material — how hard light bends. \
             ~1.33 water, ~1.5 glass, ~2.4 diamond."
        }
        // ---- Surface FX ----
        "subsurface" => {
            "Subsurface-scattering (translucency) amount — light passing through thin parts. \
             Needs a Subsurface (or Glass) material to read; wax, jade, marble, skin."
        }
        "sss_distortion" => {
            "How much the subsurface glow wraps toward the light direction — higher = more \
             forward, waxy scatter around the silhouette."
        }
        "sss_power" => {
            "Falloff sharpness of the subsurface glow — higher = a tighter, more concentrated \
             translucency at the thin edges."
        }
        "iridescence" => {
            "View-angle rainbow sheen amount — the soap-film / beetle-shell colour shift. \
             0 = off."
        }
        "irid_scale" => "Frequency of the iridescent bands — higher = more, tighter rainbow rings across the surface.",
        "irid_shift" => "Hue offset of the iridescence — rotates where the rainbow sweep begins.",
        // ---- Camera (auto-orbit) ----
        "cam_path" => {
            "Auto-orbit camera path (0 = off, 1 = horizontal circle, 2 = vertical circle, \
             4 = spiral, …). This is how you make ANY generator visibly turn or spin — the \
             geometry rotation params only move the Original cube-field."
        }
        "cam_speed" => "Auto-orbit speed — 0.1–0.3 is a slow, musical drift; 1 is a fast spin.",
        "cam_kick" => {
            "How hard each beat kicks the orbit's angular velocity — the momentum 'pump' that \
             makes the camera lurch on the beat, then coast."
        }
        "cam_damping" => {
            "How quickly the beat-kick decays — low = a long coasting glide, high = a snappy \
             settle back to the base speed."
        }
        // ---- Misc ----
        "mat_hue" => {
            "Master material hue tint around the colour wheel (~0 red, ~0.33 green, ~0.6 blue) \
             — the quickest way to recolour the whole look."
        }
        "bell_physical" => {
            "Spherical-harmonics generator only: morphs the pulsing sphere into a soft-body \
             JELLYFISH bell. 1 = a full jellyfish."
        }
        "tempo" => {
            "Manual BPM for the beat-driven motion when NOT locked to a host — the free-running \
             clock speed."
        }
        _ => return None,
    })
}

/// The capability catalog block: every generator / surface / material as `index = name:
/// description`, with indices + names read from the derived `Enum` (never hand-numbered).
fn capability_catalog() -> String {
    use nih_plug::prelude::Enum;
    let mut s = String::new();
    s.push_str("GENERATORS — the geometry engine (select_generator index):\n");
    for (i, name) in <GeneratorMode as Enum>::variants().iter().enumerate() {
        let g = <GeneratorMode as Enum>::from_index(i);
        s.push_str(&format!("  {i} = {name}: {}\n", generator_desc(g)));
    }
    s.push_str("\nSURFACES — how nodes are drawn, works across generators (select_surface index):\n");
    for (i, name) in <SurfaceMode as Enum>::variants().iter().enumerate() {
        let sm = <SurfaceMode as Enum>::from_index(i);
        s.push_str(&format!("  {i} = {name}: {}\n", surface_desc(sm)));
    }
    s.push_str("\nMATERIALS — shading (select_material index):\n");
    for (i, name) in <MaterialType as Enum>::variants().iter().enumerate() {
        let mt = <MaterialType as Enum>::from_index(i);
        s.push_str(&format!("  {i} = {name}: {}\n", material_desc(mt)));
    }
    s
}

/// Value-semantics guidance for the blow-out-prone look params — the model otherwise picks
/// legal-but-ugly values (e.g. exposure 3 EV = 8x too hot) since the catalog only lists ranges.
fn look_notes() -> &'static str {
    "LOOK NOTES: exposure is in EV stops — 0 is neutral, +3 is roughly 8x brighter and usually \
     blows the scene out to white; keep bloom_intensity, ambient and glow modest (~0.2-0.4) \
     unless a hot glow is wanted. metallic and roughness are 0..1 (low roughness = mirror). \
     True translucency needs a Glass, Refractive or Subsurface material, not just low opacity. \
     MOTION: rot_amp_*, rot_mod_* and trans_amp_* only shape the ORIGINAL cube-field generator \
     — other generators (DNA, Harmonic, Attractor, …) ignore them. To make ANY generator visibly \
     SPIN / rotate / slowly turn, set cam_path (the camera auto-orbit: 1 = horizontal circle, \
     2 = vertical circle, 4 = spiral; 0 = off) plus cam_speed (0.1–0.3 = slow). COLOUR: set \
     mat_hue (0..1 around the wheel — ~0.6 blue, ~0 red, ~0.33 green) to tint the material. \
     JELLYFISH: the Spherical-harmonics generator with bell_physical = 1 becomes a soft-body \
     jellyfish bell. Motion is beat-synced; the shape still forms with the transport stopped. \
     Prefer small, musical moves, and read_state before large changes."
}

/// Assemble the full system prompt: architecture brief + tools + the capability catalog
/// (generators/surfaces/materials) + the generated param catalog + look-value notes.
pub fn system_prompt(catalog: &[CatSlot]) -> String {
    format!(
        "{}\n\nTOOLS: set_params(params:[{{id,value}}]), select_generator(index), \
         select_surface(index), select_material(index), apply_preset(name), \
         save_preset(name), read_state(), read_feedback(), describe(text).\n\n\
         {}\n{}\n{}",
        architecture_brief(),
        capability_catalog(),
        catalog_prompt(catalog),
        look_notes(),
    )
}

// ===========================================================================
// Actuation vocabulary — id → Shared slot (the Tier-1 override routes)
// ===========================================================================

/// Whether `id` can be actuated by the Tier-1 override lane (a subset of the advertised
/// catalog; the rest are reachable via `select_*` / presets).
pub fn is_actuatable(id: &str) -> bool {
    id_range(id).is_some()
}

/// The `(min, max)` range for an actuatable id, or `None` if it has no Tier-1 route.
/// Ranges mirror `params.rs` / `clip.rs::RANGES`.
pub fn id_range(id: &str) -> Option<(f32, f32)> {
    let r = match id {
        "loop_count_x" | "loop_count_y" | "loop_count_z" => (1.0, 128.0),
        "loop_count_q" => (0.0, 256.0),
        "rot_amp_x" | "rot_amp_y" | "rot_amp_z" => (0.0, 2160.0),
        "rot_mod_x" | "rot_mod_y" | "rot_mod_z" => (-2.0, 2.0),
        "trans_amp_x" | "trans_amp_y" | "trans_amp_z" => (0.0, 200.0),
        "trans_mod_x" | "trans_mod_y" | "trans_mod_z" => (-200.0, 200.0),
        "scale_amp" => (0.0, 0.5),
        "ambient" => (0.0, 3.0),
        "key_intensity" => (0.0, 6.0),
        "fill_intensity" => (0.0, 3.0),
        "elevation" => (-90.0, 90.0),
        "azimuth" => (-180.0, 180.0),
        "glow" => (0.0, 2.0),
        "opacity" => (0.0, 1.0),
        "metallic" | "roughness" => (0.0, 1.0),
        "exposure" => (-8.0, 8.0),
        "env_intensity" => (0.0, 4.0),
        "env_rotation" => (0.0, 360.0),
        "bloom_intensity" => (0.0, 2.0),
        "bloom_threshold" => (0.0, 4.0),
        "ior" => (1.0, 2.5),
        "subsurface" | "iridescence" => (0.0, 1.0),
        "sss_distortion" => (0.0, 1.0),
        "sss_power" => (0.0, 8.0),
        "irid_scale" => (0.0, 8.0),
        "irid_shift" => (0.0, 1.0),
        "cam_speed" => (0.0, 1.0),
        "cam_kick" => (0.0, 1.0),
        "cam_damping" => (0.0, 1.0),
        // Camera auto-orbit PATH (#317 levers): the generator-agnostic "spin / rotate /
        // slowly turning" control — a CamPath index (0 Off, 1 H-circle, 2 V-circle,
        // 3 Figure-8, 4 Spiral, …). Pair with cam_speed. This is how NON-Original generators
        // (DNA, Harmonic, …) get visible rotation; their own params don't spin them.
        "cam_path" => (0.0, 11.0),
        // Material hue tint (#317 levers): a colour lever ("make it blue / red / green").
        "mat_hue" => (0.0, 1.0),
        // Harmonic soft-body BELL (#317 levers): turns the Spherical-harmonics generator into
        // a physical jellyfish bell (0 = off / spherical, 1 = soft-body bell). Set with the
        // Harmonic generator for a jellyfish.
        "bell_physical" => (0.0, 1.0),
        "tempo" => (40.0, 240.0),
        _ => return None,
    };
    Some(r)
}

/// Read an actuatable id's current raw value from a `Shared` snapshot.
pub fn current(s: &Shared, id: &str) -> Option<f32> {
    Some(match id {
        "loop_count_x" => s.loop_count[0],
        "loop_count_y" => s.loop_count[1],
        "loop_count_z" => s.loop_count[2],
        "loop_count_q" => s.loop_count[3],
        "rot_amp_x" => s.rot_amp[0],
        "rot_amp_y" => s.rot_amp[1],
        "rot_amp_z" => s.rot_amp[2],
        "rot_mod_x" => s.rot_mod[0],
        "rot_mod_y" => s.rot_mod[1],
        "rot_mod_z" => s.rot_mod[2],
        "trans_amp_x" => s.trans_amp[0],
        "trans_amp_y" => s.trans_amp[1],
        "trans_amp_z" => s.trans_amp[2],
        "trans_mod_x" => s.trans_mod[0],
        "trans_mod_y" => s.trans_mod[1],
        "trans_mod_z" => s.trans_mod[2],
        "scale_amp" => s.scale_amp,
        "ambient" => s.lighting[0],
        "key_intensity" => s.lighting[1],
        "fill_intensity" => s.lighting[2],
        "elevation" => s.lighting[3],
        "azimuth" => s.lighting[4],
        "glow" => s.lighting[5],
        "opacity" => s.lighting[6],
        "metallic" => s.pbr[0],
        "roughness" => s.pbr[1],
        "exposure" => s.pbr[2],
        "env_intensity" => s.pbr[3],
        "env_rotation" => s.pbr[4],
        "bloom_intensity" => s.pbr[5],
        "bloom_threshold" => s.pbr[6],
        "ior" => s.pbr[7],
        "subsurface" => s.surface_fx[0],
        "sss_distortion" => s.surface_fx[1],
        "sss_power" => s.surface_fx[2],
        "iridescence" => s.surface_fx[3],
        "irid_scale" => s.surface_fx[4],
        "irid_shift" => s.surface_fx[5],
        "cam_speed" => s.camera[1],
        "cam_kick" => s.camera[2],
        "cam_damping" => s.camera[3],
        "cam_path" => s.camera[0],
        "mat_hue" => s.matcol[0],
        "bell_physical" => s.bell[0],
        "tempo" => s.tempo,
        _ => return None,
    })
}

/// Write an actuatable id's raw value into a `Shared` working copy, clamped to range.
/// Returns whether the id had a route. Applied in the visual before `ParamValues` is
/// built (so geometry holds flow into `draw_tissue`) and before `build_uniforms`.
pub fn actuate(s: &mut Shared, id: &str, v: f32) -> bool {
    let Some((lo, hi)) = id_range(id) else { return false };
    let v = v.clamp(lo, hi);
    match id {
        "loop_count_x" => s.loop_count[0] = v,
        "loop_count_y" => s.loop_count[1] = v,
        "loop_count_z" => s.loop_count[2] = v,
        "loop_count_q" => s.loop_count[3] = v,
        "rot_amp_x" => s.rot_amp[0] = v,
        "rot_amp_y" => s.rot_amp[1] = v,
        "rot_amp_z" => s.rot_amp[2] = v,
        "rot_mod_x" => s.rot_mod[0] = v,
        "rot_mod_y" => s.rot_mod[1] = v,
        "rot_mod_z" => s.rot_mod[2] = v,
        "trans_amp_x" => s.trans_amp[0] = v,
        "trans_amp_y" => s.trans_amp[1] = v,
        "trans_amp_z" => s.trans_amp[2] = v,
        "trans_mod_x" => s.trans_mod[0] = v,
        "trans_mod_y" => s.trans_mod[1] = v,
        "trans_mod_z" => s.trans_mod[2] = v,
        "scale_amp" => s.scale_amp = v,
        "ambient" => s.lighting[0] = v,
        "key_intensity" => s.lighting[1] = v,
        "fill_intensity" => s.lighting[2] = v,
        "elevation" => s.lighting[3] = v,
        "azimuth" => s.lighting[4] = v,
        "glow" => s.lighting[5] = v,
        "opacity" => s.lighting[6] = v,
        "metallic" => s.pbr[0] = v,
        "roughness" => s.pbr[1] = v,
        "exposure" => s.pbr[2] = v,
        "env_intensity" => s.pbr[3] = v,
        "env_rotation" => s.pbr[4] = v,
        "bloom_intensity" => s.pbr[5] = v,
        "bloom_threshold" => s.pbr[6] = v,
        "ior" => s.pbr[7] = v,
        "subsurface" => s.surface_fx[0] = v,
        "sss_distortion" => s.surface_fx[1] = v,
        "sss_power" => s.surface_fx[2] = v,
        "iridescence" => s.surface_fx[3] = v,
        "irid_scale" => s.surface_fx[4] = v,
        "irid_shift" => s.surface_fx[5] = v,
        "cam_speed" => s.camera[1] = v,
        "cam_kick" => s.camera[2] = v,
        "cam_damping" => s.camera[3] = v,
        "cam_path" => s.camera[0] = v,
        "mat_hue" => s.matcol[0] = v,
        "bell_physical" => s.bell[0] = v,
        "tempo" => s.tempo = v,
        _ => return false,
    }
    true
}

// ===========================================================================
// UI-sync apply channel (visual → plugin editor). One `ApplyOp` per line; the editor
// mirrors each onto the real params (via `ParamSetter`) so the sliders/dropdowns never
// disagree with what the agent actually did. Append-and-drain, so a param the user then
// moves isn't re-applied (last-touched-wins). This is the wire format both sides share.
// ===========================================================================

/// One applied agent action, as it crosses the visual→editor apply channel.
#[derive(Debug, Clone, PartialEq)]
pub enum ApplyOp {
    /// Set an actuatable param to a raw value.
    Set(String, f32),
    /// Switch the active generator / surface / material by index.
    Generator(u32),
    Surface(u32),
    Material(u32),
    /// The user pressed "Release agent" — the editor stops mirroring (values stay put).
    Release,
}

impl ApplyOp {
    /// Serialize to a single apply-channel line (space-separated, one op per line).
    pub fn to_line(&self) -> String {
        match self {
            ApplyOp::Set(id, v) => format!("set {id} {v}"),
            ApplyOp::Generator(i) => format!("gen {i}"),
            ApplyOp::Surface(i) => format!("surf {i}"),
            ApplyOp::Material(i) => format!("mat {i}"),
            ApplyOp::Release => "release".to_string(),
        }
    }

    /// Parse one apply-channel line back into an `ApplyOp` (None on a malformed line).
    pub fn parse(line: &str) -> Option<ApplyOp> {
        let mut it = line.split_whitespace();
        match it.next()? {
            "set" => {
                let id = it.next()?.to_string();
                let v: f32 = it.next()?.parse().ok()?;
                Some(ApplyOp::Set(id, v))
            }
            "gen" => Some(ApplyOp::Generator(it.next()?.parse().ok()?)),
            "surf" => Some(ApplyOp::Surface(it.next()?.parse().ok()?)),
            "mat" => Some(ApplyOp::Material(it.next()?.parse().ok()?)),
            "release" => Some(ApplyOp::Release),
            _ => None,
        }
    }
}

/// The apply-channel ops for one dispatched action (empty for non-actuating actions like
/// read_state / describe / presets). Only actuatable ids are forwarded to the editor.
pub fn apply_ops(action: &AgentAction) -> Vec<ApplyOp> {
    match action {
        AgentAction::SetParams(pairs) => pairs
            .iter()
            .filter(|(id, _)| is_actuatable(id))
            .map(|(id, v)| ApplyOp::Set(id.clone(), *v))
            .collect(),
        AgentAction::SelectGenerator(i) => vec![ApplyOp::Generator(*i)],
        AgentAction::SelectSurface(i) => vec![ApplyOp::Surface(*i)],
        AgentAction::SelectMaterial(i) => vec![ApplyOp::Material(*i)],
        _ => Vec::new(),
    }
}

// ===========================================================================
// CLI command channel (#452 Tier 2) — the `organon` CLI's write path. External
// local agents (Bianca) append one op per line to `ipc::cli_cmd_path()`; the
// visual self-detects growth (no `Shared` gen counter — the CLI is never an
// IPC writer) and feeds each op through the SAME dispatch + override lane the
// Performer uses, so last-touched-wins / "Release agent" / slider mirroring /
// mind-log all apply identically. This is the wire format both sides share.
// ===========================================================================

/// One CLI command, as it crosses the CLI→visual command channel.
#[derive(Debug, Clone, PartialEq)]
pub enum CliOp {
    /// Set an actuatable param to a raw (plain-unit) value.
    Set(String, f32),
    /// Switch the active generator / surface / material by ordinal.
    Generator(u32),
    Surface(u32),
    Material(u32),
    /// Release one hold (`Some(id)`) or everything (`None`).
    Release(Option<String>),
    /// A full phrase-plan JSON (single line) for the debug executor.
    Plan(String),
}

impl CliOp {
    /// Serialize to a single command-channel line.
    pub fn to_line(&self) -> String {
        match self {
            CliOp::Set(id, v) => format!("set {id} {v}"),
            CliOp::Generator(i) => format!("gen {i}"),
            CliOp::Surface(i) => format!("surf {i}"),
            CliOp::Material(i) => format!("mat {i}"),
            CliOp::Release(None) => "release".to_string(),
            CliOp::Release(Some(id)) => format!("release {id}"),
            CliOp::Plan(json) => format!("plan {json}"),
        }
    }

    /// Parse one command-channel line (None on a malformed/unknown line, which
    /// the visual's drain skips — forward-compatible with future ops).
    pub fn parse(line: &str) -> Option<CliOp> {
        let line = line.trim();
        if let Some(json) = line.strip_prefix("plan ") {
            return Some(CliOp::Plan(json.trim().to_string()));
        }
        let mut it = line.split_whitespace();
        match it.next()? {
            "set" => {
                let id = it.next()?.to_string();
                let v: f32 = it.next()?.parse().ok()?;
                Some(CliOp::Set(id, v))
            }
            "gen" => Some(CliOp::Generator(it.next()?.parse().ok()?)),
            "surf" => Some(CliOp::Surface(it.next()?.parse().ok()?)),
            "mat" => Some(CliOp::Material(it.next()?.parse().ok()?)),
            "release" => Some(CliOp::Release(it.next().map(|s| s.to_string()))),
            _ => None,
        }
    }

    /// The [`AgentAction`] this op dispatches as, or `None` for ops the visual
    /// handles directly on the lane (release) / a plan that fails to parse.
    pub fn into_action(self) -> Option<AgentAction> {
        match self {
            CliOp::Set(id, v) => Some(AgentAction::SetParams(vec![(id, v)])),
            CliOp::Generator(i) => Some(AgentAction::SelectGenerator(i)),
            CliOp::Surface(i) => Some(AgentAction::SelectSurface(i)),
            CliOp::Material(i) => Some(AgentAction::SelectMaterial(i)),
            CliOp::Plan(json) => PhrasePlan::parse(&json).map(|p| p.as_action()),
            CliOp::Release(_) => None,
        }
    }
}

/// Every Tier-1 actuatable id (the `id_range` routes), for the CLI's `get --all`
/// / `catalog`. Kept in lock-step with [`id_range`] by the tests below.
pub const ACTUATABLE_IDS: &[&str] = &[
    "loop_count_x", "loop_count_y", "loop_count_z", "loop_count_q", //
    "rot_amp_x", "rot_amp_y", "rot_amp_z", "rot_mod_x", "rot_mod_y", "rot_mod_z", //
    "trans_amp_x", "trans_amp_y", "trans_amp_z", "trans_mod_x", "trans_mod_y",
    "trans_mod_z", "scale_amp", //
    "ambient", "key_intensity", "fill_intensity", "elevation", "azimuth", "glow",
    "opacity", //
    "metallic", "roughness", "exposure", "env_intensity", "env_rotation",
    "bloom_intensity", "bloom_threshold", "ior", //
    "subsurface", "sss_distortion", "sss_power", "iridescence", "irid_scale",
    "irid_shift", //
    "cam_path", "cam_speed", "cam_kick", "cam_damping", //
    "mat_hue", "bell_physical", "tempo",
];

/// #452: the CLI channel's startup seed — the cursor adopts the lines present
/// at PROCESS START (counted in the visual's constructor, not at first drain),
/// so a command appended after launch but before the first frame still drains;
/// only the pre-start backlog is skipped (review finding: the first-drain seed
/// was eating live writes issued during startup).
pub fn cli_seed(body: &str) -> usize {
    body.lines().filter(|l| !l.trim().is_empty()).count()
}

/// #452: one CLI-command-channel drain step, pure for testing. `prev_len` is
/// the cached file length, `len_now` the freshly-stat'ed one, `body` the file
/// content when the read SUCCEEDED (`None` = the read failed — return `None`
/// and leave ALL caller state untouched so the next frame retries; committing
/// on a failed read could drop new ops or replay old ones — review finding).
/// Returns the new lines to dispatch plus the `(len, cursor)` to commit.
pub fn cli_drain_step(
    prev_len: u64,
    len_now: u64,
    body: Option<&str>,
    cursor: usize,
) -> Option<(Vec<String>, u64, usize)> {
    if len_now == prev_len {
        return None;
    }
    let body = body?;
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let (range, _, cur) = apply_drain_plan(lines.len(), true, cursor);
    Some((lines[range].iter().map(|s| s.to_string()).collect(), len_now, cur))
}

/// Pure cursor logic for the editor's apply-channel drain (unit-tested). Given the current
/// non-empty line count `n`, whether the cursor has been seeded, and the consumed cursor,
/// return the half-open range of line indices to apply THIS frame plus the updated
/// `(seeded, cursor)`. Rules:
/// - **Seed at editor-open** (`!seeded`) by adopting the current length so a prior session's
///   lines aren't replayed — applying nothing this frame. Crucially this seeds even when the
///   file is absent/empty (`n == 0`), so the FIRST action (which creates the file) is applied
///   on the next frame rather than seeded over.
/// - **Shrink** (`n < cursor`, the visual restarted and truncated the file) → the current
///   lines are a fresh set; apply them from 0 instead of dropping them.
/// - Otherwise apply `cursor..n` (the new lines) and advance the cursor.
pub fn apply_drain_plan(n: usize, seeded: bool, cursor: usize) -> (std::ops::Range<usize>, bool, usize) {
    if !seeded {
        return (0..0, true, n);
    }
    let start = if n < cursor { 0 } else { cursor };
    (start..n, true, n)
}

// ===========================================================================
// The agent override lane (last-touched-wins)
// ===========================================================================

/// One held param: its id, the agent's target value, and the slider baseline observed
/// when the hold was (re)affirmed. If the incoming (slider-driven) value moves off the
/// baseline, the physical control was touched and the hold is released.
#[derive(Debug, Clone)]
pub struct Hold {
    pub id: String,
    pub value: f32,
    pub baseline: f32,
}

/// The visual-side agent override lane. Holds are absolute param sets; `generator` /
/// `surface` / `material` are one-shot selector overrides. Shared with the frame loop
/// behind a mutex; applied each frame by [`AgentLane::apply`].
#[derive(Debug, Default)]
pub struct AgentLane {
    pub holds: Vec<Hold>,
    pub generator: Option<u32>,
    pub surface: Option<u32>,
    pub material: Option<u32>,
}

impl AgentLane {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set (or re-affirm) a hold. Last write wins for the same id. The baseline is left
    /// `NAN` so [`apply`](Self::apply) seeds it from the first incoming frame (which
    /// reflects the current slider), making the very next slider move release it.
    pub fn set(&mut self, id: &str, value: f32) {
        if !is_actuatable(id) {
            return;
        }
        if let Some(h) = self.holds.iter_mut().find(|h| h.id == id) {
            h.value = value;
        } else {
            self.holds.push(Hold {
                id: id.to_string(),
                value,
                baseline: f32::NAN,
            });
        }
    }

    /// Release one hold by id (#452 CLI `release <param>`). The value stays
    /// wherever the hold left it; selector overrides are untouched.
    pub fn release_one(&mut self, id: &str) {
        self.holds.retain(|h| h.id != id);
    }

    /// Clear all holds + selector overrides ("Release agent").
    pub fn release_all(&mut self) {
        self.holds.clear();
        self.generator = None;
        self.surface = None;
        self.material = None;
    }

    /// The ids the agent currently holds (for the editor readout).
    pub fn held_ids(&self) -> Vec<&str> {
        self.holds.iter().map(|h| h.id.as_str()).collect()
    }

    /// Apply the lane to the visual's `Shared` working copy. For each hold: read the
    /// fresh (slider-driven) value; if it moved off the baseline by more than a small
    /// fraction of the param's range, the physical slider was touched → drop the hold
    /// (last-touched-wins). Otherwise overwrite the value and refresh the baseline.
    /// Selector overrides (generator/surface/material) persist until "Release agent" or a
    /// new agent selection — they are not per-param holds.
    pub fn apply(&mut self, s: &mut Shared) {
        let mut i = 0;
        while i < self.holds.len() {
            let id = self.holds[i].id.clone();
            let fresh = match current(s, &id) {
                Some(v) => v,
                None => {
                    self.holds.remove(i);
                    continue;
                }
            };
            let (lo, hi) = id_range(&id).unwrap_or((0.0, 1.0));
            let eps = ((hi - lo).abs() * 1.0e-3).max(1.0e-4);
            let baseline = self.holds[i].baseline;
            if baseline.is_nan() {
                // First sighting: seed the baseline from the current slider value.
                self.holds[i].baseline = fresh;
            } else if (fresh - baseline).abs() > eps {
                // Slider moved — release this hold and don't actuate.
                self.holds.remove(i);
                continue;
            }
            let v = self.holds[i].value;
            actuate(s, &id, v);
            // Keep the baseline tracking the *slider* value we just read (not our
            // overwrite): next frame's incoming `s` again reflects the slider.
            self.holds[i].baseline = fresh;
            i += 1;
        }
        if let Some(g) = self.generator {
            s.generator = g;
        }
        if let Some(sm) = self.surface {
            s.surface_mode = sm;
        }
        if let Some(m) = self.material {
            // Material type rides the spare lighting[7] slot (see param_table pack_lighting).
            s.lighting[7] = m as f32;
        }
    }
}

// ===========================================================================
// Phrase-plan (debug executor path) — scriptable, no model required
// ===========================================================================

/// One move in a hand-written phrase plan. Tier 1 keeps this minimal: an immediate set,
/// or a ramp (the full LFO/bar-latched executor is Round 2 — the `bars` field is the
/// seam; Tier 1 applies the target immediately).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PlanMove {
    /// Set `id` to `value` now.
    SetParam { id: String, value: f32 },
    /// Ramp `id` to `to` over `bars` (Tier 1: applied immediately; the executor seam is
    /// `rails_latch_step` in the visual for Round 2).
    Ramp {
        id: String,
        to: f32,
        #[serde(default)]
        bars: f32,
    },
}

/// A hand-written phrase plan read from `organic-math-plan.txt` (the debug executor).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PhrasePlan {
    #[serde(default)]
    pub name: String,
    pub moves: Vec<PlanMove>,
}

impl PhrasePlan {
    /// Parse a phrase plan from JSON. Returns `None` on malformed input (best-effort —
    /// a bad plan must never crash the visual).
    pub fn parse(json: &str) -> Option<PhrasePlan> {
        serde_json::from_str(json).ok()
    }

    /// Convert the plan into a `SetParams` action (immediate execution). The `bars` on a
    /// ramp is ignored in Tier 1 (seam kept for Round 2).
    pub fn as_action(&self) -> AgentAction {
        let pairs = self
            .moves
            .iter()
            .map(|m| match m {
                PlanMove::SetParam { id, value } => (id.clone(), *value),
                PlanMove::Ramp { id, to, .. } => (id.clone(), *to),
            })
            .collect();
        AgentAction::SetParams(pairs)
    }
}

// ===========================================================================
// Config (endpoint + model) — the organic-math-agent.txt sidecar
// ===========================================================================

/// The OpenAI-compatible localhost endpoint + model, read from `organic-math-agent.txt`
/// (line 1 = endpoint URL, line 2 = model name). Config, not `Shared` floats — so Ollama
/// / LM Studio / llama.cpp / MLX are interchangeable without a rebuild.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentConfig {
    pub endpoint: String,
    pub model: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            // LM Studio's default OpenAI-compatible endpoint (Developer / Local Server).
            // For Ollama instead, change the port to 11434. The endpoint + model are
            // editable in the Mind card and persisted to the `organic-math-agent.txt`
            // sidecar, so this is only the first-run default.
            endpoint: "http://127.0.0.1:1234/v1/chat/completions".to_string(),
            // A 26B-total / 4B-active MoE (QAT): big-model taste at ~4B inference cost,
            // so it follows the action DSL well while leaving GPU headroom for the
            // renderer. On-Mac it produced a recognizable "chrome jellyfish" unaided.
            model: "google/gemma-4-26b-a4b-qat".to_string(),
        }
    }
}

impl AgentConfig {
    /// Parse the two-line sidecar body; blanks fall back to the defaults.
    pub fn parse(body: &str) -> AgentConfig {
        let d = AgentConfig::default();
        let mut lines = body.lines();
        let endpoint = lines.next().map(str::trim).filter(|s| !s.is_empty());
        let model = lines.next().map(str::trim).filter(|s| !s.is_empty());
        AgentConfig {
            endpoint: endpoint.unwrap_or(&d.endpoint).to_string(),
            model: model.unwrap_or(&d.model).to_string(),
        }
    }

    /// Serialize to the two-line sidecar body.
    pub fn to_body(&self) -> String {
        format!("{}\n{}\n", self.endpoint, self.model)
    }

    /// Read the config from the sidecar (defaults if absent/unreadable).
    pub fn load() -> AgentConfig {
        std::fs::read_to_string(crate::ipc::agent_config_path())
            .map(|b| AgentConfig::parse(&b))
            .unwrap_or_default()
    }

    /// Write the config to the sidecar.
    pub fn save(&self) {
        let _ = std::fs::write(crate::ipc::agent_config_path(), self.to_body());
    }
}

// ===========================================================================
// OpenAI-compatible chat client (mockable)
// ===========================================================================

/// One `tool_calls[]` entry in the OpenAI wire shape (`function` calling). Carried on an
/// **assistant** turn so the model's tool intent survives into the next `complete()` — a
/// history that omits it breaks multi-turn tool use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub kind: String,
    pub function: ToolFunction,
}

/// The `function` payload of a [`ToolCall`] — a name and a JSON-string `arguments`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolFunction {
    pub name: String,
    /// Conventionally a JSON *string* on the wire (not an object).
    pub arguments: String,
}

fn default_tool_type() -> String {
    "function".to_string()
}

/// A chat message in the OpenAI wire shape. Beyond `role`/`content` it can carry
/// `tool_calls` (on an assistant turn) and a `tool_call_id` (on a `tool`-role result) so
/// multi-turn tool use round-trips correctly. Both are omitted from the wire when empty,
/// so plain user/assistant/system turns serialize exactly as before.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        ChatMessage {
            role: "system".into(),
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        ChatMessage {
            role: "user".into(),
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        ChatMessage {
            role: "assistant".into(),
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    /// An assistant turn that carries `tool_calls` (content may be empty for a tool-only
    /// reply). Pushing this before the next model call keeps the history OpenAI-valid.
    pub fn assistant_tools(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        ChatMessage {
            role: "assistant".into(),
            content: content.into(),
            tool_calls,
            tool_call_id: None,
        }
    }
    /// A `tool`-role result message keyed by the `tool_call_id` it answers.
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        ChatMessage {
            role: "tool".into(),
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// The client abstraction, so the runtime is mockable in tests (real network calls are
/// the Mac step, never hit in CI).
pub trait ChatClient: Send {
    /// Send the conversation and return the raw response body (an OpenAI
    /// chat.completions JSON string). `max_tokens` caps the reply length when `Some` (the
    /// naming path bounds a runaway reasoning model; the Performer passes `None` for an
    /// uncapped chat reply). Errors are surfaced as `Err(reason)`.
    fn complete(
        &self,
        config: &AgentConfig,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<String, String>;
}

/// A canned client for tests — returns a fixed response body regardless of input.
pub struct MockChatClient {
    pub response: String,
}

impl ChatClient for MockChatClient {
    fn complete(
        &self,
        _c: &AgentConfig,
        _m: &[ChatMessage],
        _max_tokens: Option<u32>,
    ) -> Result<String, String> {
        Ok(self.response.clone())
    }
}

/// The real localhost client: a minimal HTTP/1.1 POST over `std::net::TcpStream` (no TLS
/// — localhost model servers speak plain HTTP), so no HTTP-client dependency is added.
/// Only used on the Mac; tests use [`MockChatClient`] and never touch the network.
pub struct HttpChatClient;

impl ChatClient for HttpChatClient {
    fn complete(
        &self,
        config: &AgentConfig,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<String, String> {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        let (host, port, path) = parse_url(&config.endpoint)?;
        let body = build_request_json(&config.model, messages, max_tokens);
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let mut stream = TcpStream::connect((host.as_str(), port))
            .map_err(|e| format!("connect {host}:{port}: {e}"))?;
        // A local reasoning model can spend minutes "thinking" before it emits the reply, so
        // the read timeout must clear a slow generation — 120s was too tight and cut off a
        // 26B reasoner mid-think, erroring the request before any reply landed (#425).
        let _ = stream.set_read_timeout(Some(Duration::from_secs(READ_TIMEOUT_SECS)));
        stream
            .write_all(req.as_bytes())
            .map_err(|e| format!("write: {e}"))?;
        let mut raw = String::new();
        stream
            .read_to_string(&mut raw)
            .map_err(|e| format!("read: {e}"))?;
        // Decode the HTTP body: de-chunk `Transfer-Encoding: chunked`, else honor
        // `Content-Length` (local model servers use either), so the JSON parses.
        Ok(extract_http_body(&raw))
    }
}

/// Extract the JSON body from a raw HTTP/1.1 response: split headers at the blank line,
/// then **de-chunk** when `Transfer-Encoding: chunked` (decode hex chunk sizes, concatenate
/// the data), else take `Content-Length` bytes, else the whole body. Case-insensitive on
/// header names. Unit-tested against a canned chunked response.
pub fn extract_http_body(raw: &str) -> String {
    let Some((head, body)) = raw.split_once("\r\n\r\n") else {
        return raw.to_string();
    };
    let chunked = head.lines().any(|l| {
        let l = l.trim().to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    if chunked {
        return dechunk(body);
    }
    if let Some(len) = content_length(head) {
        let bytes = body.as_bytes();
        let end = len.min(bytes.len());
        return String::from_utf8_lossy(&bytes[..end]).into_owned();
    }
    body.to_string()
}

/// The `Content-Length` header value, if present (case-insensitive).
fn content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|l| {
        let l = l.trim();
        let low = l.to_ascii_lowercase();
        low.strip_prefix("content-length:")
            .and_then(|_| l[l.find(':')? + 1..].trim().parse::<usize>().ok())
    })
}

/// De-chunk an HTTP `Transfer-Encoding: chunked` body: each chunk is `HEX_SIZE\r\n<data>\r\n`
/// terminated by a `0\r\n` chunk. Byte-safe (chunk data can be multibyte UTF-8). Chunk-size
/// extensions after `;` are ignored.
fn dechunk(body: &str) -> String {
    let bytes = body.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        // The chunk-size line runs to the next CRLF.
        let Some(rel) = find_crlf(&bytes[i..]) else { break };
        let size_line = &bytes[i..i + rel];
        let size_str = std::str::from_utf8(size_line)
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let Ok(size) = usize::from_str_radix(size_str, 16) else { break };
        i += rel + 2; // skip the size line + its CRLF
        if size == 0 {
            break; // last chunk
        }
        let end = (i + size).min(bytes.len());
        out.extend_from_slice(&bytes[i..end]);
        i = end + 2; // skip the chunk data + its trailing CRLF
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Index of the first `\r\n` in `b`, if any.
fn find_crlf(b: &[u8]) -> Option<usize> {
    b.windows(2).position(|w| w == b"\r\n")
}

/// Parse `http://host:port/path` → (host, port, path).
///
/// ⚠️ **Scheme-checked, not host-checked.** This rejects anything that is not `http://`
/// and accepts *any* host after it — the intended endpoint is a loopback model server, but
/// nothing here enforces loopback, so a sidecar edited to a remote address will happily
/// send the conversation over the network in cleartext. Said plainly because the line
/// above used to read "Only `http` (localhost)", which described an enforcement that does
/// not exist; `SECURITY.md` records the same gap for anyone auditing from outside.
fn parse_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("only http:// endpoints are supported (no TLS client): {url}"))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| "bad port".to_string())?),
        None => (authority.to_string(), 80),
    };
    Ok((host, port, path.to_string()))
}

/// How long the localhost client waits for a reply. A reasoning model can think for a long
/// time before emitting the answer, so this is generous — the client blocks a detached
/// worker thread, never the render/audio thread.
const READ_TIMEOUT_SECS: u64 = 300;

/// Reply cap for the *naming* request (a short name never needs more). It bounds a runaway
/// reasoning model that would otherwise loop for thousands of tokens, so the round-trip
/// returns in time and `parse_name_reply` can still recover a name from the reasoning trace.
const NAMING_MAX_TOKENS: u32 = 2048;

/// Build the OpenAI chat.completions request body. `max_tokens` is included only when `Some`
/// (the naming path bounds the reply; the Performer omits it for an uncapped chat turn).
fn build_request_json(model: &str, messages: &[ChatMessage], max_tokens: Option<u32>) -> String {
    let mut v = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": 0.4,
        "stream": false,
    });
    if let Some(n) = max_tokens {
        v["max_tokens"] = serde_json::json!(n);
    }
    v.to_string()
}

// ===========================================================================
// Intelligent preset names (#425) — the local model names a saved preset from what
// the scene actually IS (generator / surface / material / palette / look), so a save
// stops being born as "Preset 7". Built editor-side, serviced in the visual (which owns
// the model client), the reply patched back onto the just-saved preset. Everything here
// is pure + headless-testable: `run_naming` takes a `ChatClient`, so a `MockChatClient`
// exercises the whole round-trip with no network.
// ===========================================================================

/// The scene identity handed to the model for naming. Built from the captured
/// `PresetValues` on the editor thread, serialized as JSON across the process boundary,
/// and turned into a prompt by [`name_user_prompt`] in the visual. `id` (the `name_gen`
/// counter value) is echoed in the reply so the editor matches it to the right preset.
///
/// Rather than a fixed handful of scalars, the request carries a **scope-aware feature
/// fingerprint** ([`scene_features`]): the distinguishing settings of *this* preset in the
/// order that matters (generator + surface form first, then material/palette/look, camera,
/// environment). `scope` names what kind of preset it is ("Scene", "Look", "Generator", …),
/// and `avoid` lists the existing names in the same list so the model makes something
/// genuinely distinct instead of colliding into a "Foo 2" duplicate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameRequest {
    pub id: u32,
    /// What kind of preset this is: "Scene", or a tab label ("Look", "Generator", …).
    pub scope: String,
    /// The distinguishing settings, most-salient first, one human-readable line each.
    pub features: Vec<String>,
    /// Existing preset names in the same list; the name must be clearly different.
    #[serde(default)]
    pub avoid: Vec<String>,
}

/// A rough camera-speed word for the fingerprint.
fn cam_speed_word(s: f32) -> &'static str {
    if s < 0.4 {
        "slow"
    } else if s > 1.2 {
        "fast"
    } else {
        "steady"
    }
}

/// Build a concise, **scope-aware** list of the distinguishing settings for a saved preset
/// — the fingerprint the model names from. For a **Scene** (`PresetScope::Global`) it spans
/// generator + surface form + material/palette/look + camera + environment; for a **tab**
/// preset it emits only that tab's salient features (a Look preset names for its
/// material/palette, a Motion preset for its camera, …). Reuses the same generator/surface/
/// material identities the Performer catalog uses. Pure + headless-testable.
pub fn scene_features(v: &crate::preset::PresetValues, scope: crate::preset::PresetScope) -> Vec<String> {
    use crate::params::{CamPath, Palette};
    use crate::preset::{EditorTab, PresetScope};

    // A field's tab is in this preset iff the scope is that tab, or the whole Scene (which
    // is exactly the four in-scene tabs — Audio/Synth/Settings are never part of a Scene).
    let want = |t: EditorTab| match scope {
        PresetScope::Global => matches!(
            t,
            EditorTab::Generator | EditorTab::Motion | EditorTab::Environment | EditorTab::Look
        ),
        PresetScope::Tab(tab) => tab == t,
    };

    let mut f: Vec<String> = Vec::new();

    // Generator + surface FORM (the geometry engine + how nodes are drawn).
    if want(EditorTab::Generator) {
        f.push(format!("Generator: {}", enum_name::<GeneratorMode>(v.generator)));
        f.push(format!("Surface form: {}", enum_name::<SurfaceMode>(v.surface_mode)));
    }

    // Material + palette + standout look (the Look tab).
    if want(EditorTab::Look) {
        f.push(format!("Material: {}", enum_name::<MaterialType>(v.mat_type)));
        f.push(format!("Palette: {}", enum_name::<Palette>(v.palette)));
        f.push(format!("Base colour: {} (hue {:.2})", hue_word(v.mat_hue), v.mat_hue));
        let mut fx: Vec<&str> = Vec::new();
        if v.iridescence > 0.02 {
            fx.push("iridescent");
        }
        if v.subsurface > 0.02 {
            fx.push("subsurface glow");
        }
        if v.glow > 0.6 {
            fx.push("emissive");
        }
        if v.metallic > 0.6 && v.roughness < 0.3 {
            fx.push("polished metal");
        }
        if v.mat_hue_cycle > 0.01 {
            fx.push("hue-cycling");
        }
        if v.plexus_overlay_on {
            fx.push("plexus web overlay");
        }
        if !fx.is_empty() {
            f.push(format!("Standout look: {}", fx.join(", ")));
        }
    }

    // Camera / motion (the Motion tab).
    if want(EditorTab::Motion) {
        if v.cam_path == 0 {
            f.push("Camera: static".to_string());
        } else {
            f.push(format!(
                "Camera: {} {} move",
                cam_speed_word(v.cam_speed),
                enum_name::<CamPath>(v.cam_path)
            ));
        }
    }

    // Environment / backdrop (the Environment tab).
    if want(EditorTab::Environment) {
        if !v.hdr_path.is_empty() {
            f.push("Environment: HDR image loaded".to_string());
        }
        f.push(
            if v.bg_visible {
                "Backdrop: environment visible"
            } else {
                "Backdrop: dark"
            }
            .to_string(),
        );
        if v.env_tint_amt > 0.02 {
            f.push("Environment tint applied".to_string());
        }
    }

    f
}

/// The system prompt: name a preset in a few evocative words, nothing else.
pub fn name_system_prompt() -> &'static str {
    "You name presets for a generative 3-D art visualizer. You are given the distinguishing \
     settings of one saved preset. Reply with ONLY the name — 2 to 4 words, Title Case, no \
     quotes, no trailing punctuation, no explanation. Ground the name in the SPECIFIC \
     combination you are given: lead from the generator and surface form, then let the \
     palette/colour and any standout look flavour it. Two presets with different generators \
     or surfaces must get clearly different names. Evoke mood and form (e.g. \"Amber Tube \
     Helix\", \"Coral Metaball Drift\", \"Obsidian Lattice\", \"Electric Jellyfish\"). Never \
     restate the parameter names verbatim."
}

/// A rough colour word for a 0..1 hue, so a small model can reason about mood.
fn hue_word(h: f32) -> &'static str {
    match (h.rem_euclid(1.0) * 12.0).floor() as i32 {
        0 => "red",
        1 => "orange",
        2 => "amber",
        3 => "yellow-green",
        4 => "green",
        5 => "teal",
        6 => "cyan",
        7 => "azure",
        8 => "blue",
        9 => "violet",
        10 => "magenta",
        _ => "rose",
    }
}

/// Display name of an `Enum` ordinal (from the derived `#[name = ...]`), clamped.
fn enum_name<T: nih_plug::prelude::Enum>(i: u32) -> &'static str {
    use nih_plug::prelude::Enum;
    let n = <T as Enum>::variants().len();
    if n == 0 {
        return "";
    }
    <T as Enum>::variants()[(i as usize).min(n - 1)]
}

/// Build the user prompt from the scope-aware feature fingerprint: the distinguishing
/// settings as a bullet list, plus the existing names the reply must differ from.
pub fn name_user_prompt(req: &NameRequest) -> String {
    let feats = if req.features.is_empty() {
        "  (no distinguishing settings)\n".to_string()
    } else {
        req.features.iter().map(|l| format!("  - {l}\n")).collect::<String>()
    };
    let avoid = if req.avoid.is_empty() {
        String::new()
    } else {
        format!(
            "\nExisting preset names — your name MUST be clearly different from every one of these:\n{}",
            req.avoid.iter().map(|n| format!("  - {n}\n")).collect::<String>()
        )
    };
    format!(
        "Name this {scope} preset.\n\n\
         Distinguishing settings (most important first):\n{feats}{avoid}\n\
         Reply with the name only.",
        scope = req.scope,
        feats = feats,
        avoid = avoid,
    )
}

/// Title-case one word, preserving an already-upper interior (so "DNA" survives).
fn title_word(w: &str) -> String {
    let mut chars = w.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Clean a raw model reply into a short preset name: take the first non-empty line, strip
/// quotes / markdown / a leading "Name:" label, keep letters/digits/spaces/hyphens,
/// Title-Case, and cap to a handful of words and 40 chars. Empty if nothing usable
/// survives (the caller then keeps the provisional `Preset N`).
pub fn sanitize_preset_name(raw: &str) -> String {
    // Pick the line holding the name: the first non-empty line, unless it is a bare
    // preamble ending in ':' (e.g. "Here you go:") followed by another line — then that.
    let lines: Vec<&str> = raw.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let line = match lines.as_slice() {
        [first, second, ..] if first.ends_with(':') => *second,
        [first, ..] => *first,
        [] => "",
    };
    // Strip a leading short label like "Name:" / "Preset:".
    let line = match line.split_once(':') {
        Some((label, rest)) if label.trim().len() <= 8 && !rest.trim().is_empty() => rest.trim(),
        _ => line,
    };
    // Drop surrounding quotes / markdown emphasis / list bullets.
    let line = line.trim_matches(|c: char| {
        c == '"' || c == '\'' || c == '`' || c == '*' || c == '_' || c == '-' || c.is_whitespace()
    });
    // Keep only name-friendly characters, then collapse to words.
    let cleaned: String = line
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let name = cleaned
        .split_whitespace()
        .take(5)
        .map(title_word)
        .collect::<Vec<_>>()
        .join(" ");
    name.chars().take(40).collect::<String>().trim().to_string()
}

/// Ask the model for a preset name (pure over the [`ChatClient`], so `MockChatClient`
/// drives the whole path in tests). `None` on transport error or an unusable reply — the
/// caller keeps the provisional name.
pub fn run_naming(client: &dyn ChatClient, cfg: &AgentConfig, req: &NameRequest) -> Option<String> {
    let messages = vec![
        ChatMessage::system(name_system_prompt()),
        ChatMessage::user(name_user_prompt(req)),
    ];
    let body = client.complete(cfg, &messages, Some(NAMING_MAX_TOKENS)).ok()?;
    let name = sanitize_preset_name(&parse_name_reply(&body));
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Extract the raw name text from a chat.completions response for the *naming* task.
/// Prefers the assistant `content`. Reasoning models served by LM Studio / llama.cpp / MLX
/// split chain-of-thought into a separate `reasoning_content` (or `reasoning`) field and can
/// return an EMPTY `content` when they exhaust their token budget mid-thought — the #425
/// field report: a local gemma looped for 7k reasoning tokens and left `content` "". When
/// `content` is empty, fall back to the reasoning trace and pull the model's own last
/// decisive candidate so a save still gets a real name instead of staying "Preset N".
fn parse_name_reply(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return String::new();
    };
    let msg = &v["choices"][0]["message"];
    let content = msg["content"].as_str().unwrap_or("").trim();
    if !content.is_empty() {
        return content.to_string();
    }
    // `content` came back empty — recover the answer from the reasoning trace.
    let reasoning = msg["reasoning_content"]
        .as_str()
        .or_else(|| msg["reasoning"].as_str())
        .unwrap_or("");
    last_decisive_candidate(reasoning)
}

/// From a reasoning trace, pick the model's likeliest final answer: the LAST complete
/// `**…**` bold span (reasoning models bold their candidate picks and converge toward the
/// end), else the last non-empty line. `sanitize_preset_name` does the final cleanup.
fn last_decisive_candidate(text: &str) -> String {
    // Last complete **bold** span with non-empty inner text.
    let mut last: Option<String> = None;
    let mut rest = text;
    while let Some(open) = rest.find("**") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("**") else { break };
        let inner = after[..close].trim();
        if !inner.is_empty() {
            last = Some(inner.to_string());
        }
        rest = &after[close + 2..];
    }
    if let Some(b) = last {
        return b;
    }
    // No bold candidate — fall back to the last non-empty line of thought.
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

// ===========================================================================
// Tool-call parsing (against a canned OpenAI response — unit-tested)
// ===========================================================================

/// One parsed tool call: the wire `id` (so the answering `tool` message can be keyed to
/// it), the function name, the raw JSON-string `arguments` (preserved for the echoed
/// assistant turn), and the mapped [`AgentAction`] (`None` for an unknown tool name).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolInvocation {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub action: Option<AgentAction>,
}

/// The model's parsed reply: free-text `content` (may be empty for a tool-only reply) plus
/// any `tool_calls`. Used by the worker to (a) echo the assistant turn back into the
/// conversation and (b) answer each tool call with a `tool`-role result.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AgentReply {
    pub text: String,
    pub tool_calls: Vec<ToolInvocation>,
}

/// Parse an OpenAI chat.completions response body into an [`AgentReply`] (text +
/// tool-call invocations with ids). Best-effort — malformed JSON yields an empty reply.
pub fn parse_reply(body: &str) -> AgentReply {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return AgentReply::default();
    };
    let msg = &v["choices"][0]["message"];
    let text = msg["content"].as_str().unwrap_or("").to_string();
    let mut tool_calls = Vec::new();
    if let Some(calls) = msg["tool_calls"].as_array() {
        for (i, call) in calls.iter().enumerate() {
            let name = call["function"]["name"].as_str().unwrap_or("").to_string();
            // Fall back to a synthetic id so the tool-result message can still be keyed.
            let id = call["id"].as_str().map(str::to_string).unwrap_or_else(|| format!("call_{i}"));
            let args_raw = call["function"]["arguments"].clone();
            // `arguments` is conventionally a JSON *string*; also accept an object. Keep a
            // string form for the echoed assistant turn, and a parsed form for dispatch.
            let (args_str, args_val): (String, serde_json::Value) = match args_raw {
                serde_json::Value::String(s) => {
                    let val = serde_json::from_str(&s).unwrap_or(serde_json::Value::Null);
                    (s, val)
                }
                other => (other.to_string(), other),
            };
            let action = tool_to_action(&name, &args_val);
            tool_calls.push(ToolInvocation { id, name, arguments: args_str, action });
        }
    }
    // Fallback for local servers that DON'T return structured `tool_calls`. This client
    // sends no `tools` schema (see `build_request_json`), so LM Studio / llama.cpp / MLX
    // typically leave `tool_calls` empty and emit the action as PLAIN TEXT in `content`,
    // exactly in the compact DSL the system prompt teaches, e.g.
    //   set_params(params:[{id:metallic,value:1},{id:roughness,value:0.05}])
    // Parse that here so the Performer actually drives params against a real model — not
    // just canned structured replies. Structured `tool_calls` still take precedence.
    if tool_calls.is_empty() {
        tool_calls = parse_dsl_calls(&text);
    }
    AgentReply { text, tool_calls }
}

/// Extract the action DSL from a model's plain-text `content` (the fallback when a local
/// server returns no structured `tool_calls`). Scans for each known tool name followed by
/// a balanced-paren argument list — `set_params(params:[{id,value},…])`,
/// `select_generator(N)`, `select_surface(N)`, `select_material(N)`,
/// `apply_preset(name)`, `save_preset(name)`, `describe(text)`, `read_state()`,
/// `read_feedback()` — in textual order. The DSL is intentionally NOT valid JSON
/// (unquoted keys), so this is a small lenient reader, not `serde_json`.
fn parse_dsl_calls(text: &str) -> Vec<ToolInvocation> {
    const NAMES: &[&str] = &[
        "set_params",
        "select_generator",
        "select_surface",
        "select_material",
        "apply_preset",
        "save_preset",
        "read_state",
        "read_feedback",
        "describe",
    ];
    // Collect (start, close, name, args) hits across all tool names. Require a leading
    // word boundary so a name embedded in a longer token doesn't match (e.g. `set_params`
    // inside `reset_params`). `(`/`)` are ASCII, so byte indexing stays on char boundaries.
    let bytes = text.as_bytes();
    let mut hits: Vec<(usize, usize, &'static str, String)> = Vec::new();
    for &name in NAMES {
        let mut from = 0;
        while let Some(rel) = text[from..].find(name) {
            let start = from + rel;
            let after = start + name.len();
            let boundary_ok = start == 0
                || !matches!(bytes[start - 1], b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_');
            let rest = &text[after..];
            let ws = rest.len() - rest.trim_start().len();
            let paren = after + ws;
            if boundary_ok && text[paren..].starts_with('(') {
                if let Some(close) = matching_paren(text, paren) {
                    hits.push((start, close, name, text[paren + 1..close].to_string()));
                    from = close + 1;
                    continue;
                }
            }
            from = after;
        }
    }
    hits.sort_by_key(|h| h.0);
    // Walk in positional order, skipping any hit that starts INSIDE an already-accepted
    // call's parenthesised span — so a tool name embedded in another call's string
    // arguments (e.g. `describe(text:"... set_params(...)")`) is not dispatched a second
    // time. Each accepted call gets a positionally-unique id so multiple calls to the same
    // tool don't collide on `tool_call_id` in the conversation history.
    let mut out: Vec<ToolInvocation> = Vec::new();
    let mut covered_until = 0usize;
    for (start, close, name, args) in hits {
        if start < covered_until {
            continue;
        }
        covered_until = close + 1;
        if let Some(action) = dsl_args_to_action(name, &args) {
            let i = out.len();
            out.push(ToolInvocation {
                id: format!("dsl_{i}_{}", action_tag(&action)),
                name: name.to_string(),
                arguments: args.trim().to_string(),
                action: Some(action),
            });
        }
    }
    out
}

/// A short, stable id suffix so a tool-result message keys back to its call.
fn action_tag(a: &AgentAction) -> String {
    match a {
        AgentAction::SetParams(_) => "set_params".into(),
        AgentAction::SelectGenerator(n) => format!("gen_{n}"),
        AgentAction::SelectSurface(n) => format!("surf_{n}"),
        AgentAction::SelectMaterial(n) => format!("mat_{n}"),
        AgentAction::ApplyPreset(_) => "apply_preset".into(),
        AgentAction::SavePreset(_) => "save_preset".into(),
        AgentAction::ReadState => "read_state".into(),
        AgentAction::ReadFeedback => "read_feedback".into(),
        AgentAction::Describe(_) => "describe".into(),
    }
}

/// Index of the `)` matching the `(` at byte offset `open` (nesting-aware). `(`/`)` are
/// ASCII, so byte scanning stays on char boundaries even if the args contain unicode.
fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Map one DSL call (name + raw arg text) to an [`AgentAction`], mirroring the structured
/// [`tool_to_action`] but reading the unquoted DSL forms.
fn dsl_args_to_action(name: &str, args: &str) -> Option<AgentAction> {
    match name {
        "set_params" => {
            let pairs = dsl_set_params_pairs(args);
            (!pairs.is_empty()).then_some(AgentAction::SetParams(pairs))
        }
        "select_generator" => Some(AgentAction::SelectGenerator(dsl_first_u32(args)?)),
        "select_surface" => Some(AgentAction::SelectSurface(dsl_first_u32(args)?)),
        "select_material" => Some(AgentAction::SelectMaterial(dsl_first_u32(args)?)),
        "apply_preset" => {
            let s = dsl_string(args, "name");
            (!s.is_empty()).then_some(AgentAction::ApplyPreset(s))
        }
        "save_preset" => {
            let s = dsl_string(args, "name");
            (!s.is_empty()).then_some(AgentAction::SavePreset(s))
        }
        "describe" => Some(AgentAction::Describe(dsl_string(args, "text"))),
        "read_state" => Some(AgentAction::ReadState),
        "read_feedback" => Some(AgentAction::ReadFeedback),
        _ => None,
    }
}

/// Parse `params:[{id:X,value:N},…]` (or a flat `{id:X,value:N}`) from `set_params` args.
fn dsl_set_params_pairs(args: &str) -> Vec<(String, f32)> {
    let mut pairs = Vec::new();
    let mut i = 0;
    while let Some(open_rel) = args[i..].find('{') {
        let open = i + open_rel;
        let Some(close_rel) = args[open..].find('}') else { break };
        let close = open + close_rel;
        let group = &args[open + 1..close];
        if let (Some(id), Some(v)) = (
            dsl_field(group, "id"),
            dsl_field(group, "value").and_then(|v| v.trim().parse::<f32>().ok()),
        ) {
            let id = id.trim().trim_matches('"').trim_matches('\'');
            if !id.is_empty() {
                pairs.push((id.to_string(), v));
            }
        }
        i = close + 1;
    }
    pairs
}

/// Read `key:<token>` out of a `{…}` group, where `<token>` runs to the next `,` or end.
fn dsl_field<'a>(group: &'a str, key: &str) -> Option<&'a str> {
    let pos = group.find(key)?;
    let after = group[pos + key.len()..].trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    let end = after.find(',').unwrap_or(after.len());
    Some(after[..end].trim())
}

/// First unsigned integer appearing in `args` (`index:5` or bare `5`).
fn dsl_first_u32(args: &str) -> Option<u32> {
    let digits: String = args
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// A string arg: drop an optional leading `key:` label, then surrounding quotes.
fn dsl_string(args: &str, key: &str) -> String {
    let a = args.trim();
    let a = a
        .strip_prefix(key)
        .map(str::trim_start)
        .and_then(|r| r.strip_prefix(':'))
        .map(str::trim)
        .unwrap_or(a);
    a.trim_matches('"').trim_matches('\'').trim().to_string()
}

/// Parse an OpenAI chat.completions response body into `(assistant_text, actions)`.
/// Back-compat convenience over [`parse_reply`] (drops the tool-call ids); the worker uses
/// [`parse_reply`] so it can round-trip the assistant tool turn + `tool` results.
pub fn parse_response(body: &str) -> (String, Vec<AgentAction>) {
    let reply = parse_reply(body);
    let actions = reply.tool_calls.into_iter().filter_map(|t| t.action).collect();
    (reply.text, actions)
}

/// Map one tool name + parsed arguments to an [`AgentAction`].
pub fn tool_to_action(name: &str, args: &serde_json::Value) -> Option<AgentAction> {
    match name {
        "set_params" => {
            let mut pairs = Vec::new();
            // Accept `{params:[{id,value},...]}` or a flat `{id,value}`.
            if let Some(arr) = args["params"].as_array() {
                for p in arr {
                    if let (Some(id), Some(val)) = (p["id"].as_str(), p["value"].as_f64()) {
                        pairs.push((id.to_string(), val as f32));
                    }
                }
            } else if let (Some(id), Some(val)) = (args["id"].as_str(), args["value"].as_f64()) {
                pairs.push((id.to_string(), val as f32));
            }
            Some(AgentAction::SetParams(pairs))
        }
        "select_generator" => Some(AgentAction::SelectGenerator(arg_u32(args, "index")?)),
        "select_surface" => Some(AgentAction::SelectSurface(arg_u32(args, "index")?)),
        "select_material" => Some(AgentAction::SelectMaterial(arg_u32(args, "index")?)),
        "apply_preset" => Some(AgentAction::ApplyPreset(arg_str(args, "name")?)),
        "save_preset" => Some(AgentAction::SavePreset(arg_str(args, "name")?)),
        "read_state" => Some(AgentAction::ReadState),
        "read_feedback" => Some(AgentAction::ReadFeedback),
        "describe" => Some(AgentAction::Describe(arg_str(args, "text").unwrap_or_default())),
        _ => None,
    }
}

fn arg_u32(args: &serde_json::Value, key: &str) -> Option<u32> {
    args[key]
        .as_u64()
        .map(|n| n as u32)
        .or_else(|| args[key].as_f64().map(|n| n as u32))
        .or_else(|| args[key].as_str().and_then(|s| s.parse().ok()))
}

fn arg_str(args: &serde_json::Value, key: &str) -> Option<String> {
    args[key].as_str().map(|s| s.to_string())
}

// ===========================================================================
// Live state / feedback snapshot — answers ReadState / ReadFeedback (finding #5)
// ===========================================================================

/// A compact live snapshot the worker injects as the `tool`-result for `read_state` /
/// `read_feedback`, so those tools return real data instead of a no-op. `state_*` come
/// from the `Shared` snapshot; `fps`/`gpu_ms`/`cpu_ms`/`instances` are the render loop's
/// perf metrics, stamped by the visual each frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveState {
    pub generator: u32,
    pub surface: u32,
    pub material: u32,
    /// A few key params (id, value) for the state readout.
    pub params: Vec<(String, f32)>,
    pub fps: f32,
    pub gpu_ms: f32,
    pub cpu_ms: f32,
    pub instances: u32,
}

/// The key params surfaced by `read_state` (kept small so the tool result stays compact).
const STATE_PARAM_IDS: &[&str] = &[
    "loop_count_x", "loop_count_y", "loop_count_z", "loop_count_q", "glow", "metallic",
    "roughness", "exposure", "tempo",
];

impl LiveState {
    /// Capture the state half (generator/surface/material + key params) from a `Shared`.
    /// The perf half is filled by [`with_perf`](Self::with_perf).
    pub fn from_shared(s: &Shared) -> Self {
        let params = STATE_PARAM_IDS
            .iter()
            .filter_map(|id| current(s, id).map(|v| (id.to_string(), v)))
            .collect();
        LiveState {
            generator: s.generator,
            surface: s.surface_mode,
            material: s.lighting[7] as u32,
            params,
            ..Default::default()
        }
    }

    /// Stamp the render loop's perf metrics onto the snapshot.
    pub fn with_perf(mut self, fps: f32, gpu_ms: f32, cpu_ms: f32, instances: u32) -> Self {
        self.fps = fps;
        self.gpu_ms = gpu_ms;
        self.cpu_ms = cpu_ms;
        self.instances = instances;
        self
    }

    /// Compact JSON for the `read_state` tool result.
    pub fn state_json(&self) -> String {
        let params: Vec<serde_json::Value> = self
            .params
            .iter()
            .map(|(id, v)| serde_json::json!({ "id": id, "value": v }))
            .collect();
        serde_json::json!({
            "generator": self.generator,
            "surface": self.surface,
            "material": self.material,
            "params": params,
        })
        .to_string()
    }

    /// Compact JSON for the `read_feedback` tool result.
    pub fn feedback_json(&self) -> String {
        serde_json::json!({
            "fps": self.fps,
            "gpu_ms": self.gpu_ms,
            "cpu_ms": self.cpu_ms,
            "instances": self.instances,
        })
        .to_string()
    }
}

// ===========================================================================
// Tests (catalog gen, tool-call parse+dispatch, override last-touched-wins, plan parse)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // A NameRequest with a representative fingerprint for the naming tests.
    fn sample_name_req() -> NameRequest {
        NameRequest {
            id: 7,
            scope: "Scene".to_string(),
            features: vec![
                "Generator: DNA".to_string(),
                "Surface form: Swept Tubes".to_string(),
                "Material: Glass".to_string(),
                "Base colour: blue (hue 0.67)".to_string(),
                "Camera: slow Figure Eight move".to_string(),
            ],
            avoid: vec!["Sapphire Helix".to_string()],
        }
    }

    #[test]
    fn name_user_prompt_lays_out_the_fingerprint_and_avoid_list() {
        let p = name_user_prompt(&sample_name_req());
        assert!(p.contains("Scene preset"), "scope in prompt: {p}");
        // Each distinguishing feature appears verbatim as a bullet…
        assert!(p.contains("Generator: DNA"), "generator feature in prompt");
        assert!(p.contains("Surface form: Swept Tubes"), "surface feature in prompt");
        assert!(p.contains("Glass"), "material feature in prompt");
        assert!(p.contains("blue"), "colour feature in prompt");
        // …and the existing name to avoid is present with the distinctness instruction.
        assert!(p.contains("Sapphire Helix"), "avoid name in prompt");
        assert!(p.to_lowercase().contains("clearly different"), "distinctness instruction: {p}");
    }

    #[test]
    fn build_request_json_includes_max_tokens_only_when_capped() {
        // The naming path caps the reply so a reasoning model can't run away; the Performer
        // omits the field for an uncapped chat turn.
        let msgs = [ChatMessage::user("hi")];
        let capped = build_request_json("m", &msgs, Some(2048));
        let v: serde_json::Value = serde_json::from_str(&capped).unwrap();
        assert_eq!(v["max_tokens"], serde_json::json!(2048));
        assert_eq!(v["stream"], serde_json::json!(false));

        let uncapped = build_request_json("m", &msgs, None);
        let v: serde_json::Value = serde_json::from_str(&uncapped).unwrap();
        assert!(v.get("max_tokens").is_none(), "no cap when None: {uncapped}");
    }

    #[test]
    fn scene_features_are_scope_aware_and_clamp() {
        use crate::params::OrganicMathParams;
        use crate::preset::{EditorTab, PresetScope, PresetValues};
        let mut v = PresetValues::capture_params_only(&OrganicMathParams::default());
        v.generator = GeneratorMode::Frenet as u32;
        v.surface_mode = SurfaceMode::Metaball as u32;
        v.mat_type = MaterialType::Chrome as u32;
        v.cam_path = 1; // a camera move
        v.bg_visible = true;

        // Scene = the full fingerprint: generator + surface + material + camera + backdrop.
        let scene = scene_features(&v, PresetScope::Global);
        let joined = scene.join(" | ");
        assert!(joined.contains("Frenet"), "generator in scene: {joined}");
        assert!(joined.contains("Metaball"), "surface in scene");
        assert!(joined.contains("Chrome"), "material in scene");
        assert!(scene.iter().any(|l| l.starts_with("Camera")), "camera in scene");
        assert!(scene.iter().any(|l| l.starts_with("Backdrop")), "backdrop in scene");

        // A Look-tab preset names only from its own fields (no generator/camera line).
        let look = scene_features(&v, PresetScope::Tab(EditorTab::Look));
        assert!(look.iter().any(|l| l.contains("Chrome")), "material in Look");
        assert!(!look.iter().any(|l| l.starts_with("Generator")), "no generator in Look");
        assert!(!look.iter().any(|l| l.starts_with("Camera")), "no camera in Look");

        // Out-of-range ordinals clamp instead of panicking (via enum_name).
        v.generator = 9999;
        v.surface_mode = 9999;
        let _ = scene_features(&v, PresetScope::Global); // no panic == pass
    }

    #[test]
    fn sanitize_preset_name_cleans_model_chatter() {
        // Quotes, a label prefix, a trailing note, and stray punctuation all get stripped.
        assert_eq!(sanitize_preset_name("\"Amber Tube Helix\""), "Amber Tube Helix");
        assert_eq!(sanitize_preset_name("Name: Obsidian Bloom"), "Obsidian Bloom");
        assert_eq!(
            sanitize_preset_name("Here you go:\nGlass Cathedral\n(hope that fits!)"),
            "Glass Cathedral"
        );
        assert_eq!(sanitize_preset_name("- **Electric Jellyfish**"), "Electric Jellyfish");
        // Preserves an all-caps token, caps the word count, and never returns empty-on-junk.
        assert_eq!(sanitize_preset_name("dna double helix"), "Dna Double Helix");
        assert!(sanitize_preset_name("").is_empty());
        assert!(sanitize_preset_name("!!!").is_empty());
        assert!(sanitize_preset_name("one two three four five six seven").split(' ').count() <= 5);
    }

    #[test]
    fn run_naming_round_trips_through_a_mock_client() {
        // The whole path: prompt → model → sanitized name, no network.
        let client = MockChatClient {
            response: r#"{"choices":[{"message":{"role":"assistant",
                "content":"\"Sapphire DNA Coil\""}}]}"#
                .to_string(),
        };
        let name = run_naming(&client, &AgentConfig::default(), &sample_name_req());
        assert_eq!(name.as_deref(), Some("Sapphire DNA Coil"));

        // An empty / unusable reply yields None (caller keeps the provisional name).
        let empty = MockChatClient {
            response: r#"{"choices":[{"message":{"role":"assistant","content":"   "}}]}"#.to_string(),
        };
        assert_eq!(run_naming(&empty, &AgentConfig::default(), &sample_name_req()), None);
    }

    #[test]
    fn run_naming_recovers_from_reasoning_only_reply() {
        // #425 field bug: a reasoning model exhausts its budget mid-thought and returns an
        // EMPTY `content`, with the whole answer stranded in `reasoning_content`. Naming must
        // still recover a real name (the model's last bold pick) instead of failing.
        let reasoning = "Let's try: **Scarlet Flowing Lattice**\\n\
                         Or maybe **Vermilion Flowing Cuboid**\\n\
                         Final decision: **Scarlet Flowing Cuboid Luster**";
        let body = format!(
            r#"{{"choices":[{{"message":{{"role":"assistant","content":"",
               "reasoning_content":"{reasoning}"}}}}]}}"#
        );
        let client = MockChatClient { response: body };
        let name = run_naming(&client, &AgentConfig::default(), &sample_name_req());
        // The LAST complete bold span wins (models converge toward the end).
        assert_eq!(name.as_deref(), Some("Scarlet Flowing Cuboid Luster"));

        // `reasoning` alias (some servers) is also honored, and a trace with no bold span
        // falls back to the last non-empty line.
        let alias = MockChatClient {
            response: r#"{"choices":[{"message":{"role":"assistant","content":"",
                "reasoning":"thinking...\nAmber Tube Helix"}}]}"#
                .to_string(),
        };
        let name = run_naming(&alias, &AgentConfig::default(), &sample_name_req());
        assert_eq!(name.as_deref(), Some("Amber Tube Helix"));
    }

    #[test]
    fn last_decisive_candidate_prefers_last_bold_span() {
        assert_eq!(
            last_decisive_candidate("**First Pick** then **Final Pick**"),
            "Final Pick"
        );
        // No bold → last non-empty line.
        assert_eq!(last_decisive_candidate("noise\n  Obsidian Lattice  \n"), "Obsidian Lattice");
        // An unterminated bold marker is ignored (no panic, no partial span).
        assert_eq!(last_decisive_candidate("**Complete** and **danglin"), "Complete");
        assert_eq!(last_decisive_candidate(""), "");
    }

    #[test]
    fn name_request_json_round_trips() {
        let req = sample_name_req();
        let json = serde_json::to_string(&req).unwrap();
        let back: NameRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn catalog_is_generated_from_param_table() {
        let cat = core_catalog();
        // A param that lives in a core block must appear mechanically.
        assert!(cat.iter().any(|c| c.id == "loop_count_x" && c.kind == SlotKind::Int));
        assert!(cat.iter().any(|c| c.id == "glow" && c.kind == SlotKind::Num));
        assert!(cat.iter().any(|c| c.id == "metallic"));
        assert!(cat.iter().any(|c| c.id == "iridescence"));
        // Enum + reserved/expr slots: mat_type is an enum in the lighting block; the
        // inc_scale expr slot in rot_mod contributes no vocab (no field name).
        assert!(cat.iter().any(|c| c.id == "mat_type" && c.kind == SlotKind::Enum));
        assert!(!cat.iter().any(|c| c.id == "inc_scale"));
        // The prompt lists actuatable ranges.
        let prompt = catalog_prompt(&cat);
        assert!(prompt.contains("glow : 0 .. 2"));
    }

    #[test]
    fn capability_catalog_covers_every_variant_and_is_bounded() {
        use nih_plug::prelude::Enum;
        // Every generator/surface/material variant has a non-empty, length-bounded
        // description (the exhaustive match already forces coverage at compile time; this
        // guards against a stub "" or accidental bloat, and keeps them single-line).
        for i in 0..<GeneratorMode as Enum>::variants().len() {
            let d = generator_desc(<GeneratorMode as Enum>::from_index(i));
            assert!(!d.is_empty() && d.len() <= 500, "generator {i} desc len {}", d.len());
            assert!(!d.contains('\n'));
        }
        for i in 0..<SurfaceMode as Enum>::variants().len() {
            let d = surface_desc(<SurfaceMode as Enum>::from_index(i));
            assert!(!d.is_empty() && d.len() <= 500, "surface {i} desc len {}", d.len());
        }
        for i in 0..<MaterialType as Enum>::variants().len() {
            let d = material_desc(<MaterialType as Enum>::from_index(i));
            assert!(!d.is_empty() && d.len() <= 500, "material {i} desc len {}", d.len());
        }
        // The assembled block names real capabilities with their indices, and the full
        // system prompt embeds it.
        let cat = capability_catalog();
        assert!(cat.contains("Swept Tubes"));
        assert!(cat.contains("Glass"));
        assert!(cat.contains("Spherical harmonics"));
        let prompt = system_prompt(&core_catalog());
        assert!(prompt.contains("GENERATORS"));
        assert!(prompt.contains("LOOK NOTES"));
    }

    #[test]
    fn tool_call_parses_and_dispatches() {
        // A canned OpenAI response with a set_params tool call (arguments as a string,
        // the wire convention).
        let body = r#"{
          "choices":[{"message":{
            "role":"assistant",
            "content":"Warming the look up.",
            "tool_calls":[
              {"type":"function","function":{"name":"set_params",
                "arguments":"{\"params\":[{\"id\":\"glow\",\"value\":1.5},{\"id\":\"metallic\",\"value\":0.8}]}"}},
              {"type":"function","function":{"name":"select_surface","arguments":"{\"index\":2}"}}
            ]
          }}]
        }"#;
        let (text, actions) = parse_response(body);
        assert_eq!(text, "Warming the look up.");
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            AgentAction::SetParams(vec![("glow".into(), 1.5), ("metallic".into(), 0.8)])
        );
        assert_eq!(actions[1], AgentAction::SelectSurface(2));

        // Dispatch onto a fresh lane.
        let mut lane = AgentLane::new();
        for a in actions {
            let outs = dispatch(&mut lane, a);
            assert!(outs.iter().all(|o| o.is_applied()), "{:?}", outs);
        }
        assert_eq!(lane.holds.len(), 2);
        assert_eq!(lane.surface, Some(2));
        let mut ids = lane.held_ids();
        ids.sort();
        assert_eq!(ids, vec!["glow", "metallic"]);
    }

    #[test]
    fn dispatch_rejects_unknown_param() {
        let mut lane = AgentLane::new();
        let outs = dispatch(
            &mut lane,
            AgentAction::SetParams(vec![("no_such_param".into(), 1.0)]),
        );
        assert_eq!(outs.len(), 1);
        assert!(!outs[0].is_applied());
        assert!(lane.holds.is_empty());
    }

    #[test]
    fn override_actuates_and_clamps() {
        let mut s = Shared::default();
        let mut lane = AgentLane::new();
        lane.set("glow", 5.0); // above the 0..2 range → clamps to 2
        lane.apply(&mut s); // first apply seeds baseline (no actuation-release)
        lane.apply(&mut s); // second apply writes the (clamped) value
        assert_eq!(s.lighting[5], 2.0);
    }

    #[test]
    fn override_is_last_touched_wins() {
        let mut s = Shared::default();
        let start = s.lighting[5];
        let mut lane = AgentLane::new();
        lane.set("glow", 1.7);
        // Frame 1: seed baseline from the current slider value; no move yet.
        lane.apply(&mut s);
        assert_eq!(lane.holds.len(), 1);
        // Frame 2: the agent value is written (slider unchanged).
        // Emulate the visual re-reading a fresh slider-driven snapshot each frame: reset
        // the look slot to the slider value before applying.
        s.lighting[5] = start;
        lane.apply(&mut s);
        assert_eq!(s.lighting[5], 1.7, "agent hold should actuate");
        // Frame 3: the user MOVES the physical slider (incoming value jumps).
        s.lighting[5] = start + 0.5;
        lane.apply(&mut s);
        assert!(lane.holds.is_empty(), "slider move must release the hold");
        assert_eq!(s.lighting[5], start + 0.5, "slider value must survive");
    }

    #[test]
    fn release_all_clears_holds_and_selectors() {
        let mut lane = AgentLane::new();
        lane.set("glow", 1.0);
        lane.generator = Some(3);
        lane.release_all();
        assert!(lane.holds.is_empty());
        assert_eq!(lane.generator, None);
    }

    #[test]
    fn phrase_plan_parses_and_becomes_setparams() {
        let json = r#"{
          "name":"warm intro",
          "moves":[
            {"op":"set_param","id":"glow","value":1.2},
            {"op":"ramp","id":"exposure","to":2.0,"bars":4}
          ]
        }"#;
        let plan = PhrasePlan::parse(json).expect("plan parses");
        assert_eq!(plan.name, "warm intro");
        assert_eq!(plan.moves.len(), 2);
        match &plan.as_action() {
            AgentAction::SetParams(p) => {
                assert_eq!(p[0], ("glow".into(), 1.2));
                assert_eq!(p[1], ("exposure".into(), 2.0)); // ramp target applied now
            }
            other => panic!("expected SetParams, got {other:?}"),
        }
        // Malformed → None (never panics).
        assert!(PhrasePlan::parse("{not json").is_none());
    }

    #[test]
    fn agent_config_round_trips() {
        let c = AgentConfig {
            endpoint: "http://127.0.0.1:1234/v1/chat/completions".into(),
            model: "qwen2.5".into(),
        };
        let back = AgentConfig::parse(&c.to_body());
        assert_eq!(c, back);
        // Blank body → defaults.
        assert_eq!(AgentConfig::parse(""), AgentConfig::default());
    }

    #[test]
    fn mock_client_is_usable_without_network() {
        let client = MockChatClient {
            response: r#"{"choices":[{"message":{"role":"assistant","content":"hi","tool_calls":[
              {"type":"function","function":{"name":"read_state","arguments":"{}"}}]}}]}"#
                .into(),
        };
        let body = client
            .complete(&AgentConfig::default(), &[ChatMessage::user("play something warm")], None)
            .unwrap();
        let (_text, actions) = parse_response(&body);
        assert_eq!(actions, vec![AgentAction::ReadState]);
    }

    #[test]
    fn url_parse_splits_authority_and_path() {
        let (h, p, path) = parse_url("http://127.0.0.1:11434/v1/chat/completions").unwrap();
        assert_eq!(h, "127.0.0.1");
        assert_eq!(p, 11434);
        assert_eq!(path, "/v1/chat/completions");
        assert!(parse_url("https://example.com/x").is_err());
    }

    #[test]
    fn extract_http_body_dechunks_chunked_response() {
        // A canned chunked HTTP/1.1 response: the JSON body split across two chunks +
        // the terminating 0-chunk. The hex sizes must be decoded and concatenated.
        let json = r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}]}"#;
        let (a, b) = json.split_at(20);
        let raw = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n{:x}\r\n{}\r\n0\r\n\r\n",
            a.len(), a, b.len(), b
        );
        let body = extract_http_body(&raw);
        assert_eq!(body, json);
        // And it parses back to the reply.
        let reply = parse_reply(&body);
        assert_eq!(reply.text, "ok");
    }

    #[test]
    fn extract_http_body_honors_content_length() {
        let json = r#"{"ok":true}"#;
        // Trailing garbage after Content-Length must be dropped.
        let raw = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}TRAILING",
            json.len(),
            json
        );
        assert_eq!(extract_http_body(&raw), json);
    }

    #[test]
    fn extract_http_body_plain_when_unframed() {
        let json = r#"{"x":1}"#;
        let raw = format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{json}");
        assert_eq!(extract_http_body(&raw), json);
    }

    #[test]
    fn parse_reply_keeps_tool_only_turn_with_ids() {
        // Tool-only reply (empty content) — the assistant turn must still be reconstructible.
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"",
          "tool_calls":[
            {"id":"call_abc","type":"function","function":{"name":"read_state","arguments":"{}"}},
            {"id":"call_def","type":"function","function":{"name":"set_params",
              "arguments":"{\"params\":[{\"id\":\"glow\",\"value\":1.0}]}"}}
          ]}}]}"#;
        let reply = parse_reply(body);
        assert_eq!(reply.text, "");
        assert_eq!(reply.tool_calls.len(), 2);
        assert_eq!(reply.tool_calls[0].id, "call_abc");
        assert_eq!(reply.tool_calls[0].action, Some(AgentAction::ReadState));
        assert_eq!(reply.tool_calls[1].id, "call_def");
        // The echoed assistant turn serializes tool_calls; a plain user turn does not.
        let echoed = ChatMessage::assistant_tools(
            reply.text.clone(),
            reply
                .tool_calls
                .iter()
                .map(|t| ToolCall {
                    id: t.id.clone(),
                    kind: "function".into(),
                    function: ToolFunction { name: t.name.clone(), arguments: t.arguments.clone() },
                })
                .collect(),
        );
        let wire = serde_json::to_string(&echoed).unwrap();
        assert!(wire.contains("\"tool_calls\""));
        assert!(!serde_json::to_string(&ChatMessage::user("hi")).unwrap().contains("tool_calls"));
        // A tool-result message keys back to the call id.
        let tool_msg = ChatMessage::tool("call_abc", "{\"generator\":0}");
        let tw = serde_json::to_string(&tool_msg).unwrap();
        assert!(tw.contains("\"role\":\"tool\""));
        assert!(tw.contains("\"tool_call_id\":\"call_abc\""));
    }

    #[test]
    fn parse_reply_extracts_dsl_set_params_from_content() {
        // The exact shape a real LM Studio / Gemma reply takes: the action is TEXT in
        // `content`, `tool_calls` is empty (no `tools` schema was sent). The Performer
        // must still dispatch it — this is the bug the on-Mac test surfaced.
        let body = r#"{"choices":[{"message":{"role":"assistant",
          "content":"set_params(params:[{id:metallic,value:1},{id:roughness,value:0.05},{id:opacity,value:0.8},{id:key_intensity,value:2},{id:rot_amp_y,value:10},{id:trans_amp_x,value:5}])",
          "tool_calls":[]}}]}"#;
        let reply = parse_reply(body);
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].name, "set_params");
        match &reply.tool_calls[0].action {
            Some(AgentAction::SetParams(pairs)) => {
                assert_eq!(pairs.len(), 6);
                assert_eq!(pairs[0], ("metallic".to_string(), 1.0));
                assert!(pairs.iter().any(|(k, v)| k == "roughness" && (*v - 0.05).abs() < 1e-6));
                assert!(pairs.iter().any(|(k, v)| k == "opacity" && (*v - 0.8).abs() < 1e-6));
                assert!(pairs.iter().any(|(k, v)| k == "key_intensity" && (*v - 2.0).abs() < 1e-6));
                assert!(pairs.iter().any(|(k, v)| k == "rot_amp_y" && (*v - 10.0).abs() < 1e-6));
                assert!(pairs.iter().any(|(k, v)| k == "trans_amp_x" && (*v - 5.0).abs() < 1e-6));
            }
            other => panic!("expected SetParams, got {other:?}"),
        }
    }

    #[test]
    fn parse_reply_prefers_structured_tool_calls_over_dsl_text() {
        // If a server DOES return structured tool_calls, ignore any DSL echoed in content.
        let body = r#"{"choices":[{"message":{"role":"assistant",
          "content":"set_params(params:[{id:glow,value:9}])",
          "tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_state","arguments":"{}"}}]}}]}"#;
        let reply = parse_reply(body);
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].action, Some(AgentAction::ReadState));
    }

    #[test]
    fn parse_dsl_handles_selects_and_presets_in_order() {
        let body = r#"{"choices":[{"message":{"role":"assistant",
          "content":"First select_generator(3), then select_material(1) and apply_preset(name:\"Jelly\").",
          "tool_calls":[]}}]}"#;
        let reply = parse_reply(body);
        let actions: Vec<_> = reply.tool_calls.iter().filter_map(|t| t.action.clone()).collect();
        assert_eq!(
            actions,
            vec![
                AgentAction::SelectGenerator(3),
                AgentAction::SelectMaterial(1),
                AgentAction::ApplyPreset("Jelly".into()),
            ]
        );
    }

    #[test]
    fn parse_dsl_ignores_bare_tool_mentions_without_parens() {
        // Prose that names a tool but doesn't call it must not dispatch anything.
        let body = r#"{"choices":[{"message":{"role":"assistant",
          "content":"I could use set_params or read_state, but I will wait.","tool_calls":[]}}]}"#;
        let reply = parse_reply(body);
        assert!(reply.tool_calls.is_empty());
    }

    #[test]
    fn parse_dsl_does_not_double_dispatch_nested_calls() {
        // A tool name embedded in another call's string args must not dispatch twice
        // (Bugbot/VADE: nested double-dispatch). Only the outer `describe` should fire.
        let body = r#"{"choices":[{"message":{"role":"assistant",
          "content":"describe(text:now I will set_params(params:[{id:glow,value:9}]))","tool_calls":[]}}]}"#;
        let reply = parse_reply(body);
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].name, "describe");
        assert!(matches!(reply.tool_calls[0].action, Some(AgentAction::Describe(_))));
    }

    #[test]
    fn parse_dsl_rejects_tool_name_as_substring() {
        // `set_params` inside `reset_params(` must not match (Bugbot: no word boundary).
        let body = r#"{"choices":[{"message":{"role":"assistant",
          "content":"reset_params(params:[{id:glow,value:1}])","tool_calls":[]}}]}"#;
        let reply = parse_reply(body);
        assert!(reply.tool_calls.is_empty());
    }

    #[test]
    fn parse_dsl_gives_repeated_tools_unique_ids() {
        // Two set_params in one reply must get distinct ids (Bugbot/VADE: duplicate
        // tool_call_id breaks the multi-turn round-trip).
        let body = r#"{"choices":[{"message":{"role":"assistant",
          "content":"set_params(params:[{id:glow,value:1}]) then set_params(params:[{id:metallic,value:1}])","tool_calls":[]}}]}"#;
        let reply = parse_reply(body);
        assert_eq!(reply.tool_calls.len(), 2);
        assert_ne!(reply.tool_calls[0].id, reply.tool_calls[1].id);
    }

    #[test]
    fn apply_op_round_trips_through_the_wire_format() {
        let ops = [
            ApplyOp::Set("metallic".into(), 1.0),
            ApplyOp::Generator(2),
            ApplyOp::Surface(2),
            ApplyOp::Material(2),
            ApplyOp::Release,
        ];
        for op in ops {
            assert_eq!(ApplyOp::parse(&op.to_line()), Some(op.clone()), "{op:?}");
        }
        assert_eq!(ApplyOp::parse("nonsense"), None);
        assert_eq!(ApplyOp::parse("set glow"), None); // missing value
    }

    #[test]
    fn cli_ops_roundtrip_the_wire_format() {
        let cases = [
            CliOp::Set("metallic".into(), 0.85),
            CliOp::Generator(3),
            CliOp::Surface(9),
            CliOp::Material(1),
            CliOp::Release(None),
            CliOp::Release(Some("glow".into())),
            CliOp::Plan(r#"{"intent":"x","moves":[]}"#.into()),
        ];
        for op in cases {
            assert_eq!(CliOp::parse(&op.to_line()), Some(op.clone()), "{op:?}");
        }
        // Malformed / unknown lines skip cleanly (forward compatibility).
        assert_eq!(CliOp::parse(""), None);
        assert_eq!(CliOp::parse("set metallic"), None);
        assert_eq!(CliOp::parse("set metallic nope"), None);
        assert_eq!(CliOp::parse("snap out.png"), None);
        // A plan line keeps its embedded spaces.
        let p = CliOp::parse(r#"plan {"intent":"a b","moves":[]}"#).unwrap();
        assert_eq!(p, CliOp::Plan(r#"{"intent":"a b","moves":[]}"#.into()));
    }

    #[test]
    fn cli_ops_map_to_the_performer_action_set() {
        assert_eq!(
            CliOp::Set("glow".into(), 1.5).into_action(),
            Some(AgentAction::SetParams(vec![("glow".into(), 1.5)]))
        );
        assert_eq!(CliOp::Generator(4).into_action(), Some(AgentAction::SelectGenerator(4)));
        assert_eq!(CliOp::Surface(2).into_action(), Some(AgentAction::SelectSurface(2)));
        assert_eq!(CliOp::Material(2).into_action(), Some(AgentAction::SelectMaterial(2)));
        // Release is handled directly on the lane, not via dispatch.
        assert_eq!(CliOp::Release(None).into_action(), None);
        // A valid plan becomes the executor action; garbage does not.
        let plan = r#"{"name":"warm","moves":[{"op":"set_param","id":"glow","value":1.0}]}"#;
        assert_eq!(
            CliOp::Plan(plan.into()).into_action(),
            Some(AgentAction::SetParams(vec![("glow".into(), 1.0)]))
        );
        assert_eq!(CliOp::Plan("not json".into()).into_action(), None);
    }

    #[test]
    fn cli_drain_step_survives_failed_reads_and_only_takes_new_lines() {
        // Unchanged length → nothing to do.
        assert_eq!(cli_drain_step(10, 10, Some("set glow 1\n"), 1), None);
        // Length changed but the read FAILED → state untouched, retry next frame.
        assert_eq!(cli_drain_step(10, 20, None, 1), None);
        // Growth: only the lines past the cursor come back; state advances.
        let (lines, len, cur) =
            cli_drain_step(10, 30, Some("set glow 1\nset metallic 0.5\n"), 1).unwrap();
        assert_eq!(lines, vec!["set metallic 0.5".to_string()]);
        assert_eq!((len, cur), (30, 2));
        // Shrink (truncated/rotated file) → the fresh content replays from 0.
        let (lines, len, cur) = cli_drain_step(30, 8, Some("release\n"), 2).unwrap();
        assert_eq!(lines, vec!["release".to_string()]);
        assert_eq!((len, cur), (8, 1));
        // A vanished file drains to empty cleanly.
        assert_eq!(cli_drain_step(30, 0, Some(""), 2), Some((vec![], 0, 0)));
        // Seeding counts only non-empty lines.
        assert_eq!(cli_seed(""), 0);
        assert_eq!(cli_seed("set glow 1\n\n  \nset metallic 0.5\n"), 2);
    }

    #[test]
    fn release_one_drops_only_that_hold() {
        let mut lane = AgentLane::new();
        lane.set("metallic", 0.9);
        lane.set("glow", 1.2);
        lane.release_one("metallic");
        assert_eq!(lane.held_ids(), vec!["glow"]);
        // Unknown id is a no-op.
        lane.release_one("nope");
        assert_eq!(lane.held_ids(), vec!["glow"]);
    }

    #[test]
    fn actuatable_ids_is_exactly_the_id_range_set() {
        // Every listed id has a range + a `current` read route…
        let s = Shared::default();
        for id in ACTUATABLE_IDS {
            assert!(id_range(id).is_some(), "{id} listed but has no range");
            assert!(current(&s, id).is_some(), "{id} listed but has no read route");
        }
        // …no duplicates…
        let set: std::collections::BTreeSet<_> = ACTUATABLE_IDS.iter().collect();
        assert_eq!(set.len(), ACTUATABLE_IDS.len());
        // …and every catalog id with a range is listed (the reverse direction:
        // a new actuation route must be added to the list to stay CLI-visible).
        for c in core_catalog() {
            if id_range(c.id).is_some() {
                assert!(ACTUATABLE_IDS.contains(&c.id), "{} has a range but is not in ACTUATABLE_IDS", c.id);
            }
        }
    }

    #[test]
    fn every_actuatable_id_has_a_gloss() {
        // #452 "describe surface" Layer 2: a new actuatable param must ship with a one-line
        // gloss (the same source-of-truth discipline as the enum descs), and no gloss is empty.
        for id in ACTUATABLE_IDS {
            let d = param_desc(id).unwrap_or_else(|| panic!("{id} has no param_desc gloss"));
            assert!(!d.trim().is_empty(), "{id} has an empty gloss");
        }
        assert!(param_desc("not_a_real_param").is_none());
    }

    #[test]
    fn apply_ops_forwards_only_actuatable_params_and_selectors() {
        // Actuatable params + selectors cross the channel; unknown ids and non-actuating
        // actions (read_state, describe, presets) do not.
        let ops = apply_ops(&AgentAction::SetParams(vec![
            ("metallic".into(), 1.0),
            ("not_a_param".into(), 5.0),
        ]));
        assert_eq!(ops, vec![ApplyOp::Set("metallic".into(), 1.0)]);
        assert_eq!(apply_ops(&AgentAction::SelectSurface(2)), vec![ApplyOp::Surface(2)]);
        assert!(apply_ops(&AgentAction::ReadState).is_empty());
        assert!(apply_ops(&AgentAction::Describe("hi".into())).is_empty());
    }

    #[test]
    fn new_levers_are_actuatable_and_round_trip() {
        // #317 levers: cam_path (spin), mat_hue (colour), bell_physical (jellyfish bell) are
        // forwarded to the editor AND have a Shared route (lane fallback) that round-trips.
        for (id, v) in [("cam_path", 4.0), ("mat_hue", 0.6), ("bell_physical", 1.0)] {
            assert!(is_actuatable(id), "{id} must be actuatable");
            let mut s = Shared::default();
            assert!(actuate(&mut s, id, v), "{id} must have a Shared route");
            assert_eq!(current(&s, id), Some(v), "{id} must read back what was written");
            assert_eq!(apply_ops(&AgentAction::SetParams(vec![(id.into(), v)])),
                       vec![ApplyOp::Set(id.into(), v)]);
        }
    }

    #[test]
    fn apply_drain_plan_applies_the_first_action_after_seeding() {
        // Editor opens before any action → file empty (n=0): seed to 0, apply nothing.
        let (r, seeded, cursor) = apply_drain_plan(0, false, 0);
        assert_eq!(r, 0..0);
        assert!(seeded);
        assert_eq!(cursor, 0);
        // First action creates the file with 3 lines → they must ALL apply (not be skipped).
        let (r, _, cursor) = apply_drain_plan(3, seeded, cursor);
        assert_eq!(r, 0..3);
        assert_eq!(cursor, 3);
        // A later action appends 2 more → only the new lines apply (last-touched-wins).
        let (r, _, cursor) = apply_drain_plan(5, true, 3);
        assert_eq!(r, 3..5);
        assert_eq!(cursor, 5);
    }

    #[test]
    fn apply_drain_plan_seeds_past_a_stale_prior_session() {
        // A stale file already has 4 lines at editor-open: seed past them (apply nothing),
        // then only genuinely new lines apply.
        let (r, seeded, cursor) = apply_drain_plan(4, false, 0);
        assert_eq!(r, 0..0);
        assert_eq!((seeded, cursor), (true, 4));
        let (r, _, cursor) = apply_drain_plan(6, seeded, cursor);
        assert_eq!(r, 4..6);
        assert_eq!(cursor, 6);
    }

    #[test]
    fn apply_drain_plan_reapplies_fresh_set_after_a_shrink() {
        // The visual restarted and truncated the file to 2 lines while cursor was 6 — those
        // 2 fresh lines must apply from the start, not be dropped.
        let (r, _, cursor) = apply_drain_plan(2, true, 6);
        assert_eq!(r, 0..2);
        assert_eq!(cursor, 2);
    }

    #[test]
    fn parse_reply_falls_back_to_dsl_when_tool_calls_absent_entirely() {
        // Some servers omit the `tool_calls` key altogether (not just an empty array).
        let body = r#"{"choices":[{"message":{"role":"assistant",
          "content":"read_state()"}}]}"#;
        let reply = parse_reply(body);
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].action, Some(AgentAction::ReadState));
    }

    #[test]
    fn live_state_snapshots_state_and_feedback() {
        let s = Shared::default();
        let ls = agent_live_from(&s).with_perf(60.0, 8.0, 4.0, 1234);
        let sj = ls.state_json();
        assert!(sj.contains("\"generator\""));
        assert!(sj.contains("\"glow\""));
        let fj = ls.feedback_json();
        assert!(fj.contains("\"instances\":1234"));
        assert!(fj.contains("\"fps\":60"));
    }

    fn agent_live_from(s: &Shared) -> LiveState {
        LiveState::from_shared(s)
    }
}
