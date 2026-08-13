//! The conversation view: a [`Transcript`] drawn natively, with a composer under it
//! (Console Spike §5.9, the second front-end).
//!
//! Scrollback above, composer below — Telegram, WhatsApp, Claude Desktop and Claude Code
//! are all this shape. The claim the console is making is that **a TUI is not a design;
//! it is what you build when a character grid is the only canvas allowed.** So the
//! interesting part of this file is not the layout, it is the [`tool_card`]: a tool call
//! drawn as a *card* — name, arguments as fields, live-versus-complete state, output —
//! instead of as the text a terminal would have printed. The structure was in the event
//! stream all along; the grid was where it got flattened.
//!
//! # What this owns, and what it does not
//!
//! [`ConversationPane`] owns one agent process, the mapper, the transcript and the
//! composer's text. It does **not** own the window, the tab strip, or the backdrop —
//! `shell_main.rs` does, exactly as it does for a terminal tab.
//!
//! The composer **writes to stdin and renders nothing** (§5.9.3 rule 2). The human turn
//! comes back on the stream under `--replay-user-messages` and is rendered from there,
//! which is what makes ordering free instead of a splice-and-hope.
//!
//! # Bounding is the view's job, deliberately
//!
//! [`crate::conversation`] leaves per-element text unbounded on purpose — a tool result
//! can be a whole file, and truncating it in the model would misrepresent the tool's
//! output while looking like the tool's output. So the clipping happens here, where it is
//! a *presentation* choice and says so on screen ("+N more lines"), and the full text is
//! still in the model.

use std::collections::{HashMap, VecDeque};

use egui::{Color32, CornerRadius, Frame, Margin, RichText};

use crate::agent_map::EventMapper;
use crate::agent_session::{AgentSession, StreamItem};
use crate::block_panel::{DEFAULT_SLIDERS, PAD, PANEL_EDGE, PANEL_FILL, PANEL_TITLE, SLIDER_WIDTH};
use crate::conversation::{
    Arguments, ArtifactBlock, ArtifactContent, Body, Change, Element, ElementId, PanelSpec,
    RunOutcome, SurfaceSpec, ToolCard, ToolState, Transcript,
};
use crate::timeline::pinned_after_scroll;

const HUMAN: Color32 = Color32::from_rgb(0xc8, 0xe6, 0xc8);
const PROSE: Color32 = Color32::from_rgb(0xd2, 0xd8, 0xd2);
const DIM: Color32 = Color32::from_rgb(0x70, 0x7c, 0x70);
const RUNNING: Color32 = Color32::from_rgb(0xe6, 0xc0, 0x4c);
const OK: Color32 = Color32::from_rgb(0x6f, 0xc2, 0x76);
const BAD: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x5f);

/// How much of a tool's output a card draws before it says how much it is not drawing.
const OUTPUT_LINES: usize = 10;
/// The same, for the two halves of an `Edit` diff.
const DIFF_LINES: usize = 12;
/// Diagnostic lines kept (non-JSON stdout, stderr). Bounded and logged, never silent.
const LOG_LINES: usize = 200;

/// How tall a rendered surface is, in **points**.
///
/// A fixed height rather than an aspect ratio, because the width is whatever the transcript
/// column is and an aspect would make a surface's size a function of the window — which is
/// the thing that makes a texture cache thrash on every drag of a window edge. The width does
/// still change with the window, so the target is resized then; that is one resize per drag,
/// not one per aspect.
///
/// 260 pt is chosen against the panel below it: tall enough that a shading gradient reads as
/// a *surface* rather than a stripe, short enough that the surface and its panel are on
/// screen together at the default 1100×720 window. Both halves in one glance is the whole
/// point of the element.
const SURFACE_HEIGHT: f32 = 260.0;

/// The plate a surface with no picture yet shows — the one frame before the console has
/// rendered into it, and every frame where the cap has taken its texture away.
const SURFACE_EMPTY: Color32 = Color32::from_rgb(0x0a, 0x0e, 0x0a);

/// What the view needs the console to draw, for one surface element, this frame.
///
/// **The seam between the two crates.** `organon-shell` knows a surface has a rect, an id and
/// a look *by name*; it cannot see `substrate_materials`, a `World`, or a `wgpu::Device`, and
/// must not learn to — the same contract [`ArtifactAction`] and
/// [`crate::block_panel::BlockAction`] are already held to. So the view says what it laid out
/// and what a hand has put the controls at; the console answers with a [`egui::TextureId`] on
/// the next frame.
///
/// ⚠️ **The size is in POINTS, and it must stay that way.** The console turns it into pixels
/// with `scene_input::pane_pixels_in`, which takes the rect's *fraction of the window* and
/// applies it to the swapchain, so `pixels_per_point` cancels instead of being remembered.
/// Handing pixels across this seam would mean this crate multiplying by a scale — the exact
/// mistake that shipped a point-sized backdrop and froze it into every epoch snapshot for a
/// session (that function's doc owns the measurement).
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceRequest {
    pub element: ElementId,
    /// The look to draw, by name: the material a driving panel last chose, or the one the
    /// surface was summoned with.
    pub look: String,
    /// `(label, value)` in `0.0..=1.0` from the panel driving this surface, in the order the
    /// panel declared them. Empty when nothing drives it.
    pub sliders: Vec<(String, f32)>,
    /// The rect the console should fill, in **points**. See the type doc.
    pub size_points: (f32, f32),
}

/// Everything one frame of [`draw`] hands back.
///
/// A struct rather than a tuple because the two halves answer different questions and are
/// consumed a screen apart in `shell_main.rs` — actions before the frame ends, surface
/// requests as the *next* frame's render list.
#[derive(Clone, Debug, Default)]
pub struct ConversationOutput {
    /// Buttons pressed in panels that drive the **console**. A button in a panel that drives
    /// a surface never appears here — it was consumed by the surface it aims at, which is the
    /// entire difference between beat 7's instrument and this one.
    pub actions: Vec<ArtifactAction>,
    /// The **visible** surfaces this frame, in transcript order.
    pub surfaces: Vec<SurfaceRequest>,
}

