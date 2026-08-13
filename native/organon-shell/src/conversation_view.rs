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
use std::sync::mpsc::Receiver;
use std::time::Instant;

use egui::{Color32, CornerRadius, Frame, Margin, RichText};

use crate::agent_event::{EventKind, ModelChoice};
use crate::agent_map::{ContextFill, EventMapper, SessionFacts};
use crate::agent_session::{AgentSession, Control, McpWiring, StreamItem};
use crate::approval::{
    approval_channel, decision_for, decision_key, resolve_choice, Choice, DecisionMemory,
    PendingApproval,
};
use crate::block_panel::{DEFAULT_SLIDERS, PAD, PANEL_EDGE, PANEL_FILL, PANEL_TITLE, SLIDER_WIDTH};
use crate::conversation::{
    Answer, ApprovalBlock, ApprovalState, Arguments, ArtifactBlock, ArtifactContent, Body, Change,
    Element, ElementId, Ignored, PanelSpec, RunOutcome, StepState, SubagentAct, SubagentLog,
    SurfaceSpec, ToolCard, ToolState, Transcript, Verdict,
};
use crate::mcp::{McpServer, NoDispatch};
use crate::mcp_http::{mcp_config_json, ConfigFile, McpHttp};
use crate::timeline::pinned_after_scroll;

/// The console's MCP `serverInfo.name`, and therefore the middle of every namespaced tool
/// name Claude Code spells: `mcp__organon__…`.
pub const SERVER_NAME: &str = "organon";

const HUMAN: Color32 = Color32::from_rgb(0xc8, 0xe6, 0xc8);
const PROSE: Color32 = Color32::from_rgb(0xd2, 0xd8, 0xd2);
const DIM: Color32 = Color32::from_rgb(0x70, 0x7c, 0x70);
const RUNNING: Color32 = Color32::from_rgb(0xe6, 0xc0, 0x4c);
/// A question waiting on a human. Deliberately not [`RUNNING`]'s amber: "the agent is busy"
/// and "the agent is blocked on you" must not read as the same state at a glance, which is
/// the whole complaint against a tool that bounces silently.
const ASKING: Color32 = Color32::from_rgb(0x7f, 0xb8, 0xe6);
const OK: Color32 = Color32::from_rgb(0x6f, 0xc2, 0x76);
const BAD: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x5f);

/// How much of a tool's output a card draws before it says how much it is not drawing.
const OUTPUT_LINES: usize = 10;
/// How many of a subagent's most recent steps a card draws.
///
/// The **tail**, not the head: the question a running `Task` card answers is "what is it
/// doing now", and the transcript keeps far more than this
/// ([`crate::conversation::Limits::max_subagent_steps`]). Everything not drawn is counted
/// on the line above it, so the card never quietly implies this was all of it.
const SUBAGENT_LINES: usize = 6;
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
/// Still a struct rather than a bare `Vec`, because what it carries is a *render list for
/// the next frame* and the name is what says so at the call site in `shell_main.rs`.
///
/// ⚠️ **One field, and it used to be two.** The other was `actions` — buttons pressed in a
/// panel that drove the console's backdrop, which only `/panel` could produce. With that
/// command retired every panel drives a surface in this same transcript, so a press is
/// consumed by the thing it aims at and never leaves this crate.
#[derive(Clone, Debug, Default)]
pub struct ConversationOutput {
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

/// A composer line the view acts on itself and **never sends to the agent**.
///
/// ⚠️ **A temporary seam, and shaped to be removed.** Summoning is about to become the
/// agent's job — a tool call the integrator answers with
/// [`Transcript::insert_artifact`](crate::conversation::Transcript::insert_artifact), where
/// the tool card is the anchor — so the summoning path is kept entirely separate from the
/// element: this enum decides *that* a surface is wanted, [`ConversationPane::summon_surface`]
/// builds one, and neither knows about the other's existence beyond that call. Deleting
/// this enum and its branch in [`ConversationPane::submit`] removes the local command and
/// touches nothing that draws.
///
/// 🚨 **There was a second command here, `/panel`, and it is gone.** It summoned a panel
/// wired to the *console* — the backdrop behind whatever terminal tab was next door — so
/// its controls changed something you could not see from the tab you clicked in. Driven by
/// a human, that reads as a panel whose knobs do nothing. `/surface` is not an alternative
/// to it; it is the same panel pointed at something in the same view, which is what makes
/// the instrument legible. Removing `/panel` took the console-driving arm of
/// [`PanelSpec::drives`] with it, so a panel now always names a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalCommand {
    /// `/surface` — put a rendered surface in the flow, and a panel under it that drives
    /// **that surface**. Control and consequence in one glance.
    Surface,
}

