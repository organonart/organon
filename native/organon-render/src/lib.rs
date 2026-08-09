//! # organon-render — the renderer
//!
//! **#626 Tier 4, second half.** `Renderer`, `RenderFrame`, `RenderPath`, every pass,
//! and the 36 generator-surface submodules `render.rs` pulls in behind `#[path]` —
//! lifted out of `organic-math-native` into a crate that knows nothing about plugin
//! hosts.
//!
//! ## The module is still called `render`, on purpose
//!
//! This file declares `pub mod render` over the unmoved `render.rs`, so
//! `organon_render::render::Renderer` matches the `crate::render::Renderer` that
//! `world.rs` and its 108 call sites already write. `organic-math-native` re-exports
//! it, which is the same facade Tiers 3 and 4 used for `organon-core` and
//! `organon-mind`: the crate boundary moves, the paths do not.
//!
//! ## ⚠️ What this crate is NOT
//!
//! It is **not** "the render set" as `.claude/hooks/doc-rules.sh` lists it. That list
//! includes `world.rs`, and `world.rs` is the *app state*, not the renderer — it owns
//! the agent chat client, the CLI reply protocol, `egui_platform`, `frame_ring` and
//! `scene_input`, and `agent` in turn needs `OrganicMathParams` and `preset`, which
//! are irreducibly host-side. **Every coupling #626 attributed to "render" was
//! `world.rs`'s**, including all six wire-format enum splits it specced. None of them
//! were needed here. `world.rs` stays upstream; its extraction is blocked on #618's
//! `World` decomposition, which is separate work already in flight.
//!
//! ## The one thing it depends on
//!
//! `organon-core::math` — 23 references, and nothing else from the workspace. Tier 3
//! had already put `math` in core, so this crate cost no new splits at all. There are
//! **zero** upward references: nothing here names `world`, `params`, `agent`, `preset`,
//! `ipc` or `lib`.
//!
//! ```console
//! $ cargo tree -p organon-render
//! ✅ nih_plug: 0   ✅ egui: 0   ✅ winit: 0   ⬦ wgpu: 1, which is the point
//! ```
//!
//! ## It collapses a double compile
//!
//! `world.rs` is `#[path]`-included by **both** `lib.rs` (behind `mind-edition`) and
//! `bin/visual.rs` — a binary's `#[path]` modules are unreachable from the library it
//! depends on, so the Mind build compiled the whole thing twice. `render` used to ride
//! along on both passes. As a crate it is compiled **once** and linked into both.

// ⚠️ `axes` and `chamber` are declared HERE, beside `render`, not inside it — because
// `render.rs` reaches them as `super::axes` / `super::chamber` (10 references). They were
// `world`'s submodules for exactly that reason, and `world.rs` said so in a comment at
// their declaration. Both are self-contained: zero `crate::` and zero `super::` paths
// between them, each carrying its own shader. A sideways reference like this is what an
// upward-reference scan misses — `crate::X` finds nothing, and the compiler is what
// tells you.

/// The world-box axes gizmo and its solids. Consumed by `render` as `super::axes`.
#[path = "axes.rs"]
pub mod axes;

/// Field Chamber (#346): analyzer panels (oscilloscope + spectrum) on the box back
/// walls. Consumed by `render` as `super::chamber`.
#[path = "chamber.rs"]
pub mod chamber;

/// The renderer proper: `Renderer`, `RenderFrame`, `RenderPath`, and every pass. Pulls
/// its own 36 `#[path]` submodules (env, post, metaball, the `rt_*` family, …).
#[path = "render.rs"]
pub mod render;
