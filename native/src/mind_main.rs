//! **Organon Mind** — the standalone entry point (#483 Tier 1).
//!
//! A standalone analysis instrument for local LLMs: point it at a `.gguf`, drive a
//! prompt, and watch the model's true architecture and live internal operation render
//! in the shared visual. See `doc/organon_mind_prd.md` for the product definition and
//! `MIND_ARCHITECTURE.md` for what exists today.
//!
//! This binary is the *same* `OrganicMath` editor as `organon-standalone`; the
//! `mind-edition` cargo feature (required to build this target) is what makes it a
//! different product — `src/edition.rs` resolves `EDITION` to `Edition::Mind`, which:
//!
//! - names the window **"Organon Mind"** (`Plugin::NAME`),
//! - forks the `$TMPDIR` **IPC namespace** to `organon-mind-*`, so a Mind session and a
//!   full-Organon session can run at the same time without stomping each other, and
//! - filters the editor to the **Mind lane** (+ Settings) — no generator / motion /
//!   look authoring surface.
//!
//! Deliberately **standalone-only**: no VST3/CLAP export, therefore no new plugin class
//! ID to freeze and none of the host audio-thread constraints (the standalone owns its
//! audio device, so the #367 inference runtime simply runs).
//!
//! Two helpers pair with a Mind session and must be pointed at the same namespace —
//! the visual is spawned with `$ORGANON_IPC_NS` set automatically by the editor, but a
//! hand-run inference runtime needs it in its own environment:
//!
//! ```sh
//! ORGANON_IPC_NS=organon-mind ./organic-math-mind-runtime
//! ```

use nih_plug::prelude::*;
use organic_math_native::OrganicMath;

fn main() {
    // #520 Tier 2 — Organon Mind needs this more than full Organon does: its Mind
    // tab is a four-card, three-column workstation, which is exactly what a fixed
    // 1280×860 cannot hold on a laptop. Marking the process lets the editor make
    // its own `NSWindow` resizable + zoomable (see `window_macos`).
    organic_math_native::window_macos::mark_standalone();
    nih_export_standalone::<OrganicMath>();
}
