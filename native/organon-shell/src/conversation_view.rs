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
    RunOutcome, ToolCard, ToolState, Transcript,
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
    /// `/panel` — put a control panel in the flow, here.
    Panel,
}

/// Recognise a local command, or `None` for an ordinary message.
///
/// Exact-match only: a message that merely *starts* with a slash is a message (a human
/// asking about `/panel` must reach the agent), and swallowing it would be a silent send
/// failure — the worst kind, because the composer clears either way.
pub fn local_command(line: &str) -> Option<LocalCommand> {
    match line.trim() {
        "/panel" => Some(LocalCommand::Panel),
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
}

impl PanelState {
    /// Where the knobs start. The terminal host's panel is the reference look, so a slider
    /// it also has starts where that one does and the two panels are the same instrument;
    /// anything else starts mid-range.
    fn for_spec(spec: &PanelSpec) -> Self {
        PanelState { sliders: spec.sliders.iter().map(|l| initial_value(l)).collect() }
    }

    fn sync(&mut self, spec: &PanelSpec) {
        if self.sliders.len() != spec.sliders.len() {
            *self = PanelState::for_spec(spec);
        }
    }
}

fn initial_value(label: &str) -> f32 {
    DEFAULT_SLIDERS
        .iter()
        .find(|(l, _)| *l == label)
        .map(|(_, v)| *v)
        .unwrap_or(0.5)
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
}

impl ConversationPane {
    /// Start a conversation in `cwd`. A spawn failure is **kept, not returned** — the tab
    /// opens and says what went wrong, which is the only way a user finds out.
    ///
    /// `buttons` are the labels an inline panel offers. A constructor argument rather than a
    /// settable field because an empty list is a panel with no buttons, which looks like a
    /// panel that is broken rather than like a caller that forgot.
    pub fn new(cwd: Option<&str>, buttons: Vec<String>) -> Self {
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
    pub fn summon_panel(&mut self) {
        let spec = PanelSpec {
            sliders: DEFAULT_SLIDERS.iter().map(|(l, _)| (*l).to_string()).collect(),
            buttons: self.buttons.clone(),
        };
        self.transcript.insert_artifact(ArtifactBlock {
            title: "◈ organon · console".to_string(),
            content: ArtifactContent::Panel(spec),
        });
        // Appended content pulls the view down, exactly as an appended element off the
        // stream does — the panel is at the bottom and being able to see it is the point.
        self.pinned = true;
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
                LocalCommand::Panel => self.summon_panel(),
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

/// Draw the pane: scrollback, then composer. Returns the artifact buttons pressed this
/// frame, for the caller to act on — see [`ArtifactAction`].
///
/// Bottom-up, because the composer's height is known and the scrollback's is whatever is
/// left — the layout every chat client resolves in that order.
pub fn draw(ui: &mut egui::Ui, pane: &mut ConversationPane) -> Vec<ArtifactAction> {
    let mut actions = Vec::new();
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        composer(ui, pane);
        ui.add_space(4.0);
        status_line(ui, pane);
        ui.separator();
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            actions = scrollback(ui, pane);
        });
    });
    actions
}

fn scrollback(ui: &mut egui::Ui, pane: &mut ConversationPane) -> Vec<ArtifactAction> {
    let mut actions = Vec::new();
    // Destructured so the transcript can be read while the widget state is written: they
    // are disjoint fields, and keeping them disjoint is the whole point of the side map.
    let ConversationPane { transcript, artifacts, pinned, .. } = pane;
    let out = egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(*pinned)
        .show(ui, |ui| {
            ui.add_space(6.0);
            if transcript.is_empty() {
                ui.label(
                    RichText::new(
                        "no messages yet — type below and press Enter, or `/panel` for a \
                         control panel",
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
                    Body::Artifact(artifact) => {
                        // Empty on the first frame; `artifact_element` syncs it to the
                        // description, which is where the starting values come from.
                        let state = artifacts.entry(element.id).or_default();
                        if let Some(button) = artifact_element(ui, element.id, artifact, state) {
                            actions.push(ArtifactAction { element: element.id, button });
                        }
                    }
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
    actions
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
fn artifact_element(
    ui: &mut egui::Ui,
    id: ElementId,
    artifact: &ArtifactBlock,
    state: &mut PanelState,
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
                match &artifact.content {
                    ArtifactContent::Panel(spec) => panel_body(ui, spec, state),
                }
            })
            .inner
    })
    .inner
}

fn panel_body(ui: &mut egui::Ui, spec: &PanelSpec, state: &mut PanelState) -> Option<String> {
    // The description is authoritative about *which* controls exist; the state is
    // authoritative about where they are. This is the only line where the two meet.
    state.sync(spec);
    let mut pressed = None;
    ui.horizontal_wrapped(|ui| {
        for label in &spec.buttons {
            if ui.button(RichText::new(label).monospace()).clicked() {
                pressed = Some(label.clone());
            }
        }
    });
    for (label, value) in spec.sliders.iter().zip(state.sliders.iter_mut()) {
        ui.add(egui::Slider::new(value, 0.0..=1.0).text(label.as_str()));
    }
    pressed
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
        for message in ["/panels", "/panel now", "what does /panel do?", "panel", "/", ""] {
            assert_eq!(local_command(message), None, "{message:?} belongs to the agent");
        }
    }

    /// The knobs start where the terminal host's panel starts, so the two front-ends draw
    /// one instrument rather than two that resemble each other.
    #[test]
    fn a_panels_widget_state_comes_from_its_description() {
        let spec = PanelSpec {
            sliders: vec!["bloom".into(), "unheard-of".into()],
            buttons: vec!["metal".into()],
        };
        let mut state = PanelState::default();
        state.sync(&spec);
        assert_eq!(state.sliders.len(), 2);
        assert_eq!(state.sliders[0], initial_value("bloom"), "shared with block_panel");
        assert!(DEFAULT_SLIDERS.iter().any(|(l, v)| *l == "bloom" && *v == state.sliders[0]));
        assert_eq!(state.sliders[1], 0.5, "an unknown control still gets a sane start");

        // A dragged value survives a re-sync — the description did not change, so nothing
        // may reach in and reset it. This is the "snaps back mid-drag" failure, headless.
        state.sliders[0] = 0.9;
        state.sync(&spec);
        assert_eq!(state.sliders[0], 0.9);
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
