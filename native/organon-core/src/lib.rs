//! # organon-core — the engine's pure spine
//!
//! **#626 Tier 3**, specified as #536 Tier 4. The first crate boundary drawn in this
//! codebase that the *compiler* enforces rather than module syntax suggesting.
//!
//! ## What belongs here
//!
//! Code with **no host, no GPU, and no UI toolkit** — nothing that pulls `nih_plug`,
//! `wgpu`, or `egui`. `Cargo.toml`'s `[dependencies]` is empty and
//! `cargo tree -p organon-core` is the tier's acceptance test; both stop meaning
//! anything the moment a dependency is added casually. See `Cargo.toml`'s header.
//!
//! ## What is here now, and why each one
//!
//! | Module | Why it moved |
//! |---|---|
//! | [`gguf`] | pure `std` file parsing. `math.rs` needs it (#536 T4 reference #1). |
//! | [`gguf_data`] | tensor payload reads; depends only on [`gguf`]. |
//! | [`edition`] | resolves #536 T4 **reference #4** — `ipc.rs → crate::edition`, ×6. |
//! | [`ipc`] | **T4** — `Shared`, the plugin↔visual wire format. See the ⚠️ below. |
//! | [`kind`] | #48 T1 — the console's **kind** vocabulary. It is here because the two front-ends that had a copy each live in *different* crates (`cli.rs` in the root crate, `conversation.rs` in `organon-console`), so this is the only crate both can see — and a closed set of words needs no host, GPU or UI. |
//! | [`math`] | **PR B** — the algorithm itself (31.5k lines). Pure; no host, no GPU. |
//! | [`params`] | **PR B** — `ParamValues` + the semantic enums, #536 T4 reference #2; plus `IndexedEnum` (organon#49 T2), core's counterpart to nih-plug's `Enum`. |
//! | [`tabs`] | resolves #536 T4 **reference #5** — `edition.rs → preset::UiTab`. |
//!
//! ⚠️ **`gguf`/`gguf_data` are here provisionally.** #536 T4 reference #1 —
//! `math.rs → crate::gguf`, ×14 — is deferred to **Tier 4**, which lifts the lens
//! builders out of `math.rs` into `organon-mind` and resolves the direction properly.
//! Moving them to core now is the cheaper of two orders: it avoids fighting the same
//! cycle twice. When Tier 4 runs, expect them to move *again*, out of core and into
//! `organon-mind`. That is the plan, not a regression.
//!
//! ## What deliberately did NOT move
//!
//! `preset.rs` keeps its `ParamSetter` logic (it is the host-automation path and is
//! nih-plug's by nature), and only the two tab *taxonomy* enums were lifted out of it.
//!
//! **`params.rs` keeps ~88 of its ~102 `#[derive(Enum)]` types.** Fourteen have a
//! counterpart here, and each is a *split* rather than a move: core owns the plain
//! semantic enum, `params.rs` owns a `Host*` mirror carrying nih-plug's derive, because
//! the **orphan rule** forbids the native crate from implementing a foreign trait for a
//! foreign type. See [`params`], and `ARCHITECTURE.md` §7 for the contract.
//!
//! | Moved in | Types | Because |
//! |---|---|---|
//! | #626 T3 | `FuncName` | `math.rs` needs it |
//! | organon#49 T1 | `GeneratorMode`, `BoidsForm`, `OscDivision` | what `world.rs` reaches for |
//! | organon#49 T2 | `SurfaceMode`, `MaterialType`, `CamPath`, `Palette` | what `cli.rs` + `agent.rs` reach for |
//! | organon#49 T4a | `FdtdSource`, `FieldVolSource`, `ColourMode`, `CalColourSource`, `FieldKind`, `FluxAxis` | the rest of what `world.rs` reaches for |
//!
//! Every wave had the same shape and a different consumer. **T2 is the one worth
//! remembering**, because its scope came from a transitive fact rather than a list:
//! `cli.rs` and `agent.rs` are on `world.rs`'s dependency path, so they must lose
//! nih-plug too — which means every enum *they* name has to be here. [`params`]'s
//! `IndexedEnum` is what replaced nih-plug's `Enum` for them.
//!
//! ⚠️ The derive count in `params.rs` is unchanged by all this — a mirror carries a derive
//! too. What changed is that fourteen of them are now adapters rather than declarations.
//!
//! 📌 **T4a closes the params half of the `world.rs` move.** `world.rs` names 26 things
//! from `crate::params`, every one a *value* type and none of them `OrganicMathParams`;
//! with these six here, all 26 resolve to core. What still holds `world.rs` upstream is
//! the set of MODULES it imports — `cli`, `agent`, `scene_input`, `frame_ring`,
//! `egui_platform` — not the parameters.
//!
//! ## ⚠️ `ipc.rs` MOVED HERE IN TIER 4 — and Tier 3's note below is now history
//!
//! Tier 3 kept `ipc.rs` in `organic-math-native` and was right to: nothing then needed
//! it from a lower crate. **Tier 4 changes that.** `organon-render` holds the visual,
//! and the visual's whole job is reading params out of `Shared` — so `Shared` has to be
//! reachable from a crate that `organon-render` can depend on. Leaving it upstream would
//! have meant `organon-render → organic-math-native → organon-render`, which cargo
//! rejects outright.
//!
//! **`Shared`'s LAYOUT did not move, which is what the invariant actually protects.**
//! The file crossed by `git mv`, byte-identical apart from one rustdoc intra-doc link
//! that no longer resolves downward. `LAYOUT_VERSION` is still `0x0285` and
//! `EXPECTED_SHARED_SIZE` still `8512`, asserted by the same goldens as before — a file
//! move is not a layout change, which is the same reasoning Tier 3 used for `math.rs`.
//!
//! `Shared` belongs here on the merits anyway: it is the contract *between* the plugin
//! and the visual, so the host-free spine both sides depend on is its natural home.
//!
//! ### Tier 3's note, kept for the record
//!
//! **`ipc.rs` did not move in Tier 3, and did not need to.** #536 T4 reference #3
//! (`math.rs → crate::ipc::Shared`) calls for co-location, but that reference is
//! **test-only** — one test, relocated to `native/tests/vecbuild_ipc.rs`. `ipc.rs` and
//! `param_table.rs` therefore came through Tier 3 at a **zero diff**, which is a far
//! stronger statement about the `Shared` layout than moving and re-verifying them.

