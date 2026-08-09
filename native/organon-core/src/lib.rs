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
//! | [`math`] | **PR B** — the algorithm itself (31.5k lines). Pure; no host, no GPU. |
//! | [`params`] | **PR B** — `FuncName` + `ParamValues`, #536 T4 reference #2. |
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
//! **`params.rs` keeps ~101 of its ~102 `#[derive(Enum)]` types** — including all 27
//! variants of `GeneratorMode`. Only `FuncName` has a counterpart here, and even that is
//! a *split* rather than a move: core owns the plain semantic enum, `params.rs` owns
//! `HostFuncName` carrying nih-plug's derive, because the **orphan rule** forbids the
//! native crate from implementing a foreign trait for a foreign type. See [`params`].
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

pub mod edition;
pub mod gguf;
pub mod ipc;
pub mod gguf_data;
pub mod math;
pub mod params;
pub mod tabs;