/// The pictures the console has ready, by element. Absent is normal, not an error: a surface
/// summoned this frame has no texture until the next one, and a surface the cap evicted has
/// none until it is rendered again.
pub type SurfaceImages = HashMap<ElementId, egui::TextureId>;

/// Is a surface worth rendering this frame?
///
/// **Vertical overlap with the viewport, and nothing else.** A surface always spans the
/// transcript's full width, so the horizontal axis can never be the discriminator; and the
/// scrollback is a tall column where all but a couple of elements are off screen at any
/// moment, which is precisely why this test exists — rendering every surface a long
/// conversation ever summoned is the melt the cap and this function together prevent.
///
/// The bounds are **exclusive**, so a surface resting exactly on the viewport edge with zero
/// visible height is not visible. A zero- or negative-height rect (egui hands one back for a
/// frame while a layout settles) is likewise not visible, which keeps a degenerate rect from
/// ever reaching the sizing arithmetic.
pub fn surface_visible(rect: egui::Rect, viewport: egui::Rect) -> bool {
    rect.width() > 0.0
        && rect.height() > 0.0
        && rect.bottom() > viewport.top()
        && rect.top() < viewport.bottom()
}

/// A button was pressed inside an inline artifact: which element, and which label.
///
/// The sibling of [`crate::block_panel::BlockAction`], and the same contract: this crate
/// draws labels it was handed and reports the one that was pressed, because what a label
/// *means* lives in `shell_main.rs` beside the material table. An [`ElementId`] rather than
/// an index because that is the identity the transcript guarantees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactAction {
    pub element: ElementId,
    pub button: String,
}

/// A composer line the view acts on itself and **never sends to the agent**.
///
/// ⚠️ **A temporary seam, and shaped to be removed.** Summoning is about to become the
/// agent's job — a tool call the integrator answers with
/// [`Transcript::insert_artifact`](crate::conversation::Transcript::insert_artifact), where
/// the tool card is the anchor — so the summoning path is kept entirely separate from the
/// element: this enum decides *that* a panel is wanted, [`ConversationPane::summon_panel`]
/// builds one, and neither knows about the other's existence beyond that call. Deleting
/// this enum and its branch in [`ConversationPane::submit`] removes the local command and
/// touches nothing that draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalCommand {
    /// `/panel` — put a control panel in the flow, here. It drives the **console**: the
    /// backdrop behind whatever terminal tab is next door, which is the instrument beat 7
    /// found wanting.
    Panel,
    /// `/surface` — put a rendered surface in the flow, and a panel under it that drives
    /// **that surface**. Control and consequence in one glance.
    Surface,
}

/// Recognise a local command, or `None` for an ordinary message.
///
/// Exact-match only: a message that merely *starts* with a slash is a message (a human
/// asking about `/panel` must reach the agent), and swallowing it would be a silent send
/// failure — the worst kind, because the composer clears either way.
pub fn local_command(line: &str) -> Option<LocalCommand> {
    match line.trim() {
        "/panel" => Some(LocalCommand::Panel),
        "/surface" => Some(LocalCommand::Surface),
        _ => None,
    }
}

/// One artifact's **live** widget state — the values a hand moves.
///
/// 🚨 **This is why it is here and not in the transcript.** The transcript is folded from an
/// event stream and its elements mutate as events arrive; a slider value living there would
/// be rewritten by the fold, and the symptom is a knob that snaps back mid-drag while the
/// agent is talking. The model names the controls, this holds their values, and
/// [`ElementId`] is the join — which is the reason that id is documented as assigned once
/// and never reused.
#[derive(Clone, Debug, Default)]
struct PanelState {
    /// Parallel to [`PanelSpec::sliders`], by index. Re-synced on a length change, which
    /// cannot happen today (a description is written once) and is cheap insurance for the
    /// day an artifact is allowed to be revised.
    sliders: Vec<f32>,
    /// The button last pressed, for a panel that [`PanelSpec::drives`] a surface. `None`
    /// until one is, which is why a surface has a summoning look of its own to fall back to.
    ///
    /// **Only a driving panel keeps this.** A panel wired to the console has no state to
    /// keep — the console *is* the state, and remembering a shadow copy of it here would be
    /// a second owner of one value, the mistake the whole side-map arrangement exists to
    /// avoid.
    material: Option<String>,
}

impl PanelState {
    /// Where the knobs start. `defaults` is the console's own `(label, value)` table, handed
    /// down for [`ConversationPane::new`]'s reason; a label absent from it starts mid-range,
    /// which is a sane knob rather than a silent zero.
    fn for_spec(spec: &PanelSpec, defaults: &[(String, f32)]) -> Self {
        PanelState {
            sliders: spec.sliders.iter().map(|l| initial_value(l, defaults)).collect(),
            material: None,
        }
    }

    fn sync(&mut self, spec: &PanelSpec, defaults: &[(String, f32)]) {
        if self.sliders.len() != spec.sliders.len() {
            let material = self.material.take();
            *self = PanelState::for_spec(spec, defaults);
            self.material = material;
        }
    }
}

fn initial_value(label: &str, defaults: &[(String, f32)]) -> f32 {
    defaults.iter().find(|(l, _)| l == label).map(|(_, v)| *v).unwrap_or(0.5)
}

/// The starting knobs a console that hands none down gets: the terminal host's panel, so the
/// two front-ends draw one instrument rather than two that resemble each other.
pub fn default_slider_table() -> Vec<(String, f32)> {
    DEFAULT_SLIDERS.iter().map(|(l, v)| ((*l).to_string(), *v)).collect()
}

