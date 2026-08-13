//! The Shell compositor skeleton — the five PRD §2 regions as egui panels.
//!
//! Layout (Shell #3 T1):
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ top bar: product · project · Ask Pi · palette · status       │
//! ├─────────┬──────────────────────────────────┬─────────────────┤
//! │ project │ agent workspace (central)        │ live inspector  │
//! ├─────────┴──────────────────────────────────┴─────────────────┤
//! │ evidence strip                                               │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! Every decision is a pure function of [`ShellApp`] state + [`Edition`], never of
//! the global `EDITION` const — the `mind_ui` rule — so the whole layout is exercised
//! by tests from a default (feature-off) build. The surface deck (PRD §7.2) arrives
//! with docking in Tier 3; T1's central region is the workspace placeholder.

use organon_core::edition::Edition;

use crate::mock_agent::MockAgent;
use crate::session::{now_unix_ms, EventKind, SessionEvent, SessionLog, SCHEMA_VERSION};
use crate::theme::Theme;
use crate::timeline;

/// The embedded live Organon viewport, as the UI sees it (Shell #6 T1).
///
/// egui types only, deliberately — the crate stays windowing-free. The bin owns the
/// wgpu texture and the `World` that fills it; this is the UI-side handle: which egui
/// texture to paint, how big it currently is, and whether the user paused it.
pub struct SceneView {
    /// The id the bin registered its offscreen texture under. Stable across resizes —
    /// the bin rebinds the same id rather than minting one per texture, which is what
    /// keeps a resize from leaking a bind group (`register_scene_texture`'s rule).
    pub texture: egui::TextureId,
    /// The texture's rendered size in points. The pane may have resized since — the
    /// bin follows the reserved rect one frame behind — and egui scales the image for
    /// the frame that gap is visible.
    pub size: [f32; 2],
    /// Flipped by the pane's Pause/Play button; read by the bin, which stops
    /// advancing the world and keeps presenting the last rendered frame.
    pub paused: bool,
    /// What the pane reserved this frame, in points — written by [`ui`], read by the
    /// bin next frame to size the offscreen texture. `None` until the first layout.
    pub desired: Option<[f32; 2]>,
}

/// The compositor's state. Deliberately tiny in T1: real state (sessions, tasks,
/// surfaces) belongs to Shell #4's schema, which this struct will consume —
/// growing fields here that #4 owns would be the two-vocabularies mistake one layer
/// down.
pub struct ShellApp {
    /// Which product this front-of-house is dressed as. Always `Edition::Shell` in
    /// the shipping binary; a parameter so tests can drive it.
    pub edition: Edition,
    /// Command palette visibility (⌘K / Ctrl+K). A stub in T1: the palette exists,
    /// opens, and closes; its command catalog is Tier 2.
    pub palette_open: bool,
    /// The palette's live query text.
    pub palette_query: String,
    /// The live viewport, once the bin has one to show (Shell #6 T1). `None`
    /// keeps the T1 workspace placeholder — which is also every headless test, since
    /// only a bin with a GPU can produce a real texture.
    pub scene: Option<SceneView>,
    /// This session's events, in seq order — what the workspace timeline renders.
    pub timeline_events: Vec<SessionEvent>,
    /// Timeline scroll state (the pin/unpin bit).
    pub timeline: timeline::TimelineState,
    /// The scripted demo agent, when one is running (Shell #7 T1). Kept
    /// attached after it finishes so the timeline stays labeled scripted for as
    /// long as its content is.
    pub mock: Option<MockAgent>,
    /// The on-disk session log, when one is attached. `None` in tests and when the
    /// store is unavailable — the in-memory timeline is then the whole session.
    pub log: Option<SessionLog>,
    /// Opens the on-disk log the first time a demo starts. The *binary* wires this
    /// to the real store (`mock_agent::open_demo_log`); tests leave it `None`, so
    /// nothing in the lib can ever write the real store on its own.
    pub log_source: Option<fn() -> Option<SessionLog>>,
}

impl ShellApp {
    pub fn new(edition: Edition) -> Self {
        Self {
            edition,
            palette_open: false,
            palette_query: String::new(),
            scene: None,
            timeline_events: Vec::new(),
            timeline: timeline::TimelineState::default(),
            mock: None,
            log: None,
            log_source: None,
        }
    }

    /// Start the built-in scripted demo (idempotent — a second press mid-story is
    /// a no-op, not a restart). Opens the on-disk log through `log_source` if one
    /// is wired and none is attached yet.
    pub fn start_demo(&mut self) {
        if self.mock.is_some() {
            return;
        }
        self.mock = Some(MockAgent::demo());
        if self.log.is_none() {
            if let Some(open) = self.log_source {
                self.log = open();
            }
        }
    }

