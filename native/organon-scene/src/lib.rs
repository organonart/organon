//! # organon-scene — the substrate, below the plugin
//!
//! **organon#49 Tier 3.** The scene's *state* — what the substrate looks like, which
//! material and lighting rig are on it, where the camera sits, and which look was live
//! over which stretch of scrollback — as pure data over [`organon_core::ipc::Shared`].
//!
//! ## The boundary
//!
//! No host, no GPU, no UI toolkit: **no `nih_plug`, no `wgpu`, no `egui`, no `winit`**.
//! `cargo tree -p organon-scene` is the acceptance test, the same one `organon-core`
//! carries. `Cargo.toml`'s header explains why this is a third crate rather than more
//! core (identity, not dependencies — core is the spine and the crates.io commitment)
//! and rather than `organon-render` (which is `world::render`, emphatically *not* the
//! world).
//!
//! ## Why this crate exists at all
//!
//! It is a step in #49's route to a Console that is not a GPL binary of the VST3 crate.
//! `world.rs` has to move below `organic-math-native`, and it cannot move until the
//! things it reaches for have. These five modules were the ones that could go first:
//! their shipped code names nothing above them.
//!
//! ## What is here
//!
//! | Module | What it owns |
//! |---|---|
//! | [`substrate_scene`] | the substrate LOOK as a pure function over `Shared` |
//! | [`substrate_materials`] | four materials + two lighting rigs, as deltas on that snapshot |
//! | [`substrate_camera`] | the camera rig — where the camera goes, how narrow the lens is |
//! | [`substrate_epochs`] | the epoch ledger — which look was live over which scrollback |
//! | [`overlay_meta`] | per-generator metadata for the capture overlay |
//!
//! ## ⚠️ Two things deliberately stayed behind
//!
//! **`scene_input` did not come.** It is the sibling that looks like it belongs: same
//! `substrate`-adjacent job, same Console Spike lineage. But 68 of its lines reach
//! `egui` — it translates pointer gestures into `CameraInput` — and that is a UI
//! concern. It travels with `world.rs` in Tier 4. Pulling it here would have meant an
//! `egui` dependency, and this crate's whole claim is the line above.
//!
//! **`substrate_scene`'s and `substrate_materials`'s test modules did not come**, and
//! that is not a gap to close casually. Their baseline is
//! `OrganicMathParams::default().to_shared()` — the *plugin's* default parameter set,
//! named as such in their own fixtures. `Shared::default()` is a deliberately different
//! thing (core calls it "the web app's helix defaults"), so substituting it would change
//! what those tests assert without changing whether they pass. They live in
//! `native/tests/substrate.rs` now, byte-for-byte, which is the answer #626 T3 reached
//! for the same problem in `math.rs`.

pub mod overlay_meta;
pub mod substrate_camera;
pub mod substrate_epochs;
pub mod substrate_materials;
pub mod substrate_scene;