/// One conversation tab: a live agent, the transcript it is writing, and the composer.
pub struct ConversationPane {
    session: Option<AgentSession>,
    transcript: Transcript,
    mapper: EventMapper,
    /// Set when the process could not be started, or has ended. Shown in place of the
    /// composer, because a tab that silently accepts input nobody will read is worse
    /// than one that says it is dead.
    pub failure: Option<String>,
    pub composer: String,
    /// Non-event lines off the child — the `Warning: no stdin data received…` the CLI
    /// opens a real run with (§5.9.3 rule 6), and anything on stderr.
    log: VecDeque<String>,
    /// Whether the view follows new elements. Re-derived from where the reader actually
    /// left the scroll each frame, so auto-scroll never fights someone reading back.
    pinned: bool,
    /// Focus the composer on the first frame this pane is drawn.
    want_focus: bool,
    /// Live widget state for the artifacts on screen, keyed by [`ElementId`]. Never in the
    /// transcript — see [`PanelState`]. Pruned against the transcript every frame, so an
    /// element the cap evicted takes its state with it.
    artifacts: HashMap<ElementId, PanelState>,
    /// The button labels a summoned panel offers, **handed down** by whoever opened the
    /// tab. This crate cannot see the console's material table and must not learn to; it
    /// draws these and reports which was pressed ([`ArtifactAction`]).
    buttons: Vec<String>,
    /// The slider labels and their starting values, handed down for the same reason the
    /// buttons are: a label like `exposure` means something to the console's `Shared`
    /// snapshot and nothing at all here, and a label this crate invented would be a knob
    /// that moves and changes no pixel — a worse instrument than no knob.
    sliders: Vec<(String, f32)>,
}

impl ConversationPane {
    /// Start a conversation in `cwd`. A spawn failure is **kept, not returned** — the tab
    /// opens and says what went wrong, which is the only way a user finds out.
    ///
    /// `buttons` are the labels an inline panel offers, and `sliders` its `(label, start)`
    /// table. Constructor arguments rather than settable fields because an empty list is a
    /// panel with no controls, which looks like a panel that is broken rather than like a
    /// caller that forgot. [`default_slider_table`] is the table for a caller with no
    /// opinion.
    pub fn new(cwd: Option<&str>, buttons: Vec<String>, sliders: Vec<(String, f32)>) -> Self {
        let (session, failure) = match AgentSession::spawn(cwd) {
            Ok(session) => (Some(session), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            session,
            transcript: Transcript::new(),
            mapper: EventMapper::new(),
            failure,
            composer: String::new(),
            log: VecDeque::new(),
            pinned: true,
            want_focus: true,
            artifacts: HashMap::new(),
            buttons,
            sliders,
        }
    }

    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    pub fn log(&self) -> impl Iterator<Item = &String> {
        self.log.iter()
    }

    /// Drain the agent and fold whatever arrived. Returns true when the view should
    /// repaint — appended *or* updated, since a streamed delta changes pixels without
    /// changing the element list.
    ///
    /// [`Change::Appended`] additionally re-arms the follow, which is the difference
    /// between "new content pulls the view down" and "a token lands and yanks the reader
    /// mid-sentence".
    pub fn pump(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else { return false };
        let items = session.pump();
        if items.is_empty() {
            return false;
        }
        let mut changed = false;
        for item in items {
            match item {
                StreamItem::Event(event) => {
                    for mapped in self.mapper.map(&event) {
                        match self.transcript.apply(mapped) {
                            Change::Appended(_) => {
                                self.pinned = true;
                                changed = true;
                            }
                            Change::Updated(_) | Change::Meta => changed = true,
                            Change::Ignored(_) => {}
                        }
                    }
                }
                StreamItem::Noise(line) => self.note(format!("stdout: {line}")),
                StreamItem::Stderr(line) => self.note(format!("stderr: {line}")),
                StreamItem::Eof => {
                    self.note("the agent process ended".to_string());
                    self.failure = Some(
                        "the agent process ended — close this tab and open a new one to \
                         start another conversation"
                            .to_string(),
                    );
                    changed = true;
                }
            }
        }
        changed
    }

    /// Put a control panel in the flow, at the end of what has been said so far.
    ///
    /// The **only** thing that builds one, so the summoning path above it — today a local
    /// command, next a tool call — is a caller rather than a participant.
    ///
    /// `drives` names the element this panel acts on, or `None` for the console itself. That
    /// argument is the difference between the two summonings and nothing else about the panel
    /// changes, which is what makes the surface a *use* of the panel rather than a fork of it.
    pub fn summon_panel(&mut self, drives: Option<ElementId>) {
        let spec = PanelSpec {
            sliders: self.sliders.iter().map(|(l, _)| l.clone()).collect(),
            buttons: self.buttons.clone(),
            drives,
        };
        let title = match drives {
            Some(_) => "◈ organon · surface controls",
            None => "◈ organon · console",
        };
        self.transcript.insert_artifact(ArtifactBlock {
            title: title.to_string(),
            content: ArtifactContent::Panel(spec),
        });
        // Appended content pulls the view down, exactly as an appended element off the
        // stream does — the panel is at the bottom and being able to see it is the point.
        self.pinned = true;
    }

    /// Put a rendered surface in the flow, with the panel that drives it directly beneath.
    ///
    /// **Two elements, in this order, and the order is the deliverable.** The surface is
    /// above so that the hand is on the controls and the eye is on the consequence, both
    /// inside one screen — which is what beat 7 could not do, because a panel wired to the
    /// console changes a backdrop that only exists on another tab.
    ///
    /// The link is [`PanelSpec::drives`], filled from the id the *first* insertion returned:
    /// an id the transcript has already issued and will never reuse. Building the panel first
    /// and patching it afterwards would need the transcript to be mutable through an element,
    /// which it deliberately is not.
    pub fn summon_surface(&mut self) {
        let spec = SurfaceSpec { look: self.default_look() };
        let inserted = self.transcript.insert_artifact(ArtifactBlock {
            title: "◈ organon · surface".to_string(),
            content: ArtifactContent::Surface(spec),
        });
        let Change::Appended(id) = inserted else {
            // `insert_artifact` appends unconditionally; anything else is a contract change
            // in the transcript, and summoning a panel with no target is the honest fallback.
            self.summon_panel(None);
            return;
        };
        self.summon_panel(Some(id));
    }

    /// The look a surface opens at: the console's first button label, since that list *is*
    /// the material table and its head is the console's own default dressing. An empty list
    /// (a caller that handed down nothing) yields an empty name, which the console reads as
    /// "no material named" — Tier 1's undressed substrate, not a failure.
    fn default_look(&self) -> String {
        self.buttons.first().cloned().unwrap_or_default()
    }

    /// Send the composer's contents and clear it. Renders nothing locally (rule 2).
    fn submit(&mut self) {
        let text = self.composer.trim().to_string();
        if text.is_empty() {
            return;
        }
        // A local command is handled here and **never written to stdin** — checked before
        // the session lookup so it works in a pane whose agent is gone. See [`LocalCommand`]
        // for why this seam is temporary.
        if let Some(command) = local_command(&text) {
            match command {
                LocalCommand::Panel => self.summon_panel(None),
                LocalCommand::Surface => self.summon_surface(),
            }
            self.composer.clear();
            return;
        }
        let Some(session) = self.session.as_mut() else { return };
        match session.send_user(&text) {
            Ok(()) => self.composer.clear(),
            Err(e) => {
                self.note(format!("could not send: {e}"));
                self.failure = Some(format!("the agent stopped listening: {e}"));
            }
        }
    }

    fn note(&mut self, line: String) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }
}