/// Recognise a local command, or `None` for an ordinary message.
///
/// Exact-match only: a message that merely *starts* with a slash is a message (a human
/// asking about `/surface` must reach the agent), and swallowing it would be a silent send
/// failure — the worst kind, because the composer clears either way.
pub fn local_command(line: &str) -> Option<LocalCommand> {
    match line.trim() {
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

/// What one pane keeps alive so its agent can ask it for permission.
///
/// Both fields are held for the pane's lifetime and read by nobody: dropping the server
/// stops it, and dropping the config deletes the file the agent was started with. That is
/// the entire contract, which is why they are underscored — a tab closing must take its
/// loopback port and its temp file with it.
struct ApprovalWiring {
    _server: McpHttp,
    _config: ConfigFile,
}

/// Start the console's MCP server and write the config the agent will be spawned with.
///
/// Returns what to hold, what to pass to the agent, and where the questions will arrive.
/// Fails as a string rather than an error type because there is exactly one caller and one
/// thing it does with a failure: say so, and run the agent unwired.
fn start_approvals(
) -> Result<(ApprovalWiring, McpWiring, Receiver<PendingApproval>), String> {
    let (gate, inbox) = approval_channel();
    // 🚨 **An empty spec table on purpose.** With no capability tools this server offers
    // exactly one tool, the permission handler — and Claude Code withholds *that* from the
    // model because `--permission-prompt-tool` names it (§7). So the model sees a connected
    // server with nothing it can call, which is the safe shape for an approvals tier.
    // Serving the console's verbs needs a `CommandService`, which borrows the session log
    // on the UI thread; that is a wiring, and it is not this one.
    let server = McpServer::new(&[], Box::new(gate)).with_server_name(SERVER_NAME);
    let permission_tool = server.permission_tool_flag_value();
    let http = McpHttp::start(server, Box::new(NoDispatch))
        .map_err(|e| format!("could not bind a loopback port: {e}"))?;
    let json = mcp_config_json(SERVER_NAME, &http.url());
    let config =
        ConfigFile::write(&std::env::temp_dir(), &ConfigFile::stem_for(http.port()), &json)
            .map_err(|e| format!("could not write the MCP config: {e}"))?;
    let wiring = McpWiring { config: config.path().to_path_buf(), permission_tool };
    Ok((ApprovalWiring { _server: http, _config: config }, wiring, inbox))
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
    /// How tall the composer's text was when it was last laid out, in points.
    ///
    /// ⚠️ **This is deliberate carried state, and it is here because the obvious version
    /// does not work** — see [`composer_box`]. A vertical [`egui::ScrollArea`] placed
    /// directly in a [`egui::Layout::bottom_up`] column anchors itself at the *top* of the
    /// remaining space, which collapses the column and leaves the scrollback nothing. The
    /// composer therefore reserves its band explicitly, and the only honest source for that
    /// band's height is what the text measured last frame. Growth lands one frame late,
    /// which is the same trade egui's own panels make.
    composer_height: f32,
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
    /// The MCP server this pane's agent asks for permission, and the config it was started
    /// with. Held, never read — see [`ApprovalWiring`]. `None` means the server could not
    /// start; the agent then runs unwired and the log says so.
    _approvals: Option<ApprovalWiring>,
    /// Where permission requests arrive from the serve thread.
    inbox: Receiver<PendingApproval>,
    /// The questions a human has not answered yet, by the element that draws them.
    ///
    /// ⚠️ **Removing an entry without answering it denies that call**, because the reply
    /// channel goes with it ([`crate::approval`]). That is what makes an approval the
    /// transcript's cap evicted fail closed instead of blocking the agent forever.
    waiting: HashMap<ElementId, PendingApproval>,
    /// "Allow and remember", which is entirely the console's — there is no upstream
    /// persistence (§5). Session-scoped: it lives here and dies with the tab.
    memory: DecisionMemory,
    /// The selectable models this account was offered, from the one `initialize` the
    /// session asks at spawn ([`crate::agent_session::Control::Initialize`]).
    ///
    /// 🚨 **Empty until it arrives, and never filled in from anywhere else.** The list is
    /// per-account and carries display names written for humans; a hardcoded table here
    /// would be a menu that is wrong for somebody, silently, on a model the CLI added
    /// after this build shipped.
    models: Vec<ModelChoice>,
    /// A model change that has been asked for and not yet confirmed. See
    /// [`PendingModel`] — this is what stops the plate asserting a model that is not yet
    /// true.
    pending_model: Option<PendingModel>,
}

/// A `set_model` in flight, from the click to the moment the strip's own model fact moves.
///
/// 🚨 **This type is the answer to "the plate must not lie during the switch".** The ack
/// for `set_model` carries **no body at all** (§2), so it says the request was accepted and
/// nothing about what the session is now running; the new model is stated only by the
/// *repeat* `system/init` that follows. Between the click and that init the console knows
/// what it asked for and not what it got — so the plate keeps showing the **confirmed**
/// model and carries this alongside as a marked, obviously-unsettled annotation. Nothing
/// here is ever promoted into [`SessionFacts::model`].
#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingModel {
    /// What the picker called the row that was clicked — the human-written `displayName`
    /// when the CLI sent one. Shown, so the annotation names the destination.
    label: String,
    /// The model the strip was reporting when the request went out.
    ///
    /// ⚠️ The confirmation test is "has this moved", **not** "does it equal what we asked
    /// for": `set_model` takes an alias and the session reports a resolved id, and the
    /// resolution table is the CLI's, not ours. Matching on a predicted string would leave
    /// the marker stuck for every alias this build has not met.
    was: Option<String>,
}

/// Has an unconfirmed model change landed?
///
/// The whole of it: the strip's own model fact is no longer what it was when the request
/// went out. Named rather than inlined because it is the one line that decides whether the
/// plate is telling the truth.
pub fn model_change_landed(was: Option<&str>, now: Option<&str>) -> bool {
    was != now
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
    /// The MCP server starts **before** the agent, necessarily: the agent is spawned with a
    /// config file naming a port that has to exist first. A server that will not start is
    /// not fatal — the tab opens, the agent runs, and the log says that a tool needing
    /// permission will fail rather than ask.
    pub fn new(cwd: Option<&str>, buttons: Vec<String>, sliders: Vec<(String, f32)>) -> Self {
        let mut log = VecDeque::new();
        let (approvals, wiring, inbox) = match start_approvals() {
            Ok((held, wiring, inbox)) => (Some(held), Some(wiring), inbox),
            Err(error) => {
                log.push_back(format!(
                    "approvals are not wired ({error}) — a tool that needs permission will \
                     fail instead of asking"
                ));
                // A dead channel rather than an `Option<Receiver>`: the drain already
                // treats a disconnected inbox as "nothing to read", so the unwired case
                // needs no second code path.
                let (_gate, inbox) = approval_channel();
                (None, None, inbox)
            }
        };
        let (session, failure) = match AgentSession::spawn(cwd, wiring.as_ref()) {
            Ok(session) => (Some(session), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            session,
            transcript: Transcript::new(),
            mapper: EventMapper::new(),
            failure,
            composer: String::new(),
            log,
            pinned: true,
            want_focus: true,
            composer_height: 0.0,
            artifacts: HashMap::new(),
            buttons,
            sliders,
            _approvals: approvals,
            inbox,
            waiting: HashMap::new(),
            memory: DecisionMemory::new(),
            models: Vec::new(),
            pending_model: None,
        }
    }

    /// What the console has been asked to remember, for whoever wants to show or audit it.
    pub fn memory(&self) -> &DecisionMemory {
        &self.memory
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
        // Approvals first, and unconditionally: a question can outlive the process that
        // asked it, and a pane whose agent has gone must still be able to draw — and
        // therefore fail closed — the request that was in flight.
        let mut changed = self.pump_approvals();
        changed |= self.retire_unanswered_controls();
        let Some(session) = self.session.as_mut() else { return changed };
        let items = session.pump();
        if items.is_empty() {
            return changed;
        }
        // Acks are set aside and answered after the fold rather than inside it: resolving
        // one needs the session, and the fold needs `note`, which is `&mut self`. Same
        // frame either way — the strip is drawn once, after all of this.
        let mut acks = Vec::new();
        for item in items {
            match item {
                StreamItem::Event(event) => {
                    if let EventKind::ControlResponse(response) = &event.kind {
                        acks.push(response.clone());
                    }
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
                    // Nothing will read an answer now. Say so on the cards rather than
                    // leaving them asking.
                    self.abandon_all_approvals();
                    changed = true;
                }
            }
        }
        for response in acks {
            changed |= self.receive_control(&response);
        }
        // The model plate's confirmation, and the only one there is: a repeat
        // `system/init` has restated the model and the mapper has taken it (rule 3's
        // amendment). Checked after the fold, so a click and its confirming init landing
        // in one pump settle in that pump.
        if let Some(pending) = &self.pending_model {
            if model_change_landed(pending.was.as_deref(), self.mapper.facts().model.as_deref()) {
                self.pending_model = None;
                changed = true;
            }
        }
        changed
    }

    /// Ask the session to change something about itself, and say so in the log if the pipe
    /// will not take it.
    ///
    /// Returns whether the request went out. **Nothing is gated on the answer** — see
    /// [`crate::agent_session::CONTROL_DEADLINE`].
    fn send_control(&mut self, control: Control) -> bool {
        let Some(session) = self.session.as_mut() else {
            self.note("the agent is not running — nothing to ask".to_string());
            return false;
        };
        let described = control.describe();
        match session.send_control(control) {
            Ok(_) => true,
            Err(e) => {
                self.note(format!("could not send {described}: {e}"));
                false
            }
        }
    }

    /// 🚨 **The no-reply path.** A control request nobody answers is retired at the
    /// deadline: the log says which one, and a plate marked as switching stops being
    /// marked. Nothing was waiting on it, so there is nothing to unblock — the whole point
    /// of this being a sweep rather than a wait.
    fn retire_unanswered_controls(&mut self) -> bool {
        let now = Instant::now();
        let abandoned = match self.session.as_mut() {
            Some(session) => session.give_up_on_controls(now),
            None => Vec::new(),
        };
        let mut changed = false;
        for control in abandoned {
            self.note(format!(
                "no answer to {} — the console has stopped waiting for one",
                control.describe()
            ));
            if matches!(control, Control::SetModel(_)) && self.pending_model.is_some() {
                // The plate goes back to reporting only what was confirmed, which is what
                // it was doing all along — the annotation is what is dropped, not a value.
                self.pending_model = None;
            }
            changed = true;
        }
        changed
    }

    /// One control response, matched back to the verb this console asked for.
    ///
    /// 📌 The mapper deliberately records **no fact** from an ack — it never issued the
    /// request and cannot tell which verb a `request_id` answers. This is the other end of
    /// that: the only place that correlation exists, and therefore the only place an ack
    /// can mean anything.
    fn receive_control(&mut self, response: &crate::agent_event::ControlResponse) -> bool {
        let Some(control) = self.session.as_mut().and_then(|s| s.resolve_control(response)) else {
            // Not ours: no id, or one the deadline already retired. Expected, not a fault.
            return false;
        };
        if let Some(error) = response.error() {
            // The CLI's own sentence, verbatim — it says *why* in language written for a
            // human, and paraphrasing it would lose the only useful part.
            let described = control.describe();
            self.note(format!("{described} was refused: {error}"));
            if matches!(control, Control::SetModel(_)) {
                self.pending_model = None;
            }
            return true;
        }
        match control {
            Control::Initialize => {
                let models = response.models();
                let found = models.len();
                self.models = models;
                self.note(format!("the session offers {found} models"));
            }
            // ⚠️ Nothing is confirmed here. The ack has no body (§2), so it says the
            // request was accepted and not what the session is now running; the repeat
            // `system/init` is what settles the plate, and `pump` watches for it.
            Control::SetModel(_) => {}
            Control::SetPermissionMode(asked) => {
                // Unlike `set_model` this verb *does* state its result, so the console can
                // confirm rather than assume — and say so when the two disagree, which
                // would mean the session ended up somewhere nobody chose.
                match response.mode() {
                    Some(got) if got == asked => {}
                    Some(got) => self.note(format!(
                        "asked for permission mode {asked}, the session reports {got}"
                    )),
                    None => self.note(format!(
                        "permission mode {asked} was accepted without confirming itself"
                    )),
                }
            }
        }
        true
    }

    /// The models this session offered, for the plate's picker.
    pub fn models(&self) -> &[ModelChoice] {
        &self.models
    }

    /// Ask for a different model, and mark the plate as unsettled until the session says
    /// otherwise.
    ///
    /// ⚠️ **A row that is already current is a no-op**, deliberately: `set_model` to the
    /// model already running produces an ack and no repeat init, so the marker would have
    /// nothing to clear it and would sit there until the deadline. Nothing changes, and
    /// nothing claims to have.
    fn choose_model(&mut self, row: &ModelRow) {
        if row.current {
            return;
        }
        let was = self.mapper.facts().model.clone();
        if self.send_control(Control::SetModel(row.value.clone())) {
            self.pending_model = Some(PendingModel { label: row.label.clone(), was });
        }
    }

    /// Ask for a different permission mode.
    ///
    /// No pending marker: this verb's ack states its own result **and** the CLI emits a
    /// dedicated `system/status` line carrying the new mode, which the mapper already
    /// reads. The mode plate is therefore never asserting anything unconfirmed, and does
    /// not need the machinery [`PendingModel`] exists for.
    fn choose_permission_mode(&mut self, mode: &str) {
        self.send_control(Control::SetPermissionMode(mode.to_string()));
    }

    /// Take every permission request the serve thread has posted since the last frame, and
    /// put each one in the flow.
    ///
    /// Never blocks: the *hook* blocks, on its own thread, and this end only ever picks up
    /// what is already there. A disconnected inbox is the unwired case and reads as empty.
    fn pump_approvals(&mut self) -> bool {
        let mut changed = false;
        while let Ok(pending) = self.inbox.try_recv() {
            self.receive_approval(pending);
            changed = true;
        }
        changed | self.sweep_abandoned()
    }

    /// 🚨 **Close every question the agent has stopped waiting for.**
    ///
    /// The serve thread notices the client is gone and marks its [`PendingApproval`]
    /// abandoned ([`crate::approval`]); this is the other end of that flag. Without it the
    /// card keeps offering *allow* for a call that already failed with *"The operation
    /// timed out"* — the exact thing a human saw on screen, and worse than showing no card,
    /// because it invites a click that cannot do anything.
    ///
    /// A sweep rather than a message: the flag is cheap to read, the map is a handful of
    /// entries, and there is no ordering to get wrong.
    fn sweep_abandoned(&mut self) -> bool {
        let dead: Vec<ElementId> = self
            .waiting
            .iter()
            .filter(|(_, pending)| pending.is_abandoned())
            .map(|(id, _)| *id)
            .collect();
        let mut changed = false;
        for id in dead {
            self.waiting.remove(&id);
            changed |= self.transcript.abandon_approval(id) != Change::Ignored(Ignored::ApprovalAlreadyAnswered);
        }
        changed
    }

    /// Close every open question at once — the agent that would have answered them is gone.
    ///
    /// ⚠️ **Removing an entry from `waiting` is what denies it**, so this both unblocks any
    /// serve thread still holding one and leaves the card saying what happened. Called when
    /// the process ends, which is the other way a question dies without anyone clicking:
    /// the client never gets to time out because there is no client.
    fn abandon_all_approvals(&mut self) -> bool {
        let open: Vec<ElementId> = self.waiting.keys().copied().collect();
        let mut changed = false;
        for id in open {
            self.waiting.remove(&id);
            changed |= self.transcript.abandon_approval(id) != Change::Ignored(Ignored::ApprovalAlreadyAnswered);
        }
        changed
    }

    /// One question: answered from memory if it has been decided before, otherwise a card.
    ///
    /// **A remembered decision still renders.** It would be cheaper to answer it silently
    /// and say nothing, and that is exactly the failure the memory has to avoid: an
    /// authority the human granted once and can no longer see is worse than being asked
    /// every time.
    fn receive_approval(&mut self, pending: PendingApproval) {
        let tool_name = pending.request().tool_name.clone();
        let tool_use_id = pending.request().tool_use_id.clone();
        let input = pending.request().input.to_string();

        if let Some(verdict) = self.memory.lookup(&tool_name, &input) {
            let decision = decision_for(verdict, pending.request());
            self.transcript.insert_approval(ApprovalBlock {
                tool_name,
                input,
                tool_use_id,
                state: ApprovalState::Answered(Answer {
                    verdict,
                    from_memory: true,
                    remembered: true,
                }),
            });
            pending.answer(decision);
        } else {
            let change = self.transcript.insert_approval(ApprovalBlock {
                tool_name,
                input,
                tool_use_id,
                state: ApprovalState::Pending,
            });
            match change {
                Change::Appended(id) => {
                    self.waiting.insert(id, pending);
                }
                // `insert_approval` appends unconditionally, so this is unreachable today.
                // Dropping `pending` here denies the call, which is the correct answer to
                // "the console cannot show this question".
                _ => {}
            }
        }
        // A question is the one thing that must never scroll past unseen.
        self.pinned = true;
    }

    /// Put a control panel in the flow, at the end of what has been said so far, driving
    /// the element `drives` names.
    ///
    /// The **only** thing that builds one, so the summoning path above it — today a local
    /// command, next a tool call — is a caller rather than a participant.
    ///
    /// ⚠️ `drives` was an `Option` while `/panel` existed, and `None` meant "drive the
    /// console's backdrop". That is the arm that was removed: a panel must now name
    /// something in this transcript, so it cannot be summoned into a state where its
    /// controls change something outside the view they are drawn in.
    pub fn summon_panel(&mut self, drives: ElementId) {
        let spec = PanelSpec {
            sliders: self.sliders.iter().map(|(l, _)| l.clone()).collect(),
            buttons: self.buttons.clone(),
            drives,
        };
        self.transcript.insert_artifact(ArtifactBlock {
            title: "◈ organon · surface controls".to_string(),
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
            // in the transcript. There is no target to drive, and a panel that drives
            // nothing is what this whole change removed — so the surface stands alone.
            self.note("the surface could not be given controls".to_string());
            return;
        };
        self.summon_panel(id);
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
/// Bottom-up, because the composer's and the strip's heights are known and the scrollback's
/// is whatever is left — the layout every chat client resolves in that order.
///
/// ⚠️ **The order of these four calls is the visual order, upside down.** In a bottom-up
/// column the *first* thing added sits lowest, so this reads: strip at the very bottom,
/// composer above it, a rule, and the scrollback taking everything that is left. The status
/// used to be added between the composer and the rule, which put it *above* the composer —
/// where a one-line band with a rule under it reads as a divider rather than as the thing it
/// is. [`status_strip`] belongs with the composer, at the bottom, which is where Claude
/// Desktop puts the model affordance and where a hand looking for it goes.
pub fn draw(
    ui: &mut egui::Ui,
    pane: &mut ConversationPane,
    images: &SurfaceImages,
) -> ConversationOutput {
    let mut out = ConversationOutput::default();
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        status_strip(ui, pane);
        ui.add_space(4.0);
        composer(ui, pane);
        ui.add_space(4.0);
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
    // Collected during the walk and joined after it. A panel is *below* its surface in the
    // ordinary case, but nothing in the model requires that — the link is an id, not an
    // adjacency — so the join happens once the whole list has been seen rather than by
    // reaching backwards mid-loop.
    let mut laid_out: Vec<LaidOutSurface> = Vec::new();
    let mut drives: Vec<PanelDrive> = Vec::new();
    // The transcript is walked immutably, so a decision taken mid-walk is applied after it.
    // The *agent* is not made to wait for that: the verdict goes back on the wire inside the
    // loop, the moment the button is read.
    let mut answered: Vec<(ElementId, Answer)> = Vec::new();
    let mut revoked: Vec<ElementId> = Vec::new();
    // Destructured so the transcript can be read while the widget state is written: they
    // are disjoint fields, and keeping them disjoint is the whole point of the side map.
    let ConversationPane {
        transcript, artifacts, pinned, sliders: defaults, waiting, memory, ..
    } = pane;
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
                        "no messages yet — type below and press Enter, or `/surface` for a \
                         rendered surface with its own controls",
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
                            panel_element(ui, element.id, artifact, spec, state, defaults);
                            // A press is consumed here: it changes the surface this panel
                            // names, and nothing else. That is the whole of what a panel
                            // can do now — the arm that also repainted the console's
                            // backdrop went with `/panel`.
                            drives.push(PanelDrive {
                                target: spec.drives,
                                material: state.material.clone(),
                                sliders: spec
                                    .sliders
                                    .iter()
                                    .cloned()
                                    .zip(state.sliders.iter().copied())
                                    .collect(),
                            });
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
                    // The second body drawn here rather than in `draw_element`, and for a
                    // sharper version of the same reason: answering one needs the question
                    // the pane is holding, which `draw_element` has no access to.
                    Body::Approval(block) => {
                        let live = waiting.contains_key(&element.id);
                        match approval_card(ui, element.id, block, live) {
                            Some(CardAct::Choose(choice)) => {
                                // Removing it is what answers it — see `waiting`.
                                if let Some(pending) = waiting.remove(&element.id) {
                                    let (decision, answer) =
                                        resolve_choice(choice, pending.request(), memory);
                                    answered.push((element.id, answer));
                                    pending.answer(decision);
                                }
                            }
                            Some(CardAct::Forget) => revoked.push(element.id),
                            None => {}
                        }
                    }
                    _ => draw_element(ui, element),
                }
                ui.add_space(8.0);
            }
        });
    *pinned = pinned_after_scroll(out.state.offset.y, out.content_size.y, out.inner_rect.height());
    for (id, answer) in answered {
        transcript.answer_approval(id, answer);
    }
    for id in revoked {
        // The key is re-derived from the card, which is what makes revocation possible from
        // the card a human is actually looking at rather than from a list somewhere else.
        if let Some(block) = transcript.get(id).and_then(|e| e.approval()) {
            let key = decision_key(&block.tool_name, &block.input);
            memory.forget(&key);
        }
        transcript.revoke_approval(id);
    }
    // State outlives its element for exactly as long as it takes to notice. The transcript
    // evicts from the front and `get` answers `None` for an evicted id, so this is a
    // one-line answer to "does the side map leak on a long session" — it does not.
    artifacts.retain(|id, _| transcript.get(*id).is_some());
    // ⚠️ The same line, with teeth: a question whose element the cap evicted can never be
    // answered by a human, so dropping it here **denies** it (`crate::approval`) instead of
    // leaving the agent blocked for the rest of the session.
    waiting.retain(|id, _| transcript.get(*id).is_some());
    ConversationOutput { surfaces: join_drives(laid_out, drives) }
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
            //
            // ⚠️ **The caret is `|`, and it must stay a character the PROPORTIONAL font
            // has.** It was `▍` (U+258D, Left Five Eighths Block), which egui's
            // proportional face does not carry — so every streaming reply ended in a
            // tofu box. The mono fix used elsewhere in this file is not available here:
            // the caret is concatenated into the prose, and the prose is proportional on
            // purpose. So the glyph changes instead of the face.
            let text = if a.complete { a.text.clone() } else { format!("{}|", a.text) };
            ui.label(RichText::new(text).color(PROSE));
        }
        Body::Tool(card) => tool_card(ui, card),
        // Drawn by `scrollback`, which holds the widget state a panel needs between
        // frames — and, for an approval, the question itself. Nothing to do here, and
        // nothing missing: an element is drawn exactly once, by whichever of the two has
        // what it needs.
        Body::Artifact(_) | Body::Approval(_) => {}
        Body::RunEnd(end) => {
            let (label, color) = match end.outcome {
                RunOutcome::Ok => ("turn complete", DIM),
                RunOutcome::Error => ("turn failed", BAD),
                RunOutcome::Cancelled => ("turn cancelled", RUNNING),
            };
            let detail = end.detail.as_deref().unwrap_or("");
            ui.horizontal(|ui| {
                // ⚠️ **`—` (U+2014), not `──` (two U+2500 box-drawing dashes).** This is
                // the site James saw drawn as two tofu boxes: egui's proportional face
                // carries no box drawing. The rest of this file answers that by switching
                // to the mono face, but a rule leading into small dim proportional text
                // does not want to be monospace — so this one takes a glyph the
                // proportional font actually has. An em dash is the same mark a
                // typesetter would have reached for anyway.
                ui.label(RichText::new(format!("— {label}")).color(color).small());
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

            if !card.subagent.is_empty() {
                subagent_body(ui, &card.subagent, card.state.is_running());
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

/// What a subagent card's header says, as text — the judgment, without the egui.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubagentSummary {
    /// How much happened. Always present when there is a log at all.
    pub counts: String,
    /// Tool steps that never came back, phrased for whether the parent is still running.
    /// `None` when there are none.
    pub open: Option<String>,
}

/// **What the header of a subagent log claims, and the two things it must not claim.**
///
/// 🚨 It reports **counts**, never liveness. §5.9.1 measured that no token deltas are
/// forwarded from a subagent, so the console cannot know whether one is thinking or has
/// silently died; the gaps between bursts are real and minutes long. A header that said
/// "working" would be inventing the one fact this whole path does not have.
///
/// ⚠️ And an open step reads differently depending on the **parent**, which is the only
/// thing that does carry liveness (behaviour 1: running-ness is a derived unresolved id).
/// While the parent runs, an unreturned step is work in flight. Once the parent has come
/// back, the same number means the subagent stopped without those tools ever returning —
/// worth seeing, and not the same sentence.
pub fn subagent_summary(log: &SubagentLog, parent_running: bool) -> SubagentSummary {
    let total = log.len() as u64 + log.dropped;
    let unit = if total == 1 { "step" } else { "steps" };
    let mut counts = format!("· {total} {unit}");
    // Depth stays silent in the ordinary case and speaks when it is not, because a
    // flattened depth-2 step read as direct misattributes who did the work.
    if let Some(depth) = log.max_depth().filter(|d| *d > 1) {
        counts.push_str(&format!(" · nested {depth} deep"));
    }
    let open = match (log.unreturned(), parent_running) {
        (0, _) => None,
        (n, true) => Some(format!("· {n} out")),
        (n, false) => Some(format!("· {n} never returned")),
    };
    SubagentSummary { counts, open }
}

/// **What a subagent is doing, inside the card that dispatched it.**
///
/// Before this, a coordinator session that fanned out showed a `Task` card sitting on
/// "running" for eight to sixteen minutes and then a wall of text — the agent's whole
/// working life reduced to a spinner. The events were there all along; §5.9.3 rule 5 was
/// dropping them because they had nowhere to go that was not a turn belonging to nobody.
///
/// 🚨 **There is no live text here, and this function must never grow any.** §5.9.1
/// measured that Claude Code does not forward token deltas from a subagent: every line
/// below is a completed fact that arrived in a burst, and the gaps between bursts are
/// real and can be minutes long. That is why nothing here streams a caret the way
/// [`draw_element`] does for the agent's own prose, and why the header says how long ago
/// nothing — it reports *counts*, which are true, rather than liveness, which would not be.
fn subagent_body(ui: &mut egui::Ui, log: &SubagentLog, running: bool) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // ⚠️ `·` and `→`/`✓`/`✗` below are all glyphs the PROPORTIONAL face carries or
        // that are drawn monospace here. The box-drawing characters this file was once
        // full of rendered as tofu; see `draw_element`'s note on the em dash.
        ui.label(RichText::new("subagent").color(ASKING).small().monospace());
        let summary = subagent_summary(log, running);
        ui.label(RichText::new(summary.counts).color(DIM).small());
        if let Some(open) = summary.open {
            let color = if running { RUNNING } else { DIM };
            ui.label(RichText::new(open).color(color).small());
        }
    });
    if log.dropped > 0 {
        ui.label(
            RichText::new(format!("+{} earlier steps not kept", log.dropped)).color(DIM).small(),
        );
    }
    let skip = log.len().saturating_sub(SUBAGENT_LINES);
    if skip > 0 {
        ui.label(RichText::new(format!("+{skip} earlier steps")).color(DIM).small());
    }
    for step in log.steps.iter().skip(skip) {
        let deeper = step.depth > 1;
        ui.horizontal(|ui| {
            if deeper {
                ui.label(RichText::new(format!("{}·", "  ".repeat(step.depth as usize - 1)))
                    .color(DIM)
                    .small()
                    .monospace());
            }
            match &step.act {
                SubagentAct::Said(text) => {
                    ui.label(RichText::new(one_line(text)).color(PROSE).small());
                }
                SubagentAct::Tool { id, name, state } => {
                    let (mark, color) = match state {
                        StepState::Running => ("→", RUNNING),
                        StepState::Done { is_error: false } => ("✓", OK),
                        StepState::Done { is_error: true } => ("✗", BAD),
                    };
                    ui.label(RichText::new(mark).color(color).small().monospace());
                    // An unnamed step is a return whose call this log never saw — the same
                    // "(call not seen)" a card shows, for the same reason.
                    let label = name.as_deref().unwrap_or("(call not seen)");
                    ui.label(RichText::new(label).color(PROSE).small().monospace());
                    if name.is_none() {
                        ui.label(RichText::new(id.as_str()).color(DIM).small().monospace());
                    }
                }
            }
        });
    }
}

/// What an approval card reported this frame.
enum CardAct {
    Choose(Choice),
    /// Revoke a decision the console remembered.
    Forget,
}

/// **The card that turns "the agent bounced" into "the agent asked".**
///
/// Before this existed, a tool that needed permission failed and rendered red — three of
/// them in James's first real session — and the console had no way to say yes. The
/// difference is not cosmetic: `--permission-prompt-tool` gates **`Bash` as well as MCP
/// tools** (§2), so one card answers for everything the agent does.
///
/// The arguments are shown, always, because that is the entire point: a human authorising a
/// `Bash` call is authorising *that command*, and a card that named only the tool would be
/// a consent dialog with the consent removed. They are rendered through the same
/// [`argument_fields`] a tool card uses, with `complete: true` — a permission request
/// carries the model's final input, never a fragment.
///
/// Returns what was pressed, or `None`.
fn approval_card(
    ui: &mut egui::Ui,
    id: ElementId,
    block: &ApprovalBlock,
    live: bool,
) -> Option<CardAct> {
    let accent = match block.state {
        ApprovalState::Pending => ASKING,
        ApprovalState::Answered(a) if a.verdict == Verdict::Allow => OK,
        ApprovalState::Answered(_) => BAD,
        // Not `BAD`: nothing was refused. The card is spent, and reads that way.
        ApprovalState::Abandoned => DIM,
    };
    ui.push_id(id.0, |ui| {
        Frame::new()
            .fill(PANEL_FILL)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(8))
            .stroke(egui::Stroke::new(1.0f32, accent))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("◈ may I").color(accent).strong().monospace());
                    ui.label(
                        RichText::new(capability_label(&block.tool_name))
                            .color(PROSE)
                            .strong()
                            .monospace(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(&block.tool_use_id).color(DIM).small().monospace());
                    });
                });
                arguments_body(
                    ui,
                    &Arguments { text: block.input.clone(), complete: true },
                );
                ui.add_space(4.0);
                match block.state {
                    ApprovalState::Pending => approval_buttons(ui, live),
                    ApprovalState::Answered(answer) => approval_verdict(ui, answer),
                    // No buttons and no verdict — the outcome, and nothing that looks
                    // like it could still change it.
                    ApprovalState::Abandoned => {
                        ui.label(
                            RichText::new(
                                "the agent stopped waiting — this call failed before it was \
                                 answered",
                            )
                            .color(DIM)
                            .small(),
                        );
                        None
                    }
                }
            })
            .inner
    })
    .inner
}

