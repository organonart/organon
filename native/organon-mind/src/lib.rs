//! # organon-mind — the interpretability instrument
//!
//! **#626 Tier 4.** The activation ring, the Mind UI surface, and the shell that
//! drives a local model — lifted out of `organic-math-native` so Organon Mind is a
//! crate rather than a `cfg` flag over one.
//!
//! ## ⚠️ The dependency direction is the counterintuitive one
//!
//! **`organon-render` depends on this crate; this crate never depends on
//! `organon-render`.** The visual builds the `NeuralGraph` and draws it, so the
//! renderer is the *consumer* of Mind's data, not its provider. #536 T5 asks for this
//! to be written down explicitly because the intuitive reading — "the visualiser is
//! upstream of what it visualises" — is backwards, and a later well-meaning fix would
//! create a cycle cargo then rejects.
//!
//! ## No nih-plug, by construction
//!
//! `mind_ui.rs` and `mind_viz.rs` used `egui` — a *re-export path*, not
//! an API dependency, exactly as #536 T5 predicted. They now use `egui` directly, and
//! `cargo tree -p organon-mind` carries no `nih_plug`.

pub mod mind_console;
pub mod mind_log;
pub mod mind_ring;
pub mod mind_shell;
/// #147 Tier 4 — painting the training strip and the run shelf. The thinking (SSE framing,
/// the fold, the state machine) is `organon_core::train`; this is the egui half only.
pub mod mind_train;
pub mod mind_ui;
pub mod mind_viz;