/// Draw the pane: scrollback, then composer. Returns what the frame produced — see
/// [`ConversationOutput`].
///
/// `images` is what the console has ready to paint into the surfaces it was asked for last
/// frame. **One frame of latency is structural, not a shortcut**: a surface's rect is an
/// output of egui layout, so nothing can know its size until the frame that lays it out has
/// run, and the texture is therefore made for the *next* one. `shell_main.rs` already sizes
/// the whole backdrop this way for the same reason, and the visible consequence is one blank
/// frame when a surface is summoned.
///
/// Bottom-up, because the composer's height is known and the scrollback's is whatever is
/// left — the layout every chat client resolves in that order.
pub fn draw(
    ui: &mut egui::Ui,
    pane: &mut ConversationPane,
    images: &SurfaceImages,
) -> ConversationOutput {
    let mut out = ConversationOutput::default();
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        composer(ui, pane);
        ui.add_space(4.0);
        status_line(ui, pane);
        ui.separator();
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            out = scrollback(ui, pane, images);
        });
    });
    out
}

/// One surface as this frame laid it out: enough to build a [`SurfaceRequest`] once the panel
/// that drives it has also been drawn.
struct LaidOutSurface {
    element: ElementId,
    look: String,
    size_points: (f32, f32),
}

/// One driving panel as this frame read it.
struct PanelDrive {
    target: ElementId,
    material: Option<String>,
    sliders: Vec<(String, f32)>,
}

fn scrollback(
    ui: &mut egui::Ui,
    pane: &mut ConversationPane,
    images: &SurfaceImages,
) -> ConversationOutput {
    let mut actions = Vec::new();
    // Collected during the walk and joined after it. A panel is *below* its surface in the
    // ordinary case, but nothing in the model requires that — the link is an id, not an
    // adjacency — so the join happens once the whole list has been seen rather than by
    // reaching backwards mid-loop.
    let mut laid_out: Vec<LaidOutSurface> = Vec::new();
    let mut drives: Vec<PanelDrive> = Vec::new();
    // Destructured so the transcript can be read while the widget state is written: they
    // are disjoint fields, and keeping them disjoint is the whole point of the side map.
    let ConversationPane { transcript, artifacts, pinned, sliders: defaults, .. } = pane;
    let out = egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(*pinned)
        .show(ui, |ui| {
            // The visible slice of the scrollback, read once for the whole walk. Every
            // surface's visibility is decided against this one rect, so two surfaces cannot
            // come to disagree about where the viewport is.
            let viewport = ui.clip_rect();
            ui.add_space(6.0);
            if transcript.is_empty() {
                ui.label(
                    RichText::new(
                        "no messages yet — type below and press Enter, `/surface` for a \
                         rendered surface with its own controls, or `/panel` for a panel \
                         that drives the console",
                    )
                    .color(DIM)
                    .italics(),
                );
            }
            for element in transcript.elements() {
                match &element.body {
                    // The one body drawn here rather than in `draw_element`: it is the only
                    // one that needs state to survive between frames, and `draw_element`
                    // has nowhere to keep it.
                    Body::Artifact(artifact) => match &artifact.content {
                        ArtifactContent::Panel(spec) => {
                            // Empty on the first frame; `panel_body` syncs it to the
                            // description, which is where the starting values come from.
                            let state = artifacts.entry(element.id).or_default();
                            let pressed = panel_element(ui, element.id, artifact, spec, state, defaults);
                            match spec.drives {
                                // Consumed here: a driving panel's button changes the
                                // surface it names and nothing else. Returning it as an
                                // action too would ALSO repaint the console's backdrop —
                                // the exact "the effect is on another tab" the surface
                                // exists to stop.
                                Some(target) => drives.push(PanelDrive {
                                    target,
                                    material: state.material.clone(),
                                    sliders: spec
                                        .sliders
                                        .iter()
                                        .cloned()
                                        .zip(state.sliders.iter().copied())
                                        .collect(),
                                }),
                                None => {
                                    if let Some(button) = pressed {
                                        actions
                                            .push(ArtifactAction { element: element.id, button });
                                    }
                                }
                            }
                        }
                        ArtifactContent::Surface(spec) => {
                            let rect = surface_element(
                                ui,
                                element.id,
                                artifact,
                                images.get(&element.id).copied(),
                            );
                            if surface_visible(rect, viewport) {
                                laid_out.push(LaidOutSurface {
                                    element: element.id,
                                    look: spec.look.clone(),
                                    size_points: (rect.width(), rect.height()),
                                });
                            }
                        }
                    },
                    _ => draw_element(ui, element),
                }
                ui.add_space(8.0);
            }
        });
    *pinned = pinned_after_scroll(out.state.offset.y, out.content_size.y, out.inner_rect.height());
    // State outlives its element for exactly as long as it takes to notice. The transcript
    // evicts from the front and `get` answers `None` for an evicted id, so this is a
    // one-line answer to "does the side map leak on a long session" — it does not.
    artifacts.retain(|id, _| transcript.get(*id).is_some());
    ConversationOutput { actions, surfaces: join_drives(laid_out, drives) }
}

