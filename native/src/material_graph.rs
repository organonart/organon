//! #472 Tier 4 — the declarative `material.json` graph.
//!
//! A human- and agent-writable JSON description of the procedural material (the
//! #472 Tier 2/3 noise layers + derived maps). It is an **interchange + gallery**
//! format on top of the already-preset-captured material params: loading a graph
//! *applies it to the plugin params* (via the host `ParamSetter`, GUI-thread), so it
//! flows through the existing params → `Shared` → compute-bake path and is captured
//! by presets automatically. Saving serialises the current params back out.
//!
//! The schema uses **string enum names** (`"fbm"`, `"multiply"`, `"albedo"`) so an
//! agent (or a person) can author it by hand. All fields have defaults, so a partial
//! graph is valid — an agent can write just the layers it cares about. The `layers`
//! array is variable-length and forward-compatible: today the engine consumes the
//! first two (Tier 3's base + overlay); later tiers consume more.

use crate::params::{
    AnimMode, BakeRes, BlendMode, MatChannel, MatNoise, MatProjection, OrganicMathParams,
};
use nih_plug::prelude::*;
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
    /// Serialise the current material params into a graph (both active layers).
    pub fn from_params(p: &OrganicMathParams) -> Self {
        let base = GraphLayer {
            enabled: true,
            noise: p.mp_noise.value().as_str().into(),
            channel: p.mp_channel.value().as_str().into(),
            blend: "normal".into(), // base layer is always the replace/base
            scale: p.mp_scale.value(),
            rotation: p.mp_rotation.value(),
            offset: [p.mp_offset_x.value(), p.mp_offset_y.value()],
            octaves: p.mp_octaves.value(),
            lacunarity: p.mp_lacunarity.value(),
            gain: p.mp_gain.value(),
            warp: p.mp_warp.value(),
            contrast: p.mp_contrast.value(),
            gamma: p.mp_gamma.value(),
            remap: [p.mp_remap_lo.value(), p.mp_remap_hi.value()],
            invert: p.mp_invert.value(),
            seed: p.mp_seed.value(),
            gradient_low: [p.mp_lo_r.value(), p.mp_lo_g.value(), p.mp_lo_b.value()],
            gradient_high: [p.mp_hi_r.value(), p.mp_hi_g.value(), p.mp_hi_b.value()],
        };
        let overlay = GraphLayer {
            enabled: p.mp2_enable.value(),
            noise: p.mp2_noise.value().as_str().into(),
            channel: p.mp2_channel.value().as_str().into(),
            blend: p.mp2_blend.value().as_str().into(),
            scale: p.mp2_scale.value(),
            rotation: p.mp2_rotation.value(),
            offset: [p.mp2_offset_x.value(), p.mp2_offset_y.value()],
            octaves: p.mp2_octaves.value(),
            lacunarity: p.mp2_lacunarity.value(),
            gain: p.mp2_gain.value(),
            warp: p.mp2_warp.value(),
            contrast: p.mp2_contrast.value(),
            gamma: p.mp2_gamma.value(),
            remap: [p.mp2_remap_lo.value(), p.mp2_remap_hi.value()],
            invert: p.mp2_invert.value(),
            seed: p.mp2_seed.value(),
            gradient_low: [p.mp2_lo_r.value(), p.mp2_lo_g.value(), p.mp2_lo_b.value()],
            gradient_high: [p.mp2_hi_r.value(), p.mp2_hi_g.value(), p.mp2_hi_b.value()],
        };
        MaterialGraph {
            version: GRAPH_VERSION,
            name: "material".into(),
            projection: p.mat_projection.value().as_str().into(),
            scale: p.mat_scale.value(),
            bake_res: p.mp_res.value().px() as u32,
            layers: vec![base, overlay],
            derive: GraphDerive {
                normal: p.mat_derive_normal.value(),
                ao: p.mat_derive_ao.value(),
                normal_from_albedo: p.mat_normal_source_albedo.value(),
                normal_strength: p.mat_derive_normal_strength.value(),
                ao_strength: p.mat_derive_ao_strength.value(),
                ao_radius: p.mat_derive_ao_radius.value(),
            },
            animation: GraphAnimation {
                enabled: p.mat_anim_enable.value(),
                mode: p.mat_anim_mode.value().as_str().into(),
                speed: p.mat_anim_speed.value(),
                flow: [p.mat_flow_x.value(), p.mat_flow_y.value()],
                displace: p.mat_displace.value(),
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

    /// Apply the graph to the plugin params via the host setter (GUI thread). Turns
    /// the material system + procedural bake on, so a loaded graph renders
    /// immediately. Missing layers disable the corresponding slots.
    pub fn apply(&self, p: &OrganicMathParams, s: &ParamSetter) {
        macro_rules! set {
            ($param:expr, $val:expr) => {{
                s.begin_set_parameter($param);
                s.set_parameter($param, $val);
                s.end_set_parameter($param);
            }};
        }
        // Turn materials + procedural on.
        set!(&p.mat_enable, true);
        set!(&p.mp_enable, true);
        set!(&p.mat_projection, MatProjection::from_str_or(&self.projection, MatProjection::Triplanar));
        set!(&p.mat_scale, self.scale);
        set!(&p.mp_res, bake_res_of(self.bake_res));

        // Base layer (layer 0).
        if let Some(l) = self.layers.first() {
            set!(&p.mp_noise, MatNoise::from_str_or(&l.noise, MatNoise::Fbm));
            set!(&p.mp_channel, MatChannel::from_str_or(&l.channel, MatChannel::Albedo));
            set!(&p.mp_scale, l.scale);
            set!(&p.mp_rotation, l.rotation);
            set!(&p.mp_offset_x, l.offset[0]);
            set!(&p.mp_offset_y, l.offset[1]);
            set!(&p.mp_octaves, l.octaves);
            set!(&p.mp_lacunarity, l.lacunarity);
            set!(&p.mp_gain, l.gain);
            set!(&p.mp_warp, l.warp);
            set!(&p.mp_contrast, l.contrast);
            set!(&p.mp_gamma, l.gamma);
            set!(&p.mp_remap_lo, l.remap[0]);
            set!(&p.mp_remap_hi, l.remap[1]);
            set!(&p.mp_invert, l.invert);
            set!(&p.mp_seed, l.seed);
            set!(&p.mp_lo_r, l.gradient_low[0]);
            set!(&p.mp_lo_g, l.gradient_low[1]);
            set!(&p.mp_lo_b, l.gradient_low[2]);
            set!(&p.mp_hi_r, l.gradient_high[0]);
            set!(&p.mp_hi_g, l.gradient_high[1]);
            set!(&p.mp_hi_b, l.gradient_high[2]);
        }

        // Overlay layer (layer 1) — enable only if present & flagged.
        if let Some(l) = self.layers.get(1) {
            set!(&p.mp2_enable, l.enabled);
            set!(&p.mp2_noise, MatNoise::from_str_or(&l.noise, MatNoise::Fbm));
            set!(&p.mp2_channel, MatChannel::from_str_or(&l.channel, MatChannel::Roughness));
            set!(&p.mp2_blend, BlendMode::from_str_or(&l.blend, BlendMode::Normal));
            set!(&p.mp2_scale, l.scale);
            set!(&p.mp2_rotation, l.rotation);
            set!(&p.mp2_offset_x, l.offset[0]);
            set!(&p.mp2_offset_y, l.offset[1]);
            set!(&p.mp2_octaves, l.octaves);
            set!(&p.mp2_lacunarity, l.lacunarity);
            set!(&p.mp2_gain, l.gain);
            set!(&p.mp2_warp, l.warp);
            set!(&p.mp2_contrast, l.contrast);
            set!(&p.mp2_gamma, l.gamma);
            set!(&p.mp2_remap_lo, l.remap[0]);
            set!(&p.mp2_remap_hi, l.remap[1]);
            set!(&p.mp2_invert, l.invert);
            set!(&p.mp2_seed, l.seed);
            set!(&p.mp2_lo_r, l.gradient_low[0]);
            set!(&p.mp2_lo_g, l.gradient_low[1]);
            set!(&p.mp2_lo_b, l.gradient_low[2]);
            set!(&p.mp2_hi_r, l.gradient_high[0]);
            set!(&p.mp2_hi_g, l.gradient_high[1]);
            set!(&p.mp2_hi_b, l.gradient_high[2]);
        } else {
            set!(&p.mp2_enable, false);
        }

        // Derived maps.
        set!(&p.mat_derive_normal, self.derive.normal);
        set!(&p.mat_derive_ao, self.derive.ao);
        set!(&p.mat_normal_source_albedo, self.derive.normal_from_albedo);
        set!(&p.mat_derive_normal_strength, self.derive.normal_strength);
        set!(&p.mat_derive_ao_strength, self.derive.ao_strength);
        set!(&p.mat_derive_ao_radius, self.derive.ao_radius);

        // Animation + displacement (#472 Tier 5).
        set!(&p.mat_anim_enable, self.animation.enabled);
        set!(&p.mat_anim_mode, AnimMode::from_str_or(&self.animation.mode, AnimMode::Drift));
        set!(&p.mat_anim_speed, self.animation.speed);
        set!(&p.mat_flow_x, self.animation.flow[0]);
        set!(&p.mat_flow_y, self.animation.flow[1]);
        set!(&p.mat_displace, self.animation.displace);
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
