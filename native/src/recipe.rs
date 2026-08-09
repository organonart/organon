//! #452 Layer 3 — the **recipe library**: named, described starting-points an agent (or a
//! human) can apply with one command to make something beautiful *without any saved presets*.
//!
//! A recipe is **built-in knowledge, not saved state** — pure compile-time data. That is the
//! whole point: a freshly-launched Organon with an empty preset store can still be driven to a
//! finished look, because the recipes ship in the binary. Applying one emits the same
//! `generator` / `surface` / `material` selects + `set`s the CLI already speaks (through the
//! #317 override lane), so it is a *launch pad* the agent then tweaks — not a preset recall
//! (which is GUI-thread today, #354). Once a look is good, the human saves it as a preset.
//!
//! Pure + unit-tested: `recipes_are_valid` resolves every recipe's generator/surface/material
//! name and checks every param id + value against `agent::id_range`, so a recipe cannot ship
//! referencing a bad param or an unresolvable selector.

/// One recipe: a named look with an intent, the three selectors (by resolvable name), and the
/// key params to set. `None` selectors leave that dimension untouched.
#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    /// Lowercase slug used on the command line (`organon recipe <name>`).
    pub name: &'static str,
    /// Human title.
    pub title: &'static str,
    /// One line: what it is / when to reach for it.
    pub intent: &'static str,
    /// Generator to select (a name/substring `resolve_enum` accepts), or `None`.
    pub generator: Option<&'static str>,
    /// Surface mode to select, or `None`.
    pub surface: Option<&'static str>,
    /// Material to select, or `None`.
    pub material: Option<&'static str>,
    /// Key params to set (raw units); every id must be in `agent::ACTUATABLE_IDS`.
    pub params: &'static [(&'static str, f32)],
}

/// The built-in recipe library. Curated to span the moods the generators cover — reach here
/// first, then tweak.
pub fn recipes() -> &'static [Recipe] {
    &RECIPES
}

/// Look up a recipe by its exact slug.
pub fn recipe(name: &str) -> Option<&'static Recipe> {
    let n = name.trim().to_lowercase();
    RECIPES.iter().find(|r| r.name == n)
}