/// Fold each driving panel's state into the surface it names.
///
/// Pure, and separated from the walk for exactly that reason: this is the arithmetic that
/// decides *what look gets rendered*, and it has three cases worth pinning — a surface with
/// no driver keeps its summoning look, a driver whose target is not on screen contributes
/// nothing, and a driver that has not yet had a button pressed changes the sliders without
/// changing the material.
///
/// Last driver wins if two name one surface. Nothing summons that today; stating the rule is
/// cheaper than discovering it.
fn join_drives(laid_out: Vec<LaidOutSurface>, drives: Vec<PanelDrive>) -> Vec<SurfaceRequest> {
    let mut surfaces: Vec<SurfaceRequest> = laid_out
        .into_iter()
        .map(|s| SurfaceRequest {
            element: s.element,
            look: s.look,
            sliders: Vec::new(),
            size_points: s.size_points,
        })
        .collect();
    for drive in drives {
        let Some(target) = surfaces.iter_mut().find(|s| s.element == drive.target) else {
            continue; // off screen, or evicted — nothing to render, so nothing to fold into.
        };
        if let Some(material) = drive.material {
            target.look = material;
        }
        target.sliders = drive.sliders;
    }
    surfaces
}

fn draw_element(ui: &mut egui::Ui, element: &Element) {
    match &element.body {
        Body::Human(h) => {
            Frame::new()
                .fill(Color32::from_rgb(0x11, 0x18, 0x11))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("you").color(DIM).small());
                    ui.label(RichText::new(&h.text).color(HUMAN));
                });
        }
        Body::Assistant(a) => {
            // No frame: the agent's prose is the page, not a card on it.
            let text = if a.complete { a.text.clone() } else { format!("{}▍", a.text) };
            ui.label(RichText::new(text).color(PROSE));
        }
        Body::Tool(card) => tool_card(ui, card),
        // Drawn by `scrollback`, which holds the widget state a panel needs between
        // frames. Nothing to do here, and nothing missing: an element is drawn exactly
        // once, by whichever of the two has what it needs.
        Body::Artifact(_) => {}
        Body::RunEnd(end) => {
            let (label, color) = match end.outcome {
                RunOutcome::Ok => ("turn complete", DIM),
                RunOutcome::Error => ("turn failed", BAD),
                RunOutcome::Cancelled => ("turn cancelled", RUNNING),
            };
            let detail = end.detail.as_deref().unwrap_or("");
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("── {label}")).color(color).small());
                if !detail.is_empty() {
                    ui.label(RichText::new(detail).color(DIM).small());
                }
            });
        }
    }
}

/// **The artifact a terminal could not have shown.**
///
/// A terminal receives a tool call as whatever text the harness chose to print, already
/// flattened. The event stream carries it structured — name, the complete input object,
/// a correlation id, and later a result — so it is drawn as a card whose state is
/// visible: amber and "running" while the id is unresolved, green or red once it is.
/// `Edit` goes one step further and renders its `old_string`/`new_string` as a real diff,
/// because those arrive as *fields*, not as a patch someone has to parse back out of
/// prose.
fn tool_card(ui: &mut egui::Ui, card: &ToolCard) {
    let (state_text, accent) = match &card.state {
        ToolState::Running => ("running", RUNNING),
        ToolState::Complete { is_error: false, .. } => ("ok", OK),
        ToolState::Complete { is_error: true, .. } => ("error", BAD),
    };
    Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(8))
        .stroke(egui::Stroke::new(1.0f32, accent))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let name = card.name.as_deref().unwrap_or("(call not seen)");
                ui.label(RichText::new(name).color(accent).strong().monospace());
                ui.label(RichText::new(state_text).color(accent).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(card.call_id.as_str()).color(DIM).small().monospace());
                });
            });

            match edit_diff(card.name.as_deref(), &card.arguments) {
                Some(diff) => diff_body(ui, &diff),
                None => arguments_body(ui, &card.arguments),
            }

            if let Some(output) = card.state.output() {
                ui.add_space(4.0);
                let (shown, hidden) = clip_lines(output, OUTPUT_LINES);
                for line in shown {
                    ui.label(RichText::new(line).monospace().small().color(PROSE));
                }
                if hidden > 0 {
                    ui.label(RichText::new(format!("+{hidden} more lines")).color(DIM).small());
                }
            }
        });
}

/// **The artifact that is not a picture of anything: a live control panel, inline.**
///
/// The terminal host next door needed a protocol to put one of these in the page — the
/// writer printing its own gap, a claim, absolute-line anchoring, reflow invalidation, and
/// surviving ConPTY's rewriting of the byte stream. Here there is no character grid, so an
/// artifact is *an element in a list that draws itself*, and this function is the whole
/// mechanism. Same widgets, same look ([`crate::block_panel`]'s own constants, imported
/// rather than re-chosen); no anchoring at all, because a flow does not have any.
///
/// Returns the label pressed this frame, if one was.
fn panel_element(
    ui: &mut egui::Ui,
    id: ElementId,
    artifact: &ArtifactBlock,
    spec: &PanelSpec,
    state: &mut PanelState,
    defaults: &[(String, f32)],
) -> Option<String> {
    // Scoped by the element's own id: two panels in one transcript are two sets of widgets,
    // and egui's positional auto-ids would otherwise hand a slider its neighbour's drag
    // state the moment anything above them changes height.
    ui.push_id(id.0, |ui| {
        Frame::new()
            .fill(PANEL_FILL)
            .stroke(egui::Stroke::new(1.0f32, PANEL_EDGE))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(PAD as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().slider_width = SLIDER_WIDTH;
                ui.label(RichText::new(&artifact.title).monospace().strong().color(PANEL_TITLE));
                panel_body(ui, spec, state, defaults)
            })
            .inner
    })
    .inner
}

