//! #472 Tier 4 — the declarative `material.json` graph.
//!
//! A human- and agent-writable JSON description of the procedural material (the
//! #472 Tier 2/3 noise layers + derived maps). It is an **interchange + gallery**
//! format on top of the already-preset-captured material params: loading a graph
//! *applies it to the live material*, so it flows through the existing
//! params → `Shared` → compute-bake path and is captured by presets automatically.
//! Saving serialises the current material back out.
//!
//! ⚠️ **"The live material" is whichever side is asking** (Console #7). In Organon's editor
//! that is the plugin params through the host `ParamSetter`, GUI-thread, exactly as before;
//! in Organon Console it is the `PresetValues` mirror, because a param cannot be written
//! from outside `nih_plug` at all. Both routes go through [`crate::param_sink::Sink`], and
//! this module is deliberately unable to tell them apart.
//!
//! The schema uses **string enum names** (`"fbm"`, `"multiply"`, `"albedo"`) so an
//! agent (or a person) can author it by hand. All fields have defaults, so a partial
//! graph is valid — an agent can write just the layers it cares about. The `layers`
//! array is variable-length and forward-compatible: today the engine consumes the
//! first two (Tier 3's base + overlay); later tiers consume more.

use crate::param_sink::{rd, wr, Sink};
use crate::params::{
    AnimMode, BakeRes, BlendMode, MatChannel, MatNoise, MatProjection, OrganicMathParams,
};
use serde::{Deserialize, Serialize};

/// The serialised graph format version (bump on a breaking schema change).
pub const GRAPH_VERSION: u32 = 1;

/// One noise layer in the graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphLayer {
    pub enabled: bool,
    pub noise: String,   // MatNoise name
    pub channel: String, // MatChannel name
    pub blend: String,   // BlendMode name (base layer ignores this)
    pub scale: f32,
    pub rotation: f32,
    pub offset: [f32; 2],
    pub octaves: i32,
    pub lacunarity: f32,
    pub gain: f32,
    pub warp: f32,
    pub contrast: f32,
    pub gamma: f32,
    pub remap: [f32; 2], // [low, high]
    pub invert: bool,
    pub seed: i32,
    pub gradient_low: [f32; 3],  // albedo gradient, linear RGB
    pub gradient_high: [f32; 3],
}

impl Default for GraphLayer {
    fn default() -> Self {
        GraphLayer {
            enabled: true,
            noise: "fbm".into(),
            channel: "albedo".into(),
            blend: "normal".into(),
            scale: 4.0,
            rotation: 0.0,
            offset: [0.0, 0.0],
            octaves: 5,
            lacunarity: 2.0,
            gain: 0.5,
            warp: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            remap: [0.0, 1.0],
            invert: false,
            seed: 0,
            gradient_low: [0.04, 0.04, 0.05],
            gradient_high: [0.80, 0.76, 0.70],
        }
    }
}

/// The derived-map controls (the correlation principle).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphDerive {
    pub normal: bool,
    pub ao: bool,
    pub normal_from_albedo: bool,
    pub normal_strength: f32,
    pub ao_strength: f32,
    pub ao_radius: f32,
}

impl Default for GraphDerive {
    fn default() -> Self {
        GraphDerive {
            normal: false,
            ao: false,
            normal_from_albedo: false,
            normal_strength: 1.0,
            ao_strength: 1.0,
            ao_radius: 2.0,
        }
    }
}

/// Temporal animation + height displacement (#472 Tier 5). Inert at defaults, so a
/// graph that omits this block renders as a static Tier 2/3 material.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphAnimation {
    pub enabled: bool,
    pub mode: String, // AnimMode name: "drift" | "evolve" | "rotate"
    pub speed: f32,
    pub flow: [f32; 2], // Drift pan direction (X, Y)
    pub displace: f32,  // height→vertex displacement amount (0 = shading only)
}

impl Default for GraphAnimation {
    fn default() -> Self {
        GraphAnimation {
            enabled: false,
            mode: "drift".into(),
            speed: 0.1,
            flow: [1.0, 0.0],
            displace: 0.0,
        }
    }
}

/// A whole procedural material.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialGraph {
    pub version: u32,
    pub name: String,
    pub projection: String, // MatProjection name
    pub scale: f32,         // sampling tiling (mat_scale)
    pub bake_res: u32,
    pub layers: Vec<GraphLayer>,
    pub derive: GraphDerive,
    pub animation: GraphAnimation,
}

