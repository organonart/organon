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
//! `console_main.rs` does, exactly as it does for a terminal tab.
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
    approval_channel, decision_key, resolve_choice, resolve_recall, Choice, DecisionMemory,
    PendingApproval,
};
use crate::block_panel::{DEFAULT_SLIDERS, SLIDER_WIDTH};
use crate::card_density::{self, DensityMap, Row};
use crate::command::CommandSpec;
use crate::conversation::{
    AgentEvent, Answer, AnsweredBy, ApprovalBlock, ApprovalState, Arguments, ArtifactBlock,
    ArtifactContent, Body, Change, Element, ElementId, ExhibitSpec, Ignored,
    PanelSpec, ResultDetail, RunOutcome, StepState, SubagentAct, SubagentLog, SubagentProgress,
    SurfaceSpec, ToolCard, ToolState, Transcript, Verdict,
};
use crate::mcp::{ExposureAudit, McpServer, NoDispatch, ToolDispatch};
use crate::mcp_http::{mcp_config_json, ConfigFile, McpHttp};
use crate::panel_stack;
use crate::posture::Form;
use crate::registry;
use crate::status_log::{
    drop_mark, Health, Remark, StatusLog, LOG_MARK_EXCEPTION, LOG_MARK_QUIET,
};
use crate::text_diff::{self, DiffRow, LineDiff};
use crate::theme::Theme;
use crate::theme_edit::{self, EditKey, ThemeChange, ThemeEditor};
use crate::timeline::pinned_after_scroll;

/// The re-wrap measurement — what a width change costs this file, per frame.
///
/// Test-only, and a *sibling* of the tests below rather than part of them: it drives
/// [`scrollback`] over transcripts of up to ten thousand elements, which is a benchmark
/// and not a correctness check. Its findings are `doc/console_rewrap_measurement.md`; the
/// module doc says what it measures and, more importantly, what it does not.
#[cfg(test)]
mod rewrap_bench;

/// What an `Edit` card costs per frame — the measurement behind [`ConversationPane::diffs`].
///
/// The sibling of [`rewrap_bench`], and deliberately a second module rather than a section
/// of it: they answer different questions about the same walk. That one asks what a change
/// of *width* costs and takes a `Read` corpus; this one asks what a tool card's own
/// derivation costs and takes five shapes of `Edit`. Its findings are
/// `doc/console_edit_diff_cost.md` — which is a sibling of the other's document for the same
/// reason.
///
/// ⚠️ The pane builder and frame driver are **shared, not copied** — this module borrows
/// [`rewrap_bench`]'s, so a change to how a bench pane is built cannot land in one figure and
/// not the other.
#[cfg(test)]
mod edit_diff_bench;

/// The console's MCP `serverInfo.name`, and therefore the middle of every namespaced tool
/// name Claude Code spells: `mcp__organon__…`.
pub const SERVER_NAME: &str = "organon";

// The transcript's colours — `human_text`, `human_fill`, `prose`, `dim` — and the card
// standings — `running`, `asking`, `ok`, `bad` — are [`Theme`]'s. Each argument that used to
// sit beside a `const` here now sits beside the field, which is where a second palette will
// read it.

/// How much of a tool's output a card draws before it says how much it is not drawing.
const OUTPUT_LINES: usize = 10;
/// How many of a subagent's most recent steps a card draws.
///
/// The **tail**, not the head: the question a running `Task` card answers is "what is it
/// doing now", and the transcript keeps far more than this
/// ([`crate::conversation::Limits::max_subagent_steps`]). Everything not drawn is counted
/// on the line above it, so the card never quietly implies this was all of it.
const SUBAGENT_LINES: usize = 6;
// An `Edit` diff's own bounds are [`crate::text_diff`]'s — `CONTEXT`, `MAX_RUN`,
// `MAX_ROWS`, `MAX_CELLS` — and live there rather than here because they are inputs to the
// alignment, not to the drawing of it. A constant here would be a second opinion.
/// Slash commands the composer can be walked back through with the Up key.
///
/// ⚠️ **In memory, for the life of the tab.** It does not survive a restart and nothing
/// writes it to disk — which is a decision rather than an omission: the session log already
/// records every command that ran (`CommandRun`), so a durable recall surface would be a
/// *second* record of the same fact, and the two would disagree the first time one of them
/// was pruned. Reading back the session log is the honest way to make this durable.
const HISTORY_LINES: usize = 100;

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

/// What the view needs the console to draw, for one surface element, this frame.
///
/// **The seam between the two crates.** `organon-console` knows a surface has a rect, an id and
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

/// One exhibit item this frame wants a picture for.
///
/// 🚨 **The opposite shape to [`SurfaceRequest`] in exactly one way, and it is the important
/// one: this carries a path.** A surface names a *look* and the console renders the world; an
/// exhibit names a **file** and the console decodes it. That makes this the only request in
/// this crate that can cause a read of the filesystem, so where the path came from is a
/// property of the whole design rather than of the decoder: it reached here from
/// `organon_core::exhibit::Exhibit::resolve`, which is reached from a human's typed `/media`
/// line and from nothing else. See that module's "where a path may come from".
///
/// ⚠️ **Points, not pixels** — [`SurfaceRequest`]'s rule and its reason, unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct ExhibitRequest {
    pub element: ElementId,
    /// Which item of the exhibit — the index into [`crate::conversation::ExhibitSpec::items`].
    /// Part of the key because a three-item exhibit is three textures, not one.
    pub item: usize,
    /// The file to decode, exactly as the human typed it.
    pub path: std::path::PathBuf,
    /// The rect to fill, in points.
    pub size_points: (f32, f32),
}

/// What the console has to say about one exhibit item.
///
/// 🚨 **Absence is a state, and `Failed` is a different one.** *Absent* means the read has not
/// finished; `Picture`/`Document` are the answer; `Failed` is a file that will never load. A
/// blank rectangle and a failed decode must not look alike (#56 T4) — collapse `Failed` into
/// absent and a broken file reads as "still loading" forever, which is the single most common
/// way a media viewer lies to the person using it.
///
/// ⚠️ **One seam for both media kinds, not a texture seam and a text seam.** They differ in
/// what they carry and agree in everything that matters here: both are the result of touching
/// a file, both are produced off the frame thread, both are keyed by item, and both are freed
/// under one budget. Two maps would mean two eviction policies for one GPU.
#[derive(Clone, Debug, PartialEq)]
pub enum ExhibitContent {
    /// Decoded and uploaded, with the pixel size the aspect ratio comes from.
    Picture { texture: egui::TextureId, size: (u32, u32) },
    /// A Markdown document's source text, read off the frame thread. Rendered by
    /// [`markdown_body`] — this crate holds the text and never the pixels.
    ///
    /// 🚨 **`Arc<str>` rather than `String`, and it is a frame-cost decision, not a style
    /// one.** The console hands the whole [`ExhibitContents`] map to the view on **every**
    /// frame, exactly as it hands over [`SurfaceImages`] — but that map holds `TextureId`s,
    /// which are `Copy` and free to clone, while a document is its entire text. A `String`
    /// here means a moderately-sized README is deep-copied sixty times a second for as long as
    /// it is held, which is a real cost in a file whose §1.7 measurement exists because frame
    /// time is load-bearing. An `Arc` makes that clone a refcount bump.
    Document(std::sync::Arc<str>),
    /// It will not load, in a sentence that **names the file** rather than quoting a decoder's
    /// internal error — `organon_core::exhibit::ExhibitError`'s rule, applied one stage later,
    /// where the failure is about bytes instead of about a name.
    Failed(String),
}

/// What the console has ready for each exhibit item.
///
/// Keyed on `(element, item)` rather than [`ElementId`] alone — the difference between a
/// gallery and a single picture is a key, and choosing the wrong one here is what "retrofitting
/// several items onto a single-item exhibit" would have meant.
pub type ExhibitContents = HashMap<(ElementId, usize), ExhibitContent>;

/// Everything one frame of [`draw`] hands back.
///
/// Still a struct rather than a bare `Vec`, because what it carries is a *render list for
/// the next frame* and the name is what says so at the call site in `console_main.rs`.
///
/// ⚠️ **One field, and it used to be two.** The other was `actions` — buttons pressed in a
/// panel that drove the console's backdrop, which only `/panel` could produce. With that
/// command retired every panel drives a surface in this same transcript, so a press is
/// consumed by the thing it aims at and never leaves this crate.
#[derive(Clone, Debug, Default)]
pub struct ConversationOutput {
    /// The **visible** surfaces this frame, in transcript order.
    pub surfaces: Vec<SurfaceRequest>,
    /// The **visible** exhibit items this frame, in transcript order. Same visibility rule as
    /// `surfaces` and the same reason: an off-screen picture costs a texture nobody sees.
    pub exhibits: Vec<ExhibitRequest>,
    /// A palette the live editor changed this frame, and what to do with the store.
    ///
    /// 🚨 **`Some` only on the frames something actually moved.** This crate is handed
    /// `&Theme` and cannot assign it — the one owner is `console_main`'s `Console`, which is
    /// [`crate::theme`]'s "one owner, no globals" rule — so an edit has to leave the drawing
    /// code as a value. Answering `Some` unconditionally would make `console_main` re-derive
    /// and re-upload egui's whole chrome every frame, because `Visuals` is held on the context
    /// rather than read per frame.
    pub theme: Option<ThemeChange>,
    /// A panel `/organon` asked for this frame, to be pushed onto the console's
    /// [`crate::panel_stack::Stack`].
    ///
    /// 🚨 **`Some` only on the frame a line was submitted**, on [`ConversationOutput::theme`]'s
    /// rule and for its reason: the stack is `console_main`'s and this crate can only leave a
    /// value. ⚠️ The *destination* travelled the other way, into `draw` as a
    /// [`crate::panel_stack::Home`] — so the refusal for "no region holds a stack" is spoken
    /// here, in the composer, where the words still are, rather than on a stderr nobody reads.
    pub panel: Option<&'static organon_core::panels::Panel>,
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

/// 🚨 **The composer's slash commands now live in [`crate::registry`], and this is where they
/// used to live.**
///
/// What was here was `local_command`: an exact match on the single string `/surface`,
/// forwarding everything else to the agent. It was described as a temporary seam that would be
/// deleted once the agent could summon a surface itself — and the mechanism it built is
/// instead the one the console needed for a much larger reason.
///
/// A measurement is what changed it. `organon console posture desktop`, typed into a
/// conversation tab, went to the agent as a message, was understood by inference, was located
/// by a tool-search call, came back as a tool call, and raised an approval card asking the
/// human to approve his own command — about thirteen seconds and a chunk of context for a
/// command he had already decided on. That path was not a bug; it was the console's older
/// architecture, in which it composited around a harness it did not own and had no way to hear
/// a human's intent except through it. This front-end ended that assumption and nobody
/// revisited the consequence.
///
/// So this seam is no longer temporary and no longer single-purpose: [`crate::registry`] holds
/// **every** verb the console answers, `/surface` among them, and generates the typed surface
/// from the same table the MCP tools are generated from. The original plan is untouched — when
/// the agent can summon an artifact by tool call, the `view.surface` entry goes and nothing
/// that draws is disturbed.
///
/// The other command that used to be here, `/panel`, is still gone: it summoned a panel wired
/// to the *console*'s backdrop, which a conversation has no scrollback to show, so its controls
/// changed something you could not see from the tab you clicked in.
pub use crate::registry::{Candidate, Lane, Palette, Receipt, Registry, Resolved, Slot};

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

/// **What this tab's agent may ask the console to do** — the console's own verbs, as MCP
/// tools it can call from inside the process it is already living in.
///
/// 🚨 **Both halves are handed down, and neither can be built here.** The vocabulary is
/// `console_main`'s `console_specs()` (it is built from the substrate's material and rig
/// tables, which this crate cannot see and must not learn to), and the dispatch is
/// `console_main`'s too, because applying a console verb needs the `Console` that owns the
/// backdrop. What this crate does is serve them and generate their schemas from the same
/// [`CommandSpec`] the CLI is generated from — one vocabulary, many renderings, never a
/// hand-written second copy.
///
/// ⚠️ The dispatch is `Send` because it runs on [`McpHttp`]'s serve thread, never on the
/// UI thread — see [`crate::mcp`]'s module doc.
///
/// 🚨 **Two dispatches, and they are two because two different questions are being asked.**
/// [`Capabilities::dispatch`] is what the *agent* reaches, through the MCP server, past the
/// approval gate — the question there is *may this agent act on my behalf*, and the card that
/// answers it is correct. [`Capabilities::local`] is what a *human's* slash command reaches,
/// with no gate, because approving your own keystroke is not a safety property: it is the
/// thirteen-second round trip [`Resolved`] exists to delete. They are the same verbs onto the
/// same audited sidecar; only the asking differs.
///
/// ⚠️ A second handle rather than a shared one because [`ToolDispatch`] is consumed by the MCP
/// server (`McpHttp::start` takes it by value and moves it onto its serve thread). The console's
/// own dispatch is a cheap value holding published cells, so a second one costs nothing and,
/// more to the point, cannot be a second *implementation* — `console_main` builds both from one
/// type, and the CLI/MCP/slash round-trip test pins that they mean the same thing.
pub struct Capabilities {
    pub specs: Vec<CommandSpec>,
    pub dispatch: Box<dyn ToolDispatch + Send>,
    /// The same verbs, reached by the person in front of the console rather than by the agent.
    pub local: Box<dyn ToolDispatch>,
}

impl Capabilities {
    /// A pane that offers the model nothing: the permission handler alone.
    ///
    /// The safe shape, and still a real one — `doc/console_approval_protocol.md` §9 point 5
    /// records that a server whose `tools/list` returns only the handler reports
    /// `status: connected` and the model simply sees no tools from it. It is what every
    /// test in this file uses, and what a caller with no verbs to offer should pass.
    ///
    /// With no specs the registry holds only the view lane, so [`Capabilities::local`] is
    /// unreachable here for the same reason [`NoDispatch`] is unreachable behind an empty MCP
    /// table: nothing resolves to it.
    pub fn none() -> Self {
        Self {
            specs: Vec::new(),
            dispatch: Box::new(NoDispatch),
            local: Box::new(NoDispatch),
        }
    }
}

impl std::fmt::Debug for Capabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capabilities")
            .field("specs", &self.specs.iter().map(|s| &s.name).collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// What one pane keeps alive so its agent can ask it for permission — and what that agent
/// was told it may call.
///
/// The first two fields are held for the pane's lifetime and read by nobody: dropping the
/// server stops it, and dropping the config deletes the file the agent was started with.
/// That is why they are underscored — a tab closing must take its loopback port and its
/// temp file with it. The last two are read once per `system/init`, for the exposure audit.
struct ApprovalWiring {
    _server: McpHttp,
    _config: ConfigFile,
    /// Every capability tool this pane serves, namespaced as the client spells it.
    served: Vec<String>,
    /// The handler's namespaced name — the one that must **not** reach the model (§7).
    handler: String,
}

/// **A verb the console means to serve, and does not.**
///
/// Two spec names that sanitise to one MCP tool name leave the later one unserved: the
/// agent is simply never told that verb exists, and everything else works. That is a naming
/// bug in the table rather than a runtime condition, so it is said rather than returned as a
/// failure — but it has to be said somewhere a human will see it.
///
/// Pure, and separate from the saying, for the reason [`audit_line`] is: the live path is
/// dead by construction — `console_main`'s own test asserts the real table has no collisions —
/// so the sentence would otherwise be verified only by reading it. This is the safety net
/// for the *next* verb somebody adds, and a safety net nobody has pulled is worth exactly as
/// much as its test.
fn collision_note(collisions: &[String]) -> Option<String> {
    if collisions.is_empty() {
        return None;
    }
    Some(format!(
        "these verbs collide as MCP tool names and are NOT served: {}",
        collisions.join(", ")
    ))
}

/// Start the console's MCP server and write the config the agent will be spawned with.
///
/// Returns what to hold, what to pass to the agent, where the questions will arrive — and
/// **anything the pane should say out loud about the wiring it just got**. That last one is
/// a return value rather than a `eprintln!` in here because this function runs before the
/// pane exists and therefore cannot reach [`ConversationPane::note`]; the caller seeds the
/// log with it. Fails as a string rather than an error type because there is exactly one
/// caller and one thing it does with a failure: say so, and run the agent unwired.
///
/// 🚨 **This used to pass an empty spec table**, so the console answered permissions for
/// everything the agent did and exposed **zero** capability tools — and an agent that wanted
/// to open the portal had to shell out to `organon.exe console portal open`, spawning a
/// process to send a message to the process it was already inside, and raising a card that
/// asked *"may I run this shell command"* instead of one naming a capability. Everything
/// needed was already here; the two ends were never joined.
fn start_approvals(
    specs: &[CommandSpec],
    dispatch: Box<dyn ToolDispatch + Send>,
) -> Result<(ApprovalWiring, McpWiring, Receiver<PendingApproval>, Vec<String>), String> {
    let (gate, inbox) = approval_channel();
    let server = McpServer::new(specs, Box::new(gate)).with_server_name(SERVER_NAME);
    let permission_tool = server.permission_tool_flag_value();
    let served = server.namespaced_tool_names();
    let mut notes = Vec::new();
    if let Some(note) = collision_note(server.name_collisions()) {
        // Both, and for the reason the exposure audit already gives a few dozen lines down:
        // stderr is empty unless the console was started from a terminal, and the band's log
        // slot holds one truncated line that the next diagnostic replaces. The pane's own log
        // is the copy that survives — drawn at the head of the scrollback, where the human
        // looking at the console is.
        eprintln!("organon-console: {note}");
        notes.push(note);
    }
    let http = McpHttp::start(server, dispatch)
        .map_err(|e| format!("could not bind a loopback port: {e}"))?;
    let json = mcp_config_json(SERVER_NAME, &http.url());
    let config =
        ConfigFile::write(&std::env::temp_dir(), &ConfigFile::stem_for(http.port()), &json)
            .map_err(|e| format!("could not write the MCP config: {e}"))?;
    let wiring =
        McpWiring { config: config.path().to_path_buf(), permission_tool: permission_tool.clone() };
    Ok((
        ApprovalWiring { _server: http, _config: config, served, handler: permission_tool },
        wiring,
        inbox,
        notes,
    ))
}

/// **§7's security property, checked against this session's own report.**
///
/// `offered` is `system/init`'s `tools` array verbatim. Pure so the three states it has to
/// tell apart can be pinned with literal values; [`crate::mcp::ExposureAudit`] carries the
/// argument for why each is a different fact.
///
/// This is deliberately the *whole* of the check the console can make by itself: it reads
/// what the CLI reports, and a CLI that reported the list wrongly would fool it. That is
/// still strictly more than a measurement nobody re-runs.
///
/// ✏️ **It returns a [`Remark`] rather than a string, because the expected world is not
/// news.** James, on the live build, striking out `approvals: handler withheld from the
/// model as measured, 14 of 14 console tools visible (47 offered)` where it stood at the head
/// of the scrollback *and* on the status band: *"Remove all this too."* A guarantee that is
/// holding, restated on every launch and after every deferred re-`init`, is the console
/// explaining its own machinery to somebody who did not build it.
///
/// 🚨 **The rule is "loud when anomalous", never "quiet".** [`ExposureAudit::confirms_withholding`]
/// is what decides, so the two anomalies — a handler the model can call, and an audit that
/// proved nothing — keep their unconditional line, and so does an unwired pane. Silence here
/// means exactly one thing: the property was checked and it held. `/trace on` shows it either
/// way, and stderr carries it unconditionally at the call site.
fn audit_line(wiring: Option<&ApprovalWiring>, offered: &[String]) -> Remark {
    let Some(wiring) = wiring else {
        // Nothing was served, so nothing was checked — an absence of proof, said out loud.
        return Remark::note(
            "approvals are not wired — nothing was served and nothing could be checked",
        );
    };
    audit_remark(&wiring.handler, &wiring.served, offered)
}

/// The wired half of [`audit_line`], from three plain lists.
///
/// ⚠️ **Split out because [`ApprovalWiring`] holds a live server and a temp file**, neither of
/// which a test can conjure — and the thing worth pinning is not the plumbing but *which of
/// the three verdicts is news*.
fn audit_remark(handler: &str, served: &[String], offered: &[String]) -> Remark {
    let audit = ExposureAudit::of(handler, served, offered);
    let text = audit.summary();
    if audit.confirms_withholding() {
        Remark::machinery(text)
    } else {
        Remark::note(text)
    }
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
    /// opens a real run with (§5.9.3 rule 6), and anything on stderr — plus the console's own
    /// remarks about this session.
    ///
    /// 🚨 **This is a surface of its own now, and it records everything.** See
    /// [`crate::status_log`] for the whole argument; the short version is that a line used to
    /// face two futures — James's conversation, or nothing — and the log is the third.
    log: StatusLog,
    /// **Is the status log open?** No.
    ///
    /// 🚨 **What "off" means is the whole of Tier 1.** James, 2026-08-20: *"I don't want to see
    /// any of the things presently visible in the status panel. … My working model here is
    /// Claude Desktop. That's the level of interactivity I want to default to in terms of
    /// showing your process as the agent or harness. **Consider you are building this for me,
    /// not for some unknown user.**"* Almost everything this pane used to print above the first
    /// message was explaining the console to a stranger — which directory a tab started in, that
    /// a command was accepted, that an empty transcript is empty. He built it. So the rule is:
    ///
    /// **A refusal is always seen; an acceptance is seen only here.** [`Remark::always`] carries
    /// it per line, and the one thing that must never be gated is the one nobody would think to
    /// check — silence on failure is the defect this tree keeps finding.
    ///
    /// 🚨 **What this flag opens has CHANGED, and the change is the point of
    /// [`crate::status_log`].** It used to widen the *conversation* — every quiet remark
    /// interleaved into the scrollback above the first message — which made `/trace on` a way of
    /// making the flow **noisier** rather than of opening a different window. It now opens the
    /// **status log**: a bounded drop-down out of the pane's permanent status line, holding every
    /// line the console has written about this session. ✏️ #127 hung it above the *band*, which
    /// pushed the composer up the screen; #129 moved it to a layer at the top, because *"the
    /// entry box should never move"*. The conversation is untouched by it in either state, which is
    /// James's governing sentence — *"it should not feel like part of the conversational flow"* —
    /// made a property of where a line lives rather than of a mode.
    ///
    /// ⚠️ **It still selects the band's own quiet half** ([`StatusReading::narration`],
    /// [`Chip::narration`]), and that is one flag doing one thing rather than two: "show me the
    /// machinery" is a single request, and the band is chrome. What it must never do again is
    /// reach into the transcript — [`element_seen`] no longer takes it.
    ///
    /// ⚠️ **`/trace on` is a *view-lane* verb** ([`registry::VERB_TRACE`]) and therefore per
    /// **pane**, not per console. The log it opens is this conversation's own, and a console-lane
    /// verb would mean a sidecar spelling and an MCP tool for a preference no other process has
    /// any use for. `ORGANON_TRACE=1` opens every tab's log, which is the same escape hatch
    /// [`Self::verbose`] has.
    tracing: bool,
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
    /// What [`edit_diff`] returned for each tool card, so the parse and the alignment
    /// happen **once per card** instead of once per card per frame.
    ///
    /// 🚨 **This is not an optimisation of a hot loop; it is the removal of a cost that was
    /// linear in the whole scrollback.** `tool_card` used to call [`edit_diff`] from its
    /// body, which re-ran `serde_json::from_str` over the entire arguments blob and
    /// [`text_diff::line_diff`] over the result on **every frame, for every `Edit` card in
    /// the transcript** — and the transcript is not virtualised, so a card thousands of
    /// lines off screen paid in full. Measured at
    /// **1.4 µs for an ordinary one-line edit and 65 µs for a large one**; at a stated one
    /// large edit in ten it was **+4.4 ms per frame** on a 400-card session, a quarter of a
    /// 60 Hz budget spent recomputing a bit-identical answer.
    /// `doc/console_edit_diff_cost.md` has the tables and
    /// `conversation_view/edit_diff_bench.rs` is the instrument.
    ///
    /// ⚠️ **Every tool card gets an entry, not only the `Edit`s.** A `None` is the answer
    /// "this card has no diff", and caching it is what stops a streaming `Edit` — whose
    /// arguments are half a JSON document and which [`edit_diff`] declines — from being
    /// re-asked every frame while it arrives.
    ///
    /// ⚠️ **Invalidation is by eviction on [`Change::Updated`], never by comparing the
    /// arguments**, and that is the only correct choice available: complete arguments are
    /// **not** immutable, since a second `ToolCall` on an unresolved card replaces the text
    /// wholesale (`conversation.rs`'s `ToolCall` arm). A fingerprint cheap enough to take
    /// every frame would have to be shorter than the text, and any such thing can collide;
    /// hashing the whole blob costs a large fraction of what it saves. The fold already
    /// names the element it changed, so the exact answer is also the cheap one. Bounded
    /// like the other side maps by the `retain` at the end of [`scrollback`].
    diffs: HashMap<ElementId, Option<EditDiff>>,
    /// How much room each settled tool card takes, and which groups a hand has opened
    /// ([`crate::card_density`]).
    ///
    /// 🚨 **A side map for the same reason [`PanelState`] is one**, and with one extra
    /// consequence: the automatic half of it is applied only while [`Self::pinned`] is true,
    /// which is what stops a card completing far above a reader's viewport from changing the
    /// height of what they are looking at. That argument is the module's, and the field is
    /// where the `pinned` bit reaches it. Pruned by the same `retain` the other two get.
    density: DensityMap,
    /// The button labels a summoned panel offers, **handed down** by whoever opened the
    /// tab. This crate cannot see the console's material table and must not learn to; it
    /// draws these and reports which was pressed ([`ArtifactAction`]).
    buttons: Vec<String>,
    /// The slider labels and their starting values, handed down for the same reason the
    /// buttons are: a label like `exposure` means something to the console's `Shared`
    /// snapshot and nothing at all here, and a label this crate invented would be a knob
    /// that moves and changes no pixel — a worse instrument than no knob.
    sliders: Vec<(String, f32)>,
    /// The MCP server this pane's agent asks for permission and calls the console's verbs
    /// on, plus the config it was started with. See [`ApprovalWiring`]. `None` means the
    /// server could not start; the agent then runs unwired and the log says so.
    approvals: Option<ApprovalWiring>,
    /// Where permission requests arrive from the serve thread.
    inbox: Receiver<PendingApproval>,
    /// The last exposure-audit sentence reported, so a repeat `system/init` that changes
    /// nothing says nothing. See where it is set — an init recurs by design.
    last_audit: Option<String>,
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
    /// Every verb this pane answers, console and view alike — see [`crate::registry`]. Built
    /// once from the handed-down catalog, because it is the same table for the tab's whole
    /// life and re-deriving it per keystroke would be work for no fact.
    registry: Registry,
    /// Where a **human's** console command goes: straight onto the console's audited sidecar,
    /// with no agent, no inference and no approval card in the way. See [`Capabilities`] for
    /// why this is not the same handle the MCP server holds.
    local: Box<dyn ToolDispatch>,
    /// Which row of the command panel is highlighted. Clamped against the live list every
    /// frame rather than kept in step with it — the list is regenerated from the line on
    /// every keystroke, so an index is the only thing that could go stale.
    palette_selected: usize,
    /// Whether Escape has shut the panel for the line as it stands.
    ///
    /// 🚨 **A fact about an EDIT, not about a string, and the difference is a shipped bug.**
    /// This was `Option<String>` — the composer's text at the moment Escape was pressed,
    /// compared for equality on every frame — and content equality cannot express "has
    /// changed since": a line becomes equal to a dismissed string again by ordinary
    /// retyping. Press Escape once at `/p` and *every* future `/p` was silently refused a
    /// panel, with nothing on screen to explain it. James hit exactly that on 2026-08-14
    /// (*"Now my tab completion broke. When I type slash p, nothing comes up"*). The
    /// dismissal is now let go of by [`ConversationPane::notice_edit`], which watches the
    /// composer change rather than asking whether it happens to match.
    palette_dismissed: bool,
    /// The composer as it stood at the end of the previous frame, and the only reason
    /// [`ConversationPane::notice_edit`] can tell an edit from a still line. The
    /// [`egui::TextEdit`] writes [`ConversationPane::composer`] in place, so there is no
    /// edit *event* to observe — a shadow copy is what makes the change observable at all.
    composer_seen: String,
    /// Whether self-completion is **held off** because the last thing the hand did was delete.
    ///
    /// 🚨 **A latch, not a per-frame test, and the difference is the whole fix.** See
    /// [`completion_held`] for the rule and for why "the line did not shrink this frame" is not
    /// good enough: the frame after a backspace is a frame in which nothing changed at all, and
    /// a rule that only refused *shrinking* frames would re-complete on that one — a flicker,
    /// which is this same defect at a different frequency.
    completion_held: bool,
    /// Slash commands this pane has sent, most recent first, walked by the arrow keys. See
    /// [`ConversationPane::remember_command`] for what earns a place and what does not.
    history: VecDeque<String>,
    /// Where a history walk currently stands, or `None` when no walk is in progress. Never
    /// trusted on its own — [`ConversationPane::walking`] cross-checks it against the
    /// composer, so an edit ends the walk without anything having to notice the edit.
    history_at: Option<usize>,
    /// Something replaced the composer's text wholesale, so the caret has to be put back at
    /// its end. Set by every site that rewrites the line — a Tab, a completion, a recall —
    /// and drained by [`composer`] at the **end of the frame that set it**, which is why the
    /// two sites before the box and the two after it can share one flag. ⚠️ It never survives
    /// a frame: a request that outlived its frame is exactly the `/hxelp` defect.
    want_caret: bool,
    /// What the last command said back, held for the band above the composer. See
    /// [`PanelReceipt`] — this is the console's answer to a receipt that scrolls off the
    /// top of the transcript before anyone can read it.
    receipt: Option<PanelReceipt>,
    /// 🚨 **Whether this composer may read the frame's Tab, Escape and arrows at all.**
    ///
    /// `composer_keys` consumes those out of the **raw event list**, not out of a focused
    /// widget, and two of them unconditionally: `arrow_owner` hands Up to the history whenever
    /// the box is empty. That was safe while the console had exactly one command input. #98
    /// Tier C gives every non-`agent` region a command line of its own, and a second input
    /// would have found its Up already taken before it ran.
    ///
    /// ⚠️ **Set from a measurement, not from a policy.** `console_main` writes this each frame
    /// from `region_line::Lines::composer_owns_keys`, which is `true` unless some region's line
    /// had egui focus on the **previous** frame — read off that widget's own
    /// [`egui::Response::has_focus`], the same fact `composer_box` already uses to decide
    /// whether Enter sends. Nothing here invents a focus state.
    ///
    /// ⚠️ **`true` is the default and that is invariant #4.** A console with no divided pane
    /// draws no region line, so nothing ever records focus, and this composer keeps every key
    /// it had before that module existed. It is deliberately **not** an `Option`: "nobody has
    /// told me" and "the composer owns them" are the same state here.
    keys: bool,
    /// Whether a command may run the instant the panel knows what it is, with no Enter.
    ///
    /// **On by default**, since the recoverability term joined [`Palette::autorun`]'s rule:
    /// what fires unasked is now only what a second command can take back.
    /// `ORGANON_PALETTE_AUTORUN=0` is the escape hatch and puts the Enter-for-everything
    /// console back for a session. The rule it obeys lives in [`Palette::autorun`], not here.
    autorun: bool,
    /// Whether the panel draws the **verbose** list — a headed row per candidate with its
    /// doc — instead of the one-row word list that is now the primary mode.
    ///
    /// Off unless `ORGANON_PALETTE_VERBOSE=1`. ⚠️ **An env var rather than a key**, and
    /// deliberately so: James asked for the list to *"be available as a verbose mode"* and
    /// said nothing about how to reach it, and a keybinding invented on his behalf is a
    /// standing claim on a key in a box that is also where he talks to an agent. The switch
    /// is here so the mode is reachable today; which key it eventually gets is his.
    verbose: bool,
    /// The live palette editor, when `/theme edit` has opened one. See [`crate::theme_edit`].
    ///
    /// ⚠️ **Per tab, and the palette it edits is not.** A `Theme` is console-wide, so two tabs
    /// with editors open are two views of one palette — which is fine and is what a hand would
    /// expect, because each tab's editor reads the live palette every frame and only *holds*
    /// the in-flight HSV of fields being dragged in that tab. What it must not do is outlive
    /// the palette it was opened against: [`ConversationPane::theme_editor_ui`] closes it if
    /// the palette changes underneath it, which is what `/theme chocolate` typed while an
    /// editor is open does.
    theme_edit: Option<ThemeEditor>,
    /// Where a panel summoned here would go, as this frame's [`draw`] was told.
    ///
    /// 🚨 **Held rather than passed down through `submit`, because it is a fact about the
    /// CONSOLE and this crate cannot see one.** The layout lives on `console_main`'s
    /// `Console`; `draw` is handed the answer at the top of every frame and
    /// [`ConversationPane::summon_organon`] reads it. Threading it through `scrollback` →
    /// `composer` → `submit` → `run_command` would be five signatures carrying one value that
    /// is constant for the frame.
    panel_home: panel_stack::Home,
    /// A panel `/organon` asked for this frame, for the console to push onto the stack.
    ///
    /// ⚠️ **An output, not an action**, exactly as [`ConversationOutput::theme`] is: this crate
    /// is handed the *destination* and cannot reach the stack itself — the stack holds
    /// `&'static Panel`s that only `console_main` can put on screen. Drained into the
    /// [`ConversationOutput`] at the end of `draw`, after the column, for the reason that
    /// function's own comment gives about `out` being replaced wholesale.
    panel_wanted: Option<&'static organon_core::panels::Panel>,
}

/// A command's answer, shown where the command was typed.
///
/// 🚨 **The defect this closes.** A slash command's receipt goes to the pane's log, and the
/// log is drawn at the **head** of the scrollback — so in any conversation longer than a
/// screen the confirmation lands far above the live edge and is, in practice, invisible.
/// James typed `/posture desktop` on 2026-08-14, the console obeyed, and nothing he could see
/// said so. The registry tier logged that as a known limitation on the grounds that the
/// transcript has no "the console said this" element and inventing one is a change to the
/// conversation model. The panel above the composer needs no such element: it is already
/// full-width, already appears and disappears with the command line, and is already where the
/// eye is.
///
/// ⚠️ **A receipt and a candidate share one region and mean opposite things** — "here is what
/// happened" against "here is what you may do — so they are distinguished structurally
/// rather than by wording: a receipt is a single marked band, a candidate list is a headed
/// list, and only one of the two is ever drawn.
struct PanelReceipt {
    /// The structured answer. `ok` is what makes a refusal outlive a success.
    receipt: Receipt,
    /// The composer's contents at the moment this was made — **read after the command lane
    /// had its way with them**, so a success answers an emptied box and a refusal answers the
    /// words it refused, which are still there to be fixed.
    answered: String,
    /// When it was first drawn, in egui's own clock. `None` until it has been on screen for a
    /// frame, which is what stops a receipt made between frames from ageing unseen.
    since: Option<f64>,
}

/// How long a **successful** receipt holds the region. A refusal never expires — see
/// [`receipt_holds`].
const RECEIPT_SECONDS: f64 = 8.0;

/// Does the receipt still own the band?
///
/// 🚨 **The asymmetry is the whole rule, and it is the one `card_density` already landed:
/// success is quiet, failure keeps its weight.** A confirmation nobody reads costs nothing —
/// the command ran. A refusal that vanishes before it is read costs the command *and* the
/// knowledge that it did not happen, which is strictly worse than the invisible receipt this
/// band replaces. So a success ages out and a refusal does not.
///
/// Both go the moment the line changes, which is the honest signal that the human has moved
/// on — and it is what hands the region back to the candidate list.
pub fn receipt_holds(ok: bool, answered: &str, composer: &str, shown_for: f64) -> bool {
    if answered != composer {
        return false;
    }
    !ok || shown_for < RECEIPT_SECONDS
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
    ///
    /// `capabilities` is what this tab's agent may ask the console to do, handed down for
    /// the same reason `buttons` and `sliders` are — see [`Capabilities`].
    /// [`Capabilities::none`] is the caller with nothing to offer.
    pub fn new(
        cwd: Option<&str>,
        buttons: Vec<String>,
        sliders: Vec<(String, f32)>,
        capabilities: Capabilities,
    ) -> Self {
        let Capabilities { specs, dispatch, local } = capabilities;
        // Built before the server is handed the specs, from the same list, so the verbs a
        // human can type and the verbs an agent is offered are one table by construction
        // rather than by two calls that happen to agree.
        let registry = Registry::new(&specs);
        // ⚠️ **Everything written here is `always`.** Each of these three lines says something
        // has gone wrong in a way nothing else on screen reports — a verb that cannot be typed,
        // a wiring diagnostic, an approval channel that never came up — which is the half of the
        // trace rule that is never gated. See [`ConversationPane::trace`].
        let mut log = StatusLog::default();
        for name in registry.collisions() {
            log.push(Remark::note(format!(
                "`{name}` cannot be typed as a slash command — another verb already holds that \
                 word"
            )));
        }
        let (approvals, wiring, inbox) = match start_approvals(&specs, dispatch) {
            Ok((held, wiring, inbox, notes)) => {
                // Whatever the wiring had to say about itself, in the pane rather than only
                // on a stderr nobody is reading. `push` rather than `note` because the
                // pane does not exist yet; the log is capped far above the handful of lines
                // this can produce.
                for text in notes {
                    log.push(Remark::note(text));
                }
                (Some(held), Some(wiring), inbox)
            }
            Err(error) => {
                log.push(Remark::note(format!(
                    "approvals are not wired ({error}) — a tool that needs permission will fail \
                     instead of asking"
                )));
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
            diffs: HashMap::new(),
            density: DensityMap::default(),
            buttons,
            sliders,
            approvals,
            inbox,
            last_audit: None,
            waiting: HashMap::new(),
            memory: DecisionMemory::new(),
            models: Vec::new(),
            pending_model: None,
            registry,
            local,
            palette_selected: 0,
            palette_dismissed: false,
            composer_seen: String::new(),
            completion_held: false,
            history: VecDeque::new(),
            history_at: None,
            want_caret: false,
            receipt: None,
            theme_edit: None,
            // The honest opening value: until `draw` is told otherwise, nothing holds a stack.
            // A pane that has never been drawn refusing `/organon` is correct — there is no
            // console around it yet.
            panel_home: panel_stack::Home::Nowhere,
            panel_wanted: None,
            // The composer owns the keyboard until a region line takes it — see the field.
            keys: true,
            // Read once, here, rather than per frame: they are switches for a session, and an
            // env lookup inside the draw path would be a syscall per keystroke.
            autorun: autorun_enabled(
                std::env::var("ORGANON_PALETTE_AUTORUN").ok().as_deref(),
            ),
            verbose: std::env::var("ORGANON_PALETTE_VERBOSE").is_ok_and(|v| v == "1"),
            // ⚠️ **Off unless asked for, which is the point** — see the field. A tab opens quiet
            // and `/trace on` is one line away.
            tracing: std::env::var("ORGANON_TRACE").is_ok_and(|v| v == "1"),
        }
    }

    /// Every verb this pane answers, as a hierarchy — the seam a pointer surface (a context
    /// menu, the pie menu) reads instead of building a table of its own.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Tell this composer whether it may read the frame's Tab, Escape and arrows.
    ///
    /// 🚨 **The console's one arbitration point between several command inputs**, and it is a
    /// *setter* rather than a parameter on [`draw`] on purpose: the answer is `true` for every
    /// caller that does not know about regions, so a console that never divides its pane —
    /// and every test in this file — is untouched. See [`ConversationPane::keys`].
    pub fn set_keys(&mut self, owned: bool) {
        self.keys = owned;
    }

    /// Fold one mapped event into the transcript and keep the pane's derived state in step
    /// with it. Returns whether anything changed, i.e. whether a repaint is owed.
    ///
    /// 🚨 **The one place a cached diff is invalidated**, and the reason it can be exact
    /// rather than approximate: an update is the only way a card's arguments can move, and
    /// the fold *says which element it moved*. Dropping the entry re-derives it on the next
    /// frame. See [`ConversationPane::diffs`] for why a fingerprint would have been both
    /// weaker and more expensive than asking the fold.
    ///
    /// ⚠️ **Every update evicts, not only an argument one** — a `ToolResult` lands here too
    /// and drops a diff that was still good. That is one recomputation per card per result,
    /// deliberately: narrowing it would mean this method reasoning about *which* field the
    /// fold touched, which is knowledge that belongs to the fold and would rot silently the
    /// day a new event arm is added.
    ///
    /// A method rather than the four lines it replaces inside the drain loop, because the
    /// eviction rule above is load-bearing and the drain loop needs a live agent process to
    /// reach. This is what the cache's tests drive.
    fn absorb(&mut self, mapped: AgentEvent) -> bool {
        match self.transcript.apply(mapped) {
            Change::Appended(_) => {
                self.pinned = true;
                true
            }
            Change::Updated(id) => {
                self.diffs.remove(&id);
                true
            }
            Change::Meta => true,
            Change::Ignored(_) => false,
        }
    }

    /// What the console has been asked to remember, for whoever wants to show or audit it.
    pub fn memory(&self) -> &DecisionMemory {
        &self.memory
    }

    /// Start asking again. The band's marker goes with it, on the next frame, because the
    /// marker is derived from this flag rather than stored beside it.
    ///
    /// Says so in the log: revoking is the one gesture here whose *effect* is that nothing
    /// visible happens until the next tool call, and a click with no acknowledgement reads
    /// as a click that missed.
    fn revoke_session_allow(&mut self) {
        if self.memory.revoke_session_allow() {
            // ✏️ **The log, not the conversation.** It is a confirmation of something the reader
            // just did, and the band already states the result: the standing-allow marker is
            // present exactly while the allow is, so it vanishes on the same frame. A sentence
            // in the flow saying what the chrome beside it already shows is the definition of
            // the narration this change removes.
            self.trace("the console is asking again — everything-for-this-session revoked".into());
        }
    }

    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    /// The console's own remarks about this session, **all of them** — including the ones no
    /// surface but the status log is drawing. A reader that wants what the *conversation* shows
    /// asks [`StatusLog::exceptions`], which is what [`scrollback`] does.
    pub fn log(&self) -> impl Iterator<Item = &Remark> {
        self.log.iter()
    }

    /// The status log itself — read by the pane's status line and by the drop-down it opens.
    pub fn status_log(&self) -> &StatusLog {
        &self.log
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
        // The same arrangement, and the same reason: the audit's line goes through `note`.
        let mut audits: Vec<Vec<String>> = Vec::new();
        for item in items {
            match item {
                StreamItem::Event(event) => {
                    if let EventKind::ControlResponse(response) = &event.kind {
                        acks.push(response.clone());
                    }
                    // 🚨 **§7's security property, re-measured against this server, every
                    // init.** The doc says the guarantee is tied to the flag and must be
                    // checked per server; serving real capability tools is exactly the
                    // change that could disturb it. The init event already carries the
                    // model's whole tool list, so the console can answer the question about
                    // itself instead of a person remembering to run a probe.
                    if let EventKind::SessionStarted(start) = &event.kind {
                        audits.push(start.tools.clone());
                    }
                    for mapped in self.mapper.map(&event) {
                        changed |= self.absorb(mapped);
                    }
                }
                // ✏️ **Reclassified to the log.** These are the child's own chatter — the
                // `Warning: no stdin data received…` the CLI opens every real run with, and
                // whatever else it puts on stderr — and they were `note`, so a routine startup
                // warning stood above the first message of every conversation. They are
                // machinery by definition: the console did not write them and cannot say what
                // they mean. ⚠️ **A stream that is genuinely broken still says so** through the
                // arm below, which sets `failure` as well, so nothing that matters rests on
                // these two lines being loud.
                StreamItem::Noise(line) => self.trace(format!("stdout: {line}")),
                StreamItem::Stderr(line) => self.trace(format!("stderr: {line}")),
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
        for offered in audits {
            let audit = audit_line(self.approvals.as_ref(), &offered);
            let line = audit.text.clone();
            // ⚠️ **Only when the verdict changes, and an init recurs.** `system/init` is
            // re-sent as deferred MCP tools finish loading — measured going 33 → 128 tools
            // between two inits with nothing asked to change — so an unconditional line
            // would repeat itself all session, and the one line that can say the approval
            // system has stopped meaning anything would be the one nobody reads. A change
            // *is* the news: `0 of 5 visible` becoming `5 of 5` is our tools arriving, and
            // anything becoming 🚨 is the only alarm this console has.
            if self.last_audit.as_deref() == Some(line.as_str()) {
                continue;
            }
            // On stderr as well as in the band's log, because the log slot holds one
            // truncated line and the next diagnostic replaces it.
            //
            // ⚠️ **Unconditionally on stderr, even when the pane keeps quiet about it.** The
            // screen is where a repeated confirmation costs something; a launch log is where
            // the audit trail lives, and a security property that is only recorded when it
            // fails is a record nobody can check afterwards.
            eprintln!("organon-console: {line}");
            self.last_audit = Some(line);
            // 🚨 The remark carries its own loudness — see [`audit_line`]. Not `note`, which
            // would put the expected world back on the band it was just taken off.
            self.remark(audit);
            changed = true;
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
            // ⚠️ **Kept, and deliberately silent.** The list is what the picker is built
            // from ([`model_rows`]), so it is load-bearing — but a note counting it put
            // "the session offers 5 models" on the band's log at every cold start, where
            // the one line of diagnostic width is worth more than a number nobody can act
            // on. The count is discoverable by clicking the plate, which is where a list
            // of models belongs.
            Control::Initialize => self.models = response.models(),
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

    /// One question: answered from memory if the console already has an answer for it,
    /// otherwise a card.
    ///
    /// **An answer the console gave still renders.** It would be cheaper to answer silently
    /// and say nothing, and that is exactly the failure the memory has to avoid: an
    /// authority the human granted once and can no longer see is worse than being asked
    /// every time. That holds harder for the session-wide allow than for a per-call one —
    /// it is the grant with the widest reach and the fewest clicks behind it.
    fn receive_approval(&mut self, pending: PendingApproval) {
        let tool_name = pending.request().tool_name.clone();
        let tool_use_id = pending.request().tool_use_id.clone();
        let input = pending.request().input.to_string();

        if let Some(recall) = self.memory.recall(&tool_name, &input) {
            let (decision, answer) = resolve_recall(recall, pending.request());
            self.transcript.insert_approval(ApprovalBlock {
                tool_name,
                input,
                tool_use_id,
                state: ApprovalState::Answered(answer),
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

    /// Put a file from disk in the flow, or say why not.
    ///
    /// 🚨 **The only door a path enters the console by.** Everything downstream — the request,
    /// the off-thread read, the decoder — trusts that whatever reaches it came from a human's
    /// keystrokes, and this is the function that makes that true. It is reached from the
    /// composer's view lane and from nothing else; `organon_core::exhibit`'s module doc owns
    /// the argument, and `registry::VERB_MEDIA` records why the verb is not in the MCP catalog.
    ///
    /// ⚠️ **The path is not resolved, not canonicalised and not checked for existence here.**
    /// Existence is the reader's answer to give — it is IO, and it belongs off the frame thread
    /// with every other touch of the disk. What a nonexistent file earns is
    /// `ExhibitContent::Failed` naming it, which is the same plate a corrupt file earns and the
    /// right one for both: the person typed a name, and the name is what they need back.
    fn summon_media(&mut self, args: &serde_json::Value, typed: &str) -> Receipt {
        let raw = args.get(registry::MEDIA_ARG).and_then(|v| v.as_str()).unwrap_or_default();
        // Split on whitespace, which is what makes `/media a.png b.png c.png` one exhibit of
        // three. ⚠️ It is also why a path *containing* a space cannot be typed here — a real
        // limit, stated rather than hidden, and the reason the refusal below quotes the pieces
        // it actually tried.
        let paths: Vec<std::path::PathBuf> =
            raw.split_whitespace().map(std::path::PathBuf::from).collect();
        let exhibit = match organon_core::exhibit::Exhibit::resolve(&paths) {
            Ok(exhibit) => exhibit,
            Err(why) => {
                // The refusal is the product here — it names the file and what would have
                // worked. `note` puts it in the pane's log, where a command's answer goes.
                let message = why.to_string();
                self.note(message.clone());
                return Receipt { ok: false, text: message };
            }
        };
        let count = exhibit.len();
        let title = format!("◈ organon · {}", exhibit.kind.as_word());
        let content = ExhibitSpec::place(exhibit);
        match self.transcript.insert_artifact(ArtifactBlock { title, content }) {
            Change::Appended(_) => Receipt { ok: true, text: typed.to_string() },
            // `insert_artifact` appends unconditionally, so this is a contract change in the
            // transcript rather than something a person did — said out loud for that reason.
            _ => {
                let message = format!("{count} item(s) could not be placed in the transcript");
                self.note(message.clone());
                Receipt { ok: false, text: message }
            }
        }
    }

    /// Ask for one of Organon's editor panels **in the panel stack**, or say why not.
    ///
    /// 🚨 **A panel does not land in the transcript, and there is no fallback that puts one
    /// there.** James, 2026-08-20: *"Would we ever want a panel inline? A panel should not
    /// scroll away. That doesn't make sense."* A transcript is a log and a control is not a log
    /// entry — a panel is used *while* watching what it changes, and one that scrolls off
    /// mid-drag was never usable. So with no region holding a stack this **refuses by name**
    /// and says what would have made one; it does not fall back and it does not quietly do
    /// nothing, which is `Ring::Empty`'s rule at the scale of a verb.
    ///
    /// The push itself is `console_main`'s — see [`ConversationPane::panel_wanted`].
    ///
    /// 🚨 **This is the second gate on the `(tab, panel)` pair, and the last one.** The command
    /// schema declares the panel argument as the *union* of every slug on every tab — one value
    /// list per argument is all a schema has — so `/organon motion surface` satisfies the
    /// schema with a real slug on the wrong tab. A line **typed in the composer** is now
    /// refused before it gets here, by [`crate::registry::NarrowFn`]'s hook, which is what lets
    /// that refusal name the tab's own panels while the words are still in the box. A call that
    /// did not come through the composer has had no such check, so `panels::find` still runs
    /// here: this arm is not dead, it is the door the other callers use.
    fn summon_organon(&mut self, args: &serde_json::Value, typed: &str) -> Receipt {
        let word = |key: &str| args.get(key).and_then(|v| v.as_str()).unwrap_or_default();
        let tab_word = word(registry::ORGANON_TAB_ARG);
        let slug = word(registry::ORGANON_PANEL_ARG);
        let Some(tab) = organon_core::tabs::UiTab::from_word(tab_word) else {
            // Unreachable through the composer — the tab argument's `Choice` is the tab list,
            // so `resolve` refuses an unknown word before this runs. Said rather than
            // `unwrap`ped: this is also the arm a future MCP caller would arrive through.
            let message = format!("`{tab_word}` is not one of Organon's tabs");
            self.note(message.clone());
            return Receipt { ok: false, text: message };
        };
        let Some(panel) = organon_core::panels::find(tab, slug) else {
            let known = organon_core::panels::in_tab(tab);
            let message = if known.is_empty() {
                // ⚠️ The same sentence the composer refuses with and the same one the ring
                // draws when it is empty — written once in `registry`, because a second
                // spelling of it here would drift the day a tab is joined.
                registry::unmapped_tab(tab_word)
            } else {
                format!(
                    "the {tab_word} tab has no panel called `{slug}` — it has: {}",
                    known.iter().map(|p| p.slug).collect::<Vec<_>>().join(", ")
                )
            };
            self.note(message.clone());
            return Receipt { ok: false, text: message };
        };
        // 🚨 **The second door onto a column, gated by the same function as the first.** A panel
        // this build has no controls for used to be admitted and then explain itself where its
        // controls would be; it is refused by name instead — `panel_stack::admit` carries why.
        // Asked *before* the destination, because "this panel has no controls" is true whatever
        // the layout is, and answering the layout's question first would send a person off to
        // declare a region for a panel that was never going to draw anything.
        if let Err(refusal) = panel_stack::admit(panel) {
            let message = refusal.to_string();
            self.note(message.clone());
            return Receipt { ok: false, text: message };
        }
        let panel_stack::Home::Shown(region) = self.panel_home else {
            // The refusal a person meets the first time, and the whole of how they learn a
            // region has to be declared. One sentence, written once, in `panel_stack`.
            let message = panel_stack::Refusal::NoRegion.to_string();
            self.note(message.clone());
            return Receipt { ok: false, text: message };
        };
        self.panel_wanted = Some(panel);
        // ⚠️ **The region is named, not merely implied.** There is one stack and possibly
        // several regions showing it, so "it went somewhere" would leave a person hunting the
        // window for a panel that is on screen.
        //
        // ✏️ **…and it is `trace`, like every other console-lane acceptance.** The panel is
        // *in* the region a frame later, which is the strongest possible statement of where it
        // went; every refusal above stays on `note`. Recorded either way.
        self.trace(format!("{} → the panel stack in `{}`", panel.title, region.as_word()));
        Receipt { ok: true, text: typed.to_string() }
    }

    /// The look a surface opens at: the console's first button label, since that list *is*
    /// the material table and its head is the console's own default dressing. An empty list
    /// (a caller that handed down nothing) yields an empty name, which the console reads as
    /// "no material named" — Tier 1's undressed substrate, not a failure.
    fn default_look(&self) -> String {
        self.buttons.first().cloned().unwrap_or_default()
    }

    /// Send the composer's contents and clear it. Renders nothing locally (rule 2).
    ///
    /// 🚨 **A command is resolved before the session is even looked up**, so it works in a pane
    /// whose agent has died — and, far more importantly, so that it costs no inference and
    /// raises no approval card. That is the whole of what this change buys: see
    /// [`Registry::resolve`].
    ///
    /// ⚠️ **A refusal does not clear the composer**, and every other outcome does. The words
    /// stay where a hand can fix them, which is what makes refusing an unknown slash command
    /// strictly better than the forwarding it replaced: nothing a person typed can vanish.
    fn submit(&mut self, theme: &Theme, theme_name: &str) {
        let text = self.composer.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.palette_selected = 0;
        self.palette_dismissed = false;
        self.history_at = None;
        let resolved = self.registry.resolve(&text);
        self.remember_command(&text, &resolved);
        let outgoing = match resolved {
            Resolved::Message => text.clone(),
            Resolved::Escaped(line) => line,
            Resolved::Refused(message) => {
                self.note(message.clone());
                // The composer is deliberately NOT cleared, so the refusal answers the words
                // that are still in it — which is exactly what makes it hold the band until
                // they are edited.
                self.answer(Receipt { ok: false, text: message });
                return;
            }
            Resolved::Run { lane, name, args } => {
                let receipt = self.run_command(lane, &name, &text, args, theme, theme_name);
                self.composer.clear();
                // After the clear, so `answered` is the emptied box the success is about.
                self.answer(receipt);
                return;
            }
        };
        let Some(session) = self.session.as_mut() else { return };
        match session.send_user(&outgoing) {
            Ok(()) => self.composer.clear(),
            Err(e) => {
                self.note(format!("could not send: {e}"));
                self.failure = Some(format!("the agent stopped listening: {e}"));
            }
        }
    }

    /// Run one resolved command. `typed` is what the human wrote, so the receipt echoes their
    /// own words rather than a catalog name they never saw.
    ///
    /// The two lanes really do need two arms, and the split is not cosmetic: a view verb acts
    /// on *this transcript* and returns nothing, while a console verb is handed to the same
    /// dispatch an agent's tool call reaches — which writes the console's sidecar, which the
    /// frame path drains through the real [`crate::command::CommandService`]. So a slash
    /// command still leaves a `CommandRun` record; what it skips is the agent, not the audit.
    fn run_command(
        &mut self,
        lane: Lane,
        name: &str,
        typed: &str,
        args: serde_json::Value,
        theme: &Theme,
        theme_name: &str,
    ) -> Receipt {
        match lane {
            Lane::View => match name {
                registry::VERB_SURFACE => {
                    self.summon_surface();
                    Receipt { ok: true, text: typed.to_string() }
                }
                registry::VERB_ORGANON => self.summon_organon(&args, typed),
                registry::VERB_MEDIA => self.summon_media(&args, typed),
                // ⚠️ **The word is looked up rather than trusted.** `validate_args` has already
                // checked it against `TRACE_WORDS`, so the `else` is a belt on a brace — but it
                // is the arm that would fire if a third word were added to that list and not
                // here, and answering `off` to an unknown state would be the quiet failure this
                // whole tier is about.
                registry::VERB_TRACE => {
                    match args.get(registry::TRACE_ARG).and_then(|v| v.as_str()) {
                        Some("on") => {
                            self.set_tracing(true);
                            Receipt { ok: true, text: typed.to_string() }
                        }
                        Some("off") => {
                            self.set_tracing(false);
                            Receipt { ok: true, text: typed.to_string() }
                        }
                        other => {
                            let message = format!(
                                "`{}`: `{}` is not one of {} — that is a wiring bug, not \
                                 something you typed wrongly",
                                registry::VERB_TRACE,
                                other.unwrap_or("<missing>"),
                                registry::TRACE_WORDS.join(" | "),
                            );
                            self.note(message.clone());
                            Receipt { ok: false, text: message }
                        }
                    }
                }
                registry::VERB_HELP => {
                    // Collected first: `help_lines` borrows the registry and `trace` wants the
                    // pane.
                    let lines = self.registry.help_lines();
                    // ✏️ **`trace`, not `note` — and then the log is opened, which is what makes
                    // that safe.** A verb table is the clearest possible case of a true, routine
                    // line that is not an exception, and nineteen of them landing in the middle
                    // of a conversation is the leak this change closes. But a `/help` whose
                    // output went somewhere closed would read as a verb that does nothing, so
                    // the log is opened in the same breath: the table appears, in the surface
                    // built to hold exactly this.
                    for line in lines {
                        self.trace(line);
                    }
                    self.set_tracing(true);
                    Receipt { ok: true, text: typed.to_string() }
                }
                other => {
                    let message = format!(
                        "`{other}` is in the registry's view lane and nothing here answers it \
                         — that is a wiring bug, not something you typed wrongly"
                    );
                    self.note(message.clone());
                    Receipt { ok: false, text: message }
                }
            },
            Lane::Console => {
                // 🚨 **Two of `/theme`'s argument values open a surface here instead of
                // dispatching**, and this is the one place a console-lane verb is answered
                // locally. It is not a lane violation so much as the lane's own edge: the
                // palette really is console-wide state (which is why the *edits* leave on
                // `ConversationOutput` for `console_main` to own), but the editor is a panel in
                // this transcript, drawn in this pane's band, and there is nothing on the
                // sidecar for a dispatch to reach that could draw it. Every other value of
                // every other console verb falls straight through to the dispatch below.
                if name == registry::VERB_THEME {
                    if let Some(word) = args.get(registry::THEME_ARG).and_then(|v| v.as_str()) {
                        if theme_edit::is_edit_word(word) {
                            return self.open_editor_receipt(typed, theme, theme_name);
                        }
                    }
                }
                // The call first, so the borrow of `local` has ended before the note takes the
                // whole pane.
                let result = self.local.call(name, args);
                let receipt = registry::receipt_of(typed, &result);
                // 🚨 **The line James pointed at**: `ok /viewport center agent —
                // {"accepted":"viewport center agent"}`, one per command, accumulating above the
                // first message. It is a true sentence about a thing that already announced
                // itself — the layout moved — so it is narration, and narration is what
                // [`Self::trace`] is for. ⚠️ **The refusal is not**: nothing else on screen would
                // say a console command was rejected, so it goes through `note` and is seen
                // whatever the mode. `receipt.ok` is asked rather than the text parsed, which is
                // exactly what `registry::Receipt` carries that field for.
                if receipt.ok {
                    self.trace(registry::receipt(typed, &result));
                } else {
                    self.note(registry::receipt(typed, &result));
                }
                receipt
            }
        }
    }

    /// Hold a command's answer over the composer until the line it answers is edited.
    ///
    /// ⚠️ **`answered` is read from the composer as it is *now*** — after the run lane has
    /// cleared it or the refusal lane has left it alone — which is what gives the two
    /// outcomes their different lifetimes without either of them knowing about the other.
    fn answer(&mut self, receipt: Receipt) {
        self.receipt = Some(PanelReceipt { receipt, answered: self.composer.clone(), since: None });
    }

    /// Open the editor and answer the line that asked for it.
    ///
    /// ⚠️ The receipt is written but will not be *seen*: `command_panel` gives the band to the
    /// editor, which is the surface the receipt would have been announcing. It still goes to
    /// the pane's log, and it is still what `Resolved::Run`'s contract requires, so the two
    /// halves of §1.9's receipt rule stay true even where one of them is invisible. The keys
    /// are named on the editor's own last row instead, which is where a hand is looking.
    fn open_editor_receipt(&mut self, typed: &str, theme: &Theme, name: &str) -> Receipt {
        self.open_theme_editor(theme, name, None);
        let text = format!("{typed} - editing `{name}` live; nothing is stored until you save");
        // ✏️ **`trace`, not `note`.** James circled two of these at the head of a transcript:
        // *"it should not feel like part of the conversational flow… when everything is moving
        // right, I generally don't care about this stuff unless there is some exception or
        // problem."* An editor that opened is not an exception — the editor is on screen, and
        // the band's own last row names its keys. The line is still **recorded**, so `/trace on`
        // and anything that later reads the log still have it; it simply is not drawn.
        self.trace(text.clone());
        Receipt { ok: true, text }
    }

    /// Open the live palette editor on the palette being painted, optionally on one field.
    ///
    /// ⚠️ `focus` has **no command form yet** and is always `None` from the slash surface.
    /// `/theme`'s schema carries one argument, so `/theme edit human_text` is not a line the
    /// registry can produce; the parameter exists because landing on a named colour is a
    /// one-line change the moment a second argument is worth adding, and because the editor's
    /// tests need it. It is not dead code pretending to be a feature — nothing claims the
    /// command exists.
    ///
    /// 🚨 **`name` is handed in and cannot be recovered here, which is exactly why it is a
    /// parameter.** This crate is given the palette's *values* and not its label, and the
    /// obvious recovery — match the `Theme` against the four compiled ones — is wrong precisely
    /// in the case that matters: a palette carrying stored overrides equals none of them, so a
    /// tuned `light` would fail to identify as `light` and its next save would be filed
    /// somewhere else or nowhere. So the label travels down from `console_main`, which is the
    /// only place that knows it (`Console::theme_name`, set by `theme::select`), through
    /// [`draw`]'s `theme_name`. The invariant "an override is filed under the palette it was
    /// tuned against" is maintained by that thread being correct, and by nothing cleverer.
    fn open_theme_editor(&mut self, theme: &Theme, name: &str, focus: Option<&str>) {
        self.theme_edit = Some(ThemeEditor::open(theme, name, focus));
    }

    /// Draw the editor and report what it changed, closing it on Escape.
    ///
    /// 🚨 **The editor closes itself if the palette changes underneath it.** `console_main`
    /// owns the one `Theme`, and `/theme chocolate` — or the CLI, or an agent's tool call —
    /// can repaint while an editor is open on `light`. Its held HSV would then describe
    /// colours that are no longer there, and the next drag would snap a chocolate field to a
    /// light hue. Comparing the incoming palette against what the editor last painted is one
    /// `PartialEq` on a struct of sixty-eight colours per frame, which is nothing, and it is
    /// the only signal available: this crate is not told when the palette is reassigned.
    /// ⚠️ **Returns the plate's overflow alongside the change**, for the reason
    /// [`command_panel`] states: the editor draws through the same [`plate`] as the candidate
    /// list, so it is subject to the same rule — a plate that outgrows its reservation paints
    /// over the composer instead of pushing the scrollback up. Discarding it here would have
    /// left the tallest band in the console as the one place that guarantee did not hold.
    fn theme_editor_ui(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
    ) -> (f32, Option<ThemeChange>) {
        let Some(editor) = self.theme_edit.as_mut() else { return (0.0, None) };
        if editor.working() != theme {
            // Not ours. Somebody else repainted; the session is over and the palette on screen
            // is the truth.
            self.theme_edit = None;
            // ✏️ **`trace`, not `note`** — the third of the lines James circled. It explains a
            // *normal* outcome of a thing he just did: the palette he repainted is the palette
            // on screen, and the editor for the old one closed because it was no longer editing
            // anything. Recorded, not drawn.
            self.trace(
                "the palette changed while the theme editor was open, so the editor closed — \
                 `/theme edit` reopens it on the new one"
                    .to_string(),
            );
            return (0.0, None);
        }
        let rows = editor.band_rows();
        let row = ui
            .text_style_height(&egui::TextStyle::Body)
            .max(ui.text_style_height(&egui::TextStyle::Monospace));
        let spacing = ui.spacing().item_spacing.y;
        // The same arithmetic `candidate_panel` uses, for `plate`'s reserved-not-discovered
        // reason. A `DragValue` is taller than a label, so the row metric is floored at the
        // interact size or the last row would be clipped by its own widgets.
        let row = row.max(ui.spacing().interact_size.y);
        let band = rows as f32 * row
            + (rows.saturating_sub(1)) as f32 * spacing
            + 2.0 * PALETTE_PAD_Y as f32
            + 2.0 * PALETTE_STROKE;

        let mut change = None;
        let overflow = plate(ui, band, theme, |ui| {
            if let Some(editor) = self.theme_edit.as_mut() {
                change = editor.ui(ui, theme);
            }
        });
        (overflow, change)
    }

    /// What the panel above the composer would offer for the line as it stands, or `None`
    /// when the line is not a command line, has been dismissed, or has nothing to say.
    fn palette(&self) -> Option<Palette> {
        if self.palette_dismissed {
            return None;
        }
        let palette = self.registry.candidates(&self.composer)?;
        (!palette.is_empty()).then_some(palette)
    }

    /// Let go of a dismissal the moment the line is edited.
    ///
    /// 🚨 **Called once per frame, before anything reads the panel**, and it is the whole of
    /// what makes §1.9's *"Escape shuts the panel until the line changes"* true. The rule
    /// lives here rather than in [`ConversationPane::palette`] on purpose: `palette` is the
    /// question, and a `&self` read that quietly rewrote state to answer itself would put the
    /// rule in the place that is *asked* rather than the place that *knows*.
    ///
    /// ⚠️ The one case it does not catch: a line replaced by an identical line **within a
    /// single frame** — select-all then paste the same text, both landing on one pass. The
    /// text never differs at the moment this looks, so the dismissal survives. Every ordinary
    /// route to retyping a string passes through a frame in which it is shorter.
    fn notice_edit(&mut self) {
        if self.composer != self.composer_seen {
            self.composer_seen.clear();
            self.composer_seen.push_str(&self.composer);
            self.palette_dismissed = false;
        }
    }

    /// Whether the arrows are still walking history, asked of the composer rather than
    /// remembered.
    ///
    /// 🚨 **A walk ends when the human edits the recalled line, and this is how that is
    /// noticed without a second flag to keep in step.** [`ConversationPane::history_at`] is
    /// only believed while the composer still holds exactly what the walk put there; one
    /// keystroke makes them differ and the walk is simply over, with nothing to reset.
    fn walking(&self) -> bool {
        self.history_at.and_then(|at| self.history.get(at)) == Some(&self.composer)
    }

    /// Step through the command history: `back` is the Up key, forward is Down.
    ///
    /// **It does not wrap**, and the panel's highlight does — the difference is deliberate.
    /// A ring of eight verbs has no end worth feeling; a history does, and a walk that
    /// silently rolled from the oldest command to the newest would be indistinguishable from
    /// having lost your place. Stepping forward past the newest returns to an empty box,
    /// which is where the walk started.
    fn history_step(&mut self, back: bool) {
        if self.history.is_empty() {
            return;
        }
        // Never `self.history_at` directly: an index left over from a walk the human has
        // since typed over would otherwise resume from wherever it stopped.
        let at = self.walking().then_some(self.history_at).flatten();
        let next = match (at, back) {
            (None, true) => Some(0),
            (None, false) => return,
            (Some(at), true) => Some((at + 1).min(self.history.len() - 1)),
            (Some(0), false) => None,
            (Some(at), false) => Some(at - 1),
        };
        self.history_at = next;
        self.composer = next.and_then(|at| self.history.get(at).cloned()).unwrap_or_default();
        self.palette_selected = 0;
        self.palette_dismissed = false;
        self.want_caret = true;
        // The answer to the previous command is not an answer to a line just recalled.
        self.receipt = None;
    }

    /// Put a sent line into the history, if it earns a place.
    ///
    /// **Commands only.** James asked for a *slash command* buffer, and prose is both long
    /// and the thing the transcript above already keeps — a walk that had to step over three
    /// paragraphs to reach `/posture desktop` would not be a recall surface.
    ///
    /// ⚠️ **A refusal is remembered, and that is the case the buffer is most for**: a command
    /// that ran is a command you no longer need back, while one the registry refused is a
    /// line with a typo in it that you want in front of you again to fix. `Resolved::Escaped`
    /// is not a command at all — `//` means the line is a message — so it is left out with
    /// the prose.
    fn remember_command(&mut self, text: &str, resolved: &Resolved) {
        if !matches!(resolved, Resolved::Run { .. } | Resolved::Refused(_)) {
            return;
        }
        // No consecutive duplicates: running one command twice is one thing to walk back to.
        if self.history.front().map(String::as_str) == Some(text) {
            return;
        }
        if self.history.len() == HISTORY_LINES {
            self.history.pop_back();
        }
        self.history.push_front(text.to_string());
    }

    /// Take a candidate: the line becomes its completion, **whole**. Never a splice — a
    /// [`Candidate::completion`] is the entire line, which is why one renderer's accept is
    /// every renderer's accept.
    fn accept(&mut self, candidate: &Candidate) {
        self.composer = candidate.completion.clone();
        self.palette_selected = 0;
        self.palette_dismissed = false;
        // A rewritten line leaves egui's caret wherever it was — see `put_caret_at_end`.
        self.want_caret = true;
        // The answer to the previous command is not an answer to this line.
        self.receipt = None;
    }

    /// Add an **exception** to the console's own remarks about this session — recorded in the
    /// status log like everything else, and additionally drawn at the head of the scrollback and
    /// lighting the pane's status line. Public because the tab's working directory is decided by
    /// whoever opened the tab (`console_main`), not in here, and it is exactly the kind of thing
    /// this pane exists to say out loud.
    ///
    /// **Always seen.** This is the loud half of the pair; [`Self::trace`] is the quiet one, and
    /// the default staying loud is what [`Remark`] is for.
    ///
    /// 🚨 **The bar is James's: *"unless there is some exception or problem."*** A refusal, a
    /// send that failed, an audit that proved nothing, a tab with no project. If a line is true
    /// and routine it belongs in [`Self::trace`] — which no longer means it is thrown away.
    pub fn note(&mut self, line: String) {
        self.remark(Remark::note(line));
    }

    /// Add a line to the status log and **nowhere else** — the machinery, not the news.
    ///
    /// ⚠️ **This is no longer a way of hiding something.** Before [`crate::status_log`] a traced
    /// line was drawn only under `/trace on`, interleaved into the conversation, so choosing it
    /// meant choosing between noise and silence. It now means "the log, and only the log": the
    /// line is kept, it is summarised on the pane's status line, and it is in the panel a click on
    /// that line — or `/trace on` — drops down.
    /// So the test is simply whether the thing described is an exception — and when it is
    /// ambiguous, this is the right answer.
    pub fn trace(&mut self, line: String) {
        self.remark(Remark::machinery(line));
    }

    fn remark(&mut self, remark: Remark) {
        self.log.push(remark);
    }

    /// Open or close the status log. Answers what to say about it — see [`registry::VERB_TRACE`].
    ///
    /// 🚨 **Opening it acknowledges it**, which is the only event that clears the band's
    /// indicator — [`StatusLog::acknowledge`] carries the argument for why looking, and nothing
    /// else, is what counts as having read it.
    ///
    /// ⚠️ **`on` is echoed and `off` is not**, and that falls straight out of the rule rather
    /// than being a second decision: the acknowledgement goes to [`Self::trace`], i.e. into the
    /// log itself, where opening it is exactly what puts it on screen. Switching off simply goes
    /// quiet, which is the thing asked for and needs no sentence.
    ///
    /// ⚠️ **Acknowledge AFTER the line is written**, or the line announcing the log would count
    /// as unread the moment it was recorded — a machinery line cannot light the indicator, but
    /// the ordering is the kind that only stops being harmless once somebody changes one of the
    /// two.
    pub fn set_tracing(&mut self, on: bool) {
        self.tracing = on;
        self.trace(if on {
            "status log open — every line this console wrote about the session. `/trace off` \
             closes it."
                .to_string()
        } else {
            "status log closed".to_string()
        });
        if on {
            self.log.acknowledge();
        }
    }

    /// The status line was clicked: open the log, or close it if it is already open.
    pub fn toggle_log(&mut self) {
        self.set_tracing(!self.tracing);
    }

    /// Whether the status log is open. Read by [`draw`], [`status_line`] and [`log_drop_down`].
    pub fn tracing(&self) -> bool {
        self.tracing
    }
}

/// Draw the pane: scrollback, then composer. Returns what the frame produced — see
/// [`ConversationOutput`].
///
/// `images` is what the console has ready to paint into the surfaces it was asked for last
/// frame. **One frame of latency is structural, not a shortcut**: a surface's rect is an
/// output of egui layout, so nothing can know its size until the frame that lays it out has
/// run, and the texture is therefore made for the *next* one. `console_main.rs` already sizes
/// the whole backdrop this way for the same reason, and the visible consequence is one blank
/// frame when a surface is summoned.
///
/// Bottom-up, because the composer's and the strip's heights are known and the scrollback's
/// is whatever is left — the layout every chat client resolves in that order.
///
/// ⚠️ **The order of these calls is the visual order, upside down.** In a bottom-up
/// column the *first* thing added sits lowest, so this reads: strip at the very bottom,
/// composer above it, a rule, and the scrollback taking everything that is left. The status
/// used to be added between the composer and the rule, which put it *above* the composer —
/// where a one-line band with a rule under it reads as a divider rather than as the thing it
/// is. [`status_strip`] belongs with the composer, at the bottom, which is where Claude
/// Desktop puts the model affordance and where a hand looking for it goes.
///
/// 🚨 **THE ENTRY BOX NEVER MOVES, and that is a layout invariant rather than a preference.**
/// James, 2026-08-21, on the surface #127 shipped: *"its positioning isn't right. It should not
/// be displacing the entry box. The entry box should never move."* Everything that can appear,
/// vanish or resize now lives on one side or the other of the composer and never between the
/// band and it: the status log is a permanent one-row line at the **top** of the pane plus an
/// `egui::Area` drop-down that takes no layout space at all. The property is measured, not
/// asserted — [`composer_rect`] publishes the box's rect every frame and
/// [`tests::the_entry_box_never_moves_when_the_status_log_opens`] compares it open against
/// closed at two pane heights.
/// `theme_name` is the palette's canonical name, handed down because this crate is given the
/// palette's *values* and cannot recover its label — once a stored override has been laid over
/// it, the live palette equals none of the compiled ones. The live editor files what it saves
/// under this name, and filing under the wrong one would apply a light-theme correction to a
/// dark palette.
pub fn draw(
    ui: &mut egui::Ui,
    pane: &mut ConversationPane,
    images: &SurfaceImages,
    exhibits: &ExhibitContents,
    theme: &Theme,
    theme_name: &str,
    form: &Form,
    panel_home: panel_stack::Home,
) -> ConversationOutput {
    // 🚨 **Set before anything is laid out**, because the composer is drawn inside the column
    // below and a `/organon` line submitted there reads it in the same frame. See
    // [`ConversationPane::panel_home`] on why it is a field rather than five more parameters.
    pane.panel_home = panel_home;
    let mut out = ConversationOutput::default();
    // 🚨 **Held out here and assigned after the column, because `scrollback` returns a whole
    // `ConversationOutput` and `out = scrollback(…)` REPLACES the struct.** Writing the
    // editor's change straight onto `out` inside the column — which is what this did when the
    // editor landed — discards it three lines later, every frame, silently: the drag computes
    // a correct `ThemeChange`, and nothing downstream ever sees it. Assign after the column,
    // never inside it.
    let mut theme_change: Option<ThemeChange> = None;
    // Taken before anything is allocated in the pane, so the ticks mark the *conversation
    // area* — the whole of what this front-end was given — rather than whatever the
    // bottom-up column happened to leave over.
    let area = ui.max_rect();
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        status_strip(ui, pane, theme);
        ui.add_space(4.0);
        composer(ui, pane, theme, theme_name);
        // Above the composer, and drawn AFTER it: the composer's own keys may have completed
        // the line this frame, and the panel must show the ring that completion opened rather
        // than the one it closed. In a bottom-up column later means higher.
        ui.add_space(4.0);
        let (_overflow, change) = command_panel(ui, pane, theme, form);
        theme_change = change;
        ui.add_space(4.0);
        ui.separator();
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            // 🚨 **The status line is the FIRST thing in the top-down remainder, and it is one
            // row whatever it says.** Everything below it — including the scrollback — is laid
            // out after it, so a change of state moves nothing; and it is on the far side of the
            // bottom-up column from the composer, so it cannot move the entry box at all. That
            // is the whole reason the surface moved here: #127 drew the log between the band and
            // the composer, and opening it pushed the box James types into up the screen.
            let line = status_line(ui, pane, theme);
            ui.add_space(4.0);
            out = scrollback(ui, pane, images, exhibits, theme, form);
            // 🚨 **LAST, and in a layer of its own.** The drop-down is an `egui::Area`, so it
            // takes no space in this column and cannot displace anything: it hangs off the
            // status line and paints over the page, Quake-console style. Drawing it after the
            // scrollback is what puts it above the transcript within that layer's own order.
            log_drop_down(ui, pane, theme, line, area);
        });
    });
    out.theme = theme_change;
    // Held out and assigned here for `theme_change`'s reason, spelled out four lines above:
    // `out = scrollback(…)` replaces the struct, so anything written onto `out` inside the
    // column is discarded three lines later, every frame, in silence.
    out.panel = pane.panel_wanted.take();
    // Last, so nothing the flow draws can cover them — the same call-order enforcement the
    // patch paints rely on, one layer up. At terminal posture this returns without touching
    // the painter.
    registration_ticks(ui, area, theme, form);
    out
}

/// **The four corner marks that say where the page is** — a printer's registration mark, and
/// the one thing in the desktop form that is not part of any element.
///
/// They arrive with posture and nothing else: at the terminal end
/// [`Form::tick_color`] answers `None` and this function returns before it has asked the
/// painter for anything, which is why the tier can claim it draws nothing new.
///
/// Colour is [`Theme::dim`] — "present, but not being read" is exactly what a registration
/// mark is, and it is the one role on the palette that already means it. Giving them a field
/// of their own would have added a colour every future palette has to answer for, to say
/// something `dim` already says.
fn registration_ticks(ui: &egui::Ui, area: egui::Rect, theme: &Theme, form: &Form) {
    let Some(color) = form.tick_color(theme.dim) else { return };
    let len = form.tick_len;
    let stroke = egui::Stroke::new(1.0f32, color);
    let painter = ui.painter();
    // (corner, which way its arms point) — the signs are what turn one loop into four
    // corners rather than four copies of the same two lines.
    for (corner, dx, dy) in [
        (area.left_top(), 1.0, 1.0),
        (area.right_top(), -1.0, 1.0),
        (area.left_bottom(), 1.0, -1.0),
        (area.right_bottom(), -1.0, -1.0),
    ] {
        painter.line_segment([corner, corner + egui::vec2(dx * len, 0.0)], stroke);
        painter.line_segment([corner, corner + egui::vec2(0.0, dy * len)], stroke);
    }
}

/// A small caption in the flow — a word that *names* a thing rather than being read.
///
/// Posture's letter-spacing lands here and nowhere else, so "a label" has one spelling and a
/// site that forgets it is a site that does not go through this function. The size is
/// resolved from the live style rather than assumed, because the tracking is specified in
/// **em**: a console at a larger text size must open its labels by the same proportion, not
/// by the same number of points.
fn label(ui: &egui::Ui, text: impl Into<String>, color: Color32, form: &Form) -> RichText {
    let size = egui::TextStyle::Small.resolve(ui.style()).size;
    RichText::new(text).color(color).small().extra_letter_spacing(form.tracking(size))
}

/// Body text at posture's line height.
///
/// ⚠️ At the terminal end this passes `None`, which is not the same as passing the number
/// egui would have computed: `None` *is* what the console does today, so the layout is
/// unchanged by construction rather than by arithmetic that ought to agree.
fn body(ui: &egui::Ui, text: impl Into<String>, color: Color32, form: &Form) -> RichText {
    let row = ui.text_style_height(&egui::TextStyle::Body);
    RichText::new(text).color(color).line_height(form.body_line_height(row))
}

/// The rule down a card's left edge — the other half of the border's exchange.
///
/// Drawn *after* the frame it belongs to, from the rect the frame reports, because the
/// height of a card is an output of laying its contents out and nothing knows it earlier.
/// Two independent ways for this to draw nothing, and both are meant: posture at the
/// terminal end, and a palette whose [`Theme::card_left_rule`] is transparent — which
/// `organon`'s is.
fn card_left_rule(ui: &egui::Ui, rect: egui::Rect, theme: &Theme, form: &Form) {
    let Some(color) = form.left_rule_color(theme.card_left_rule) else { return };
    ui.painter().vline(rect.left(), rect.y_range(), egui::Stroke::new(LEFT_RULE_WIDTH, color));
}

/// The left rule's width in points. Not a posture token: what changes between the two
/// postures is whether the rule is there, and a rule that also thickened would be two
/// statements in one lerp.
const LEFT_RULE_WIDTH: f32 = 2.0;

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
    exhibits: &ExhibitContents,
    theme: &Theme,
    form: &Form,
) -> ConversationOutput {
    // Collected during the walk and joined after it. A panel is *below* its surface in the
    // ordinary case, but nothing in the model requires that — the link is an id, not an
    // adjacency — so the join happens once the whole list has been seen rather than by
    // reaching backwards mid-loop.
    let mut laid_out: Vec<LaidOutSurface> = Vec::new();
    let mut wanted_exhibits: Vec<ExhibitRequest> = Vec::new();
    let mut drives: Vec<PanelDrive> = Vec::new();
    // The transcript is walked immutably, so a decision taken mid-walk is applied after it.
    // The *agent* is not made to wait for that: the verdict goes back on the wire inside the
    // loop, the moment the button is read.
    let mut answered: Vec<(ElementId, Answer)> = Vec::new();
    let mut revoked: Vec<ElementId> = Vec::new();
    // Density toggles, collected for the same reason a verdict is: the transcript and the
    // density map are both read through the walk, so a press is applied after it.
    let mut toggled_cards: Vec<ElementId> = Vec::new();
    let mut toggled_groups: Vec<ElementId> = Vec::new();
    // Destructured so the transcript can be read while the widget state is written: they
    // are disjoint fields, and keeping them disjoint is the whole point of the side map.
    let ConversationPane {
        transcript,
        artifacts,
        diffs,
        density: density_map,
        pinned,
        sliders: defaults,
        waiting,
        memory,
        log,
        ..
    } = pane;
    // ✏️ **`tracing` is deliberately not destructured any more.** The scrollback used to read it
    // twice — to widen the remark loop and to draw an empty-transcript hint — and both were the
    // conversation being changed by a *view mode*. It now decides nothing in here, which is the
    // structural half of the promise `/trace on` makes.
    // 🚨 **Everything about card density is decided here, before a single row is laid out**,
    // and `*pinned` is what makes it safe: an automatic collapse is applied only while the
    // view is following the live edge, where `stick_to_bottom` holds the last row still and
    // the shrink is absorbed above it. `crate::card_density`'s module doc owns the argument.
    // `*pinned` is last frame's reading, re-derived from where the reader actually left the
    // scroll — which is exactly the question being asked, one frame's worth of lag included.
    density_map.settle(transcript.elements().iter(), *pinned);
    let gated = card_density::gated_calls(transcript.elements().iter());
    let rows = card_density::plan(&card_density::slots(
        transcript.elements().iter(),
        density_map,
        &gated,
    ));
    // Re-borrowed immutably for the walk; the toggles it collects are applied after it ends,
    // which is when this borrow does.
    let density: &DensityMap = density_map;
    let out = egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(*pinned)
        .show(ui, |ui| {
            // The visible slice of the scrollback, read once for the whole walk. Every
            // surface's visibility is decided against this one rect, so two surfaces cannot
            // come to disagree about where the viewport is.
            //
            // ⚠️ Read from the scroll area's own `Ui`, which is the right rect however the
            // pane was inset: a surface is visible or not according to where the *viewport*
            // is, and a pane-level margin narrows the viewport along with everything else, so
            // this stays the question being asked rather than a stale copy of the window.
            let viewport = ui.clip_rect();
            // ✏️ **Still a closure, though nothing wraps it here any more.** The margin moved
            // to the pane (see below), and the two callers that remain are this one and the
            // grouped-row branch inside the walk itself. Kept as a closure rather than
            // inlined because the alternative is a second copy of the body.
            let mut walk = |ui: &mut egui::Ui| {
                ui.add_space(6.0);
                // The console's own remarks about this session, above the first message —
                // **the exceptions, and nothing else, in either mode.**
                //
                // 🚨 **`tracing` is deliberately NOT read here, and that is the change.** It used
                // to widen this loop to the whole log, which is how `/trace on` came to mean
                // "make my conversation noisier" — the one thing James asked for the opposite
                // of. Every quiet line still exists, in `crate::status_log`, summarised at the
                // top of the pane; what it no longer has is a route into the flow.
                //
                // ⚠️ Anything written here was once drawn NOWHERE at all, so "approvals are not
                // wired — a tool that needs permission will fail instead of asking" had never
                // once been visible. That is why `note` stays the loud default and why the
                // indicator exists: the failure this surface keeps finding is silence, and the
                // remedy for a quiet line is now a place to keep it rather than a mode.
                let mut said = 0_usize;
                for remark in log.exceptions() {
                    ui.label(RichText::new(&remark.text).color(theme.dim).italics());
                    said += 1;
                }
                if said > 0 {
                    ui.add_space(6.0);
                }
                // ✏️ **The empty-transcript hint is gone entirely.** It was already drawn only
                // while tracing — the console explaining itself to somebody who has not seen it
                // before, which James is not — and `tracing` now means "the status log is open",
                // which has nothing to say about whether a conversation has started. A sentence
                // that would appear in the flow because a *log* was opened is the leak this
                // change closes. An empty transcript is self-evidently empty and the composer is
                // directly below it with its own hint.
                // One element, drawn as itself. A closure rather than the loop body it used
                // to be, because there are now two callers: the ordinary walk, and the
                // members of a group a hand has opened. The body is untouched.
                let mut draw_body = |ui: &mut egui::Ui, element: &Element| {
                    // 🚨 The quiet/loud rule, applied to the transcript itself — see
                    // [`element_seen`]. Skipped **whole**, its trailing `card_gap` included,
                    // so a hidden element leaves no space behind it.
                    if !element_seen(&element.body) {
                        return;
                    }
                    match &element.body {
                        // The one body drawn here rather than in `draw_element`: it is the only
                        // one that needs state to survive between frames, and `draw_element`
                        // has nowhere to keep it.
                        Body::Artifact(artifact) => match &artifact.content {
                            ArtifactContent::Panel(spec) => {
                                // Empty on the first frame; `panel_body` syncs it to the
                                // description, which is where the starting values come from.
                                let state = artifacts.entry(element.id).or_default();
                                panel_element(
                                    ui, element.id, artifact, spec, state, defaults, theme, form,
                                );
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
                                    theme,
                                    form,
                                );
                                if surface_visible(rect, viewport) {
                                    laid_out.push(LaidOutSurface {
                                        element: element.id,
                                        look: spec.look.clone(),
                                        size_points: (rect.width(), rect.height()),
                                    });
                                }
                            }
                            // The two media arms share a renderer and differ by one flag: what
                            // they place is the same card with the same items and the same
                            // per-item states, and only the body of an item differs. A second
                            // function would have been the same code twice with one `match`
                            // moved into it.
                            ArtifactContent::Image(spec) | ArtifactContent::Markdown(spec) => {
                                let picture =
                                    matches!(artifact.content, ArtifactContent::Image(_));
                                let rects = exhibit_element(
                                    ui, element.id, artifact, spec, picture, exhibits, theme,
                                    form,
                                );
                                // Only pictures are requested: a document's rect is `ZERO` by
                                // contract, and asking for one would be asking the console to
                                // decode a texture nothing draws.
                                if picture {
                                    for (i, rect) in rects.iter().enumerate() {
                                        if surface_visible(*rect, viewport) {
                                            wanted_exhibits.push(ExhibitRequest {
                                                element: element.id,
                                                item: i,
                                                path: spec.items[i].path.clone(),
                                                size_points: (rect.width(), rect.height()),
                                            });
                                        }
                                    }
                                } else {
                                    // A document still has to be *read*, and the read is the
                                    // console's off-thread job exactly as a decode is. The
                                    // request carries a zero size, which is what tells the
                                    // reader it wants bytes rather than a texture.
                                    for (i, item) in spec.items.iter().enumerate() {
                                        wanted_exhibits.push(ExhibitRequest {
                                            element: element.id,
                                            item: i,
                                            path: item.path.clone(),
                                            size_points: (0.0, 0.0),
                                        });
                                    }
                                }
                            }
                        },
                        // The second body drawn here rather than in `draw_element`, and for a
                        // sharper version of the same reason: answering one needs the question
                        // the pane is holding, which `draw_element` has no access to.
                        Body::Approval(block) => {
                            let live = waiting.contains_key(&element.id);
                            match approval_card(ui, element.id, block, live, theme, form) {
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
                        // The third body drawn here rather than in `draw_element`, for the
                        // same reason as the first: it needs state that survives between
                        // frames. What survives is its diff — see
                        // [`ConversationPane::diffs`], and note that the entry is computed
                        // here and merely *read* by `tool_card`, so the card stays a
                        // function of what it is given.
                        Body::Tool(card) => {
                            let diff = diffs.entry(element.id).or_insert_with(|| {
                                edit_diff(card.name.as_deref(), &card.arguments)
                            });
                            // The one arm density reaches: a settled success is one line,
                            // and everything else — running, failed, hand-opened — is the
                            // card it always was. `tool_card` is unchanged.
                            if density.is_open(element.id) {
                                tool_card(ui, card, diff.as_ref(), theme, form);
                            } else if dense_card(
                                ui,
                                element.id,
                                card,
                                diff.as_ref().map(|d| &d.diff),
                                gated.contains(card.call_id.as_str()),
                                theme,
                                form,
                            ) {
                                toggled_cards.push(element.id);
                            }
                        }
                        _ => draw_element(ui, element, theme, form),
                    }
                    ui.add_space(form.card_gap);
                };
                let elements = transcript.elements();
                for row in &rows {
                    match row {
                        Row::One(index) => draw_body(ui, &elements[*index]),
                        Row::Group { key, start, len } => {
                            let open = density.is_group_open(*key);
                            let line = card_density::group_line(
                                elements.range(*start..*start + *len).filter_map(|e| e.tool()),
                            );
                            if group_row(ui, *key, &line, open, theme, form) {
                                toggled_groups.push(*key);
                            }
                            if open {
                                for index in *start..*start + *len {
                                    draw_body(ui, &elements[index]);
                                }
                            } else {
                                ui.add_space(form.card_gap);
                            }
                        }
                    }
                }
            };
            // 🚨 **No margin is claimed here, and its absence is the fix rather than a
            // regression.** This walk used to wrap itself in a `Frame` carrying the posture's
            // margin, which inset the transcript and *only* the transcript: the composer, the
            // command panel and the status strip below it stayed flush to the pane's edge, and
            // a terminal tab — which never reaches this function at all — was untouched by
            // `posture desktop` entirely. The margin now belongs to whoever draws the pane
            // (`console_main.rs`'s `draw_active_pane`), so this `Ui` arrives already inset and
            // everything the pane lays out moves together. Claiming it a second time here
            // would inset the transcript twice and centre it inside an already-centred column.
            walk(ui);
        });
    *pinned = pinned_after_scroll(out.state.offset.y, out.content_size.y, out.inner_rect.height());
    // A hand's answer, recorded as the hand's — nothing automatic ever undoes it, which is
    // the whole of "a card that re-collapses under a reader's hand is worse than one that
    // never collapsed". Applied after the walk for the same reason a verdict is.
    for id in toggled_cards {
        density_map.toggle_card(id);
    }
    for key in toggled_groups {
        density_map.toggle_group(key);
    }
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
    // The same line again, and it is what makes the diff cache safe on a session that runs
    // all day: an entry holds at most `MAX_ROWS` rows of text however large the edit it came
    // from was, so the cache is bounded by the transcript's own cap rather than by anything
    // about the edits.
    diffs.retain(|id, _| transcript.get(*id).is_some());
    // ⚠️ The same line, with teeth: a question whose element the cap evicted can never be
    // answered by a human, so dropping it here **denies** it (`crate::approval`) instead of
    // leaving the agent blocked for the rest of the session.
    waiting.retain(|id, _| transcript.get(*id).is_some());
    // And once more for the density map, so a card the cap evicted takes its collapsed state
    // and any group it anchored with it.
    density_map.retain(|id| transcript.get(id).is_some());
    // The scrollback cannot change the palette — only the editor above the composer can, and
    // `draw` assigns that field from `command_panel`'s answer. Nor can it summon a panel: a
    // `/organon` line arrives through the composer, and `draw` drains that field too, for the
    // same reason and after the same column.
    ConversationOutput {
        surfaces: join_drives(laid_out, drives),
        exhibits: wanted_exhibits,
        theme: None,
        panel: None,
    }
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

/// **Is this transcript element on screen, given the mode?**
///
/// 🚨 **`— turn complete` was the harness narrating itself, and James's model is Claude
/// Desktop, where a finished turn is simply a finished turn.** The reply is on the page and
/// the composer is live again; both say it without a caption, and the caption said it after
/// every single turn for the life of the tab.
///
/// This is the pane's standing rule ([`Remark`]) reaching one surface further out: an
/// **acceptance** is not drawn, a **failure** always is. `Error` and `Cancelled` therefore keep
/// their captions unconditionally — they are the two a reader cannot infer from a page that has
/// merely stopped growing, and a turn that was cancelled looks exactly like one that finished if
/// nothing says otherwise.
///
/// 🚨 **It no longer takes the trace mode, and that is deliberate rather than tidying.** `/trace
/// on` now opens the status log ([`crate::status_log`]) and must not reach into the transcript at
/// all — a click on the status line that also put `— turn complete` under every reply would
/// be exactly the leak this change closes, arriving by a new route. A caption is not a
/// [`Remark`] and has no log to move to, so the honest consequence is that the successful one is
/// simply not drawn: the reply is on the page and the composer is live again, which is the
/// paragraph above.
///
/// ⚠️ **[`RunEnd::detail`] goes with the element it rides on.** On the failure arms — where a
/// detail carries a reason — it is still drawn. On a success it is the same post-turn
/// narration by another name, which is what the wire's `status_detail` is; the band's own
/// echo of that field is hidden on the identical argument ([`StatusReading::narration`]).
///
/// ⚠️ Every other body returns `true` **by falling through, not by being listed** — a new
/// `Body` variant is visible until somebody decides otherwise, which is the safe default for
/// a surface whose failure mode is swallowing something.
fn element_seen(body: &Body) -> bool {
    match body {
        Body::RunEnd(end) => end.outcome != RunOutcome::Ok,
        _ => true,
    }
}

fn draw_element(ui: &mut egui::Ui, element: &Element, theme: &Theme, form: &Form) {
    match &element.body {
        Body::Human(h) => {
            let framed = Frame::new()
                .fill(theme.human_fill)
                .corner_radius(form.card_corner())
                .inner_margin(form.human_margin())
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(label(ui, "you", theme.dim, form));
                    ui.label(body(ui, &h.text, theme.human_text, form));
                });
            card_left_rule(ui, framed.response.rect, theme, form);
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
            ui.label(body(ui, text, theme.prose, form));
        }
        // Drawn by `scrollback`, which holds the widget state a panel needs between
        // frames, the question an approval is asking, and the diff a tool card draws.
        // Nothing to do here, and nothing missing: an element is drawn exactly once, by
        // whichever of the two has what it needs.
        // Drawn in the walk rather than here, all three for one reason: they need state that
        // survives between frames (a slider mid-drag, a panel's live widget values) and this
        // function has nowhere to keep it.
        Body::Artifact(_) | Body::Approval(_) | Body::Tool(_) => {}
        Body::RunEnd(end) => {
            // Named `outcome` rather than `label`, which is now a function in this module:
            // a local of that name would shadow it and the two calls below would stop
            // compiling — a rename is cheaper than a second spelling of "a small caption".
            let (outcome, color) = match end.outcome {
                RunOutcome::Ok => ("turn complete", theme.dim),
                RunOutcome::Error => ("turn failed", theme.bad),
                RunOutcome::Cancelled => ("turn cancelled", theme.running),
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
                ui.label(label(ui, format!("— {outcome}"), color, form));
                if !detail.is_empty() {
                    ui.label(label(ui, detail, theme.dim, form));
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
/// `Edit` goes one step further and renders its `old_string`/`new_string` as a real
/// **aligned** diff ([`crate::text_diff`]), because those arrive as *fields*, not as a
/// patch someone has to parse back out of prose. And the result's own sibling object —
/// `tool_use_result`, which a terminal never sees — becomes [`detail_rows`]: for a `Read`,
/// how much of the file the call actually covered.
///
/// ⚠️ **`diff` is handed in rather than computed here**, and that is a correctness-neutral
/// change with a large price attached: computing it in this body meant re-parsing the
/// arguments and re-aligning the text on every frame, for every card in a scrollback that
/// is not virtualised. [`ConversationPane::diffs`] owns it now. `None` means this card has
/// no diff to draw — it is not an `Edit`, or its arguments have not settled — which is
/// exactly what [`edit_diff`] returns and is why the two cases stay one branch here.
fn tool_card(
    ui: &mut egui::Ui,
    card: &ToolCard,
    diff: Option<&EditDiff>,
    theme: &Theme,
    form: &Form,
) {
    let (state_text, accent) = match &card.state {
        ToolState::Running => ("running", theme.running),
        ToolState::Complete { is_error: false, .. } => ("ok", theme.ok),
        ToolState::Complete { is_error: true, .. } => ("error", theme.bad),
    };
    // ⚠️ **The accent is on the border, and the border is what posture takes away.** At the
    // desktop end this card is separated by fill and by the rule the palette may or may not
    // give it, so the state stops being readable from the edge — which is why the state
    // *word* beside the name is not optional and never becomes a colour alone. Whether the
    // rule should instead be drawn in this accent is a real question and a one-line change;
    // it needs somebody who can look at a window, and is named in `CONSOLE_ARCHITECTURE.md`
    // §1.6 rather than guessed at here.
    let mut framed = Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(form.card_corner())
        .inner_margin(form.card_margin());
    if let Some(stroke) = form.card_stroke(accent) {
        framed = framed.stroke(stroke);
    }
    let framed = framed
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let name = card.name.as_deref().unwrap_or("(call not seen)");
                ui.label(RichText::new(name).color(accent).strong().monospace());
                ui.label(label(ui, state_text, accent, form));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(card.call_id.as_str()).color(theme.dim).small().monospace(),
                    );
                });
            });

            match diff {
                Some(diff) => diff_body(ui, diff, theme),
                None => arguments_body(ui, &card.arguments, theme),
            }

            for row in detail_rows(&card.detail, &card.arguments) {
                ui.label(RichText::new(row).color(theme.dim).small().monospace());
            }

            // Above the step log on purpose: this describes the task, the log details it.
            if !card.progress.is_empty() {
                progress_body(ui, &card.progress, theme);
            }

            if !card.subagent.is_empty() {
                subagent_body(ui, &card.subagent, card.state.is_running(), theme);
            }

            if let Some(output) = card.state.output() {
                ui.add_space(4.0);
                let (shown, hidden) = clip_lines(output, OUTPUT_LINES);
                for line in shown {
                    ui.label(RichText::new(line).monospace().small().color(theme.prose));
                }
                if hidden > 0 {
                    ui.label(
                        RichText::new(format!("+{hidden} more lines")).color(theme.dim).small(),
                    );
                }
            }
        });
    card_left_rule(ui, framed.response.rect, theme, form);
}

/// **A tool call that worked, as one line** — no frame, no bevel, no border.
///
/// James, looking at a real screenshot: *"a typical screenshot is five or six tool calls,
/// each with a beveled border around it, so it feels like a list of bevel-bordered status
/// updates. You don't want to see all that while you're developing."* This is the answer, and
/// it is a *presentation* answer: [`crate::card_density`] decides **when**, and nothing
/// anywhere drops a byte. The card is one click away and its full arguments, its full output
/// and its correlation id are all still in the model.
///
/// Three parts, in the order a person reads them: **the verb, the object, and a magnitude**.
/// Colour is spent on none of them — success is quiet, and a page of quiet rows is what makes
/// the one red bordered failure legible from across the room.
///
/// 🚨 **The `toolu_` id is drawn when the call was gated, and that is the whole of how the
/// approval↔result link survives.** An approval card and the result it authorises share
/// nothing but that id (`doc/console_approval_protocol.md` §3), so a gated call keeps its own
/// row ([`card_density::Slot::gated`]) *and* keeps the id on it. An ungated call has no
/// approval to be linked to; its id is one click away like everything else.
///
/// Returns whether the row was clicked.
fn dense_card(
    ui: &mut egui::Ui,
    id: ElementId,
    card: &ToolCard,
    diff: Option<&LineDiff>,
    gated: bool,
    theme: &Theme,
    form: &Form,
) -> bool {
    let line = card_density::dense_line(card, diff);
    ui.push_id(id.0, |ui| {
        let mut hit = false;
        ui.horizontal(|ui| {
            let mark = RichText::new(card_density::MORE).color(theme.dim).monospace();
            hit |= ui
                .add(egui::Label::new(mark).sense(egui::Sense::click()))
                .on_hover_text("open — the arguments, the output and the id are all still here")
                .clicked();
            let verb = RichText::new(&line.verb).color(theme.prose).monospace();
            hit |= ui.add(egui::Label::new(verb).sense(egui::Sense::click())).clicked();
            if let Some(object) = &line.object {
                let object = RichText::new(object).color(theme.dim).monospace().small();
                hit |= ui.add(egui::Label::new(object).sense(egui::Sense::click())).clicked();
            }
            if let Some(magnitude) = &line.magnitude {
                let magnitude = label(ui, format!("· {magnitude}"), theme.dim, form);
                hit |= ui.add(egui::Label::new(magnitude).sense(egui::Sense::click())).clicked();
            }
            if gated {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(card.call_id.as_str()).color(theme.dim).small().monospace(),
                    );
                });
            }
        });
        hit
    })
    .inner
}

/// **A run of calls that all worked, as one row.**
///
/// The second half of the density rule: six dense lines are quieter than six cards, and one
/// row is quieter still. What it says is a count and the verbs — see
/// [`card_density::GroupLine`] for why there is no duration in it, which is the one thing the
/// brief that commissioned this asked for and the wire does not carry.
///
/// ⚠️ **Expanding does not restructure.** The group's membership is decided by
/// [`card_density::plan`] from the *settled* bit alone, so opening it — or opening one of its
/// members afterwards — makes rows taller and never splits the group under the reader's hand.
///
/// Returns whether the row was clicked.
fn group_row(
    ui: &mut egui::Ui,
    key: ElementId,
    line: &card_density::GroupLine,
    open: bool,
    theme: &Theme,
    form: &Form,
) -> bool {
    // ⚠️ Namespaced, because the key IS the first member's [`ElementId`] and that member's own
    // row pushes the same number one level down. Two siblings on one id is an egui clash.
    ui.push_id(("card-density-group", key.0), |ui| {
        let mut hit = false;
        ui.horizontal(|ui| {
            let mark = if open { card_density::LESS } else { card_density::MORE };
            let mark = RichText::new(mark).color(theme.dim).monospace();
            hit |= ui
                .add(egui::Label::new(mark).sense(egui::Sense::click()))
                .on_hover_text(if open {
                    "collapse — every call here succeeded"
                } else {
                    "open — every call here succeeded"
                })
                .clicked();
            let count = RichText::new(&line.count).color(theme.prose).monospace();
            hit |= ui.add(egui::Label::new(count).sense(egui::Sense::click())).clicked();
            if let Some(verbs) = &line.verbs {
                // `·` U+00B7 is carried by both of egui's faces — the same glyph the subagent
                // header and `capability_label` already use.
                let verbs = label(ui, format!("· {verbs}"), theme.dim, form);
                hit |= ui.add(egui::Label::new(verbs).sense(egui::Sense::click())).clicked();
            }
        });
        hit
    })
    .inner
}

/// What a dispatch card's progress row says, as text — the judgment, without the egui.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressSummary {
    /// The harness's own one-line gloss of what the agent is doing.
    pub headline: Option<String>,
    /// The measured trailer: last tool, tool count, elapsed, tokens — in that order, and
    /// only the parts that were reported. `None` when none were.
    pub facts: Option<String>,
    /// A terminal status, once one has been reported.
    pub status: Option<String>,
}

/// **What a dispatch card may claim about an agent that is still working.**
///
/// This is the answer to the card that said "running" and then nothing for eight to
/// sixteen minutes. Everything in it was stated by the harness on a `system`/`task_*`
/// line ([`crate::conversation::SubagentProgress`]).
///
/// 🚨 **It reports what was last said, never what is true now, and the difference is the
/// whole honesty of the row.** §5.9.1's measurement is untouched: no token deltas are
/// forwarded from a subagent, so this is not a feed and must never be drawn as one — no
/// caret, no partial text, nothing that implies prose is arriving. What changed is only
/// that the harness *does* narrate its own progress, and that narration is a fact.
///
/// ⚠️ **The elapsed time is the HARNESS'S stopwatch, not ours**, and it is frozen between
/// lines. `conversation.rs` has no clock by design; a ticking number here would be the
/// view's own arithmetic wearing the harness's voice, and it would keep counting for an
/// agent that had silently died. Reported as given, and it stops when the reports do —
/// which is itself the honest signal.
pub fn progress_summary(progress: &SubagentProgress) -> Option<ProgressSummary> {
    if progress.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(tool) = &progress.last_tool {
        // `→` U+2192 is in Hack and not in the proportional face — see `step_mark`, whose
        // measurement this borrows, and note the draw site below is `.monospace()` for it.
        parts.push(format!("→ {tool}"));
    }
    if let Some(n) = progress.tool_uses {
        parts.push(format!("{n} tool{}", if n == 1 { "" } else { "s" }));
    }
    if let Some(ms) = progress.duration_ms {
        parts.push(elapsed(ms));
    }
    if let Some(tokens) = progress.total_tokens {
        parts.push(format!("{} tokens", grouped(tokens)));
    }
    Some(ProgressSummary {
        headline: progress.description.clone(),
        facts: (!parts.is_empty()).then(|| parts.join(" · ")),
        status: progress.status.clone(),
    })
}

/// A duration a person can read, from the harness's own milliseconds.
///
/// Tenths below a minute because that is the range a tool step lives in and a whole second
/// there loses the difference between fast and instant; whole seconds above it, because
/// nobody reads a tenth off a sixteen-minute agent. **Truncating, never rounding up** —
/// the same rule [`crate::agent_map::ContextFill::percent`] argues: never report a figure
/// the work has not reached.
fn elapsed(ms: u64) -> String {
    if ms < 60_000 {
        format!("{}.{}s", ms / 1000, (ms % 1000) / 100)
    } else {
        let seconds = ms / 1000;
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

/// `61477` → `61,477`. Exact, because a token count is a measurement and `61.5k` would be
/// this view rounding one — cheap here, and the habit is what matters.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// **What the harness says the dispatched agent is doing**, drawn above its step log.
///
/// 🚨 Nothing here streams and nothing here ticks — [`progress_summary`] owns both
/// refusals and this function must not grow past what it returns.
fn progress_body(ui: &mut egui::Ui, progress: &SubagentProgress, theme: &Theme) {
    let Some(summary) = progress_summary(progress) else { return };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // `task`, not `subagent`: the row below is what the agent *did*, this is what the
        // harness says about the task as a whole. Two labels because they are two claims
        // with two different sources.
        ui.label(RichText::new("task").color(theme.asking).small().monospace());
        if let Some(headline) = &summary.headline {
            ui.label(RichText::new(headline).color(theme.prose).small());
        }
        if let Some(status) = &summary.status {
            ui.label(RichText::new(status).color(theme.dim).small());
        }
    });
    if let Some(facts) = &summary.facts {
        // Monospace for the `→`, per `step_mark`'s glyph measurement.
        ui.label(RichText::new(facts).color(theme.dim).small().monospace());
    }
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

/// One subagent step's mark, and the colour that goes with it.
///
/// 🚨 **`✓` and `✗` were TOFU here, and `.monospace()` was never going to fix them** —
/// which is why this is a character change rather than the font change the two earlier
/// tofu fixes in this file were. James's fan-out capture showed `□ Bash` where a returned
/// step belonged. Measured, by reading the `cmap` tables of all four fonts egui 0.33
/// bundles (`Hack-Regular`, `Ubuntu-Light`, `NotoEmoji-Regular`, `emoji-icon-font`):
/// **U+2713 `✓` and U+2717 `✗` are in none of them.** Asking for the mono face only
/// chooses *which* font is missing the glyph. The same read confirms the earlier fix was
/// right about its own case — `◈` U+25C8 and `●` U+25CF are in Hack and not in Ubuntu,
/// exactly as that note says — so the two rules are siblings, not rivals: **draw symbols
/// monospace, and only draw symbols Hack has.**
///
/// The three replacements are each measured present:
///
/// * `→` U+2192 — Hack only, unchanged, and the reason the `.monospace()` at the draw
///   site stays.
/// * `•` U+2022 — a returned step. In **both** faces, so this one cannot regress even if
///   a later edit drops the mono call. A bullet rather than a tick because there is no
///   tick in the fonts available; green is what says the return was clean.
/// * `×` U+00D7 — a step that returned an error. Also in both faces, and read as a
///   failure mark by everyone without needing to be a dingbat.
fn step_mark(state: &StepState, theme: &Theme) -> (&'static str, Color32) {
    match state {
        StepState::Running => ("→", theme.running),
        StepState::Done { is_error: false } => ("•", theme.ok),
        StepState::Done { is_error: true } => ("×", theme.bad),
    }
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
fn subagent_body(ui: &mut egui::Ui, log: &SubagentLog, running: bool, theme: &Theme) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // ⚠️ `·` is carried by the proportional face; the step marks below are drawn
        // monospace *and* chosen from what the mono face has — [`step_mark`] says which,
        // and why the pair of rules is not one rule. This comment used to claim `✓`/`✗`
        // were covered by that argument. They were not, and the band showed a box.
        ui.label(RichText::new("subagent").color(theme.asking).small().monospace());
        let summary = subagent_summary(log, running);
        ui.label(RichText::new(summary.counts).color(theme.dim).small());
        if let Some(open) = summary.open {
            let color = if running { theme.running } else { theme.dim };
            ui.label(RichText::new(open).color(color).small());
        }
    });
    if log.dropped > 0 {
        ui.label(
            RichText::new(format!("+{} earlier steps not kept", log.dropped))
                .color(theme.dim)
                .small(),
        );
    }
    let skip = log.len().saturating_sub(SUBAGENT_LINES);
    if skip > 0 {
        ui.label(RichText::new(format!("+{skip} earlier steps")).color(theme.dim).small());
    }
    for step in log.steps.iter().skip(skip) {
        let deeper = step.depth > 1;
        ui.horizontal(|ui| {
            if deeper {
                ui.label(RichText::new(format!("{}·", "  ".repeat(step.depth as usize - 1)))
                    .color(theme.dim)
                    .small()
                    .monospace());
            }
            match &step.act {
                SubagentAct::Said(text) => {
                    ui.label(RichText::new(one_line(text)).color(theme.prose).small());
                }
                SubagentAct::Tool { id, name, state } => {
                    let (mark, color) = step_mark(state, theme);
                    ui.label(RichText::new(mark).color(color).small().monospace());
                    // An unnamed step is a return whose call this log never saw — the same
                    // "(call not seen)" a card shows, for the same reason.
                    let label = name.as_deref().unwrap_or("(call not seen)");
                    ui.label(RichText::new(label).color(theme.prose).small().monospace());
                    if name.is_none() {
                        ui.label(RichText::new(id.as_str()).color(theme.dim).small().monospace());
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
    theme: &Theme,
    form: &Form,
) -> Option<CardAct> {
    let accent = match block.state {
        ApprovalState::Pending => theme.asking,
        ApprovalState::Answered(a) if a.verdict == Verdict::Allow => theme.ok,
        ApprovalState::Answered(_) => theme.bad,
        // Not `theme.bad`: nothing was refused. The card is spent, and reads that way.
        ApprovalState::Abandoned => theme.dim,
    };
    ui.push_id(id.0, |ui| {
        let mut framed = Frame::new()
            .fill(theme.panel_fill)
            .corner_radius(form.card_corner())
            .inner_margin(form.card_margin());
        if let Some(stroke) = form.card_stroke(accent) {
            framed = framed.stroke(stroke);
        }
        let framed = framed
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("◈ may I").color(accent).strong().monospace());
                    ui.label(
                        RichText::new(capability_label(&block.tool_name))
                            .color(theme.prose)
                            .strong()
                            .monospace(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(&block.tool_use_id).color(theme.dim).small().monospace(),
                        );
                    });
                });
                arguments_body(
                    ui,
                    &Arguments { text: block.input.clone(), complete: true },
                    theme,
                );
                ui.add_space(4.0);
                match block.state {
                    ApprovalState::Pending => approval_buttons(ui, live, theme),
                    ApprovalState::Answered(answer) => approval_verdict(ui, answer, theme),
                    // No buttons and no verdict — the outcome, and nothing that looks
                    // like it could still change it.
                    ApprovalState::Abandoned => {
                        ui.label(
                            RichText::new(
                                "the agent stopped waiting — this call failed before it was \
                                 answered",
                            )
                            .color(theme.dim)
                            .small(),
                        );
                        None
                    }
                }
            });
        card_left_rule(ui, framed.response.rect, theme, form);
        framed.inner
    })
    .inner
}

/// The four buttons, widening left to right — and the widest one is marked.
///
/// 🚨 **"allow everything" is amber, not green.** It is the only button here that decides
/// more than the call in front of it, and the colour is the one thing a hand reads before
/// the word. [`Theme::mode_alert`] is reused rather than a fourth colour chosen: it is already what
/// the band uses for *"the console may not be the one being asked"*, which is precisely what
/// this button creates. The hover states the whole consequence, including where to revoke it
/// — that revoke is on the band, not here, because this card will scroll away.
fn approval_buttons(ui: &mut egui::Ui, live: bool, theme: &Theme) -> Option<CardAct> {
    let mut act = None;
    ui.horizontal_wrapped(|ui| {
        if !live {
            // The question is gone — evicted, or already answered elsewhere — so the
            // buttons would send a verdict nothing is waiting for. Say so instead.
            ui.label(
                RichText::new("this question is no longer answerable").color(theme.dim).small(),
            );
            return;
        }
        if ui.button(RichText::new("allow").color(theme.ok).monospace()).clicked() {
            act = Some(CardAct::Choose(Choice::Allow));
        }
        if ui.button(RichText::new("allow & remember").color(theme.ok).monospace()).clicked() {
            act = Some(CardAct::Choose(Choice::AllowAndRemember));
        }
        if ui
            .button(
                RichText::new("allow everything this session")
                    .color(theme.mode_alert)
                    .monospace(),
            )
            .on_hover_text(SESSION_ALLOW_CONSEQUENCE)
            .clicked()
        {
            act = Some(CardAct::Choose(Choice::AllowEverythingThisSession));
        }
        if ui.button(RichText::new("deny").color(theme.bad).monospace()).clicked() {
            act = Some(CardAct::Choose(Choice::Deny));
        }
    });
    act
}

fn approval_verdict(ui: &mut egui::Ui, answer: Answer, theme: &Theme) -> Option<CardAct> {
    let (word, color) = match answer.verdict {
        Verdict::Allow => ("allowed", theme.ok),
        Verdict::Deny => ("denied", theme.bad),
    };
    let mut act = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(word).color(color).small().strong());
        // ⚠️ **The two standing sources say different things, because they are undone in
        // different places.** "a decision you already made" sends a reader to this card's
        // own `forget`; a session-wide allow has no per-call entry to forget and is revoked
        // from the band, so a card that said the same sentence would send them looking for
        // a button that is not there.
        match answer.by {
            AnsweredBy::Click => {}
            AnsweredBy::ThisCall => {
                ui.label(
                    RichText::new("· from a decision you already made").color(theme.dim).small(),
                );
            }
            AnsweredBy::SessionAllow => {
                ui.label(
                    RichText::new("· you allowed everything this session — revoke on the band")
                        .color(theme.mode_alert)
                        .small(),
                );
            }
        }
        if answer.remembered {
            ui.label(
                RichText::new("· the console will answer this the same way again")
                    .color(theme.dim)
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
#[allow(clippy::too_many_arguments)]
fn panel_element(
    ui: &mut egui::Ui,
    id: ElementId,
    artifact: &ArtifactBlock,
    spec: &PanelSpec,
    state: &mut PanelState,
    defaults: &[(String, f32)],
    theme: &Theme,
    form: &Form,
) {
    // Scoped by the element's own id: two panels in one transcript are two sets of widgets,
    // and egui's positional auto-ids would otherwise hand a slider its neighbour's drag
    // state the moment anything above them changes height.
    ui.push_id(id.0, |ui| {
        let mut framed = Frame::new()
            .fill(theme.panel_fill)
            .corner_radius(form.card_corner())
            .inner_margin(form.card_margin());
        if let Some(stroke) = form.card_stroke(theme.panel_edge) {
            framed = framed.stroke(stroke);
        }
        let framed = framed.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().slider_width = SLIDER_WIDTH;
            ui.label(RichText::new(&artifact.title).monospace().strong().color(theme.panel_title));
            panel_body(ui, spec, state, defaults, theme);
        });
        card_left_rule(ui, framed.response.rect, theme, form);
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
    theme: &Theme,
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
            let text = if chosen { text.color(theme.panel_title).strong() } else { text };
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
    theme: &Theme,
    form: &Form,
) -> egui::Rect {
    ui.push_id(id.0, |ui| {
        let mut framed = Frame::new()
            .fill(theme.panel_fill)
            .corner_radius(form.card_corner())
            .inner_margin(form.card_margin());
        if let Some(stroke) = form.card_stroke(theme.panel_edge) {
            framed = framed.stroke(stroke);
        }
        let framed = framed
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(&artifact.title).monospace().strong().color(theme.panel_title),
                );
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
                            // The identity multiplier, not a colour — a theme tinting the
                            // engine's own render would be overpainting the answer.
                            Color32::WHITE,
                        );
                    }
                    None => {
                        // The one block nested *inside* a card today, and the only reader of
                        // `nested_radius`.
                        ui.painter().rect_filled(rect, form.nested_corner(), theme.surface_empty);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "rendering…",
                            egui::FontId::monospace(12.0),
                            theme.dim,
                        );
                    }
                }
                rect
            });
        card_left_rule(ui, framed.response.rect, theme, form);
        framed.inner
    })
    .inner
}

/// How tall one picture in an exhibit is allowed to be, in points.
///
/// The same figure as [`SURFACE_HEIGHT`] and for the same reason: a card in a flowing
/// transcript has to leave the flow readable, so a picture gets a band rather than its natural
/// size. The *width* is the card's and the aspect ratio is honoured inside it — see
/// [`fit_within`], which is what stops a wide screenshot being stretched to a square.
const EXHIBIT_HEIGHT: f32 = SURFACE_HEIGHT;

/// The largest Markdown document drawn in full, in bytes.
///
/// 🚨 **A bound on what is *drawn*, not on what is read.** §1.7's measurement is the reason:
/// the transcript is not virtualised, so every element's galley is laid out on every frame
/// whether or not it is on screen, and layout is linear in text length. A 2 MB README dropped
/// into a conversation would therefore cost its full layout on every frame for the rest of the
/// session — not once. 64 KB is a long document and roughly the point at which one element
/// starts to dominate a frame.
///
/// ⚠️ **Truncation is stated in the card**, never silent, on `text_diff`'s rule: a document
/// that quietly stops half way is indistinguishable from a document that ends there.
const MAX_DOCUMENT_BYTES: usize = 64 * 1024;

/// The rect a picture of `size` fills inside `bounds`, centred, preserving aspect ratio.
///
/// ⚠️ **Never `bounds` itself.** Painting a texture into the whole band stretches it, and a
/// stretched screenshot is a subtly wrong picture rather than an obviously missing one — the
/// failure mode that survives review because it looks fine until you know the source.
fn fit_within(bounds: egui::Rect, size: (u32, u32)) -> egui::Rect {
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let scale = (bounds.width() / w).min(bounds.height() / h);
    let fitted = egui::vec2(w * scale, h * scale);
    egui::Rect::from_center_size(bounds.center(), fitted)
}

/// One exhibit, drawn as a card: a title, then every item with its label.
///
/// Returns the rect each item was given, in transcript order, so the caller can build the
/// [`ExhibitRequest`]s — the same shape [`surface_element`] uses, and for the same reason: the
/// size is only known once egui has laid the card out, and the picture arrives next frame.
#[allow(clippy::too_many_arguments)]
fn exhibit_element(
    ui: &mut egui::Ui,
    id: ElementId,
    artifact: &ArtifactBlock,
    spec: &ExhibitSpec,
    picture: bool,
    contents: &ExhibitContents,
    theme: &Theme,
    form: &Form,
) -> Vec<egui::Rect> {
    ui.push_id(id.0, |ui| {
        let mut framed = Frame::new()
            .fill(theme.panel_fill)
            .corner_radius(form.card_corner())
            .inner_margin(form.card_margin());
        if let Some(stroke) = form.card_stroke(theme.panel_edge) {
            framed = framed.stroke(stroke);
        }
        let framed = framed.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(&artifact.title).monospace().strong().color(theme.panel_title));
            // The count is stated only when there is more than one, so the common case reads as
            // a picture rather than as a gallery of one.
            if spec.items.len() > 1 {
                ui.label(
                    RichText::new(format!("{} items", spec.items.len()))
                        .monospace()
                        .small()
                        .color(theme.dim),
                );
            }
            ui.add_space(4.0);
            let mut rects = Vec::with_capacity(spec.items.len());
            for (i, item) in spec.items.iter().enumerate() {
                if i > 0 {
                    ui.add_space(6.0);
                }
                ui.label(RichText::new(&item.label).monospace().small().color(theme.dim));
                let state = contents.get(&(id, i));
                if picture {
                    rects.push(picture_item(ui, state, theme, form));
                } else {
                    document_item(ui, state, theme);
                    // A document has no texture and so no rect to report — but the vector is
                    // index-aligned with the items by contract, so it gets a degenerate one
                    // rather than a hole. `ZERO` is what the caller tests to skip it.
                    rects.push(egui::Rect::ZERO);
                }
            }
            rects
        });
        card_left_rule(ui, framed.response.rect, theme, form);
        framed.inner
    })
    .inner
}

/// One picture, in whichever of its four states it is in. Returns the band it was allotted —
/// the size the console is being asked to decode into.
fn picture_item(
    ui: &mut egui::Ui,
    state: Option<&ExhibitContent>,
    theme: &Theme,
    form: &Form,
) -> egui::Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, EXHIBIT_HEIGHT), egui::Sense::hover());
    match state {
        Some(ExhibitContent::Picture { texture, size }) => {
            ui.painter().rect_filled(rect, form.nested_corner(), theme.surface_empty);
            ui.painter().image(
                *texture,
                fit_within(rect, *size),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                // The identity multiplier — a theme tinting a person's own photograph would be
                // repainting the thing they asked to look at.
                Color32::WHITE,
            );
        }
        Some(ExhibitContent::Failed(why)) => failed_plate(ui, rect, why, theme, form),
        // A document's content in a picture's slot cannot happen through `place`, but it is a
        // `HashMap` lookup rather than a proof — so it says so instead of drawing nothing.
        Some(ExhibitContent::Document(_)) => {
            failed_plate(ui, rect, "not a picture", theme, form);
        }
        None => {
            ui.painter().rect_filled(rect, form.nested_corner(), theme.surface_empty);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "reading...",
                egui::FontId::monospace(12.0),
                theme.dim,
            );
        }
    }
    rect
}

/// The plate a picture that will never arrive shows.
///
/// 🚨 **Deliberately unlike the "reading..." plate** — a different word, and the failure text
/// under it. The two states are one frame apart and permanent respectively, and the whole point
/// of `ExhibitContent::Failed` is lost if they look the same.
fn failed_plate(ui: &mut egui::Ui, rect: egui::Rect, why: &str, theme: &Theme, form: &Form) {
    ui.painter().rect_filled(rect, form.nested_corner(), theme.surface_empty);
    ui.painter().text(
        rect.center() - egui::vec2(0.0, 8.0),
        egui::Align2::CENTER_CENTER,
        "cannot show this file",
        egui::FontId::monospace(12.0),
        theme.bad,
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 8.0),
        egui::Align2::CENTER_CENTER,
        why,
        egui::FontId::monospace(11.0),
        theme.dim,
    );
}

/// One document, in whichever of its three states it is in.
fn document_item(ui: &mut egui::Ui, state: Option<&ExhibitContent>, theme: &Theme) {
    match state {
        Some(ExhibitContent::Document(text)) => markdown_body(ui, text, theme),
        Some(ExhibitContent::Failed(why)) => {
            ui.label(RichText::new("cannot show this file").monospace().color(theme.bad));
            ui.label(RichText::new(why).monospace().small().color(theme.dim));
        }
        Some(ExhibitContent::Picture { .. }) => {
            ui.label(RichText::new("cannot show this file").monospace().color(theme.bad));
            ui.label(RichText::new("not a document").monospace().small().color(theme.dim));
        }
        None => {
            ui.label(RichText::new("reading...").monospace().italics().color(theme.dim));
        }
    }
}

/// Markdown, drawn as text — headings, bullets, fenced code and paragraphs.
///
/// 🚨 **A subset, on purpose, and with no dependency.** `organon-console`'s manifest is
/// deliberately spare and every entry is argued (`doc/arch/topology.md`); a Markdown crate
/// would be a parser, an AST and an HTML model pulled in to make four kinds of line look
/// different in a card. What is *not* rendered — tables, links as links, images, nested
/// emphasis — is shown as its own source text, which is readable and honest, rather than
/// silently swallowed.
///
/// ⚠️ **Every line is drawn `.monospace()`**, which is the tofu fix this file applies
/// everywhere: a document from disk can hold any codepoint, and Hack is the widest of the four
/// bundled faces. A character none of them has is still a box — that is a property of egui's
/// bundled fonts, not something this function can fix, and it is why the *labels* the console
/// derives are ASCII-folded upstream (`organon_core::exhibit::Item::new`) while a document's
/// own body is passed through as written.
fn markdown_body(ui: &mut egui::Ui, text: &str, theme: &Theme) {
    let (shown, truncated) = match text.char_indices().nth(MAX_DOCUMENT_BYTES) {
        Some((at, _)) => (&text[..at], true),
        None => (text, false),
    };
    let mut in_code = false;
    for line in shown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            ui.label(RichText::new(line).monospace().small().color(theme.prose));
            continue;
        }
        let heading = trimmed.chars().take_while(|c| *c == '#').count();
        if heading > 0 && heading <= 6 && trimmed.chars().nth(heading) == Some(' ') {
            let body = trimmed[heading + 1..].trim();
            // Two sizes, not six: past the second, a deeper heading in a card this size reads
            // as body text with extra weight, and the weight is what carries the level.
            let size = if heading == 1 { 15.0 } else { 13.0 };
            ui.add_space(4.0);
            ui.label(
                RichText::new(body).monospace().strong().size(size).color(theme.panel_title),
            );
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            // An ASCII hyphen, not a bullet character. `•` U+2022 is in the allowlist, but the
            // body of a document is already monospace and a hyphen is what its source says.
            ui.label(RichText::new(format!("  - {rest}")).monospace().color(theme.prose));
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        ui.label(RichText::new(line).monospace().color(theme.prose));
    }
    if truncated {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "-- shown to {} KB; the file continues --",
                MAX_DOCUMENT_BYTES / 1024
            ))
            .monospace()
            .small()
            .italics()
            .color(theme.dim),
        );
    }
}

fn arguments_body(ui: &mut egui::Ui, args: &Arguments, theme: &Theme) {
    for (key, value) in argument_fields(args) {
        ui.horizontal_wrapped(|ui| {
            if !key.is_empty() {
                ui.label(RichText::new(format!("{key}:")).color(theme.dim).small().monospace());
            }
            ui.label(RichText::new(value).color(theme.prose).small().monospace());
        });
    }
}

/// The aligned diff, row by row.
///
/// ⚠️ **Every row is drawn `.monospace()`, the elisions included.** The prefixes are what
/// makes a diff scannable and they only line up in a fixed-pitch face — and a dim
/// proportional summary row between two mono ones reads as a different kind of thing
/// rather than as part of the same column.
fn diff_body(ui: &mut egui::Ui, diff: &EditDiff, theme: &Theme) {
    if !diff.path.is_empty() {
        ui.label(RichText::new(&diff.path).color(theme.dim).small().monospace());
    }
    for note in diff_notes(&diff.diff) {
        ui.label(RichText::new(note).color(theme.dim).small());
    }
    for row in &diff.diff.rows {
        let (text, color) = match row {
            DiffRow::Context(line) => (format!("  {line}"), theme.dim),
            DiffRow::Removed(line) => (format!("- {line}"), theme.bad),
            DiffRow::Added(line) => (format!("+ {line}"), theme.ok),
            DiffRow::Elided(n) => (format!("  … {n} unchanged lines"), theme.dim),
            DiffRow::Held(n) => (format!("  … {n} more lines"), theme.dim),
        };
        ui.label(RichText::new(text).color(color).small().monospace());
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

// The band's plates ([`Theme::strip_fill`] and its edge, the model plate), the model's two
// type colours, the permission plate's two voices and the context ring's four are all
// [`Theme`]'s. The argument for each — why the mode marker is amber and not [`Theme::bad`],
// why an unmeasured ring is a different grey from an empty one, why the arc is blue — is
// written beside the field it belongs to.
const CONTEXT_RING_STROKE: f32 = 2.0;

/// What joins two chips on the band's dim right-hand half.
///
/// ⚠️ **A constant because it is now measured as well as drawn.** [`strip_right_reserve`] lays
/// the chip run out to find how wide it is, and a separator spelled twice would make the
/// measurement and the drawing able to disagree by exactly the width of a separator per gap —
/// which is precisely how much overlap it takes to put one segment under the next.
const CHIP_SEP: &str = " · ";

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

/// The context half of the band: the little ring at the far right.
///
/// 🚨 **The TRACK is chrome and the FILL is the measurement, and separating those two is
/// what lets the ring be present from the first frame without claiming anything.** This
/// reverses an earlier decision, so the reversal is stated rather than quietly applied:
/// `Unknown` used to draw *nothing at all*, on the grounds that an empty ring reads as
/// "0 % full" — a confident, specific, false number. That reasoning was right about the
/// arc and wrong about the circle. A ring drawn with no arc in it is not a reading of
/// zero; it is the container the reading will appear in, exactly as an empty gauge face
/// is not a needle pointing at nought. What outweighed the original call is a cost it did
/// not price: the whole dim half — cost, ring, chips — materialised at the first turn's
/// `result`, so the band a hand had been looking at for a minute *rearranged itself* the
/// moment the session became interesting. Stable chrome is worth more than the point, and
/// the point survives anyway, because the arc still refuses.
///
/// 🚨 **An unmeasured ring must not be mistakable for a measured 0 %**, and that is a real
/// case rather than a hypothetical: a `message_start` reporting a zero prompt against a
/// known window builds a `Known` fill whose `fraction()` is `0.0`, which draws no arc
/// either. Two states, one picture, is precisely the false claim the original design
/// feared — so the *track itself* carries the difference: [`ring_track_color`] draws the
/// unmeasured circle at [`Theme::context_track_empty`], visibly fainter than the
/// [`Theme::context_track`] a measured reading sits on, and [`ring_hover_rows`] answers "not
/// measured yet" where the other answers "0 % at the last request".
///
/// ⚠️ A session's first turn still has no *arc*, and the arc appears at that turn's
/// `result`. From then on it moves **per API round trip**, not per turn — a `message_start`
/// updates it mid-turn, which is the visible consequence of the numerator being what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextSlot {
    /// One or both halves have not been measured yet. The track is drawn; no arc is.
    Unknown,
    /// Both halves measured. See [`ContextFill`] for exactly what they are.
    Known(ContextFill),
}

impl ContextSlot {
    /// Whether the reading has crossed [`CONTEXT_HIGH_PERCENT`].
    ///
    /// 🚨 **Derived from [`ContextFill::percent`] — the same number the hover prints — and
    /// that identity is the correctness, not a convenience.** This test used to do the
    /// threshold arithmetic a second time, in its own integer form, and two arithmetics
    /// for one decision is exactly how they came to disagree: at `7 495 / 10 000` the
    /// rounding `percent()` said **75** while this comparison said `749 500 < 750 000` and
    /// stayed **false**, so the hover read "75 % at the last request" beside a ring that
    /// was still blue. Reading the displayed number makes that contradiction
    /// *unrepresentable* rather than merely absent at today's inputs — the colour is a
    /// statement about the printed figure, so it must be computed from it.
    ///
    /// The threshold is compared in whole percent because that is the resolution the
    /// reader is given: a ring whose colour turned on a difference the hover cannot
    /// express would be unanswerable from the interface.
    pub fn is_high(&self) -> bool {
        match self {
            ContextSlot::Unknown => false,
            ContextSlot::Known(fill) => fill.percent() >= CONTEXT_HIGH_PERCENT,
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
    ///
    /// ✏️ **This is now the plate's HOVER rather than the band's text.** See [`Self::short`].
    pub text: String,
    /// The same fact in **two words**, which is what the band draws.
    ///
    /// 🚨 **The persistent-warning invariant is kept; the verbosity is not.** James, 2026-08-21:
    /// *"we don't want to show words like `default` and `allow all` at all times. That would be a
    /// sort of verbose form of the interface. We should have either icons or some other way of
    /// not having to show all those characters."* The resting state — `default` — is now a single
    /// dim [`mode_glyph`] and no words at all, which is the whole of what he asked for. An
    /// abnormal mode still carries **words**, because "the console may not be the one being
    /// asked" is the one thing on this band that a colour alone must not be trusted to say, and
    /// this section's note argues at length that the warning must be standing rather than
    /// dismissible. Two words is the compromise: legible without being a sentence, and the
    /// sentence is one hover away in [`Self::text`].
    pub short: String,
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

// ---------------------------------------------------------------------------
// The console's own standing allow
// ---------------------------------------------------------------------------
//
// 🚨 **This is the console's own memory widened, and it must never be confused with a
// permission mode — in the code or on the screen.** The two above it are upstream:
// `bypassPermissions` is unreachable (the CLI refuses it without a launch flag the console
// does not pass) and `dontAsk` **refuses** rather than allows. This one is ours: the handler
// still runs, the card is still drawn, the transcript still records every call — the console
// simply answers *yes* on the human's behalf.
//
// The band therefore has to distinguish **two different facts with two different remedies**:
//
// * *a mode is silencing approvals* — [`ModeSlot::marker`], fixed by changing the mode; and
// * *you allowed everything* — [`SessionAllowSlot`], fixed by revoking it, which is what
//   clicking the plate does.
//
// Both can be true at once, and the band says both rather than picking one. The marker is
// **derived in [`strip_content`] from the memory's own flag**, exactly as the mode's is
// derived from the reported mode: true for as long as the condition holds, and impossible to
// dismiss or to leave stuck. A console that has stopped asking while still looking like the
// authority is precisely what the mode marker exists to prevent, and a grant the human made
// themselves earns no exemption from that.
//
// ⚠️ [`Theme::mode_alert`]'s amber and not red, for the reason already argued for the mode
// marker: this band is looked at for hours, and a permanent klaxon trains the eye to skip it.

/// **The plate's mark.** `×` for the same reason [`mode_glyph`] uses it: the console is not
/// asking. The two facts are still told apart — this plate wears [`Theme::mode_alert`] and its
/// own two words, and it is the only one of the pair that is clickable to *revoke* — but they
/// answer the same question and the eye should not have to learn two symbols for it.
///
/// ✏️ **This replaced the words `allow all`, which James named as one of the two offenders.**
pub const SESSION_ALLOW_LABEL: &str = MODE_GLYPH_SILENT;

/// The standing marker's **two words**, which is what the band draws beside the mark.
///
/// ✏️ **The sentence moved to the hover.** It was `you allowed everything — the console is not
/// asking` — 48 characters standing on a one-line band, and the segment James photographed being
/// painted over. [`SESSION_ALLOW_MARKER`] is still that sentence and is still what the plate's
/// hover and the revoke target carry; the band gets the short form.
pub const SESSION_ALLOW_SHORT: &str = "allowing all";

/// The standing marker's sentence — **what is happening** — on the plate's hover.
pub const SESSION_ALLOW_MARKER: &str = "you allowed everything — the console is not asking";

/// The whole consequence, for the button's hover and the plate's.
pub const SESSION_ALLOW_CONSEQUENCE: &str =
    "every tool this agent calls is allowed without asking, until you revoke it or close \
     this tab. Nothing is written to disk. The band carries a marker for as long as it is \
     on, and clicking that marker revokes it. Decisions you denied and remembered still \
     apply.";

/// The band's half of the standing allow: present exactly while it is on.
///
/// A struct of one field rather than a bare `bool` so the plate draws from the same shape
/// the mode's does, and so the sentence has one home.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionAllowSlot {
    /// The whole sentence, for the hover. See [`SESSION_ALLOW_MARKER`].
    pub marker: &'static str,
    /// The two words the band draws. See [`SESSION_ALLOW_SHORT`].
    pub short: &'static str,
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
            short: "auto-edits".to_string(),
            severity: ModeSeverity::Note,
        }),
        "dontAsk" => Some(ModeMarker {
            text: "you are not being asked — anything needing permission is refused".to_string(),
            short: "not asking".to_string(),
            severity: ModeSeverity::Alert,
        }),
        _ => Some(ModeMarker {
            text: "not the console's default — approvals may not reach you".to_string(),
            short: "non-default".to_string(),
            severity: ModeSeverity::Note,
        }),
    }
}

/// **The icon the permission plate draws instead of the mode's name.**
///
/// 🚨 **`◈` = you are being asked; `×` = you are not.** That is the only distinction this band
/// has ever needed to carry at a glance, and it is the one the mode's *name* was never carrying:
/// `dontAsk` and `acceptEdits` tell a reader nothing about which of the two they are in. `◈` is
/// already the console's approval mark — it is what `status_reading` puts in front of
/// *"permission requests — waiting on you"* and what the approval card's own `◈ may I` uses — so
/// the plate is not teaching a new symbol, it is repeating one.
///
/// ⚠️ **Both are on the glyph allowlist and both are drawn `.monospace()`** — `×` U+00D7 is in
/// Hack *and* Ubuntu-Light, `◈` U+25C8 in Hack alone. See
/// [`tests::no_symbol_the_console_draws_is_a_glyph_egui_lacks`], which walks these.
///
/// ⚠️ **An unrecognised mode gets `◈`, not `×`.** The console does not know that it has stopped
/// being asked, and a mark that asserts it would be a guess; the colour and
/// [`ModeMarker::short`] carry "this is not the default", which is what is actually known.
pub fn mode_glyph(mode: &str) -> &'static str {
    match mode.trim() {
        "dontAsk" => MODE_GLYPH_SILENT,
        _ => MODE_GLYPH_ASKS,
    }
}

/// The plate's mark when approvals still reach the human. See [`mode_glyph`].
pub const MODE_GLYPH_ASKS: &str = "◈";
/// The plate's mark when they do not. See [`mode_glyph`].
pub const MODE_GLYPH_SILENT: &str = "×";

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
    /// **Whether this reading is the harness narrating rather than a live condition.**
    ///
    /// 🚨 **The band's own half of [`Remark::seen`], and it has to be decided where the
    /// reading is built.** Two of the seven readings are echoes of what the agent last said
    /// about itself — `needs_action`, which is its own sentence, and `last_status_detail`,
    /// which is its summary of a turn already on the page above. Both are `Asking`/`Ready`,
    /// the same standings a pending approval and an idle session produce, so nothing
    /// downstream could tell them apart from the standing alone; a caller re-deriving it from
    /// the *text* would be a second rule to keep in step. See [`Self::seen_text`].
    pub narration: bool,
}

impl StatusReading {
    /// The text a person sees, given the mode. Empty means the band draws nothing here.
    ///
    /// ⚠️ **The field is left whole rather than blanked at construction**, so the reading a
    /// test pins and the reading `/trace on` shows are the same value — the mode is a
    /// question asked at the drawing, not a fact about what was read.
    pub fn seen_text(&self, tracing: bool) -> &str {
        if self.narration && !tracing {
            return "";
        }
        &self.text
    }
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
    /// **Present exactly while the console's own session-wide allow is active** — a
    /// different fact from [`mode`](Self::mode) with a different remedy, so it is a
    /// different slot rather than a second value in that one. See the section above
    /// [`SessionAllowSlot`].
    pub session_allow: Option<SessionAllowSlot>,
    /// A model change that has been asked for and not confirmed — the row's label, not a
    /// model id. Drawn *beside* [`model`](Self::model), never in place of it: see
    /// [`PendingModel`].
    pub pending_model: Option<String>,
    /// `(label, value)` rows for the model plate's hover — the identity that does not fit,
    /// and must not be allowed to try.
    pub identity: Vec<(String, String)>,
    pub reading: StatusReading,
    /// How full the model's context was at the last request — the ring at the far right,
    /// or [`ContextSlot::Unknown`], which draws the ring's track and no arc.
    pub context: ContextSlot,
    /// Dim, right-aligned, joined with `·`. Bounded by construction: at least one — the
    /// session's cost is on the band from the first frame — and at most three.
    ///
    /// ⚠️ **Each carries whether it is narration**, exactly as [`Remark`] and
    /// [`StatusReading`] do. Read them through [`StripContent::chips_seen`] rather than
    /// filtering here: the marker is set where the chip is built, and a second rule written
    /// at a draw site is the drift this band already spends its comments preventing.
    pub chips: Vec<Chip>,
}

/// One dim right-hand chip, and whether a person sees it without asking.
///
/// 🚨 **The quiet/loud rule, third instance.** [`Remark`] carries it for the console's own log
/// lines and [`StatusReading::narration`] for the band's standing; this is the same decision
/// for the band's numbers. The session's spend and the last turn's duration are **harness
/// telemetry** — true, measured, and not what James is looking at the band for; a tally of
/// permission decisions *he* made is not, because it is the console reporting its own
/// authority and nothing else on screen says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub text: String,
    /// False = seen whatever the mode. True = only under `/trace on`.
    pub narration: bool,
}

impl StripContent {
    /// The chips a person sees, given the mode, in the order the band draws them.
    ///
    /// ⚠️ **The vector is left whole rather than pruned at construction**, on
    /// [`StatusReading::seen_text`]'s rule: what the band *knows* does not change with the
    /// mode, only what it draws, and a test that pins the arithmetic should not have to open
    /// a pane to do it.
    pub fn chips_seen(&self, tracing: bool) -> Vec<&str> {
        self.chips
            .iter()
            .filter(|chip| tracing || !chip.narration)
            .map(|chip| chip.text.as_str())
            .collect()
    }

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
    /// Whether the console is allowing everything for the rest of this session
    /// ([`crate::approval::DecisionMemory::session_allow`]).
    ///
    /// A live reading rather than a [`SessionFacts`] field, on this struct's own rule: it is
    /// the console's own state, it is true or false right now, and it goes away with the
    /// tab. Nothing on the wire ever states it — no upstream mode means this.
    pub session_allow: bool,
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
    // Two constructors, and which one a branch reaches for is the decision — see
    // [`StatusReading::narration`]. `say` is a live condition; `echo` is the agent's own
    // account of itself, which the transcript directly above the band already carries.
    let say = |standing, text: String| StatusReading { standing, text, narration: false };
    let echo = |standing, text: String| StatusReading { standing, text, narration: true };
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
        // ✏️ **An echo, so it is off the band unless the pane is tracing.** James struck
        // `◈ What are we working on?` out of the live build: it is the agent's closing line,
        // and it is already the last thing in the transcript a few pixels above. The reading
        // is still *taken* — `/trace on` shows it, and it still colours the standing — but
        // the band no longer repeats the page.
        return echo(Standing::Asking, format!("◈ {action}"));
    }
    if !live.has_session {
        // Empty on purpose: the model plate already reads "no model yet", and a band that
        // says "connecting…" twice reads as a bug rather than as one state.
        return say(Standing::Connecting, String::new());
    }
    // Both arms are narration on the same argument as `needs_action` above: the detail is the
    // harness's own summary of a turn that is already on the page, and a bare "ready" is a
    // console with nothing to report saying so. A live composer is what "ready" looks like.
    match &facts.last_status_detail {
        Some(detail) => echo(Standing::Ready, detail.clone()),
        None => echo(Standing::Ready, "ready".to_string()),
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
) -> StripContent {
    let model = match (&facts.model, failure.is_some()) {
        (Some(raw), _) => ModelSlot::Named(model_label(raw)),
        (None, false) => ModelSlot::Connecting,
        (None, true) => ModelSlot::Absent,
    };
    // Three at most, in reading order. Each is either a measurement or absent — there is no
    // arm here that computes one number out of two.
    let mut chips: Vec<Chip> = Vec::new();
    // 🚨 **Always, and zero before the first `result` is a measurement rather than a
    // placeholder.** Nothing has been spent, so nought is simply what the session has
    // cost — there is none of the honesty tension the ring's arc has, because this is a
    // total and not a proportion of an unknown. Showing it from the first frame is what
    // gives the dim half something to *be* at a cold start, which is the whole of why the
    // band no longer rearranges itself at the end of turn one. `unwrap_or(0.0)` rather
    // than a separate cold-start string so the figure keeps [`cost_label`]'s shape for the
    // life of the session: `$0.0000` becomes `$0.0123` in place, and never reformats.
    //
    // "session" is not decoration: `cost_usd` accumulates on the wire and the sibling
    // token counts do not, so the one number on the band says which kind it is.
    //
    // ✏️ **Narration, so it is off the default band and lives under `/trace on`.** Everything
    // above is still true about the *number*; what changed is who it is for. James, striking
    // it out on the live build alongside the approvals audit: his model is Claude Desktop,
    // which shows you which model you are talking to and not what the last turn cost. The
    // reading is still taken, still bounded, still `$0.0000` from the first frame — it simply
    // is not what the band is for. ⚠️ **The cold-start argument above therefore now applies to
    // the traced band**, which is the only place the reformat could ever be seen.
    chips.push(Chip {
        text: format!("session {}", cost_label(facts.cost_usd.unwrap_or(0.0))),
        narration: true,
    });
    // ⚠️ **This one stays conditional, and the asymmetry with the cost above is the
    // decision.** "0 remembered decisions" is *true*, but it is a tally of things the
    // human did rather than a meter that runs on its own, and there is nothing to watch
    // until the first one exists. It is also the one chip whose arrival the reader
    // themself caused — they answered a permission card and asked for it to be
    // remembered — so it is not something that happens *to* the band. Band height is
    // unaffected either way; [`STRIP_CHROME`] reserves one row of text regardless.
    //
    // 🚨 **And it is the one chip that is NOT narration.** It reports how far the console has
    // delegated its own authority — the same class of fact as the standing-allow marker and
    // the mode marker beside it — and nothing else on screen states it. A band that hid this
    // while hiding the spend would be quiet about the wrong one of the two.
    if live.remembered > 0 {
        let n = live.remembered;
        let plural = if n == 1 { "decision" } else { "decisions" };
        chips.push(Chip { text: format!("{n} remembered {plural}"), narration: false });
    }
    // ⚠️ **The one right-hand element with no honest cold-start form, so it is omitted.**
    // There is no last turn before the first turn, and `last turn 0.0s` would be a
    // *duration* asserted about an event that did not happen — not a zero total like the
    // cost and not an empty container like the ring's track, but a fabricated measurement.
    // Neither of the two escape hatches works: `last turn —` is a chip whose whole content
    // is an apology, and the ring's track/fill split has no analogue in a string. So it
    // arrives at the first `result`, alongside the ring's first arc, and the band's
    // *height* does not move when it does — which is the property James asked for and the
    // one `the_strip_is_one_band_and_leaves_the_scrollback_the_rest` pins.
    //
    // ✏️ **Narration too, for the cost chip's reason**: how long the harness took is the
    // harness's own account of itself.
    if let Some(ms) = facts.last_turn_duration_ms {
        chips.push(Chip { text: format!("last turn {}", duration_label(ms)), narration: true });
    }
    // The marker is derived, never remembered: it is true exactly while the reported mode
    // is non-default, which is the property the persistent-warning decision asked for.
    let mode = ModeSlot {
        mode: facts.permission_mode.clone(),
        marker: facts.permission_mode.as_deref().and_then(mode_marker),
    };
    // Derived on the same rule and for a sharper version of the same reason: the console
    // granting itself the authority to stop asking must be visible for exactly as long as
    // that is true, and neither stick nor be dismissible.
    let session_allow = live.session_allow.then_some(SessionAllowSlot {
        marker: SESSION_ALLOW_MARKER,
        short: SESSION_ALLOW_SHORT,
    });
    // Two measurements or nothing. `context_fill` refuses when either half is missing,
    // and there is no arm here that supplies one — see [`ContextSlot`].
    let context = match facts.context_fill() {
        Some(fill) => ContextSlot::Known(fill),
        None => ContextSlot::Unknown,
    };
    StripContent {
        model,
        mode,
        session_allow,
        pending_model: None,
        identity: identity_rows(facts, session),
        reading: status_reading(failure, live, facts),
        context,
        chips,
    }
}

/// **How much of the left half the two permission marks need**, before the model plate takes any.
///
/// 🚨 **The third instance of the same rule, one level down**, and it is needed for the same
/// reason the other two were: the marks are unconditional — they are the standing statement about
/// whether the console is still the authority — so at a width where *something* must give, what
/// gives is the identity beside them, by truncating. Without this the marks simply drew past the
/// end of the left group and under the chips, which is the overlap in a new place.
///
/// The arithmetic mirrors what [`mode_plate`] and [`session_allow_plate`] actually build: one
/// mono glyph inside `MODEL_PAD_X`/`MODEL_STROKE`, plus the gap before it.
fn band_marks_reserve(ui: &egui::Ui, content: &StripContent) -> f32 {
    let spacing = ui.spacing().item_spacing.x;
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let plate = |glyph: &str| {
        ui.painter()
            .layout_no_wrap(glyph.to_string(), mono.clone(), egui::Color32::WHITE)
            .size()
            .x
            + MODEL_PAD_X as f32 * 2.0
            + MODEL_STROKE * 2.0
            + spacing
    };
    let mut width = 0.0;
    if let Some(mode) = content.mode.mode.as_deref() {
        width += plate(mode_glyph(mode));
    }
    if content.session_allow.is_some() {
        width += plate(SESSION_ALLOW_LABEL);
    }
    width
}

/// **The least room the band's left half is worth having**, below which the telemetry chips are
/// dropped rather than the identity squeezed.
///
/// 🚨 **Priority, stated as a number.** The model chip is what the band is *for* — James kept it
/// deliberately, on the Claude Desktop model: you should always be able to see which model you
/// are talking to. The session spend and the last turn's duration are harness telemetry and are
/// already behind `/trace on`. So when the two cannot both fit, the telemetry goes and the
/// identity stays, rather than the identity eliding to `cla…` beside a full-width cost.
const BAND_LEFT_FLOOR: f32 = 190.0;

/// **Draw one of the band's optional words, or drop it.** Answers whether it was drawn.
///
/// 🚨 **This is the "drop by priority" half of the band's width rule, and the plates keep their
/// marks either way.** [`strip_right_reserve`] stops the left group as a whole from running under
/// the right one; within that group the *marks* are small and unconditional while the **words**
/// beside them are the give. A word that does not fit is not drawn at all — never half-drawn and
/// never overlapping — and the sentence it abbreviates is still on the plate's hover, which is
/// where the whole of it lived even when the two words did fit.
///
/// ⚠️ **Measured, not truncated.** `Label::truncate` on a two-word marker produces `not a…`,
/// which is a warning a reader has to guess at; the mark beside it is already carrying the same
/// fact unambiguously, so dropping the word is strictly better than eliding it. The reading is
/// the one item that truncates, because a truncated sentence is still a sentence.
fn band_word(ui: &mut egui::Ui, text: &str, color: Color32) -> Option<egui::Response> {
    let style = egui::TextStyle::Small.resolve(ui.style());
    let wanted = ui.painter().layout_no_wrap(text.to_string(), style, Color32::WHITE).size().x;
    if wanted + ui.spacing().item_spacing.x > ui.available_width() {
        return None;
    }
    Some(ui.label(RichText::new(text).color(color).small()))
}

fn standing_color(standing: Standing, theme: &Theme) -> Color32 {
    match standing {
        Standing::Dead => theme.bad,
        Standing::Asking => theme.asking,
        // One colour for both, deliberately. The distinction the palette has to carry is
        // busy-versus-blocked ([`Theme::asking`]'s note); busy-with-tools and busy-writing are the
        // same answer to "can I walk away", and giving them two amber-ish colours would spend
        // the band's whole colour budget on a difference the text already spells out.
        Standing::Working | Standing::Generating => theme.running,
        Standing::Connecting | Standing::Ready => theme.dim,
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
    /// The standing-allow marker was clicked: start asking again.
    ///
    /// ⚠️ **No confirmation, deliberately.** It revokes an authority rather than granting
    /// one, so the worst a stray click can do is make the console ask a question — and a
    /// confirm dialog in front of the *safe* direction would be the one place in this path
    /// where friction sits on the wrong side.
    RevokeSessionAllow,
}

fn status_strip(ui: &mut egui::Ui, pane: &mut ConversationPane, theme: &Theme) {
    let content = strip_content(
        pane.failure.as_deref(),
        LiveCounts {
            pending_approvals: pane.transcript.pending_approvals().len(),
            running_tools: pane.transcript.running_tools().len(),
            remembered: pane.memory.len(),
            session_allow: pane.memory.session_allow(),
            has_session: pane.transcript.session_id().is_some(),
            // The one reading that comes off the mapper rather than the transcript: the
            // bracket is a stream fact, and the transcript is an ordered list of what was
            // said, which cannot hold "and it is still being said".
            generating: pane.mapper.is_generating(),
        },
        pane.mapper.facts(),
        pane.transcript.session_id(),
    )
    .switching_to(pane.pending_model.as_ref().map(|p| p.label.as_str()));
    let rows = model_rows(&pane.models, pane.mapper.facts().model.as_deref());
    match strip_box(ui, &content, &rows, theme, pane.tracing) {
        Some(StripAct::ChooseModel(row)) => pane.choose_model(&row),
        Some(StripAct::ChooseMode(mode)) => pane.choose_permission_mode(mode),
        Some(StripAct::RevokeSessionAllow) => pane.revoke_session_allow(),
        None => {}
    }
}

// ---------------------------------------------------------------------------
// The status line and the log it drops down
// ---------------------------------------------------------------------------
//
// 🚨 **This surface replaced #127's drawer, and the reason is a layout invariant.** #127 drew
// the log immediately above the band, in a bottom-up column — so opening it pushed the composer
// up the screen. James, 2026-08-21: *"its positioning isn't right. It should not be displacing
// the entry box. The entry box should never move. So put the entry box back where it was and put
// the status log at the top, sort of like a Quake console drop-down. … By default, it's a status
// line and it sums up everything with a nice color theme to let you know everything's okay or
// warning or attention. And then you can click it and it expands down like a dropdown and shows
// more detail about whatever needs your attention."*
//
// So there are two pieces and the split is the design:
//
// * [`status_line`] is **permanent and exactly one row**, at the top of the pane. It never
//   appears or vanishes, so nothing below it can move when it changes; only its colour and its
//   words do. It is drawn in the top-down remainder, i.e. on the far side of the composer from
//   the band, which is what makes "the entry box never moves" structural rather than careful.
// * [`log_drop_down`] is an **`egui::Area`** — a layer, not a child. It takes no space in any
//   column, so opening it cannot displace anything by construction; it hangs off the status
//   line's bottom edge and paints over the page.

/// How many rows of log the drop-down shows before it scrolls.
///
/// ⚠️ **A ceiling, not a size**: the panel takes the smaller of this and [`LOG_PANEL_SHARE`] of
/// what the pane has below the status line, so a short console does not have its conversation
/// covered whole by a log somebody opened. Both bounds are needed — the fraction alone would make
/// the panel enormous on a tall window, and the row count alone would cover a short one entirely.
const LOG_PANEL_ROWS: f32 = 12.0;

/// The most of the pane below the status line the drop-down may cover.
const LOG_PANEL_SHARE: f32 = 0.55;

/// The status line's own padding and rule — the same shape [`STRIP_CHROME`] names for the band,
/// spelled separately because the two surfaces are allowed to differ and a shared constant would
/// hide it if they ever did.
const STATUS_LINE_CHROME: f32 = STRIP_PAD_Y as f32 * 2.0 + STRIP_STROKE * 2.0;

/// The dim word at the right of the status line, naming the surface a click opens.
///
/// ⚠️ **Two words rather than a glyph.** A caret alone is discoverable only by people who already
/// know; this is the one place the console gets to say what the surface *is*, and it costs a
/// fixed, measured 60-odd points that the summary truncates against.
const STATUS_LINE_NAME: &str = "status log";

/// Which of the palette's three states a [`Health`] is.
///
/// 🚨 **Pure, and it reaches for fields the [`Theme`] already owns** — no colour is invented for
/// this surface. `ok`/`asking`/`bad` are exactly "fine / worth your attention / broken", which is
/// the axis James named, and the band next to it already teaches the eye what each one means.
fn health_color(health: Health, theme: &Theme) -> Color32 {
    match health {
        Health::Ok => theme.ok,
        Health::Warning => theme.asking,
        Health::Attention => theme.bad,
    }
}

/// **The permanent one-line summary at the top of the pane, and the door to the log.**
///
/// Returns the rect it occupied, which is what [`log_drop_down`] hangs off.
///
/// 🚨 **It is always drawn and it is always one row.** An indicator that appears when there is
/// something to say is an indicator that reflows the page when there is — and the whole point of
/// this tier is that nothing reflows. So an empty log gets `nothing to report`, in
/// [`Theme::ok`], on a line that is the same height as the one carrying a broken pipe.
///
/// 🚨 **Everything it says is derived from the log's contents** by [`StatusLog::summary`], on
/// every frame. There is no flag anybody sets and nothing here judges anything — which is what
/// stops it becoming the status line this tree keeps finding, the kind that cannot be wrong.
fn status_line(ui: &mut egui::Ui, pane: &mut ConversationPane, theme: &Theme) -> egui::Rect {
    let summary = pane.status_log().summary();
    let open = pane.tracing();
    // The taller of the two faces this line can draw, for `strip_box`'s reason: the mark and the
    // summary are `Monospace`, the surface's name is `Body`, and reserving one of them is right
    // only by accident of which happens to be taller.
    let row = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.text_style_height(&egui::TextStyle::Monospace));
    let color = health_color(summary.health, theme);
    let inner = ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), row + STATUS_LINE_CHROME),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            Frame::new()
                .fill(theme.strip_fill)
                .stroke(egui::Stroke::new(STRIP_STROKE, theme.strip_edge))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(STRIP_PAD_X, STRIP_PAD_Y))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        // The name is a fixed item and the summary is the flexible one, measured
                        // first for the reason `strip_right_reserve` spells out at length: a
                        // label truncating to "whatever is left" is not bounded by anything when
                        // nothing has been taken yet, and the two then paint over each other.
                        let reserve = status_line_reserve(ui);
                        let left = reading_room(ui.available_width(), reserve);
                        ui.allocate_ui_with_layout(
                            egui::vec2(left, ui.available_height()),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                // ⚠️ `.monospace()` at both marks is the tofu fix the band and
                                // the approval card already carry — egui's proportional face has
                                // neither `●` nor `·`.
                                ui.label(
                                    RichText::new(drop_mark(open)).color(theme.dim).monospace(),
                                );
                                ui.label(
                                    RichText::new(if summary.health == Health::Ok {
                                        LOG_MARK_QUIET
                                    } else {
                                        LOG_MARK_EXCEPTION
                                    })
                                    .color(color)
                                    .monospace(),
                                );
                                ui.add(
                                    egui::Label::new(
                                        // Dim while healthy, coloured when not: a console with
                                        // nothing to report should not be shouting in green at
                                        // the top of every frame, and one that broke should be
                                        // impossible to read past.
                                        RichText::new(&summary.text)
                                            .color(if summary.health == Health::Ok {
                                                theme.dim
                                            } else {
                                                color
                                            })
                                            .monospace(),
                                    )
                                    .truncate(),
                                );
                            },
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    RichText::new(STATUS_LINE_NAME).color(theme.dim).small(),
                                );
                            },
                        );
                    });
                });
        },
    );
    let rect = inner.response.rect;
    let clicked = ui
        .interact(rect, ui.id().with("status-line"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if open {
            "the console's own log — click to close, or `/trace off`"
        } else {
            "the console's own log — click to open, or `/trace on`"
        })
        .clicked();
    if clicked {
        pane.toggle_log();
    }
    rect
}

/// What the status line's fixed right-hand item needs, before the flexible summary takes any.
///
/// The same arithmetic as [`strip_right_reserve`] and for the same measured reason; kept separate
/// because the two surfaces reserve different things and a shared function would have to take a
/// list of them, which is a worse way of saying "these are different".
fn status_line_reserve(ui: &egui::Ui) -> f32 {
    let spacing = ui.spacing().item_spacing.x;
    let style = egui::TextStyle::Small.resolve(ui.style());
    let name = ui
        .painter()
        .layout_no_wrap(STATUS_LINE_NAME.to_string(), style, egui::Color32::WHITE)
        .size()
        .x;
    name + spacing * 2.0
}

/// **The status log, dropped down over the page.** Draws nothing while it is closed.
///
/// 🚨 **An `egui::Area`, which is the whole point.** A child of the column would take space and
/// therefore move something; a layer cannot. It is positioned under [`status_line`]'s rect and
/// constrained to `area` — the conversation's own rect, handed down rather than re-derived,
/// because by the time this runs the column's cursor has moved and `ui.max_rect()` no longer
/// describes the pane.
///
/// ⚠️ **The id is derived from the `Ui`'s**, not a constant: a console divided into regions draws
/// several of these panes, and a fixed `Id` would give them one shared drop-down that opens in
/// whichever region drew last.
///
/// ⚠️ **It shows the log whole**, exceptions and machinery alike, newest at the bottom. There is
/// no filter and no mode: the quiet/loud decision has already been spent on which lines reach the
/// *conversation*, and spending it twice would give this surface its own opinion about what is
/// worth keeping — exactly the judgement that kept leaking chrome back into the flow.
///
/// ⚠️ **A row TRUNCATES; it never wraps and there is no horizontal scrollbar.** A trace line that
/// wraps stops looking like an entry — the second visual row has no timestamp and no mark, so the
/// column that makes the surface readable is broken by the first long line. A horizontal
/// scrollbar was the other candidate and is worse: it puts every long line behind a gesture, and
/// the identifying half of a console line is its beginning. The whole text is on the row's hover.
fn log_drop_down(
    ui: &mut egui::Ui,
    pane: &ConversationPane,
    theme: &Theme,
    line: egui::Rect,
    area: egui::Rect,
) {
    if !pane.tracing() {
        return;
    }
    let log = pane.status_log();
    let row = ui.text_style_height(&egui::TextStyle::Monospace);
    let top = line.bottom() + 4.0;
    let below = (area.bottom() - top).max(row);
    let rows = (row * LOG_PANEL_ROWS).min(below * LOG_PANEL_SHARE).max(row);
    let width = line.width();
    egui::Area::new(ui.id().with("status-log-drop"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(line.left(), top))
        .constrain_to(area)
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(theme.strip_fill)
                .stroke(egui::Stroke::new(STRIP_STROKE, theme.strip_edge))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(STRIP_PAD_X, STRIP_PAD_Y))
                .show(ui, |ui| {
                    ui.set_min_width(width);
                    ui.set_max_width(width);
                    ui.horizontal(|ui| {
                        // 🚨 **The date lives here and nowhere else, and that is what a session
                        // spanning midnight costs.** The rows say `HH:MM:SS` so they stay a
                        // column; `00:07:03` under `23:58:11` is unreadable unless something
                        // names the day, so the header names it — one date, or both.
                        let head = match log.date_span() {
                            Some(span) => format!("status log · {span}"),
                            None => "status log".to_string(),
                        };
                        ui.label(RichText::new(head).color(theme.dim).monospace().small());
                        // ✏️ **`` `/trace off` closes `` was drawn here and is gone**, and #130's
                        // reasoning survives this surface's move intact: a keystroke taught on
                        // screen for as long as the panel is open is ambience, however true it
                        // is, and James's rule reaches it — *"We never want text just pasted in
                        // explaining something into the UI."*
                        //
                        // ⚠️ **The way out is still named, one action later**, and better than it
                        // was: the status line this panel hangs off is directly above it, carries
                        // its own `-` disclosure mark, and its hover reads *"the console's own log
                        // — click to close, or `/trace off`"*. That is an answer to something you
                        // did, which is the form the rule leaves standing.
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("status-log-rows")
                        .max_height(rows)
                        .auto_shrink([false, true])
                        // Newest at the bottom, and the view sits there: a log is read from its
                        // end, and the end is where anything that just happened is.
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if log.is_empty() {
                                ui.label(
                                    RichText::new("nothing yet")
                                        .color(theme.dim)
                                        .monospace()
                                        .italics(),
                                );
                                return;
                            }
                            for remark in log.iter() {
                                log_row(ui, remark, theme);
                            }
                        });
                });
        });
}

/// One entry of the log, as one line: **time, mark, text.**
///
/// 🚨 **Structure by alignment, not by chrome.** James, 2026-08-21: *"it looks too much like
/// unstructured text. It should be more like entries in a trace log where each line is an entry.
/// And I don't mean add more rounded borders around each entry."* So there is no frame, no fill
/// and no rule per row. What makes a row read as an entry is that three things line up down the
/// panel: a fixed-width clock, a one-character mark, and the text — all in the mono face, which
/// is what a trace log looks like and is also the only face that carries `●` and `·`.
fn log_row(ui: &mut egui::Ui, remark: &Remark, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new(remark.at.clock()).color(theme.dim).monospace().small());
        let (mark, mark_color) = if remark.always {
            (LOG_MARK_EXCEPTION, theme.bad)
        } else {
            (LOG_MARK_QUIET, theme.dim)
        };
        ui.label(RichText::new(mark).color(mark_color).monospace());
        ui.add(
            egui::Label::new(
                RichText::new(&remark.text)
                    .color(if remark.always { theme.prose } else { theme.dim })
                    .monospace(),
            )
            .truncate(),
        )
        .on_hover_text(&remark.text);
    });
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
    theme: &Theme,
    // Whether this pane is narrating — the one input the band takes that is a *mode* rather
    // than a reading. It selects between what `StripContent` holds and what it draws; see
    // `StripContent::chips_seen` and `StatusReading::seen_text`.
    tracing: bool,
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
                .fill(theme.strip_fill)
                .stroke(egui::Stroke::new(STRIP_STROKE, theme.strip_edge))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(STRIP_PAD_X, STRIP_PAD_Y))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let reading = &content.reading;
                        // 🚨 `seen_text`, not `text`: an echo of the agent's own last line is
                        // off the band unless the pane is tracing. See `StatusReading`.
                        let reading_text = reading.seen_text(tracing);
                        // 🚨 `chips_seen`, not `chips`: the spend and the last turn's duration
                        // are harness telemetry and are drawn only while tracing. See `Chip`.
                        // Read here rather than at the draw site below because the left half's
                        // width budget depends on how wide they are — `strip_right_reserve`.
                        let mut chips = content.chips_seen(tracing);
                        // 🚨 **The chips are the first whole segment to go**, before anything on
                        // the left is squeezed — see [`BAND_LEFT_FLOOR`] for the priority and why
                        // it is the identity that stays. Decided once, here, so the reservation
                        // and the draw site below cannot disagree about what is on the band.
                        if reading_room(
                            ui.available_width(),
                            strip_right_reserve(ui, &chips),
                        ) < BAND_LEFT_FLOOR
                        {
                            chips.clear();
                        }
                        // 🚨 **The WHOLE left half is bounded, not just the reading — and that
                        // is what #129 changed.** The reservation used to be taken after the
                        // plates had already been added, so it bounded the one flexible item and
                        // nothing else: a model name long enough, or a permission marker long
                        // enough, still ran under the right-hand group, which is the overlap
                        // James photographed (`allow all` painted over `you allowed
                        // everything…`). Measuring first and allocating the remainder to a
                        // sub-`Ui` makes it structural: nothing in the left group can be drawn
                        // outside a rect that was sized before any of it existed.
                        let room = reading_room(
                            ui.available_width(),
                            strip_right_reserve(ui, &chips),
                        );
                        let mut act: Option<StripAct> = None;
                        let left = ui.allocate_ui_with_layout(
                            egui::vec2(room, ui.available_height()),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                // The identity, bounded by what the two marks will need — see
                                // [`band_marks_reserve`]. It shrinks to its text when there is
                                // room, so an ordinary band is unchanged.
                                let identity = reading_room(
                                    ui.available_width(),
                                    band_marks_reserve(ui, content),
                                );
                                ui.allocate_ui_with_layout(
                                    egui::vec2(identity, ui.available_height()),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| act = model_plate(ui, content, models, theme),
                                );
                                act = act.take().or(mode_plate(ui, content, theme));
                                // Immediately after the mode, because the two answer one
                                // question between them — "is the console still the
                                // authority?" — and reading them apart would be reading half
                                // the answer.
                                act = act.take().or(session_allow_plate(ui, content, theme));
                                // 🚨 The last item and the first to give way: at a width the
                                // plates have already spent, there is nothing left for a
                                // sentence and an ellipsis on its own says less than nothing.
                                if !reading_text.is_empty() && ui.available_width() > 1.0 {
                                    ui.add(
                                        egui::Label::new(
                                            // ⚠️ **`.monospace()` is the tofu fix, not a style
                                            // choice.** `status_reading` builds these strings
                                            // with `◈` (U+25C8) and `●` (U+25CF); egui's
                                            // PROPORTIONAL face has neither, so `● generating`
                                            // drew as a box. The mono face carries them — it
                                            // renders `htop`'s box drawing in the terminal tab
                                            // next door — and this is the same fix the approval
                                            // card's own `◈ may I` already carries. Leave it
                                            // on, or the band's symbols come back as boxes.
                                            //
                                            // Full size, not `.small()`: this is a *reading*,
                                            // the second thing a hand looks for after the model
                                            // name, and it is the only item between the plates
                                            // and the dim half. Left small it would be the one
                                            // shrunken word in a band that is otherwise one
                                            // size, which reads as a mistake rather than as a
                                            // hierarchy.
                                            RichText::new(reading_text)
                                                .color(standing_color(reading.standing, theme))
                                                .monospace(),
                                        )
                                        .truncate(),
                                    );
                                }
                            },
                        );
                        // The dim half. Right-aligned so the eye lands on the model and the
                        // standing first.
                        //
                        // ⚠️ **Dim, not small.** These carry numbers a hand reads across a
                        // desk — the session's spend, the turn it just paid for — and at
                        // `.small()` they were legibly smaller than the model name sitting
                        // opposite them on the same band. Colour is what makes this half
                        // secondary; size was doing a second job it was never needed for,
                        // and doing it at the cost of the one thing on the band with a
                        // number in it.
                        let right = ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                // First in a right-to-left layout is rightmost: the ring
                                // is the last thing on the band, which is where a gauge
                                // that is true continuously belongs and where the eye
                                // learns to find it without reading.
                                context_ring(ui, &content.context, theme);
                                // The same `chips` the budget was measured against, drawn with
                                // the separator that measurement used — see `CHIP_SEP`.
                                if !chips.is_empty() {
                                    ui.label(RichText::new(chips.join(CHIP_SEP)).color(theme.dim));
                                }
                            },
                        );
                        // 🚨 **Published so "no segment paints over its neighbour" can be
                        // MEASURED.** Both rects are the groups' own *content* bounds —
                        // `allocate_ui_with_layout` and `with_layout` return `min_rect`, not the
                        // size they asked for — so this reports what was actually drawn rather
                        // than what was intended. See [`band_group_rects`]; the band's height is
                        // NOT a detector here, because `Ui::horizontal` does not wrap and an
                        // overflowing left group stays exactly one row tall while running
                        // straight under the chips.
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(band_rects_id(), (left.response.rect, right.response.rect))
                        });
                        act
                    })
                    .inner
                })
                .inner
        },
    )
    .inner
}

/// The id [`strip_box`] files its two group rects under.
fn band_rects_id() -> egui::Id {
    egui::Id::new("organon-console-band-rects")
}

/// **What the band's two halves actually occupied on the last frame** — `(left, right)`.
///
/// 🚨 Same argument as [`composer_rect`]: the overlap James photographed twice is a geometric
/// fact, and a geometric fact asserted in prose is one nobody notices breaking. Nothing in the
/// draw path reads this; it exists so
/// [`tests::the_bands_two_halves_never_overlap_however_narrow_it_gets`] can check the property
/// instead of the height, which cannot see it.
pub fn band_group_rects(ctx: &egui::Context) -> Option<(egui::Rect, egui::Rect)> {
    ctx.data(|d| d.get_temp::<(egui::Rect, egui::Rect)>(band_rects_id()))
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
/// **What the band's right-hand fixed items need, before the flexible reading is laid out.**
///
/// 🚨 **The band had no width budget at all, and one segment painted over the next.** James,
/// on the live build: `◈ What are we working on?ession $1.18 · last turn 5.1s` — the echo's
/// tail running *under* the chips. The cause is egui's ordinary idiom used with an unbounded
/// left-hand item: the reading is added to the horizontal first and `Label::truncate` truncates
/// to `available_width`, which at that moment is *everything*; the right-aligned group added
/// after it is then handed a zero-width rect and lays out leftwards over what is already there.
/// Truncating "to what is left" is only a bound when something has already been taken.
///
/// So the fixed items are **measured first** and everything to their left is given the
/// remainder. The ring allocates a Body-height square every frame, measured or not
/// ([`context_ring`]), and the chips are one non-wrapping Body run — both are exactly as wide as
/// they are and neither can give way.
///
/// ⚠️ **Fixed items keep their space, the variable half gives way** — and at a width too narrow
/// for even the fixed set, [`reading_room`] returns nought and the left half is allocated a
/// zero-width rect, inside which the plates draw nothing and the reading is not drawn at all.
///
/// ✏️ **The status log's indicator used to be measured here and no longer exists.** The log's
/// door is now the permanent [`status_line`] at the top of the pane, so the band has one fewer
/// fixed item and hands that width back to the reading — which is the direction James asked the
/// band to move in: *"we don't want to show words like `default` and `allow all` at all times."*
fn strip_right_reserve(ui: &egui::Ui, chips: &[&str]) -> f32 {
    let spacing = ui.spacing().item_spacing.x;
    // Through the painter rather than `Ui::fonts`: laying a galley out takes the font cache
    // mutably, and the painter is the handle that has it.
    let measure = |text: String, style: egui::TextStyle| {
        let font = style.resolve(ui.style());
        ui.painter().layout_no_wrap(text, font, egui::Color32::WHITE).size().x
    };
    // The ring's own rule: a Body-height square, allocated whether or not it has an arc.
    let mut width = ui.text_style_height(&egui::TextStyle::Body);
    if !chips.is_empty() {
        width += spacing + measure(chips.join(CHIP_SEP), egui::TextStyle::Body);
    }
    // One more gap, between the left half and the first thing to its right.
    width + spacing
}

/// How much of a band its flexible half may take.
///
/// Pure, and a function rather than the subtraction written inline, because the property worth
/// pinning is the one a narrow window breaks: **the flexible half never takes room the fixed
/// items need**, and it never asks for a negative width. See
/// [`tests::the_band_gives_the_fixed_items_their_width_before_the_echo`].
///
/// Used by both bands — [`strip_box`] for its whole left group, and [`status_line`] for its
/// summary. One arithmetic, so the two cannot come to disagree about what "narrow" does.
fn reading_room(available: f32, reserved: f32) -> f32 {
    (available - reserved).max(0.0)
}

fn context_ring(ui: &mut egui::Ui, slot: &ContextSlot, theme: &Theme) {
    // 🚨 **Allocated and drawn every frame, measured or not** — see [`ContextSlot`] for the
    // track/fill split and for the decision this reverses. The allocation is the half that
    // matters structurally: a child that appears at the first `result` is a band that
    // reshuffles at the first `result`.
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
        egui::Stroke::new(CONTEXT_RING_STROKE, ring_track_color(slot, theme)),
    );
    let filled = match slot {
        ContextSlot::Unknown => 0.0,
        ContextSlot::Known(fill) => fill.fraction(),
    };
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
        let color = if slot.is_high() { theme.context_arc_high } else { theme.context_arc };
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(CONTEXT_RING_STROKE, color),
        ));
    }
    // The provenance lives here, in the same shape as the model plate's identity hover:
    // what it measures, what it does not, and both raw counts.
    response.on_hover_ui(|ui| {
        for (label, value) in ring_hover_rows(slot) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{label}:")).color(theme.dim).small().monospace());
                ui.label(RichText::new(value).color(theme.prose).small().monospace());
            });
        }
    });
}

/// Which circle the ring draws — **the whole of the difference between "no reading yet"
/// and "a reading of nought"**, in the only place a reader who is not hovering can see it.
///
/// A pure function of the slot rather than two `circle_stroke` calls in the two arms of a
/// `match`, because the property this has to hold — that the two are *not the same
/// colour* — is a statement about the pair, and a test can only make it about the pair if
/// there is one thing to ask twice.
fn ring_track_color(slot: &ContextSlot, theme: &Theme) -> Color32 {
    match slot {
        ContextSlot::Unknown => theme.context_track_empty,
        ContextSlot::Known(_) => theme.context_track,
    }
}

/// The ring's hover, for either state.
///
/// 🚨 **An unmeasured ring says so in words, and says when that changes.** The faint track
/// is the glanceable half of the distinction; this is the answerable half. Without it the
/// only way to tell an empty container from a zero reading would be to remember which
/// shade of green means which, which is not a thing an interface may require.
fn ring_hover_rows(slot: &ContextSlot) -> Vec<(String, String)> {
    match slot {
        ContextSlot::Known(fill) => context_rows(fill),
        // ⚠️ "not measured yet", never "0 %". The second row is what makes the first
        // actionable: it names the event the reader is waiting on rather than leaving them
        // to wonder whether the ring is broken.
        ContextSlot::Unknown => vec![
            ("context".to_string(), "not measured yet".to_string()),
            (
                "waiting on".to_string(),
                "a window from `result`, a prompt from `message_start`".to_string(),
            ),
        ],
    }
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
    theme: &Theme,
) -> Option<StripAct> {
    let plate = Frame::new()
        .fill(theme.model_fill)
        .stroke(egui::Stroke::new(MODEL_STROKE, theme.model_edge))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(MODEL_PAD_X, MODEL_PAD_Y))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            match &content.model {
                ModelSlot::Named(label) => {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&label.name).color(theme.model_text).strong().monospace(),
                        )
                        .truncate(),
                    );
                    if let Some(variant) = &label.variant {
                        ui.label(
                            RichText::new(variant).color(theme.model_badge).small().monospace(),
                        );
                    }
                }
                // Never an empty box and never "None": a plate with nothing in it during
                // connection reads as broken, which is worse than the strip being honest
                // about not knowing yet.
                ModelSlot::Connecting => {
                    ui.label(RichText::new("no model yet").color(theme.dim).small().italics());
                }
                ModelSlot::Absent => {
                    ui.label(RichText::new("no model").color(theme.dim).small().italics());
                }
            }
            if let Some(pending) = &content.pending_model {
                // Dim, italic and arrowed: it reads as a destination rather than as the
                // identity beside it, which is exactly the distinction being kept. Dropped
                // rather than elided at a narrow width — `→ Def…` is not a destination.
                let _ = band_word(ui, &format!("→ {pending}"), theme.dim);
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
                    ui.label(
                        RichText::new(format!("{label}:")).color(theme.dim).small().monospace(),
                    );
                    ui.label(RichText::new(value).color(theme.prose).small().monospace());
                });
            }
        });
    }
    egui::Popup::menu(&response)
        .show(|ui| model_picker(ui, models, theme))
        .and_then(|inner| inner.inner)
}

/// The model menu, built from the CLI's own `models` array.
///
/// An empty list is the honest case, not a failure: `initialize` is asked once at spawn and
/// its answer may not have arrived, or the session may not have answered it at all. The
/// picker says so rather than offering a table this build invented — see [`model_rows`].
fn model_picker(ui: &mut egui::Ui, models: &[ModelRow], theme: &Theme) -> Option<StripAct> {
    ui.set_min_width(240.0);
    if models.is_empty() {
        ui.label(
            RichText::new("the model list has not arrived — this session has not answered its \
                           `initialize` yet")
                .color(theme.dim)
                .small()
                .italics(),
        );
        return None;
    }
    let mut act = None;
    for row in models {
        let mut text = RichText::new(&row.label).monospace();
        if row.current {
            text = text.color(theme.model_text).strong();
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
            ui.label(RichText::new("in use").color(theme.dim).small());
        }
    }
    ui.separator();
    // 📌 One dim line, not a dialog. See [`MODEL_SWITCH_COST`].
    ui.label(RichText::new(MODEL_SWITCH_COST).color(theme.dim).small());
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
fn mode_plate(ui: &mut egui::Ui, content: &StripContent, theme: &Theme) -> Option<StripAct> {
    let Some(mode) = content.mode.mode.as_deref() else {
        // Before the first init the console does not know the mode, and a control that
        // offers to change something unknown is a control that can silently change it to
        // what it already was.
        return None;
    };
    let severity = content.mode.marker.as_ref().map(|m| m.severity);
    let accent = match severity {
        Some(ModeSeverity::Alert) => theme.mode_alert,
        Some(ModeSeverity::Note) => theme.mode_note,
        None => theme.dim,
    };
    let plate = Frame::new()
        .fill(theme.model_fill)
        .stroke(egui::Stroke::new(MODEL_STROKE, accent))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(MODEL_PAD_X, MODEL_PAD_Y))
        .show(ui, |ui| {
            // 🚨 **A mark, not the mode's name.** `default` was a word on the band from the
            // first frame of every session and said nothing a reader could act on; the mark
            // says the one thing they can — whether approvals still reach them. See
            // [`mode_glyph`], and `.monospace()` there for the tofu rule.
            ui.label(RichText::new(mode_glyph(mode)).color(accent).monospace());
        });
    // ⚠️ **The mode's name has NOT been dropped, it has moved.** It is the first row of the
    // plate's hover, so `which mode is this?` is still answerable from the band — it simply is
    // not asserted at a reader who did not ask. The picker underneath spells all three out.
    let hover = match mode_consequence(mode) {
        Some(consequence) => format!("{mode} — {consequence}"),
        None => format!("{mode} — a mode this build has not measured"),
    };
    let response = plate
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hover);
    // The persistent marker, in its **short** form — see [`ModeMarker::short`] for why the
    // sentence is on the hover and two words are on the band, and [`band_word`] for what a band
    // too narrow to hold even those does instead. ⚠️ **The mark above is unconditional**, so a
    // pane narrow enough to lose the words still says that the console may not be the one being
    // asked; the words are the give, the warning is not.
    if let Some(marker) = &content.mode.marker {
        if let Some(word) = band_word(ui, &marker.short, accent) {
            word.on_hover_text(&marker.text);
        }
    }
    egui::Popup::menu(&response)
        .show(|ui| mode_picker(ui, mode, theme))
        .and_then(|inner| inner.inner)
}

/// **The standing allow, on the band, for as long as it is on** — and the place it is
/// revoked from.
///
/// 🚨 Nothing at all when there is no standing allow, which is the normal case: this is the
/// one plate on the band that is *absent* rather than empty, because its whole job is to be
/// noticed. The band's height does not move either way — [`strip_box`] reserves one row
/// before anything lays itself out.
///
/// The revoke is here rather than on a card because there is no one card it belongs to: the
/// grant covers every call, including ones that have not happened yet. A human who realises
/// they granted too much must not have to go looking for the card they clicked.
fn session_allow_plate(
    ui: &mut egui::Ui,
    content: &StripContent,
    theme: &Theme,
) -> Option<StripAct> {
    let slot = content.session_allow.as_ref()?;
    let plate = Frame::new()
        .fill(theme.model_fill)
        .stroke(egui::Stroke::new(MODEL_STROKE, theme.mode_alert))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(MODEL_PAD_X, MODEL_PAD_Y))
        .show(ui, |ui| {
            ui.label(
                RichText::new(SESSION_ALLOW_LABEL).color(theme.mode_alert).small().monospace(),
            );
        });
    let clicked = plate
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{SESSION_ALLOW_CONSEQUENCE}\n\nClick to revoke."))
        .clicked();
    // ✏️ The **short** form — see [`SESSION_ALLOW_SHORT`] for what moved and why — and dropped
    // rather than elided when the band is too narrow for it ([`band_word`]). ⚠️ The plate above
    // is unconditional, so the grant is still stated and still revocable at any width.
    let marker = band_word(ui, slot.short, theme.mode_alert).map(|word| {
        word.on_hover_text(format!(
            "{}\n\n{SESSION_ALLOW_CONSEQUENCE}\n\nClick to revoke.",
            slot.marker
        ))
    });
    // The marker itself is clickable too, because it is the wider target and it is what the
    // eye actually lands on — the plate beside it is the label, not the button.
    let marker_clicked =
        marker.map(|m| m.interact(egui::Sense::click()).clicked()).unwrap_or(false);
    (clicked || marker_clicked).then_some(StripAct::RevokeSessionAllow)
}

/// The three modes, each labelled by what happens.
fn mode_picker(ui: &mut egui::Ui, current: &str, theme: &Theme) -> Option<StripAct> {
    ui.set_min_width(320.0);
    let mut act = None;
    for row in MODE_ROWS {
        let accent = match row.severity {
            ModeSeverity::Alert => theme.mode_alert,
            ModeSeverity::Note => theme.prose,
        };
        let chosen = row.value == current;
        let mut name = RichText::new(row.value).monospace().color(accent);
        if chosen {
            name = name.strong();
        }
        let clicked = ui.add(egui::Button::new(name).frame(false)).clicked();
        // The consequence is not a tooltip: it is the label. A hover would put the one
        // sentence that matters behind a gesture nobody makes while deciding.
        ui.label(
            RichText::new(row.consequence)
                .color(if chosen { theme.prose } else { theme.dim })
                .small(),
        );
        if chosen {
            ui.label(RichText::new("in use").color(theme.dim).small());
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

// ---------------------------------------------------------------------------
// The command panel — the region directly above the composer
// ---------------------------------------------------------------------------

/// The most candidates the panel draws at once.
///
/// 🚨 **A cap rather than a scroll area, and the reason is this file's oldest measured
/// hazard**: a vertical [`egui::ScrollArea`] dropped into a [`egui::Layout::bottom_up`]
/// column places itself at the top of the remaining space and swallows the pane — 684 pt of a
/// 684 pt pane, measured, see [`composer_box`]. `console.background` already offers more
/// materials than fit on a screen, so the list genuinely overflows; the honest answer to that
/// is a count of what is left and an invitation to type one more letter, which is also the
/// faster way to reach the one you want.
const PALETTE_MAX_ROWS: usize = 8;

/// How many candidate rows are drawn, and how many are left over.
pub fn palette_rows(total: usize) -> (usize, usize) {
    let shown = total.min(PALETTE_MAX_ROWS);
    (shown, total - shown)
}

/// The panel's plate, matching the band below it — see [`strip_box`]'s constants.
const PALETTE_PAD_X: i8 = 10;
const PALETTE_PAD_Y: i8 = 6;
const PALETTE_STROKE: f32 = 1.0;

/// The mark on the highlighted row, and on every other one.
///
/// ⚠️ **ASCII, deliberately.** A disclosure triangle is the obvious character and is in none
/// of egui's four bundled fonts — `card_density`'s marks and the subagent card's dingbats
/// both shipped as boxes before the allowlist guard existed. `>` is in everything.
const PALETTE_HERE: &str = ">";
const PALETTE_THERE: &str = " ";

/// What sits between two words of the compact row. James wrote the row out himself —
/// `surface|theme|posture|…` — and this is that, given room to breathe.
const PALETTE_SEP: &str = " | ";
/// The two characters that mark the word Tab would take, wrapped around it.
///
/// ⚠️ **A bracket rather than a colour alone**, and rather than the `>` the verbose list
/// uses. Colour alone is a weak signal in a row of same-sized words and dies in a
/// screenshot; a leading `>` reads as a bullet when there is only one row of them. Brackets
/// say *selected* at a glance and survive both. ASCII, for [`PALETTE_HERE`]'s reason.
const PALETTE_PICKED: (&str, &str) = ("[", "]");
/// The head of the tail note when the words outran the pane. See [`compact_fit`].
const PALETTE_MORE: &str = "+";
/// What the row says when the line **as it stands** is already a whole command.
///
/// 🚨 **The alternative was a blank panel, and a blank panel reads as a broken one.** `/surface`
/// takes no arguments: there is nothing to offer, so the row had nothing in it and (with a
/// space typed after the verb) the panel disappeared altogether. James: *"slash surface shows
/// no options."* True, and beside the point — what the console knew and did not say is that
/// Enter would run the line. ASCII, for [`PALETTE_HERE`]'s reason.
///
/// ✏️ **It outlived the head row's permanent key legend, and deliberately.** That line —
/// `Tab completes - Enter runs`, whose second half this used to borrow — was on screen for
/// as long as the panel was, teaching two keystrokes to a reader who wrote them. This one
/// appears **only** in the state where the panel would otherwise be blank, and a blank panel
/// reads as a broken one. Instruction that is always there is chrome; the same words shown
/// exactly where the surface has nothing else to say are the surface not lying about itself.
const PALETTE_RUNS: &str = "Enter runs";

/// One word of the compact row, already carrying whatever marks it as chosen.
///
/// 🚨 **The words and the drawing come from one derivation.** [`compact_line`] renders these
/// to a plain string for a test and for a report; [`compact_band`] colours the same pieces
/// into a [`egui::text::LayoutJob`]. A renderer that built its own words would be able to
/// disagree with the string the tests pin — the failure `registry` exists to prevent, one
/// scale down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactWord {
    pub text: String,
    /// The one Tab would take. Exactly one word carries this when there are any.
    pub here: bool,
    /// Not a continuation at all: a statement that Enter would run the line as it stands. At
    /// most one word carries this, it is always the first, and it is never something Tab can
    /// take — which is why it is a field rather than another candidate with a special label.
    pub runs: bool,
}

/// The compact row's words, in table order, for the line as it stands.
///
/// Derived from [`Registry::candidates`] and from nothing else — the house rule is that
/// nothing restates the vocabulary, so this list narrows as letters are typed for free and
/// gains a verb the day the catalog does.
///
/// ⚠️ **The hint stands in for the words when there are none.** `/patch ` wants a whole
/// number and `/camera distance ` wants a number in a stated band: neither has options to
/// list, and [`Palette::hint`] is the sentence written for exactly those. A row that showed
/// nothing there would be the console knowing the answer and declining to say it.
///
/// 🚨 **…and [`PALETTE_RUNS`] leads, for the same reason one scale up.** A line that is already
/// a whole command is the case where *both* of the above are empty — `/surface` and `/surface `
/// offer nothing because there is nothing left to offer — and the row went blank on exactly the
/// lines a hand had finished typing. It comes **first** because [`compact_fit`] drops from the
/// tail: put last, the one thing a settled line has to say would be the first thing a narrow
/// pane hid.
pub fn compact_words(palette: &Palette, selected: usize) -> Vec<CompactWord> {
    let mut words = Vec::new();
    if palette.runnable {
        words.push(CompactWord { text: PALETTE_RUNS.to_string(), here: false, runs: true });
    }
    if palette.candidates.is_empty() {
        words.extend(
            palette.hint().map(|hint| CompactWord { text: hint, here: false, runs: false }),
        );
        return words;
    }
    words.extend(palette.candidates.iter().enumerate().map(|(index, candidate)| {
        let here = index == selected;
        let text = if here {
            format!("{}{}{}", PALETTE_PICKED.0, candidate.label, PALETTE_PICKED.1)
        } else {
            candidate.label.clone()
        };
        CompactWord { text, here, runs: false }
    }));
    words
}

/// How many of `words` fit across `columns` monospace cells, and how many are left over.
///
/// 🚨 **Counted rather than truncated, and the reason is the glyph allowlist.** egui's own
/// truncation appends `…` (U+2026), which is in none of its four bundled fonts and would
/// ship as a box — the exact defect `no_symbol_the_console_draws_is_a_glyph_egui_lacks`
/// exists to catch. A count is also the more useful answer: `+3` says how much narrowing is
/// left to do, where an ellipsis says only that something was hidden.
///
/// **Columns, not points**, because the row is drawn entirely in the monospace face: one
/// character is one advance, so a width in characters is exact rather than an estimate. A
/// proportional row would have to be laid out before it could be measured.
pub fn compact_fit(words: &[CompactWord], columns: usize) -> (usize, usize) {
    let width = |shown: usize| -> usize {
        words.iter().take(shown).map(|w| w.text.chars().count()).sum::<usize>()
            + PALETTE_SEP.chars().count() * shown.saturating_sub(1)
    };
    if words.is_empty() || width(words.len()) <= columns {
        return (words.len(), 0);
    }
    // Drop from the tail until the row AND the note counting what was dropped both fit. The
    // note grows a digit as more is hidden, so it is measured at each step rather than once.
    let mut shown = words.len();
    while shown > 0 {
        let hidden = words.len() - shown;
        let note = PALETTE_SEP.chars().count()
            + PALETTE_MORE.chars().count()
            + hidden.to_string().chars().count();
        if width(shown) + note <= columns {
            break;
        }
        shown -= 1;
    }
    (shown, words.len() - shown)
}

/// The compact row as one plain string — what a human sees, without egui.
///
/// Exists so the row can be *read* in a test and quoted to somebody who is not looking at the
/// window. [`compact_band`] draws these same pieces; nothing here is a second derivation.
pub fn compact_line(palette: &Palette, selected: usize, columns: usize) -> String {
    compact_join(&compact_words(palette, selected), columns)
}

/// Fit a row of words to `columns` and join them — the row as one plain string.
///
/// 🚨 **Split out of [`compact_line`] so a SECOND producer of words gets the same row.**
/// `region_line` builds its words from a pruned palette rather than from
/// `Registry::candidates`, and a second joiner beside it would be a second answer to "what does
/// a hidden count look like" — which is exactly the drift `compact_fit`'s own doc argues
/// against. Two producers, one fitting rule, one separator, one `+N`.
pub fn compact_join(words: &[CompactWord], columns: usize) -> String {
    let (shown, hidden) = compact_fit(words, columns);
    let mut line = words
        .iter()
        .take(shown)
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(PALETTE_SEP);
    if hidden > 0 {
        if !line.is_empty() {
            line.push_str(PALETTE_SEP);
        }
        line.push_str(PALETTE_MORE);
        line.push_str(&hidden.to_string());
    }
    line
}

/// A row of the panel, exactly one [`palette_row_height`] tall.
///
/// 🚨 **This is the overlap fix, and `ui.horizontal` is what it replaces.** `Ui::horizontal`
/// seeds its child with `spacing().interact_size.y` — 18 pt on egui's default style, on the
/// assumption that a horizontal row holds something interactive — and
/// `allocate_ui_with_layout_dyn` then advances by `frame_rect.union(final_child_rect)`, so a
/// row of text still costs the whole 18. The panel's band was arithmetic over *text* heights,
/// which here are 15.125 pt: **measured at 2.875 pt of overflow per row**, by putting
/// `ui.horizontal` back and reading `plate`'s own return. Allocating the row explicitly makes
/// the arithmetic and the drawing the same statement.
///
/// ⚠️ **And the overflow goes DOWNWARD, which is why it was visible rather than merely
/// wrong.** `plate` reserves its band in a bottom-up column but lays out top-down inside it,
/// so rows that outgrow the reservation are painted past its lower edge — over the composer,
/// which was placed there first. Ten rows (a head, eight verbs and a `+N` line, which is what
/// a bare `/` draws against the real table) put ~29 pt of panel across the top line of the
/// text box. That is exactly what James reported: *"Your current box extends lower than that
/// and covers a bit of the text."*
fn palette_row(ui: &mut egui::Ui, row: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), row),
        egui::Layout::left_to_right(egui::Align::Center),
        add,
    );
}

/// One row of the panel, in points.
///
/// ⚠️ **Posture is in here and it is not decoration.** [`body`] applies
/// [`Form::body_line_height`], which at the desktop end is strictly *greater* than the text's
/// own height — so a band measured from `text_style_height` alone is short by that
/// difference at every posture but the terminal one, and short means painting over the
/// composer. The three heights are maxed rather than chosen because different rows of the
/// same panel use different ones.
fn palette_row_height(ui: &egui::Ui, form: &Form) -> f32 {
    let body = ui.text_style_height(&egui::TextStyle::Body);
    let mono = ui.text_style_height(&egui::TextStyle::Monospace);
    body.max(mono).max(form.body_line_height(body).unwrap_or(0.0))
}

/// The plate's height for `rows` rows of content.
pub fn palette_band(rows: usize, row: f32, spacing: f32) -> f32 {
    rows as f32 * row
        + rows.saturating_sub(1) as f32 * spacing
        + 2.0 * PALETTE_PAD_Y as f32
        + 2.0 * PALETTE_STROKE
}

/// Draw whatever owns the region above the composer this frame: a command's answer, or the
/// candidates for the line being typed. **Never both** — they mean opposite things and one
/// region cannot say two things at once.
///
/// The receipt wins while it holds ([`receipt_holds`]), because it is about a line that has
/// already been sent and the candidates are about a line that has not.
///
/// Returns how far the plate outgrew the band it reserved — **zero by construction**, and a
/// test pins it, because a plate that outgrows its reservation paints over the composer
/// rather than pushing the scrollback up. See [`palette_row`] for the mechanism that made
/// that a live defect.
///
/// ⚠️ **Two returns, and they answer different questions.** The `f32` is that overflow, owed
/// by every band this function can draw including the editor's; the `Option<ThemeChange>` is
/// what a hand moved in the palette editor. They travel together because one function draws
/// all three surfaces and the caller needs both — not because they are related.
fn command_panel(
    ui: &mut egui::Ui,
    pane: &mut ConversationPane,
    theme: &Theme,
    form: &Form,
) -> (f32, Option<ThemeChange>) {
    // 🚨 **The editor wins the band outright while it is open**, ahead of both a receipt and
    // the candidate list. Those two answer a line — one being typed, one just sent — and are
    // gone within seconds; the editor is a surface a hand is *working in*, and one that
    // vanished because a keystroke reached the composer underneath would be unusable. It is
    // also the only one of the three a person explicitly opened and can explicitly close.
    if pane.theme_edit.is_some() {
        return pane.theme_editor_ui(ui, theme);
    }
    let now = ui.input(|i| i.time);
    // 🚨 **A successful receipt is not drawn unless the pane is tracing.** ✏️ It always was, for
    // eight seconds, and it is the *"anything like that"* half of James's complaint: `ok /theme
    // dark — {"accepted":"theme dark"}` sitting over the composer while the console repaints in
    // front of him. The refusal band stays unconditional — see [`receipt_holds`], whose whole
    // subject is that a refusal outlives a success, and which this extends rather than replaces.
    //
    // ⚠️ **Held, not skipped.** `pane.receipt` is still set and still ages; only the *drawing* is
    // gated. Clearing it here instead would mean `/trace on` mid-receipt changed what the pane
    // believes happened, and the band is a view of that state rather than the state itself.
    let show_receipt = pane.tracing || pane.receipt.as_ref().is_some_and(|r| !r.receipt.ok);
    if let Some(held) = pane.receipt.as_mut().filter(|_| show_receipt) {
        // Stamped on the first frame it is drawn, not when it was made: a receipt must not
        // age while nothing is on screen to have read it.
        let since = *held.since.get_or_insert(now);
        if receipt_holds(held.receipt.ok, &held.answered, &pane.composer, now - since) {
            let receipt = held.receipt.clone();
            return (receipt_band(ui, &receipt, theme, form), None);
        }
        pane.receipt = None;
    }
    let Some(palette) = pane.palette().and_then(|p| drawn_palette(p, &pane.composer)) else {
        return (0.0, None);
    };
    let palette = &palette;
    // Clamped rather than kept in step: the list is rebuilt from the line on every keystroke,
    // so an index from the previous list is the one thing that can be out of range.
    let selected = pane.palette_selected.min(palette.candidates.len().saturating_sub(1));
    pane.palette_selected = selected;
    let overflow = if pane.verbose {
        candidate_panel(ui, &pane.registry, &palette, selected, theme, form)
    } else {
        compact_band(ui, &palette, selected, theme, form)
    };
    (overflow, None)
}

/// What the panel actually draws for `line`, or `None` when there is nothing left to draw.
///
/// 🚨 **A sole candidate that is already the whole line is not a choice.** By the time this
/// runs [`palette_complete`] has taken every completion available, so a one-item list here is
/// offering the word the line already ends with — the thing James asked not to be shown
/// (*"Do not show me the single choice… simply complete the completion"*). It is a decision
/// about drawing, so it lives here rather than in the registry: [`Palette::sole_completion`]
/// still reports it and [`Palette::autorun`] still needs it.
///
/// ⚠️ **Dropping the word is not the same as dropping the row, and conflating the two is the
/// second half of what James hit.** `/surface` is exactly this case *and* a whole command, so
/// returning nothing here left a blank region above the composer on the lines a hand had
/// finished typing. The redundant candidate is dropped from **this copy** — display only,
/// never the palette [`palette_keys`] reads — and what survives is whatever the line is still
/// true about, which [`Palette::is_empty`] is the one judge of.
///
/// Pure, and separate from the drawing, so a test can read the row a human would see without
/// standing up an [`egui::Ui`] — and so there is one derivation of it rather than two.
fn drawn_palette(mut palette: Palette, line: &str) -> Option<Palette> {
    if palette.sole_completion(line).is_none() && palette.candidates.len() == 1 {
        palette.candidates.clear();
    }
    (!palette.is_empty()).then_some(palette)
}

/// **The primary panel: one row, every word the line could become next.**
///
/// James, 2026-08-14, having used the verbose list: *"I want the primary mode to be more
/// compact and I want it to be simply a list of the available terms. […] I'm not suggesting
/// it is purely text with pipes in between. You can be a little more creative than that, but
/// I want it to be a very useful display."*
///
/// So: the same full-width plate, one row high, holding the words and nothing else — no
/// heading, no per-word doc, no key legend. Everything that was cut is still one env var
/// away in [`candidate_panel`].
///
/// ⚠️ **Monospace throughout, and that is load-bearing rather than a look.** [`compact_fit`]
/// measures the row in *characters*, which is only exact when a character has one width.
fn compact_band(
    ui: &mut egui::Ui,
    palette: &Palette,
    selected: usize,
    theme: &Theme,
    form: &Form,
) -> f32 {
    let row = palette_row_height(ui, form);
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    // The mono face's advance, asked of the fonts rather than assumed — a console at a larger
    // text size has a wider cell and must fit fewer words, not the same number clipped.
    let cell = ui.ctx().fonts_mut(|f| f.glyph_width(&font, '0')).max(1.0);
    let columns = ((ui.available_width() - 2.0 * PALETTE_PAD_X as f32) / cell).floor().max(0.0);
    let words = compact_words(palette, selected);
    let (shown, hidden) = compact_fit(&words, columns as usize);
    plate(ui, palette_band(1, row, 0.0), theme, |ui| {
        palette_row(ui, row, |ui| {
            let mut job = egui::text::LayoutJob::default();
            let mut piece = |text: &str, color: Color32| {
                job.append(
                    text,
                    0.0,
                    egui::TextFormat { font_id: font.clone(), color, ..Default::default() },
                );
            };
            for (index, word) in words.iter().take(shown).enumerate() {
                if index > 0 {
                    piece(PALETTE_SEP, theme.dim);
                }
                // The run marker takes the affirmative colour the receipt band's `ok` uses —
                // it is the same claim about the same line, one keystroke earlier. The
                // highlighted candidate shares it and is told apart by its brackets.
                let color = if word.runs || word.here { theme.ok } else { theme.prose };
                piece(&word.text, color);
            }
            if hidden > 0 {
                if shown > 0 {
                    piece(PALETTE_SEP, theme.dim);
                }
                piece(&format!("{PALETTE_MORE}{hidden}"), theme.dim);
            }
            ui.add(egui::Label::new(job));
        });
    })
}

/// One command's answer, in the place the command was typed.
fn receipt_band(ui: &mut egui::Ui, receipt: &Receipt, theme: &Theme, form: &Form) -> f32 {
    let row = palette_row_height(ui, form);
    plate(ui, palette_band(1, row, 0.0), theme, |ui| {
        palette_row(ui, row, |ui| {
            // The marker is a word, not a glyph: the two symbols that would say this best
            // (`✓`, `✗`) are in none of egui's fonts and shipped as boxes once already.
            let (mark, color) =
                if receipt.ok { ("ok", theme.ok) } else { ("refused", theme.bad) };
            ui.add(egui::Label::new(label(ui, mark, color, form)).truncate());
            ui.add(
                egui::Label::new(body(
                    ui,
                    receipt.text.clone(),
                    if receipt.ok { theme.dim } else { theme.human_text },
                    form,
                ))
                .truncate(),
            );
        })
    })
}

/// The candidates for the line as it stands: what the head of the line has settled, then
/// every continuation of it, one row each with its own doc.
///
/// ⚠️ **This is now the *verbose* mode**, off unless `ORGANON_PALETTE_VERBOSE=1` — see
/// [`ConversationPane::verbose`]. It was the whole panel until 2026-08-14; James used it,
/// liked that it existed, and asked for something a tenth the height as the thing that opens
/// by default. Kept whole rather than trimmed: what it says is what a person wants the first
/// few times, and [`compact_band`] is what they want after that.
fn candidate_panel(
    ui: &mut egui::Ui,
    registry: &Registry,
    palette: &Palette,
    selected: usize,
    theme: &Theme,
    form: &Form,
) -> f32 {
    let (shown, hidden) = palette_rows(palette.candidates.len());
    let hint = palette.hint();
    let rows = 1 + shown + usize::from(hidden > 0) + usize::from(hint.is_some());
    let row = palette_row_height(ui, form);
    let spacing = ui.spacing().item_spacing.y;
    let band = palette_band(rows, row, spacing);

    // The head: what this ring is *of*. For a settled verb that is its own derived usage
    // line and its own doc, so the panel restates nothing.
    let (title, note) = match palette.verb().and_then(|verb| registry.entry(verb)) {
        Some(entry) => (entry.usage(), entry.doc().to_string()),
        None => ("commands".to_string(), String::new()),
    };
    plate(ui, band, theme, |ui| {
        palette_row(ui, row, |ui| {
            ui.add(egui::Label::new(label(ui, title, theme.panel_title, form)).truncate());
            if !note.is_empty() {
                ui.add(egui::Label::new(label(ui, note, theme.dim, form)).truncate());
            }
            // ✏️ **The right of this row used to carry `Tab completes - Enter runs`.** It was
            // the panel teaching its own keystrokes for as long as the panel was open, which
            // is chrome on every frame for a reader who already types them. The row keeps its
            // height either way — [`palette_band`] reserves rows, not content — so the title
            // and its usage line simply have the width back. [`PALETTE_RUNS`] is the one
            // place the words survive, and its doc says why that case is different.
        });
        for (index, candidate) in palette.candidates.iter().take(shown).enumerate() {
            let here = index == selected;
            palette_row(ui, row, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(if here { PALETTE_HERE } else { PALETTE_THERE })
                            .monospace()
                            .color(if here { theme.ok } else { theme.dim }),
                    )
                    .truncate(),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(candidate.label.clone())
                            .monospace()
                            .color(if here { theme.human_text } else { theme.prose }),
                    )
                    .truncate(),
                );
                if !candidate.doc.is_empty() {
                    ui.add(
                        egui::Label::new(label(ui, candidate.doc.clone(), theme.dim, form))
                            .truncate(),
                    );
                }
            });
        }
        if hidden > 0 {
            palette_row(ui, row, |ui| {
                ui.add(
                    egui::Label::new(label(
                        ui,
                        format!("{PALETTE_MORE}{hidden} more - type another letter to narrow"),
                        theme.dim,
                        form,
                    ))
                    .truncate(),
                );
            });
        }
        if let Some(hint) = hint {
            palette_row(ui, row, |ui| {
                ui.add(egui::Label::new(label(ui, hint, theme.dim, form)).truncate());
            });
        }
    })
}

/// The reserved band both halves of the panel sit in. Returns how far the plate outgrew that
/// band — see [`command_panel`].
///
/// 🚨 **Reserved, not discovered**, for [`strip_box`]'s measured reason: this is a bottom-up
/// column, and a child that places itself at `available_rect_before_wrap().min` takes
/// everything between the top of the remaining space and the cursor at its bottom.
///
/// 🚨 **…and the reservation is not a clip.** A bottom-up parent grows the child's *upper*
/// edge, while the child lays out downwards from the top of what it was given — so content
/// taller than `band` does not push anything, it paints straight over whatever was placed
/// below, which is the composer. That is why the overflow is returned rather than assumed
/// away: it is the one number that says whether the panel is sitting on the text box.
fn plate(ui: &mut egui::Ui, band: f32, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) -> f32 {
    // What is left for the rows once the plate's own chrome is paid for — and therefore the
    // number the row arithmetic in `palette_band` has to have got right. ⚠️ Measured from the
    // CONTENT rather than from the frame: `Frame`'s own rect fills whatever it was allocated,
    // so it reports `band` back whether the rows fitted inside it or spilled out of the
    // bottom, which is the one distinction being asked about.
    let budget = band - 2.0 * PALETTE_PAD_Y as f32 - 2.0 * PALETTE_STROKE;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), band),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            Frame::new()
                .fill(theme.strip_fill)
                .stroke(egui::Stroke::new(PALETTE_STROKE, theme.strip_edge))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(PALETTE_PAD_X, PALETTE_PAD_Y))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    add(ui);
                    (ui.min_rect().height() - budget).max(0.0)
                })
                .inner
        },
    )
    .inner
}

/// What one key press means to the command panel, while the panel is open.
///
/// A free function over literal values, for [`composer_key`]'s reason: egui's modifier
/// matching is shift-permissive, and a table of plain values is the only place that hazard is
/// legible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKey {
    /// Take the highlighted candidate. **Never sends.**
    Accept,
    /// Move the highlight down the list, and up it.
    Next,
    Prev,
    /// Shut the panel until the line changes.
    Dismiss,
    /// Not the panel's business.
    Ignore,
}

/// 🚨 **Tab accepts, Enter sends, and they are never the same key.**
///
/// The composer is also where a human talks to the agent, so the send key has to mean one
/// thing always. Making Enter *accept a completion* when a panel happens to be open would
/// give one key two meanings chosen by invisible state, and the failure mode is sending the
/// wrong thing — the one thing this box must not do. Tab cannot send anything at all, which
/// is what makes it safe to hand the panel.
///
/// ⚠️ **Enter with exactly one candidate left is deliberately NOT an accept.** `/theme` names
/// one verb and is *not* a runnable command, so an Enter that accepted would have to either
/// run an incomplete command or silently rewrite the line and wait for a second Enter — one
/// key doing two different things one keystroke apart. Instead Enter goes to
/// [`Registry::resolve`], which refuses it by name (*"`/theme` needs `name`"*) and **does not
/// clear the composer**, so the words are still there and Tab is one key away.
///
/// ⚠️ **Escape's hazard is real here but it is not the terminal's.** In a terminal tab
/// Escape belongs to the child and must be consumed before `term_view` clones the event
/// vector; the conversation front-end has no child reading keys, so that hazard does not
/// apply. A different one does: egui's focus manager reads `Escape` out of the **raw input**,
/// in `Focus::begin_pass`, *before* any of this code runs, and drops the focused widget —
/// and `TextEdit` exposes no setter for `EventFilter::escape` to stop it. So Escape cannot be
/// prevented from blurring the composer; it is *repaired* instead, by re-requesting focus in
/// the same frame the panel is dismissed. That costs one frame in which nothing is focused
/// and no keystroke can arrive.
///
/// ⚠️ `matches_exact` throughout, never `matches_logically`: the latter would let Shift+Tab
/// and Ctrl+Escape through as their bare selves, which is the trap [`composer_key`] documents.
pub fn palette_key(key: egui::Key, mods: egui::Modifiers) -> PaletteKey {
    let bare = mods.matches_exact(egui::Modifiers::NONE);
    let shifted = mods.matches_exact(egui::Modifiers::SHIFT);
    match key {
        egui::Key::Tab if bare => PaletteKey::Accept,
        egui::Key::Tab if shifted => PaletteKey::Prev,
        egui::Key::ArrowDown if bare => PaletteKey::Next,
        egui::Key::ArrowUp if bare => PaletteKey::Prev,
        egui::Key::Escape if bare => PaletteKey::Dismiss,
        _ => PaletteKey::Ignore,
    }
}

/// Move the highlight, wrapping. `len` of nought answers nought — a list with nothing in it
/// has no row to be on, and wrapping arithmetic on an empty list is a panic waiting to be
/// reached by someone holding a key down.
pub fn move_selection(current: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    let current = current.min(len - 1);
    if forward {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    }
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

// The composer's plate and its three edges are [`Theme`]'s `composer_fill`,
// `composer_edge`, `composer_edge_focus` and `composer_edge_dead`.

/// What an empty composer says.
///
/// ✏️ **A live one says nothing, and that is the whole of it.** It read `message the agent —
/// Enter sends, Shift+Enter for a new line`, which is the first thing James saw on the build
/// after #117 and the reason for this change: *"Consider you are building this for me, not
/// for some unknown user."* He knows Enter sends. He knows what the box under a conversation
/// is for. A hint that is only news the first time is chrome on every frame after it.
///
/// 🚨 **A *dead* one still speaks, and the asymmetry is the rule rather than an exception.**
/// An empty box with no hint reads as *ready*; a **disabled** box with no hint reads as
/// broken, and the reason it is disabled is a fact about the world that no pixel carries.
/// So the live hint goes and this one stays — trimmed from a sentence to the label it always
/// was, since the composer it sits in is what "not running" is about.
const COMPOSER_HINT_DEAD: &str = "not running";

/// What an empty composer says, given whether the agent is alive.
///
/// A function rather than the conditional written at the widget, so the asymmetry
/// [`COMPOSER_HINT_DEAD`] argues for is something a test can hold rather than something a
/// reader has to find inside a builder chain.
fn composer_hint(live: bool) -> &'static str {
    if live {
        ""
    } else {
        COMPOSER_HINT_DEAD
    }
}

fn composer(ui: &mut egui::Ui, pane: &mut ConversationPane, theme: &Theme, theme_name: &str) {
    let live = pane.failure.is_none();
    // First of all, and before anything asks whether the panel is open: an edit made since
    // the last frame is what lets go of an Escape. See `ConversationPane::notice_edit`.
    pane.notice_edit();
    // 🚨 **Before the widget, necessarily.** egui hands each widget a *clone* of the event
    // list taken when the widget runs, so an event removed here is an event the `TextEdit`
    // never sees — and an event removed *after* it has already been acted on. Tab, the
    // arrows and Escape all have meanings inside a text box, and the console may only take
    // them in the states that earn them.
    // 🚨 **The editor is asked first, and it answers instead of the panel rather than as well
    // as it.** Both want Tab, the arrows and Escape, and only one of the two is ever on screen
    // — `command_panel` gives the band to the editor outright — so letting both read the same
    // frame's keys would move a highlight nobody can see.
    // 🚨 **…and only while this composer owns the keyboard.** Both readers below consume out
    // of the raw event list, so a region command line that had focus last frame would find its
    // Tab, Escape and arrows already gone. `keys` is the measurement that decides; it is `true`
    // for every console that has not divided its pane, which is what keeps invariant #4.
    if pane.keys {
        if pane.theme_edit.is_some() {
            theme_edit_keys(ui, pane);
        } else if live {
            composer_keys(ui, pane);
        }
    }
    // Three disjoint fields, borrowed separately, so the box can own the text while
    // `submit` still needs the whole pane afterwards. The id comes back out because the
    // caret is put right at the end of this function — see `put_caret_at_end`.
    let (submit, box_id) = composer_box(
        ui,
        &mut pane.composer,
        live,
        &mut pane.want_focus,
        &mut pane.composer_height,
        theme,
    );
    if submit {
        pane.submit(theme, theme_name);
    } else {
        // 🚨 **Insertion or deletion, decided here and nowhere else.** `notice_edit` above
        // synced `composer_seen` to the line as it stood at the *start* of this frame, and the
        // box has just written this frame's keystroke into `composer` — so this is the one
        // point in the pass where both halves of the edit exist. See `completion_held`.
        pane.completion_held =
            completion_held(&pane.composer_seen, &pane.composer, pane.completion_held);
        // 🚨 **Read here, before `palette_complete` rewrites the line.** This is "did the hand
        // touch the box this frame", and it is the whole of the settled-frame rule below;
        // asked after the completion it would be true of every frame a completion ran on,
        // which is the opposite of what it means. See `palette_autorun`.
        let edited = pane.composer_seen != pane.composer;
        // Both after the box, so they see the line as it stands *after* this frame's typing.
        // Completing first: `autorun` asks whether the line is now a whole command, and a line
        // one completion short of being one is exactly the case that has just been fixed.
        palette_complete(pane);
        palette_autorun(ui.ctx(), pane, edited, theme, theme_name);
    }
    // 🚨 **LAST, and that is the whole of the caret fix.** Every site that can rewrite the
    // line wholesale sets `want_caret` — the arrows' history walk and Tab's accept before the
    // box, the self-completion and autorun's accept after it — so the only place that can see
    // *all* of them is the end of the pass. Draining it here puts the caret at the end of the
    // line on the **same frame the line was rewritten**, so the next character typed lands
    // where a human is looking. ⚠️ Draining it into `composer_box` instead is what produced
    // `/hxelp`: the box runs before the completion does, so it could only ever honour the
    // *previous* frame's request, and by then this frame's keystroke had already been placed
    // at the stale index.
    if std::mem::take(&mut pane.want_caret) {
        put_caret_at_end(ui.ctx(), box_id, &pane.composer);
    }
}

/// Put egui's caret at the end of a text box's text.
///
/// 🚨 **This is state written *between* frames, which is why it works.** egui's `TextEdit`
/// keeps its cursor as an index of its own, loaded at the top of the widget and stored at the
/// bottom; a completion that replaces `/h` with `/help` leaves that index at 2. Writing the
/// state after the widget has run means the *next* frame's `TextEdit` loads a caret already at
/// the end, so the next character typed appends. Writing it before the widget runs would be
/// overwritten by the widget itself.
///
/// ⚠️ **No state, no caret, no error.** `load_state` answers `None` until the box has drawn at
/// least once — a dead pane, or the very first frame — and there is nothing to correct in that
/// case, so the miss is silent by construction rather than by suppression.
///
/// 🚨 **Shared with [`crate::region_line`]** for [`completion_held`]'s reason and one of its
/// own: the id is whatever the caller's box turned out to have, so nothing here is about the
/// composer in particular. #131 measured the region line without it — Tab on `add su` produced
/// `add surface ` with the caret still at 6, and the next two characters landed as
/// `add suXYrface `. The same defect the composer's `/hxelp` was, on a second surface.
pub(crate) fn put_caret_at_end(ctx: &egui::Context, id: egui::Id, text: &str) {
    let Some(mut state) = egui::TextEdit::load_state(ctx, id) else { return };
    let end = egui::text::CCursor::new(text.chars().count());
    state.cursor.set_char_range(Some(egui::text_selection::CCursorRange::one(end)));
    state.store(ctx, id);
}

/// Who owns Up and Down this frame.
///
/// 🚨 **One key, three meanings, resolved here rather than by whichever branch runs first.**
/// The panel walks its candidates with the arrows; the history walks itself with them; and a
/// multiline [`egui::TextEdit`] moves the caret between lines with them, which is what a
/// human writing a paragraph to an agent expects. The last of those is the one that must
/// never be taken by surprise, so it is the default and the other two have to earn the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowOwner {
    /// A panel is open and the arrows move its highlight.
    Panel,
    /// The command history: either a walk is already under way, or the box is empty and
    /// pressing Up is the only thing Up could mean.
    History,
    /// Nobody here. The keys fall through to the text box and move the caret.
    TextBox,
}

/// Read this frame's keys on the **editor's** behalf, consuming exactly the ones it acts on.
///
/// ⚠️ **State-conditional in the same way [`palette_keys`] is**, and for the same reason: an
/// editor is only open because somebody typed `/theme edit`, so nothing is taken from a hand
/// that has not opened one. ⚠️ It claims **no printing key and not Enter** — see
/// [`crate::theme_edit::edit_key`] — because the composer is still live underneath and a
/// message must remain sendable without closing the editor first.
fn theme_edit_keys(ui: &egui::Ui, pane: &mut ConversationPane) {
    if pane.theme_edit.is_none() {
        return;
    }
    let acts: Vec<EditKey> = ui.input_mut(|i| {
        let mut acts = Vec::new();
        i.events.retain(|event| {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else {
                return true;
            };
            match theme_edit::edit_key(*key, *modifiers) {
                EditKey::Ignore => true,
                act => {
                    acts.push(act);
                    false
                }
            }
        });
        acts
    });
    for act in acts {
        let Some(editor) = pane.theme_edit.as_mut() else { return };
        if !editor.key(act) {
            pane.theme_edit = None;
            // egui's focus manager already dropped the composer on Escape, before any of this
            // ran — `palette_key`'s note, and the same repair.
            pane.want_focus = true;
            return;
        }
    }
}

/// Decide who the arrows belong to, from the three facts that settle it.
///
/// The rule, in order:
///
/// 1. **A walk in progress keeps them.** Recalling `/theme dark` puts a command line in the
///    box, which opens a panel — so without this the second Up would move a highlight and
///    the walk would be one step deep for ever. A walk ends by editing the line, not by
///    changing the subject.
/// 2. **An open panel takes them next**, which is §1.9's rule unchanged.
/// 3. **An empty box hands them to history**, because an empty text box has no caret motion
///    to perform: Up there can only mean "what did I type before".
/// 4. **Otherwise the text box keeps them.** Prose, a half-written paragraph, a command line
///    whose panel was dismissed with Escape — in all three the caret is what Up is for, and
///    a history that stole the key would replace a message someone was writing.
///
/// ⚠️ **Case 4 covers the dismissed-panel line deliberately.** Escape means "stop showing me
/// this", not "hand my arrow keys to something else"; and the line in the box is text a hand
/// is working on, which is case 4's whole subject.
pub fn arrow_owner(walking: bool, panel_open: bool, composer_empty: bool) -> ArrowOwner {
    if walking {
        ArrowOwner::History
    } else if panel_open {
        ArrowOwner::Panel
    } else if composer_empty {
        ArrowOwner::History
    } else {
        ArrowOwner::TextBox
    }
}

/// Read this frame's keys on the console's behalf, consuming exactly the ones it acts on.
///
/// ⚠️ **State-conditional, and that is the whole of why it is safe.** Tab and Escape are
/// taken only while a panel is open — a panel is only open for a line beginning with `/`, so
/// a human typing prose keeps them entirely. The arrows are taken only when
/// [`arrow_owner`] says so.
///
/// ⚠️ **The raw key is carried alongside the act, and it has to be.** `palette_key` maps
/// **Shift+Tab** to [`PaletteKey::Prev`] — the same act ArrowUp produces — so routing on the
/// act alone would hand Shift+Tab to the history and let the panel's own key start walking
/// commands.
fn composer_keys(ui: &egui::Ui, pane: &mut ConversationPane) {
    let palette = pane.palette();
    let owner =
        arrow_owner(pane.walking(), palette.is_some(), pane.composer.is_empty());
    let acts: Vec<(egui::Key, PaletteKey)> = ui.input_mut(|i| {
        let mut acts = Vec::new();
        i.events.retain(|event| {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else {
                return true;
            };
            let act = palette_key(*key, *modifiers);
            if act == PaletteKey::Ignore {
                return true;
            }
            let ours = match key {
                egui::Key::ArrowUp | egui::Key::ArrowDown => owner != ArrowOwner::TextBox,
                _ => palette.is_some(),
            };
            if ours {
                acts.push((*key, act));
            }
            !ours
        });
        acts
    });
    for (key, act) in acts {
        let arrow = matches!(key, egui::Key::ArrowUp | egui::Key::ArrowDown);
        if arrow && owner == ArrowOwner::History {
            pane.history_step(key == egui::Key::ArrowUp);
            continue;
        }
        match act {
            PaletteKey::Next => {
                let len = palette.as_ref().map_or(0, |p| p.candidates.len());
                pane.palette_selected = move_selection(pane.palette_selected, len, true);
            }
            PaletteKey::Prev => {
                let len = palette.as_ref().map_or(0, |p| p.candidates.len());
                pane.palette_selected = move_selection(pane.palette_selected, len, false);
            }
            PaletteKey::Accept => {
                if let Some(candidate) =
                    palette.as_ref().and_then(|p| p.candidates.get(pane.palette_selected))
                {
                    pane.accept(candidate);
                }
            }
            PaletteKey::Dismiss => {
                pane.palette_dismissed = true;
                // egui's focus manager already dropped the composer before this code ran —
                // see `palette_key`. Asking for it back lands next frame.
                pane.want_focus = true;
            }
            PaletteKey::Ignore => {}
        }
    }
}

/// The most completions one frame may chain.
///
/// 🚨 **A bound rather than a `loop`**, and it is not defensive tidiness. Accepting rewrites
/// the line, and the new line may have a lone candidate of its own: `/pos` becomes
/// `/posture `, which — for a verb with one posture — would become `/posture desktop`. That
/// cascade is wanted. What it must not be able to do is spin: `sole_completion` already
/// refuses a candidate that would rewrite the line to itself, so a cycle would need two
/// completions that alternate, which the registry has no way to produce today and which a
/// future `Choice` table has no way to be trusted not to. Four is well past the deepest ring
/// the table has (verb → value → keyword → value).
const PALETTE_COMPLETE_STEPS: usize = 4;

/// 🚨 **THE RULE: complete on insertion, never on deletion.**
///
/// James, on a running build, 2026-08-14: *"once I have typed slash surface, I am no longer
/// able to backspace out of it."* Deleting from `/surface` leaves `/surfac`, whose only
/// candidate is still `surface`, whose completion is `/surface` — so the very deletion that
/// was just made was put straight back, on the same frame, for ever. It trapped every verb
/// reachable by a unique prefix and every value once its prefix was unique, and the only way
/// out of a mistyped command was to select the whole line and start again.
///
/// ⚠️ **It was worse than an undo, and the measurement is worth keeping.** Accepting a
/// completion rewrites the whole line and puts the caret at its end, so the *next* backspace
/// deletes from wherever that left it. Eight backspaces on `/surface`, driven through real
/// frames with the guard below removed, produced `/surface`, `/surfae`, `/surface`, `/surfce`,
/// `/surfc`, `/surface`, `/surace`, `/surac` — the line does not merely refuse to shorten, it
/// loses characters out of the middle of the word.
///
/// The resolution is the one every editor uses and it is stated as a rule about the *edit*
/// rather than about the line: a completion is something typing earns, so **a frame that added
/// text may complete and a frame that removed text may not**. `before` is the composer as it
/// stood when this frame began, `after` is the same line once the box has written this frame's
/// keystroke into it, and `held` is the answer from last time.
///
/// 🚨 **Held, not merely refused, and this is the part a one-frame test cannot see.** The frame
/// *after* a backspace is a frame in which nothing changed at all — so a rule that only refused
/// shrinking frames would complete on that next one, and the deletion would be undone one frame
/// later instead of immediately. That is the same bug at 60 Hz, presenting as a flicker.
/// A deletion therefore *latches* completion off, and only an insertion lets it go again.
///
/// # What this deliberately does not cover
///
/// The measure is the line's **length in bytes**, which answers "did this frame add text" and
/// nothing finer. Three cases are therefore classified by what they did to the length rather
/// than by what they were, and all three are stated rather than defended:
///
/// - **A paste that replaces a long line with a shorter one** reads as a deletion, so it does
///   not complete. It is an insertion by intent and a shortening in fact.
/// - **Select-all, then type one character** reads as a deletion for the same reason. The
///   pasted or typed text is still in the box either way; the *next* inserted character
///   releases the latch and completion resumes, so the cost is bounded at one keystroke and
///   there is no state to get stuck in.
/// - **A same-length replacement** (`/theme dark` pasted over `/theme edit`) changes nothing
///   about the latch, and completes or not according to whatever the previous edit was.
///
/// What it *does* cover is the case the bug was: a deletion that lands on a line whose sole
/// candidate now differs from it. That is refused, however many characters are left, all the
/// way back to a bare `/` and out of the line entirely.
///
/// ⚠️ **The first frame of a line that was never typed completes.** A composer set wholesale —
/// by a test, by a recall, by anything that is not a keystroke — arrives with `before` equal to
/// `after`, so the latch keeps whatever it held, and a fresh pane holds `false`. A line that
/// appears out of nowhere has not been deleted from, so it is not treated as though it had.
///
/// 🚨 **Shared with [`crate::region_line`], which is the whole reason it is `pub(crate)`.** That
/// control gained self-completion in #131 and inherited this rule with it — and a second copy
/// would be a second answer to *"may this frame complete?"*, which is the one question the two
/// surfaces must never disagree about. The measurements above were taken here and hold there.
pub(crate) fn completion_held(before: &str, after: &str, held: bool) -> bool {
    match after.len().cmp(&before.len()) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => held,
    }
}

/// Take the completion when there is only one, with no Tab.
///
/// James, 2026-08-14: *"when I type slash p [Tab] d so that it narrows down to just one
/// choice, 'desktop', Do not show me the single choice like you currently do. Simply complete
/// the completion because it's the only option."*
///
/// 🚨 **This completes and never runs**, which is the whole distinction from
/// [`palette_autorun`] below. This one rewrites the composer for **every** verb; autorun
/// submits, and only for the verbs a second command can take back. They are separate
/// mechanisms with separate switches, and the fact that a completion may hand autorun a line
/// it then runs is a *chain*, not a merge — `/su` completes to `/surface` here and stops
/// there, because `surface` is not something autorun may fire.
///
/// ⚠️ **Escape suppresses it**, for free and correctly: `pane.palette()` answers `None` while
/// the panel is dismissed, so a human who has shut the panel is not having their line
/// rewritten behind it.
///
/// ⚠️ **What it buys beyond the keystroke.** `/portal` puts `portal` in the *verb* slot —
/// `candidates` reads a line with no trailing space as "still typing this word" — so a verb
/// whose arguments are its entire point offered none of them until a space was typed. James
/// hit exactly that. `verb_candidate` gives a verb-with-arguments a trailing space in its
/// completion, so completing the lone `portal` is what opens the argument ring at all.
fn palette_complete(pane: &mut ConversationPane) {
    // 🚨 **Never on a deletion.** Asked once, outside the loop: the cascade is one insertion's
    // consequence, and re-asking it per step would let a completion argue with the edit that
    // started it. See `completion_held` — this is the whole of the backspace fix.
    if pane.completion_held {
        return;
    }
    for _ in 0..PALETTE_COMPLETE_STEPS {
        let Some(palette) = pane.palette() else { return };
        let Some(only) = palette.sole_completion(&pane.composer) else { return };
        let only = only.clone();
        pane.accept(&only);
    }
}

/// What `ORGANON_PALETTE_AUTORUN` means, as a pure function of the value so it can be pinned
/// without a test writing to the process environment.
///
/// 🚨 **The default is ON**, which it was not until the recoverability term joined
/// [`Palette::autorun`]'s rule. ⚠️ **The escape hatch is `=0`, and the variable's existing
/// spelling keeps its existing meaning**: anyone who set `=1` to switch autorun on still gets
/// autorun. Inverting the sense of a variable already in somebody's environment — making `=1`
/// mean *off* — is the trap this avoids; renaming it would have been the other way to avoid it,
/// at the cost of a name in a shell profile silently doing nothing.
fn autorun_enabled(var: Option<&str>) -> bool {
    var != Some("0")
}

/// Run a command the panel is certain of **and can afford to be wrong about**, with no Enter.
///
/// 🚨 The certainty is [`Palette::autorun`]'s — one candidate, it completes, and the command
/// it completes to is recoverable — and the switch is the pane's; this is only the wiring
/// between them, plus the one thing neither of them can see: whether the hand has stopped.
///
/// 🚨 **It waits for a settled frame, and that is not a nicety.** `edited` says the composer
/// changed on *this* frame, and a fire is refused while it is true, so the earliest a command
/// can run is the first frame in which nothing was typed. Two reasons, and the second is the
/// one that matters:
///
/// - The completed line is **drawn at least once** before it disappears. Firing on the same
///   frame as the keystroke means the human never sees what ran.
/// - A keystroke arriving while the fire is pending **cancels** it rather than racing it.
///   Typing `su` fast used to be `/s` → fire, with the `u` landing in whatever the composer
///   became; now the `u` arrives on the frame the fire was waiting for, re-asks the palette,
///   and the answer is simply different. ⚠️ This is a separate mechanism from the caret, which
///   `put_caret_at_end` puts at the end of the line on the rewrite's own frame — the wait is
///   about *when a command runs*, not about where the next character goes.
///
/// ⚠️ **A settled frame has to be made to happen.** egui repaints on input, so on a keystroke
/// that settles the line there may be no next frame until something else moves the mouse — and
/// the command would run minutes later, which is worse than either extreme. The repaint is
/// requested explicitly when a fire is deferred. ⚠️ It is requested *only* then: an
/// unconditional request would turn the composer into a 60 Hz spinner for a feature that is
/// idle almost always.
///
/// 🚨 **It obeys [`completion_held`] too, and here the stake is higher than a rewritten line.**
/// Backspacing `/theme dark` to `/theme dar` leaves one candidate that *completes*, so this
/// would have **run the command** on a keystroke that was trying to erase it — the same defect
/// as the completion trap, one consequence worse. Deleting is never an instruction to act.
fn palette_autorun(
    ctx: &egui::Context,
    pane: &mut ConversationPane,
    edited: bool,
    theme: &Theme,
    theme_name: &str,
) {
    if pane.completion_held {
        return;
    }
    let Some(palette) = pane.palette() else { return };
    let Some(candidate) = palette.autorun(pane.autorun) else { return };
    if edited {
        // The line is ready and the hand is not. Come back next frame and ask again — with no
        // memory of this one, so a keystroke arriving meanwhile simply changes the answer.
        ctx.request_repaint();
        return;
    }
    let candidate = candidate.clone();
    pane.accept(&candidate);
    pane.submit(theme, theme_name);
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
///
/// 🚨 **`lock_focus(true)` is what makes Tab available to the command panel at all**, and it
/// is not a preference. egui's focus manager reads Tab out of the *raw* input in
/// `Focus::begin_pass`, before any of this file runs, so consuming the event here is too late
/// to stop focus leaving for whatever button the scrollback drew — the keystrokes after it
/// would go somewhere invisible. `lock_focus` sets `EventFilter::tab`, which is the flag that
/// pass tests, so focus stays put whether or not the panel takes the key. ⚠️ The visible
/// consequence when the panel is *shut*: Tab now indents the message rather than moving
/// focus, which is what a text box does everywhere else.
///
/// Returns whether this frame asked to send, **and the `TextEdit`'s id**. The id is the one
/// thing about the widget its caller cannot derive: a completion replaces the whole line and
/// egui's cursor is an index it keeps itself, so somebody has to put that cursor back at the
/// end afterwards. This box deliberately knows nothing about completions, so it hands out the
/// id and [`composer`] does it — see [`put_caret_at_end`]. ⚠️ Doing it *in here* is what this
/// function used to do, and it could not work: the box runs before the completion does, so the
/// only request it could honour was the previous frame's.
fn composer_box(
    ui: &mut egui::Ui,
    text: &mut String,
    live: bool,
    want_focus: &mut bool,
    measured: &mut f32,
    theme: &Theme,
) -> (bool, egui::Id) {
    let row = ui.text_style_height(&egui::TextStyle::Monospace);
    // The floor is what makes the box big before anything is in it; the ceiling is what
    // stops a pasted essay from eating the scrollback.
    let inner = measured.clamp(row * COMPOSER_ROWS as f32, row * COMPOSER_MAX_ROWS);
    let band = inner + 2.0 * COMPOSER_PAD_Y as f32 + 2.0 * COMPOSER_STROKE;

    let placed = ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), band),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            // `begin`/`end` rather than `show`, so the edge can be coloured by a focus state
            // that is only known once the widget inside has run. The frame shape is inserted
            // behind the content either way, so this costs nothing.
            let mut framed = Frame::new()
                .fill(theme.composer_fill)
                .stroke(egui::Stroke::new(COMPOSER_STROKE, theme.composer_edge))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(COMPOSER_PAD_X, COMPOSER_PAD_Y))
                .begin(ui);

            let (submit, focused, id) = {
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
                            .text_color(if live { theme.human_text } else { theme.dim })
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
                            // See this function's doc: the flag the FOCUS manager tests, not
                            // a taste about indenting.
                            .lock_focus(true)
                            // Empty while live — see [`COMPOSER_HINT_DEAD`] for why only the
                            // disabled box carries one.
                            .hint_text(composer_hint(live));
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
                        (submit, focused, response.id)
                    });
                *measured = scrolled.content_size.y;
                scrolled.inner
            };

            framed.frame.stroke = egui::Stroke::new(
                COMPOSER_STROKE,
                match (live, focused) {
                    (false, _) => theme.composer_edge_dead,
                    (true, true) => theme.composer_edge_focus,
                    (true, false) => theme.composer_edge,
                },
            );
            framed.end(ui);
            (submit, id)
        },
    );
    // 🚨 **Published so the invariant can be MEASURED.** "The entry box never moves" was stated
    // in prose and was false for the whole life of #127; a property nothing reads is a property
    // nobody notices breaking. See [`composer_rect`].
    let rect = placed.response.rect;
    ui.ctx().data_mut(|d| d.insert_temp(composer_rect_id(), rect));
    placed.inner
}

/// The id [`composer_box`] files its rect under, and [`composer_rect`] reads it back from.
fn composer_rect_id() -> egui::Id {
    egui::Id::new("organon-console-composer-rect")
}

/// **Where the entry box was drawn on the last frame**, or `None` before it has been drawn.
///
/// 🚨 **This exists to make one sentence checkable.** James, 2026-08-21: *"The entry box should
/// never move."* [`draw`]'s layout is what makes that true — everything that can appear or vanish
/// is on one side or the other of the composer, and the status log's drop-down is a layer rather
/// than a child — but the layout is four nested closures and the property is not visible in any
/// one of them. So the box states where it landed, and
/// [`tests::the_entry_box_never_moves_when_the_status_log_opens`] compares that rect with the log
/// closed against open, at more than one pane height.
///
/// ⚠️ **One id for the process, not one per pane**, which is right for what it is used for and
/// would be wrong for anything else: a console divided into regions draws several composers and
/// the last one wins. It is a measurement hook, never a layout input — nothing in the draw path
/// reads it, so a stale or contested value cannot move anything.
pub fn composer_rect(ctx: &egui::Context) -> Option<egui::Rect> {
    ctx.data(|d| d.get_temp::<egui::Rect>(composer_rect_id()))
}

// ---------------------------------------------------------------------------
// The pure part — clipping and field extraction, tested headless
// ---------------------------------------------------------------------------

/// One `Edit` call, as a card draws it: the file it touches, and the aligned diff.
///
/// The alignment itself is [`crate::text_diff`]'s — a module with no egui in it, so the
/// part that can be *wrong* is tested with plain strings rather than by looking at a card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditDiff {
    pub path: String,
    pub diff: LineDiff,
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
        diff: text_diff::line_diff(old, new),
    })
}

/// What a diff says about *itself*, above its rows — the size of the change, and each
/// reason a card might have to show less than the whole of it.
///
/// Pure, and separate from [`diff_body`] for the reason every judgment in this file is:
/// "an identical pair reads as *no change* rather than as the block twice" is a sentence
/// a test can hold, and a `ui.label` inside a draw call is not.
///
/// ⚠️ **Order is deliberate.** The reason a diff looks strange comes before its arithmetic:
/// a reader whose rows are visibly identical needs "whitespace only" first, and the counts
/// afterwards.
pub fn diff_notes(diff: &LineDiff) -> Vec<String> {
    if diff.unchanged {
        return vec!["no change — old_string and new_string are identical".to_string()];
    }
    let mut notes = Vec::new();
    if diff.whitespace_only {
        notes.push("whitespace only — no visible character differs".to_string());
    }
    if let Some((old_lines, new_lines)) = diff.declined {
        notes.push(format!(
            "not aligned — {old_lines} lines against {new_lines} is past the diff budget"
        ));
    }
    if diff.has_changes() {
        notes.push(format!("{} removed, {} added", diff.removed, diff.added));
    }
    notes
}

/// What a tool said about its own result, as rows for the card — the sibling object a
/// terminal never sees at all.
///
/// 🚨 **The honesty rule governs this absolutely, and it is what makes the function
/// short.** [`ResultDetail`] carries only fields a real capture contains, so there is
/// nothing here to invent; what is left is deciding what is worth *repeating*.
///
/// Two rules, and each one is about not saying a thing twice:
///
/// 1. **The path is shown only when the arguments do not already state it.** A `Read`
///    card already prints `file_path: …` as an argument field, and a second copy under it
///    is pure noise. ⚠️ The case this preserves is the **orphan** card — a result whose
///    call was never seen has no arguments at all, and then the detail's path is the only
///    thing on the card that says what the tool touched.
/// 2. **The counts are one row, and only when both halves are there.** `4 lines` alone
///    says nothing a person wants; `4 of 4` and `4 of 900` are the fact. A `startLine`
///    is appended only when it is not the first line, because "from line 1" is what
///    reading a file means.
pub fn detail_rows(detail: &ResultDetail, args: &Arguments) -> Vec<String> {
    let mut rows = Vec::new();
    if let Some(path) = &detail.file_path {
        let stated = argument_fields(args).into_iter().any(|(_, value)| &value == path);
        if !stated {
            rows.push(path.clone());
        }
    }
    if let (Some(lines), Some(total)) = (detail.lines, detail.total_lines) {
        let mut row = format!("{lines} of {total} lines");
        if let Some(start) = detail.start_line.filter(|s| *s > 1) {
            row.push_str(&format!(", from line {start}"));
        }
        rows.push(row);
    }
    rows
}

/// The first `max` lines of `text`, and how many were left behind.
pub fn clip_lines(text: &str, max: usize) -> (Vec<&str>, usize) {
    let total = text.lines().count();
    (text.lines().take(max).collect(), total.saturating_sub(max))
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

    // -- the progress row ----------------------------------------------------

    /// A progress value with the capture's own first-card numbers on it.
    fn captured_progress() -> SubagentProgress {
        SubagentProgress {
            description: Some("Reading one.txt".into()),
            last_tool: Some("Read".into()),
            tool_uses: Some(1),
            total_tokens: Some(62949),
            duration_ms: Some(10335),
            status: Some("completed".into()),
        }
    }

    /// **CONTRACT** — the row a card shows for a working agent, in the order it reads:
    /// what it is doing, then what it has measurably done.
    #[test]
    fn the_progress_row_reports_the_activity_then_the_measured_trailer() {
        let summary = progress_summary(&captured_progress()).expect("a reported progress");
        assert_eq!(summary.headline.as_deref(), Some("Reading one.txt"));
        assert_eq!(summary.facts.as_deref(), Some("→ Read · 1 tool · 10.3s · 62,949 tokens"));
        assert_eq!(summary.status.as_deref(), Some("completed"));
    }

    /// A card whose harness has said nothing shows no row at all — the same rule
    /// [`ResultDetail`] follows, and the reason `SubagentProgress` is not an `Option`.
    #[test]
    fn a_card_with_nothing_reported_draws_no_progress_row() {
        assert_eq!(progress_summary(&SubagentProgress::default()), None);
    }

    /// **CONTRACT** — only the parts the harness stated appear. A `task_started` carries a
    /// description and no counts at all, and the row must not print zeros for the rest:
    /// a `0 tools` that means "nobody said" is a measurement the wire never made.
    #[test]
    fn an_unreported_fact_is_absent_rather_than_zero() {
        let started = SubagentProgress {
            description: Some("Read one.txt second line".into()),
            ..SubagentProgress::default()
        };
        let summary = progress_summary(&started).expect("a description is enough");
        assert_eq!(summary.headline.as_deref(), Some("Read one.txt second line"));
        assert_eq!(summary.facts, None, "no counts were stated, so none are shown");
        assert_eq!(summary.status, None);
    }

    /// Tenths under a minute, whole seconds over it, and **truncating** either way — the
    /// same never-overstate rule `ContextFill::percent` argues. A 16-minute agent does not
    /// want a tenth, and a 900 ms step does.
    #[test]
    fn elapsed_reads_in_tenths_below_a_minute_and_never_rounds_up() {
        assert_eq!(elapsed(7890), "7.8s", "7.89s must not read as 7.9");
        assert_eq!(elapsed(10335), "10.3s");
        assert_eq!(elapsed(900), "0.9s");
        assert_eq!(elapsed(0), "0.0s");
        assert_eq!(elapsed(59_999), "59.9s", "the last moment below the switch");
        assert_eq!(elapsed(60_000), "1m 0s");
        assert_eq!(elapsed(963_000), "16m 3s", "the coordinator case this exists for");
    }

    /// A token count is a measurement, so it is grouped rather than abbreviated: `61.5k`
    /// would be this view rounding a number the harness stated exactly.
    #[test]
    fn token_counts_are_grouped_exactly_rather_than_abbreviated() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(62949), "62,949");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    /// Singular reads as one, as it does in the subagent header beside it.
    #[test]
    fn one_tool_is_not_one_tools() {
        let one = SubagentProgress { tool_uses: Some(1), ..SubagentProgress::default() };
        assert_eq!(progress_summary(&one).unwrap().facts.as_deref(), Some("1 tool"));
        let two = SubagentProgress { tool_uses: Some(2), ..SubagentProgress::default() };
        assert_eq!(progress_summary(&two).unwrap().facts.as_deref(), Some("2 tools"));
    }

    /// 🚨 CONTRACT — **every glyph the row draws must be one Hack actually has.** The
    /// subagent step marker shipped as tofu for exactly this reason (`step_mark` carries
    /// the `cmap` measurement); the row reuses `→` and `·` and introduces nothing new, so
    /// this test is the guard on that promise rather than a fresh measurement.
    #[test]
    fn the_progress_row_introduces_no_glyph_the_step_marks_have_not_proved() {
        let summary = progress_summary(&captured_progress()).expect("a reported progress");
        let drawn = format!(
            "{}{}{}",
            summary.headline.unwrap_or_default(),
            summary.facts.unwrap_or_default(),
            summary.status.unwrap_or_default()
        );
        let proved = ['→', '·'];
        for ch in drawn.chars() {
            assert!(
                ch.is_ascii() || proved.contains(&ch),
                "{ch:?} (U+{:04X}) is neither ASCII nor a measured-present symbol — see \
                 `step_mark` before adding one",
                ch as u32
            );
        }
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
        // Aligned, so the shared first line is context and not a removal-plus-addition.
        assert_eq!(
            diff.diff.rows,
            vec![
                DiffRow::Context("let a = 1;".into()),
                DiffRow::Removed("let b = 2;".into()),
                DiffRow::Added("let b = 3;".into()),
                DiffRow::Added("let c = 4;".into()),
            ]
        );
        assert_eq!((diff.diff.removed, diff.diff.added), (1, 2));
    }

    /// **CONTRACT.** The notes above the rows are the diff's own account of itself, in the
    /// order a confused reader needs them: why it looks odd, then how big it is.
    #[test]
    fn a_diff_reports_its_own_size_and_every_reason_it_shows_less() {
        let plain = text_diff::line_diff("a\nb", "a\nc");
        assert_eq!(diff_notes(&plain), vec!["1 removed, 1 added"]);

        let identical = text_diff::line_diff("a\nb", "a\nb");
        assert_eq!(
            diff_notes(&identical),
            vec!["no change — old_string and new_string are identical"],
            "and no count line, because there is nothing to count"
        );

        let reindent = text_diff::line_diff("  x", "    x");
        assert_eq!(
            diff_notes(&reindent).first().map(String::as_str),
            Some("whitespace only — no visible character differs"),
            "the explanation comes before the arithmetic"
        );
    }

    /// **CONTRACT.** A diff past the alignment budget says so on the card, naming the two
    /// sizes — the house rule that what does not work is as visible as what does.
    #[test]
    fn a_declined_alignment_is_stated_on_the_card_with_its_sizes() {
        let old = (1..=200).map(|i| format!("old {i}")).collect::<Vec<_>>().join("\n");
        let new = (1..=200).map(|i| format!("new {i}")).collect::<Vec<_>>().join("\n");
        let notes = diff_notes(&text_diff::line_diff(&old, &new));
        assert!(
            notes.iter().any(|n| n == "not aligned — 200 lines against 200 is past the diff budget"),
            "{notes:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Card density, through the real `scrollback`. [`crate::card_density`]'s own tests hold
    // the judgments; these hold the **wiring** — that the collapse gate is fed the pane's
    // scroll state and not `true`, and that the map is pruned like every other side map.
    // Both are things a green build cannot tell you.

    /// A pane holding one finished tool call, plus the element's id.
    fn pane_with_finished_call(name: &str, is_error: bool) -> (ConversationPane, ElementId) {
        pane_with_finished_call_capped(name, is_error, Transcript::new())
    }

    fn pane_with_finished_call_capped(
        name: &str,
        is_error: bool,
        transcript: Transcript,
    ) -> (ConversationPane, ElementId) {
        let mut pane = rewrap_bench::bench_pane(transcript);
        let id = crate::conversation::ToolId("toolu_one".into());
        pane.absorb(AgentEvent::ToolCall {
            id: id.clone(),
            name: name.into(),
            arguments: Some(r#"{"file_path":"src/lib.rs"}"#.to_string()),
        });
        pane.absorb(AgentEvent::ToolResult {
            id,
            output: "a\nb\nc".into(),
            is_error,
            detail: ResultDetail::default(),
        });
        let element = pane.transcript.elements()[0].id;
        (pane, element)
    }

    /// 🚨 **CONTRACT: the collapse gate is fed the pane's own scroll state.**
    ///
    /// This is the wiring half of the scroll-stability argument — the pure half is
    /// `card_density::nothing_settles_while_a_reader_is_scrolled_up`. A `scrollback` that
    /// passed `true` here would compile, pass every unit test in that module, and jump a
    /// reader's view every time a long-running call finished above them.
    #[test]
    fn a_card_collapses_only_while_the_view_is_following_the_live_edge() {
        let (mut pane, id) = pane_with_finished_call("Read", false);
        pane.pinned = false;
        draw_once(&mut pane);
        assert!(pane.density.is_open(id), "a reader scrolled up sees no height change");

        pane.pinned = true;
        draw_once(&mut pane);
        assert!(!pane.density.is_open(id), "back at the live edge, it settles");
    }

    /// 🚨 **CONTRACT: a failure keeps its weight even at the live edge.** The one card the
    /// density rule must never touch, checked through the same path that collapses the
    /// others.
    #[test]
    fn a_failed_card_is_never_collapsed_by_the_walk() {
        let (mut pane, id) = pane_with_finished_call("Bash", true);
        pane.pinned = true;
        for _ in 0..3 {
            draw_once(&mut pane);
            assert!(pane.density.is_open(id), "a failure stays open, bordered and loud");
        }
    }

    /// The same `retain` line every other side map gets, for the same reason.
    #[test]
    fn a_card_the_cap_evicted_takes_its_density_with_it() {
        let capped = Transcript::with_limits(crate::conversation::Limits {
            max_elements: 2,
            ..crate::conversation::Limits::default()
        });
        let (mut pane, id) = pane_with_finished_call_capped("Read", false, capped);
        pane.pinned = true;
        draw_once(&mut pane);
        assert!(!pane.density.is_open(id), "the card settled, so there is state to lose");

        // Push past the cap; the transcript evicts from the front, taking the card with it.
        // ⚠️ Ids are never reused, so the check below cannot be satisfied by a collision.
        pane.absorb(AgentEvent::HumanInput { text: "one".into() });
        pane.absorb(AgentEvent::HumanInput { text: "two".into() });
        assert!(pane.transcript.get(id).is_none(), "the element really is gone");
        draw_once(&mut pane);
        assert!(pane.density.is_open(id), "an evicted element's density is forgotten");
    }

    /// 🚨 **CONTRACT: an approval and the result it authorises stay linked.** The `toolu_` id
    /// is the only thing joining them (`doc/console_approval_protocol.md` §3), so the gated
    /// call keeps a row of its own — never inside a group, where the id would not be drawn.
    #[test]
    fn an_approval_and_the_result_it_authorised_stay_linked() {
        let mut pane = rewrap_bench::bench_pane(Transcript::new());
        pane.transcript.insert_approval(ApprovalBlock {
            tool_name: "Bash".into(),
            input: r#"{"command":"ls"}"#.into(),
            tool_use_id: "toolu_gated".into(),
            state: ApprovalState::Pending,
        });
        for (index, id) in ["toolu_gated", "toolu_b", "toolu_c", "toolu_d"].iter().enumerate() {
            let id = crate::conversation::ToolId((*id).into());
            pane.absorb(AgentEvent::ToolCall {
                id: id.clone(),
                name: format!("Tool{index}"),
                arguments: Some("{}".to_string()),
            });
            pane.absorb(AgentEvent::ToolResult {
                id,
                output: String::new(),
                is_error: false,
                detail: ResultDetail::default(),
            });
        }
        pane.pinned = true;
        draw_once(&mut pane);

        let elements = pane.transcript.elements();
        let gated = card_density::gated_calls(elements.iter());
        assert!(gated.contains("toolu_gated"), "the id comes off the approval card itself");
        let rows =
            card_density::plan(&card_density::slots(elements.iter(), &pane.density, &gated));
        // The approval is row 0, the gated call keeps row 1 to itself, and only the three
        // ungated calls behind it become a group.
        assert_eq!(
            rows,
            vec![
                Row::One(0),
                Row::One(1),
                Row::Group { key: elements[2].id, start: 2, len: 3 },
            ]
        );
    }

    // -----------------------------------------------------------------------
    // The diff cache. See [`ConversationPane::diffs`] for what it is for and
    // `conversation_view/edit_diff_bench.rs` for what it saves.

    /// The `Edit` arguments a cache test edits, so the two strings differ in one line.
    fn edit_args(old: &str, new: &str) -> String {
        serde_json::json!({ "file_path": "src/lib.rs", "old_string": old, "new_string": new })
            .to_string()
    }

    /// Draw one frame of `pane`'s scrollback, which is what fills the cache.
    ///
    /// The pane driver is `edit_diff_bench`'s, shared rather than copied — see
    /// [`rewrap_bench::bench_pane`].
    fn draw_once(pane: &mut ConversationPane) {
        let ctx = egui::Context::default();
        edit_diff_bench::frame(
            &ctx,
            pane,
            &SurfaceImages::new(),
            &Theme::organon(),
            edit_diff_bench::Cache::On,
        );
    }

    /// A pane holding one `Edit` card whose arguments are `args`, plus the element's id.
    fn pane_with_edit(args: &str) -> (ConversationPane, ElementId) {
        let mut pane = rewrap_bench::bench_pane(Transcript::new());
        pane.absorb(AgentEvent::ToolCall {
            id: crate::conversation::ToolId("t1".into()),
            name: "Edit".into(),
            arguments: Some(args.to_string()),
        });
        let id = pane.transcript.elements()[0].id;
        (pane, id)
    }

    /// **CONTRACT.** What the cache holds is exactly what [`edit_diff`] would have returned.
    ///
    /// The whole claim of the cache is that it changes *when* the work happens and never
    /// *what* it produces, so this is the property the saving is only worth having if it
    /// holds.
    #[test]
    fn a_cached_diff_is_what_edit_diff_would_have_returned() {
        let args = edit_args("let a = 1;\nlet b = 2;", "let a = 1;\nlet b = 3;");
        let (mut pane, id) = pane_with_edit(&args);
        assert!(pane.diffs.is_empty(), "nothing is computed before a frame asks for it");
        draw_once(&mut pane);
        let cached = pane.diffs.get(&id).expect("the frame must have cached this card");
        assert_eq!(cached, &edit_diff(Some("Edit"), &complete(&args)), "the cache must not lie");
        assert!(cached.is_some(), "a complete Edit has a diff, so this is not vacuous");
    }

    /// 🚨 **CONTRACT, and the one that makes the cache safe: replacing a card's complete
    /// arguments replaces its diff.**
    ///
    /// This is not hypothetical and it is why the cache is invalidated by eviction rather
    /// than by any test on the arguments themselves. `Arguments::complete` is **not** a
    /// promise of immutability — a second `ToolCall` for an id that is not yet *resolved*
    /// overwrites the text wholesale ([`crate::conversation::Transcript::apply`]) — so a
    /// cache keyed on "complete" alone would show the first arguments' diff forever, under
    /// a card displaying the second arguments' path.
    #[test]
    fn replacing_complete_arguments_replaces_the_cached_diff() {
        let first = edit_args("let a = 1;", "let a = 2;");
        let (mut pane, id) = pane_with_edit(&first);
        draw_once(&mut pane);
        let before = pane.diffs.get(&id).cloned().expect("cached");

        let second = edit_args("let z = 9;", "let z = 8;");
        pane.absorb(AgentEvent::ToolCall {
            id: crate::conversation::ToolId("t1".into()),
            name: "Edit".into(),
            arguments: Some(second.clone()),
        });
        assert!(
            !pane.diffs.contains_key(&id),
            "the fold reported an update and the entry must be gone before the next frame"
        );

        draw_once(&mut pane);
        let after = pane.diffs.get(&id).cloned().expect("recached");
        assert_ne!(after, before, "the card is showing a stale diff of arguments it no longer has");
        assert_eq!(after, edit_diff(Some("Edit"), &complete(&second)));
    }

    /// **CONTRACT.** A streaming call caches its `None` and picks up a diff the moment its
    /// arguments settle.
    ///
    /// The `None` is cached on purpose — it is what stops a half-arrived `Edit` being
    /// re-asked on every frame while it streams — and this pins that it is not *sticky*.
    #[test]
    fn a_streaming_card_caches_no_diff_and_gains_one_when_its_arguments_settle() {
        let id = crate::conversation::ToolId("t1".into());
        let mut pane = rewrap_bench::bench_pane(Transcript::new());
        pane.absorb(AgentEvent::ToolCall { id: id.clone(), name: "Edit".into(), arguments: None });
        let element = pane.transcript.elements()[0].id;
        pane.absorb(AgentEvent::ToolArgumentsDelta { id: id.clone(), fragment: "{\"old_st".into() });
        draw_once(&mut pane);
        assert_eq!(
            pane.diffs.get(&element),
            Some(&None),
            "half a JSON document must cache the absence of a diff, not a diff"
        );

        let settled = edit_args("let a = 1;", "let a = 2;");
        pane.absorb(AgentEvent::ToolCall {
            id,
            name: "Edit".into(),
            arguments: Some(settled.clone()),
        });
        draw_once(&mut pane);
        assert_eq!(
            pane.diffs.get(&element),
            Some(&edit_diff(Some("Edit"), &complete(&settled))),
            "a cached None outlived the arguments arriving"
        );
    }

    /// **CONTRACT.** A card the transcript's cap evicted takes its cached diff with it.
    ///
    /// The same line, and the same reason, as the `artifacts` retain beside it: a side map
    /// on a session that runs all day leaks unless something prunes it against the
    /// transcript. Worth its own test here because a diff entry is far larger than a
    /// `PanelState` — an `Edit` card's arguments can be tens of kilobytes.
    ///
    /// ⚠️ **Driven through the real cap** — a two-element [`Limits`] and enough cards to
    /// overflow it — rather than by assigning a fresh [`Transcript`] over the pane's. The
    /// short version would have proved the `retain` fires and nothing about the mechanism
    /// that actually removes elements: eviction is from the **front**, one at a time, while
    /// the pane keeps running and `next_element` keeps climbing. Replacing the transcript
    /// wholesale is something no code in this crate does, and it restarts the id counter,
    /// so it would have tested a pruning story that cannot occur.
    #[test]
    fn a_card_the_cap_evicted_takes_its_cached_diff_with_it() {
        let mut pane = rewrap_bench::bench_pane(Transcript::with_limits(
            crate::conversation::Limits { max_elements: 2, ..Default::default() },
        ));
        let mut ids = Vec::new();
        for n in 0..4 {
            pane.absorb(AgentEvent::ToolCall {
                id: crate::conversation::ToolId(format!("t{n}")),
                name: "Edit".into(),
                arguments: Some(edit_args(&format!("let a = {n};"), &format!("let a = {n}1;"))),
            });
            ids.push(pane.transcript.elements().back().expect("a card").id);
            draw_once(&mut pane);
        }
        assert_eq!(pane.transcript.elements().len(), 2, "the cap must actually have evicted");
        assert_eq!(pane.diffs.len(), 2, "one cached diff per surviving card, and no more");
        for gone in &ids[..2] {
            assert!(!pane.diffs.contains_key(gone), "an evicted card left its diff behind");
        }
        for kept in &ids[2..] {
            assert!(pane.diffs.contains_key(kept), "a card still on screen lost its diff");
        }
    }

    /// **CONTRACT.** The path is not printed twice. A `Read` card already carries
    /// `file_path` as an argument field, so the detail contributes only the line counts.
    #[test]
    fn a_detail_does_not_repeat_a_path_the_arguments_already_state() {
        let path = "C:\\work\\demo\\fx-a.txt";
        let args = complete(&serde_json::json!({ "file_path": path }).to_string());
        let detail = ResultDetail {
            file_path: Some(path.into()),
            lines: Some(4),
            total_lines: Some(4),
            start_line: Some(1),
        };
        assert_eq!(detail_rows(&detail, &args), vec!["4 of 4 lines"]);
    }

    /// **CONTRACT.** …and it *is* printed on an orphan card, where the arguments were never
    /// seen and the detail's path is the only record of what the tool touched.
    #[test]
    fn an_orphan_cards_detail_carries_the_path_because_nothing_else_does() {
        let detail = ResultDetail {
            file_path: Some("C:\\work\\demo\\fx-a.txt".into()),
            lines: Some(4),
            total_lines: Some(900),
            start_line: Some(40),
        };
        assert_eq!(
            detail_rows(&detail, &Arguments::pending()),
            vec!["C:\\work\\demo\\fx-a.txt", "4 of 900 lines, from line 40"],
            "and the start line is stated, because it is not the top of the file"
        );
    }

    /// **CONTRACT.** A tool that said nothing adds no rows, and a half-reported count is
    /// not completed by guessing the other half.
    #[test]
    fn a_detail_with_nothing_measured_renders_nothing() {
        assert!(detail_rows(&ResultDetail::default(), &Arguments::pending()).is_empty());
        let half = ResultDetail { lines: Some(4), ..Default::default() };
        assert!(
            detail_rows(&half, &Arguments::pending()).is_empty(),
            "'4 lines' with no total says nothing a person wants, and inventing the total is worse"
        );
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

    /// **`/surface` still resolves to exactly what it always did**, now out of the registry
    /// rather than out of a two-arm match.
    ///
    /// This is what is left here of `only_an_exact_slash_surface_is_a_local_command`. The
    /// property that test defended — a message that merely mentions a command is not a
    /// command, and nothing a person typed may vanish — now lives in
    /// [`crate::registry`]'s own tests, where the rules are, and is *stronger* there: an
    /// unknown slash command is refused with the known set instead of being forwarded to an
    /// agent that will make something up about it, and a refusal leaves the words in the
    /// composer. What belongs in this file is only the join: that the view lane's spelling
    /// has not moved.
    #[test]
    fn the_view_lane_keeps_the_spelling_it_had() {
        let registry = Registry::new(&[]);
        assert_eq!(
            registry.resolve("/surface"),
            Resolved::Run {
                lane: Lane::View,
                name: registry::VERB_SURFACE.into(),
                args: serde_json::json!({}),
            }
        );
        assert_eq!(
            registry.entry("surface").map(|e| e.name()),
            Some(registry::VERB_SURFACE),
            "a pane offered no console verbs still answers its own"
        );
        // 🚨 The retired command. `/panel` drove the console's backdrop, which a conversation
        // has no scrollback to show, so its controls appeared to do nothing. It is not
        // aliased to `/surface` and it is not swallowed — it is refused, by name, with the
        // list of what would have worked.
        assert!(matches!(registry.resolve("/panel"), Resolved::Refused(_)));
        assert_eq!(registry.resolve("what does /surface do?"), Resolved::Message);
    }

    /// 🚨 **`/organon look surface` asks the console for a panel in the STACK, and leaves
    /// nothing in the transcript.**
    ///
    /// ✏️ **This test was `organon_summons_a_panel_into_the_transcript` and asserted the
    /// opposite** — a `Body::Organon` element at the back of the flow. That body is gone:
    /// James, *"A panel should not scroll away."* A transcript is a log and a control is not a
    /// log entry, so what a summon produces now is a *request* the console drains
    /// ([`ConversationOutput::panel`]) plus a note naming the region it went to.
    #[test]
    fn organon_asks_the_console_for_a_panel_in_the_stack() {
        let mut pane = ConversationPane::new(None, Vec::new(), Vec::new(), Capabilities::none());
        pane.panel_home = panel_stack::Home::Shown(crate::region::Region::Left);
        let before = pane.transcript.len();
        let receipt = pane.summon_organon(
            &serde_json::json!({ "tab": "look", "panel": "surface" }),
            "/organon look surface",
        );
        assert!(receipt.ok, "{}", receipt.text);
        assert_eq!(pane.panel_wanted, Some(&organon_core::panels::LOOK_SURFACE));
        assert_eq!(pane.transcript.len(), before, "nothing landed in the flow");
        // The region is named where a person can read it, not merely implied — there is one
        // stack and possibly several regions showing it.
        assert!(
            pane.log.last().is_some_and(|line| line.text.contains("left")),
            "the answer does not say which region: {:?}",
            pane.log.last()
        );
    }

    /// 🚨 **With no region holding a stack there is nowhere for a panel to go, and that is a
    /// refusal by name — never a fallback and never a silence.** This is the sentence a person
    /// meets the first time they type `/organon`, and it is the whole of how they learn a
    /// region has to be declared first.
    #[test]
    fn organon_refuses_when_no_region_holds_a_stack() {
        let mut pane = ConversationPane::new(None, Vec::new(), Vec::new(), Capabilities::none());
        assert_eq!(pane.panel_home, panel_stack::Home::Nowhere, "the opening value");
        let before = pane.transcript.len();
        let receipt = pane.summon_organon(
            &serde_json::json!({ "tab": "look", "panel": "surface" }),
            "/organon look surface",
        );
        assert!(!receipt.ok, "a panel with nowhere to go is not a success");
        assert_eq!(receipt.text, panel_stack::Refusal::NoRegion.to_string());
        assert!(receipt.text.contains("viewport"), "it names the fix: {}", receipt.text);
        assert_eq!(pane.panel_wanted, None, "nothing was asked of the console");
        assert_eq!(pane.transcript.len(), before, "and nothing fell back into the flow");
    }

    /// 🚨 **A panel this build has no controls for is refused by NAME, and the sentence it used
    /// to draw on its own card is that refusal.**
    ///
    /// James, 2026-08-21: *"We never want text just pasted in explaining something into the
    /// UI."* `panel_stack::NOT_TRANSPLANTED` was exactly that, drawn where the controls would
    /// have been, on twenty-one of twenty-five panels at once. `panel_stack::admit` is the gate
    /// and this is the door it closes on the `/organon` side; `panel_stack`'s own
    /// `add_refuses_a_panel_with_no_controls_and_remove_does_not` is the other.
    ///
    /// ⚠️ **The subject is re-derived from the table, never named**, so this keeps testing the
    /// refusal as panels are transplanted — and says so out loud on the day none is left.
    ///
    /// ⚠️ **Asserted BEFORE the destination**: the pane is given a region here, so a pass proves
    /// the refusal is about the panel rather than about there being nowhere to put it.
    #[test]
    fn organon_refuses_a_panel_that_has_no_controls_rather_than_stacking_an_empty_card() {
        let declared = organon_core::panels::PANELS
            .iter()
            .find(|p| p.status == organon_core::panels::Status::Declared)
            .expect("every panel is transplanted — this refusal has no subject left");
        let mut pane = ConversationPane::new(None, Vec::new(), Vec::new(), Capabilities::none());
        pane.panel_home = panel_stack::Home::Shown(crate::region::Region::Left);
        let before = pane.transcript.len();
        let receipt = pane.summon_organon(
            &serde_json::json!({ "tab": declared.tab.word(), "panel": declared.slug }),
            "/organon look temporal",
        );
        assert!(!receipt.ok, "a panel with no controls was accepted: {}", receipt.text);
        assert!(receipt.text.contains(declared.slug), "unnamed: {}", receipt.text);
        assert_eq!(pane.panel_wanted, None, "nothing was asked of the console");
        assert_eq!(pane.transcript.len(), before, "and nothing fell back into the flow");
    }

    /// `/media` with one picture puts an image exhibit in the flow.
    #[test]
    fn media_summons_an_image_exhibit() {
        let mut pane = ConversationPane::new(None, Vec::new(), Vec::new(), Capabilities::none());
        let receipt =
            pane.summon_media(&serde_json::json!({ "path": "/x/shot.png" }), "/media /x/shot.png");
        assert!(receipt.ok, "{}", receipt.text);
        let last = pane.transcript.elements().back().expect("an element landed");
        let Body::Artifact(artifact) = &last.body else { panic!("not an artifact") };
        let ArtifactContent::Image(spec) = &artifact.content else {
            panic!("not an image exhibit: {:?}", artifact.content)
        };
        assert_eq!(spec.items.len(), 1);
        assert_eq!(spec.single().expect("exactly one").label, "shot.png");
        assert_eq!(artifact.content.kind(), organon_core::kind::Kind::Image);
    }

    /// ⚠️ **Several paths are ONE exhibit, not one artifact each.** The distinction is the
    /// whole of "collections from day one": three candidates the agent generated are one thing
    /// a person looks through, and three separate cards would make the gallery a later feature
    /// instead of a later *placement*.
    #[test]
    fn several_paths_are_one_exhibit_with_several_items() {
        let mut pane = ConversationPane::new(None, Vec::new(), Vec::new(), Capabilities::none());
        let before = pane.transcript.elements().len();
        let receipt = pane.summon_media(
            &serde_json::json!({ "path": "/x/a.png /x/b.png /x/c.jpg" }),
            "/media /x/a.png /x/b.png /x/c.jpg",
        );
        assert!(receipt.ok, "{}", receipt.text);
        assert_eq!(
            pane.transcript.elements().len(),
            before + 1,
            "three pictures are one element, not three"
        );
        let Body::Artifact(artifact) = &pane.transcript.elements().back().unwrap().body else {
            panic!("not an artifact")
        };
        let ArtifactContent::Image(spec) = &artifact.content else { panic!("not an image") };
        assert_eq!(spec.items.len(), 3);
        assert_eq!(spec.single(), None, "a gallery has no single item");
    }

    /// A markdown path lands on the *other* arm — the falsification test #56 T4 asks for,
    /// at the placement seam: one switch, two destinations.
    #[test]
    fn a_markdown_path_lands_on_the_markdown_arm() {
        let mut pane = ConversationPane::new(None, Vec::new(), Vec::new(), Capabilities::none());
        assert!(pane.summon_media(&serde_json::json!({ "path": "/x/notes.md" }), "/media").ok);
        let Body::Artifact(artifact) = &pane.transcript.elements().back().unwrap().body else {
            panic!("not an artifact")
        };
        assert!(matches!(artifact.content, ArtifactContent::Markdown(_)));
        assert_eq!(artifact.content.kind(), organon_core::kind::Kind::Markdown);
    }

    /// 🚨 **A refused path leaves NOTHING in the transcript**, and says why in the log. A card
    /// that appeared and then showed an error would be a permanent monument to a typo, in a
    /// flow a person cannot edit — the refusal belongs in the command's own answer, which is
    /// where every other refused verb puts it.
    #[test]
    fn a_refused_path_places_no_card_and_says_why() {
        let mut pane = ConversationPane::new(None, Vec::new(), Vec::new(), Capabilities::none());
        let before = pane.transcript.elements().len();
        let receipt =
            pane.summon_media(&serde_json::json!({ "path": "/music/take.mp3" }), "/media");
        assert!(!receipt.ok, "an mp3 is refused in this build");
        assert_eq!(pane.transcript.elements().len(), before, "nothing was placed");
        assert!(receipt.text.contains("take.mp3"), "it names the file: {}", receipt.text);
        assert!(
            receipt.text.contains("playback device"),
            "with the real reason, not a generic one: {}",
            receipt.text
        );
        assert!(
            pane.log.iter().any(|l| l.text.contains("take.mp3")),
            "and the person can read it in the log: {:?}",
            pane.log
        );
    }

    /// The empty case, which `ArgKind::Text` cannot refuse for us — a required argument stops
    /// `/media` with no word at all, but `/media " "` reaches here with nothing in it.
    #[test]
    fn media_with_no_usable_path_is_refused_rather_than_placing_an_empty_card() {
        let mut pane = ConversationPane::new(None, Vec::new(), Vec::new(), Capabilities::none());
        let receipt = pane.summon_media(&serde_json::json!({ "path": "   " }), "/media");
        assert!(!receipt.ok);
        assert!(pane.transcript.elements().is_empty(), "an empty exhibit is not a card");
    }

    /// 🚨 **The pair, checked at the door a typed line does not use.** `surface` is a real slug,
    /// so the command *schema* cannot refuse `/organon motion surface` — the declared value
    /// space is the union across tabs ([`crate::registry::NarrowFn`]). A composer line is
    /// refused before it reaches here, by that hook; this call bypasses the composer exactly as
    /// a non-typed caller would, and it must still be refused and leave nothing behind.
    ///
    /// ⚠️ **One sentence, asserted as one sentence.** It is
    /// [`crate::registry::unmapped_tab`]'s, not a second phrasing that merely resembles it —
    /// which is what the `contains("not addressable yet")` this replaced could not tell apart.
    #[test]
    fn an_unknown_pair_is_refused_by_the_view_lane() {
        let mut pane = ConversationPane::new(None, Vec::new(), Vec::new(), Capabilities::none());
        let notes_before = pane.log.len();
        let receipt = pane.summon_organon(
            &serde_json::json!({ "tab": "motion", "panel": "surface" }),
            "/organon motion surface",
        );
        assert!(!receipt.ok);
        assert_eq!(receipt.text, registry::unmapped_tab("motion"));
        // ✏️ It used to assert no `Body::Organon` landed in the flow; a panel cannot land there
        // at all now, so what "leaves no panel behind" means is that the console was asked for
        // nothing — the tab check runs *before* the destination check, so this stays a
        // statement about the pair rather than about the layout.
        assert_eq!(pane.panel_wanted, None, "a refused pair asks the console for nothing");
        // …and the refusal is *visible*, not merely returned: it goes into the console's own
        // remarks above the transcript, which is where every other refusal lands.
        assert!(pane.log.len() > notes_before);
    }

    /// A slug that is on the tab but misspelled is refused with that tab's own list — the
    /// alternatives, while the words are still in the composer.
    #[test]
    fn a_misspelled_panel_is_refused_with_the_tabs_own_list() {
        let mut pane = ConversationPane::new(None, Vec::new(), Vec::new(), Capabilities::none());
        let receipt = pane.summon_organon(
            &serde_json::json!({ "tab": "look", "panel": "surfase" }),
            "/organon look surfase",
        );
        assert!(!receipt.ok);
        assert!(receipt.text.contains("surface"), "got: {}", receipt.text);
        assert!(receipt.text.contains("bloom"), "the whole tab, got: {}", receipt.text);
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
                    let (sent, _id) = composer_box(
                        ui,
                        &mut pane.text,
                        pane.live,
                        &mut pane.want_focus,
                        &mut pane.measured,
                        &Theme::organon(),
                    );
                    submitted = sent;
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

    /// ⚠️ CONTRACT: **no ring FILL until both halves are measured** — the arc is a
    /// proportion and there is none before a `result` has stated a window and a
    /// `message_start` has stated a prompt. (The ring's *track* is drawn throughout; that
    /// is [`ring_track_color`]'s contract, pinned separately below. This test was named
    /// `the_band_carries_no_ring_until_both_halves_are_measured` and asserted the whole
    /// ring was absent — the half of it that still holds is this one.)
    #[test]
    fn the_band_carries_no_ring_fill_until_both_halves_are_measured() {
        let cold = strip_content(None, LiveCounts::default(), &SessionFacts::default(), None);
        assert_eq!(cold.context, ContextSlot::Unknown, "nothing measured");

        let window_only = SessionFacts { context_window: Some(1_000_000), ..started("m") };
        let half = strip_content(None, live(0, 0), &window_only, None);
        assert_eq!(
            half.context,
            ContextSlot::Unknown,
            "a denominator alone is not a proportion"
        );

        let both = strip_content(None, live(0, 0), &filled(54_050, 1_000_000), None);
        let ContextSlot::Known(fill) = both.context else {
            panic!("{:?}", both.context)
        };
        assert_eq!(fill.percent(), 5);
    }

    /// 🚨 CONTRACT: **an unmeasured ring must not look like a measured nought.** Both draw
    /// a bare circle — a zero fill sweeps no arc either — so the whole of the difference
    /// has to live in the track and in the hover, or the ring is making the confident,
    /// specific, false claim that the draw-nothing rule was written to prevent.
    ///
    /// A zero reading is reachable rather than theoretical: a `message_start` reporting a
    /// zero prompt against a known window builds exactly this pair.
    #[test]
    fn an_unmeasured_ring_is_distinguishable_from_a_measured_nought() {
        let theme = Theme::organon();
        let nought = ContextSlot::Known(ContextFill { prompt_tokens: 0, context_window: 1_000_000 });
        assert_eq!(nought.is_high(), false, "nought is not high, and draws no arc");
        assert_eq!(
            match nought {
                ContextSlot::Known(fill) => fill.fraction(),
                ContextSlot::Unknown => 0.0,
            },
            0.0,
            "the reachable case: a real reading that sweeps no arc"
        );

        assert_ne!(
            ring_track_color(&ContextSlot::Unknown, &theme),
            ring_track_color(&nought, &theme),
            "two states drawing one picture is the false claim itself"
        );
        // The unmeasured track is the FAINTER of the two — it is a container, and the one
        // holding an answer should be the one that carries more presence.
        let dim = ring_track_color(&ContextSlot::Unknown, &theme);
        let lit = ring_track_color(&nought, &theme);
        assert!(
            dim.r() < lit.r() && dim.g() < lit.g() && dim.b() < lit.b(),
            "the empty container must be the fainter circle: {dim:?} vs {lit:?}"
        );
        // …and it is still visible against the band it sits on, or it is not a container.
        assert_ne!(dim, theme.strip_fill, "an invisible track is a ring that vanished");

        // The glanceable half is a colour; the answerable half is words.
        let unmeasured = ring_hover_rows(&ContextSlot::Unknown);
        let measured = ring_hover_rows(&nought);
        assert_ne!(unmeasured, measured, "the hover has to answer 'which is this?'");
        let (label, value) = &unmeasured[0];
        assert_eq!(label, "context");
        assert_eq!(value, "not measured yet", "never a percentage it has not been given");
        assert!(
            !unmeasured.iter().any(|(_, v)| v.contains('%')),
            "no percent sign anywhere on an unmeasured hover: {unmeasured:?}"
        );
        assert_eq!(measured[0].1, "0% at the last request", "the measured nought says so");
    }

    /// 📌 CONTRACT: **the model list is a picker input and never band text.** A note
    /// counting it — "the session offers 5 models" — used to be written at every
    /// `initialize` ack and spent the band's one line of diagnostic width on a number
    /// nobody can act on. The list itself is untouched: it is what the plate's menu is
    /// built from, and `the_picker_is_built_from_the_list_the_cli_offered` is the other
    /// half of this contract.
    #[test]
    fn the_band_says_nothing_about_how_many_models_were_offered() {
        assert_eq!(model_rows(&offered(), None).len(), 4, "the list is kept, and it is the picker's");

        let mut facts = started("claude-opus-5[1m]");
        facts.cost_usd = Some(0.42);
        facts.last_turn_duration_ms = Some(7_389);
        let content = strip_content(None, live(0, 2), &facts, Some("abc"));
        let band = format!(
            "{} {} {:?}",
            content.chips_seen(true).join(CHIP_SEP),
            content.reading.text,
            content.identity
        );
        for word in ["models", "offers", "offered"] {
            assert!(!band.contains(word), "the band must not mention the list ({word}): {band}");
        }
    }

    /// The percentage the reader is **actually shown**, read back out of the ring's hover.
    ///
    /// Parsed from the rendered string rather than recomputed, so a test comparing the
    /// ring's colour against it is comparing against the display and not against a third
    /// copy of the same arithmetic — which is the mistake this helper exists to stop
    /// repeating.
    fn shown_percent(slot: ContextSlot) -> u64 {
        let ContextSlot::Known(fill) = slot else {
            panic!("no reading to display: {slot:?}")
        };
        let (_, value) = context_rows(&fill).remove(0);
        value
            .split('%')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("no percentage in {value:?}"))
    }

    /// CONTRACT: amber at exactly [`CONTEXT_HIGH_PERCENT`], not a token before it — and the
    /// colour is checked **against the number the hover prints**, never against
    /// [`ContextSlot::is_high`] alone. The threshold is the console's own judgement and is
    /// therefore the one number here a reader could reasonably argue with, so it is pinned
    /// rather than approximated.
    ///
    /// ⚠️ The shape of this test is the finding it came from. It used to assert `is_high()`
    /// in isolation, which pins *where* the ring turns amber and says nothing about whether
    /// the ring and its own hover agree — and they did not. See
    /// `the_ring_cannot_contradict_the_percentage_it_prints` for the case that got through.
    #[test]
    fn the_ring_turns_amber_at_three_quarters_and_not_before() {
        let at = |prompt| strip_content(None, live(0, 0), &filled(prompt, 1_000), None).context;

        for prompt in [0, 1, 500, 748, 749, 750, 751, 999, 1_000] {
            let slot = at(prompt);
            let shown = shown_percent(slot);
            assert_eq!(
                slot.is_high(),
                shown >= CONTEXT_HIGH_PERCENT,
                "{prompt}/1000: the ring's colour disagrees with the {shown}% it prints"
            );
        }

        assert_eq!(shown_percent(at(749)), 74, "74.9% has not reached 75");
        assert!(!at(749).is_high(), "74.9% is still blue");
        assert_eq!(shown_percent(at(750)), 75, "the exact boundary reads as the boundary");
        assert!(at(750).is_high(), "75.0% is the boundary and it counts as high");
        assert!(at(1_000).is_high());
        assert!(!ContextSlot::Unknown.is_high(), "no reading is not a high reading");
    }

    /// ⚠️ REGRESSION — the review's own case, which is why the pair is so specific:
    /// `prompt_tokens = 7 495` over a `context_window = 10 000`.
    ///
    /// Two arithmetics decided one thing. `percent()` rounded `74.95` to **75** while
    /// `is_high()` computed `749 500 >= 750 000` and said **false**, so the hover read
    /// "75 % at the last request" beside a ring that was still blue. Both halves of the fix
    /// are pinned here: the reading **floors**, so it says 74 rather than claiming a
    /// threshold it has not reached, and the colour is *derived* from that reading, so the
    /// two cannot part company again.
    #[test]
    fn the_ring_cannot_contradict_the_percentage_it_prints() {
        let slot = strip_content(None, live(0, 0), &filled(7_495, 10_000), None).context;
        assert_eq!(shown_percent(slot), 74, "74.95% has not reached 75 and must not claim it");
        assert!(!slot.is_high(), "and a ring below the threshold is not amber");
    }

    /// CONTRACT: the reading **floors**. A fill gauge that rounds *overstates*, and this
    /// readout exists precisely because its obvious numerator overstated by 1.97× — so it
    /// may never report a fill the conversation has not reached.
    #[test]
    fn the_percentage_floors_and_never_claims_a_fill_it_has_not_reached() {
        let pct = |prompt, window| ContextFill { prompt_tokens: prompt, context_window: window }.percent();
        assert_eq!(pct(1, 10_000), 0, "a hundredth of a percent is 0, not 1");
        assert_eq!(pct(7_499, 10_000), 74, "74.99% is not 75%");
        assert_eq!(pct(7_500, 10_000), 75, "exactly 75% is 75%");
        assert_eq!(pct(9_999, 10_000), 99, "99.99% is not a full window");
        assert_eq!(pct(10_000, 10_000), 100);
        assert_eq!(pct(20_000, 10_000), 100, "mispaired halves clamp, as the arc does");
    }

    /// ⚠️ CONTRACT: an **exact** three quarters reads as 75 whatever the window, and is
    /// amber — flooring must not push the true boundary off by one.
    ///
    /// This is the thing flooring could most plausibly get wrong, so it is swept rather
    /// than sampled: 50 000 windows, each with a prompt at exactly three quarters of it.
    ///
    /// 📌 Honest about what this does *not* show. Flooring [`ContextFill::fraction`] as an
    /// `f32` passes this sweep too — measured, 0 misreads across every window tried, and
    /// the same at 12 M. At realistic window sizes both spellings are exact, because both
    /// counts are well under `2^24` and convert to `f32` losslessly. The argument for
    /// integer division is therefore *not* that the float is wrong here; it is that the
    /// float is only right **contingently**, and
    /// `a_window_past_the_f32_integer_limit_still_reads_its_exact_percentage` pins the
    /// range where that contingency runs out.
    #[test]
    fn an_exact_three_quarters_reads_as_seventy_five_whatever_the_window() {
        for window in (1..=50_000u64).map(|n| n * 4) {
            let fill = ContextFill { prompt_tokens: window / 4 * 3, context_window: window };
            assert_eq!(fill.percent(), 75, "exactly 75% of a {window}-token window");
            assert!(ContextSlot::Known(fill).is_high(), "…and that is amber");
        }
    }

    /// ⚠️ REGRESSION against reimplementing [`ContextFill::percent`] over
    /// [`ContextFill::fraction`] — which is why this pair is so specific:
    /// `16 777 233 / 16 946 700` is exactly **99 %**, and an `f32` reads it as **98**.
    ///
    /// Both counts are past `2^24 = 16 777 216`, the last integer an `f32` represents
    /// exactly, so the conversion loses the numerator *before* any division happens and no
    /// amount of care downstream recovers it. Today's windows are a million tokens and
    /// nowhere near this, which is exactly why it is worth a test: the float spelling is
    /// correct only while windows stay small, that has been the direction of travel in one
    /// direction only, and the failure is a silently understated fill — the same direction
    /// of error, again, that this whole readout was built to remove.
    #[test]
    fn a_window_past_the_f32_integer_limit_still_reads_its_exact_percentage() {
        let fill = ContextFill { prompt_tokens: 16_777_233, context_window: 16_946_700 };
        assert_eq!(fill.prompt_tokens * 100, fill.context_window * 99, "exactly 99%, by construction");
        assert_eq!(fill.percent(), 99, "and it must read 99, not the f32's 98");
        assert!(ContextSlot::Known(fill).is_high());
    }

    /// CONTRACT: the ring is a proportion of the window and nothing else — it does not
    /// grow with the session, and a compaction that shrinks the prompt shrinks the ring.
    /// Latest-wins all the way through, which is what makes that true for free.
    #[test]
    fn the_ring_follows_the_last_prompt_down_as_well_as_up() {
        let grown = strip_content(None, live(0, 0), &filled(800_000, 1_000_000), None);
        let compacted = strip_content(None, live(0, 0), &filled(120_000, 1_000_000), None);
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
            session_allow: false,
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
        let content = strip_content(None, cold, &SessionFacts::default(), None);
        assert_eq!(content.model, ModelSlot::Connecting, "the plate says it is coming");
        assert_eq!(content.reading.standing, Standing::Connecting);
        assert_eq!(
            content.reading.text, "",
            "and the status half stays silent rather than saying it a second time"
        );
        assert!(content.identity.is_empty(), "nothing is known, so the hover claims nothing");
        // ⚠️ This assertion **moved**: it used to read `content.chips.is_empty()`, "no
        // cost, no memory, no turn — no chips". That encoded the dim half appearing at the
        // first `result`, which is the reshuffle this tier removes. The cost is now on the
        // band from the first frame at its true value; the other two are still absent, and
        // `the_cold_band_reports_a_cost_and_a_ring_and_does_not_grow` says why.
        // ⚠️ `chips_seen(true)` — the **traced** band, which is where the spend now lives.
        // The arithmetic is unchanged and is what this pins; `the_default_band_carries_no_
        // harness_telemetry` pins the other half, that a quiet band shows none of it.
        assert_eq!(
            content.chips_seen(true),
            vec!["session $0.0000"],
            "the session's spend is on the band from the first frame, and it is nought"
        );
    }

    /// Once init arrives the model is the headline, and everything else it said is on the
    /// hover rather than on the band.
    #[test]
    fn the_model_becomes_the_headline_once_init_arrives() {
        let content =
            strip_content(None, live(0, 0), &started("claude-opus-5[1m]"), Some("abc-123"));
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
                session_allow: false,
                has_session: true,
                generating: true,
            },
            &facts,
            Some("abc-123"),
        );
        assert_eq!(content.reading.standing, Standing::Dead);
        assert_eq!(content.reading.text, "the agent stopped listening: broken pipe");
    }

    /// Waiting on a human outranks working, because only one of the two the agent can finish
    /// on its own. This is the ordering the first status line had; it is kept, not rederived.
    #[test]
    fn waiting_on_a_human_outranks_working() {
        let facts = started("claude-opus-5");
        let both = strip_content(None, live(1, 4), &facts, Some("abc"));
        assert_eq!(both.reading.standing, Standing::Asking);
        assert_eq!(both.reading.text, "◈ 1 permission request — waiting on you");

        let working = strip_content(None, live(0, 4), &facts, Some("abc"));
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

        let idle = strip_content(None, live(0, 0), &facts, Some("abc"));
        assert_eq!(idle.reading.standing, Standing::Asking);
        assert_eq!(idle.reading.text, "◈ pick one of the three options");

        let resumed = strip_content(None, live(0, 1), &facts, Some("abc"));
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
        let content = strip_content(None, live_generating(0, 0), &facts, Some("abc"));
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
        let closed = strip_content(None, live(0, 0), &facts, Some("abc"));
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

        let with_tools = strip_content(None, live_generating(0, 2), &facts, Some("abc"));
        assert_eq!(with_tools.reading.standing, Standing::Working);
        assert_eq!(
            with_tools.reading.text, "● 2 tools running",
            "the specific reading wins: it names what is happening, generating only says that \
             something is"
        );

        let writing = strip_content(None, live_generating(0, 0), &facts, Some("abc"));
        assert_eq!(
            writing.reading.standing,
            Standing::Generating,
            "a demand the human already answered must not sit on the band through the reply \
             that answered it"
        );
        assert_eq!(writing.reading.text, "● generating");

        // …and the moment the bracket closes, the demand is what is left to say.
        let idle = strip_content(None, live(0, 0), &facts, Some("abc"));
        assert_eq!(idle.reading.standing, Standing::Asking);
        assert_eq!(idle.reading.text, "◈ pick one of the three options");
    }

    /// The two readings above generating still outrank it. A dead agent has no tokens
    /// arriving whatever the last thing it said was, and a pending question is the agent
    /// *halted* on a human — the one state it cannot get out of by itself.
    #[test]
    fn a_dead_agent_and_a_pending_question_both_still_outrank_generating() {
        let facts = started("claude-opus-5");
        let asked = strip_content(None, live_generating(1, 0), &facts, Some("abc"));
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
        );
        assert_eq!(gone.reading.standing, Standing::Dead);
        assert_eq!(gone.reading.text, "the agent process ended");
    }

    /// Busy is one colour. The distinction the palette has to carry is busy-versus-blocked,
    /// and busy-with-tools against busy-writing is the same answer to "can I walk away" —
    /// the text already spells out which.
    #[test]
    fn generating_and_working_read_as_the_same_kind_of_busy() {
        let t = Theme::organon();
        assert_eq!(standing_color(Standing::Generating, &t), standing_color(Standing::Working, &t));
        assert_ne!(standing_color(Standing::Generating, &t), standing_color(Standing::Ready, &t));
        assert_ne!(standing_color(Standing::Generating, &t), standing_color(Standing::Asking, &t));
    }

    /// With nothing outstanding the band reports the turn the agent described, not a guess.
    #[test]
    fn a_quiet_session_reports_what_the_last_turn_said() {
        let mut facts = started("claude-opus-5");
        facts.last_status_detail = Some("wrote the strip and ran the tests".into());
        let content = strip_content(None, live(0, 0), &facts, Some("abc"));
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
        let content = strip_content(None, counts, &facts, Some("abc"));
        assert_eq!(
            content.chips_seen(true),
            vec!["session $0.1234", "2 remembered decisions", "last turn 7.4s"]
        );
        let band = content.chips_seen(true).join(CHIP_SEP);
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
            strip_content(None, live(0, 0), &facts, Some("abc")).switching_to(Some("Sonnet"));
        let ModelSlot::Named(label) = &switching.model else { panic!("{:?}", switching.model) };
        assert_eq!(
            label.name, "claude-opus-5",
            "the plate still says what the session last reported, not what was asked for"
        );
        assert_eq!(switching.pending_model.as_deref(), Some("Sonnet"));

        // …and once the repeat init lands, the marker has nothing left to say.
        let settled = strip_content(None, live(0, 0), &started("claude-sonnet-5"), Some("abc"));
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
            strip_content(None, live(0, 0), &facts, Some("abc")).mode
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
        let cold = strip_content(None, LiveCounts::default(), &SessionFacts::default(), None);
        assert_eq!(cold.mode, ModeSlot::default());
    }

    /// 🚨 CONTRACT: **the resting band spells nothing out — but an abnormal one still uses
    /// words.**
    ///
    /// James, 2026-08-21: *"we don't want to show words like `default` and `allow all` at all
    /// times. That would be a sort of verbose form of the interface. We should have either icons
    /// or some other way of not having to show all those characters."* So the two named
    /// offenders are gone from the resting band — `default` is a dim [`mode_glyph`] and nothing
    /// else, and `allow all` is not drawn at all when there is no standing allow.
    ///
    /// 🚨 **And the persistent-warning invariant is unchanged, which is the half a "make it
    /// compact" change would quietly lose.** An abnormal mode still carries two words on the
    /// band, permanently, uncloseable — see [`ModeMarker::short`]. Colour alone is not allowed to
    /// be the only statement that the console may not be the one being asked.
    ///
    /// ⚠️ **Mutation-checked, both run.** Make [`mode_glyph`] return the mode's name and this
    /// fails with `left: "default", right: "◈"`; empty `dontAsk`'s [`ModeMarker::short`] and it
    /// fails with *"dontAsk's band words are not two words"*.
    #[test]
    fn the_resting_band_carries_marks_and_the_abnormal_one_still_carries_words() {
        // The resting state: nothing on the band spells the mode out.
        assert_eq!(mode_glyph(MODE_DEFAULT), MODE_GLYPH_ASKS);
        assert!(
            !mode_glyph(MODE_DEFAULT).contains(MODE_DEFAULT),
            "the band is still spelling `default` at rest",
        );
        assert!(mode_marker(MODE_DEFAULT).is_none(), "and it says nothing beside it");
        assert!(
            !SESSION_ALLOW_LABEL.contains("allow"),
            "the plate is still spelling `allow all`: {SESSION_ALLOW_LABEL}",
        );

        // The abnormal states: a mark AND two words, every time.
        for mode in ["acceptEdits", "dontAsk", "plan"] {
            let marker = mode_marker(mode).expect("not the default");
            assert!(
                marker.short.split_whitespace().count() <= 2 && !marker.short.is_empty(),
                "{mode}'s band words are not two words: {:?}",
                marker.short,
            );
            assert!(
                marker.short.len() < marker.text.len(),
                "{mode}'s short form is not shorter than its sentence",
            );
            assert!(
                !marker.short.contains(mode),
                "{mode}'s marker repeats the mode's own name, which the plate already carries",
            );
        }
        // `dontAsk` is the one state whose mark differs, because it is the one where approvals
        // do not reach the human at all.
        assert_eq!(mode_glyph("dontAsk"), MODE_GLYPH_SILENT);
        assert_eq!(mode_glyph("acceptEdits"), MODE_GLYPH_ASKS);
        assert_eq!(
            mode_glyph("plan"),
            MODE_GLYPH_ASKS,
            "an unmeasured mode must not ASSERT that you are not being asked",
        );

        // The standing allow keeps its two words and its whole sentence, in two places.
        assert!(SESSION_ALLOW_SHORT.split_whitespace().count() <= 2);
        assert!(SESSION_ALLOW_SHORT.len() < SESSION_ALLOW_MARKER.len());
        assert!(SESSION_ALLOW_MARKER.contains("allowed"), "the hover still says what happened");
    }

    /// 🚨 CONTRACT: **the capability tools a caller hands down are the ones served, and the
    /// permission handler is never one of them.**
    ///
    /// This is the connection Part 1 was missing: the server was built with an empty spec
    /// table, so the console answered permissions for everything and exposed nothing, and an
    /// agent wanting a console verb had to shell out to `organon.exe` — a separate process,
    /// to talk to the process it was already inside.
    ///
    /// It starts a real loopback server, because that is the wiring under test; the port is
    /// ephemeral and the config file deletes itself with the wiring.
    #[test]
    fn the_verbs_handed_down_are_the_verbs_served() {
        let specs = vec![
            CommandSpec {
                name: "console.portal".into(),
                doc: "Open or close the portal".into(),
                target: crate::command::TargetKind::Viewport,
                args: vec![crate::command::ArgSpec {
                    name: "state".into(),
                    kind: crate::command::ArgKind::Choice(vec!["open".into(), "close".into()]),
                    required: true,
                }],
                reversal: crate::command::Reversal::Recoverable,
            },
            CommandSpec {
                name: "console.background".into(),
                doc: "What sits behind the glyphs".into(),
                target: crate::command::TargetKind::Viewport,
                args: Vec::new(),
                reversal: crate::command::Reversal::Recoverable,
            },
        ];
        let (wiring, mcp, _inbox, notes) =
            start_approvals(&specs, Box::new(NoDispatch))
                .expect("a loopback port and a temp config");
        assert!(notes.is_empty(), "a table with no collisions has nothing to report: {notes:?}");

        assert_eq!(
            wiring.served,
            ["mcp__organon__console_portal", "mcp__organon__console_background"],
            "dotted names are sanitised, and the mapping is the server's — not a second table"
        );
        assert_eq!(wiring.handler, "mcp__organon__approve_tool");
        assert_eq!(mcp.permission_tool, wiring.handler, "the flag names the same handler");
        assert!(
            !wiring.served.contains(&wiring.handler),
            "the handler is the console's own gate, never a capability the model may call"
        );

        // And the empty case is still reachable and still honest: a caller with nothing to
        // offer serves the handler alone (§9 point 5's safest shape).
        let (bare, _, _, _) =
            start_approvals(&[], Box::new(NoDispatch)).expect("wiring");
        assert!(bare.served.is_empty());
    }

    /// 🚨 CONTRACT: **a verb that is not served says so where a human is, not only on
    /// stderr.** A console started from a PATH shim has no terminal attached, so an
    /// `eprintln!` about a silently-missing capability is written to nobody — the same defect
    /// the pane's log itself had until the scrollback started drawing it. The note comes back
    /// from `start_approvals` so [`ConversationPane::new`] can seed the log with it, which is
    /// what puts it at the head of the scrollback and in the band's slot.
    ///
    /// ⚠️ This is the *only* path here that can go quiet: the exposure audit already says
    /// both, and a server that will not start is reported by the `Err` arm. Do not add a
    /// third.
    #[test]
    fn a_colliding_verb_is_reported_to_the_pane_and_not_only_to_stderr() {
        // Nothing pure to construct here, so the collision is stated directly: two names that
        // sanitise to one tool name. `mcp.rs` owns the transform and pins it.
        let note = collision_note(&["console.portal".to_string()])
            .expect("a collision has something to say");
        assert!(note.contains("console.portal"), "it names the verb that was lost: {note}");
        assert!(note.contains("NOT served"), "and says what happened to it: {note}");
        assert!(
            collision_note(&[]).is_none(),
            "and a clean table adds no line to a log a human reads every session"
        );

        // And the wiring really carries it out to the caller — the half an `eprintln!` inside
        // `start_approvals` could not do.
        let colliding = vec![
            CommandSpec {
                name: "console.portal".into(),
                doc: "Open or close the portal".into(),
                target: crate::command::TargetKind::Viewport,
                args: Vec::new(),
                reversal: crate::command::Reversal::Recoverable,
            },
            CommandSpec {
                name: "console/portal".into(),
                doc: "The same tool name, spelled differently".into(),
                target: crate::command::TargetKind::Viewport,
                args: Vec::new(),
                reversal: crate::command::Reversal::Recoverable,
            },
        ];
        let (wiring, _, _, notes) =
            start_approvals(&colliding, Box::new(NoDispatch))
                .expect("a loopback port and a temp config");
        assert_eq!(notes.len(), 1, "one line, naming the loss: {notes:?}");
        assert!(notes[0].contains("console/portal"), "{:?}", notes[0]);
        assert_eq!(
            wiring.served,
            ["mcp__organon__console_portal"],
            "and the first spelling is still served — a collision is not a failure"
        );
    }

    /// ⚠️ CONTRACT: with no wiring there is nothing to audit, and the line says so rather
    /// than reading as a pass. The arithmetic itself is pinned in [`crate::mcp`]; this is the
    /// adapter, and the only thing it can get wrong is claiming to have checked.
    #[test]
    fn an_unwired_pane_reports_that_nothing_was_checked() {
        let line = audit_line(None, &["Bash".to_string()]);
        assert!(line.text.contains("not wired"), "{}", line.text);
        assert!(
            !line.text.contains("withheld"),
            "silence must not read as a clean bill: {}",
            line.text
        );
        assert!(line.always, "an audit that proved nothing is not something to keep quiet about");
    }

    /// 🚨 CONTRACT: **the approvals audit is silent exactly when the withholding property
    /// holds, and loud in every other case.** James struck the passing line out of the live
    /// build — it appeared at the head of the scrollback and again on the status band, on
    /// every launch and after every deferred re-`init`. What must never become quiet is the
    /// case it exists for: the handler reachable by the model, or an init that reported no
    /// tools at all and therefore proved nothing.
    ///
    /// ⚠️ The two anomalous arms are asserted **by the flag**, not by their wording, so a
    /// later rewording of [`crate::mcp::ExposureAudit::summary`] cannot silently make one of
    /// them quiet.
    #[test]
    fn the_approvals_audit_speaks_only_when_the_guarantee_is_not_holding() {
        let handler = "mcp__organon__approve_tool";
        let served = ["mcp__organon__console_portal".to_string()];
        let line = |offered: &[&str]| {
            let offered: Vec<String> = offered.iter().map(|s| (*s).to_string()).collect();
            audit_remark(handler, &served, &offered)
        };

        // The expected world: a list was reported and the handler is not on it.
        let quiet = line(&["Bash", "mcp__organon__console_portal"]);
        assert!(quiet.text.contains("withheld"), "{}", quiet.text);
        assert!(!quiet.always, "a guarantee that is holding is not news: {}", quiet.text);
        // ✏️ **…and the log still keeps it.** The old spelling of these two lines asked
        // `Remark::seen`, which is gone with the mode it answered: a quiet line is no longer
        // hidden-unless-tracing, it is *in the status log and nowhere else*. What is worth
        // asserting is that it is recorded and that it does not light the indicator.
        let mut log = StatusLog::default();
        log.push(quiet.clone());
        assert_eq!(log.len(), 1, "a passing audit was thrown away rather than logged");
        assert!(!log.attention(), "a guarantee that is holding lit the status line");

        // 🚨 The breach.
        let breach = line(&[handler]);
        assert!(breach.always, "a handler the model can call must never be quiet: {}", breach.text);

        // And an init that reported nothing has proved nothing, which is also not a pass.
        let unproven = line(&[]);
        assert!(unproven.always, "an unchecked guarantee is not a held one: {}", unproven.text);

        // ⚠️ A served name the model cannot see is the ordinary deferred-loading case and
        // must NOT make the line loud — otherwise every cold start reads as a fault.
        let deferred = line(&["Bash"]);
        assert!(
            !deferred.always,
            "a withheld capability is not a breach of the handler guarantee: {}",
            deferred.text
        );
    }

    /// 🚨 CONTRACT: **the default band carries no harness telemetry and no echo of the page
    /// above it.** James, on the live build, striking out `session $1.18 · last turn 5.1s` and
    /// `◈ What are we working on?`: his model is Claude Desktop, which tells you which model
    /// you are talking to and nothing about what the last turn cost or how long it took.
    ///
    /// ⚠️ **What must survive is asserted alongside**, because a rule that only says what to
    /// hide is one careless edit away from hiding the band. The model, the mode and the
    /// console's own remembered-decisions tally are facts about the world, not narration.
    #[test]
    fn the_default_band_carries_no_harness_telemetry() {
        let mut facts = started("claude-opus-5[1m]");
        facts.cost_usd = Some(1.18);
        facts.last_turn_duration_ms = Some(5_100);
        facts.needs_action = Some("What are we working on?".into());
        let counts = LiveCounts { remembered: 2, ..live(0, 0) };
        let content = strip_content(None, counts, &facts, Some("abc"));

        // Quiet: the tally, and nothing else.
        assert_eq!(
            content.chips_seen(false),
            vec!["2 remembered decisions"],
            "the spend and the turn's duration are the harness talking about itself"
        );
        assert_eq!(
            content.reading.seen_text(false),
            "",
            "the agent's own closing line is already the last thing in the transcript"
        );

        // Tracing: everything, in the order it was built.
        assert_eq!(
            content.chips_seen(true),
            vec!["session $1.18", "2 remembered decisions", "last turn 5.1s"],
            "`/trace on` is where the machinery lives, and it is unchanged"
        );
        assert_eq!(content.reading.seen_text(true), "◈ What are we working on?");

        // ⚠️ And the readings that are NOT narration are on the quiet band, unconditionally.
        // A pane that hid these would be quiet about the only three things it can tell you
        // that the page above it cannot.
        let dead = strip_content(Some("the agent stopped listening"), live(0, 0), &facts, None);
        assert_eq!(dead.reading.seen_text(false), "the agent stopped listening");
        let asking = strip_content(None, live(1, 0), &facts, Some("abc"));
        assert_eq!(asking.reading.seen_text(false), "◈ 1 permission request — waiting on you");
        let working = strip_content(None, live(0, 2), &facts, Some("abc"));
        assert_eq!(working.reading.seen_text(false), "● 2 tools running");
        assert_eq!(
            content.model,
            ModelSlot::Named(model_label("claude-opus-5[1m]")),
            "the model chip is not telemetry — it is who you are talking to"
        );
    }

    /// 🚨 CONTRACT: **the band gives its fixed items their width before the flexible one
    /// takes any**, so no segment can be painted over by its neighbour.
    ///
    /// James, on the live build: `◈ What are we working on?ession $1.18 · last turn 5.1s` —
    /// the echo's tail running *under* the chips, because `Label::truncate` had truncated it
    /// to "everything left", which is not a bound when nothing has been taken yet. This is
    /// the arithmetic [`strip_right_reserve`] and [`reading_room`] replaced it with, and the
    /// property is the one a narrow window breaks first.
    ///
    /// ⚠️ **Mutation-checked**: spell [`reading_room`] as a bare subtraction and the narrow
    /// case below fails with a negative width — which egui turns into a panic on the
    /// allocation, not into a smaller label.
    ///
    /// ✏️ **What the reservation covers changed with #129 and the property did not.** The status
    /// log's indicator has left the band, so it is no longer measured here; and the remainder is
    /// now given to the **whole left group** rather than to the reading alone, which is what
    /// makes the model plate and the permission markers unable to overflow either. See
    /// [`the_band_holds_one_line_at_a_narrow_width`] for the end-to-end half of that.
    #[test]
    fn the_band_gives_the_fixed_items_their_width_before_the_echo() {
        // Ordinary: the left half gets everything the fixed items do not need, and no more.
        assert_eq!(reading_room(600.0, 180.0), 420.0);
        assert!(
            reading_room(600.0, 180.0) + 180.0 <= 600.0,
            "the two halves must not add up to more band than there is"
        );
        // Narrow: the fixed items alone outgrow the band, and the flexible one gives way
        // entirely rather than asking for a negative allocation.
        assert_eq!(reading_room(120.0, 180.0), 0.0);
        assert_eq!(reading_room(0.0, 0.0), 0.0);

        // And the reservation itself grows with what is actually on the right, so hiding the
        // telemetry hands the width back to the reading rather than leaving a hole.
        let ctx = egui::Context::default();
        let (bare, full) = {
            let mut out = (0.0_f32, 0.0_f32);
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    out.0 = strip_right_reserve(ui, &[]);
                    out.1 = strip_right_reserve(ui, &["session $1.18", "last turn 5.1s"]);
                });
            });
            out
        };
        assert!(bare > 0.0, "the ring is allocated every frame, measured or not: {bare}");
        assert!(full > bare, "chips take width and the left half must be told: {full} vs {bare}");
    }

    /// 🚨 CONTRACT: **at a width too narrow for everything, segments give way — they do not
    /// paint over each other, and the band stays one line.**
    ///
    /// James photographed the failure twice, and #125's fix covered only the flexible reading:
    /// `allow all` was still drawn *over* `you allowed everything…` because the left group's own
    /// items had no budget at all. The fix is structural — [`strip_box`] measures the right-hand
    /// fixed set first and allocates the remainder to a sub-`Ui`, so nothing in the left group
    /// can be drawn outside a rect that was sized before any of it existed.
    ///
    /// ⚠️ **The band's HEIGHT cannot see this and it is worth saying why**, because it is the
    /// obvious assertion and it is useless: `Ui::horizontal` does not wrap, so an overflowing
    /// left group stays exactly one row tall and simply runs under the chips — which is precisely
    /// what the photograph shows. Measuring the height was tried here and passed against
    /// deliberately broken code. The rects are what settle it, hence [`band_group_rects`].
    ///
    /// ⚠️ **Mutation-checked**: give the left group `ui.available_width()` instead of
    /// [`reading_room`]'s remainder and this fails at every width below 900 pt, naming both
    /// rects.
    #[test]
    fn the_bands_two_halves_never_overlap_however_narrow_it_gets() {
        let ctx = egui::Context::default();
        let mut pane = FakePane::new("x");
        pane.want_focus = false;
        let busy = the_busiest_band();
        for width in [260.0_f32, 380.0, 520.0, 900.0] {
            let mut rects = None;
            for _ in 0..3 {
                let _ = strip_frame_at(&ctx, &busy, &mut pane, width);
                rects = band_group_rects(&ctx);
            }
            let (left, right) = rects.expect("the band drew and published its halves");
            assert!(
                left.right() <= right.left() + 0.5,
                "at {width} pt the band's left half runs under its right: left {left:?}, right \
                 {right:?}",
            );
        }
    }

    /// The busiest band that can occur: a long model id with a badge, a pending switch, a
    /// `dontAsk` marker, a standing allow, both telemetry chips, a pending-approvals reading and
    /// a full ring. Shared by the two band-geometry contracts so they cannot drift apart about
    /// what "busiest" means.
    fn the_busiest_band() -> StripContent {
        let mut facts = started("claude-opus-5[1m]");
        facts.cost_usd = Some(1.2345);
        facts.last_turn_duration_ms = Some(7_389);
        facts.permission_mode = Some("dontAsk".into());
        facts.context_window = Some(1_000_000);
        facts.last_prompt_tokens = Some(910_000);
        strip_content(
            None,
            LiveCounts {
                pending_approvals: 2,
                running_tools: 0,
                remembered: 9,
                session_allow: true,
                has_session: true,
                generating: true,
            },
            &facts,
            Some("11111111-2222-3333-4444-555555555555"),
        )
        .switching_to(Some("Default (recommended)"))
    }

    /// ⚠️ **A companion, not the contract** — see
    /// [`the_bands_two_halves_never_overlap_however_narrow_it_gets`] for why height alone proves
    /// nothing. This still pins the *other* half of "one band": that nothing in it is ever taller
    /// than the row [`strip_box`] reserved, which is how the ring or a plate would break it.
    #[test]
    fn the_band_holds_one_line_at_a_narrow_width() {
        let ctx = egui::Context::default();
        let mut pane = FakePane::new("x");
        pane.want_focus = false;

        let busy = the_busiest_band();

        // 260 pt is narrower than a single console tab is ever likely to be and is the point:
        // the property has to hold where the fixed set alone is wider than the band.
        for width in [260.0_f32, 380.0, 520.0, 900.0] {
            let mut band = 0.0;
            for _ in 0..3 {
                band = strip_frame_at(&ctx, &busy, &mut pane, width).0;
            }
            assert!(band > 0.0, "the band vanished at {width} pt");
            assert!(
                band < 44.0,
                "the band became two lines at {width} pt ({band}) — a segment wrapped instead \
                 of giving way",
            );
        }
    }

    /// 🚨 CONTRACT: **a turn that ended well leaves no caption; a turn that failed or was
    /// cancelled always does.** See [`element_seen`] — this is the pane's quiet/loud rule
    /// reaching the transcript.
    ///
    /// ⚠️ **Mutation-checked**: make the arm `_ => false` and the two failure rows fail; make it
    /// `_ => true` and the success row fails.
    ///
    /// ✏️ **No mode column any more.** The success caption used to come back under `/trace on`;
    /// that verb now opens the status log and is forbidden from touching the transcript, so the
    /// answer is one value per outcome. See [`element_seen`].
    #[test]
    fn a_finished_turn_says_nothing_and_a_broken_one_always_does() {
        let end = |outcome| Body::RunEnd(crate::conversation::RunEnd { outcome, detail: None });
        for (outcome, seen) in [
            (RunOutcome::Ok, false),
            (RunOutcome::Error, true),
            (RunOutcome::Cancelled, true),
        ] {
            assert_eq!(element_seen(&end(outcome)), seen, "{outcome:?}");
        }
        // Everything else is unconditional, and stays so by falling through rather than by
        // being listed — a new `Body` must be visible until somebody decides otherwise.
        let human = Body::Human(crate::conversation::HumanBlock { text: "hello".into() });
        assert!(element_seen(&human));
    }

    /// 🚨 CONTRACT: **a live composer hints nothing; a dead one says why.** It read `message
    /// the agent — Enter sends, Shift+Enter for a new line`, which is the console explaining
    /// itself to somebody who has not seen it before — and the first thing James saw on the
    /// build after #117. See [`COMPOSER_HINT_DEAD`] for why the asymmetry is the rule.
    #[test]
    fn only_a_dead_composer_carries_a_hint() {
        assert_eq!(composer_hint(true), "", "an empty box under a conversation reads as ready");
        assert_eq!(
            composer_hint(false),
            "not running",
            "a DISABLED box with no hint reads as broken, which is the case that earns words"
        );
        assert!(
            !composer_hint(false).contains("Enter"),
            "and what it says is the fact, not the keystroke contract"
        );
    }

    /// 🚨 CONTRACT: **the standing-allow marker is on the band for exactly as long as the
    /// allow is, and it is DERIVED rather than stored.** Same rule as the mode marker above,
    /// for the same reason: a console that has stopped asking while still looking like the
    /// authority is the failure both markers exist to prevent — and a flag somebody has to
    /// remember to clear is how a marker gets stuck on, or worse, stuck off.
    #[test]
    fn the_session_allow_marker_is_present_exactly_while_it_is_active() {
        let facts = started("claude-opus-5");
        let band = |on: bool| {
            let counts = LiveCounts { session_allow: on, ..live(0, 0) };
            strip_content(None, counts, &facts, Some("abc"))
        };

        assert_eq!(band(false).session_allow, None, "the console asking is not news");
        let marked = band(true).session_allow.expect("a console that has stopped asking must say so");
        assert_eq!(marked.marker, SESSION_ALLOW_MARKER);
        assert!(
            marked.marker.contains("not asking"),
            "the marker says what is happening, not what it is called: {}",
            marked.marker
        );

        // Derived: the same inputs give the same band, and turning the allow off takes the
        // marker with it in the very next frame. There is nowhere for it to be latched.
        assert_eq!(band(true), band(true));
        assert_eq!(band(false).session_allow, None);
    }

    /// 🚨 CONTRACT: **"you allowed everything" and "a mode is silencing approvals" are
    /// different facts with different remedies, and the band says which.** Both can be true
    /// at once. The mode marker is fixed by changing the mode; this one is fixed by revoking
    /// the grant, on this band. A single merged warning would name the wrong cure half the
    /// time.
    #[test]
    fn the_band_tells_a_standing_allow_apart_from_a_mode_that_silences_approvals() {
        let mut facts = started("claude-opus-5");
        facts.permission_mode = Some("dontAsk".into());
        let both = strip_content(
            None,
            LiveCounts { session_allow: true, ..live(0, 0) },
            &facts,
            Some("abc"),
        );
        let mode = both.mode.marker.expect("the mode still speaks for itself");
        let ours = both.session_allow.expect("and so does ours");
        assert_ne!(mode.text, ours.marker, "two facts, two sentences");
        assert!(mode.text.contains("refused"), "upstream refuses: {}", mode.text);
        assert!(ours.marker.contains("allowed"), "ours allows: {}", ours.marker);
        // ⚠️ The console's own grant is NOT a permission mode and must never be reported as
        // one — `dontAsk` and `bypassPermissions` are upstream and mean something else.
        assert_eq!(both.mode.mode.as_deref(), Some("dontAsk"), "the plate reports the wire's mode");
        for row in MODE_ROWS {
            assert!(!row.value.contains("session"), "the picker offers no such mode: {}", row.value);
        }
        assert!(!SESSION_ALLOW_CONSEQUENCE.contains("bypass"));

        // …and with the mode back to default, ours is the only marker left standing.
        facts.permission_mode = Some(MODE_DEFAULT.into());
        let ours_only = strip_content(
            None,
            LiveCounts { session_allow: true, ..live(0, 0) },
            &facts,
            Some("abc"),
        );
        assert!(ours_only.mode.marker.is_none());
        assert!(ours_only.session_allow.is_some());
    }

    /// 📌 CONTRACT: the standing allow is **not** counted among the remembered decisions.
    /// It has no card, no key and no `forget`; folding it into that tally would make one
    /// number mean two things and hide the wider grant inside the narrower count.
    #[test]
    fn a_standing_allow_is_not_one_of_the_remembered_decisions() {
        let facts = started("claude-opus-5");
        let counts = LiveCounts { session_allow: true, remembered: 0, ..live(0, 0) };
        let content = strip_content(None, counts, &facts, Some("abc"));
        assert!(
            !content.chips_seen(true).iter().any(|c| c.contains("remembered")),
            "no entries, so no chip: {:?}",
            content.chips
        );
        assert!(content.session_allow.is_some(), "the grant is carried as a marker instead");
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
        let asking = strip_content(None, live(1, 0), &facts, Some("abc"));
        assert!(asking.reading.text.starts_with('◈'), "{}", asking.reading.text);
        let working = strip_content(None, live(0, 2), &facts, Some("abc"));
        assert!(working.reading.text.starts_with('●'), "{}", working.reading.text);
        let writing = strip_content(None, live_generating(0, 0), &facts, Some("abc"));
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

    /// Every non-ASCII character the console is **measured** to be able to draw.
    ///
    /// 🚨 Read out of the `cmap` tables of the four fonts egui 0.33 bundles — `Hack-Regular`,
    /// `Ubuntu-Light`, `NotoEmoji-Regular`, `emoji-icon-font` — rather than assumed. egui does
    /// no OS font fallback, so a codepoint in none of those four is a box on screen no matter
    /// which family the draw site asks for.
    ///
    /// **Adding a symbol to the console means adding it here, which means measuring it.**
    /// That is the point of an allowlist over a blocklist: the previous guard forbade the one
    /// range that had bitten (`U+2500..=U+259F`), and `✓` U+2713 sailed straight past it into
    /// a card that did not exist when the guard was written.
    fn drawable(c: char) -> bool {
        // ASCII is in everything.
        if c.is_ascii() {
            return true;
        }
        // `·` U+00B7 and `×` U+00D7 are in Hack *and* Ubuntu-Light — Latin-1, carried
        // everywhere. `•` U+2022 is in both faces too. `→` U+2192, `◈` U+25C8 and `●`
        // U+25CF are in Hack alone, which is exactly why their draw sites say
        // `.monospace()`. `—` U+2014 is the em dash the prose already uses.
        matches!(c, '·' | '×' | '•' | '→' | '◈' | '●' | '—')
    }

    /// 🚨 CONTRACT: **no site the console draws symbols at may draw one egui has no glyph
    /// for**, and the two rules that keep that true are siblings rather than one rule.
    ///
    /// *Choose* a character the mono face has, *then* ask for the mono face. The band's
    /// earlier tofu fix was the second half alone and was right about its own case; the
    /// subagent card's `✓`/`✗` were already drawn `.monospace()` and were boxes anyway,
    /// because Hack has no dingbats. A guard that only tested the band could not have caught
    /// it — this one covers both sites, and the previous band-only version is the reason the
    /// same defect reached a third site before anyone saw it.
    #[test]
    fn no_symbol_the_console_draws_is_a_glyph_egui_lacks() {
        let mut checked = 0;
        let mut check = |where_: &str, text: &str| {
            for c in text.chars() {
                assert!(
                    drawable(c),
                    "{where_} draws U+{:04X} {c:?}, which is in none of egui's fonts \
                     — pick a character Hack has, or measure this one and list it in \
                     `drawable`: {text}",
                    c as u32
                );
            }
            checked += 1;
        };

        // The band's status half, in every state that carries a symbol.
        let facts = started("claude-opus-5");
        for (name, content) in [
            ("asking", strip_content(None, live(1, 0), &facts, Some("abc"))),
            ("working", strip_content(None, live(0, 2), &facts, Some("abc"))),
            ("writing", strip_content(None, live_generating(0, 0), &facts, Some("abc"))),
        ] {
            check(name, &content.reading.text);
            // The traced set, which is a superset of the quiet one — the glyph guard has to
            // walk every string the band CAN draw, not only the ones it draws by default.
            check(name, &content.chips_seen(true).join(CHIP_SEP));
        }

        // The subagent card's step markers — the site the band-only guard did not reach.
        for state in [
            StepState::Running,
            StepState::Done { is_error: false },
            StepState::Done { is_error: true },
        ] {
            let (mark, _) = step_mark(&state, &Theme::organon());
            check("a subagent step marker", mark);
            // Belt and braces on the one that regressed: the dingbats are gone by name.
            assert!(!mark.contains('✓') && !mark.contains('✗'), "{state:?} is back on a dingbat");
        }
        // 🚨 **The status log's surfaces — the newest sites, and the reason the guard grew.** A
        // disclosure caret (`▾`) and a clock separator are exactly the characters somebody
        // reaches for here, and the first is tofu. Every string these two surfaces can draw is
        // checked: the marks, the summary in all three of its states, the timestamp column, the
        // header's date span (including the `→` a session crossing midnight puts in it), and the
        // plates' new compact faces.
        for open in [true, false] {
            check("the status line's disclosure mark", drop_mark(open));
        }
        check("the status line's name", STATUS_LINE_NAME);
        check("a log row's exception mark", crate::status_log::LOG_MARK_EXCEPTION);
        check("a log row's quiet mark", crate::status_log::LOG_MARK_QUIET);
        {
            use crate::status_log::{LogTime, Remark as R, StatusLog};
            let mut log = StatusLog::default();
            check("an empty log's summary", &log.summary().text);
            log.push(R {
                text: "ok /theme organon".into(),
                always: false,
                at: LogTime { year: 2026, month: 8, day: 21, hour: 23, minute: 58, second: 11 },
            });
            check("a quiet log's summary", &log.summary().text);
            check("a log row's clock", &log.iter().next().expect("one line").at.clock());
            log.push(R::note("could not send: broken pipe"));
            check("an unread log's summary", &log.summary().text);
            log.push(R::note("the agent process ended"));
            check("a multiply-unread log's summary", &log.summary().text);
            log.acknowledge();
            check("an acknowledged log's summary", &log.summary().text);
            check("the log header's date", &log.date_span().expect("two lines have a day"));
            log.push(R {
                text: "after midnight".into(),
                always: false,
                at: LogTime { year: 2026, month: 8, day: 22, hour: 0, minute: 7, second: 3 },
            });
            check(
                "the log header's date span",
                &log.date_span().expect("a crossed midnight still has a span"),
            );
        }
        // The permission plates' compact faces — a mark instead of the mode's name, and two
        // words instead of a sentence. `×` is Latin-1 and in both faces; `◈` is Hack's.
        for mode in [MODE_DEFAULT, "acceptEdits", "dontAsk", "plan"] {
            check("the permission plate's mark", mode_glyph(mode));
            if let Some(marker) = mode_marker(mode) {
                check("a permission marker's band words", &marker.short);
                check("a permission marker's sentence", &marker.text);
            }
        }
        check("the standing allow's mark", SESSION_ALLOW_LABEL);
        check("the standing allow's band words", SESSION_ALLOW_SHORT);
        check("the standing allow's sentence", SESSION_ALLOW_MARKER);
        // The density rows — the newest site, and the reason this guard is an allowlist. A
        // disclosure triangle is the obvious character for both marks and is in none of the
        // four fonts, exactly like the dingbats above.
        for mark in [card_density::MORE, card_density::LESS] {
            check("a density disclosure mark", mark);
        }
        let group = card_density::group_line([&ToolCard {
            call_id: crate::conversation::ToolId("toolu_x".into()),
            name: Some("Read".into()),
            arguments: complete("{}"),
            state: ToolState::Complete { output: String::new(), is_error: false },
            subagent: SubagentLog::default(),
            detail: ResultDetail::default(),
            progress: SubagentProgress { tool_uses: Some(3), ..SubagentProgress::default() },
        }]
        .into_iter());
        check("a group row's count", &group.count);
        check("a group row's verbs", group.verbs.as_deref().unwrap_or_default());
        check("a dispatch magnitude", &card_density::magnitude(&tool_card_for_glyphs(), None).unwrap());

        // The command panel — the newest site, and the one whose obvious characters are the
        // worst offenders: `▸`/`▾` for the highlight and `…` for a `Float`'s band are all
        // tofu. Every string it can draw is checked, including the ones DERIVED from a
        // schema, since a range or an option list is exactly where a stray glyph hides.
        check("the panel's highlight", PALETTE_HERE);
        check("the panel's other rows", PALETTE_THERE);
        check("the compact row's separator", PALETTE_SEP);
        check("the compact row's selection marks", PALETTE_PICKED.0);
        check("the compact row's selection marks", PALETTE_PICKED.1);
        check("the compact row's remainder note", PALETTE_MORE);
        check("the compact row's run marker", PALETTE_RUNS);
        // ⚠️ **The one clause an aliased ring adds to a drawn string.** It appears in a
        // composer refusal and in `/help`, both of which this console draws, and it is not
        // reachable from `palette_specs()` — that fixture has no `ChoiceAliased` and cannot
        // gain one without moving the two compact-row witnesses below. So it is checked
        // directly, against the real region table rather than a made-up pair.
        check(
            "an aliased ring's short-form clause",
            &crate::command::short_form_note(
                crate::region::REGION_ALIASES.iter().copied(),
            ),
        );
        let registry = Registry::new(&palette_specs());
        // ⚠️ `/surface` and `/surface ` are here for the run marker's sake: they are the two
        // lines whose whole row IS that string, and they are the reason it is a constant
        // rather than a literal at the draw site.
        // ⚠️ The three `/organon` lines are here for the strings this crate *writes* rather than
        // reads off a schema: a panel's "— not transplanted yet", a tab's "not mapped yet" mark,
        // and the sentence an empty ring draws through `Palette::hint`. Those are prose, and
        // prose is exactly where a stray dash or ellipsis gets typed.
        for line in [
            "/",
            "/theme ",
            "/camera ",
            "/camera yaw ",
            "/theme c",
            "/surface",
            "/surface ",
            "/organon ",
            "/organon look ",
            "/organon generator ",
        ] {
            let palette = registry.candidates(line).expect("a command line");
            for candidate in &palette.candidates {
                check("a candidate's label", &candidate.label);
                check("a candidate's doc", &candidate.doc);
            }
            if let Some(hint) = palette.hint() {
                check("a slot's hint", &hint);
            }
            if let Some(entry) = palette.verb().and_then(|verb| registry.entry(verb)) {
                check("the panel's head", &entry.usage());
                check("the panel's head note", entry.doc());
            }
            // 🚨 The compact row is checked as a **whole rendered line**, at a width that
            // fits and at one that does not, so the separators, the selection marks and the
            // remainder note are all covered as they are actually assembled rather than only
            // as constants. This is the row a human reads on nearly every frame the panel is
            // open; it is now the primary mode and the largest new draw site in the file.
            for columns in [200, 24, 0] {
                for selected in 0..palette.candidates.len().max(1) {
                    check("the compact row", &compact_line(&palette, selected, columns));
                }
            }
        }
        for receipt in [
            Receipt { ok: true, text: "/theme chocolate".into() },
            Receipt { ok: false, text: "`/theme` needs `name`".into() },
        ] {
            check("a receipt's text", &receipt.text);
        }
        check("a receipt's marker", "ok");
        check("a receipt's marker", "refused");
        // 🚨 **The hole this guard had, and the reason it is worth naming.** `registry::receipt`
        // formats the log's line — and the status band's — in `registry.rs`, and it opened
        // with `✓` (U+2713) for four hours until James photographed a running console drawing
        // `☐ /rig daylight`. The guard existed. It walked an enumerated list of *draw sites*,
        // and a string built in one module and drawn in another fell straight between them.
        // That is the fourth time this exact defect has shipped and every earlier fix was
        // site-local, so the fix this time is to check the **string's producer** from the
        // file that draws it.
        for (typed, result) in [
            ("/surface", Ok(serde_json::Value::Null)),
            ("/theme chocolate", Ok(serde_json::json!({ "accepted": "theme chocolate" }))),
            ("/theme", Err("`/theme` needs `name`".to_string())),
        ] {
            check("the log's receipt line", &crate::registry::receipt(typed, &result));
        }
        check("the log's ok marker", crate::registry::RECEIPT_OK);

        // The live colour editor — the same region, the same hazard. Its group headings are
        // hand-written prose in `theme.rs`'s field macro and its field names are drawn
        // verbatim, so both are enumerated rather than sampled. See `theme_edit::drawn_strings`.
        for text in theme_edit::drawn_strings() {
            check("the theme editor", &text);
        }

        // The exhibit's own drawn strings (§1.13). Every one of these is a *plate* rather than
        // prose — a person sees them exactly when something is missing or wrong, which is the
        // worst moment for a tofu box to appear and suggest the console itself is broken.
        for plate in ["reading...", "cannot show this file", "not a picture", "not a document"] {
            check("an exhibit plate", plate);
        }
        check(
            "the document truncation note",
            &format!("-- shown to {} KB; the file continues --", MAX_DOCUMENT_BYTES / 1024),
        );
        // The terminal placement's notice, from `block_panel` — a string built in one module
        // and drawn in another, which is the exact shape of the hole named above.
        for content in [crate::block_panel::PatchContent::Image, crate::block_panel::PatchContent::Markdown] {
            check("a media patch notice", content.media_notice().expect("a media arm speaks"));
        }
        // 🚨 **The refusals, from `organon_core::exhibit`** — the largest new family of drawn
        // strings and the one most likely to grow. They are built in *another crate* from a
        // path a person typed, so they are checked here, where they are drawn. Note the last
        // case: a non-ASCII **file name** must not reach a plate un-folded, and `Item::new` is
        // what folds it.
        {
            use organon_core::exhibit::{Exhibit, ExhibitError, Item, KNOWN_UNBUILT};
            use std::path::PathBuf;
            for (ext, _) in KNOWN_UNBUILT {
                let err = Exhibit::resolve(&[PathBuf::from(format!("/x/f.{ext}"))])
                    .expect_err("a known-unbuilt kind refuses");
                check("an exhibit refusal", &err.to_string());
            }
            for err in [
                Exhibit::resolve(&[]).unwrap_err(),
                Exhibit::resolve(&[PathBuf::from("/x/f.qqq")]).unwrap_err(),
                Exhibit::resolve(&[PathBuf::from("/a.png"), PathBuf::from("/b.md")]).unwrap_err(),
                ExhibitError::NotYet { path: PathBuf::from("/a.mp3"), why: "reason".into() },
            ] {
                check("an exhibit refusal", &err.to_string());
            }
            check("an item label", &Item::new("/x/\u{8272}\u{5f69}.png").label);
        }

        assert!(checked >= 30, "the guard must actually have looked at something: {checked}");
    }

    /// A settled dispatch card, for the glyph guard above — its magnitude is the one density
    /// string that reaches for a separator (`·`).
    fn tool_card_for_glyphs() -> ToolCard {
        ToolCard {
            call_id: crate::conversation::ToolId("toolu_x".into()),
            name: Some("Agent".into()),
            arguments: complete("{}"),
            state: ToolState::Complete { output: String::new(), is_error: false },
            subagent: SubagentLog::default(),
            detail: ResultDetail::default(),
            progress: SubagentProgress {
                tool_uses: Some(3),
                duration_ms: Some(12_400),
                ..SubagentProgress::default()
            },
        }
    }

    /// One frame of the strip, headless — the same shape [`composer_frame`] uses, measuring
    /// the room the band took *away from what follows it*.
    fn strip_frame(ctx: &egui::Context, content: &StripContent, pane: &mut FakePane) -> (f32, f32) {
        strip_frame_at(ctx, content, pane, 900.0)
    }

    /// [`strip_frame`] at a chosen pane width — the narrow case is the one that breaks a band,
    /// and a fixed 900 pt harness cannot see it.
    fn strip_frame_at(
        ctx: &egui::Context,
        content: &StripContent,
        pane: &mut FakePane,
        width: f32,
    ) -> (f32, f32) {
        let mut band = 0.0;
        let mut left = 0.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 700.0),
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
                    // Quiet, like a real tab: the band's reserved height is one row whatever
                    // the mode hides, which is the property this harness measures.
                    let _ = strip_box(ui, content, &[], &Theme::organon(), false);
                    band = before - ui.available_height();
                    ui.add_space(4.0);
                    let _ = composer_box(
                        ui,
                        &mut pane.text,
                        pane.live,
                        &mut pane.want_focus,
                        &mut pane.measured,
                        &Theme::organon(),
                    );
                    left = ui.available_height();
                });
            });
        });
        (band, left)
    }

    /// One whole frame of the real [`draw`], headless — every child in its real order, which is
    /// the only way a layout invariant about one of them can be measured at all. Answers where
    /// the entry box landed, via [`composer_rect`].
    fn draw_frame(
        ctx: &egui::Context,
        pane: &mut ConversationPane,
        height: f32,
    ) -> Option<egui::Rect> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, height),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let _ = draw(
                    ui,
                    pane,
                    &SurfaceImages::new(),
                    &Default::default(),
                    &Theme::organon(),
                    "organon",
                    &Form::TERMINAL,
                    crate::panel_stack::Home::Nowhere,
                );
            });
        });
        composer_rect(ctx)
    }

    /// 🚨🚨 **CONTRACT: THE ENTRY BOX NEVER MOVES.** This is the reason #129 exists.
    ///
    /// James, 2026-08-21, on the surface #127 shipped: *"its positioning isn't right. It should
    /// not be displacing the entry box. The entry box should never move. So put the entry box
    /// back where it was and put the status log at the top, sort of like a Quake console
    /// drop-down."* #127 drew the log between the band and the composer in a bottom-up column,
    /// so opening it pushed the box James types into up the screen by nine rows.
    ///
    /// **What this pins is the rect, not the arrangement**: the composer's rect with the log
    /// closed must equal its rect with the log open, *exactly* — not "about the same" — and at
    /// more than one pane height, because a share-of-the-pane bound is precisely the kind that
    /// holds at 700 pt and fails at 360. It also closes the log again and checks the box came
    /// back to the same place, which is the direction a linger or an animation would break.
    ///
    /// ⚠️ **Mutation-checked**: put `log_drop_down` back in the bottom-up column as a child (or
    /// give the status line a height that varies with the summary) and the second assertion
    /// fails naming both rects. A prose invariant is what got us here.
    #[test]
    fn the_entry_box_never_moves_when_the_status_log_opens() {
        for height in [700.0_f32, 460.0, 360.0] {
            let ctx = egui::Context::default();
            let mut pane = rewrap_bench::bench_pane(Transcript::new());
            pane.want_focus = false;
            // A log with real content in it, including an exception — so the summary is in its
            // widest state and the drop-down has more rows than it can show.
            for i in 0..40 {
                pane.trace(format!("ok /viewport center agent ({i})"));
            }
            pane.note("could not send: broken pipe".to_string());

            let mut closed = None;
            for _ in 0..3 {
                closed = draw_frame(&ctx, &mut pane, height);
            }
            let closed = closed.expect("the composer drew and published its rect");

            pane.set_tracing(true);
            let mut open = None;
            for _ in 0..3 {
                open = draw_frame(&ctx, &mut pane, height);
            }
            let open = open.expect("the composer still drew with the log open");
            assert_eq!(
                closed, open,
                "the status log moved the entry box at {height} pt: closed {closed:?}, open \
                 {open:?}",
            );

            pane.set_tracing(false);
            let mut shut = None;
            for _ in 0..3 {
                shut = draw_frame(&ctx, &mut pane, height);
            }
            assert_eq!(
                Some(closed),
                shut,
                "the entry box did not come back to where it was at {height} pt",
            );
            assert!(closed.width() > 0.0 && closed.height() > 0.0, "an empty rect proves nothing");
        }
    }

    /// 🚨 CONTRACT: **the status line is one row in every state it can be in.**
    ///
    /// It is permanent and it sits above the scrollback, so a line that grew with what it had to
    /// say would reflow the transcript under it every time the console said something — the
    /// reshuffle the band spent a whole tier eliminating, arriving at a new surface.
    ///
    /// ⚠️ **Mutation-checked**: make the summary label `.wrap()` instead of `.truncate()` and
    /// this fails at state 2 with `[30.0, 30.0, 42.0, …]` — the long line becoming two rows.
    ///
    /// ⚠️ **And a correction worth keeping, because I got it wrong first.** I claimed *dropping*
    /// `.truncate()` would fail this, and it does not: an egui `left_to_right` layout defaults to
    /// `Extend`, so an unbounded label runs off the end at exactly one row tall. `.truncate()`'s
    /// real job here is the **horizontal** bound — it is what keeps the summary off the surface's
    /// name at the right, the same rule as [`strip_right_reserve`] one surface over — and height
    /// cannot see that any more than it could see the band's overlap. Same lesson as
    /// [`the_bands_two_halves_never_overlap_however_narrow_it_gets`], found the same way: by
    /// running the mutation instead of asserting it.
    #[test]
    fn the_status_line_is_one_row_in_every_state() {
        let ctx = egui::Context::default();
        let theme = Theme::organon();
        let line_at = |ctx: &egui::Context, pane: &mut ConversationPane, width: f32| {
            let mut rect = egui::Rect::NOTHING;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 700.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    rect = status_line(ui, pane, &theme);
                });
            });
            rect
        };
        let mut pane = rewrap_bench::bench_pane(Transcript::new());
        pane.want_focus = false;
        let mut heights: Vec<f32> = Vec::new();
        for width in [900.0_f32, 300.0] {
            // Empty, quiet, broken, read — the four the summary can be in.
            heights.push(line_at(&ctx, &mut pane, width).height());
            pane.trace("stderr: Warning: no stdin data received on the first turn".to_string());
            heights.push(line_at(&ctx, &mut pane, width).height());
            pane.note(
                "approvals are not wired (address already in use) — a tool that needs \
                 permission will fail instead of asking, and nothing else on screen says so"
                    .to_string(),
            );
            heights.push(line_at(&ctx, &mut pane, width).height());
            pane.log.acknowledge();
            heights.push(line_at(&ctx, &mut pane, width).height());
        }
        let first = heights[0];
        assert!(first > 0.0, "the status line took no space at all: {heights:?}");
        for (i, h) in heights.iter().enumerate() {
            assert!(
                (h - first).abs() < 0.5,
                "the status line changed height at state {i}: {heights:?}",
            );
        }
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
                session_allow: true,
                has_session: true,
                generating: true,
            },
            &facts,
            Some("11111111-2222-3333-4444-555555555555"),
        )
        .switching_to(Some("Default (recommended)"));
        let empty = strip_content(None, LiveCounts::default(), &SessionFacts::default(), None);

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

    /// 🚨 **THE ANTI-RESHUFFLE PROPERTY, and the whole of why this tier exists.**
    ///
    /// The dim half used to be *empty* until the first turn's `result`, at which point the
    /// cost, the ring and the last-turn figure all arrived at once and the band a hand had
    /// been looking at for a minute rearranged itself. So the contract is in two halves and
    /// both are asserted here rather than inferred:
    ///
    /// 1. **At a cold start the band already reports something** — the session's spend, at
    ///    its true value of nought, and a ring, as a track with no arc in it.
    /// 2. **The band is exactly as tall then as it is fully populated.** This is the
    ///    assertion `the_strip_is_one_band_and_leaves_the_scrollback_the_rest` also makes,
    ///    and it is deliberately made twice: there it is a corollary of "one line, whatever
    ///    arrives", here it is the primary claim. An always-present ring is a new child of
    ///    the horizontal layout, and a child taller than the reserved row is precisely how a
    ///    one-line strip becomes two.
    #[test]
    fn the_cold_band_reports_a_cost_and_a_ring_and_does_not_grow() {
        let theme = Theme::organon();
        let cold = strip_content(None, LiveCounts::default(), &SessionFacts::default(), None);

        // (1) There is something on the right from the first frame, and it is true.
        assert_eq!(
            cold.chips_seen(true),
            vec!["session $0.0000"],
            "nought spent is a measurement, not a placeholder"
        );
        assert_eq!(cold.context, ContextSlot::Unknown, "…and the ring has no reading yet");
        assert_eq!(
            ring_track_color(&cold.context, &theme),
            theme.context_track_empty,
            "which draws the empty track — present, and not a claim of 0%"
        );
        assert!(
            !cold.chips_seen(true).iter().any(|c| c.contains("last turn")),
            "there has been no last turn, so no duration is invented for one: {:?}",
            cold.chips
        );

        // (2) …and the band it sits in is the height it will always be.
        let ctx = egui::Context::default();
        let mut pane = FakePane::new("x");
        pane.want_focus = false;

        let mut facts = started("claude-opus-5[1m]");
        facts.cost_usd = Some(12.3456);
        facts.last_turn_duration_ms = Some(7_389);
        facts.context_window = Some(1_000_000);
        facts.last_prompt_tokens = Some(910_000);
        let settled = strip_content(
            None,
            LiveCounts { remembered: 9, has_session: true, ..live(0, 3) },
            &facts,
            Some("11111111-2222-3333-4444-555555555555"),
        );
        assert_eq!(settled.chips.len(), 3, "the settled band carries all three chips");

        let mut cold_band = 0.0;
        for _ in 0..3 {
            cold_band = strip_frame(&ctx, &cold, &mut pane).0;
        }
        let mut settled_band = 0.0;
        for _ in 0..3 {
            settled_band = strip_frame(&ctx, &settled, &mut pane).0;
        }
        assert!(cold_band > 0.0, "the cold band is real space: {cold_band}");
        assert!(
            (cold_band - settled_band).abs() < 0.5,
            "the band must not grow when the first turn lands — that is the reshuffle: \
             cold {cold_band} vs settled {settled_band}"
        );
    }

    // -----------------------------------------------------------------------
    // The command panel
    // -----------------------------------------------------------------------

    /// A registry with the shapes that matter: a required `Choice`, a verb that takes
    /// nothing, the all-optional camera, and — since the line completing itself made it
    /// load-bearing — **a verb that is another verb's prefix**. The real table lives in
    /// `console_main`, which this crate cannot see, the same reason `registry.rs`'s own
    /// fixture exists; `camera`/`camera.read` is copied from it because a count of one is the
    /// whole trigger for a completion and `/camera` looks finished while being two.
    ///
    /// ⚠️ **Every verb here is `Recoverable`, and the fixture is still not one-sided**:
    /// `Registry::new` always adds the view lane, so `surface` and `organon` (both
    /// `Permanent`) and `help` (`Recoverable`) are in this table too. `/su` and `/h` are
    /// therefore the two sides of the autorun rule, with no spec to add for either.
    fn palette_specs() -> Vec<CommandSpec> {
        use crate::command::{ArgKind, ArgSpec, Reversal, TargetKind};
        vec![
            CommandSpec {
                name: "console.theme".into(),
                doc: "Every colour the console paints".into(),
                target: TargetKind::Viewport,
                args: vec![ArgSpec {
                    name: "name".into(),
                    // The two editor words are values of this argument in the real spec too
                    // (`console_main::console_specs`), and they have to be here or
                    // `Registry::resolve` refuses `/theme edit` during validation — before the
                    // intercept in `run_command` can ever see it.
                    kind: ArgKind::Choice(
                        ["organon", "light", "dark", "chocolate"]
                            .into_iter()
                            .map(str::to_string)
                            .chain(theme_edit::EDIT_WORDS.iter().map(|s| (*s).to_string()))
                            .collect(),
                    ),
                    required: true,
                }],
                reversal: Reversal::Recoverable,
            },
            CommandSpec {
                name: "console.camera".into(),
                doc: "Where the viewer stands".into(),
                target: TargetKind::Viewport,
                args: vec![
                    ArgSpec { name: "reset".into(), kind: ArgKind::Bool, required: false },
                    ArgSpec {
                        name: "yaw".into(),
                        kind: ArgKind::Float { min: -180.0, max: 180.0 },
                        required: false,
                    },
                ],
                reversal: Reversal::Recoverable,
            },
            CommandSpec {
                name: "console.camera.read".into(),
                doc: "Where the viewer stands right now".into(),
                target: TargetKind::Viewport,
                args: Vec::new(),
                reversal: Reversal::Recoverable,
            },
        ]
    }

    /// 🚨 CONTRACT: **both editor words open the editor, and neither reaches the dispatch.**
    /// `/theme edit` is a console-lane verb answered locally — the one place that happens — so
    /// this pins both halves: the editor exists afterwards, and the pane's `local` dispatch was
    /// not called (it is `NoDispatch`, which would have produced a failed receipt).
    #[test]
    fn both_editor_words_open_the_editor_without_dispatching() {
        for word in theme_edit::EDIT_WORDS {
            let mut pane = palette_pane();
            pane.composer = format!("/theme {word}");
            pane.submit(&Theme::light(), "light");

            assert!(pane.theme_edit.is_some(), "`/theme {word}` did not open the editor");
            let receipt = pane.receipt.as_ref().expect("a receipt").receipt.clone();
            assert!(receipt.ok, "`/theme {word}` was refused: {}", receipt.text);
            assert!(receipt.text.contains("light"), "the receipt names the palette: {receipt:?}");
            assert!(
                receipt.text.contains("nothing is stored"),
                "the receipt must say the edits are not saved yet: {receipt:?}"
            );
            assert_eq!(pane.composer, "", "a command that ran clears the composer");
            let editor = pane.theme_edit.as_ref().unwrap();
            assert_eq!(editor.name(), "light");
            assert_eq!(editor.working(), &Theme::light(), "opening changed a colour");
            assert_eq!(editor.unsaved(), 0);
        }
    }

    /// CONTRACT: a palette **name** still dispatches, and opens no editor. This is the arm the
    /// intercept must not swallow — it is every other value of the same argument.
    #[test]
    fn a_palette_name_still_goes_to_the_dispatch() {
        let mut pane = palette_pane();
        pane.composer = "/theme chocolate".into();
        pane.submit(&Theme::light(), "light");
        assert!(pane.theme_edit.is_none(), "a palette name opened the editor");
        // `NoDispatch` refuses, which is what proves the call actually left this crate.
        let receipt = pane.receipt.as_ref().expect("a receipt").receipt.clone();
        assert!(!receipt.ok, "NoDispatch should have refused: {receipt:?}");
    }

    /// 🚨 CONTRACT: **the editor takes the band from the candidate list**, and gives it back
    /// when it closes. Two surfaces cannot own one region; §1.9 already made that argument for
    /// the receipt and the candidates, and the editor is the third claimant.
    #[test]
    fn the_editor_owns_the_band_while_it_is_open() {
        let ctx = egui::Context::default();
        let mut pane = palette_pane();

        // A command line with the editor shut: the candidate list has the band.
        pane.composer = "/theme ".into();
        let (candidates, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert!(candidates > 0.0, "the candidate list drew nothing");

        // Same line, editor open: the band is the editor's and it is a different height.
        pane.open_theme_editor(&Theme::organon(), "organon", None);
        let (editor, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert!(editor > 0.0, "the editor drew nothing");
        assert_ne!(
            editor, candidates,
            "the band is the same height either way, so it is drawing the same thing — the \
             editor is not actually taking the region from the candidate list"
        );
        assert!(pane.theme_edit.is_some(), "drawing a frame closed the editor");

        // Escape closes it, and the band goes back to the candidates.
        let (after, _) = palette_frame(
            &ctx,
            &mut pane,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(pane.theme_edit.is_none(), "Escape did not close the editor");
        assert_eq!(
            after, candidates,
            "the band did not go back to the candidate list after the editor closed"
        );
    }

    /// 🚨 CONTRACT: **the editor closes itself if the palette changes underneath it.** Its held
    /// HSV describes colours that are no longer on screen, so the next drag would snap a field
    /// to a hue from the outgoing palette. `/theme chocolate` typed elsewhere, the CLI, or an
    /// agent's tool call can all do this while an editor is open.
    #[test]
    fn the_editor_closes_when_the_palette_is_repainted_under_it() {
        let ctx = egui::Context::default();
        let mut pane = palette_pane();
        pane.open_theme_editor(&Theme::light(), "light", None);

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        };
        let mut change = None;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    // Note the palette handed in is NOT the one the editor was opened on.
                    change = command_panel(ui, &mut pane, &Theme::chocolate(), &Form::TERMINAL).1;
                });
            });
        });
        assert!(pane.theme_edit.is_none(), "the editor survived a repaint underneath it");
        assert!(change.is_none(), "a closing editor must not also emit a palette");
        assert!(
            pane.log.iter().any(|l| l.text.contains("the palette changed")),
            "the console said nothing about closing the editor: {:?}",
            pane.log
        );
    }

    /// CONTRACT: an untouched editor emits nothing. `console_main` re-derives egui's whole
    /// chrome for every change it is handed, so a `Some` per frame would rebuild and re-upload
    /// it sixty times a second for a palette nobody is touching.
    #[test]
    fn an_untouched_editor_asks_for_nothing() {
        let ctx = egui::Context::default();
        let mut pane = palette_pane();
        pane.open_theme_editor(&Theme::organon(), "organon", None);
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                ..Default::default()
            };
            let mut change = None;
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        change =
                            command_panel(ui, &mut pane, &Theme::organon(), &Form::TERMINAL).1;
                    });
                });
            });
            assert!(change.is_none(), "an idle editor asked for a repaint");
        }
    }

    /// A pane with a real registry, no agent, and nothing in the transcript.
    /// 🚨 **`autorun` is ON here because it is on in the product**, and a palette test running
    /// against a configuration nobody ships is a test of nothing. `bench_pane` leaves it off —
    /// a bench must not run commands — so this is where it is put back. The handful of tests
    /// that are about *completion* rather than running switch it off again by name, which
    /// makes that a statement rather than an accident.
    fn palette_pane() -> ConversationPane {
        let mut pane = rewrap_bench::bench_pane(Transcript::new());
        pane.registry = Registry::new(&palette_specs());
        pane.want_focus = true;
        pane.autorun = autorun_enabled(None);
        pane
    }

    /// One frame of composer + panel, in the real bottom-up arrangement. Returns the room the
    /// **panel** took away from what follows it, and what the scrollback was left with.
    ///
    /// 🚨 **Every frame this harness runs also asserts the panel did not paint over the
    /// composer**, which is the defect James reported on 2026-08-14 (*"Your current box
    /// extends lower than that and covers a bit of the text"*). It is checked here rather
    /// than only in a test of its own because the failure is a *height arithmetic* bug: it
    /// appears whenever a row is added, a font changes or a posture widens the line, and a
    /// single dedicated test would only ever cover the list that existed when it was written.
    fn palette_frame(
        ctx: &egui::Context,
        pane: &mut ConversationPane,
        events: Vec<egui::Event>,
    ) -> (f32, f32) {
        let mut band = 0.0;
        let mut left = 0.0;
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
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    composer(ui, pane, &Theme::organon(), "organon");
                    let before = ui.available_height();
                    let (over, _) = command_panel(ui, pane, &Theme::organon(), &Form::TERMINAL);
                    assert_eq!(
                        over, 0.0,
                        "the plate outgrew the band it reserved by {over} pt, and in a \
                         bottom-up column that is painted straight over the composer"
                    );
                    band = before - ui.available_height();
                    left = ui.available_height();
                });
            });
        });
        (band, left)
    }

    /// The compact row a human would see for the pane's line, as text — or `""` when no panel
    /// is drawn at all.
    ///
    /// 🚨 **Through [`drawn_palette`], which is the renderer's own decision**, so this cannot
    /// drift from what `command_panel` puts on screen the way a hand-written expectation
    /// would. `ctx` is taken because the answer is only meaningful after a frame has run: the
    /// line completes itself during one, and the row describes the line as it ends up.
    fn row_for(pane: &mut ConversationPane, ctx: &egui::Context) -> String {
        let _ = palette_frame(ctx, pane, Vec::new());
        pane.palette()
            .and_then(|p| drawn_palette(p, &pane.composer))
            .map(|p| compact_line(&p, pane.palette_selected, 200))
            .unwrap_or_default()
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]
    }

    /// 🚨 CONTRACT: **Tab completes, Enter runs, and neither is ever the other.**
    ///
    /// The trap is the one [`composer_key`] documents from the other side:
    /// `matches_logically` is permissive about shift, so the obvious spelling of "Tab" would
    /// swallow Shift+Tab and the obvious spelling of "Escape" would swallow Ctrl+Escape.
    /// `matches_exact` is what keeps the table below honest.
    #[test]
    fn the_panels_keys_are_exact_and_never_the_send_key() {
        use egui::{Key, Modifiers};
        assert_eq!(palette_key(Key::Tab, Modifiers::NONE), PaletteKey::Accept);
        assert_eq!(palette_key(Key::Tab, Modifiers::SHIFT), PaletteKey::Prev);
        assert_eq!(palette_key(Key::ArrowDown, Modifiers::NONE), PaletteKey::Next);
        assert_eq!(palette_key(Key::ArrowUp, Modifiers::NONE), PaletteKey::Prev);
        assert_eq!(palette_key(Key::Escape, Modifiers::NONE), PaletteKey::Dismiss);
        // 🚨 Enter belongs to the composer in every state. A panel that could send would be
        // a completion surface that occasionally messages an agent.
        for mods in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::CTRL] {
            assert_eq!(palette_key(Key::Enter, mods), PaletteKey::Ignore, "{mods:?}");
        }
        // …and the modified spellings the permissive matcher would have eaten.
        for (k, m) in [
            (Key::Tab, Modifiers::CTRL),
            (Key::ArrowDown, Modifiers::SHIFT),
            (Key::ArrowUp, Modifiers::CTRL),
            (Key::Escape, Modifiers::SHIFT),
            (Key::Escape, Modifiers::CTRL),
        ] {
            assert_eq!(palette_key(k, m), PaletteKey::Ignore, "{k:?} + {m:?}");
        }
    }

    /// The highlight wraps, and an empty list has no row to be on — the arithmetic that would
    /// otherwise panic under a held-down arrow key.
    #[test]
    fn the_highlight_wraps_and_an_empty_list_has_no_row() {
        assert_eq!(move_selection(0, 3, true), 1);
        assert_eq!(move_selection(2, 3, true), 0, "forward from the last wraps to the first");
        assert_eq!(move_selection(0, 3, false), 2, "and back from the first to the last");
        assert_eq!(move_selection(0, 0, true), 0, "nothing to move through");
        assert_eq!(move_selection(0, 0, false), 0);
        assert_eq!(move_selection(9, 3, true), 0, "an index left over from a longer list");
    }

    /// 🚨 CONTRACT: **a refusal outlives a success.** The asymmetry is the point — a
    /// confirmation nobody reads cost nothing, a refusal nobody reads cost the command.
    #[test]
    fn a_refusal_holds_the_band_and_a_confirmation_lets_it_go() {
        // Both hold while the line they answer is untouched.
        assert!(receipt_holds(true, "", "", 0.0));
        assert!(receipt_holds(false, "/theme", "/theme", 0.0));
        // A success ages out; a refusal does not, however long it sits there.
        assert!(!receipt_holds(true, "", "", RECEIPT_SECONDS + 0.1));
        assert!(receipt_holds(false, "/theme", "/theme", RECEIPT_SECONDS * 1000.0));
        // Either goes the moment the line moves on, which is what hands the region back to
        // the candidates.
        assert!(!receipt_holds(false, "/theme", "/theme ", 0.0));
        assert!(!receipt_holds(true, "", "/", 0.0));
    }

    /// 🚨 CONTRACT: **the compact row is the vocabulary, never a copy of it.**
    ///
    /// James wrote the row he wanted out by hand —
    /// `surface|theme|posture|background|rig|patch|portal|camera` — and the one thing this
    /// must not do is contain that string. It is built from [`Registry::candidates`], so it
    /// narrows as letters are typed for free, and it gains a verb on the day the catalog does
    /// rather than on the day somebody remembers.
    #[test]
    fn the_compact_row_is_the_registrys_own_words() {
        let registry = Registry::new(&palette_specs());
        let row = |line: &str, selected: usize| {
            compact_line(&registry.candidates(line).expect("a command line"), selected, 200)
        };
        // Everything, with the word Tab would take marked. ⚠️ `organon` is here because the
        // fixture's registry gained it with §1.11's ring — the row is the registry's own
        // words, so a verb added anywhere lands in this string without anyone editing it.
        assert_eq!(
            row("/", 0),
            "[theme] | camera | camera.read | surface | help | trace | media | organon"
        );
        assert_eq!(
            row("/", 2),
            "theme | camera | [camera.read] | surface | help | trace | media | organon"
        );
        // …and it narrows, because the generator does.
        assert_eq!(row("/c", 0), "[camera] | camera.read");
        // The value ring is the same row: an `ArgKind::Choice` IS a list of words.
        // ⚠️ `edit` and `adjust` sit in this row beside the four palettes because they are
        // values of the *same* argument (§1.10) — the live editor is reached by completing
        // `/theme` like anything else. A row that hid them would be the surface disagreeing
        // with the registry, which is the one thing this test exists to prevent.
        assert_eq!(row("/theme ", 1), "organon | [light] | dark | chocolate | edit | adjust");
        // ⚠️ And where there are no words there is the sentence, which is the only place
        // `Palette::hint` has ever been drawn.
        assert_eq!(row("/camera yaw ", 0), "yaw: a number from -180 to 180");
    }

    /// The row counts what it could not fit rather than truncating it.
    ///
    /// ⚠️ **Not a taste.** egui's own truncation appends `…`, which is in none of its bundled
    /// fonts and would ship as a box — the defect the glyph allowlist exists to catch, and
    /// the reason this arithmetic is here at all rather than being left to `Label::truncate`.
    #[test]
    fn the_compact_row_counts_what_it_could_not_fit() {
        let words = |n: usize| -> Vec<CompactWord> {
            (0..n)
                .map(|_| CompactWord { text: "abcd".into(), here: false, runs: false })
                .collect()
        };
        // "abcd | abcd | abcd" is 18 columns.
        assert_eq!(compact_fit(&words(3), 18), (3, 0), "it all fits");
        assert_eq!(compact_fit(&words(3), 17), (2, 1), "…and one short, one is dropped");
        // The note grows with the count, and is paid for out of the same width.
        assert_eq!(compact_fit(&words(20), 0), (0, 20), "a pane with no room shows the count");
        assert_eq!(compact_fit(&[], 40), (0, 0), "nothing to fit and nothing hidden");
        let registry = Registry::new(&palette_specs());
        let palette = registry.candidates("/").expect("a command line");
        assert_eq!(compact_line(&palette, 0, 24), "[theme] | camera | +6");
    }

    /// The list is capped rather than scrolled — see [`PALETTE_MAX_ROWS`] for the measured
    /// reason a `ScrollArea` cannot go here.
    #[test]
    fn a_long_list_is_capped_and_says_how_much_it_left() {
        assert_eq!(palette_rows(3), (3, 0));
        assert_eq!(palette_rows(PALETTE_MAX_ROWS), (PALETTE_MAX_ROWS, 0));
        assert_eq!(palette_rows(PALETTE_MAX_ROWS + 5), (PALETTE_MAX_ROWS, 5));
        assert_eq!(palette_rows(0), (0, 0));
    }

    /// 🚨 **THE RULE THAT MUST NOT BREAK: the composer is also where a human talks to the
    /// agent, so nothing may pop up over prose.**
    ///
    /// Driven through real frames rather than through [`Registry::candidates`] alone, because
    /// the property being defended is about the *region*: a panel that took no height took no
    /// height from the scrollback either.
    #[test]
    fn the_panel_opens_only_for_a_command_line() {
        let ctx = egui::Context::default();
        for prose in ["what does /surface do?", "hello", "", "//surface"] {
            let mut pane = palette_pane();
            pane.composer = prose.to_string();
            let (band, _) = palette_frame(&ctx, &mut pane, Vec::new());
            assert_eq!(band, 0.0, "{prose:?} must draw no panel at all");
        }
        let mut pane = palette_pane();
        pane.composer = "/".to_string();
        let (band, left) = palette_frame(&ctx, &mut pane, Vec::new());
        assert!(band > 0.0, "…and a bare slash must draw one: {band}");
        assert!(left > 300.0, "which still leaves the scrollback the remainder: {left}");
    }

    /// 🚨 CONTRACT: **a line Enter would run says so, rather than showing nothing.**
    ///
    /// James, on a running build, 2026-08-14: *"slash surface shows no options."* `surface`
    /// takes no arguments, so there genuinely are none — and the row went blank, then (once a
    /// space was typed after the verb) the panel disappeared outright. A panel that vanishes is
    /// read as a broken one, and this is the fifth time this week the console has known
    /// something and said nothing.
    ///
    /// ⚠️ Both spellings are pinned because they fail *differently*: `/surface` had a
    /// redundant one-item list that the renderer dropped, leaving an empty row; `/surface `
    /// had no candidates at all, so [`Palette::is_empty`] was true and there was no panel to
    /// draw. One fix would not have found the other.
    #[test]
    fn a_finished_command_says_that_enter_would_run_it() {
        let ctx = egui::Context::default();
        let mut settled = palette_pane();
        settled.composer = "/surface".to_string();
        assert_eq!(
            row_for(&mut settled, &ctx),
            "Enter runs",
            "the verb is complete and takes nothing, so this is the whole truth about it"
        );
        let (band, _) = palette_frame(&ctx, &mut settled, Vec::new());
        assert!(band > 0.0, "…and it is a drawn row, not a hidden one: {band}");

        let mut spaced = palette_pane();
        spaced.composer = "/surface ".to_string();
        assert_eq!(row_for(&mut spaced, &ctx), "Enter runs", "the trailing-space spelling too");

        // A runnable line that still has continuations shows both, and the run marker leads —
        // `compact_fit` drops from the tail, so last would be first to be hidden.
        let mut more = palette_pane();
        more.composer = "/camera ".to_string();
        assert_eq!(row_for(&mut more, &ctx), "Enter runs | [reset] | yaw", "both, run marker first");

        // ⚠️ And the other half, untouched: a line that runs nothing still says nothing.
        let mut half = palette_pane();
        half.composer = "/theme ".to_string();
        assert!(
            !row_for(&mut half, &ctx).contains(PALETTE_RUNS),
            "`/theme` still needs a value, so Enter would refuse it"
        );
        for prose in ["what does /surface do?", "hello", ""] {
            let mut pane = palette_pane();
            pane.composer = prose.to_string();
            assert_eq!(row_for(&mut pane, &ctx), "", "{prose:?} must draw no panel at all");
        }
    }

    /// 🚨 CONTRACT: **the primary panel is one row and stays one row, however many words it
    /// has to show.** That is the whole of what James asked for — *"only one row high"* — and
    /// it is also what makes the region above the composer stop moving: a band whose height
    /// changed with the list pushed the scrollback up and down on every keystroke, which the
    /// honesty ledger already named as the second-most-likely reason the panel would not be
    /// worth having.
    #[test]
    fn the_compact_panel_is_one_row_whatever_the_list_holds() {
        let ctx = egui::Context::default();
        let mut one = palette_pane();
        one.composer = "/c".to_string();
        let (narrow, _) = palette_frame(&ctx, &mut one, Vec::new());
        let mut all = palette_pane();
        all.composer = "/theme ".to_string();
        let (wide, left) = palette_frame(&ctx, &mut all, Vec::new());
        assert!(narrow > 0.0, "two words still draw a row: {narrow}");
        assert_eq!(wide, narrow, "…and four draw the same one row");
        assert!(wide < 60.0, "a row, not a page: {wide}");
        assert!(left > 300.0, "the scrollback keeps the rest of the 700 pt pane: {left}");
    }

    /// The verbose list is the old panel, whole: as tall as it has rows, and it stops. The
    /// busiest list this crate can build must not eat the pane — the failure this file has
    /// already had once.
    #[test]
    fn the_verbose_panel_grows_with_its_rows_and_never_swallows_the_pane() {
        let ctx = egui::Context::default();
        let mut one = palette_pane();
        one.verbose = true;
        one.composer = "/c".to_string();
        let (narrow, _) = palette_frame(&ctx, &mut one, Vec::new());
        let mut all = palette_pane();
        all.verbose = true;
        all.composer = "/theme ".to_string();
        let (wide, left) = palette_frame(&ctx, &mut all, Vec::new());
        assert!(narrow > 0.0 && wide > narrow, "four rows exceed one: {narrow} vs {wide}");
        assert!(wide < 300.0, "…and the panel is still a band, not a page: {wide}");
        assert!(left > 300.0, "the scrollback keeps the rest of the 700 pt pane: {left}");
    }

    /// 🚨 **Tab completes the line — whole — and sends nothing.**
    ///
    /// Driven with a real Tab through a real frame, because the half that cannot be tested
    /// any other way is the *consumption*: egui's focus manager reads Tab out of the raw
    /// input before this file runs, which is why `lock_focus(true)` is on the widget.
    #[test]
    fn tab_completes_the_line_and_enter_is_what_runs_it() {
        let ctx = egui::Context::default();
        let mut pane = palette_pane();
        // ⚠️ `/th` cannot be used here any more: `theme` is the only verb it leaves, so the
        // line completes itself before a Tab could reach it. `/c` leaves two.
        pane.composer = "/c".to_string();
        // One frame to take focus, the way the composer's own tests do.
        let _ = palette_frame(&ctx, &mut pane, Vec::new());
        assert_eq!(pane.composer, "/c", "two candidates, so nothing has been decided");

        let _ = palette_frame(&ctx, &mut pane, key(egui::Key::Tab, egui::Modifiers::NONE));
        assert_eq!(pane.composer, "/camera ", "the whole line, with the next ring opened");
        assert!(pane.transcript.elements().is_empty(), "and nothing was sent or run");

        // The next ring completes the same way, and Tab is still not a send.
        let mut theme = palette_pane();
        theme.composer = "/theme ".to_string();
        let _ = palette_frame(&ctx, &mut theme, Vec::new());
        let _ = palette_frame(&ctx, &mut theme, key(egui::Key::Tab, egui::Modifiers::NONE));
        assert_eq!(theme.composer, "/theme organon", "the highlighted option, taken whole");
        assert!(theme.receipt.is_none(), "Tab still has not run anything");

        // ⚠️ And Enter on an INCOMPLETE command refuses by name without clearing the box —
        // the behaviour that makes "Enter never accepts" affordable.
        let mut half = palette_pane();
        half.composer = "/theme".to_string();
        let _ = palette_frame(&ctx, &mut half, Vec::new());
        // ⚠️ The line completed to `/theme ` on that first frame — one candidate — which is
        // the point: completing opened the value ring, and the command is *still* incomplete,
        // so Enter must still refuse it rather than run a half-command.
        let _ = palette_frame(&ctx, &mut half, enter(egui::Modifiers::NONE));
        assert_eq!(half.composer, "/theme ", "a refusal never swallows what was typed");
        let refused = half.receipt.as_ref().expect("the refusal is shown where it was typed");
        assert!(!refused.receipt.ok);
        assert!(refused.receipt.text.contains("needs `name`"), "{}", refused.receipt.text);
    }

    /// Escape shuts the panel until the line moves, and asks for the focus egui's own focus
    /// manager took on the way past — see [`palette_key`] for why that repair is necessary.
    ///
    /// 🚨 **The second half is a shipped bug, retyped exactly as James hit it.** The
    /// dismissal used to be the composer's *text* at the moment Escape was pressed, compared
    /// for equality every frame — so once `/c` had been dismissed, every future `/c` was
    /// silently refused a panel for the life of the tab (*"Now my tab completion broke. When
    /// I type slash p, nothing comes up"*). Content equality cannot say "has changed since";
    /// only watching the change can. Clearing the box and retyping the identical string is
    /// the exact case that reached him and the one this pins.
    #[test]
    fn escape_shuts_the_panel_until_the_line_changes() {
        let ctx = egui::Context::default();
        let mut pane = palette_pane();
        // Two candidates, so the line sits still instead of completing itself.
        pane.composer = "/c".to_string();
        let _ = palette_frame(&ctx, &mut pane, Vec::new());
        let (open, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert!(open > 0.0);

        let _ = palette_frame(&ctx, &mut pane, key(egui::Key::Escape, egui::Modifiers::NONE));
        let (shut, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert_eq!(shut, 0.0, "dismissed");
        assert!(pane.want_focus || ctx.memory(|m| m.focused().is_some()), "focus is asked back");
        // …and it stays shut across a frame in which nothing was typed.
        let (still, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert_eq!(still, 0.0, "a quiet frame is not an edit");

        // The very next keystroke brings it back.
        pane.composer.push('a');
        let (back, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert!(back > 0.0, "and the next keystroke brings it back: {back}");

        // 🚨 The case that reached James: dismiss, clear, retype the identical string.
        let _ = palette_frame(&ctx, &mut pane, key(egui::Key::Escape, egui::Modifiers::NONE));
        let (dismissed, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert_eq!(dismissed, 0.0, "dismissed at `/ca`");
        pane.composer.clear();
        let _ = palette_frame(&ctx, &mut pane, Vec::new());
        pane.composer = "/ca".to_string();
        let (retyped, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert!(
            retyped > 0.0,
            "retyping a line that was once dismissed must not stay poisoned: {retyped}"
        );
    }

    /// 🚨 **Auto-execute, through real frames: what fires, and the two shapes that must not.**
    ///
    /// `/h` is `help`, which takes nothing and reads a table — one candidate, complete,
    /// recoverable, so it runs and the box empties. `/th` is `theme`, which still needs a
    /// value, so certainty about the *verb* is not certainty about the *command*. `/s` is
    /// `surface`, which is certain and complete and still waits, because a surface in the
    /// transcript is not something a second command takes back.
    ///
    /// ⚠️ **`/s` is the regression this test exists for.** It is the line the old rule fired
    /// on, and it is the line James would have met first.
    #[test]
    fn auto_execute_runs_what_it_can_take_back_and_waits_for_the_rest() {
        let ctx = egui::Context::default();

        let mut reading = palette_pane();
        reading.composer = "/h".to_string();
        let _ = palette_frame(&ctx, &mut reading, Vec::new());
        assert_eq!(reading.composer, "", "`/h` could only be `help`, so it ran and emptied the box");
        assert!(reading.receipt.is_some(), "…and left a receipt to say so");

        // ⚠️ The line **completes** — that is the other mechanism, and it is on for every verb
        // — and then sits there. What a human sees is the row saying `Enter runs`.
        let mut irreversible = palette_pane();
        irreversible.composer = "/s".to_string();
        for _ in 0..3 {
            let _ = palette_frame(&ctx, &mut irreversible, Vec::new());
        }
        assert_eq!(irreversible.composer, "/surface", "completed, and then stopped");
        assert!(
            irreversible.transcript.elements().is_empty(),
            "nothing was put in the transcript by a keystroke"
        );
        assert_eq!(
            row_for(&mut irreversible, &ctx),
            PALETTE_RUNS,
            "the ask is the marker that already existed, not a second phrasing of it"
        );

        // The case that must not fire because the command is not finished. ⚠️ The line
        // completes to `/theme ` first — completion carrying it one word further is what makes
        // this interesting rather than trivial.
        let mut waiting = palette_pane();
        waiting.composer = "/th".to_string();
        for _ in 0..3 {
            let _ = palette_frame(&ctx, &mut waiting, Vec::new());
        }
        assert_eq!(waiting.composer, "/theme ", "`theme` still needs a value — it must sit there");
        assert!(waiting.receipt.is_none(), "and nothing was run to leave a receipt");

        // …and the escape hatch really is one: the same line, with the switch off, behaves
        // exactly as the console did before this became the default.
        let mut off = palette_pane();
        off.autorun = autorun_enabled(Some("0"));
        off.composer = "/h".to_string();
        for _ in 0..3 {
            let _ = palette_frame(&ctx, &mut off, Vec::new());
        }
        assert_eq!(off.composer, "/help", "completed, never run");
        assert!(off.receipt.is_none());
    }

    /// 🚨 CONTRACT: **a command does not run on the frame its last character landed.**
    ///
    /// The completed line has to be drawn at least once before it disappears, and a keystroke
    /// arriving while a fire is pending has to *cancel* it rather than race it. So the fire
    /// waits for a frame in which nothing was typed.
    ///
    /// ⚠️ Driven through egui's own `TextEdit`, because "this frame had a keystroke in it" is
    /// only true of a real event — a composer assigned between frames is synced by
    /// `notice_edit` before anything looks at it, and is deliberately treated as settled
    /// already (nothing was typed, so there is no hand to wait for).
    #[test]
    fn a_command_waits_for_one_frame_in_which_nothing_was_typed() {
        let ctx = egui::Context::default();
        let mut pane = typing_pane(&ctx, "/");
        let _ = palette_frame(&ctx, &mut pane, typed("h"));
        assert_eq!(
            pane.composer, "/help",
            "the line completed on the keystroke's own frame, as it always has"
        );
        assert!(pane.receipt.is_none(), "…and did NOT run on it");

        let _ = palette_frame(&ctx, &mut pane, Vec::new());
        assert_eq!(pane.composer, "", "the first quiet frame is the one that runs it");
        assert!(pane.receipt.is_some());

        // 🚨 The point of the wait: a keystroke arriving inside that window re-asks the
        // question instead of losing a race with it.
        let mut interrupted = typing_pane(&ctx, "/");
        let _ = palette_frame(&ctx, &mut interrupted, typed("h"));
        assert_eq!(interrupted.composer, "/help");
        let _ = palette_frame(&ctx, &mut interrupted, typed("x"));
        // ⚠️ **`/helpx`, and the two halves of that are separate contracts.** The character
        // lands at the *end* because the completion's caret request is drained at the end of
        // the frame that made it — `put_caret_at_end`, pinned on its own by
        // `a_character_typed_after_a_completion_lands_at_the_end_of_it`. What this test owns is
        // the line below: whatever the interrupted line turned out to say, no command ran.
        assert_eq!(interrupted.composer, "/helpx", "the character landed after the completion");
        assert!(interrupted.receipt.is_none(), "and nothing ran on a line nobody meant");

        // 🚨 …and it does not merely postpone: `/helpx` is not a command, so quiet frames pass
        // and nothing happens. The keystroke cancelled the fire outright.
        for _ in 0..3 {
            let _ = palette_frame(&ctx, &mut interrupted, Vec::new());
        }
        assert_eq!(interrupted.composer, "/helpx");
        assert!(interrupted.receipt.is_none());
    }

    /// 🚨 CONTRACT: **the character after a completion lands at the end of it.**
    ///
    /// Measured on a running build: typing `/`, then `h`, completed the line to `/help` — and
    /// the next character produced **`/hxelp`**. The completion rewrote the text on its frame
    /// and asked for the caret; the request was drained by the *next* frame's `composer_box`,
    /// which runs *before* the completion does, so it could only ever honour the previous
    /// frame's ask — and by the time it ran, that frame's keystroke had already been placed at
    /// the stale index, after `/h`. The window was one frame wide, which is ~16 ms at 60 fps
    /// and well inside a fast burst.
    ///
    /// The fix is an ordering, not a second flag: `want_caret` is drained at the **end** of
    /// `composer`, after `palette_complete` and `palette_autorun`, so the caret moves on the
    /// same frame the line was rewritten. This test is the property, driven through egui's own
    /// `TextEdit` — the caret is egui's index, so nothing short of a real widget can show it.
    ///
    /// ⚠️ **Autorun is off here on purpose.** `/help` is recoverable, so with the runner on
    /// the line would fire on the first settled frame and there would be no line left to type
    /// into. The caret is completion's business; when the command runs is `a_command_waits_
    /// for_one_frame_in_which_nothing_was_typed`'s.
    #[test]
    fn a_character_typed_after_a_completion_lands_at_the_end_of_it() {
        let ctx = egui::Context::default();
        let mut pane = typing_pane(&ctx, "/");
        pane.autorun = autorun_enabled(Some("0"));

        let _ = palette_frame(&ctx, &mut pane, typed("h"));
        assert_eq!(pane.composer, "/help", "the line completed on the keystroke's own frame");

        let _ = palette_frame(&ctx, &mut pane, typed("x"));
        assert_eq!(pane.composer, "/helpx", "…and the very next character appended to it");

        // 🚨 The window was one frame wide, so a second character proves nothing on its own —
        // it is the *first* one after the rewrite that used to land mid-word. Typing on is
        // still worth pinning: a fix that moved the caret once and then lost it would pass the
        // assertion above and fail here.
        let _ = palette_frame(&ctx, &mut pane, typed("y"));
        assert_eq!(pane.composer, "/helpxy");
    }

    /// The same property for the *other* two sites that rewrite the line wholesale — a history
    /// recall and a Tab accept — which run **before** the box rather than after it.
    ///
    /// 🚨 **This is why the drain is at the end of the frame rather than moved a few lines up.**
    /// Both orders have to work through one flag, and a fix that only rearranged the completion
    /// case would leave these two to a second mechanism that could drift away from it.
    #[test]
    fn a_recalled_line_and_a_tab_accept_leave_the_caret_at_the_end_too() {
        let ctx = egui::Context::default();

        // A recall: the arrows replace the line, and typing continues from its end.
        let mut recalled = typing_pane(&ctx, "");
        recalled.autorun = autorun_enabled(Some("0"));
        recalled.history.push_front("/theme dark".to_string());
        let _ = palette_frame(&ctx, &mut recalled, key(egui::Key::ArrowUp, egui::Modifiers::NONE));
        assert_eq!(recalled.composer, "/theme dark", "the walk put the line in the box");
        let _ = palette_frame(&ctx, &mut recalled, typed("!"));
        assert_eq!(recalled.composer, "/theme dark!", "and the caret came with it");

        // A Tab accept: the same, one mechanism over. ⚠️ The line is `/theme ` rather than
        // `/th`, because `/th` completes itself on the frame that puts it in the box and there
        // would be no Tab left to test — a value ring with six options in it is the one place
        // Tab still has work to do.
        let mut tabbed = typing_pane(&ctx, "/theme ");
        tabbed.autorun = autorun_enabled(Some("0"));
        let _ = palette_frame(&ctx, &mut tabbed, key(egui::Key::Tab, egui::Modifiers::NONE));
        assert_eq!(tabbed.composer, "/theme organon", "Tab took the highlighted candidate");
        let _ = palette_frame(&ctx, &mut tabbed, typed("!"));
        assert_eq!(tabbed.composer, "/theme organon!", "and typing carried on from its end");
    }

    /// The rule the environment variable states, without a test writing to the process
    /// environment. ⚠️ **`=1` must still mean ON** — it is in shell profiles, and a variable
    /// that quietly comes to mean the opposite of what somebody wrote is worse than no
    /// variable at all.
    #[test]
    fn the_autorun_switch_defaults_on_and_is_turned_off_by_zero() {
        assert!(autorun_enabled(None), "unset is the new default, and the new default is on");
        assert!(!autorun_enabled(Some("0")), "`=0` is the escape hatch");
        assert!(autorun_enabled(Some("1")), "`=1` means exactly what it always meant");
        assert!(autorun_enabled(Some("")), "anything else is not the escape hatch");
    }

    /// 🚨 CONTRACT: **a lone candidate completes itself, is never shown, and never runs.**
    ///
    /// James, 2026-08-14: *"Do not show me the single choice like you currently do. Simply
    /// complete the completion because it's the only option."* The three assertions below are
    /// the three halves of that: the line moves, the panel does not draw a one-item list, and
    /// with autorun off — the default — the transcript stays empty.
    ///
    /// ⚠️ **"Never shown" is about the redundant *word*, not about the row.** `/theme dark` is
    /// a whole command, so the row now says so — see [`PALETTE_RUNS`]. What must not appear is
    /// a one-item list offering `dark` to a line that already ends in it, and the assertions
    /// read the row's text to pin exactly that.
    #[test]
    fn a_lone_candidate_completes_itself_and_is_never_shown() {
        let ctx = egui::Context::default();
        // ⚠️ **The runner is switched off for the whole of this test, deliberately.** `theme` is
        // recoverable, so in the product `/theme d` completes *and then runs* — which is the
        // right behaviour and the wrong instrument for measuring completion. Everything below
        // is about the line in the box; `auto_execute_runs_what_it_can_take_back_and_waits_
        // for_the_rest` is about what happens to it afterwards.
        let off = autorun_enabled(Some("0"));
        let mut pane = palette_pane();
        pane.autorun = off;
        pane.composer = "/theme d".to_string();
        let _ = palette_frame(&ctx, &mut pane, Vec::new());
        assert_eq!(pane.composer, "/theme dark", "the only option it could have meant");
        let settled = pane.palette().expect("a settled command still has a palette");
        assert_eq!(
            compact_line(&settled, 0, 200),
            "Enter runs | [dark]",
            "the palette still reports the redundant word; the renderer is what drops it"
        );
        assert_eq!(
            row_for(&mut pane, &ctx),
            PALETTE_RUNS,
            "…so what a human sees is the one thing left that is true of the line"
        );
        assert!(pane.transcript.elements().is_empty(), "completing is not running");
        assert!(pane.receipt.is_none());

        // 🚨 The case a count alone gets wrong: `/camera` is a whole verb AND the prefix of
        // another, so it is two candidates and must sit there being a list.
        let mut two = palette_pane();
        two.autorun = off;
        two.composer = "/camera".to_string();
        let (shown, _) = palette_frame(&ctx, &mut two, Vec::new());
        assert_eq!(two.composer, "/camera", "two candidates settle nothing");
        assert!(shown > 0.0, "…so the row is drawn: {shown}");

        // ⚠️ The case James hit from the other end. A verb whose arguments are its whole
        // point offers none of them until the line reaches its value slot, and only the
        // trailing space in a verb's completion gets it there — so completing is what opens
        // the ring at all. `/theme` stands in for `/portal` here; the shape is identical.
        let mut opened = palette_pane();
        opened.autorun = off;
        opened.composer = "/theme".to_string();
        let _ = palette_frame(&ctx, &mut opened, Vec::new());
        assert_eq!(opened.composer, "/theme ", "one step, and the value ring is open");
        let palette = opened.palette().expect("the value ring");
        assert_eq!(
            palette.candidates.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["organon", "light", "dark", "chocolate", "edit", "adjust"],
            "which is what a human sees after typing a verb and nothing else"
        );

        // Escape suppresses it, for free: no panel, no completion, nothing rewritten.
        let mut shut = palette_pane();
        shut.autorun = off;
        shut.composer = "/theme d".to_string();
        let _ = palette_frame(&ctx, &mut shut, Vec::new());
        shut.composer = "/theme da".to_string();
        let _ = palette_frame(&ctx, &mut shut, key(egui::Key::Escape, egui::Modifiers::NONE));
        let _ = palette_frame(&ctx, &mut shut, Vec::new());
        assert_eq!(shut.composer, "/theme da", "a dismissed panel rewrites nothing");
    }

    /// One frame's worth of a key held down, and one of a character typed.
    fn backspace() -> Vec<egui::Event> {
        key(egui::Key::Backspace, egui::Modifiers::NONE)
    }

    fn typed(text: &str) -> Vec<egui::Event> {
        vec![egui::Event::Text(text.to_string())]
    }

    /// A pane whose composer already holds `line`, with the caret at its end, ready to be
    /// typed into or deleted from through real frames.
    fn typing_pane(ctx: &egui::Context, line: &str) -> ConversationPane {
        let mut pane = palette_pane();
        pane.composer = line.to_string();
        // Without this egui's `TextEdit` has no cursor to delete from, and a Backspace event
        // reaches a widget that does not know where it is.
        pane.want_caret = true;
        let _ = palette_frame(ctx, &mut pane, Vec::new());
        pane
    }

    /// 🚨 CONTRACT: **a hand can always delete its way back out of a command line.**
    ///
    /// James, on a running build, 2026-08-14: *"once I have typed slash surface, I am no longer
    /// able to backspace out of it."* `/surfac` leaves `surface` as its only candidate, whose
    /// completion is `/surface` — so every backspace was undone on the frame it happened, and
    /// select-all-and-retype was the only way to correct a mistyped command. It trapped every
    /// verb with a unique prefix, which is nearly all of them.
    ///
    /// Driven one keystroke at a time through **real frames and egui's own `TextEdit`**,
    /// because that is the only place the deletion actually happens: a composer assigned
    /// between frames is synced by `notice_edit` before anything looks at it, so a test that
    /// popped a character itself would be testing nothing.
    #[test]
    fn backspace_walks_out_of_a_completed_command_one_character_at_a_time() {
        let ctx = egui::Context::default();
        let mut pane = typing_pane(&ctx, "/surface");
        assert_eq!(pane.composer, "/surface", "a settled line, left alone");

        let mut seen = vec![pane.composer.clone()];
        for _ in 0..8 {
            let _ = palette_frame(&ctx, &mut pane, backspace());
            seen.push(pane.composer.clone());
        }
        assert_eq!(
            seen,
            [
                "/surface", "/surfac", "/surfa", "/surf", "/sur", "/su", "/s", "/", "",
            ],
            "every backspace has to land, all the way back to `/` and out of the line"
        );
    }

    /// 🚨 CONTRACT: **a deletion is not undone one frame later either**, which is the same
    /// defect at 60 Hz and would present as a flicker rather than as a line that will not
    /// shorten.
    ///
    /// The frame after a backspace is a frame in which nothing changed at all, so a rule that
    /// merely refused *shrinking* frames would complete on that next one. The latch is what
    /// makes the refusal outlive the keystroke — and what lets go of it is an insertion, not
    /// the passage of time.
    #[test]
    fn a_deletion_is_not_undone_on_the_quiet_frames_after_it() {
        let ctx = egui::Context::default();
        let mut pane = typing_pane(&ctx, "/surface");
        let _ = palette_frame(&ctx, &mut pane, backspace());
        assert_eq!(pane.composer, "/surfac", "the deletion landed");
        for frame in 0..8 {
            let _ = palette_frame(&ctx, &mut pane, Vec::new());
            assert_eq!(pane.composer, "/surfac", "put back on quiet frame {frame}");
        }

        // …and typing is what starts it up again, on the very next character.
        let _ = palette_frame(&ctx, &mut pane, backspace());
        assert_eq!(pane.composer, "/surfa");
        let _ = palette_frame(&ctx, &mut pane, typed("c"));
        assert_eq!(
            pane.composer, "/surface",
            "an inserted character completes exactly as it did before the fix"
        );
    }

    /// 🚨 CONTRACT: **deleting never runs anything**, which is the same rule with a worse
    /// consequence attached.
    ///
    /// With `autorun` on, backspacing `/surface` to `/surfac` leaves one candidate that
    /// *completes* — so the keystroke trying to erase the command would have executed it.
    #[test]
    fn a_backspace_never_runs_the_command_it_is_erasing() {
        let ctx = egui::Context::default();
        let mut pane = typing_pane(&ctx, "/surface");
        pane.autorun = true;
        let _ = palette_frame(&ctx, &mut pane, backspace());
        assert_eq!(pane.composer, "/surfac", "the line shortened");
        assert!(pane.transcript.elements().is_empty(), "and nothing ran");
        assert!(pane.receipt.is_none());
    }

    /// The rule itself, stated as the pure function both halves read — including the three
    /// cases it deliberately gets *approximately* right. See [`completion_held`].
    #[test]
    fn completion_is_taken_on_an_insertion_and_never_on_a_deletion() {
        // The two that matter, from either starting state.
        assert!(completion_held("/surface", "/surfac", false), "a deletion holds it off");
        assert!(!completion_held("/surfac", "/surface", true), "an insertion lets it go");
        // Nothing changed: whatever was true stays true. This is the flicker guard — the frame
        // after a backspace is exactly this case.
        assert!(completion_held("/surfac", "/surfac", true), "a quiet frame is not an insertion");
        assert!(!completion_held("/surfac", "/surfac", false));

        // ⚠️ The edges, each classified by what it did to the length rather than by what it
        // meant. All three are stated in the doc; none of them can get stuck, because the next
        // inserted character releases the latch.
        assert!(
            completion_held("/theme chocolate", "/th", false),
            "a paste that shortens the line reads as a deletion, and does not complete"
        );
        assert!(
            completion_held("/surface", "/", false),
            "select-all then type one character is shorter, so it reads as a deletion too"
        );
        assert!(
            !completion_held("/theme edit", "/theme dark", false),
            "a same-length replacement changes nothing about the latch"
        );
    }

    /// 🚨 CONTRACT: **the cascade is bounded.**
    ///
    /// Chaining is wanted — completing a verb opens its value ring, and a ring with one
    /// option in it is an answer too. What it must never be able to do is spin, so the loop
    /// counts rather than testing a condition it cannot prove. `/th` is the deepest chain this
    /// fixture can build: `theme` is unique on that prefix, and it stops at four options.
    ///
    /// ✏️ **It was `/t` until `/trace` joined the table** — a plain consequence of a second verb
    /// starting with the same letter, and worth leaving visible rather than renaming the verb
    /// around: a one-letter prefix settling is a property of the *vocabulary*, not of the
    /// cascade, and this test is about the cascade.
    #[test]
    fn the_completion_cascade_is_bounded_and_terminates() {
        assert_eq!(PALETTE_COMPLETE_STEPS, 4, "past the deepest ring the table has");
        let ctx = egui::Context::default();
        let mut pane = palette_pane();
        pane.composer = "/th".to_string();
        let _ = palette_frame(&ctx, &mut pane, Vec::new());
        assert_eq!(pane.composer, "/theme ", "verb, then a ring it cannot settle");

        // 🚨 The fixed point. `/surface` is its own sole completion, so a rule that counted
        // candidates and stopped there would rewrite the line to itself until the cap ran
        // out — and would do it on every frame, for ever.
        let mut settled = palette_pane();
        settled.composer = "/surface".to_string();
        for _ in 0..3 {
            let _ = palette_frame(&ctx, &mut settled, Vec::new());
        }
        assert_eq!(settled.composer, "/surface", "a completed line is left alone");
    }

    /// 🚨 CONTRACT: **Up means three different things and the rule that picks one is a pure
    /// function**, because the wrong pick costs a message somebody was writing.
    #[test]
    fn the_arrows_belong_to_whoever_has_earned_them() {
        use ArrowOwner::*;
        // A walk in progress keeps them, even though a recalled command line opens a panel —
        // without this the second Up would move a highlight and the walk would stop one step
        // in.
        assert_eq!(arrow_owner(true, true, false), History);
        assert_eq!(arrow_owner(true, false, true), History);
        // An open panel is next, which is the rule the palette shipped with.
        assert_eq!(arrow_owner(false, true, false), Panel);
        // An empty box has no caret motion to perform, so Up can only mean "what did I type".
        assert_eq!(arrow_owner(false, false, true), History);
        // 🚨 And otherwise the text box keeps them. Prose, a half-written paragraph, and a
        // command line whose panel was dismissed with Escape are all this case: the caret is
        // what Up is for, and a history that stole the key would replace what was in the box.
        assert_eq!(arrow_owner(false, false, false), TextBox);
    }

    /// The history holds commands, most recent first, and walking it does not wrap.
    #[test]
    fn the_history_remembers_commands_and_not_prose() {
        let mut pane = palette_pane();
        for line in ["/surface", "/surface", "hello there", "//surface", "/theme nonesuch"] {
            pane.composer = line.to_string();
            pane.submit(&Theme::organon(), "organon");
        }
        assert_eq!(
            pane.history.iter().cloned().collect::<Vec<_>>(),
            ["/theme nonesuch", "/surface"],
            "a refusal is kept — it is the line you most want back — and prose is not, and \
             `//` is an escape meaning the line was a message"
        );
        assert_eq!(pane.history.len(), 2, "…and running one command twice is one entry");

        // Walking. Up goes back, Down comes forward, and the end is an end.
        pane.composer.clear();
        pane.history_step(true);
        assert_eq!(pane.composer, "/theme nonesuch");
        pane.history_step(true);
        assert_eq!(pane.composer, "/surface");
        pane.history_step(true);
        assert_eq!(pane.composer, "/surface", "the oldest is the oldest; it does not wrap");
        pane.history_step(false);
        assert_eq!(pane.composer, "/theme nonesuch");
        pane.history_step(false);
        assert_eq!(pane.composer, "", "forward past the newest is the empty box you started in");

        // 🚨 Editing the recalled line ends the walk, and the next Up starts a new one from
        // the top rather than resuming from wherever the old one had reached.
        pane.history_step(true);
        pane.history_step(true);
        assert_eq!(pane.composer, "/surface");
        pane.composer.push('x');
        assert!(!pane.walking(), "one keystroke and the walk is over");
        pane.history_step(true);
        assert_eq!(pane.composer, "/theme nonesuch", "a new walk starts at the newest");
    }

    /// The keys, through real frames: Up recalls into an empty box and leaves prose alone.
    #[test]
    fn up_recalls_a_command_and_never_touches_a_message_being_written() {
        let ctx = egui::Context::default();
        let mut pane = palette_pane();
        pane.composer = "/surface".to_string();
        pane.submit(&Theme::organon(), "organon");
        let _ = palette_frame(&ctx, &mut pane, Vec::new());
        assert_eq!(pane.composer, "", "the send emptied the box");

        let _ = palette_frame(&ctx, &mut pane, key(egui::Key::ArrowUp, egui::Modifiers::NONE));
        assert_eq!(pane.composer, "/surface", "Up in an empty box is the last command");

        // ⚠️ A message being written keeps its own arrows — the caret is what Up means there,
        // and this is the box a human talks to an agent in.
        let mut writing = palette_pane();
        writing.composer = "hello there".to_string();
        writing.history.push_front("/surface".to_string());
        let _ = palette_frame(&ctx, &mut writing, key(egui::Key::ArrowUp, egui::Modifiers::NONE));
        assert_eq!(writing.composer, "hello there", "prose is not a history walk");

        // …and while a panel is open the arrows still move its highlight.
        let mut choosing = palette_pane();
        choosing.composer = "/theme ".to_string();
        choosing.history.push_front("/surface".to_string());
        let _ = palette_frame(&ctx, &mut choosing, Vec::new());
        let _ =
            palette_frame(&ctx, &mut choosing, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
        assert_eq!(choosing.composer, "/theme ", "the line did not move");
        assert_eq!(choosing.palette_selected, 1, "the highlight did");
    }

    /// A command's answer lands where the command was typed, which is the whole of the
    /// addendum that tier carried: the log is drawn at the head of the scrollback, so in any
    /// conversation longer than a screen a receipt there is invisible.
    ///
    /// ✏️ **…and a *successful* one is now drawn only while the pane is tracing.** It ended
    /// `assert!(band > 0.0)` unconditionally; James asked for `ok /theme dark —
    /// {"accepted":"theme dark"}` over the composer to stop appearing while the console
    /// repaints in front of him. The receipt is still *made* and still ages — only the drawing
    /// moved — which is what lets `/trace on` show one that is already in hand.
    #[test]
    fn a_successful_receipt_is_held_and_drawn_only_while_tracing() {
        let ctx = egui::Context::default();
        let mut pane = palette_pane();
        pane.composer = "/surface".to_string();
        let _ = palette_frame(&ctx, &mut pane, Vec::new());
        let _ = palette_frame(&ctx, &mut pane, enter(egui::Modifiers::NONE));
        let answer = pane.receipt.as_ref().expect("a receipt");
        assert!(answer.receipt.ok);
        assert_eq!(answer.receipt.text, "/surface");
        assert_eq!(answer.answered, "", "a success answers the box it emptied");

        let (quiet, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert_eq!(quiet, 0.0, "a quiet console confirmed a command that confirmed itself");
        assert!(pane.receipt.is_some(), "the receipt was destroyed rather than merely not drawn");

        pane.tracing = true;
        let (loud, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert!(loud > 0.0, "tracing did not bring the held receipt back: {loud}");
    }

    /// A dispatch that accepts everything, so the **quiet** half of the receipt rule is
    /// reachable in a fixture whose production `local` (`mcp::NoDispatch`) refuses everything.
    struct Accepts;
    impl crate::mcp::ToolDispatch for Accepts {
        fn call(
            &mut self,
            _command: &str,
            _args: serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            Ok(serde_json::Value::Null)
        }
    }

    /// 🚨 **The whole of Tier 1's rule, at the one place it is decided.**
    ///
    /// `ok /viewport center agent — {"accepted":"viewport center agent"}` is the line James
    /// pointed at, and what makes it droppable is not that it is routine — it is that the thing
    /// it describes announced itself, because the layout moved. A refusal announces itself
    /// nowhere, so it is the half that must never be gated.
    ///
    /// ⚠️ **Both halves in one test on purpose**: they are the two arms of one `if`, and a test
    /// that only pinned the quiet one would pass with the loud arm deleted.
    #[test]
    fn a_console_command_that_worked_is_narration_and_one_that_did_not_is_news() {
        let args = || serde_json::json!({ "name": "dark" });
        let mut accepted = palette_pane();
        accepted.local = Box::new(Accepts);
        let receipt = accepted.run_command(
            Lane::Console,
            "console.theme",
            "/theme dark",
            args(),
            &Theme::organon(),
            "organon",
        );
        assert!(receipt.ok, "{}", receipt.text);
        let last = accepted.log.last().expect("the receipt reached the log either way");
        assert!(last.text.contains("/theme dark"), "{}", last.text);
        assert!(!last.always, "an acceptance is narration — the log holds it and nothing else");
        // 🚨 **Kept, and out of the conversation.** The two properties that used to be one
        // `seen(mode)` question are now separate facts, which is the point: the line is in the
        // log whatever the mode, and it is not among what the scrollback draws.
        assert_eq!(accepted.log.iter().filter(|r| r.text.contains("/theme dark")).count(), 1);
        assert!(
            !accepted.log.exceptions().any(|r| r.text.contains("/theme dark")),
            "an acceptance reached the conversation"
        );
        assert!(!accepted.log.attention(), "an acceptance lit the status line");

        // The fixture's own `local` is `NoDispatch`, which refuses everything — so this is the
        // refusal path with nothing rigged.
        let mut refused = palette_pane();
        let receipt = refused.run_command(
            Lane::Console,
            "console.theme",
            "/theme dark",
            args(),
            &Theme::organon(),
            "organon",
        );
        assert!(!receipt.ok, "the fixture's dispatch refuses: {}", receipt.text);
        let last = refused.log.last().expect("a refusal reaches the log");
        assert!(last.always, "a refusal was hidden behind a mode nobody had turned on");
        // ⚠️ Matched by the remark's own text rather than by the typed line: a refusal carries
        // the dispatch's sentence, which does not echo what was typed — that is `RECEIPT_OK`'s
        // arm. See [`registry::receipt`].
        let refusal = last.text.clone();
        assert!(
            refused.log.exceptions().any(|r| r.text == refusal),
            "a refusal did not reach the conversation: {refusal}"
        );
        assert!(refused.log.attention(), "a refusal left the status line dark");
    }

    /// `/trace on` and `/trace off`, through the same lane a typed line takes.
    ///
    /// ⚠️ **Switching on is echoed and switching off is not**, and that is not two rules: the
    /// acknowledgement goes into the log, which is the thing `on` has just put on screen and
    /// `off` has just taken off it.
    ///
    /// 🚨 **And neither word reaches the conversation**, which is what the verb now means. The
    /// old spelling of this test asked `Remark::seen` — a question about a mode that widened the
    /// scrollback. There is no such mode.
    #[test]
    fn trace_is_off_until_it_is_asked_for_and_one_word_puts_it_back() {
        let mut pane = palette_pane();
        assert!(!pane.tracing(), "a tab opens quiet");
        let flow_before = pane.log.exceptions().count();
        let say = |pane: &mut ConversationPane, word: &str| {
            pane.run_command(
                Lane::View,
                registry::VERB_TRACE,
                &format!("/trace {word}"),
                serde_json::json!({ registry::TRACE_ARG: word }),
                &Theme::organon(),
                "organon",
            )
        };
        assert!(say(&mut pane, "on").ok);
        assert!(pane.tracing(), "`/trace on` did not open the log");
        let last = pane.log.last().expect("it says so");
        assert!(last.text.contains("status log open"), "{}", last.text);

        assert!(say(&mut pane, "off").ok);
        assert!(!pane.tracing(), "`/trace off` did not close it");
        assert!(
            pane.log.last().expect("it is still in the log").text.contains("closed"),
            "the log did not record its own closing"
        );
        assert_eq!(
            pane.log.exceptions().count(),
            flow_before,
            "opening and closing the status log put something in the conversation",
        );
    }

    /// 🚨 CONTRACT: **opening the log is what clears the status line, in both directions.**
    ///
    /// The other half of this lives in [`crate::status_log`], where the arithmetic is; what is
    /// pinned here is that the *verb* is wired to it. A `/trace on` that opened the panel and
    /// left the dot lit would be a badge that never clears, which is a badge nobody reads.
    ///
    /// ⚠️ **Mutation-checked**: drop the `self.log.acknowledge()` from
    /// [`ConversationPane::set_tracing`] and the middle assertion fails with *"opening the log
    /// did not clear the indicator"*.
    #[test]
    fn opening_the_log_clears_the_indicator_and_a_later_exception_lights_it_again() {
        let mut pane = palette_pane();
        pane.note("could not send interrupt: broken pipe".to_string());
        assert!(pane.status_log().attention(), "an exception left the indicator dark");
        pane.toggle_log();
        assert!(pane.tracing(), "the indicator's click did not open the log");
        assert!(!pane.status_log().attention(), "opening the log did not clear the indicator");
        pane.trace("ok /theme dark".to_string());
        assert!(!pane.status_log().attention(), "machinery lit an acknowledged log");
        pane.note("the agent process ended".to_string());
        assert!(pane.status_log().attention(), "a new exception did not light the indicator");
    }

    /// 🚨 CONTRACT: **the status line is derived from the log and nothing else, and it is the
    /// ONLY surface that reports it.**
    ///
    /// The summary is [`crate::status_log::StatusLog::summary`] read out; there is no flag some
    /// caller maintained, so there is no second opinion to drift. And #129 removed the band's
    /// own indicator — two doors to one surface is duplication, and the band is the thing James
    /// asked to say *less* — so this also pins that the band knows nothing about the log at all.
    ///
    /// ⚠️ **Mutation-checked**: give [`StripContent`] a log field again and this stops compiling,
    /// which is the strongest form the check can take.
    #[test]
    fn the_status_line_reads_the_summary_off_the_log_and_the_band_knows_nothing_of_it() {
        let theme = Theme::organon();
        let mut pane = palette_pane();
        assert_eq!(
            pane.status_log().summary().health,
            Health::Ok,
            "a fresh tab is not in trouble",
        );

        pane.trace("stderr: warming up".to_string());
        let quiet = pane.status_log().summary();
        assert_eq!(quiet.health, Health::Ok, "machinery lit the status line");
        assert_eq!(health_color(quiet.health, &theme), theme.ok);
        assert_eq!(quiet.text, "all clear · 1 line");

        pane.note("could not send: broken pipe".to_string());
        let loud = pane.status_log().summary();
        assert_eq!(loud.health, Health::Attention, "an exception left the status line green");
        assert_eq!(health_color(loud.health, &theme), theme.bad);
        assert_eq!(loud.text, "could not send: broken pipe");
        assert_eq!(loud.lines, 2, "the summary knows how much is behind it");

        pane.toggle_log();
        let read = pane.status_log().summary();
        assert_eq!(read.health, Health::Warning, "a session that broke is not 'all clear'");
        assert_eq!(health_color(read.health, &theme), theme.asking);

        // 🚨 The three states are three DIFFERENT colours, all of them the theme's own. A palette
        // that answered the same colour twice would make one of the states unreadable.
        assert_ne!(theme.ok, theme.asking);
        assert_ne!(theme.asking, theme.bad);
        assert_ne!(theme.ok, theme.bad);

        // …and the band, meanwhile, has nothing to say about any of it.
        let band = strip_content(None, live(0, 0), pane.mapper.facts(), Some("abc"));
        assert!(
            !format!("{band:?}").contains("log"),
            "the band grew a second door to the status log: {band:?}",
        );
    }

    /// 🚨 **A refusal keeps the band whatever the mode.** The quiet default costs a
    /// confirmation, never a reason — and this is the one of the two that nothing else on
    /// screen would say.
    ///
    /// ⚠️ **Driven through the real command path rather than by planting a `PanelReceipt`.** The
    /// gate reads `receipt.ok`, and a test that set that field by hand would keep passing if the
    /// production path stopped producing a refusal at all.
    #[test]
    fn a_refusal_holds_the_band_even_when_the_console_is_quiet() {
        let ctx = egui::Context::default();
        let mut pane = palette_pane();
        assert!(!pane.tracing, "the fixture opens quiet, like a real tab");
        pane.composer = "/camera yaw 999".to_string();
        let _ = palette_frame(&ctx, &mut pane, Vec::new());
        let _ = palette_frame(&ctx, &mut pane, enter(egui::Modifiers::NONE));
        let answer = pane.receipt.as_ref().expect("a receipt");
        assert!(!answer.receipt.ok, "999 is outside the yaw ring: {}", answer.receipt.text);
        let (band, _) = palette_frame(&ctx, &mut pane, Vec::new());
        assert!(band > 0.0, "a quiet console swallowed a refusal: {band}");
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