fn approval_buttons(ui: &mut egui::Ui, live: bool) -> Option<CardAct> {
    let mut act = None;
    ui.horizontal_wrapped(|ui| {
        if !live {
            // The question is gone — evicted, or already answered elsewhere — so the
            // buttons would send a verdict nothing is waiting for. Say so instead.
            ui.label(RichText::new("this question is no longer answerable").color(DIM).small());
            return;
        }
        if ui.button(RichText::new("allow").color(OK).monospace()).clicked() {
            act = Some(CardAct::Choose(Choice::Allow));
        }
        if ui.button(RichText::new("allow & remember").color(OK).monospace()).clicked() {
            act = Some(CardAct::Choose(Choice::AllowAndRemember));
        }
        if ui.button(RichText::new("deny").color(BAD).monospace()).clicked() {
            act = Some(CardAct::Choose(Choice::Deny));
        }
    });
    act
}

fn approval_verdict(ui: &mut egui::Ui, answer: Answer) -> Option<CardAct> {
    let (word, color) = match answer.verdict {
        Verdict::Allow => ("allowed", OK),
        Verdict::Deny => ("denied", BAD),
    };
    let mut act = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(word).color(color).small().strong());
        if answer.from_memory {
            ui.label(RichText::new("· from a decision you already made").color(DIM).small());
        }
        if answer.remembered {
            ui.label(
                RichText::new("· the console will answer this the same way again")
                    .color(DIM)
                    .small(),
            );
            // The revocation. It is on the card, not in a settings screen, because this is
            // where a human is looking when they realise they granted too much.
            if ui.button(RichText::new("forget").monospace().small()).clicked() {
                act = Some(CardAct::Forget);
            }
        }
    });
    act
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
) {
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
                panel_body(ui, spec, state, defaults);
            });
    });
}

/// ⚠️ **Returns nothing, and used to return the button that was pressed.** That value was
/// read by exactly one caller — the arm that turned a press into an [`ArtifactAction`] for
/// the console's backdrop — and it went with `/panel`. A press now lands in
/// [`PanelState::material`], which is what the surface reads.
fn panel_body(
    ui: &mut egui::Ui,
    spec: &PanelSpec,
    state: &mut PanelState,
    defaults: &[(String, f32)],
) {
    // The description is authoritative about *which* controls exist; the state is
    // authoritative about where they are. This is the only line where the two meet.
    state.sync(spec, defaults);
    ui.horizontal_wrapped(|ui| {
        for label in &spec.buttons {
            // The panel shows which material its surface is wearing — it can, because that
            // material is this element's to mirror. A panel wired to the console never
            // could, which is one more way that arm read as broken.
            let chosen = state.material.as_deref() == Some(label.as_str());
            let text = RichText::new(label).monospace();
            let text = if chosen { text.color(PANEL_TITLE).strong() } else { text };
            if ui.button(text).clicked() {
                state.material = Some(label.clone());
            }
        }
    });
    for (label, value) in spec.sliders.iter().zip(state.sliders.iter_mut()) {
        ui.add(egui::Slider::new(value, 0.0..=1.0).text(label.as_str()));
    }
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

// ---------------------------------------------------------------------------
// The status strip
// ---------------------------------------------------------------------------
//
// **One band under the composer, and the model is its headline.** Claude Desktop's strip is
// the base: the thing you look at to answer "who am I talking to, and what is it doing right
// now", sitting with the composer rather than fenced off above it.
//
// 🚨 **Everything here is reported or measured, and the omissions are deliberate.**
// `SessionFacts`' own doc lists what the event stream does not honestly carry — a
// context-window percentage, a quota percentage, a session token total — and none of them are
// reconstructed here. Two more judgements are this file's rather than the mapper's:
//
// * **"● N tools running" says tools, not "thinking".** `Transcript::is_working` is derived
//   from unresolved tool calls only, so a model writing prose with nothing in flight is *not*
//   working by that test. A label reading "thinking" would therefore be false exactly when it
//   was most reassuring. That hole is closed by a **different** signal rather than by
//   loosening this one: [`Standing::Generating`] is the `message_start` … `message_stop`
//   bracket off the wire ([`crate::agent_map`] rule 7), so the band can say tokens are
//   arriving *because they are*, and "N tools running" still means exactly N tool calls.
//   ⚠️ Neither reading is ever derived from "a turn is open and nothing else is happening",
//   and no rate, bar or estimate goes beside them: the stream carries none of the three.
// * **Cost is labelled `session`, and per-turn tokens are not shown at all.** `cost_usd` is
//   cumulative on the wire and `last_turn_usage` is not, so one band carrying both invites the
//   reader to add them up. Cost answers "what has this conversation cost" in four characters;
//   the tokens are one hover away from being needed and were cut rather than qualified.
//
// The band is **one line, always**. Anything that would make it two goes into the model
// plate's hover instead ([`identity_rows`]), which is why the session id, the cwd, the
// permission mode and the MCP roster are not on screen: they are identity, not status, and a
// strip that grows a second row has stopped being a strip.

/// The plate the strip sits on, and the plate the model sits on inside it.
const STRIP_FILL: Color32 = Color32::from_rgb(0x0b, 0x11, 0x0b);
const STRIP_EDGE: Color32 = Color32::from_rgb(0x22, 0x2c, 0x22);
const MODEL_FILL: Color32 = Color32::from_rgb(0x15, 0x1e, 0x15);
const MODEL_EDGE: Color32 = Color32::from_rgb(0x3a, 0x50, 0x3a);
/// The model's own name — brighter than [`PROSE`], because it is the one thing on this band
/// that is an *identity* rather than a reading.
const MODEL_TEXT: Color32 = Color32::from_rgb(0xc6, 0xdf, 0xc6);
/// The bracketed variant beside it (`1m` → `1M`): present, secondary, never mistaken for the
/// name.
const MODEL_BADGE: Color32 = Color32::from_rgb(0x8a, 0xb0, 0x8a);

/// The permission plate's two voices.
///
/// ⚠️ **Neither is [`BAD`], and that is the decision.** The non-default marker is on the
/// band for *hours*, not for a moment, and this is a band a working hand looks at
/// constantly — a red klaxon that never goes away is a red klaxon the eye learns to skip,
/// which would leave the console back where it started. Amber is legible against the dim
/// half without competing with an actual failure.
const MODE_ALERT: Color32 = Color32::from_rgb(0xd8, 0x9a, 0x5c);
const MODE_NOTE: Color32 = Color32::from_rgb(0x8a, 0xa6, 0xc2);

/// The context ring's unfilled circumference — present enough to say "this is a dial and
/// it is not full", dim enough not to compete with the band's actual readings.
const CONTEXT_TRACK: Color32 = Color32::from_rgb(0x2c, 0x38, 0x2c);
/// The filled arc below [`CONTEXT_HIGH_PERCENT`].
///
/// Blue rather than a green off this band's own palette, and that is the point: every
/// other colour here is a *standing* — [`RUNNING`] busy, [`ASKING`] blocked, [`BAD`]
/// gone — and the ring is not a standing. It is a resource gauge, true continuously, and
/// giving it a hue no reading uses is what keeps a half-full context from looking like a
/// state the agent is in.
const CONTEXT_ARC: Color32 = Color32::from_rgb(0x5f, 0x93, 0xcc);
/// …and above it. [`MODE_ALERT`]'s exact amber, reused rather than re-chosen: it already
/// means "worth acting on, not a failure" on this band, which is precisely the reading.
const CONTEXT_ARC_HIGH: Color32 = MODE_ALERT;
const CONTEXT_RING_STROKE: f32 = 2.0;

/// Where the ring turns amber — **a display decision, and the console's own.**
///
/// Nothing on the wire says when the CLI will compact a conversation, so any threshold
/// here is a judgement about how much runway a reader needs rather than a measurement,
/// and it says so instead of borrowing the authority of the two numbers around it.
///
/// Seventy-five, because the amber has to arrive while the answers are still cheap —
/// start a fresh tab, ask for a summary, let a long tool result go — and each of those
/// costs a turn or two. A turn is not small against this window: on
/// `claude_stream_two_tools.jsonl` one turn's two requests carried 52 556 then 54 050
/// tokens, so the conversation grew ~1 500 in a single round trip and had already spent
/// 5 % of a 1 000 000 window on its first. A warning at 90 % would leave a handful of
/// round trips; a quarter of the window leaves room to finish the thought.
const CONTEXT_HIGH_PERCENT: u64 = 75;

const STRIP_PAD_X: i8 = 10;
const STRIP_PAD_Y: i8 = 5;
const STRIP_STROKE: f32 = 1.0;
const MODEL_PAD_X: i8 = 7;
const MODEL_PAD_Y: i8 = 2;
const MODEL_STROKE: f32 = 1.0;

/// Everything the band costs on top of one row of text: both plates' padding and both edges.
///
/// Named and derived rather than a round number, because the band is **reserved before the
/// content is laid out** — the same discipline [`composer_box`] documents — and a reserved
/// band that disagrees with its own chrome is how a one-line strip quietly becomes two.
const STRIP_CHROME: f32 = 2.0 * STRIP_PAD_Y as f32
    + 2.0 * STRIP_STROKE
    + 2.0 * MODEL_PAD_Y as f32
    + 2.0 * MODEL_STROKE;

/// What the strip's status half is reporting, in priority order — highest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// The process could not start, or has ended. Outranks everything: nothing else on the
    /// band means anything once there is nobody behind it.
    Dead,
    /// Blocked on a human — a pending permission request, or a finished turn that asked for
    /// something. Outranks [`Standing::Working`] because the agent can finish work on its own
    /// and cannot finish this.
    Asking,
    /// Tool calls in flight. **Not "thinking"** — see this section's note.
    Working,
    /// An assistant message is open: tokens are arriving right now.
    ///
    /// Measured, never inferred — [`EventMapper::is_generating`] is the `message_start` …
    /// `message_stop` bracket and nothing else. It is deliberately *not* "requesting",
    /// which means the opposite half of a round trip and is emitted once per run rather
    /// than once per message; [`crate::agent_map`] rule 7 owns that argument.
    Generating,
    /// Alive, but nothing has arrived yet. The cold start every session opens in.
    Connecting,
    /// Alive, nothing outstanding.
    Ready,
}

/// The model half of the band. Three states, because "no model" before `system/init` and "no
/// model, ever" after a spawn failure are different sentences, and an empty box is neither.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelSlot {
    Named(ModelLabel),
    /// No `system/init` yet, and the agent is alive — it is coming.
    Connecting,
    /// …and it is not, because the agent is gone.
    Absent,
}

/// The context half of the band: the little ring at the far right, or nothing at all.
///
/// 🚨 **`Unknown` draws nothing, and that is the honest rendering rather than a shortcut.**
/// A ring is a proportion; before the first `result` there is no window and before the
/// first `message_start` there is no prompt, and a ring drawn empty in either case reads
/// as *"0 % full"* — a confident, specific, false number. [`ModelSlot::Connecting`] says
/// "no model yet" instead of vanishing because that plate is the headline affordance and
/// a hole where it sits reads as broken; the ring sits at the end of the dim half beside
/// the chips, which appear when their number does and are absent before it. It follows
/// the chips.
///
/// ⚠️ So a session's first turn has no ring, and the ring appears at that turn's `result`.
/// From then on it moves **per API round trip**, not per turn — a `message_start` updates
/// it mid-turn, which is the visible consequence of the numerator being what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextSlot {
    /// One or both halves have not been measured yet. Nothing is drawn.
    Unknown,
    /// Both halves measured. See [`ContextFill`] for exactly what they are.
    Known(ContextFill),
}

impl ContextSlot {
    /// Whether the reading has crossed [`CONTEXT_HIGH_PERCENT`].
    ///
    /// Integer arithmetic on purpose: a threshold compared as a float is a threshold that
    /// can fall on either side of its own boundary depending on the window, and this one
    /// is pinned at exactly 75 % by a test.
    pub fn is_high(&self) -> bool {
        match self {
            ContextSlot::Unknown => false,
            ContextSlot::Known(fill) => {
                fill.prompt_tokens.saturating_mul(100)
                    >= fill.context_window.saturating_mul(CONTEXT_HIGH_PERCENT)
            }
        }
    }
}

/// A model identifier, split for display.
///
/// See [`model_label`] for exactly what that split does and does not do to the string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelLabel {
    /// The identifier with any trailing bracketed suffix removed: `claude-opus-5`.
    pub name: String,
    /// That suffix's contents, upper-cased for a badge: `1m` → `1M`. `None` when there was
    /// no suffix.
    pub variant: Option<String>,
}

// ---------------------------------------------------------------------------
// The permission-mode control
// ---------------------------------------------------------------------------
//
// 🚨 **This is the control that can quietly remove the console's authority, and the
// design is built around that rather than around the enum.**
// `doc/console_session_control_protocol.md` §10 measured it: put a session in `dontAsk`
// and every tool that would have raised an approval card comes back **refused**
// (`decision_reason_type: "mode"`) without the console's handler ever being consulted —
// while the console still passes `--permission-prompt-tool`, still holds the handler, and
// still *looks* like the authority. The failure a user experiences is "the agent suddenly
// cannot do anything and nobody asked me why."
//
// Three consequences, each a decision rather than an implementation detail:
//
// * **Each row is labelled by what happens, not by the mode's name.** "dontAsk" tells a
//   reader nothing; "no approval cards — anything needing permission is refused" tells
//   them everything.
// * **The warning is PERSISTENT, not a confirmation.** A dialog clicked through at the
//   moment of choosing is exactly the warning people stop reading, and the hazard is not
//   that moment — it is the hours afterwards when the band still looks like the authority.
//   So whenever the mode is not `default`, [`ModeSlot::marker`] is on the band for as long
//   as that stays true. Legible, not shrill: this band is looked at constantly and a
//   permanent klaxon trains the eye to skip it.
// * **Three modes are offered and no others.** `bypassPermissions` is refused outright by
//   a session the console did not launch with `--dangerously-skip-permissions` (§9), so
//   the row would be a dead button. `plan` and `auto` were never measured against the
//   console's handler — and the control that governs authority is the wrong place to
//   guess. ⚠️ A mode arriving from *outside* this picker (a session spawned with
//   `--permission-mode`) is still reported and still marked; the picker's shortlist
//   governs what can be *chosen*, never what can be *shown*.

/// How loudly the band carries a non-default permission mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeSeverity {
    /// Worth stating and standing out from the dim half. `acceptEdits`, and any mode this
    /// build has not met.
    Note,
    /// The console is no longer being asked. `dontAsk`, and nothing else today.
    Alert,
}

/// The persistent marker a non-default permission mode puts on the band.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeMarker {
    /// **What is happening**, in the words a reader needs — the mode's *name* is on the
    /// plate beside it, so this never spends the band's one line repeating it.
    pub text: String,
    pub severity: ModeSeverity,
}

/// The permission half of the band: what the session reports, and the marker that goes
/// with it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModeSlot {
    /// The mode exactly as the session spelled it, or `None` before the first init.
    pub mode: Option<String>,
    /// **Present exactly when the mode is not `default`.** See this section's note: the
    /// warning is the standing state of the band, not an event.
    pub marker: Option<ModeMarker>,
}

/// The wire spelling of the mode in which the console is the approval authority.
///
/// ⚠️ Not `manual`. `--help` spells the flag-side choice `manual` and the wire enum has
/// `default` with no `manual` at all (§7); this is the one the strip reads and the one
/// `set_permission_mode` takes.
pub const MODE_DEFAULT: &str = "default";

/// The marker for a reported mode — `None` only for `default`.
///
/// 🚨 A mode this build has not met still gets a marker. The rule the band is keeping is
/// *"the console says so whenever it may not be the one being asked"*, and an unrecognised
/// mode is precisely the case where that cannot be ruled out.
pub fn mode_marker(mode: &str) -> Option<ModeMarker> {
    match mode.trim() {
        "" | MODE_DEFAULT => None,
        "acceptEdits" => Some(ModeMarker {
            text: "edits are auto-accepted".to_string(),
            severity: ModeSeverity::Note,
        }),
        "dontAsk" => Some(ModeMarker {
            text: "you are not being asked — anything needing permission is refused".to_string(),
            severity: ModeSeverity::Alert,
        }),
        _ => Some(ModeMarker {
            text: "not the console's default — approvals may not reach you".to_string(),
            severity: ModeSeverity::Note,
        }),
    }
}