impl Default for MaterialGraph {
    fn default() -> Self {
        MaterialGraph {
            version: GRAPH_VERSION,
            name: "material".into(),
            projection: "triplanar".into(),
            scale: 1.0,
            bake_res: 512,
            layers: vec![GraphLayer::default()],
            derive: GraphDerive::default(),
            animation: GraphAnimation::default(),
        }
    }
}

impl MaterialGraph {
    /// Serialise the current material into a graph (both active layers).
    ///
    /// ⚠️ **"Current" is whatever the sink says it is** — the live params in Organon's editor,
    /// the mirror in Organon Console. `p` supplies the parameter *metadata* either way; see
    /// [`crate::param_sink`] for why the two cannot be one handle.
    pub(crate) fn from_sink(p: &OrganicMathParams, sink: &Sink) -> Self {
        let base = GraphLayer {
            enabled: true,
            noise: rd!(sink, p, mp_noise).as_str().into(),
            channel: rd!(sink, p, mp_channel).as_str().into(),
            blend: "normal".into(), // base layer is always the replace/base
            scale: rd!(sink, p, mp_scale),
            rotation: rd!(sink, p, mp_rotation),
            offset: [rd!(sink, p, mp_offset_x), rd!(sink, p, mp_offset_y)],
            octaves: rd!(sink, p, mp_octaves),
            lacunarity: rd!(sink, p, mp_lacunarity),
            gain: rd!(sink, p, mp_gain),
            warp: rd!(sink, p, mp_warp),
            contrast: rd!(sink, p, mp_contrast),
            gamma: rd!(sink, p, mp_gamma),
            remap: [rd!(sink, p, mp_remap_lo), rd!(sink, p, mp_remap_hi)],
            invert: rd!(sink, p, mp_invert),
            seed: rd!(sink, p, mp_seed),
            gradient_low: [rd!(sink, p, mp_lo_r), rd!(sink, p, mp_lo_g), rd!(sink, p, mp_lo_b)],
            gradient_high: [rd!(sink, p, mp_hi_r), rd!(sink, p, mp_hi_g), rd!(sink, p, mp_hi_b)],
        };
        let overlay = GraphLayer {
            enabled: rd!(sink, p, mp2_enable),
            noise: rd!(sink, p, mp2_noise).as_str().into(),
            channel: rd!(sink, p, mp2_channel).as_str().into(),
            blend: rd!(sink, p, mp2_blend).as_str().into(),
            scale: rd!(sink, p, mp2_scale),
            rotation: rd!(sink, p, mp2_rotation),
            offset: [rd!(sink, p, mp2_offset_x), rd!(sink, p, mp2_offset_y)],
            octaves: rd!(sink, p, mp2_octaves),
            lacunarity: rd!(sink, p, mp2_lacunarity),
            gain: rd!(sink, p, mp2_gain),
            warp: rd!(sink, p, mp2_warp),
            contrast: rd!(sink, p, mp2_contrast),
            gamma: rd!(sink, p, mp2_gamma),
            remap: [rd!(sink, p, mp2_remap_lo), rd!(sink, p, mp2_remap_hi)],
            invert: rd!(sink, p, mp2_invert),
            seed: rd!(sink, p, mp2_seed),
            gradient_low: [rd!(sink, p, mp2_lo_r), rd!(sink, p, mp2_lo_g), rd!(sink, p, mp2_lo_b)],
            gradient_high: [rd!(sink, p, mp2_hi_r), rd!(sink, p, mp2_hi_g), rd!(sink, p, mp2_hi_b)],
        };
        MaterialGraph {
            version: GRAPH_VERSION,
            name: "material".into(),
            projection: rd!(sink, p, mat_projection).as_str().into(),
            scale: rd!(sink, p, mat_scale),
            bake_res: rd!(sink, p, mp_res).px() as u32,
            layers: vec![base, overlay],
            derive: GraphDerive {
                normal: rd!(sink, p, mat_derive_normal),
                ao: rd!(sink, p, mat_derive_ao),
                normal_from_albedo: rd!(sink, p, mat_normal_source_albedo),
                normal_strength: rd!(sink, p, mat_derive_normal_strength),
                ao_strength: rd!(sink, p, mat_derive_ao_strength),
                ao_radius: rd!(sink, p, mat_derive_ao_radius),
            },
            animation: GraphAnimation {
                enabled: rd!(sink, p, mat_anim_enable),
                mode: rd!(sink, p, mat_anim_mode).as_str().into(),
                speed: rd!(sink, p, mat_anim_speed),
                flow: [rd!(sink, p, mat_flow_x), rd!(sink, p, mat_flow_y)],
                displace: rd!(sink, p, mat_displace),
            },
        }
    }