    /// Append one event to the timeline — and to the session log when one is
    /// attached, in which case the log's envelope (its seq + timestamp) is the
    /// authority. A log write failure degrades to in-memory rather than dropping
    /// the event: the screen must never lose story to a disk problem.
    pub fn record(&mut self, kind: EventKind) {
        let event = match self.log.as_mut().and_then(|log| log.append(kind.clone()).ok()) {
            Some(event) => event,
            None => SessionEvent {
                seq: self.timeline_events.len() as u64,
                at_unix_ms: now_unix_ms(),
                schema_version: SCHEMA_VERSION,
                kind,
            },
        };
        self.timeline_events.push(event);
    }

    /// The top-bar heading: product name, and the project slot (empty until
    /// Shell #4's Project object exists).
    pub fn heading(&self) -> String {
        format!("{} · Project: —", self.edition.product_name())
    }

    /// Toggle the palette, clearing the query on open so a stale search never
    /// greets the user. Pure, and the one piece of input logic T1 owns.
    pub fn toggle_palette(&mut self) {
        self.palette_open = !self.palette_open;
        if self.palette_open {
            self.palette_query.clear();
        }
    }
}

/// The palette shortcut: ⌘K on the Mac arrives as `Command`, Ctrl+K everywhere else —
/// egui's `command` modifier maps to whichever the platform means.
fn palette_shortcut(i: &egui::InputState) -> bool {
    i.key_pressed(egui::Key::K) && i.modifiers.command
}

/// Run one frame of the Shell UI. The caller owns the `egui::Context` and the frame
/// loop (`src/main.rs` for the product; a bare context in tests) — and the [`Theme`],
/// which is borrowed here rather than held on [`ShellApp`] so that the palette has exactly
/// one owner in a process however many front-ends are on screen.
pub fn ui(ctx: &egui::Context, app: &mut ShellApp, theme: &Theme) {
    if ctx.input(palette_shortcut) {
        app.toggle_palette();
    }

    // Drive the scripted demo from the frame clock. The mock has no thread of its
    // own (that is what makes it deterministic), so while it still has beats left
    // we ask for a repaint — otherwise an idle window would stall the story.
    let now_ms = (ctx.input(|i| i.time) * 1000.0) as u64;
    let due = app.mock.as_mut().map(|m| m.tick(now_ms)).unwrap_or_default();
    for kind in due {
        app.record(kind);
    }
    if app.mock.as_ref().is_some_and(|m| !m.is_done()) {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    top_bar(ctx, app);

    egui::TopBottomPanel::bottom("evidence-strip")
        .exact_height(28.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.weak("Evidence · command runs · captures · delegated work — Shell #4");
            });
        });

    egui::SidePanel::left("project-rail")
        .resizable(true)
        .default_width(200.0)
        .show(ctx, |ui| {
            ui.add_space(6.0);
            ui.strong("Project");
            ui.separator();
            for slot in ["Repositories", "Tasks", "Agents", "Sessions", "Artifacts", "Layouts"] {
                ui.weak(slot);
            }
        });

    egui::SidePanel::right("inspector")
        .resizable(true)
        .default_width(240.0)
        .show(ctx, |ui| {
            ui.add_space(6.0);
            ui.strong("Inspector");
            ui.separator();
            ui.weak("Follows focus: task, artifact, command run,");
            ui.weak("viewport object, build/test state.");
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        // The workspace region, in its three honest states (#6 T1 × #7 T1):
        //   viewport + timeline → the working arrangement: scene on top, story below;
        //   viewport alone      → the scene fills the region (with the demo affordance);
        //   neither             → the placeholder.
        // The timeline branch is one hook call — the rendering lives in `timeline.rs`.
        let has_timeline = !app.timeline_events.is_empty();
        if let Some(scene) = app.scene.as_mut() {
            if has_timeline {
                // The split is a T1 fixed ratio, not a docking decision — real
                // docking is #3 Tier 3's, and this arrangement folds into it.
                let scene_h = (ui.available_height() * 0.45).max(120.0);
                ui.allocate_ui(egui::vec2(ui.available_width(), scene_h), |ui| {
                    ui.set_min_height(scene_h);
                    scene_pane(ui, scene);
                });
                ui.separator();
            } else {
                scene_pane(ui, scene);
            }
        }
        if has_timeline {
            // T1: approval decisions are surfaced, not acted on — the bridge that
            // routes them back to an agent (and into the log) is a later #7 tier.
            let _action = timeline::show(
                ui,
                &app.timeline_events,
                &mut app.timeline,
                app.mock.is_some(),
                theme,
            );
        } else if app.scene.is_none() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                ui.heading(app.edition.product_name());
                ui.add_space(12.0);
                ui.weak("The agent workspace — conversation, plans, tool events,");
                ui.weak("diffs, artifacts, and live surfaces land here.");
                ui.add_space(8.0);
                ui.weak("⌘K — command palette");
                ui.add_space(12.0);
                if ui
                    .button("▶ Demo session")
                    .on_hover_text(
                        "Replay the built-in scripted story (mock agent — no runtime attached)",
                    )
                    .clicked()
                {
                    app.start_demo();
                }
            });
        }
    });

    if app.palette_open {
        command_palette(ctx, app);
    }
}