static RECIPES: &[Recipe] = &[
    Recipe {
        name: "helix",
        title: "Glass Helix",
        intent: "A glassy rope coiling through space — reach for spring / coil / helix motion. \
                 Frenet strands swept into glass tubes, drifting on a slow spiral orbit.",
        generator: Some("frenet"),
        surface: Some("swept"),
        material: Some("glass"),
        params: &[
            ("ior", 1.45),
            ("roughness", 0.08),
            ("metallic", 0.0),
            ("glow", 0.15),
            ("bloom_intensity", 0.25),
            ("cam_path", 4.0),
            ("cam_speed", 0.15),
            ("cam_kick", 0.3),
            ("cam_damping", 0.6),
        ],
    },
    Recipe {
        name: "jellyfish",
        title: "Pulsing Jellyfish",
        intent: "A soft-body medusa bell that breathes on the beat — organic, translucent, \
                 alive. Spherical-harmonics with the physical bell on and a waxy subsurface skin.",
        generator: Some("spherical"),
        surface: Some("original"),
        material: Some("subsurface"),
        params: &[
            ("bell_physical", 1.0),
            ("subsurface", 0.85),
            ("sss_distortion", 0.4),
            ("sss_power", 2.5),
            ("glow", 0.25),
            ("mat_hue", 0.55),
            ("opacity", 0.9),
            ("cam_path", 1.0),
            ("cam_speed", 0.1),
        ],
    },
    Recipe {
        name: "lattice",
        title: "Chrome Lattice",
        intent: "A clean, still cube-of-cubes in polished chrome — architectural and crisp. The \
                 seed geometry at rest (all amplifiers zero), mirror-metal, slow horizontal orbit.",
        generator: Some("cube field"),
        surface: Some("original"),
        material: Some("chrome"),
        params: &[
            ("loop_count_x", 5.0),
            ("loop_count_y", 5.0),
            ("loop_count_z", 5.0),
            ("rot_amp_x", 0.0),
            ("rot_amp_y", 0.0),
            ("rot_amp_z", 0.0),
            ("trans_amp_x", 0.0),
            ("trans_amp_y", 0.0),
            ("trans_amp_z", 0.0),
            ("scale_amp", 0.0),
            ("metallic", 1.0),
            ("roughness", 0.05),
            ("env_intensity", 1.5),
            ("cam_path", 1.0),
            ("cam_speed", 0.12),
        ],
    },
    Recipe {
        name: "chaos",
        title: "Strange-Attractor Smoke",
        intent: "Smoky, orbiting, never-repeating filaments from a strange attractor — turbulence \
                 and flow. Swept tubes with a warm glow and a spiralling camera.",
        generator: Some("strange"),
        surface: Some("swept"),
        material: Some("standard"),
        params: &[
            ("glow", 0.4),
            ("metallic", 0.2),
            ("roughness", 0.4),
            ("bloom_intensity", 0.4),
            ("bloom_threshold", 1.0),
            ("mat_hue", 0.05),
            ("cam_path", 4.0),
            ("cam_speed", 0.2),
            ("cam_kick", 0.4),
            ("cam_damping", 0.5),
        ],
    },
    Recipe {
        name: "dna",
        title: "Refracting DNA",
        intent: "A literal double helix in refracting glass — biology, genetics, molecular themes. \
                 Antiparallel backbones swept into tinted glass tubes on a vertical orbit.",
        generator: Some("dna"),
        surface: Some("swept"),
        material: Some("refractive"),
        params: &[
            ("ior", 1.5),
            ("roughness", 0.1),
            ("glow", 0.15),
            ("bloom_intensity", 0.2),
            ("cam_path", 2.0),
            ("cam_speed", 0.12),
        ],
    },
    Recipe {
        name: "nebula",
        title: "Nebula Fire",
        intent: "A glowing additive point-cloud fire — nebula, flame, smoky cloud. The density-map \
                 attractor sprayed as soft Gaussian splats with heavy bloom.",
        generator: Some("density-map"),
        surface: Some("splat"),
        material: Some("standard"),
        params: &[
            ("glow", 0.6),
            ("bloom_intensity", 0.8),
            ("bloom_threshold", 0.6),
            ("exposure", 0.5),
            ("mat_hue", 0.08),
            ("cam_path", 1.0),
            ("cam_speed", 0.08),
        ],
    },
    Recipe {
        name: "ribbons",
        title: "Curl-Noise Ribbons",
        intent: "Smooth swirling ribbons of flow — ink, smoke, wind. Curl-noise streamlines as \
                 swept tubes with a cool tint and a gentle drift.",
        generator: Some("curl"),
        surface: Some("swept"),
        material: Some("standard"),
        params: &[
            ("glow", 0.3),
            ("roughness", 0.3),
            ("metallic", 0.1),
            ("bloom_intensity", 0.3),
            ("mat_hue", 0.6),
            ("cam_path", 4.0),
            ("cam_speed", 0.12),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent;
    use crate::params::{GeneratorMode, MaterialType, SurfaceMode};
    use nih_plug::prelude::Enum;

    fn resolves<E: Enum>(which: &str) -> bool {
        let vars = E::variants();
        let lw = which.to_lowercase();
        if vars.iter().any(|v| v.to_lowercase() == lw) {
            return true;
        }
        vars.iter().filter(|v| v.to_lowercase().contains(&lw)).count() == 1
    }

    #[test]
    fn recipes_are_valid() {
        assert!(!recipes().is_empty());
        let mut seen = std::collections::BTreeSet::new();
        for r in recipes() {
            assert!(seen.insert(r.name), "duplicate recipe slug {}", r.name);
            assert_eq!(r.name, r.name.to_lowercase(), "{} slug must be lowercase", r.name);
            assert!(!r.intent.trim().is_empty(), "{} has no intent", r.name);
            if let Some(g) = r.generator {
                assert!(resolves::<GeneratorMode>(g), "{}: generator '{g}' does not resolve", r.name);
            }
            if let Some(s) = r.surface {
                assert!(resolves::<SurfaceMode>(s), "{}: surface '{s}' does not resolve", r.name);
            }
            if let Some(m) = r.material {
                assert!(resolves::<MaterialType>(m), "{}: material '{m}' does not resolve", r.name);
            }
            for (id, v) in r.params {
                let range = agent::id_range(id)
                    .unwrap_or_else(|| panic!("{}: param '{id}' is not actuatable", r.name));
                assert!(
                    *v >= range.0 && *v <= range.1,
                    "{}: {id}={v} out of range {:?}",
                    r.name,
                    range
                );
            }
        }
        // Lookup works and is slug-exact.
        assert!(recipe("helix").is_some());
        assert!(recipe("HELIX").is_some()); // case-insensitive
        assert!(recipe("nope").is_none());
    }
}