fn panel_body(
    ui: &mut egui::Ui,
    spec: &PanelSpec,
    state: &mut PanelState,
    defaults: &[(String, f32)],
) -> Option<String> {
    // The description is authoritative about *which* controls exist; the state is
    // authoritative about where they are. This is the only line where the two meet.
    state.sync(spec, defaults);
    let mut pressed = None;
    ui.horizontal_wrapped(|ui| {
        for label in &spec.buttons {
            // A driving panel shows which material its surface is wearing; a console panel
            // cannot, because the console's own state is not this element's to mirror.
            let chosen = spec.drives.is_some() && state.material.as_deref() == Some(label.as_str());
            let text = RichText::new(label).monospace();
            let text = if chosen { text.color(PANEL_TITLE).strong() } else { text };
            if ui.button(text).clicked() {
                pressed = Some(label.clone());
                if spec.drives.is_some() {
                    state.material = Some(label.clone());
                }
            }
        }
    });
    for (label, value) in spec.sliders.iter().zip(state.sliders.iter_mut()) {
        ui.add(egui::Slider::new(value, 0.0..=1.0).text(label.as_str()));
    }
    pressed
}

/// **The artifact that *is* a picture: the engine, rendered into a rectangle of the page.**
///
/// Returns the rect it laid out, in points, which is the whole of what the console needs to
/// size a render target for it. Two things about that rect are worth stating:
///
/// * **It comes from egui layout, not from arithmetic.** The terminal host had to derive a
///   patch's rectangle from absolute line numbers, a scroll anchor, a cell height and a
///   reflow-invalidation rule, because a character grid has no other way to say "here".
///   `allocate_exact_size` is the same statement in one call, and it is the simplification
///   the conversation view was built to buy.
/// * **It is in points, and stays that way** — see [`SurfaceRequest`].
///
/// Painting is one `image` call at UV 0..1: the console renders the target at exactly this
/// rect's pixel size, so there is no fit policy to get wrong and no letterboxing. A surface
/// with no picture yet draws its plate and says so, rather than leaving a hole that reads as
/// a layout bug.
fn surface_element(
    ui: &mut egui::Ui,
    id: ElementId,
    artifact: &ArtifactBlock,
    image: Option<egui::TextureId>,
) -> egui::Rect {
    ui.push_id(id.0, |ui| {
        Frame::new()
            .fill(PANEL_FILL)
            .stroke(egui::Stroke::new(1.0f32, PANEL_EDGE))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(PAD as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&artifact.title).monospace().strong().color(PANEL_TITLE));
                ui.add_space(4.0);
                let width = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(width, SURFACE_HEIGHT),
                    egui::Sense::hover(),
                );
                match image {
                    Some(texture) => {
                        ui.painter().image(
                            texture,
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    None => {
                        ui.painter().rect_filled(rect, CornerRadius::same(4), SURFACE_EMPTY);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "rendering…",
                            egui::FontId::monospace(12.0),
                            DIM,
                        );
                    }
                }
                rect
            })
            .inner
    })
    .inner
}

fn arguments_body(ui: &mut egui::Ui, args: &Arguments) {
    for (key, value) in argument_fields(args) {
        ui.horizontal_wrapped(|ui| {
            if !key.is_empty() {
                ui.label(RichText::new(format!("{key}:")).color(DIM).small().monospace());
            }
            ui.label(RichText::new(value).color(PROSE).small().monospace());
        });
    }
}

fn diff_body(ui: &mut egui::Ui, diff: &EditDiff) {
    if !diff.path.is_empty() {
        ui.label(RichText::new(&diff.path).color(DIM).small().monospace());
    }
    let (removed, removed_more) = clip_slice(&diff.removed, DIFF_LINES);
    let (added, added_more) = clip_slice(&diff.added, DIFF_LINES);
    for line in removed {
        ui.label(RichText::new(format!("- {line}")).color(BAD).small().monospace());
    }
    if removed_more > 0 {
        ui.label(RichText::new(format!("  +{removed_more} more removed")).color(DIM).small());
    }
    for line in added {
        ui.label(RichText::new(format!("+ {line}")).color(OK).small().monospace());
    }
    if added_more > 0 {
        ui.label(RichText::new(format!("  +{added_more} more added")).color(DIM).small());
    }
}

fn status_line(ui: &mut egui::Ui, pane: &ConversationPane) {
    ui.horizontal(|ui| {
        if let Some(failure) = &pane.failure {
            ui.label(RichText::new(failure).color(BAD).small());
            return;
        }
        if pane.transcript.is_working() {
            let n = pane.transcript.running_tools().len();
            let plural = if n == 1 { "tool" } else { "tools" };
            ui.label(RichText::new(format!("● {n} {plural} running")).color(RUNNING).small());
        } else {
            let session = pane.transcript.session_id().unwrap_or("connecting…");
            ui.label(RichText::new(session).color(DIM).small().monospace());
        }
        if let Some(last) = pane.log.back() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(last).color(DIM).small());
            });
        }
    });
}

fn composer(ui: &mut egui::Ui, pane: &mut ConversationPane) {
    let live = pane.failure.is_none();
    ui.horizontal(|ui| {
        ui.label(RichText::new("›").color(if live { HUMAN } else { DIM }).monospace());
        let edit = egui::TextEdit::singleline(&mut pane.composer)
            .desired_width(ui.available_width())
            .font(egui::TextStyle::Monospace)
            .hint_text(if live { "message the agent" } else { "the agent is not running" });
        let response = ui.add_enabled(live, edit);
        if pane.want_focus && live {
            response.request_focus();
            pane.want_focus = false;
        }
        // Enter submits and keeps focus — the composer is where the next message is
        // going, so handing focus back to nothing would cost a click per turn.
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            pane.submit();
            pane.want_focus = true;
        }
    });
}

// ---------------------------------------------------------------------------
// The pure part — clipping and field extraction, tested headless
// ---------------------------------------------------------------------------

/// One `Edit` call, as a card draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditDiff {
    pub path: String,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