/// The full consequence sentence for a mode the picker offers, for the plate's hover.
/// `None` for a mode that arrived from outside the picker.
pub fn mode_consequence(mode: &str) -> Option<&'static str> {
    MODE_ROWS.iter().find(|row| row.value == mode).map(|row| row.consequence)
}

/// One row of the permission-mode picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeRow {
    /// What `set_permission_mode` takes.
    pub value: &'static str,
    /// **What happens**, which is the label. The mode's name is the second half of it.
    pub consequence: &'static str,
    pub severity: ModeSeverity,
}

/// The three modes the console offers, and the whole of what it offers.
///
/// See this section's note for why the list is three and why each omission is deliberate.
pub const MODE_ROWS: &[ModeRow] = &[
    ModeRow {
        value: MODE_DEFAULT,
        consequence: "the console asks you — every gated tool raises an approval card",
        severity: ModeSeverity::Note,
    },
    ModeRow {
        value: "acceptEdits",
        // Honest about the size of the measurement: §11 tested exactly one gate reason
        // (`workingDir`) and found the handler still consulted. It does **not** establish
        // that this mode never short-circuits, and the label does not claim it does.
        consequence: "file edits are auto-accepted where the CLI's own gate allows it — \
                      measured against one gate only; other requests still raise a card",
        severity: ModeSeverity::Note,
    },
    ModeRow {
        value: "dontAsk",
        consequence: "no approval cards at all — anything needing permission is refused, \
                      and the console is never asked",
        severity: ModeSeverity::Alert,
    },
];

// ---------------------------------------------------------------------------
// The model picker
// ---------------------------------------------------------------------------

/// One row of the model picker, built from what the CLI itself offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRow {
    /// What `set_model` takes — an alias, a full id, or `default`.
    pub value: String,
    /// The human-written `displayName`, falling back to `value` when the CLI sent none.
    pub label: String,
    /// The CLI's own sentence of guidance, when it sent one.
    pub detail: Option<String>,
    /// Whether this row is what the session is running right now.
    pub current: bool,
}

/// The picker's rows.
///
/// 🚨 **Built from the CLI's `models` array and from nothing else.** The list is
/// per-account and can gain a model after this build ships, so there is no table here to
/// go stale — an empty list is an empty picker that says the list has not arrived, which
/// is the honest rendering of "this session did not answer `initialize`".
///
/// ⚠️ **Two rows can both be current**, and that is not a bug: `default` and `opus[1m]`
/// both resolve to `claude-opus-5[1m]` in the measured capture, so both genuinely name the
/// model in use. Matching on `resolvedModel` *and* `value` is what `resolvedModel` exists
/// for — the schema states it is there so a host can match a persisted explicit id back to
/// the alias row that covers it.
pub fn model_rows(models: &[ModelChoice], current: Option<&str>) -> Vec<ModelRow> {
    models
        .iter()
        .map(|choice| ModelRow {
            value: choice.value.clone(),
            label: choice
                .display_name
                .clone()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| choice.value.clone()),
            detail: choice.description.clone().filter(|d| !d.trim().is_empty()),
            current: current.is_some_and(|current| {
                choice.resolved_model.as_deref() == Some(current) || choice.value == current
            }),
        })
        .collect()
}

/// What the picker says about the price of switching, once, at the bottom.
///
/// 📌 **Measured, and not a warning dialog.** A model change invalidates the prompt cache:
/// the turn after one carries `cache_miss_reason: model_changed` and re-created 69 228
/// tokens where the previous turn had read 25 282 from cache — $0.30 then $0.42 for the
/// same three-token reply (§2a). There is nothing to fix and nothing to confirm; a plate
/// this easy to click should simply not imply the click is free.
pub const MODEL_SWITCH_COST: &str =
    "switching re-reads the conversation — the next turn pays a cache miss (~49k tokens \
     measured)";

/// What the status half says this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusReading {
    pub standing: Standing,
    /// Empty is legal and means "draw nothing" — the model plate is already saying it.
    pub text: String,
}

/// Everything one frame of the strip draws, decided before anything is laid out.
///
/// Split from the drawing for the same reason [`composer_box`] is split from
/// [`ConversationPane`]: the interesting part is the *priority ordering*, and testing it
/// through a real pane would mean spawning an agent process to find out what a band says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripContent {
    pub model: ModelSlot,
    /// The permission mode, and the persistent marker that goes with a non-default one.
    pub mode: ModeSlot,
    /// A model change that has been asked for and not confirmed — the row's label, not a
    /// model id. Drawn *beside* [`model`](Self::model), never in place of it: see
    /// [`PendingModel`].
    pub pending_model: Option<String>,
    /// `(label, value)` rows for the model plate's hover — the identity that does not fit,
    /// and must not be allowed to try.
    pub identity: Vec<(String, String)>,
    pub reading: StatusReading,
    /// How full the model's context was at the last request — the ring at the far right,
    /// or [`ContextSlot::Unknown`] and no ring at all.
    pub context: ContextSlot,
    /// Dim, right-aligned, joined with `·`. Bounded by construction: at most three.
    pub chips: Vec<String>,
    /// The most recent diagnostic line off the child, if any. Drawn truncated.
    pub log: Option<String>,
}

impl StripContent {
    /// Mark the plate as carrying a model change that has not been confirmed yet.
    ///
    /// A builder rather than a parameter of [`strip_content`] because it is the one input
    /// the band takes that is **view state** rather than a reported fact: it exists from
    /// the click until the repeat `system/init`, and nothing on the wire ever states it.
    pub fn switching_to(mut self, label: Option<&str>) -> Self {
        self.pending_model = label.map(str::to_string);
        self
    }
}

/// The **live** inputs the strip reads — the things that are true right now and will not be
/// true in a minute — gathered so the decision below can be a pure function of plain values
/// rather than of a live [`Transcript`], a [`DecisionMemory`] and an [`EventMapper`].
///
/// The split against [`SessionFacts`] is the retention rule, not the source: everything there
/// was *reported* and stands until something replaces it; everything here is a reading taken
/// this frame, and every field of it goes back to zero or `false` on its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveCounts {
    pub pending_approvals: usize,
    pub running_tools: usize,
    /// How many decisions the console has been asked to remember this session.
    pub remembered: usize,
    /// Whether a `system/init` has been seen — the cold-start discriminator.
    pub has_session: bool,
    /// [`EventMapper::is_generating`]: an assistant message is open and tokens are arriving.
    ///
    /// ⚠️ **This is the field that says why the struct is not "the transcript's numbers".**
    /// It comes off the mapper, not the transcript, and it is here rather than on
    /// [`SessionFacts`] because of what it *is*: a state that flips on and off, against a
    /// type whose every field is a reported value that persists until replaced. Mixing the
    /// two would mean a strip that keeps claiming the agent is writing after it stopped —
    /// [`crate::agent_map`] rule 7 carries the full argument and the clearing paths.
    pub generating: bool,
}

/// **The priority ordering, and the whole of it.**
///
/// 1. **Dead outranks everything.** A tab whose agent is gone must say so before it says
///    anything else; every other reading on the band describes a process that exists.
/// 2. **A pending approval outranks running tools.** The agent is *halted* on a human, and
///    only one of the two states it can get out of by itself. This is the ordering the first
///    status line had, and it is kept for the reason it had it.
/// 3. **Running tools outrank generating**, and the two are true *together* for most of a
///    turn — a tool block opens inside the message that called it, so the bracket is still
///    open while the call runs. The ordering is therefore not about which is more urgent; it
///    is about which sentence is worth the one line there is. "● 3 tools running" names what
///    is happening and can be checked against the cards above it; "● generating" only says
///    that *something* is. The specific reading wins, and the general one is what the band
///    falls back to when there is nothing more specific to say — which is exactly the stretch
///    of a turn that used to read as idle.
/// 4. **Generating outranks a finished turn's `needs_action`**, for the identical reason
///    running tools do. ⚠️ This is the one place the "waiting outranks working" rule is
///    deliberately not applied: `needs_action` describes a turn that has *ended*, and the
///    mapper only clears it when the next `post_turn_summary` arrives — so a demand the human
///    already answered stays standing for the whole of the reply that answered it. Tokens
///    arriving now are live activity by exactly the measure a running tool is, and a rule
///    that keeps a stale "waiting on you" off the band has to cover both or it does not hold.
/// 5. **`needs_action` then, verbatim.** It is the agent's own sentence about what it wants.
/// 6. **Cold start**, when no init has been seen.
/// 7. **`last_status_detail`**, else a bare "ready".
///
/// ⚠️ **Between two messages of one turn this falls through to 7 for a frame or two** — the
/// bracket really has closed and the next one has not opened yet. That flicker is the honest
/// answer, and it is the price of refusing the alternative: holding "generating" across the
/// gap would mean inventing a turn-open state the wire does not report, and that is the
/// version that gets stuck on when a turn ends in a way nobody predicted.
pub fn status_reading(
    failure: Option<&str>,
    live: LiveCounts,
    facts: &SessionFacts,
) -> StatusReading {
    let say = |standing, text: String| StatusReading { standing, text };
    if let Some(failure) = failure {
        return say(Standing::Dead, failure.to_string());
    }
    if live.pending_approvals > 0 {
        let n = live.pending_approvals;
        let plural = if n == 1 { "request" } else { "requests" };
        return say(Standing::Asking, format!("◈ {n} permission {plural} — waiting on you"));
    }
    if live.running_tools > 0 {
        let n = live.running_tools;
        let plural = if n == 1 { "tool" } else { "tools" };
        return say(Standing::Working, format!("● {n} {plural} running"));
    }
    if live.generating {
        // No count, no rate, no estimate. The wire says a message is open; it does not say
        // how much is left, how fast it is arriving, or when it will stop, and every one of
        // those would have to be invented to be shown.
        return say(Standing::Generating, "● generating".to_string());
    }
    if let Some(action) = &facts.needs_action {
        return say(Standing::Asking, format!("◈ {action}"));
    }
    if !live.has_session {
        // Empty on purpose: the model plate already reads "no model yet", and a band that
        // says "connecting…" twice reads as a bug rather than as one state.
        return say(Standing::Connecting, String::new());
    }
    match &facts.last_status_detail {
        Some(detail) => say(Standing::Ready, detail.clone()),
        None => say(Standing::Ready, "ready".to_string()),
    }
}

/// Split a reported model identifier into a name and a badge.
///
/// ⚠️ **Nothing is dropped.** The only two transformations are structural: a *trailing*
/// bracketed suffix is moved out of the name into [`ModelLabel::variant`], and that suffix's
/// contents are upper-cased so `1m` reads as the megatoken window it is rather than as a
/// typo. `claude-opus-5[1m]` is therefore recoverable from the pair, and the **verbatim**
/// string is on the plate's hover either way ([`identity_rows`]).
///
/// **What this deliberately does not do is prettify.** `claude-opus-5` is not rewritten to
/// "Opus 5", tempting as that is next to Claude Desktop: the field is whatever the CLI
/// reported, and a table of nice names would silently mangle the first identifier that is not
/// on it — an alias, a snapshot date, a gateway's fully-qualified id. A strip that renames a
/// model it does not recognise is a strip that lies about which model you are talking to.
pub fn model_label(raw: &str) -> ModelLabel {
    let trimmed = raw.trim();
    if let Some(open) = trimmed.rfind('[') {
        if trimmed.ends_with(']') {
            let inner = &trimmed[open + 1..trimmed.len() - 1];
            let name = trimmed[..open].trim_end();
            // An empty pair of brackets is punctuation, not a variant, and a name that is
            // *only* a suffix is not a name — both fall through to the verbatim spelling.
            if !inner.is_empty() && !name.is_empty() {
                return ModelLabel {
                    name: name.to_string(),
                    variant: Some(inner.to_uppercase()),
                };
            }
        }
    }
    ModelLabel { name: trimmed.to_string(), variant: None }
}

/// The identity that does not fit on the band, for the model plate's hover.
///
/// Everything here is verbatim from the stream, `model` above all: whatever [`model_label`]
/// rearranged on screen, this row is the string the CLI actually reported.
fn identity_rows(facts: &SessionFacts, session: Option<&str>) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut row = |label: &str, value: String| rows.push((label.to_string(), value));
    if let Some(model) = &facts.model {
        row("model", model.clone());
    }
    if let Some(mode) = &facts.permission_mode {
        row("permissions", mode.clone());
    }
    if let Some(version) = &facts.cli_version {
        row("cli", version.clone());
    }
    if let Some(cwd) = &facts.cwd {
        row("cwd", cwd.clone());
    }
    if facts.tools > 0 {
        row("tools", facts.tools.to_string());
    }
    if !facts.mcp_servers.is_empty() {
        let servers = facts
            .mcp_servers
            .iter()
            .map(|(name, status)| format!("{name} ({status})"))
            .collect::<Vec<_>>()
            .join(", ");
        row("mcp", servers);
    }
    // Reported as given. ⚠️ `rate_limit_resets_at` is a unix timestamp and is **not** shown:
    // rendering it needs a clock and a timezone, and an unexplained ten-digit number is a
    // debug field wearing a label.
    match (&facts.rate_limit_type, &facts.rate_limit_status) {
        (Some(kind), Some(status)) => row("limit", format!("{kind} — {status}")),
        (Some(kind), None) => row("limit", kind.clone()),
        (None, Some(status)) => row("limit", status.clone()),
        (None, None) => {}
    }
    if let Some(session) = session {
        row("session", session.to_string());
    }
    rows
}

/// Session-cumulative cost. Four decimals under a dollar, two over it — a turn of a
/// conversation costs cents, and `$0.00` for eight minutes' work reads as "free".
fn cost_label(cost: f64) -> String {
    if cost >= 1.0 {
        format!("${cost:.2}")
    } else {
        format!("${cost:.4}")
    }
}

/// A turn's wall time, in the unit a human would have used for it.
fn duration_label(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    if ms < 60_000 {
        return format!("{:.1}s", ms as f64 / 1_000.0);
    }
    format!("{}m{:02}s", ms / 60_000, (ms % 60_000) / 1_000)
}

/// Decide the whole band. Pure, and the only place the strip's content is chosen.
pub fn strip_content(
    failure: Option<&str>,
    live: LiveCounts,
    facts: &SessionFacts,
    session: Option<&str>,
    log: Option<&str>,
) -> StripContent {
    let model = match (&facts.model, failure.is_some()) {
        (Some(raw), _) => ModelSlot::Named(model_label(raw)),
        (None, false) => ModelSlot::Connecting,
        (None, true) => ModelSlot::Absent,
    };
    // Three at most, in reading order. Each is either a measurement or absent — there is no
    // arm here that computes one number out of two.
    let mut chips: Vec<String> = Vec::new();
    if let Some(cost) = facts.cost_usd {
        // "session" is not decoration: `cost_usd` accumulates on the wire and the sibling
        // token counts do not, so the one number on the band says which kind it is.
        chips.push(format!("session {}", cost_label(cost)));
    }
    if live.remembered > 0 {
        let n = live.remembered;
        let plural = if n == 1 { "decision" } else { "decisions" };
        chips.push(format!("{n} remembered {plural}"));
    }
    if let Some(ms) = facts.last_turn_duration_ms {
        chips.push(format!("last turn {}", duration_label(ms)));
    }
    // The marker is derived, never remembered: it is true exactly while the reported mode
    // is non-default, which is the property the persistent-warning decision asked for.
    let mode = ModeSlot {
        mode: facts.permission_mode.clone(),
        marker: facts.permission_mode.as_deref().and_then(mode_marker),
    };
    // Two measurements or nothing. `context_fill` refuses when either half is missing,
    // and there is no arm here that supplies one — see [`ContextSlot`].
    let context = match facts.context_fill() {
        Some(fill) => ContextSlot::Known(fill),
        None => ContextSlot::Unknown,
    };
    StripContent {
        model,
        mode,
        pending_model: None,
        identity: identity_rows(facts, session),
        reading: status_reading(failure, live, facts),
        context,
        chips,
        log: log.map(str::to_string),
    }
}

fn standing_color(standing: Standing) -> Color32 {
    match standing {
        Standing::Dead => BAD,
        Standing::Asking => ASKING,
        // One colour for both, deliberately. The distinction the palette has to carry is
        // busy-versus-blocked ([`ASKING`]'s note); busy-with-tools and busy-writing are the
        // same answer to "can I walk away", and giving them two amber-ish colours would spend
        // the band's whole colour budget on a difference the text already spells out.
        Standing::Working | Standing::Generating => RUNNING,
        Standing::Connecting | Standing::Ready => DIM,
    }
}

/// What the band asked for this frame.
///
/// Returned rather than acted on in place, for the reason [`approval_card`] returns a
/// [`CardAct`]: the drawing walks a `&StripContent`, and the pane it would have to mutate
/// is what that content was read from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum StripAct {
    /// A model row was clicked.
    ChooseModel(ModelRow),
    /// A permission mode was clicked. `&'static str` because the shortlist is this file's.
    ChooseMode(&'static str),
}

fn status_strip(ui: &mut egui::Ui, pane: &mut ConversationPane) {
    let content = strip_content(
        pane.failure.as_deref(),
        LiveCounts {
            pending_approvals: pane.transcript.pending_approvals().len(),
            running_tools: pane.transcript.running_tools().len(),
            remembered: pane.memory.len(),
            has_session: pane.transcript.session_id().is_some(),
            // The one reading that comes off the mapper rather than the transcript: the
            // bracket is a stream fact, and the transcript is an ordered list of what was
            // said, which cannot hold "and it is still being said".
            generating: pane.mapper.is_generating(),
        },
        pane.mapper.facts(),
        pane.transcript.session_id(),
        pane.log.back().map(String::as_str),
    )
    .switching_to(pane.pending_model.as_ref().map(|p| p.label.as_str()));
    let rows = model_rows(&pane.models, pane.mapper.facts().model.as_deref());
    match strip_box(ui, &content, &rows) {
        Some(StripAct::ChooseModel(row)) => pane.choose_model(&row),
        Some(StripAct::ChooseMode(mode)) => pane.choose_permission_mode(mode),
        None => {}
    }
}