/// The live Organon viewport filling the central workspace (Shell #6 T1).
///
/// The image fills whatever the header leaves, and the reserved size is reported back
/// through [`SceneView::desired`] so the bin can size the offscreen texture to match
/// on the next frame — the same one-frame-behind arrangement the Mind workstation
/// pane uses, and just as harmless here.
fn scene_pane(ui: &mut egui::Ui, scene: &mut SceneView) {
    ui.horizontal(|ui| {
        ui.strong("Organon — live");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if scene.paused { "Play" } else { "Pause" };
            if ui.button(label).clicked() {
                scene.paused = !scene.paused;
            }
        });
    });
    ui.separator();
    let avail = ui.available_size();
    scene.desired = Some([avail.x, avail.y]);
    // `Sense::hover()` — the pane claims no gestures in T1; camera input over the
    // viewport is a later tier's `scene_input` wiring, not a decision to preempt here.
    let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
    if rect.is_positive() {
        ui.painter().image(
            scene.texture,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            // Not a colour and not themeable: `image`'s last argument is a per-channel
            // MULTIPLIER, and white is the identity. A theme reaching in here would tint
            // the engine's own output.
            egui::Color32::WHITE,
        );
    }
}

fn top_bar(ctx: &egui::Context, app: &mut ShellApp) {
    egui::TopBottomPanel::top("top-bar").exact_height(32.0).show(ctx, |ui| {
        ui.horizontal_centered(|ui| {
            ui.strong(app.heading());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // A status dot, not a claim: nothing is connected yet, and the UI
                // should say so rather than glow green on faith.
                ui.weak("● no runtime");
                if ui.button("⌘K").on_hover_text("Command palette").clicked() {
                    app.toggle_palette();
                }
                let _ = ui.button("Ask Pi").on_hover_text(
                    "The primary agent connects in Shell #7",
                );
                // The demo entry point survives the placeholder being covered by a
                // viewport (#6 × #7): once a scene fills the workspace, this is the
                // only way to start the scripted story by hand.
                if app.mock.is_none() && app.timeline_events.is_empty() {
                    if ui
                        .button("▶ Demo")
                        .on_hover_text("Replay the built-in scripted session (mock agent)")
                        .clicked()
                    {
                        app.start_demo();
                    }
                }
            });
        });
    });
}

