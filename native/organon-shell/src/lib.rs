//! Organon Shell — the agent-operating workstation (Shell #3).
//!
//! A native, GPU-composited workspace for working with AI agents.
//! Product definition: `doc/organon_shell_prd.md`
//! (private annex); living code state: `SHELL_ARCHITECTURE.md` at the repo root, which
//! travels with this crate wherever it goes.
//!
//! Tier 1 scope (Shell #3): the compositor skeleton — the five PRD §2 regions
//! as egui panels, dark, keyboard-first, with a command-palette stub. Everything here
//! is a pure function of state + `Edition`, so it is unit-tested from a default
//! (feature-off) build; only `src/main.rs` (the window binary) needs
//! `--features shell-edition`.
//!
//! Shell #4 T1 adds [`session`]: the PRD §5 schema and the append-only
//! session event log — the shared vocabulary #5 and #7 build against.
//! Shell #5 T1 adds [`command`]: the typed command service — catalog as
//! data, one dispatch entry point, every dispatch leaving a `CommandRun` record.
//! Shell #7 T1 adds [`mock_agent`] (a scripted, pull-ticked stand-in for
//! Pi) and [`timeline`] (the workspace's typed-card rendering of a session's
//! events) — the workspace side of the agent bridge, real adapters to follow.

//! Shell #10 T1 (PRD v3) adds [`term`] (the PTY + adopted VT core) and
//! [`term_view`] (the egui glyph grid) — the terminal the product now IS. The
//! v2-era modules below it remain compiled and tested: `session`/`command` are
//! load-bearing foundations; `app`/`timeline`/`mock_agent` await re-homing into
//! the block system (tree C) and the structured register (tree D).
//!
//! Console Spike Tier 4 adds [`scroll_anchor`]: absolute line coordinates over the
//! scrollback, so the backdrop's look-epochs can be painted as viewport bands that
//! age with the text instead of with the window. Pure arithmetic — its caller
//! contract is the checklist in its module doc.
//!
//! Console Spike §5.9 forks the console into **two front-ends over one renderer**, and
//! five modules make the second one. The terminal host ([`term`] + [`term_view`]) is
//! untouched and remains the universal fallback. Beside it now: [`agent_event`] decodes
//! Claude Code's NDJSON, [`conversation`] folds those into a renderable transcript,
//! [`agent_map`] is the seam between the two, [`agent_session`] owns the live child
//! process, and [`conversation_view`] draws the result. `SHELL_ARCHITECTURE.md` §
//! "Two front-ends" owns the shape.

pub mod agent_event;
pub mod agent_map;
pub mod agent_session;
pub mod app;
pub mod block_anchor;
pub mod command;
pub mod conversation;
pub mod conversation_view;
pub mod harness;
pub mod mock_agent;
pub mod platform;
pub mod scroll_anchor;
pub mod session;
pub mod tabs;
pub mod term;
pub mod term_view;
pub mod timeline;