/// Draw the band.
///
/// 🚨 **The band is reserved, not discovered** — `allocate_ui_with_layout`, exactly as
/// [`composer_box`] does and for the same measured reason: this is a bottom-up column, and a
/// child that places itself at `available_rect_before_wrap().min` eats everything between the
/// top of the remaining space and the cursor at its bottom. Reserving the taller of the two
/// faces the band draws plus [`STRIP_CHROME`] is also what holds the strip to a single line no
/// matter what arrives — every label that could be long is [`egui::Label::truncate`]d rather
/// than wrapped.
fn strip_box(
    ui: &mut egui::Ui,
    content: &StripContent,
    models: &[ModelRow],
) -> Option<StripAct> {
    // ⚠️ **The reserved row must cover the tallest face the band actually draws**, not one
    // of them. Two are in play and neither is decorative: the model name and the standing
    // are `Monospace`, the chips and the log are `Body`. Reserving `Body` alone was only
    // ever right by accident of which face happened to be taller, and the moment the dim
    // half stopped being `.small()` that accident became the whole margin. Take the larger
    // of the two and the reservation cannot disagree with what it holds.
    let row = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.text_style_height(&egui::TextStyle::Monospace));
    let band = row + STRIP_CHROME;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), band),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            Frame::new()
                .fill(STRIP_FILL)
                .stroke(egui::Stroke::new(STRIP_STROKE, STRIP_EDGE))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(STRIP_PAD_X, STRIP_PAD_Y))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let mut act = model_plate(ui, content, models);
                        act = act.or(mode_plate(ui, content));
                        let reading = &content.reading;
                        if !reading.text.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    // ⚠️ **`.monospace()` is the tofu fix, not a style
                                    // choice.** `status_reading` builds these strings with
                                    // `◈` (U+25C8) and `●` (U+25CF); egui's PROPORTIONAL
                                    // face has neither, so `● generating` drew as a box.
                                    // The mono face carries them — it renders `htop`'s box
                                    // drawing in the terminal tab next door — and this is
                                    // the same fix the approval card's own `◈ may I`
                                    // already carries. Leave it on, or the band's symbols
                                    // come back as boxes.
                                    //
                                    // Full size, not `.small()`: this is a *reading*, the
                                    // second thing a hand looks for after the model name,
                                    // and it is the only item between the plates and the
                                    // dim half. Left small it would be the one shrunken
                                    // word in a band that is otherwise one size, which
                                    // reads as a mistake rather than as a hierarchy.
                                    RichText::new(&reading.text)
                                        .color(standing_color(reading.standing))
                                        .monospace(),
                                )
                                .truncate(),
                            );
                        }
                        // The dim half. Right-aligned so the eye lands on the model and the
                        // standing first; the log is added last and is therefore leftmost,
                        // which is what gives it the slack and lets it truncate into it.
                        //
                        // ⚠️ **Dim, not small.** These carry numbers a hand reads across a
                        // desk — the session's spend, the turn it just paid for — and at
                        // `.small()` they were legibly smaller than the model name sitting
                        // opposite them on the same band. Colour is what makes this half
                        // secondary; size was doing a second job it was never needed for,
                        // and doing it at the cost of the one thing on the band with a
                        // number in it. The band is still one line: the reserved row above
                        // now covers this face, and the log still [`Label::truncate`]s into
                        // whatever slack the chips leave — larger text simply means less of
                        // it fits before the ellipsis.
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                // First in a right-to-left layout is rightmost: the ring
                                // is the last thing on the band, which is where a gauge
                                // that is true continuously belongs and where the eye
                                // learns to find it without reading.
                                context_ring(ui, &content.context);
                                if !content.chips.is_empty() {
                                    ui.label(
                                        RichText::new(content.chips.join(" · ")).color(DIM),
                                    );
                                }
                                if let Some(log) = &content.log {
                                    ui.add(
                                        egui::Label::new(RichText::new(log).color(DIM))
                                            .truncate(),
                                    );
                                }
                            },
                        );
                        act
                    })
                    .inner
                })
                .inner
        },
    )
    .inner
}

/// The context ring: how full the model's window was at the **last request**.
///
/// A dial rather than a number because the question it answers is "how much room is
/// left", which is a proportion, and because it has to be readable without being read —
/// this band is looked at for hours and the ring is at the edge of the eye. The counts
/// themselves are on the hover, where they cost no width and cannot push the band to two
/// lines.
///
/// ⚠️ **The diameter is exactly one [`egui::TextStyle::Body`] row**, which is what makes
/// it free: [`strip_box`] reserves `row + STRIP_CHROME` *before* laying anything out, and
/// the plates beside this are that same row plus their own padding. A ring drawn at any
/// size a designer liked would be the one child of the horizontal layout taller than the
/// reservation, and the strip would silently become two lines.
///
/// 🚨 **An arc, not a pie.** A filled wedge past 180° is not convex, and egui's
/// `convex_polygon` tessellation produces a folded-over shape for one — it would draw
/// *wrongly* exactly as the reading became urgent. A thick stroked polyline has no such
/// case and is what the indicator this copies looks like anyway.
fn context_ring(ui: &mut egui::Ui, slot: &ContextSlot) {
    // Nothing at all when either half is missing: see [`ContextSlot`] for why an empty
    // ring is worse than no ring.
    let ContextSlot::Known(fill) = slot else {
        return;
    };
    let diameter = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(diameter, diameter),
        egui::Sense::hover(),
    );
    let center = rect.center();
    // Inset by half the stroke so the ring stays inside its own allocation rather than
    // bleeding a pixel into the chip beside it.
    let radius = diameter * 0.5 - CONTEXT_RING_STROKE * 0.5;
    let painter = ui.painter();
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(CONTEXT_RING_STROKE, CONTEXT_TRACK),
    );
    let filled = fill.fraction();
    if filled > 0.0 {
        // Twelve o'clock, clockwise — a clock face, because that is the shape everyone
        // already knows how to read. Screen y grows downward, so a growing angle is
        // clockwise here without a sign flip.
        const SEGMENTS: f32 = 64.0;
        let steps = ((SEGMENTS * filled).ceil() as usize).max(1);
        let sweep = filled * std::f32::consts::TAU;
        let points: Vec<egui::Pos2> = (0..=steps)
            .map(|step| {
                let angle = -std::f32::consts::FRAC_PI_2
                    + sweep * (step as f32 / steps as f32);
                center + egui::vec2(radius * angle.cos(), radius * angle.sin())
            })
            .collect();
        let color = if slot.is_high() { CONTEXT_ARC_HIGH } else { CONTEXT_ARC };
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(CONTEXT_RING_STROKE, color),
        ));
    }
    // The provenance lives here, in the same shape as the model plate's identity hover:
    // what it measures, what it does not, and both raw counts.
    response.on_hover_ui(|ui| {
        for (label, value) in context_rows(fill) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{label}:")).color(DIM).small().monospace());
                ui.label(RichText::new(value).color(PROSE).small().monospace());
            });
        }
    });
}

/// The ring's hover, and the place the reading states what it is.
///
/// ⚠️ **"at the last request" is not phrasing, it is the marker.** A turn makes several
/// API round trips and this is one of them — the most recent — so a hover that said
/// "context used" would invite the reader to add turns up, which is the exact mistake
/// `result.usage` would have made for them. `agent_map`'s [`ContextFill`] carries the
/// argument.
fn context_rows(fill: &ContextFill) -> Vec<(String, String)> {
    vec![
        ("context".to_string(), format!("{}% at the last request", fill.percent())),
        (
            "prompt".to_string(),
            format!("{} tokens", thousands(fill.prompt_tokens)),
        ),
        (
            "window".to_string(),
            format!("{} tokens", thousands(fill.context_window)),
        ),
        (
            "measured".to_string(),
            "message_start.usage / modelUsage.contextWindow".to_string(),
        ),
    ]
}

/// `54050` → `54,050`. Six-figure token counts are unreadable without it, and this band
/// has no number formatting anywhere else to borrow.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// **The headline affordance**: which model is behind this tab, on its own plate — and now
/// the control that changes it.
///
/// A plate rather than a label because that is the difference between an identity and a debug
/// field — it is the one thing on the band that answers "who", and it is the first thing a
/// hand goes looking for. The rest of what the session said about itself is on its hover,
/// where it costs no vertical space and cannot push the band to two lines.
///
/// 🚨 **The plate never asserts a model it has not been told about.** Clicking a row issues
/// `set_model`, whose ack carries no body; the new model is stated only by the *repeat*
/// `system/init` that follows. So the name on the plate stays the **confirmed** one and the
/// destination is drawn beside it, dim and arrowed, until the session says otherwise —
/// [`PendingModel`] carries the full argument.
fn model_plate(
    ui: &mut egui::Ui,
    content: &StripContent,
    models: &[ModelRow],
) -> Option<StripAct> {
    let plate = Frame::new()
        .fill(MODEL_FILL)
        .stroke(egui::Stroke::new(MODEL_STROKE, MODEL_EDGE))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(MODEL_PAD_X, MODEL_PAD_Y))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            match &content.model {
                ModelSlot::Named(label) => {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&label.name).color(MODEL_TEXT).strong().monospace(),
                        )
                        .truncate(),
                    );
                    if let Some(variant) = &label.variant {
                        ui.label(RichText::new(variant).color(MODEL_BADGE).small().monospace());
                    }
                }
                // Never an empty box and never "None": a plate with nothing in it during
                // connection reads as broken, which is worse than the strip being honest
                // about not knowing yet.
                ModelSlot::Connecting => {
                    ui.label(RichText::new("no model yet").color(DIM).small().italics());
                }
                ModelSlot::Absent => {
                    ui.label(RichText::new("no model").color(DIM).small().italics());
                }
            }
            if let Some(pending) = &content.pending_model {
                // Dim, italic and arrowed: it reads as a destination rather than as the
                // identity beside it, which is exactly the distinction being kept.
                ui.label(
                    RichText::new(format!("→ {pending}")).color(DIM).small().italics(),
                );
            }
        });
    let response = plate
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if !content.identity.is_empty() {
        response.clone().on_hover_ui(|ui| {
            for (label, value) in &content.identity {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{label}:")).color(DIM).small().monospace());
                    ui.label(RichText::new(value).color(PROSE).small().monospace());
                });
            }
        });
    }
    egui::Popup::menu(&response)
        .show(|ui| model_picker(ui, models))
        .and_then(|inner| inner.inner)
}

/// The model menu, built from the CLI's own `models` array.
///
/// An empty list is the honest case, not a failure: `initialize` is asked once at spawn and
/// its answer may not have arrived, or the session may not have answered it at all. The
/// picker says so rather than offering a table this build invented — see [`model_rows`].
fn model_picker(ui: &mut egui::Ui, models: &[ModelRow]) -> Option<StripAct> {
    ui.set_min_width(240.0);
    if models.is_empty() {
        ui.label(
            RichText::new("the model list has not arrived — this session has not answered its \
                           `initialize` yet")
                .color(DIM)
                .small()
                .italics(),
        );
        return None;
    }
    let mut act = None;
    for row in models {
        let mut text = RichText::new(&row.label).monospace();
        if row.current {
            text = text.color(MODEL_TEXT).strong();
        }
        let mut button = ui.add(egui::Button::new(text).frame(false));
        if let Some(detail) = &row.detail {
            button = button.on_hover_text(detail);
        }
        if button.clicked() && !row.current {
            act = Some(StripAct::ChooseModel(row.clone()));
            ui.close();
        }
        if row.current {
            ui.label(RichText::new("in use").color(DIM).small());
        }
    }
    ui.separator();
    // 📌 One dim line, not a dialog. See [`MODEL_SWITCH_COST`].
    ui.label(RichText::new(MODEL_SWITCH_COST).color(DIM).small());
    act
}

/// **The control that can quietly take the console's authority away** — and the band's
/// standing statement about whether it still has it.
///
/// 🚨 The plate is present in every state, so a hand always has somewhere to look; the
/// *marker* beside it appears exactly when the mode is not `default` and stays for as long
/// as that is true. That persistence is the design, not an oversight — the section note
/// above [`ModeSeverity`] carries the argument, and it is the reason there is no
/// confirmation dialog anywhere in this path.
fn mode_plate(ui: &mut egui::Ui, content: &StripContent) -> Option<StripAct> {
    let Some(mode) = content.mode.mode.as_deref() else {
        // Before the first init the console does not know the mode, and a control that
        // offers to change something unknown is a control that can silently change it to
        // what it already was.
        return None;
    };
    let severity = content.mode.marker.as_ref().map(|m| m.severity);
    let accent = match severity {
        Some(ModeSeverity::Alert) => MODE_ALERT,
        Some(ModeSeverity::Note) => MODE_NOTE,
        None => DIM,
    };
    let plate = Frame::new()
        .fill(MODEL_FILL)
        .stroke(egui::Stroke::new(MODEL_STROKE, accent))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(MODEL_PAD_X, MODEL_PAD_Y))
        .show(ui, |ui| {
            ui.label(RichText::new(mode).color(accent).small().monospace());
        });
    let response = plate
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let response = match mode_consequence(mode) {
        Some(consequence) => response.on_hover_text(consequence),
        None => response,
    };
    // The persistent marker. Truncated like everything else on the band, because one line
    // is one line — the whole sentence is on the plate's hover.
    if let Some(marker) = &content.mode.marker {
        ui.add(
            egui::Label::new(RichText::new(&marker.text).color(accent).small()).truncate(),
        );
    }
    egui::Popup::menu(&response)
        .show(|ui| mode_picker(ui, mode))
        .and_then(|inner| inner.inner)
}

/// The three modes, each labelled by what happens.
fn mode_picker(ui: &mut egui::Ui, current: &str) -> Option<StripAct> {
    ui.set_min_width(320.0);
    let mut act = None;
    for row in MODE_ROWS {
        let accent = match row.severity {
            ModeSeverity::Alert => MODE_ALERT,
            ModeSeverity::Note => PROSE,
        };
        let chosen = row.value == current;
        let mut name = RichText::new(row.value).monospace().color(accent);
        if chosen {
            name = name.strong();
        }
        let clicked = ui.add(egui::Button::new(name).frame(false)).clicked();
        // The consequence is not a tooltip: it is the label. A hover would put the one
        // sentence that matters behind a gesture nobody makes while deciding.
        ui.label(RichText::new(row.consequence).color(if chosen { PROSE } else { DIM }).small());
        if chosen {
            ui.label(RichText::new("in use").color(DIM).small());
        }
        ui.add_space(4.0);
        if clicked && !chosen {
            act = Some(StripAct::ChooseMode(row.value));
            ui.close();
        }
    }
    act
}

/// What the composer does with one key press.
///
/// The decision is a free function so it can be tested with literal values, the way
/// [`crate::term::encode_key`] is: the whole hazard here is egui's *shift-permissive*
/// modifier matching, and a table of plain values is the only place that is legible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerKey {
    /// Send what is in the box.
    Send,
    /// Insert a line break. **Performed by the widget, not by us** — this arm mirrors the
    /// predicate `TextEdit::return_key` will apply, so the two can be read side by side.
    Newline,
    /// Neither. Falls through to whatever else wants it.
    Ignore,
}

/// Enter sends; Shift+Enter breaks the line; every other Enter does nothing.
///
/// ⚠️ **The obvious spellings of this are all wrong**, because
/// [`egui::Modifiers::matches_logically`] is *permissive about shift*: if the pattern does
/// not ask for shift, a press **with** shift still matches it. So `consume_key(NONE, Enter)`
/// eats Shift+Enter as well, and `key_pressed(Enter)` ignores modifiers outright. The way
/// out is to be exact where it matters: [`egui::Modifiers::matches_exact`] against `NONE` is
/// true for a bare Enter and for nothing else.
///
/// **Ctrl+Enter and Alt+Enter are deliberately [`ComposerKey::Ignore`], not
/// [`ComposerKey::Send`].** Both are send-shortcuts in *some* chat client, and guessing
/// wrong sends a half-written message — the one failure this box must not have. Ignoring
/// them leaves the modified press free to mean something later without breaking anyone's
/// muscle memory in the meantime.
///
/// The [`ComposerKey::Newline`] arm reproduces the widget's own test
/// (`modifiers.matches_logically(SHIFT)`), which — via
/// [`egui::Modifiers::cmd_ctrl_matches`] — rejects Ctrl+Shift+Enter while accepting bare
/// Shift+Enter. That is why it is a separate arm from `Ignore` even though nothing in this
/// file acts on it: it is the contract the tests pin.
pub fn composer_key(key: egui::Key, mods: egui::Modifiers) -> ComposerKey {
    if key != egui::Key::Enter {
        return ComposerKey::Ignore;
    }
    if mods.matches_exact(egui::Modifiers::NONE) {
        return ComposerKey::Send;
    }
    if mods.matches_logically(egui::Modifiers::SHIFT) {
        return ComposerKey::Newline;
    }
    ComposerKey::Ignore
}