pub mod console_ops;
pub mod edition;
pub mod exhibit;
/// #452 Tier 3 — the `snap` / `record` request+reply wire format, beside the `ipc` paths
/// it is the format of. Arrived in organon#49 T4c-i from `cli.rs`, which cannot itself
/// descend; `crate::cli` re-exports it so no caller moved.
pub mod eyes;
pub mod gguf;
pub mod ipc;
// organon#217 T1 — the glyph ring: the cell-grid channel between the `organon-glyphs`
// producer and the world. Here, not in organon-mind or organon-world, because BOTH the
// writer (a leaf crate that links ttfx) and the reader (the world) must see one
// definition, and core is the only crate both already depend on. Host-free: memmap2 +
// bytemuck, which core carries already.
pub mod glyph_ring;
pub mod gguf_data;
pub mod kind;
/// organon#217 T7 phase one — a glyph outline as an extruded, bevelled mesh: `ab_glyph`
/// outlines → flattened contours → a non-zero scanline cap → a bevel band → a side wall →
/// an LRU mesh atlas. Plain `Vec`s of vertices and indices; **no `wgpu`, no draw**, and
/// nothing in the renderer reads it yet — §14 of `doc/pbr_text_engine.md` says treating
/// T7 as a prerequisite is how this project would fail to ship.
///
/// ⚠️ **Behind the optional `letterform` feature**, so the default build does not compile
/// it and `cargo tree -p organon-core` still reports six dependencies. A green default
/// `cargo test -p organon-core` therefore says nothing about this module.
#[cfg(feature = "letterform")]
pub mod letterform;
/// #147 Tier 2 — the LoRA adapter reader. `‖ΔW‖_F` and the effective rank of the update
/// per adapted module, read from `adapter_config.json` + `adapter_model.safetensors`.
/// Pure arithmetic over a directory: no network, no `Shared`, no renderer, and `ΔW` is
/// never materialized. Here rather than in the root crate for the same reason [`gguf`]
/// is — it is file parsing with no host, no GPU and no UI, and the lens that will draw
/// it (T3) lives lower down than the crate that owns the editor.
pub mod lora;
pub mod math;
pub mod panels;
pub mod params;
pub mod tabs;
/// #147 Tier 4 — the training strip and the run shelf: the Studio's SSE progress stream, its
/// metrics backfill and its run history, as pure framing plus one worker thread. 🚨 It never
/// calls [`unsloth::StudioClient::probe`] — the health route is unauthenticated, so a green
/// probe could never mean "connected"; this module opens with an *authenticated* call so its
/// first success is real evidence. Editor-side readout: **no `Shared` field, no
/// `LAYOUT_VERSION` movement**, and no new dependency.
pub mod train;
/// #147 Tier 1 — the connection to Unsloth Studio: an endpoint, a bearer token held as a
/// secret, a `/api/health` probe, and the three refusals (*not configured* / *unreachable*
/// / *unauthorized*) that a person fixes three different ways. Beside [`lora`] because they
/// are the two halves of one tier — that module reads what a fine-tune moved, this one
/// reaches the service that catalogs them. Hand-rolled HTTP/1.1 over `std::net`, so core's
/// invariant (no `nih_plug`, `wgpu`, `egui`, `winit`) and its dependency list are both
/// untouched. 🚨 The token is read from `UNSLOTH_API_KEY` and never written anywhere by us
/// — the module doc defends that placement and names what it does not buy.
pub mod unsloth;
/// organon#49 T5a — the viewpoint's band and origin. Named for the word the code already
/// used ("the viewpoint may tip", "where the viewpoint starts"), and deliberately NOT `camera`:
/// `organon_console::camera` is a different subject (who owns the viewpoint, hand or agent) and
/// reads these constants, so two `camera` modules in one workspace would be a trap.
pub mod viewpoint;
