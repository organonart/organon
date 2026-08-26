//! Organon Console — the agent-operating workstation (Console #3).
//!
//! A native, GPU-composited workspace for working with AI agents.
//! Product definition: `doc/organon_shell_prd.md`
//! (private annex); living code state: `CONSOLE_ARCHITECTURE.md` at the repo root, which
//! travels with this crate wherever it goes.
//!
//! Tier 1 scope (Console #3): the compositor skeleton — the five PRD §2 regions
//! as egui panels, dark, keyboard-first, with a command-palette stub. Everything here
//! is a pure function of state + `Edition`, so it is unit-tested from a default
//! (feature-off) build; only `src/main.rs` (the window binary) needs
//! `--features console-edition`.
//!
//! Console #4 T1 adds [`session`]: the PRD §5 schema and the append-only
//! session event log — the shared vocabulary #5 and #7 build against.
//! Console #5 T1 adds [`command`]: the typed command service — catalog as
//! data, one dispatch entry point, every dispatch leaving a `CommandRun` record.
//! Console #7 T1 adds [`mock_agent`] (a scripted, pull-ticked stand-in for
//! Pi) and [`timeline`] (the workspace's typed-card rendering of a session's
//! events) — the workspace side of the agent bridge, real adapters to follow.

//! Console #10 T1 (PRD v3) adds [`term`] (the PTY + adopted VT core) and
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
//! Console Spike Tier 5 adds the two halves of a **patch** — a rectangle a writer left in its
//! own output and then claimed. [`block_anchor`] is *where*: the same arithmetic applied to a
//! reserved run of lines, two integers in and viewport rows out. [`block_panel`] is *what*:
//! the kinds a claim can name, one of which is a live egui control panel rather than a
//! picture of one. The split is the tier's whole shape — claim, anchor and ledger are common
//! to every kind, and the kind selects the paint and nothing before it.
//!
//! Console Spike §5.9 forks the console into **two front-ends over one renderer**, and
//! five modules make the second one. The terminal host ([`term`] + [`term_view`]) is
//! untouched and remains the universal fallback. Beside it now: [`agent_event`] decodes
//! Claude Code's NDJSON, [`conversation`] folds those into a renderable transcript,
//! [`agent_map`] is the seam between the two, [`agent_session`] owns the live child
//! process, and [`conversation_view`] draws the result. `CONSOLE_ARCHITECTURE.md` §
//! "Two front-ends" owns the shape.
//!
//! [`text_diff`] is the sixth, and the smallest: an `Edit` arrives as two whole strings
//! and a card has to show what changed between them, so the line alignment lives in its
//! own module with no egui in it and is tested with plain strings.
//!
//! [`prefs`] is the console's **first preference writer**. Everything a user could
//! choose until now died with the process: the one user-config path in this crate is a
//! *read* with no matching write (`harnesses.json`), and every other knob is an
//! `ORGANON_SHELL_*` variable sampled at startup. `preferences.json` sits beside
//! `harnesses.json` in the same store and is written atomically — its module doc owns the
//! three properties that follow from making a durable promise.
//!
//! **Approvals** close the loop that front-end opened: [`mcp`] is the console's MCP server
//! as a value, [`mcp_http`] serves it over loopback HTTP inside the console process so
//! Claude Code's `--permission-prompt-tool` can reach it, and [`approval`] is the console's
//! answer — the blocking hook, the decision memory that "allow and remember" is built from,
//! and what a click means. `doc/console_approval_protocol.md` is the measured spec all
//! three are written against.
//!
//! [`theme`] holds every colour the console paints. It is a plain struct with one owner in
//! the app state and `&Theme` at every draw site — the roles were already named (`RUNNING`,
//! `CONTEXT_ARC`, `COMPOSER_EDGE_DEAD`), and what they lacked was the ability to hold a
//! second answer, which a `const` cannot.
//!
//! [`registry`] is the console's **command vocabulary as one table**, and the slash commands
//! generated from it. Its reason is a measurement: a verb typed by a human used to be sent to
//! the agent, understood by inference, found by a tool search, called back as a tool, and then
//! offered to that same human for approval — about thirteen seconds for a command he had
//! already decided on. A console that owns its composer does not need to ask an agent what its
//! own user meant. The registry is also shaped for the pointer surface that comes next: a
//! group, a verb and its argument choices, which is a radial menu's three rings with no second
//! table.
//!
//! [`posture`] is the **second** axis, orthogonal to that one: the theme is what the console
//! is made of, posture is how it holds itself — terminal-tight or desktop-open. Every form
//! token in it is a scalar, so the desktop state is the same draw code reading different
//! numbers rather than a second renderer, and one `t ∈ [0,1]` reaches every draw site as a
//! resolved `&Form` beside the `&Theme`.
//!
//! [`layout`] is what makes an arrangement of [`region`]s **a thing with a name that survives the
//! process**: `layouts.json` beside `harnesses.json`, the harness registry's discipline for the
//! file and [`prefs`]'s for the writing of it. Its one hard rule is that a load is a
//! **transaction** — a saved arrangement arrives all at once, possibly from another build's file,
//! so it is validated whole and either replaces the layout or refuses by name, never half-applies.
//! `doc/organon_is_the_product.md` §4 is why that is not a convenience.
//!
//! [`module`] is what "approve a repo" means, as data — `doc/organon_module_viewport.md` §3.
//! Two files with two authors: `organon-module.toml` in the module's own repo **requests**,
//! and `modules.json` beside `harnesses.json` **grants**. The split is structural rather than
//! documented: the two grant sets are two types with no conversion between them, and the approval
//! step takes the **names** a person chose rather than a finished grant — so a manifest cannot
//! grant itself anything, and one manifest's grants cannot be attached to another's approval
//! either. The unit of trust is a **commit**, so a record naming only a
//! branch is refused by name on load; and the commit that was *built* is a second field from
//! the commit that was *approved*, because the record is a lie exactly when they silently
//! differ. Nothing here starts a process — no clone, no build, no launch.
//!
//! [`card_density`] is the **third**: how much room a tool call takes once it has stopped
//! being news. Theme is what the console is made of, posture is how it holds itself, density
//! is how long a thing keeps its weight. Success collapses to one line and a run of them to
//! one row; a failure never does, which is the asymmetry that makes the quiet safe to read.
//! Pure functions and a side map — no egui, so the judgments are tested without a window.

pub mod agent_event;
pub mod agent_map;
pub mod agent_session;
pub mod app;
pub mod approval;
pub mod block_anchor;
pub mod block_panel;
pub mod camera;
pub mod card_density;
pub mod command;
pub mod conversation;
pub mod conversation_view;
pub mod harness;
pub mod layout;
pub mod log_file;
pub mod mcp;
pub mod mcp_http;
pub mod mock_agent;
pub mod module;
pub mod module_work;
pub mod module_host;
pub mod module_input;
pub mod panel_stack;
pub mod platform;
pub mod portal;
pub mod posture;
pub mod prefs;
pub mod region;
pub mod region_line;
pub mod registry;
pub mod screen;
pub mod scroll_anchor;
pub mod session;
pub mod status_log;
pub mod tabs;
pub mod term;
pub mod term_view;
pub mod text_diff;
pub mod theme;
pub mod theme_edit;
pub mod timeline;