/// A tool's arguments as `(key, value)` rows.
///
/// ⚠️ **Only parsed once [`Arguments::complete`] is true.** While a call is streaming,
/// its text is genuinely half a JSON document, and the contract
/// [`crate::conversation::Arguments`] states is that a view may *show* it but must not
/// present it as structured data. So the streaming case comes back as a single unnamed
/// row carrying the raw fragment — which is the honest rendering of "this is arriving".
pub fn argument_fields(args: &Arguments) -> Vec<(String, String)> {
    if !args.complete {
        return if args.text.is_empty() {
            Vec::new()
        } else {
            vec![(String::new(), args.text.clone())]
        };
    }
    match serde_json::from_str::<serde_json::Value>(&args.text) {
        Ok(serde_json::Value::Object(map)) => map
            .into_iter()
            .map(|(k, v)| {
                let value = match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                (k, one_line(&value))
            })
            .collect(),
        // Complete but not an object (or not parseable): show it rather than hide it.
        _ if args.text.is_empty() || args.text == "null" => Vec::new(),
        _ => vec![(String::new(), one_line(&args.text))],
    }
}

/// `Edit`'s structured before/after, when the call is one and its input has settled.
pub fn edit_diff(name: Option<&str>, args: &Arguments) -> Option<EditDiff> {
    if name != Some("Edit") || !args.complete {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&args.text).ok()?;
    let old = value.get("old_string")?.as_str()?;
    let new = value.get("new_string")?.as_str()?;
    Some(EditDiff {
        path: value.get("file_path").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        removed: old.lines().map(str::to_string).collect(),
        added: new.lines().map(str::to_string).collect(),
    })
}

/// The first `max` lines of `text`, and how many were left behind.
pub fn clip_lines(text: &str, max: usize) -> (Vec<&str>, usize) {
    let total = text.lines().count();
    (text.lines().take(max).collect(), total.saturating_sub(max))
}

fn clip_slice(lines: &[String], max: usize) -> (&[String], usize) {
    let shown = lines.len().min(max);
    (&lines[..shown], lines.len() - shown)
}