/// The composer's floor, in rows. **This is "big by default"** — the box opens at three
/// rows whether or not there is anything in it, because a one-line field is what makes an
/// input read as an afterthought.
const COMPOSER_ROWS: usize = 3;
/// …and its ceiling, in the same rows. Past this the box stops growing and starts
/// scrolling: the scrollback is the other half of a bottom-up layout, and a pasted essay
/// must not be able to eat it. [`egui::TextEdit`] has no maximum-height knob of its own
/// (`clip_text` is a no-op on multiline), so the cap is the band [`composer_box`] reserves,
/// with an [`egui::ScrollArea`] inside it to make the overflow reachable.
const COMPOSER_MAX_ROWS: f32 = 12.0;

/// The plate's padding and edge width, in points. Named because the band the composer
/// reserves is `text height + this chrome`, and the two must not drift apart.
const COMPOSER_PAD_X: i8 = 10;
const COMPOSER_PAD_Y: i8 = 8;
const COMPOSER_STROKE: f32 = 1.0;

/// The composer's plate, and its edge at rest, focused, and dead.
const COMPOSER_FILL: Color32 = Color32::from_rgb(0x0e, 0x14, 0x0e);
const COMPOSER_EDGE: Color32 = Color32::from_rgb(0x2b, 0x38, 0x2b);
const COMPOSER_EDGE_FOCUS: Color32 = Color32::from_rgb(0x4c, 0x7a, 0x52);
const COMPOSER_EDGE_DEAD: Color32 = Color32::from_rgb(0x3a, 0x2c, 0x2c);

/// What the hint teaches while the box is empty. The keystroke contract is written here
/// rather than shown as a permanent caption, because a caption that is always on screen is
/// a row of chrome the box pays for on every frame — and this one stops being news after
/// the first message.
const COMPOSER_HINT: &str = "message the agent — Enter sends, Shift+Enter for a new line";
const COMPOSER_HINT_DEAD: &str = "the agent is not running";

fn composer(ui: &mut egui::Ui, pane: &mut ConversationPane) {
    let live = pane.failure.is_none();
    // Three disjoint fields, borrowed separately, so the box can own the text while
    // `submit` still needs the whole pane afterwards.
    let submit = composer_box(
        ui,
        &mut pane.composer,
        live,
        &mut pane.want_focus,
        &mut pane.composer_height,
    );
    if submit {
        pane.submit();
    }
}

/// Draw the composer and report whether this frame asked for it to be sent.
///
/// Split out from [`composer`] with nothing but a `&mut String` because the Enter contract
/// has to be driven headless, and a [`ConversationPane`] would mean spawning a real agent
/// process to test a keystroke.
///
/// # Two things here are not the obvious spelling, and both were measured
///
/// ⚠️ **[`egui::Response::lost_focus`] cannot be the submit trigger.** The only
/// `surrender_focus` on Enter is in `TextEdit`'s *singleline* branch; a multiline box keeps
/// focus straight through, so the old `lost_focus() && key_pressed(Enter)` test would never
/// fire again — silently, with a green build. The guard is [`egui::Response::has_focus`]
/// instead, and keeping focus across a send then costs nothing, since nothing takes it away.
///
/// 🚨 **A vertical [`egui::ScrollArea`] cannot be dropped straight into a
/// [`egui::Layout::bottom_up`] column.** It places itself at `available_rect_before_wrap()
/// .min` — the *top* of the remaining space — while a bottom-up cursor sits at the bottom,
/// so allocating it collapses the whole column: measured at **684 pt of a 684 pt pane, for
/// one row of text, with `max_height` set to 100**. `ui.vertical`, `ui.scope` and an
/// enclosing `Frame` all inherit the same failure; `ui.horizontal` places correctly but
/// then pins the area to one row. So the composer **reserves its band first**
/// (`allocate_ui_with_layout`, which does go through the placer and therefore lands on the
/// cursor) and lays out top-down inside it. The band's height is the text's height from the
/// previous frame — read from [`egui::scroll_area::ScrollAreaOutput::content_size`], which
/// is the *unclipped* content and so cannot feed back on the band that clips it. Growth
/// therefore lands one frame late, which is the same trade egui's own panels make.
fn composer_box(
    ui: &mut egui::Ui,
    text: &mut String,
    live: bool,
    want_focus: &mut bool,
    measured: &mut f32,
) -> bool {
    let row = ui.text_style_height(&egui::TextStyle::Monospace);
    // The floor is what makes the box big before anything is in it; the ceiling is what
    // stops a pasted essay from eating the scrollback.
    let inner = measured.clamp(row * COMPOSER_ROWS as f32, row * COMPOSER_MAX_ROWS);
    let band = inner + 2.0 * COMPOSER_PAD_Y as f32 + 2.0 * COMPOSER_STROKE;

    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), band),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            // `begin`/`end` rather than `show`, so the edge can be coloured by a focus state
            // that is only known once the widget inside has run. The frame shape is inserted
            // behind the content either way, so this costs nothing.
            let mut framed = Frame::new()
                .fill(COMPOSER_FILL)
                .stroke(egui::Stroke::new(COMPOSER_STROKE, COMPOSER_EDGE))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(COMPOSER_PAD_X, COMPOSER_PAD_Y))
                .begin(ui);

            let (submit, focused) = {
                let ui = &mut framed.content_ui;
                ui.set_width(ui.available_width());
                // Fill the reserved band even when the text has just shrunk, so the plate
                // never leaves a one-frame gap under itself while the band catches up.
                ui.set_min_height(inner);
                let scrolled = egui::ScrollArea::vertical()
                    // Named, because a scroll offset that lives on a positional auto-id is
                    // one sibling away from belonging to something else.
                    .id_salt("composer")
                    .show(ui, |ui| {
                        let edit = egui::TextEdit::multiline(text)
                            .desired_rows(COMPOSER_ROWS)
                            .desired_width(f32::INFINITY)
                            // The enclosing `Frame` is the look; the widget's own would be a
                            // second border inside the first.
                            .frame(false)
                            .margin(Margin::ZERO)
                            .font(egui::TextStyle::Monospace)
                            .text_color(if live { HUMAN } else { DIM })
                            // 🚨 The inversion that makes the keystrokes work: declaring
                            // **Shift**+Enter as the return key sets `pattern.shift`, and a
                            // pattern that asks for shift is the one case
                            // `matches_logically` is strict about — so a bare Enter no
                            // longer matches, and falls through the widget untouched for
                            // `composer_key` to read below.
                            .return_key(egui::KeyboardShortcut::new(
                                egui::Modifiers::SHIFT,
                                egui::Key::Enter,
                            ))
                            .hint_text(if live { COMPOSER_HINT } else { COMPOSER_HINT_DEAD });
                        let response = ui.add_enabled(live, edit);
                        if *want_focus && live {
                            response.request_focus();
                            *want_focus = false;
                        }
                        let focused = response.has_focus();
                        // Read, never consumed: egui hands widgets a *clone* of the event
                        // list, so the Enter the `TextEdit` declined is still here — and
                        // consuming it through `consume_key` would take Shift+Enter with it,
                        // for the reason `composer_key` documents.
                        let submit = focused
                            && ui.input(|i| {
                                i.events.iter().any(|event| {
                                    matches!(
                                        event,
                                        egui::Event::Key { key, pressed: true, modifiers, .. }
                                            if composer_key(*key, *modifiers) == ComposerKey::Send
                                    )
                                })
                            });
                        (submit, focused)
                    });
                *measured = scrolled.content_size.y;
                scrolled.inner
            };

            framed.frame.stroke = egui::Stroke::new(
                COMPOSER_STROKE,
                match (live, focused) {
                    (false, _) => COMPOSER_EDGE_DEAD,
                    (true, true) => COMPOSER_EDGE_FOCUS,
                    (true, false) => COMPOSER_EDGE,
                },
            );
            framed.end(ui);
            submit
        },
    )
    .inner
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

