//! # organon-world — the window layer, below the plugin
//!
//! **organon#49 Tier 4b.** What `world.rs` reaches for that needed a crate carrying
//! `egui` and `wgpu`: viewport camera input, the egui platform seam, the two IPC rings,
//! and the frame recorder.
//!
//! ## The boundary
//!
//! **No `nih_plug`** — `cargo tree -p organon-world` is the acceptance test, the same one
//! every other engine crate holds. That is the *only* prohibition here: this crate
//! deliberately carries `egui`, and takes `wgpu` + `winit` when `world.rs` arrives in
//! T4c. It exists precisely because the crates below it forbid those, and the plugin
//! crate above it forbids nothing and is therefore the wrong home for anything reusable.
//!
//! ⚠️ **`recorder` is not here, though it looks like it belongs.** It is one of nine
//! submodules `world.rs` declares by `#[path]`, and `bin/visual.rs` includes `world.rs`
//! the same way — so `#[path = "recorder.rs"]` resolves against *the includer's*
//! directory and breaks the moment the file moves. The library build cannot see that
//! failure at all; `cargo check --lib` does not compile binaries. It travels with
//! `world.rs` in T4c. `Cargo.toml` records the full trap.
//!
//! `Cargo.toml`'s header carries the full argument for why no existing crate could host
//! this, and why it is one crate rather than two.
//!
//! ## What is here
//!
//! | Module | What it owns |
//! |---|---|
//! | [`scene_input`] | the viewport's camera input — egui gestures → `CameraInput` |
//! | [`egui_platform`] | the egui platform seam: input and geometry, without naming a window |
//! | [`frame_ring`] | the frame mmap ring the visual publishes to |
//! | [`audio_ring`] | the audio mmap ring the plugin publishes to |
//!
//! ## ⚠️ What must not drift in
//!
//! **`agent` and `cli` are not here**, though `world.rs` imports both. They carry
//! `param_table` and `preset` — the plugin's own automation surface — and Tier 4c has to
//! answer them on their own terms rather than by widening this crate. The five modules
//! above were the ones whose shipped code names nothing above them; that is the whole
//! membership rule, and it is the same one `organon-scene` was drawn by.
//!
//! **`world.rs` itself arrives in T4c**, with its nine `#[path]` submodules. It is not
//! here yet because that move collapses a real duality — `world.rs` is compiled *twice*
//! today, once as `lib.rs`'s cfg-gated module and once through `bin/visual.rs`'s
//! `#[path]` include — and that touches the default build which ships the VST3 cdylib.
//! It deserves its own change and its own before/after binary-size measurement.

pub mod audio_ring;
pub mod egui_platform;
pub mod frame_ring;
pub mod scene_input;