/// Collapse a value to one display line. A tool argument can be a whole file's contents;
/// the card shows that it is there and how big, not all of it.
fn one_line(value: &str) -> String {
    const LIMIT: usize = 160;
    let flat: String = value.replace(['\n', '\r'], "⏎");
    if flat.chars().count() <= LIMIT {
        return flat;
    }
    let head: String = flat.chars().take(LIMIT).collect();
    format!("{head}… ({} chars)", value.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(text: &str) -> Arguments {
        Arguments { text: text.to_string(), complete: true }
    }

    /// The contract that matters: a streaming call's half-JSON is shown, never parsed.
    #[test]
    fn partial_arguments_are_shown_raw_and_never_parsed() {
        let partial = Arguments { text: r#"{"file_path": "C:\\work\"#.into(), complete: false };
        let fields = argument_fields(&partial);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "", "a fragment has no key to claim");
        assert!(fields[0].1.contains("file_path"));
        assert!(argument_fields(&Arguments::pending()).is_empty());
    }

    #[test]
    fn complete_arguments_become_fields() {
        let fields = argument_fields(&complete(r#"{"file_path":"a.txt","limit":20}"#));
        assert_eq!(
            fields,
            vec![("file_path".to_string(), "a.txt".to_string()), ("limit".into(), "20".into())]
        );
    }

    /// A tool argument can be a whole file. The card must say so rather than draw it.
    #[test]
    fn a_huge_argument_is_summarised_on_one_line() {
        let body = "x".repeat(5000);
        let args = complete(&serde_json::json!({ "content": body }).to_string());
        let fields = argument_fields(&args);
        assert_eq!(fields.len(), 1);
        assert!(fields[0].1.ends_with("(5000 chars)"), "{:?}", fields[0].1);
        assert!(!fields[0].1.contains('\n'));
    }

    /// Newlines are flattened, because a "field" that is forty rows tall stops being one.
    #[test]
    fn multi_line_values_stay_one_line() {
        let args = complete(&serde_json::json!({ "cmd": "a\nb\nc" }).to_string());
        assert_eq!(argument_fields(&args)[0].1, "a⏎b⏎c");
    }

    /// The second artifact: `Edit` arrives with its before and after as *fields*, so the
    /// diff needs no parsing out of prose.
    #[test]
    fn edit_becomes_a_diff() {
        let args = complete(
            &serde_json::json!({
                "file_path": "src/lib.rs",
                "old_string": "let a = 1;\nlet b = 2;",
                "new_string": "let a = 1;\nlet b = 3;\nlet c = 4;",
            })
            .to_string(),
        );
        let diff = edit_diff(Some("Edit"), &args).expect("a diff");
        assert_eq!(diff.path, "src/lib.rs");
        assert_eq!(diff.removed, vec!["let a = 1;", "let b = 2;"]);
        assert_eq!(diff.added, vec!["let a = 1;", "let b = 3;", "let c = 4;"]);
    }

    /// …and only for `Edit`, only once settled, and never on a shape that lacks them.
    #[test]
    fn nothing_else_is_mistaken_for_a_diff() {
        let args = complete(r#"{"file_path":"a.txt"}"#);
        assert!(edit_diff(Some("Read"), &args).is_none());
        assert!(edit_diff(Some("Edit"), &args).is_none(), "no old/new means no diff");
        let streaming = Arguments { text: r#"{"old_string":"a","new_"#.into(), complete: false };
        assert!(edit_diff(Some("Edit"), &streaming).is_none(), "never parse a fragment");
    }

    /// A local command is recognised **exactly**, because the cost of the two mistakes is
    /// wildly asymmetric: failing to recognise one sends a slash-word to the agent, which is
    /// merely odd, while over-recognising one swallows a real message — and the composer
    /// clears either way, so the human watches their sentence vanish into nothing.
    #[test]
    fn only_an_exact_slash_panel_is_a_local_command() {
        assert_eq!(local_command("/panel"), Some(LocalCommand::Panel));
        assert_eq!(local_command("  /panel \n"), Some(LocalCommand::Panel), "trimmed like a send");
        assert_eq!(local_command("/surface"), Some(LocalCommand::Surface));
        assert_eq!(local_command(" /surface  "), Some(LocalCommand::Surface));
        for message in [
            "/panels",
            "/panel now",
            "what does /panel do?",
            "panel",
            "/surfaces",
            "/surface slate",
            "/",
            "",
        ] {
            assert_eq!(local_command(message), None, "{message:?} belongs to the agent");
        }
    }

    /// The knobs start where the console said they start, so the two front-ends draw one
    /// instrument rather than two that resemble each other.
    #[test]
    fn a_panels_widget_state_comes_from_its_description() {
        let defaults = default_slider_table();
        let spec = PanelSpec {
            sliders: vec!["bloom".into(), "unheard-of".into()],
            buttons: vec!["metal".into()],
            drives: None,
        };
        let mut state = PanelState::default();
        state.sync(&spec, &defaults);
        assert_eq!(state.sliders.len(), 2);
        assert_eq!(state.sliders[0], initial_value("bloom", &defaults), "from the table");
        assert!(DEFAULT_SLIDERS.iter().any(|(l, v)| *l == "bloom" && *v == state.sliders[0]));
        assert_eq!(state.sliders[1], 0.5, "an unknown control still gets a sane start");

        // A dragged value survives a re-sync — the description did not change, so nothing
        // may reach in and reset it. This is the "snaps back mid-drag" failure, headless.
        state.sliders[0] = 0.9;
        state.sync(&spec, &defaults);
        assert_eq!(state.sliders[0], 0.9);
    }

    /// A re-sync forced by a *changed* description rebuilds the knobs — and must not throw
    /// away the material the surface is currently wearing, which would repaint the picture
    /// for a reason nobody asked for.
    #[test]
    fn a_resync_rebuilds_the_knobs_and_keeps_the_material() {
        let defaults = default_slider_table();
        let mut state = PanelState { sliders: vec![0.9], material: Some("metal".into()) };
        let wider = PanelSpec {
            sliders: vec!["bloom".into(), "drift".into()],
            buttons: Vec::new(),
            drives: Some(ElementId(7)),
        };
        state.sync(&wider, &defaults);
        assert_eq!(state.sliders.len(), 2, "the description won");
        assert_eq!(state.material.as_deref(), Some("metal"), "the surface's look survived");
    }

    fn rect(top: f32, bottom: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(0.0, top), egui::pos2(400.0, bottom))
    }

    /// The cheap half of the cap: a conversation is a tall column and almost every surface
    /// it ever summoned is off screen, so this is what stops the render list growing with the
    /// transcript.
    #[test]
    fn only_surfaces_overlapping_the_viewport_are_rendered() {
        let viewport = rect(100.0, 500.0);
        assert!(surface_visible(rect(200.0, 400.0), viewport), "wholly inside");
        assert!(surface_visible(rect(50.0, 150.0), viewport), "clipped at the top");
        assert!(surface_visible(rect(450.0, 700.0), viewport), "clipped at the bottom");
        assert!(surface_visible(rect(0.0, 900.0), viewport), "taller than the viewport");

        assert!(!surface_visible(rect(0.0, 100.0), viewport), "resting on the top edge");
        assert!(!surface_visible(rect(500.0, 600.0), viewport), "resting on the bottom edge");
        assert!(!surface_visible(rect(600.0, 800.0), viewport), "far below");
        assert!(!surface_visible(rect(-900.0, -10.0), viewport), "far above");
        assert!(!surface_visible(rect(200.0, 200.0), viewport), "a collapsed rect is nothing");
    }

    fn laid_out(id: u64, look: &str) -> LaidOutSurface {
        LaidOutSurface {
            element: ElementId(id),
            look: look.to_string(),
            size_points: (400.0, SURFACE_HEIGHT),
        }
    }

    /// **The wiring, headless.** A panel's controls reach the surface it names, and only
    /// that one; an unnamed surface keeps the look it was summoned with.
    #[test]
    fn a_driving_panel_reaches_its_own_surface_and_no_other() {
        let surfaces = join_drives(
            vec![laid_out(1, "graphite"), laid_out(2, "graphite")],
            vec![PanelDrive {
                target: ElementId(2),
                material: Some("metal".into()),
                sliders: vec![("light".into(), 0.8)],
            }],
        );
        assert_eq!(surfaces.len(), 2);
        assert_eq!(surfaces[0].look, "graphite", "the undriven surface kept its summoning look");
        assert!(surfaces[0].sliders.is_empty());
        assert_eq!(surfaces[1].look, "metal");
        assert_eq!(surfaces[1].sliders, vec![("light".to_string(), 0.8)]);
    }

    /// A panel whose target is off screen — or whose target the cap evicted — contributes
    /// nothing, and specifically does not conjure a request for a surface that was never
    /// laid out. That request would be a render with no rect to size it.
    #[test]
    fn a_driver_with_no_visible_target_renders_nothing() {
        let surfaces = join_drives(
            Vec::new(),
            vec![PanelDrive {
                target: ElementId(9),
                material: Some("paper".into()),
                sliders: vec![("light".into(), 0.1)],
            }],
        );
        assert!(surfaces.is_empty());
    }

    /// A panel nobody has pressed a button in still drives the sliders. Material and knobs
    /// are independent, so a first drag must not have to be preceded by a click.
    #[test]
    fn sliders_drive_before_any_button_is_pressed() {
        let surfaces = join_drives(
            vec![laid_out(3, "slate")],
            vec![PanelDrive {
                target: ElementId(3),
                material: None,
                sliders: vec![("exposure".into(), 0.75)],
            }],
        );
        assert_eq!(surfaces[0].look, "slate", "no button pressed, no material change");
        assert_eq!(surfaces[0].sliders, vec![("exposure".to_string(), 0.75)]);
    }

    /// Clipping is the VIEW's, and it reports what it hid — the model keeps the whole
    /// output, so "10 lines" must never be mistaken for "that was all of it".
    #[test]
    fn output_clipping_reports_what_it_hid() {
        let text = (1..=25).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        let (shown, hidden) = clip_lines(&text, 10);
        assert_eq!(shown.len(), 10);
        assert_eq!(shown[0], "1");
        assert_eq!(hidden, 15);
        let (all, none) = clip_lines("one\ntwo", 10);
        assert_eq!(all, vec!["one", "two"]);
        assert_eq!(none, 0);
    }
}