    /// Serialise to pretty JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    /// Parse from JSON (partial graphs OK — missing fields take defaults).
    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }

    /// Apply the graph (GUI thread). Turns the material system + procedural bake on, so a
    /// loaded graph renders immediately. Missing layers disable the corresponding slots.
    ///
    /// ⚠️ **Where it lands is the sink's business.** In Organon's editor that is the host
    /// `ParamSetter`, gesture-wrapped exactly as before, so a loaded graph is still one
    /// automation-recordable edit per param; in Organon Console it is the `PresetValues`
    /// mirror. This function does not know and must not care — see [`crate::param_sink`].
    pub(crate) fn apply(&self, p: &OrganicMathParams, sink: &mut Sink) {
        // Turn materials + procedural on.
        wr!(sink, p, mat_enable, true);
        wr!(sink, p, mp_enable, true);
        wr!(sink, p, mat_projection, MatProjection::from_str_or(&self.projection, MatProjection::Triplanar));
        wr!(sink, p, mat_scale, self.scale);
        wr!(sink, p, mp_res, bake_res_of(self.bake_res));

        // Base layer (layer 0).
        if let Some(l) = self.layers.first() {
            wr!(sink, p, mp_noise, MatNoise::from_str_or(&l.noise, MatNoise::Fbm));
            wr!(sink, p, mp_channel, MatChannel::from_str_or(&l.channel, MatChannel::Albedo));
            wr!(sink, p, mp_scale, l.scale);
            wr!(sink, p, mp_rotation, l.rotation);
            wr!(sink, p, mp_offset_x, l.offset[0]);
            wr!(sink, p, mp_offset_y, l.offset[1]);
            wr!(sink, p, mp_octaves, l.octaves);
            wr!(sink, p, mp_lacunarity, l.lacunarity);
            wr!(sink, p, mp_gain, l.gain);
            wr!(sink, p, mp_warp, l.warp);
            wr!(sink, p, mp_contrast, l.contrast);
            wr!(sink, p, mp_gamma, l.gamma);
            wr!(sink, p, mp_remap_lo, l.remap[0]);
            wr!(sink, p, mp_remap_hi, l.remap[1]);
            wr!(sink, p, mp_invert, l.invert);
            wr!(sink, p, mp_seed, l.seed);
            wr!(sink, p, mp_lo_r, l.gradient_low[0]);
            wr!(sink, p, mp_lo_g, l.gradient_low[1]);
            wr!(sink, p, mp_lo_b, l.gradient_low[2]);
            wr!(sink, p, mp_hi_r, l.gradient_high[0]);
            wr!(sink, p, mp_hi_g, l.gradient_high[1]);
            wr!(sink, p, mp_hi_b, l.gradient_high[2]);
        }

        // Overlay layer (layer 1) — enable only if present & flagged.
        if let Some(l) = self.layers.get(1) {
            wr!(sink, p, mp2_enable, l.enabled);
            wr!(sink, p, mp2_noise, MatNoise::from_str_or(&l.noise, MatNoise::Fbm));
            wr!(sink, p, mp2_channel, MatChannel::from_str_or(&l.channel, MatChannel::Roughness));
            wr!(sink, p, mp2_blend, BlendMode::from_str_or(&l.blend, BlendMode::Normal));
            wr!(sink, p, mp2_scale, l.scale);
            wr!(sink, p, mp2_rotation, l.rotation);
            wr!(sink, p, mp2_offset_x, l.offset[0]);
            wr!(sink, p, mp2_offset_y, l.offset[1]);
            wr!(sink, p, mp2_octaves, l.octaves);
            wr!(sink, p, mp2_lacunarity, l.lacunarity);
            wr!(sink, p, mp2_gain, l.gain);
            wr!(sink, p, mp2_warp, l.warp);
            wr!(sink, p, mp2_contrast, l.contrast);
            wr!(sink, p, mp2_gamma, l.gamma);
            wr!(sink, p, mp2_remap_lo, l.remap[0]);
            wr!(sink, p, mp2_remap_hi, l.remap[1]);
            wr!(sink, p, mp2_invert, l.invert);
            wr!(sink, p, mp2_seed, l.seed);
            wr!(sink, p, mp2_lo_r, l.gradient_low[0]);
            wr!(sink, p, mp2_lo_g, l.gradient_low[1]);
            wr!(sink, p, mp2_lo_b, l.gradient_low[2]);
            wr!(sink, p, mp2_hi_r, l.gradient_high[0]);
            wr!(sink, p, mp2_hi_g, l.gradient_high[1]);
            wr!(sink, p, mp2_hi_b, l.gradient_high[2]);
        } else {
            wr!(sink, p, mp2_enable, false);
        }

        // Derived maps.
        wr!(sink, p, mat_derive_normal, self.derive.normal);
        wr!(sink, p, mat_derive_ao, self.derive.ao);
        wr!(sink, p, mat_normal_source_albedo, self.derive.normal_from_albedo);
        wr!(sink, p, mat_derive_normal_strength, self.derive.normal_strength);
        wr!(sink, p, mat_derive_ao_strength, self.derive.ao_strength);
        wr!(sink, p, mat_derive_ao_radius, self.derive.ao_radius);

        // Animation + displacement (#472 Tier 5).
        wr!(sink, p, mat_anim_enable, self.animation.enabled);
        wr!(sink, p, mat_anim_mode, AnimMode::from_str_or(&self.animation.mode, AnimMode::Drift));
        wr!(sink, p, mat_anim_speed, self.animation.speed);
        wr!(sink, p, mat_flow_x, self.animation.flow[0]);
        wr!(sink, p, mat_flow_y, self.animation.flow[1]);
        wr!(sink, p, mat_displace, self.animation.displace);
    }
}