/// How a gated tool names itself on an approval card.
///
/// **MCP's real value here is legibility, not permission** (§"What this decides"): approvals
/// are answered either way, so the argument for naming tools as capabilities is that a card
/// can say *"organon · background"* rather than `mcp__organon__background`. The namespacing
/// is the client's wire spelling and carries no information a human wants — but the server
/// it came from does, so it is kept and separated rather than stripped.
///
/// A built-in (`Bash`, `Write`) is already the name a human reads, and is left alone.
pub fn capability_label(tool_name: &str) -> String {
    let Some(rest) = tool_name.strip_prefix("mcp__") else {
        return tool_name.to_string();
    };
    match rest.split_once("__") {
        Some((server, tool)) if !server.is_empty() && !tool.is_empty() => {
            format!("{server} · {tool}")
        }
        _ => tool_name.to_string(),
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

    fn step(act: SubagentAct, depth: u8) -> crate::conversation::SubagentStep {
        crate::conversation::SubagentStep { act, depth }
    }

    fn tool_step(name: &str, state: StepState, depth: u8) -> crate::conversation::SubagentStep {
        step(SubagentAct::Tool { id: name.into(), name: Some(name.into()), state }, depth)
    }

    fn log_of(steps: Vec<crate::conversation::SubagentStep>, dropped: u64) -> SubagentLog {
        SubagentLog { steps: steps.into(), dropped }
    }

    /// The header counts what happened, **including what the log no longer holds** — a
    /// trace that silently starts in the middle reads as the whole trace.
    #[test]
    fn the_subagent_header_counts_dropped_steps_too() {
        let log = log_of(vec![step(SubagentAct::Said("a".into()), 1)], 40);
        assert_eq!(subagent_summary(&log, true).counts, "· 41 steps");
        let one = log_of(vec![step(SubagentAct::Said("a".into()), 1)], 0);
        assert_eq!(subagent_summary(&one, true).counts, "· 1 step", "singular reads as one");
    }

    /// Depth is stated only when there is nesting to state. A flattened depth-2 step read
    /// as direct misattributes the work; a depth badge on every ordinary card is noise.
    #[test]
    fn the_subagent_header_mentions_depth_only_when_it_is_nested() {
        let flat = log_of(vec![step(SubagentAct::Said("a".into()), 1)], 0);
        assert_eq!(subagent_summary(&flat, true).counts, "· 1 step");
        let nested = log_of(
            vec![step(SubagentAct::Said("a".into()), 1), step(SubagentAct::Said("b".into()), 3)],
            0,
        );
        assert_eq!(subagent_summary(&nested, true).counts, "· 2 steps · nested 3 deep");
    }

    /// 🚨 CONTRACT — the same open-step count means two different things, and the parent is
    /// what decides which. While the parent runs it is work in flight; once the parent has
    /// returned it means the subagent stopped without those tools ever coming back. The
    /// header must not report the second as the first.
    #[test]
    fn an_open_step_reads_differently_once_the_parent_has_returned() {
        let log = log_of(vec![tool_step("Grep", StepState::Running, 1)], 0);
        assert_eq!(subagent_summary(&log, true).open.as_deref(), Some("· 1 out"));
        assert_eq!(
            subagent_summary(&log, false).open.as_deref(),
            Some("· 1 never returned"),
            "a finished parent with an open step is not 'working'"
        );
    }

    /// Nothing open, nothing said about it — the header stays quiet rather than printing a
    /// zero.
    #[test]
    fn a_fully_returned_subagent_says_nothing_about_open_steps() {
        let log = log_of(
            vec![tool_step("Grep", StepState::Done { is_error: false }, 1)],
            0,
        );
        assert_eq!(subagent_summary(&log, true).open, None);
        assert_eq!(subagent_summary(&log, false).open, None);
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
    fn only_an_exact_slash_surface_is_a_local_command() {
        assert_eq!(local_command("/surface"), Some(LocalCommand::Surface));
        assert_eq!(local_command(" /surface \n"), Some(LocalCommand::Surface), "trimmed like a send");
        for message in [
            "/surfaces",
            "/surface slate",
            "surface",
            "/",
            "",
            // 🚨 The retired command. `/panel` drove the console's backdrop, which a
            // conversation has no scrollback to show, so its controls appeared to do
            // nothing. It is not swallowed and it is not aliased to `/surface` — it is an
            // ordinary message now, and reaches the agent like any other sentence.
            "/panel",
            "  /panel  ",
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
            drives: ElementId(1),
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
            drives: ElementId(7),
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

    /// An approval card names a **capability**, not a wire identifier — the honest reason
    /// for putting the console's verbs on MCP at all, since approvals are answered either
    /// way. A name that is not namespaced is already the one a human reads.
    #[test]
    fn a_gated_tool_is_named_the_way_a_human_reads_it() {
        assert_eq!(capability_label("mcp__organon__background"), "organon · background");
        assert_eq!(capability_label("mcp__probe__echo_probe"), "probe · echo_probe");
        assert_eq!(capability_label("Bash"), "Bash", "a built-in is already legible");
        assert_eq!(capability_label("Write"), "Write");
        // Nothing that is not the measured shape is reinterpreted into one.
        assert_eq!(capability_label("mcp__lonely"), "mcp__lonely");
        assert_eq!(capability_label("mcp____x"), "mcp____x", "an empty server name is not a name");
        assert_eq!(capability_label(""), "");
    }

    /// The arguments a card shows are the model's **final** input, so they go through the
    /// settled path — a permission request never carries a fragment. This pins the join
    /// between the request's JSON text and the rows the card draws.
    #[test]
    fn an_approvals_arguments_render_as_the_fields_a_human_is_authorising() {
        let input = serde_json::json!({ "command": "cargo build --release" }).to_string();
        let fields = argument_fields(&Arguments { text: input, complete: true });
        assert_eq!(fields, vec![("command".to_string(), "cargo build --release".to_string())]);
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

    // -----------------------------------------------------------------------
    // The composer
    // -----------------------------------------------------------------------

    /// The whole keystroke contract, as literal values.
    ///
    /// Enter sends and Shift+Enter breaks the line — the shape Claude Desktop, Slack and
    /// every other composer worth copying uses. Ctrl+Enter and Alt+Enter do **neither**:
    /// each is "send" in some client and "newline" in another, and a wrong guess sends a
    /// half-written message, so they are left free rather than assigned.
    #[test]
    fn enter_sends_and_shift_enter_breaks_the_line() {
        use egui::{Key, Modifiers};
        assert_eq!(composer_key(Key::Enter, Modifiers::NONE), ComposerKey::Send);
        assert_eq!(composer_key(Key::Enter, Modifiers::SHIFT), ComposerKey::Newline);
        assert_eq!(
            composer_key(Key::Enter, Modifiers::CTRL),
            ComposerKey::Ignore,
            "Ctrl+Enter must not send — the cost of guessing wrong is a half-written message"
        );
        assert_eq!(composer_key(Key::Enter, Modifiers::ALT), ComposerKey::Ignore);
        assert_eq!(
            composer_key(Key::Enter, Modifiers::CTRL | Modifiers::SHIFT),
            ComposerKey::Ignore,
            "and the widget will not insert a newline for it either"
        );
    }

    /// Nothing that is not Enter is the composer's business.
    #[test]
    fn any_other_key_falls_through() {
        use egui::{Key, Modifiers};
        for key in [Key::A, Key::Tab, Key::Escape, Key::ArrowDown, Key::Space] {
            assert_eq!(composer_key(key, Modifiers::NONE), ComposerKey::Ignore, "{key:?}");
            assert_eq!(composer_key(key, Modifiers::SHIFT), ComposerKey::Ignore, "{key:?}");
        }
    }

    /// One frame of the composer, headless. Returns whether it asked to send, the text
    /// after the frame, and how much vertical space the box took.
    ///
    /// `egui::RawInput::default()` carries `focused: true`, which is what makes
    /// `Response::has_focus` mean anything with no window in sight.
    fn composer_frame(
        ctx: &egui::Context,
        pane: &mut FakePane,
        events: Vec<egui::Event>,
    ) -> (bool, f32) {
        let mut submitted = false;
        let mut height = 0.0;
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                // The real layout: the composer sits at the bottom of a bottom-up column,
                // which is the arrangement the height assertions below are about.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    // Measured as the room the composer took *away from what follows it*,
                    // which is the question a bottom-up column actually asks — and unlike
                    // `min_rect`, it means the same thing in either layout direction.
                    let before = ui.available_height();
                    submitted = composer_box(
                        ui,
                        &mut pane.text,
                        pane.live,
                        &mut pane.want_focus,
                        &mut pane.measured,
                    );
                    height = before - ui.available_height();
                });
            });
        });
        (submitted, height)
    }

    /// The three pieces of a [`ConversationPane`] the composer actually touches — a real
    /// one would spawn an agent process to test a keystroke.
    struct FakePane {
        text: String,
        live: bool,
        want_focus: bool,
        measured: f32,
    }

    impl FakePane {
        fn new(text: &str) -> Self {
            Self { text: text.to_string(), live: true, want_focus: true, measured: 0.0 }
        }
    }

    fn enter(modifiers: egui::Modifiers) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }]
    }

    /// 🚨 **The contract a version bump can silently break.**
    ///
    /// `Modifiers::matches_logically` is shift-permissive, so the default `return_key`
    /// cannot tell Enter from Shift+Enter and neither can `consume_key`. The composer works
    /// around that by *inverting* the shortcut — Shift+Enter is declared as the return key,
    /// leaving a bare Enter to fall through for `composer_key` to read. That is a behaviour
    /// of egui, not of this file, and `native/tests/egui_popup_contract.rs` exists because a
    /// 0.31→0.33 change of exactly this kind already killed a keypress here once. So it is
    /// pinned by driving real events through a real frame.
    #[test]
    fn enter_submits_and_shift_enter_types_a_newline() {
        let ctx = egui::Context::default();
        let mut pane = FakePane::new("hello");
        // One frame to take focus; the request only lands for the frame after it.
        for _ in 0..2 {
            let (idle, _) = composer_frame(&ctx, &mut pane, Vec::new());
            assert!(!idle, "an empty frame sends nothing");
        }

        let (sent, _) = composer_frame(&ctx, &mut pane, enter(egui::Modifiers::NONE));
        assert!(sent, "a bare Enter must reach the caller as a send");
        assert_eq!(pane.text, "hello", "and must NOT have been typed into the box");

        let (sent, _) = composer_frame(&ctx, &mut pane, enter(egui::Modifiers::SHIFT));
        assert!(!sent, "Shift+Enter must not send");
        assert_eq!(pane.text, "hello\n", "Shift+Enter is the newline");
    }

    /// A dead agent's composer is disabled, so nothing it is typed at can be sent.
    #[test]
    fn a_dead_composer_never_submits() {
        let ctx = egui::Context::default();
        let mut pane = FakePane::new("hello");
        pane.live = false;
        let mut submitted = false;
        for events in [Vec::new(), enter(egui::Modifiers::NONE), enter(egui::Modifiers::NONE)] {
            let (sent, _) = composer_frame(&ctx, &mut pane, events);
            submitted |= sent;
        }
        assert!(!submitted, "a disabled composer cannot take focus, so it cannot send");
        assert_eq!(pane.text, "hello", "nor can it be typed into");
    }

    /// **The layout contract, which the egui source could not answer and a probe had to.**
    ///
    /// The box grows from a three-row floor and stops at a twelve-row ceiling, and — the
    /// part that matters for the pane as a whole — the space it takes out of the bottom-up
    /// column is that height and not the whole column. The naive spelling of this (a
    /// `ScrollArea` with `max_height`, dropped into the bottom-up layout) measured **684 pt
    /// of a 684 pt pane at every content size**, i.e. a scrollback with nothing left; see
    /// [`composer_box`] for why and for what replaced it.
    #[test]
    fn the_box_grows_from_three_rows_and_stops_at_the_ceiling() {
        let mut heights = Vec::new();
        // 8 sits between the floor and the ceiling; 40 and 200 are both past it.
        for rows in [1usize, 3, 8, 40, 200] {
            let ctx = egui::Context::default();
            let mut pane = FakePane::new(&vec!["x"; rows].join("\n"));
            pane.want_focus = false;
            // Three frames: the band follows the text by one, deliberately.
            let mut height = 0.0;
            for _ in 0..3 {
                height = composer_frame(&ctx, &mut pane, Vec::new()).1;
            }
            heights.push(height);
        }
        let [one, three, eight, forty, lots] = heights[..] else { unreachable!() };
        assert!(one > 0.0, "the box must occupy real space with one row in it: {one}");
        assert!(one < 250.0, "and must not swallow the 700 pt pane it sits in: {one}");
        assert_eq!(
            one, three,
            "one row and three rows must be the same height — three is the FLOOR, not a fit"
        );
        assert!(eight > three, "eight rows must have grown past the floor: {three} -> {eight}");
        assert!(forty > eight, "and kept growing towards the ceiling: {eight} -> {forty}");
        assert_eq!(
            forty, lots,
            "past the ceiling it must stop growing and scroll instead: {forty} vs {lots}"
        );
        // The ceiling is a stated number of rows, not whatever fell out of the layout.
        assert!(
            forty < 4.0 * three,
            "twelve rows must be near four times the three-row floor: {three} -> {forty}"
        );
    }

    // -----------------------------------------------------------------------
    // The status strip
    // -----------------------------------------------------------------------

    /// A session that has said `system/init` and nothing else.
    fn started(model: &str) -> SessionFacts {
        SessionFacts {
            model: Some(model.to_string()),
            cwd: Some("C:/work".into()),
            permission_mode: Some("default".into()),
            cli_version: Some("2.1.228".into()),
            tools: 17,
            ..Default::default()
        }
    }

    /// A session with both halves of a context reading measured.
    fn filled(prompt: u64, window: u64) -> SessionFacts {
        SessionFacts {
            context_window: Some(window),
            last_prompt_tokens: Some(prompt),
            ..started("claude-opus-5[1m]")
        }
    }

    /// ⚠️ CONTRACT: no ring until both halves are measured. An empty ring reads as
    /// "0% full", which is a specific claim the console cannot make before a `result`
    /// has stated a window and a `message_start` has stated a prompt.
    #[test]
    fn the_band_carries_no_ring_until_both_halves_are_measured() {
        let cold = strip_content(None, LiveCounts::default(), &SessionFacts::default(), None, None);
        assert_eq!(cold.context, ContextSlot::Unknown, "nothing measured");

        let window_only = SessionFacts { context_window: Some(1_000_000), ..started("m") };
        let half = strip_content(None, live(0, 0), &window_only, None, None);
        assert_eq!(
            half.context,
            ContextSlot::Unknown,
            "a denominator alone is not a proportion"
        );

        let both = strip_content(None, live(0, 0), &filled(54_050, 1_000_000), None, None);
        let ContextSlot::Known(fill) = both.context else {
            panic!("{:?}", both.context)
        };
        assert_eq!(fill.percent(), 5);
    }

    /// CONTRACT: amber at exactly [`CONTEXT_HIGH_PERCENT`], not a token before it. The
    /// threshold is the console's own judgement and is therefore the one number here a
    /// reader could reasonably argue with — so it is pinned rather than approximated.
    #[test]
    fn the_ring_turns_amber_at_three_quarters_and_not_before() {
        let at = |prompt| strip_content(None, live(0, 0), &filled(prompt, 1_000), None, None).context;
        assert!(!at(749).is_high(), "74.9% is still blue");
        assert!(at(750).is_high(), "75.0% is the boundary and it counts as high");
        assert!(at(1_000).is_high());
        assert!(!ContextSlot::Unknown.is_high(), "no reading is not a high reading");
    }

    /// CONTRACT: the ring is a proportion of the window and nothing else — it does not
    /// grow with the session, and a compaction that shrinks the prompt shrinks the ring.
    /// Latest-wins all the way through, which is what makes that true for free.
    #[test]
    fn the_ring_follows_the_last_prompt_down_as_well_as_up() {
        let grown = strip_content(None, live(0, 0), &filled(800_000, 1_000_000), None, None);
        let compacted = strip_content(None, live(0, 0), &filled(120_000, 1_000_000), None, None);
        assert!(grown.context.is_high());
        assert!(!compacted.context.is_high(), "a compacted context is not a full one");
        let ContextSlot::Known(fill) = compacted.context else {
            panic!("{:?}", compacted.context)
        };
        assert_eq!(fill.percent(), 12);
    }

    /// The hover is where the reading states what it is, so it must say *which* request
    /// it describes and where both numbers came from.
    #[test]
    fn the_rings_hover_names_the_last_request_and_its_two_sources() {
        let fill = ContextFill { prompt_tokens: 54_050, context_window: 1_000_000 };
        let rows = context_rows(&fill);
        let joined = rows
            .iter()
            .map(|(label, value)| format!("{label}: {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("at the last request"), "{joined}");
        assert!(joined.contains("54,050"), "the prompt, readable: {joined}");
        assert!(joined.contains("1,000,000"), "the window, readable: {joined}");
        assert!(joined.contains("message_start.usage"), "provenance: {joined}");
        assert!(joined.contains("modelUsage.contextWindow"), "provenance: {joined}");
    }

    fn live(pending: usize, running: usize) -> LiveCounts {
        LiveCounts {
            pending_approvals: pending,
            running_tools: running,
            remembered: 0,
            has_session: true,
            generating: false,
        }
    }

    /// The same, with an assistant message open — the state the strip could not see at all
    /// before [`Standing::Generating`] existed.
    fn live_generating(pending: usize, running: usize) -> LiveCounts {
        LiveCounts { generating: true, ..live(pending, running) }
    }

    /// **The cold start, which every session opens in.** Before `system/init` there is no
    /// model, no session id and no facts at all — and a band that answers that with an empty
    /// plate or the word "None" is worse than one that says it is connecting.
    #[test]
    fn before_the_first_line_the_strip_says_it_is_connecting() {
        let cold = LiveCounts::default();
        assert!(!cold.generating, "nothing has opened a message, so nothing claims one is open");
        let content = strip_content(None, cold, &SessionFacts::default(), None, None);
        assert_eq!(content.model, ModelSlot::Connecting, "the plate says it is coming");
        assert_eq!(content.reading.standing, Standing::Connecting);
        assert_eq!(
            content.reading.text, "",
            "and the status half stays silent rather than saying it a second time"
        );
        assert!(content.identity.is_empty(), "nothing is known, so the hover claims nothing");
        assert!(content.chips.is_empty(), "no cost, no memory, no turn — no chips");
    }

    /// Once init arrives the model is the headline, and everything else it said is on the
    /// hover rather than on the band.
    #[test]
    fn the_model_becomes_the_headline_once_init_arrives() {
        let content =
            strip_content(None, live(0, 0), &started("claude-opus-5[1m]"), Some("abc-123"), None);
        let ModelSlot::Named(label) = &content.model else { panic!("{:?}", content.model) };
        assert_eq!(label.name, "claude-opus-5");
        assert_eq!(label.variant.as_deref(), Some("1M"));
        assert_eq!(content.reading.standing, Standing::Ready);
        assert_eq!(content.reading.text, "ready");
        // The identity is real and complete, and none of it is on the band itself.
        let rows: Vec<&str> = content.identity.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(rows, vec!["model", "permissions", "cli", "cwd", "tools", "session"]);
        assert_eq!(
            content.identity[0].1, "claude-opus-5[1m]",
            "the hover carries the string the CLI actually reported, suffix and all"
        );
    }

    /// A dead agent outranks everything: nothing else on the band describes a process that
    /// still exists.
    #[test]
    fn a_dead_agent_outranks_everything() {
        let mut facts = started("claude-opus-5");
        facts.needs_action = Some("answer the question".into());
        let content = strip_content(
            Some("the agent stopped listening: broken pipe"),
            LiveCounts {
                pending_approvals: 2,
                running_tools: 3,
                remembered: 1,
                has_session: true,
                generating: true,
            },
            &facts,
            Some("abc-123"),
            None,
        );
        assert_eq!(content.reading.standing, Standing::Dead);
        assert_eq!(content.reading.text, "the agent stopped listening: broken pipe");
    }

    /// Waiting on a human outranks working, because only one of the two the agent can finish
    /// on its own. This is the ordering the first status line had; it is kept, not rederived.
    #[test]
    fn waiting_on_a_human_outranks_working() {
        let facts = started("claude-opus-5");
        let both = strip_content(None, live(1, 4), &facts, Some("abc"), None);
        assert_eq!(both.reading.standing, Standing::Asking);
        assert_eq!(both.reading.text, "◈ 1 permission request — waiting on you");

        let working = strip_content(None, live(0, 4), &facts, Some("abc"), None);
        assert_eq!(working.reading.standing, Standing::Working);
        assert_eq!(
            working.reading.text, "● 4 tools running",
            "and it says TOOLS — `is_working` is tool-derived, so calling it thinking would be \
             false exactly when a model is writing prose with nothing in flight"
        );
    }

    /// `needs_action` is the agent's own sentence about what it wants, and it is surfaced —
    /// but it describes a turn that has **ended**, so live work supersedes it. The mapper only
    /// clears the field on the next `post_turn_summary`, so without this a demand the human
    /// already answered would sit on the band for the whole of the next turn.
    #[test]
    fn a_finished_turns_demand_is_surfaced_and_yields_to_live_work() {
        let mut facts = started("claude-opus-5");
        facts.needs_action = Some("pick one of the three options".into());
        facts.last_status_detail = Some("asked a question".into());

        let idle = strip_content(None, live(0, 0), &facts, Some("abc"), None);
        assert_eq!(idle.reading.standing, Standing::Asking);
        assert_eq!(idle.reading.text, "◈ pick one of the three options");

        let resumed = strip_content(None, live(0, 1), &facts, Some("abc"), None);
        assert_eq!(resumed.reading.standing, Standing::Working);
        assert_eq!(resumed.reading.text, "● 1 tool running");
    }

    /// **The hole this closed.** A model writing prose with no tool in flight used to fall all
    /// the way through to "ready" — which is most of a turn reading as though nothing were
    /// happening. It now reports the `message_start` … `message_stop` bracket, and it reports
    /// only that: no count, no rate, no bar, no estimate, because the wire carries none of
    /// them and each would have to be invented to be drawn.
    #[test]
    fn an_open_message_reports_generating_and_nothing_more_than_that() {
        let facts = started("claude-opus-5");
        let content = strip_content(None, live_generating(0, 0), &facts, Some("abc"), None);
        assert_eq!(content.reading.standing, Standing::Generating);
        assert_eq!(content.reading.text, "● generating");
        for invented in ["%", "/s", "tok", "eta", "left", "of"] {
            assert!(
                !content.reading.text.contains(invented),
                "the band must not imply a rate or a remainder ({invented}): {}",
                content.reading.text
            );
        }
        // The same session with the bracket closed is the old reading, unchanged.
        let closed = strip_content(None, live(0, 0), &facts, Some("abc"), None);
        assert_eq!(closed.reading.standing, Standing::Ready);
        assert_eq!(closed.reading.text, "ready");
    }

    /// **Where generating sits, and it sits in two places at once.**
    ///
    /// Below running tools, because a tool block opens *inside* the message that called it —
    /// so both are true for most of a turn and "3 tools running" is the sentence worth the one
    /// line. Above `needs_action`, because that describes a turn which has *ended* and is only
    /// cleared by the next `post_turn_summary`; tokens arriving now are live activity by
    /// exactly the measure a running tool is, and the rule that keeps a stale "waiting on you"
    /// off the band has to cover both.
    #[test]
    fn generating_yields_to_running_tools_and_supersedes_a_finished_demand() {
        let mut facts = started("claude-opus-5");
        facts.needs_action = Some("pick one of the three options".into());
        facts.last_status_detail = Some("asked a question".into());

        let with_tools = strip_content(None, live_generating(0, 2), &facts, Some("abc"), None);
        assert_eq!(with_tools.reading.standing, Standing::Working);
        assert_eq!(
            with_tools.reading.text, "● 2 tools running",
            "the specific reading wins: it names what is happening, generating only says that \
             something is"
        );

        let writing = strip_content(None, live_generating(0, 0), &facts, Some("abc"), None);
        assert_eq!(
            writing.reading.standing,
            Standing::Generating,
            "a demand the human already answered must not sit on the band through the reply \
             that answered it"
        );
        assert_eq!(writing.reading.text, "● generating");

        // …and the moment the bracket closes, the demand is what is left to say.
        let idle = strip_content(None, live(0, 0), &facts, Some("abc"), None);
        assert_eq!(idle.reading.standing, Standing::Asking);
        assert_eq!(idle.reading.text, "◈ pick one of the three options");
    }

    /// The two readings above generating still outrank it. A dead agent has no tokens
    /// arriving whatever the last thing it said was, and a pending question is the agent
    /// *halted* on a human — the one state it cannot get out of by itself.
    #[test]
    fn a_dead_agent_and_a_pending_question_both_still_outrank_generating() {
        let facts = started("claude-opus-5");
        let asked = strip_content(None, live_generating(1, 0), &facts, Some("abc"), None);
        assert_eq!(asked.reading.standing, Standing::Asking);
        assert_eq!(asked.reading.text, "◈ 1 permission request — waiting on you");

        // ⚠️ The clearing path there is no event for: the process dies mid-message, so the
        // mapper's state stays lit and nothing on the stream will ever put it out. `Dead`
        // outranking everything is what answers that, which is why it is not a clear.
        let gone = strip_content(
            Some("the agent process ended"),
            live_generating(0, 0),
            &facts,
            Some("abc"),
            None,
        );
        assert_eq!(gone.reading.standing, Standing::Dead);
        assert_eq!(gone.reading.text, "the agent process ended");
    }

    /// Busy is one colour. The distinction the palette has to carry is busy-versus-blocked,
    /// and busy-with-tools against busy-writing is the same answer to "can I walk away" —
    /// the text already spells out which.
    #[test]
    fn generating_and_working_read_as_the_same_kind_of_busy() {
        assert_eq!(standing_color(Standing::Generating), standing_color(Standing::Working));
        assert_ne!(standing_color(Standing::Generating), standing_color(Standing::Ready));
        assert_ne!(standing_color(Standing::Generating), standing_color(Standing::Asking));
    }

    /// With nothing outstanding the band reports the turn the agent described, not a guess.
    #[test]
    fn a_quiet_session_reports_what_the_last_turn_said() {
        let mut facts = started("claude-opus-5");
        facts.last_status_detail = Some("wrote the strip and ran the tests".into());
        let content = strip_content(None, live(0, 0), &facts, Some("abc"), None);
        assert_eq!(content.reading.standing, Standing::Ready);
        assert_eq!(content.reading.text, "wrote the strip and ran the tests");
    }

    /// 🚨 **The labelling rule, pinned.** `cost_usd` is cumulative on the wire and the token
    /// counts beside it are not, so the one money figure on the band says which kind it is —
    /// and no token total appears, because summing per-turn usage double-counts cache reads
    /// and there is no other source for one.
    #[test]
    fn the_chips_say_what_kind_of_number_they_are() {
        let mut facts = started("claude-opus-5");
        facts.cost_usd = Some(0.1234);
        facts.last_turn_duration_ms = Some(7_389);
        facts.last_turn_usage = Some(crate::agent_event::Usage {
            input_tokens: 12_000,
            output_tokens: 400,
            cache_creation_input_tokens: 900,
            cache_read_input_tokens: 50_000,
        });
        let counts = LiveCounts { remembered: 2, ..live(0, 0) };
        let content = strip_content(None, counts, &facts, Some("abc"), None);
        assert_eq!(
            content.chips,
            vec!["session $0.1234", "2 remembered decisions", "last turn 7.4s"]
        );
        let band = content.chips.join(" · ");
        assert!(!band.contains("token"), "no token figure is shown at all: {band}");
        for figure in ["12000", "12,000", "12.0k", "400", "50000", "62900"] {
            assert!(!band.contains(figure), "nor anything derived from one ({figure}): {band}");
        }
    }

    /// The two number formats, at the boundaries that decide them.
    #[test]
    fn costs_and_durations_read_the_way_a_human_would_say_them() {
        assert_eq!(cost_label(0.0), "$0.0000", "cents matter, so a turn never reads as free");
        assert_eq!(cost_label(0.1234), "$0.1234");
        assert_eq!(cost_label(12.5), "$12.50", "past a dollar the four decimals are noise");
        assert_eq!(duration_label(0), "0ms");
        assert_eq!(duration_label(999), "999ms");
        assert_eq!(duration_label(7_389), "7.4s");
        assert_eq!(duration_label(59_999), "60.0s");
        assert_eq!(duration_label(125_000), "2m05s");
    }

    /// ⚠️ **What the model transform drops, which is nothing.**
    ///
    /// The suffix is *relocated* into a badge and upper-cased, so the reported string is
    /// recoverable from the pair — and the verbatim spelling is on the hover regardless. What
    /// this test really pins is the refusal: an identifier the console does not recognise is
    /// passed through untouched rather than prettified into a name that would be wrong.
    #[test]
    fn the_model_suffix_is_relocated_not_dropped() {
        let one_m = model_label("claude-opus-5[1m]");
        assert_eq!(one_m.name, "claude-opus-5");
        assert_eq!(one_m.variant.as_deref(), Some("1M"), "upper-cased, and that is the only edit");
        assert_eq!(
            format!("{}[{}]", one_m.name, one_m.variant.unwrap().to_lowercase()),
            "claude-opus-5[1m]",
            "the reported string is recoverable from what is drawn"
        );

        // No suffix, and nothing invented.
        assert_eq!(model_label("claude-opus-5"), ModelLabel {
            name: "claude-opus-5".into(),
            variant: None
        });
        // A dated snapshot keeps its date: it is part of *which model*, not decoration.
        assert_eq!(model_label("claude-3-5-sonnet-20241022").name, "claude-3-5-sonnet-20241022");
        // A gateway's fully-qualified id survives intact — this is the case a nice-names
        // table would have mangled.
        assert_eq!(
            model_label("us.anthropic.claude-opus-5-v1:0").name,
            "us.anthropic.claude-opus-5-v1:0"
        );
        // Degenerate brackets are punctuation, not a variant.
        assert_eq!(model_label("weird[]").name, "weird[]");
        assert_eq!(model_label("[1m]").name, "[1m]");
        assert_eq!(model_label("").name, "");
    }

    // -----------------------------------------------------------------------
    // The two controls: the model picker and the permission mode
    // -----------------------------------------------------------------------

    /// The five rows the measured account was offered (§3), verbatim in shape — including
    /// the `haiku` row, which carries **no** `supportsEffort` and no display-name surprises.
    fn offered() -> Vec<ModelChoice> {
        let row = |value: &str, resolved: &str, display: &str, description: &str| ModelChoice {
            value: value.into(),
            resolved_model: Some(resolved.into()),
            display_name: Some(display.into()),
            description: Some(description.into()),
            supports_effort: true,
            effort_levels: vec!["low".into(), "high".into()],
        };
        vec![
            row(
                "default",
                "claude-opus-5[1m]",
                "Default (recommended)",
                "Opus 5 with 1M context · Best for everyday, complex tasks",
            ),
            row("opus[1m]", "claude-opus-5[1m]", "Opus (1M context)", "The big one"),
            row("sonnet", "claude-sonnet-5", "Sonnet", "Faster"),
            ModelChoice { value: "haiku".into(), ..Default::default() },
        ]
    }

    /// 📌 CONTRACT: the picker's rows come from the CLI's own `models` array — display
    /// names and all — and **nothing here invents a model**. The list is per-account and
    /// can gain a row after this build ships, which is the whole reason there is no table.
    #[test]
    fn the_picker_is_built_from_the_list_the_cli_offered() {
        let rows = model_rows(&offered(), Some("claude-sonnet-5"));
        assert_eq!(rows.len(), 4, "one row per offered model, no more and no fewer");
        assert_eq!(rows[0].label, "Default (recommended)", "the human-written name is the label");
        assert_eq!(rows[0].value, "default", "…and `set_model` still takes the wire value");
        assert_eq!(
            rows[2].detail.as_deref(),
            Some("Faster"),
            "the CLI's own sentence of guidance is carried, not paraphrased"
        );
        // A row the CLI sent bare falls back to its value rather than to an empty button.
        assert_eq!(rows[3].label, "haiku");
        assert_eq!(rows[3].detail, None);

        let current: Vec<&str> =
            rows.iter().filter(|r| r.current).map(|r| r.value.as_str()).collect();
        assert_eq!(current, vec!["sonnet"], "exactly the row in use is marked");
    }

    /// ⚠️ CONTRACT: two rows may both be current, and that is honest rather than a bug —
    /// `default` and `opus[1m]` resolve to the same model in the measured capture, so both
    /// genuinely name what is running. Matching on `resolvedModel` is what that field is
    /// documented to exist for.
    #[test]
    fn an_alias_and_the_row_it_resolves_to_are_both_in_use() {
        let rows = model_rows(&offered(), Some("claude-opus-5[1m]"));
        let current: Vec<&str> =
            rows.iter().filter(|r| r.current).map(|r| r.value.as_str()).collect();
        assert_eq!(current, vec!["default", "opus[1m]"]);
    }

    /// 📌 CONTRACT: no list is the normal state of a session that has not answered its
    /// `initialize` yet, and it degrades to an empty picker rather than to a guess.
    #[test]
    fn an_empty_model_list_degrades_to_nothing_rather_than_to_a_guess() {
        assert!(model_rows(&[], Some("claude-opus-5")).is_empty());
        assert!(model_rows(&[], None).is_empty(), "and no model reported is not a match either");
        // Nothing is current when nothing is known — a row that claimed to be in use
        // before the first init would be the plate lying in its quietest form.
        let rows = model_rows(&offered(), None);
        assert!(rows.iter().all(|r| !r.current));
    }

    /// 🚨 CONTRACT: **the plate never asserts a model that has not been confirmed.**
    /// `set_model`'s ack carries no body, so between the click and the repeat `system/init`
    /// the console knows what it asked for and not what it got. The confirmed name stays on
    /// the plate; the destination is carried beside it, marked.
    #[test]
    fn the_plate_keeps_the_confirmed_model_while_a_switch_is_in_flight() {
        let facts = started("claude-opus-5[1m]");
        let switching =
            strip_content(None, live(0, 0), &facts, Some("abc"), None).switching_to(Some("Sonnet"));
        let ModelSlot::Named(label) = &switching.model else { panic!("{:?}", switching.model) };
        assert_eq!(
            label.name, "claude-opus-5",
            "the plate still says what the session last reported, not what was asked for"
        );
        assert_eq!(switching.pending_model.as_deref(), Some("Sonnet"));

        // …and once the repeat init lands, the marker has nothing left to say.
        let settled = strip_content(None, live(0, 0), &started("claude-sonnet-5"), Some("abc"), None);
        assert_eq!(settled.pending_model, None);
        let ModelSlot::Named(label) = &settled.model else { panic!("{:?}", settled.model) };
        assert_eq!(label.name, "claude-sonnet-5");
    }

    /// 📌 CONTRACT: the confirmation test is **"has the reported model moved"**, not "does
    /// it equal what we asked for". `set_model` takes an alias and the session answers with
    /// a resolved id, and that resolution table is the CLI's — predicting it here would
    /// leave the marker stuck for every alias this build has not met.
    #[test]
    fn a_switch_lands_when_the_reported_model_moves_at_all() {
        assert!(!model_change_landed(Some("claude-opus-5[1m]"), Some("claude-opus-5[1m]")));
        assert!(model_change_landed(Some("claude-opus-5[1m]"), Some("claude-sonnet-5")));
        // A model this build has never heard of still settles the plate.
        assert!(model_change_landed(Some("claude-opus-5[1m]"), Some("some-model-from-2027")));
        // And the cold-start case: nothing reported, then something.
        assert!(model_change_landed(None, Some("claude-sonnet-5")));
    }

    /// 🚨 CONTRACT: **whenever the mode is not `default`, the band carries a marker — and
    /// when it is, it carries none.** The hazard `dontAsk` creates is not the moment of
    /// choosing, it is the hours afterwards during which the console still looks like the
    /// approval authority; so the warning is the standing state of the band rather than a
    /// dialog somebody clicked through once.
    #[test]
    fn a_non_default_permission_mode_is_marked_for_as_long_as_it_lasts() {
        let mode_of = |mode: &str| {
            let mut facts = started("claude-opus-5");
            facts.permission_mode = Some(mode.to_string());
            strip_content(None, live(0, 0), &facts, Some("abc"), None).mode
        };

        let ordinary = mode_of("default");
        assert_eq!(ordinary.mode.as_deref(), Some("default"));
        assert!(ordinary.marker.is_none(), "the console being the authority is not news");

        let edits = mode_of("acceptEdits");
        let marker = edits.marker.expect("acceptEdits is not the default and must say so");
        assert_eq!(marker.severity, ModeSeverity::Note);

        let silent = mode_of("dontAsk");
        let marker = silent.marker.expect("the mode that disarms the console must say so");
        assert_eq!(marker.severity, ModeSeverity::Alert, "this one is not a footnote");
        assert!(
            marker.text.contains("refused"),
            "the marker has to say what actually happens: {}",
            marker.text
        );
        // ⚠️ A mode that arrived from outside the picker — a session spawned with
        // `--permission-mode plan` — is still marked. The shortlist governs what can be
        // chosen, never what can be shown.
        let unmeasured = mode_of("plan");
        assert!(unmeasured.marker.is_some(), "an unrecognised mode is precisely the unclear case");

        // Before the first init there is no mode and nothing to mark.
        let cold = strip_content(None, LiveCounts::default(), &SessionFacts::default(), None, None);
        assert_eq!(cold.mode, ModeSlot::default());
    }

    /// 🚨 CONTRACT: **exactly three modes are offered, each labelled by what happens.**
    /// `bypassPermissions` is refused by a session the console did not launch with
    /// `--dangerously-skip-permissions`, so the row would be a dead button; `plan` and
    /// `auto` were never measured against the console's handler, and the control that
    /// governs authority is the wrong place to guess.
    #[test]
    fn the_mode_picker_offers_three_modes_and_names_the_consequence_of_each() {
        let offered: Vec<&str> = MODE_ROWS.iter().map(|r| r.value).collect();
        assert_eq!(offered, vec!["default", "acceptEdits", "dontAsk"]);
        for withheld in ["bypassPermissions", "auto", "plan", "manual"] {
            assert!(!offered.contains(&withheld), "{withheld} must not be offerable");
        }
        for row in MODE_ROWS {
            assert!(
                row.consequence.len() > row.value.len(),
                "a row is labelled by what happens, not by its name: {}",
                row.value
            );
            assert!(!row.consequence.contains(row.value), "{}", row.value);
        }
        // The one that removes the console's cards says so in those words.
        let silent = MODE_ROWS.iter().find(|r| r.value == "dontAsk").expect("the row");
        assert_eq!(silent.severity, ModeSeverity::Alert);
        assert!(silent.consequence.contains("refused"));
        assert!(silent.consequence.contains("no approval cards"));
        // …and `acceptEdits` does not overclaim: §11 measured one gate reason, not all.
        let edits = MODE_ROWS.iter().find(|r| r.value == "acceptEdits").expect("the row");
        assert!(edits.consequence.contains("measured against one gate only"));
    }

    /// 📌 The picker does not imply the switch is free. Measured: the turn after a model
    /// change re-created ~49k tokens the cache would have covered. One line, not a dialog.
    #[test]
    fn the_picker_says_what_a_switch_costs() {
        assert!(MODEL_SWITCH_COST.contains("cache"));
        assert!(MODEL_SWITCH_COST.contains("49k"));
        assert!(!MODEL_SWITCH_COST.contains('?'), "a statement, not a confirmation");
    }

    /// ⚠️ CONTRACT: the band's status symbols are `◈` and `●`, and the site that draws them
    /// **must** ask for the mono face — egui's proportional font carries neither, which is
    /// what put a tofu box on screen where `● generating` belonged. This test pins the
    /// strings; the `.monospace()` in [`strip_box`] is the other half, and the comment there
    /// says so.
    #[test]
    fn the_bands_symbols_are_the_ones_the_mono_face_has_to_draw() {
        let facts = started("claude-opus-5");
        let asking = strip_content(None, live(1, 0), &facts, Some("abc"), None);
        assert!(asking.reading.text.starts_with('◈'), "{}", asking.reading.text);
        let working = strip_content(None, live(0, 2), &facts, Some("abc"), None);
        assert!(working.reading.text.starts_with('●'), "{}", working.reading.text);
        let writing = strip_content(None, live_generating(0, 0), &facts, Some("abc"), None);
        assert_eq!(writing.reading.text, "● generating");
        // Box drawing is the one class the proportional face definitely lacks, and it is
        // what the turn marker used to reach for. Nothing on the band may use it.
        for reading in [&asking.reading, &working.reading, &writing.reading] {
            assert!(
                !reading.text.chars().any(|c| ('\u{2500}'..='\u{259f}').contains(&c)),
                "no box-drawing or block element on the band: {}",
                reading.text
            );
        }
    }

    /// One frame of the strip, headless — the same shape [`composer_frame`] uses, measuring
    /// the room the band took *away from what follows it*.
    fn strip_frame(ctx: &egui::Context, content: &StripContent, pane: &mut FakePane) -> (f32, f32) {
        let mut band = 0.0;
        let mut left = 0.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                // The real arrangement: strip lowest, composer above it, scrollback the rest.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    let before = ui.available_height();
                    // No rows: the menu is only built while the popup is open, and a
                    // headless single frame never opens one.
                    let _ = strip_box(ui, content, &[]);
                    band = before - ui.available_height();
                    ui.add_space(4.0);
                    let _ = composer_box(
                        ui,
                        &mut pane.text,
                        pane.live,
                        &mut pane.want_focus,
                        &mut pane.measured,
                    );
                    left = ui.available_height();
                });
            });
        });
        (band, left)
    }

    /// 🚨 **The strip must not swallow the pane, and must stay one line.**
    ///
    /// This is the failure mode that already bit this file once: a child dropped into a
    /// bottom-up column that places itself at the top of the remaining space measured 684 pt
    /// of a 684 pt pane. So the assertion is not "it looks right", it is that the scrollback
    /// still gets the remainder — with the busiest band that can occur, including a log line
    /// far too long for the width, which must truncate rather than wrap into a second row.
    #[test]
    fn the_strip_is_one_band_and_leaves_the_scrollback_the_rest() {
        let ctx = egui::Context::default();
        let mut pane = FakePane::new("x");
        pane.want_focus = false;

        let mut facts = started("claude-opus-5[1m]");
        facts.cost_usd = Some(1.2345);
        facts.last_turn_duration_ms = Some(7_389);
        // The busiest band there is now includes the permission marker *and* an
        // unconfirmed model change — the two things this tier added to a row that was
        // already full, and either of which wrapping would make the strip two lines.
        facts.permission_mode = Some("dontAsk".into());
        // …and the ring, which is the one thing on the band that is not text and so the
        // one thing that could be taller than the row the band reserves for it.
        facts.context_window = Some(1_000_000);
        facts.last_prompt_tokens = Some(910_000);
        let busy = strip_content(
            None,
            LiveCounts {
                pending_approvals: 2,
                running_tools: 0,
                remembered: 9,
                has_session: true,
                generating: true,
            },
            &facts,
            Some("11111111-2222-3333-4444-555555555555"),
            Some(&"a diagnostic line off the child that is far too long for the band ".repeat(8)),
        )
        .switching_to(Some("Default (recommended)"));
        let empty = strip_content(None, LiveCounts::default(), &SessionFacts::default(), None, None);

        let mut busy_band = 0.0;
        let mut left = 0.0;
        for _ in 0..3 {
            let (band, remaining) = strip_frame(&ctx, &busy, &mut pane);
            busy_band = band;
            left = remaining;
        }
        let (cold_band, _) = strip_frame(&ctx, &empty, &mut pane);

        assert!(busy_band > 0.0, "the strip must occupy real space: {busy_band}");
        assert!(
            busy_band < 44.0,
            "one band, not two — an overlong log line must truncate, not wrap: {busy_band}"
        );
        assert!(
            (busy_band - cold_band).abs() < 0.5,
            "and the band is the same height with everything in it as with nothing: \
             {cold_band} vs {busy_band}"
        );
        assert!(
            left > 400.0,
            "the scrollback must keep the remainder of the 700 pt pane, not a sliver: {left}"
        );
    }

    /// The band follows the text rather than a guess: the same three-row floor, one extra
    /// row at a time, must move the height exactly once per row until the ceiling.
    #[test]
    fn the_band_tracks_the_text_row_by_row() {
        let ctx = egui::Context::default();
        let mut pane = FakePane::new("x");
        pane.want_focus = false;
        let mut last = 0.0;
        for rows in 4..=9 {
            pane.text = vec!["x"; rows].join("\n");
            let mut height = 0.0;
            for _ in 0..3 {
                height = composer_frame(&ctx, &mut pane, Vec::new()).1;
            }
            assert!(height > last, "row {rows} did not grow the box: {last} -> {height}");
            last = height;
        }
    }
}