fn command_palette(ctx: &egui::Context, app: &mut ShellApp) {
    let mut open = app.palette_open;
    egui::Window::new("Command palette")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_min_width(420.0);
            let edit = egui::TextEdit::singleline(&mut app.palette_query)
                .hint_text("Type a command… (catalog lands with the command service)")
                .desired_width(f32::INFINITY);
            let response = ui.add(edit);
            response.request_focus();
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.palette_open = false;
            }
        });
    // The window's own close button and Escape both end at the same state.
    app.palette_open &= open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_names_the_product() {
        let shell = ShellApp::new(Edition::Shell);
        assert!(shell.heading().starts_with("Organon Shell"));
        // Edition-parametric on purpose: the same skeleton dressed as another
        // product names that product — no string is hard-coded in the layout.
        let full = ShellApp::new(Edition::Full);
        assert!(full.heading().starts_with("Organon ·"));
    }

    #[test]
    fn palette_toggle_clears_stale_query() {
        let mut app = ShellApp::new(Edition::Shell);
        app.toggle_palette();
        assert!(app.palette_open);
        app.palette_query.push_str("stale");
        app.toggle_palette();
        assert!(!app.palette_open);
        app.toggle_palette();
        assert!(app.palette_open);
        assert!(app.palette_query.is_empty(), "reopening must not greet with a stale query");
    }

    /// The whole layout runs headless — egui needs no window to lay out a frame, so
    /// the panel structure is exercised for every edition from a feature-off build.
    #[test]
    fn ui_runs_headless_for_every_edition() {
        for edition in [Edition::Shell, Edition::Full, Edition::Mind] {
            let ctx = egui::Context::default();
            let mut app = ShellApp::new(edition);
            app.palette_open = true; // cover the palette path too
            let _ = ctx.run(egui::RawInput::default(), |ctx| ui(ctx, &mut app, &Theme::organon()));
        }
    }

    /// The viewport pane path (Shell #6 T1), with a dummy texture id — no GPU
    /// in a test, but the layout, the header, and the size report are all pure egui.
    #[test]
    fn scene_pane_reports_reserved_size() {
        let ctx = egui::Context::default();
        let mut app = ShellApp::new(Edition::Shell);
        app.scene = Some(SceneView {
            texture: egui::TextureId::User(1),
            size: [640.0, 360.0],
            paused: false,
            desired: None,
        });
        let mut input = egui::RawInput::default();
        input.screen_rect =
            Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0)));
        let _ = ctx.run(input, |ctx| ui(ctx, &mut app, &Theme::organon()));
        let scene = app.scene.as_ref().expect("scene survives the frame");
        let desired = scene.desired.expect("the pane must report what it reserved");
        // The pane sits inside rails and bars, so the reserve is well below the window
        // but must still be a real region, not a degenerate one.
        assert!(desired[0] > 100.0 && desired[1] > 100.0, "reserved {desired:?}");
        assert!(!scene.paused, "nothing in a frame flips pause on its own");
    }

    /// One frame at raw-input time `t` — the same clock `ui()` ticks the mock from,
    /// so tests drive the demo with literal seconds instead of waiting them out.
    fn frame_at(app: &mut ShellApp, t: f64) {
        let ctx = egui::Context::default();
        let raw = egui::RawInput { time: Some(t), ..Default::default() };
        let _ = ctx.run(raw, |ctx| ui(ctx, app, &Theme::organon()));
    }

    /// The demo timeline renders headless for every edition: start the mock, tick
    /// it through `ui()` frames past the story's end, and lay the populated
    /// timeline out — the smoke test the plain skeleton already gets, now with
    /// every card treatment on screen.
    #[test]
    fn demo_timeline_renders_headless_for_every_edition() {
        for edition in [Edition::Shell, Edition::Full, Edition::Mind] {
            let mut app = ShellApp::new(edition);
            app.start_demo();
            frame_at(&mut app, 0.0);
            assert!(!app.timeline_events.is_empty(), "the opening beat lands at t=0");
            frame_at(&mut app, 60.0); // far past the ~8.5s story
            let total = app.timeline_events.len();
            assert!(total >= 18, "the whole story arrived: {total}");
            assert!(app.mock.as_ref().unwrap().is_done());
            // Seq is contiguous from 0 — the fallback envelope path (no log).
            assert!(app.timeline_events.iter().enumerate().all(|(i, e)| e.seq == i as u64));
            // One more frame with the finished mock: the timeline (not the
            // placeholder) is what renders, and rendering it emits nothing new.
            frame_at(&mut app, 61.0);
            assert_eq!(app.timeline_events.len(), total);
        }
    }

    /// With a log attached, the demo leaves a genuine on-disk session: every
    /// timeline event is a line in `events.jsonl`, with the log's envelope as the
    /// authority on seq.
    #[test]
    fn demo_events_land_in_an_attached_session_log() {
        let root = std::env::temp_dir()
            .join(format!("organon-shell-app-demo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = ShellApp::new(Edition::Shell);
        app.log = Some(crate::session::SessionLog::open(&root, "demo").unwrap());
        app.start_demo();
        frame_at(&mut app, 0.0);
        frame_at(&mut app, 60.0);
        let on_disk = app.log.as_ref().unwrap().read_all().unwrap();
        assert_eq!(on_disk.len(), app.timeline_events.len());
        assert_eq!(on_disk, app.timeline_events, "the screen shows exactly what the log holds");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `start_demo` is idempotent and only consults `log_source` when no log is
    /// attached — a second press mid-story must not restart it or reopen a store.
    #[test]
    fn start_demo_is_idempotent() {
        let mut app = ShellApp::new(Edition::Shell);
        app.start_demo();
        frame_at(&mut app, 0.0);
        let after_first = app.timeline_events.len();
        app.start_demo();
        frame_at(&mut app, 0.1);
        assert_eq!(app.timeline_events.len(), after_first, "no restart, no re-emission");
    }
}