/// Nearest `BakeRes` for a pixel size.
fn bake_res_of(px: u32) -> BakeRes {
    if px <= 320 {
        BakeRes::R256
    } else if px <= 768 {
        BakeRes::R512
    } else {
        BakeRes::R1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_round_trips_through_json() {
        let g = MaterialGraph::default();
        let json = g.to_json();
        let back = MaterialGraph::from_json(&json).expect("parse");
        assert_eq!(back.layers.len(), g.layers.len());
        assert_eq!(back.derive.normal_strength, g.derive.normal_strength);
    }

    #[test]
    fn partial_graph_takes_defaults() {
        // An agent-authored minimal graph: one worley height layer + derive.
        let json = r#"{
            "name": "rock",
            "layers": [ { "noise": "worley", "channel": "height", "scale": 8 } ],
            "derive": { "normal": true, "ao": true }
        }"#;
        let g = MaterialGraph::from_json(json).expect("parse");
        assert_eq!(g.layers.len(), 1);
        assert_eq!(g.layers[0].noise, "worley");
        assert_eq!(g.layers[0].channel, "height");
        assert!(g.derive.normal);
        assert!(g.derive.ao);
        // Untouched fields fell back to defaults.
        assert_eq!(g.layers[0].octaves, 5);
        assert_eq!(g.projection, "triplanar");
    }

    #[test]
    fn animation_block_round_trips_and_defaults_inert() {
        // Default graph: animation off, no displacement (a static Tier 2/3 material).
        let g = MaterialGraph::default();
        assert!(!g.animation.enabled);
        assert_eq!(g.animation.displace, 0.0);
        // An agent-authored animated graph, partially specified.
        let json = r#"{
            "layers": [ { "noise": "fbm", "channel": "height" } ],
            "animation": { "enabled": true, "mode": "evolve", "speed": 0.5, "displace": 0.3 }
        }"#;
        let g = MaterialGraph::from_json(json).expect("parse");
        assert!(g.animation.enabled);
        assert_eq!(g.animation.mode, "evolve");
        assert_eq!(g.animation.displace, 0.3);
        assert_eq!(AnimMode::from_str_or(&g.animation.mode, AnimMode::Drift), AnimMode::Evolve);
        // Unspecified flow fell back to the default.
        assert_eq!(g.animation.flow, [1.0, 0.0]);
    }

    #[test]
    fn unknown_enum_names_fall_back() {
        assert_eq!(MatNoise::from_str_or("nonsense", MatNoise::Fbm), MatNoise::Fbm);
        assert_eq!(MatNoise::from_str_or("WORLEY", MatNoise::Value), MatNoise::Worley);
        assert_eq!(BlendMode::from_str_or("mul", BlendMode::Normal), BlendMode::Multiply);
        assert_eq!(MatChannel::from_str_or("colour", MatChannel::Roughness), MatChannel::Albedo);
    }
}
