//! # organon-world — the window layer, below the plugin
//!
//! **organon#49 Tier 4b + 4c-ii.** The world and everything around it that needed a crate
//! carrying `egui`, `wgpu` and `winit` together: viewport camera input, the egui platform
//! seam, the two IPC rings, `World` itself, and the separate-process visual binary.
//!
//! ## The boundary
//!
//! **No `nih_plug`** — `cargo tree -p organon-world` is the acceptance test, the same one
//! every other engine crate holds. That is the *only* prohibition here: this crate
//! deliberately carries `egui`, `wgpu` and `winit`. It exists precisely because the crates
//! below it forbid those, and the plugin crate above it forbids nothing and is therefore
//! the wrong home for anything reusable.
//!
//! ## What is here
//!
//! ### Always compiled (T4b)
//!
//! | Module | What it owns |
//! |---|---|
//! | [`scene_input`] | the viewport's camera input — egui gestures → `CameraInput` |
//! | [`egui_platform`] | the egui platform seam: input and geometry, without naming a window |
//! | [`frame_ring`] | the frame mmap ring the visual publishes to |
//! | [`audio_ring`] | the audio mmap ring the plugin publishes to |
//!
//! ### Behind the `world` feature (T4c-ii)
//!
//! [`world`] — 13.5k lines — plus the nine submodules it declares by `#[path]` (`capture`,
//! `overlay`, `rt`, `metal_island`, `gpu_timer`, `recorder`, `snap`, `ui_layer`,
//! `winit_platform`) and the `organic-math-visual` binary with its three platform modules
//! (`hdr_macos`, `hdr_windows`, `launch_macos`).
//!
//! ## 🚨 Why `world` is a feature, and why the binary came with it
//!
//! **The feature protects a measured number.** `native/src/lib.rs` used to declare
//! `pub mod world` behind `#[cfg(any(mind-edition, console-edition))]`, because ungated it
//! grew the shipping plugin cdylib by **+490 KB** (12 749 728 → 13 250 704 bytes). This
//! crate is an *unconditional* dependency of `organic-math-native`, so an always-public
//! `world` here would put those bytes straight back. The gate did not change in T4c-ii —
//! only which manifest states it.
//!
//! **The visual binary had to move too, and that is the non-obvious part.**
//! `bin/visual.rs` never used the library's `world` module: it `#[path]`-*included*
//! `world.rs`'s source into itself, compiling the same 13.5k lines a second time,
//! precisely because a `#[path]` include is not a cargo feature and could therefore give
//! the binary a world the cdylib did not get. Moving `world.rs` leaves that include
//! pointing at nothing, and the obvious repair fails: **cargo features unify across every
//! target of a package**, so a visual that stayed in `organic-math-native` and asked for
//! `organon-world/world` would hand the cdylib in that same package the same +490 KB.
//!
//! 📌 **So the dual compilation is gone, not relocated.** The world is compiled once now.
//! T4b's header predicted this move would "collapse a real duality" and asked for a
//! before/after binary-size measurement; the PR carries it.
//!
//! ## ⚠️ What must not drift in
//!
//! **`agent` and `cli` are not here.** `world.rs` no longer names either — T4c-i moved the
//! Performer to `organon-agent` and gave `World::new` a catalog parameter, which is what
//! kept `param_table` out of this crate — but they carry the plugin's automation surface
//! and Tier 5 has to answer them on their own terms rather than by widening this crate.
//! The membership rule is unchanged and is the one `organon-scene` was drawn by: a module
//! comes only if its shipped code names nothing above it.

pub mod audio_ring;
pub mod egui_platform;
pub mod frame_ring;
pub mod scene_input;

/// organon#49 T4c-ii — the world: the scene, the beat clock, the camera, `frame_body`, and
/// the nine `#[path]` submodules it declares.
///
/// ⚠️ **Gated, and `Cargo.toml`'s `world` feature carries the reason** — this crate is an
/// unconditional dependency of the crate that exports the VST3, and an ungated world puts a
/// measured +490 KB back into the shipping cdylib. `organic-math-native` turns the feature
/// on under `mind-edition` / `console-edition`, which is exactly where it used to declare
/// this module itself.
#[cfg(feature = "world")]
pub mod world;
