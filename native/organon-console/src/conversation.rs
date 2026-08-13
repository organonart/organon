//! Agent events → a renderable transcript (Console Spike §5.9, the conversation view).
//!
//! The console forks into two front-ends over one renderer. The **terminal host** runs any
//! program and paints its character grid. The **conversation view** does the opposite: it
//! consumes an agent's *structured* event stream and renders it natively, because the
//! character grid was only ever a lossy encoding of something that had structure before it
//! was flattened (execution plan §5.9, and §5.9.1 for why Claude Code is the first
//! integration).
//!
//! This module is the middle of three pieces. A **decoder** turns NDJSON lines into typed
//! events; an **integrator** spawns the process and draws the result; and in between sits
//! this: a state machine that folds a stream of events into an ordered list of
//! **renderable elements**. It is not a widget and not a layout — there is no egui here,
//! no rect, no font, no pixel, no clock and no I/O — so it is `cargo test -p organon-console`
//! in seconds, on any machine, with no GPU and no window. The same bar `scroll_anchor` and
//! `block_anchor` are held to, for the same reason.
//!
//! # It defines its own input type, deliberately
//!
//! [`AgentEvent`] is **not** the decoder's type and is not meant to be. Two modules cannot
//! own one type, and a transcript that spoke the wire format fluently would have to change
//! shape every time the wire did — on a CLI whose event set has already grown
//! `rate_limit_event` and `system/post_turn_summary` without telling anyone. So the
//! integrator writes a short mapping from the decoder's events onto these nine, and the
//! seam is where harness-specific knowledge stops. A second harness (Pi, §5.9.1) maps onto
//! the same nine or the model is wrong.
//!
//! # The six behaviours that make this non-trivial
//!
//! **1. "A tool is running" has no event.** Claude Code emits the model's *emission* of a
//! tool call and, later, the result. There is no start-of-execution signal to listen for,
//! so running-ness cannot be a flag somebody sets — it is **derived from an unresolved
//! id**, and it stops being true when [`AgentEvent::ToolResult`] arrives for that id.
//! Read-only tools are dispatched concurrently, so several ids are routinely unresolved at
//! once; [`Transcript::running_tools`] reports all of them, in call order.
//!
//! **2. Streaming and complete content both arrive for the same text.** Token deltas come
//! first, then the authoritative complete message. The complete message is applied **by
//! replacement, not by appending** ([`AgentEvent::AssistantMessage`]), and that one choice
//! is what makes the deltas *pure presentation*: any accumulation error — a dropped
//! fragment, a duplicated one, a mis-ordered one — is corrected within the turn that
//! produced it, with no reconciliation logic and no way for the two paths to disagree at
//! rest. A delta arriving *after* the complete message is noise by definition and is
//! counted rather than applied ([`Stats::late_deltas`]).
//!
//! **3. "Finished" does not mean "stop accepting events."** A run is bracketed by a
//! session start and a final result, but trailing events genuinely arrive after that
//! result — a tool dispatched before the end can be resolved after it. So
//! [`AgentEvent::RunFinished`] closes nothing: it records an outcome on the current
//! [`Turn`], and any later event still lands in that turn, which flags itself
//! ([`Turn::trailing`]) rather than dropping the event or panicking. It follows that a run
//! ending does **not** resolve its outstanding tool cards: there is no evidence they
//! finished, and inventing one would be the same lie in the other direction.
//!
//! **4. Arguments arrive as JSON fragments.** A tool call's input streams in as partial
//! text before the complete block lands. [`Arguments`] is therefore *text plus a
//! completeness bit* — never parsed, never presented as structured data while
//! [`Arguments::complete`] is false. This module parses no JSON at all; a view that wants
//! typed arguments parses [`Arguments::text`] itself, and only once it is complete.
//!
//! **5. Subagent output never streams.** Claude Code does not forward token deltas from
//! subagents, so on a coordinator session a large fraction of the visible text arrives as
//! one complete burst. A message that never received a single delta is therefore **normal,
//! not an error**, and must be indistinguishable from one that streamed. That is why
//! [`AssistantBlock`] carries no "was streamed" flag: the property is enforced by there
//! being nothing to differ.
//!
//! **6. A subagent is not a turn — it is something a tool call is doing.** Claude Code
//! scopes a subagent's lines with `parent_tool_use_id`, naming the `Task` call that spawned
//! it. Folded as ordinary events they become assistant turns belonging to nobody, which is
//! why they were dropped outright at first (§5.9.3 rule 5). They are folded instead onto
//! **the tool card that spawned them**, as a [`SubagentLog`] of [`SubagentStep`]s, and
//! [`AgentEvent::SubagentActivity`] is the one event that addresses an existing element
//! rather than appending one.
//!
//! 🚨 **Nothing about this is live text, and the model must not let a view pretend it is.**
//! Behaviour 5 is the reason: no deltas ever arrive for a subagent, so a step is only ever
//! a *complete* burst. [`Subagent::Said`] therefore carries a whole string with no
//! completeness bit — there is no provisional state for it to be in — and there is no
//! subagent equivalent of [`AgentEvent::AssistantDelta`] to append one.
//!
//! ⚠️ **Depth is flattened to one, deliberately, and recorded rather than discarded.** A
//! subagent can dispatch its own subagent, and nesting cards inside cards inside a
//! scrollback has no bottom. So every step — however deep the agent that produced it —
//! lands on the **top-level** card, carrying the [`SubagentStep::depth`] it was produced
//! at. One card, one flat log, and a view that can still say a step came from two levels
//! down instead of implying it was direct. See [`Transcript::apply`]'s
//! `SubagentActivity` arm for how the chain is resolved.
//!
//! # Ordering and identity
//!
//! Elements are appended in arrival order and **mutate in place**: a tool card that
//! resolves does not move, and text emitted after a tool call lands after it. Every element
//! carries an [`ElementId`] that is assigned once, never reused, and never changes — so a
//! view may key per-element state (a scroll anchor, an expanded/collapsed bit, a GPU
//! texture for an inline artifact) on it across frames.
//!
//! That is not hypothetical any more: [`Body::Artifact`] is an element the console puts in
//! the flow itself, and a live control panel was the first thing drawn for one. The element
//! carries a **description** — a title, slider names, button names — and nothing else; every
//! value a hand can move lives in the view, in a side map keyed by [`ElementId`]. This split
//! is the reason ids have to be stable rather than a nice property of them being so.
//!
//! [`ArtifactContent::Surface`] is the second kind and the one that makes the id do real
//! work: a rendered picture, whose **GPU texture** the view holds under that id and whose
//! **look** a [`PanelSpec::drives`] panel changes from a few elements below it. Nothing about
//! either the texture or the live look is in this module — which is exactly the test that the
//! split holds.
//!
//! [`Body::Approval`] is the third such element and the one with something real on the
//! other end of it: an agent thread, blocked on a socket, waiting for a human to press a
//! button. The same split holds and matters more — the element *describes* a decision
//! (which tool, which arguments, pending or answered), the view draws it, and the pane
//! holds the half-answered question and sends the verdict back. There is no channel in this
//! module and there must never be one.
//!
//! Ids are also **contiguous** over the retained window, because every id issued is
//! immediately appended and eviction only ever happens at the front. That is what makes
//! [`Transcript::get`] O(1) index arithmetic instead of a map, and
//! [`element_ids_are_contiguous_over_the_retained_window`](self) pins it.
//!
//! # What is bounded, and what is not
//!
//! [`Limits::max_elements`] caps the element count and evicts from the **front**, oldest
//! first. Every eviction is counted ([`Stats::dropped_elements`]) and the cap is readable
//! ([`Transcript::limits`]) — a silent truncation reads as "we covered everything", which
//! is exactly the failure this repo keeps paying for. Evicting a still-running tool card
//! also loses a running id, so that is counted separately
//! ([`Stats::dropped_unresolved_tools`]); with any cap large enough for a visible
//! transcript it stays zero.
//!
//! **Per-element text is deliberately unbounded.** A tool result can be a whole file, and
//! truncating it here would misrepresent the tool's output while looking like the tool's
//! output — the decoder owns line size, and a view owns how much of a card it draws. If
//! that ever needs a limit it belongs next to `max_elements`, with its own counter, not as
//! a quiet `truncate()`.

use std::collections::{HashMap, VecDeque};

/// Identifies one assistant text block. The integrator must make it unique **per rendered
/// block**, not merely per message: one Claude Code `assistant` event can carry several
/// content blocks, and two blocks sharing an id would replace each other.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageId(pub String);

/// A tool call's correlation id — Claude Code's `tool_use.id`, echoed by `tool_use_id` on
/// the result. This is the *only* thing that ties a result to its call.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolId(pub String);

impl MessageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ToolId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for MessageId {
    fn from(s: &str) -> Self {
        MessageId(s.to_string())
    }
}

impl From<String> for MessageId {
    fn from(s: String) -> Self {
        MessageId(s)
    }
}

impl From<&str> for ToolId {
    fn from(s: &str) -> Self {
        ToolId(s.to_string())
    }
}

impl From<String> for ToolId {
    fn from(s: String) -> Self {
        ToolId(s)
    }
}

/// Stable identity for one renderable element. Assigned once, never reused, never changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(pub u64);

/// Stable identity for one turn — a human input and everything the agent did about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TurnId(pub u64);

/// How a run ended. Deliberately coarse: this is the shape every harness can supply, and a
/// harness-specific status string belongs in [`RunEnd::detail`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Ok,
    Error,
    Cancelled,
}

/// The nine events a transcript folds. See the module doc for why this is not the
/// decoder's type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentEvent {
    /// A session begins (Claude Code's `system/init`). Opens a fresh turn **only** if the
    /// current turn already holds something, so the ordinary "init, then the human speaks"
    /// prelude does not leave an empty turn behind.
    SessionStarted { session_id: String },
    /// The human's input — what opens a turn.
    HumanInput { text: String },
    /// A token-level fragment of an assistant text block, to be appended.
    AssistantDelta { message: MessageId, text: String },
    /// The authoritative complete text of an assistant block. **Replaces** whatever the
    /// deltas accumulated; see behaviour 2 in the module doc.
    AssistantMessage { message: MessageId, text: String },
    /// The model emitted a tool call. Send this **before** any argument fragments for the
    /// same id — it is what creates the card to attach them to.
    ///
    /// `arguments` is `Some` only when the text is the **complete, authoritative** input
    /// (Claude Code's `tool_use.input`, re-serialised). `None` means "not yet known";
    /// fragments may follow. Sending the event twice — once to open the card with its
    /// name, once with complete arguments — is the intended streaming shape.
    ToolCall { id: ToolId, name: String, arguments: Option<String> },
    /// A partial fragment of a tool call's JSON input. Appended verbatim; never parsed.
    ToolArgumentsDelta { id: ToolId, fragment: String },
    /// The tool's result, correlated by `id`. This is the **only** thing that ends a
    /// card's running state.
    ///
    /// `detail` is what the tool reported *about* the result beyond its text — see
    /// [`ResultDetail`], and note that it is not an `Option`: a harness that reports
    /// nothing and a tool that has nothing to report are the same thing to a reader.
    ToolResult { id: ToolId, output: String, is_error: bool, detail: ResultDetail },
    /// The run reached its final result. Records an outcome; closes nothing (behaviour 3).
    RunFinished { outcome: RunOutcome, detail: Option<String> },
    /// **A subagent reported something** (behaviour 6). The one event that addresses an
    /// element already in the flow instead of appending a new one.
    ///
    /// `parent` is the tool call the subagent is running inside, exactly as the harness
    /// spelled it — Claude Code's `parent_tool_use_id`. It may name a top-level card, or a
    /// tool call made *by* a subagent (which is how depth 2+ arrives), or nothing we ever
    /// saw. [`Transcript::apply`] resolves which, and the mapper does not have to know.
    SubagentActivity { parent: ToolId, activity: Subagent },
}

/// One thing a subagent did, before it is placed on a card.
///
/// 🚨 **Every arm is a completed fact.** There is no fragment and no in-progress arm,
/// because behaviour 5 measured that no deltas are forwarded for a subagent — an activity
/// that could be half-arrived would be modelling something the wire cannot produce.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subagent {
    /// A complete text block the subagent produced.
    Said(String),
    /// The subagent called a tool of its own.
    Used { id: ToolId, name: String },
    /// One of the subagent's own tool calls came back.
    ///
    /// ⚠️ **The output is deliberately not carried, and this is the one place this module
    /// declines content rather than truncating it.** Everywhere else per-element text is
    /// unbounded on principle. Here the content would be a tool's full output, nested two
    /// frames deep inside a card inside a scrollback, multiplied by every tool every
    /// subagent runs — on the coordinator session this feature exists for, twelve agents
    /// working for a quarter of an hour. What a progress line needs is that the step
    /// *finished* and whether it failed; the parent `Task`'s own result is carried in full
    /// by the card, as it always was.
    Returned { id: ToolId, is_error: bool },
}

/// Whether one of a subagent's own tool steps came back.
///
/// A deliberately smaller [`ToolState`]: that type's `Complete` arm carries the tool's
/// output, and [`Subagent::Returned`] argues why a nested step does not. Reusing it would
/// have meant storing `output: String::new()` and having
/// [`ToolState::output`] answer `Some("")` — a card claiming a tool returned nothing when
/// what happened is that we declined to keep it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    Running,
    Done { is_error: bool },
}

impl StepState {
    pub fn is_running(&self) -> bool {
        matches!(self, StepState::Running)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, StepState::Done { is_error: true })
    }
}

/// What one retained step *is*, once it is on a card.
///
/// ⚠️ **Not the same shape as [`Subagent`], on the module's own precedent.** Three events
/// fold into two stored arms because a tool's call and its return are one thing that
/// changes, not two things that happened — exactly as [`AgentEvent::ToolCall`] and
/// [`AgentEvent::ToolResult`] fold into one [`ToolCard`] that mutates in place
/// (behaviour 1). A log that appended on return would spend half its retained steps
/// restating ids it had already shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubagentAct {
    /// A complete text block the subagent produced.
    Said(String),
    /// A tool the subagent ran. `name` is `None` when the return was seen without the call
    /// — [`Stats::unmatched_subagent_returns`], the same keep-it-anyway rule an orphan
    /// [`ToolCard`] follows.
    Tool { id: ToolId, name: Option<String>, state: StepState },
}

/// One entry in a [`SubagentLog`] — what happened, and how far down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubagentStep {
    pub act: SubagentAct,
    /// `1` for the subagent the card itself spawned, `2` for one that subagent spawned,
    /// and so on. Recorded rather than nested — see behaviour 6.
    ///
    /// Saturates at [`MAX_TRACKED_DEPTH`]: past that the number stops being a count and
    /// starts being a claim about a chain this module stopped following.
    pub depth: u8,
}

/// The deepest nesting [`SubagentStep::depth`] will report.
///
/// 🚨 **A ceiling on the number, not on the attribution, and the distinction is the bug
/// this constant caused before it was written down.** Every step lands on the top-level
/// card no matter how deep the chain — that is the whole point of flattening, and making
/// the *ownership* conditional on this value instead had deep steps fall through to the
/// orphan path and open new top-level cards, which is the nesting hazard again in a flat
/// disguise. Past this depth the chain is still followed; the reported number simply stops
/// counting, because a `depth: 200` badge is noise where "deeper than 8" is information.
///
/// ⚠️ What actually bounds the ownership map is **eviction**: an entry is swept when the
/// card it points at leaves the retained window.
pub const MAX_TRACKED_DEPTH: u8 = 8;

/// What a subagent did, inside the tool card that spawned it.
///
/// Ordinary arrival order, oldest first, capped by [`Limits::max_subagent_steps`] and
/// evicted from the front like the transcript itself — with the same rule about saying so:
/// [`SubagentLog::dropped`] is on the log, where a view drawing the log will see it,
/// because a trace that silently starts in the middle reads as the whole trace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubagentLog {
    pub steps: VecDeque<SubagentStep>,
    /// Steps evicted from the front of this log. Also totalled in
    /// [`Stats::dropped_subagent_steps`].
    pub dropped: u64,
}

impl SubagentLog {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Steps still retained. **Not** the number that happened — add [`Self::dropped`] for
    /// that, which is why the two are separate fields rather than one total.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// The deepest level any retained step came from. `None` for an empty log, `1` for a
    /// log with no nesting in it — so a view can stay silent about depth in the ordinary
    /// case instead of labelling every step.
    pub fn max_depth(&self) -> Option<u8> {
        self.steps.iter().map(|s| s.depth).max()
    }

    /// How many of this log's retained tool steps have not come back.
    ///
    /// ⚠️ **Not evidence the subagent is working**, and a view must not draw it as such.
    /// The only thing that says the subagent is still going is the *parent card* being
    /// unresolved (behaviour 1) — a subagent that died mid-tool leaves this above zero
    /// forever, exactly as an abandoned tool call does in the main flow, and for the same
    /// reason: nothing on the wire retracts it.
    pub fn unreturned(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(&s.act, SubagentAct::Tool { state, .. } if state.is_running()))
            .count()
    }
}

/// A tool call's input, as text plus a completeness bit.
///
/// `complete == false` means what is in `text` may be **half a JSON document** — a view
/// may show it (that is the point of streaming) but must not parse it, and must not
/// present it as structured data. This module never parses it either way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Arguments {
    pub text: String,
    pub complete: bool,
}

impl Arguments {
    /// Nothing known yet — the state a card opens in, and the state an orphan card
    /// (a result whose call was never seen) stays in forever.
    pub fn pending() -> Self {
        Arguments { text: String::new(), complete: false }
    }
}

/// A tool card's state. There is no `Started` — see behaviour 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolState {
    /// The call was emitted and its id is unresolved. This is *derived*, not reported.
    Running,
    Complete { output: String, is_error: bool },
}

impl ToolState {
    pub fn is_running(&self) -> bool {
        matches!(self, ToolState::Running)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, ToolState::Complete { is_error: true, .. })
    }

    /// The tool's output, or `None` while it is still running.
    pub fn output(&self) -> Option<&str> {
        match self {
            ToolState::Running => None,
            ToolState::Complete { output, .. } => Some(output),
        }
    }
}

/// What a tool reported *about* its result, beyond the text of it.
///
/// 🚨 **Every field here appears in a real capture, and the list stops there.** Claude
/// Code sends an undocumented `tool_use_result` beside the `tool_result` block, and the
/// only shape any capture on this machine contains is a `Read`'s: `{"type":"text","file":
/// {"filePath","content","numLines","startLine","totalLines"}}`. So those are the fields,
/// and the ones a richer card would obviously want — a byte count, an exit status, a
/// truncation flag, the unified patch Pi's `Edit` result carries — are **absent because
/// nothing has been observed sending them**, not because they were forgotten. An omitted
/// field beats an invented one; this repo labels what it shows measured or derived, and a
/// field with no capture behind it could be labelled neither.
///
/// 📌 **`content` is decoded and dropped.** It is the file's text, which is already the
/// `tool_result` block's own content in numbered form — carrying it here would be the same
/// file twice in one card, and [`ToolState::Complete`] already holds the version a person
/// reads.
///
/// ⚠️ This is a *harness-agnostic* shape on purpose, like every other type in this module:
/// "the tool acted on this file and covered N of its M lines" is a sentence Pi's
/// `totalLines`/`truncatedBy` can also fill in. The wire spellings stay in
/// [`crate::agent_map`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResultDetail {
    /// The file the tool acted on, spelled exactly as the tool spelled it — an absolute
    /// Windows path in the capture, backslashes included. Not normalised: a path a person
    /// can paste is worth more than a tidy one.
    pub file_path: Option<String>,
    /// Lines this result covers.
    pub lines: Option<u64>,
    /// Lines the file has. Equal to [`Self::lines`] for a whole small file, which is why
    /// both are kept — "4 of 4" and "4 of 900" are different facts and a card that showed
    /// only the first number could not tell them apart.
    pub total_lines: Option<u64>,
    /// The first line covered, when the tool said one.
    pub start_line: Option<u64>,
}

impl ResultDetail {
    /// Whether the tool said anything at all. A card with nothing to add adds no row.
    pub fn is_empty(&self) -> bool {
        *self == ResultDetail::default()
    }
}

/// One tool call and its outcome, as a view draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCard {
    pub call_id: ToolId,
    /// `None` only for an **orphan** card: a result — or a subagent's activity — arrived
    /// for an id whose call was never seen (a compaction boundary, a resumed session, a
    /// parent evicted by the cap). The content is real and is kept rather than dropped;
    /// what is missing is the name.
    pub name: Option<String>,
    pub arguments: Arguments,
    pub state: ToolState,
    /// What a subagent running inside this call has reported (behaviour 6). Empty for the
    /// overwhelming majority of cards — only a `Task` call ever gains one — and never
    /// `Option`, because "no subagent" and "a subagent that has said nothing yet" are the
    /// same thing to every reader of it.
    pub subagent: SubagentLog,
    /// What the tool said about its own result ([`ResultDetail`]). Empty until the result
    /// arrives, and empty forever for a tool that reports nothing — `Default` for the same
    /// reason [`Self::subagent`] is.
    pub detail: ResultDetail,
}

/// One assistant text block.
///
/// It carries no record of whether it streamed, on purpose: subagent text never streams
/// (behaviour 5), and a view able to tell the difference would eventually draw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistantBlock {
    pub message_id: MessageId,
    pub text: String,
    /// `false` while only deltas have arrived — the text is provisional and may be
    /// replaced wholesale by the authoritative message.
    pub complete: bool,
}

/// One human input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HumanBlock {
    pub text: String,
}

/// One inline artifact — something the conversation shows that is not text.
///
/// **This is a description, not a thing on screen.** There is no rect here, no colour, no
/// widget and no closure, for the same reason there is no egui anywhere else in this
/// module: the moment the model knows about layout it stops being a testable state machine
/// and becomes a bad UI framework. It says *what* to show; the view decides how, and owns
/// every pixel of the answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactBlock {
    /// The line naming the artifact, so it is obviously the console's and not the agent's.
    pub title: String,
    pub content: ArtifactContent,
}

/// What kind of artifact it is. The second arm is the one the first was shaped for: an
/// engine-rendered picture, which needs a render-to-texture path and an eviction cap, is a
/// variant rather than a second [`Body`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtifactContent {
    /// A control panel: named sliders and named buttons, in draw order.
    Panel(PanelSpec),
    /// A rendered surface — a picture the engine draws, living in the flow.
    Surface(SurfaceSpec),
}

/// A rendered surface, **described**: what it is a picture *of*, and nothing else.
///
/// 🚨 **No size, no rect, no `TextureId`, and no live look.** The rect is egui layout's — the
/// simplification a conversation buys over a character grid, where a picture had to be
/// arithmetic over reserved rows. The texture is the view's, keyed by [`ElementId`] and
/// bounded by a cap the view states. And the look a hand is *currently* dragging is view
/// state for [`PanelSpec`]'s reason: the transcript is folded from an event stream and its
/// elements mutate as events arrive, so a value living here would be rewritten mid-drag.
///
/// What survives here is the **summoning** look — what the surface opened as. The view seeds
/// from it once and owns it after, exactly as it owns a slider's starting value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurfaceSpec {
    /// The look the surface opened at, by name. Meaningless to this crate — the console's
    /// material table is in the binary — so it is carried, never interpreted, on
    /// [`PanelSpec::buttons`]' contract exactly.
    pub look: String,
}

/// A control panel's controls, **by name only**.
///
/// ⚠️ **A slider's value is deliberately absent.** A value changes as fast as a hand can
/// drag, and the transcript is derived from an event stream whose elements mutate as events
/// arrive — so a value living here would be state with two owners, and the visible symptom
/// is a slider that snaps back while the agent is talking. The view keys the live values off
/// [`ElementId`], which is exactly what stable ids are for. What starts at what is then the
/// first of those values, i.e. also the view's.
///
/// The buttons are named rather than numbered for the reason
/// [`crate::block_panel::BlockAction`] gives: this crate cannot see what a button *means*
/// (the console's material table is in the binary, not here), so it draws a label and
/// reports the label that was pressed, and a shared index would be two lists free to drift.
/// ⚠️ **No `Default`.** It would have to invent a target, and a panel pointed at an element
/// that does not exist is precisely the shape `/panel`'s removal was meant to make
/// unbuildable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelSpec {
    pub sliders: Vec<String>,
    pub buttons: Vec<String>,
    /// The element this panel's controls act on.
    ///
    /// **This is the fix for the flaw beat 7 exposed.** A panel wired to the global backdrop
    /// changes something you cannot see from where you clicked — the effect lands on another
    /// tab, because a conversation has no scrollback for a backdrop to band across. Naming a
    /// target here makes the consequence an element a few rows up, in the same view.
    ///
    /// 🚨 **It is no longer an `Option`.** `None` meant "drive the console", which is what
    /// `/panel` summoned, and a human driving one found exactly the flaw above: the knobs
    /// appeared to do nothing, because their effect was on another tab. The command is gone
    /// and so is the arm — a panel that cannot name a target cannot be built.
    ///
    /// An [`ElementId`] rather than an index, for the reason ids exist: the transcript's
    /// indices shift as the cap evicts from the front, and a panel that silently started
    /// driving its neighbour would be the worst possible failure of an instrument. A target
    /// the cap has evicted simply stops resolving ([`Transcript::get`] answers `None`) and
    /// the panel drives nothing — which the view says on screen rather than papering over.
    pub drives: ElementId,
}

/// Which way a permission request was answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny,
}

/// How an approval was answered, once it was.
///
/// The two flags are separate because they answer different questions and a card says both:
/// `from_memory` is *who* answered (the console, from a decision already made), `remembered`
/// is whether the answer was written down for next time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer {
    pub verdict: Verdict,
    /// Answered from the console's decision memory rather than by a click just now.
    pub from_memory: bool,
    /// The decision is in the memory, and will answer the identical call again — until it
    /// is revoked ([`Transcript::revoke_approval`]).
    pub remembered: bool,
}

/// An approval's state. `Pending` is the only state in which the agent is waiting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalState {
    Pending,
    Answered(Answer),
    /// 🚨 **Nobody is waiting for this any more.** The agent's request died before a human
    /// answered it — its turn was cancelled, the process ended, or the client gave up on
    /// the call.
    ///
    /// A third state rather than an `Answered(Deny)`, because the two are different events
    /// and a card that conflated them would claim a human refused something nobody was
    /// asked about. The wire *is* denied, and fails closed
    /// ([`crate::approval::ABANDONED`]) — but that is the console cleaning up, not a
    /// decision, and the card says so.
    Abandoned,
}

impl ApprovalState {
    pub fn is_pending(&self) -> bool {
        matches!(self, ApprovalState::Pending)
    }

    pub fn answer(&self) -> Option<Answer> {
        match self {
            ApprovalState::Answered(a) => Some(*a),
            ApprovalState::Pending | ApprovalState::Abandoned => None,
        }
    }
}

/// **One "may I?" as an element in the flow.**
///
/// A permission request is the console's own insertion, exactly like [`ArtifactBlock`] —
/// no harness emits it, and it arrives through [`Transcript::insert_approval`] rather than
/// as a ninth [`AgentEvent`], for the reason that method gives.
///
/// 🚨 **It describes a decision; it does not make one.** There is no channel here, no
/// closure and no egui — the same rule the whole module keeps, and a sharper case of it,
/// because the thing on the other end of a real approval is a blocked thread on a socket.
/// The view draws this and reports the button; the pane holds the half-answered question
/// and sends the verdict back. If this type ever gains a `Sender`, the transcript has
/// stopped being a state machine and become an actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalBlock {
    /// The tool being gated, namespaced as the client spells it: `Bash`, `mcp__organon__…`.
    /// Not necessarily one of the console's — the handler answers for everything the agent
    /// calls, which is the finding that made this feature worth building.
    pub tool_name: String,
    /// The proposed arguments as JSON **text**, on [`Arguments`]' contract: carried, never
    /// interpreted. A view parses it; this module does not.
    pub input: String,
    /// The `toolu_…` id, so a card can be tied to the tool element for the same call.
    pub tool_use_id: String,
    pub state: ApprovalState,
}

/// The end of a run, as an element so it keeps its place in the flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunEnd {
    pub outcome: RunOutcome,
    /// Whatever the harness said about it — Claude Code's `subtype`/`status_detail`. Free
    /// text; this module never interprets it.
    pub detail: Option<String>,
}

/// What an element actually is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Human(HumanBlock),
    Assistant(AssistantBlock),
    Tool(ToolCard),
    RunEnd(RunEnd),
    /// An inline artifact. Unlike the other four this arrives through
    /// [`Transcript::insert_artifact`] rather than from an [`AgentEvent`] — see that
    /// method for why the summoning path is deliberately not an event.
    Artifact(ArtifactBlock),
    /// A permission request awaiting a human. Inserted by the console for the same reason
    /// an artifact is, and answered by [`Transcript::answer_approval`].
    Approval(ApprovalBlock),
}

/// One renderable thing, with stable identity and a turn it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    pub id: ElementId,
    pub turn: TurnId,
    pub body: Body,
}

impl Element {
    pub fn human(&self) -> Option<&HumanBlock> {
        match &self.body {
            Body::Human(h) => Some(h),
            _ => None,
        }
    }

    pub fn assistant(&self) -> Option<&AssistantBlock> {
        match &self.body {
            Body::Assistant(a) => Some(a),
            _ => None,
        }
    }

    pub fn tool(&self) -> Option<&ToolCard> {
        match &self.body {
            Body::Tool(t) => Some(t),
            _ => None,
        }
    }

    pub fn run_end(&self) -> Option<&RunEnd> {
        match &self.body {
            Body::RunEnd(r) => Some(r),
            _ => None,
        }
    }

    pub fn artifact(&self) -> Option<&ArtifactBlock> {
        match &self.body {
            Body::Artifact(a) => Some(a),
            _ => None,
        }
    }

    pub fn approval(&self) -> Option<&ApprovalBlock> {
        match &self.body {
            Body::Approval(a) => Some(a),
            _ => None,
        }
    }
}

/// Whether a turn is still running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnState {
    Open,
    Finished(RunOutcome),
}

/// A turn: a human input and everything the agent did about it.
///
/// Grouping is carried by [`Element::turn`] rather than by nesting, so an element never
/// moves between containers and a view can walk one flat list and break on the id change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub id: TurnId,
    /// The session this turn belongs to, if one was announced.
    pub session: Option<String>,
    pub state: TurnState,
    /// Set when an event landed in this turn **after** it finished — a legitimately late
    /// tool result, most often. Not an error; worth being able to see.
    pub trailing: bool,
    /// How many of this turn's elements are still retained (the cap evicts from the front).
    pub retained: usize,
}

/// What one [`Transcript::apply`] did, so a view can repaint or auto-scroll precisely.
///
/// ⚠️ An `Appended` may be accompanied by an eviction at the front when the cap is
/// reached; [`Stats::dropped_elements`] is the record of that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Appended(ElementId),
    Updated(ElementId),
    /// Transcript metadata moved; no element changed.
    Meta,
    Ignored(Ignored),
}

/// Why an event changed nothing. Each is also counted in [`Stats`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ignored {
    /// A delta for a block whose authoritative text has already arrived (behaviour 2).
    LateDelta,
    /// A second result, or a late call, for an id that is already resolved.
    ToolAlreadyResolved,
    /// An argument fragment for an id with no card. Nothing renderable to attach it to —
    /// and nothing is lost, because the authoritative arguments arrive with the call.
    UnknownToolFragment,
    /// An answer for an approval that has already been answered, or for an id that is not
    /// an approval at all. A question is answered **once**: the agent is unblocked by the
    /// first answer, and a second would record a verdict nothing acted on.
    ApprovalAlreadyAnswered,
}

/// Bounds, stated rather than assumed. See "What is bounded" in the module doc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Maximum retained elements; the oldest are evicted first. Treated as at least 1 —
    /// a transcript that cannot hold the element it was just given is not a transcript.
    pub max_elements: usize,
    /// Maximum retained [`SubagentStep`]s **per card**, oldest evicted first.
    ///
    /// ⚠️ A second cap rather than a share of the first, because the two count different
    /// things: `max_elements` bounds the flow a human scrolls, and this bounds a trace
    /// that accumulates inside one element of it without ever appending to that flow. A
    /// single `Task` can outrun any element budget on its own — the session that motivated
    /// this dispatched twelve agents for a quarter of an hour each — and it must not be
    /// able to evict the conversation around it to do so. Treated as at least 1.
    pub max_subagent_steps: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            // The same order of magnitude as `term::SCROLLBACK_LINES`, and for the same
            // reason: far past any session a human reads back through, near enough that
            // memory stays bounded on a coordinator run that fans out for hours.
            max_elements: 10_000,
            // Two orders smaller on purpose. This is the *inside* of one card, which a
            // view shows the tail of; a hundred steps is already more trace than anyone
            // reads, and the count that matters — how many there were — survives eviction
            // in `SubagentLog::dropped`.
            max_subagent_steps: 100,
        }
    }
}

/// Everything the model chose not to render, counted. Nothing here is fatal; all of it is
/// the kind of thing that is invisible until someone asks why a card never resolved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub events: u64,
    pub dropped_elements: u64,
    /// Evicted while still running — a running id lost to the cap.
    pub dropped_unresolved_tools: u64,
    /// Results whose call was never seen; kept as an orphan card, not discarded.
    pub orphan_results: u64,
    /// Subagent activity whose parent call was never seen — the same shape as
    /// [`orphan_results`](Self::orphan_results) and handled the same way, by keeping the
    /// content on a nameless card rather than discarding it. Counted separately because
    /// the two arrive for different reasons and only one of them is evidence the parent
    /// **finished**.
    pub orphan_subagent_activity: u64,
    /// A [`Subagent::Returned`] naming a step this card's log has no [`Subagent::Used`]
    /// for. Recorded as its own step rather than dropped; counted so a correlation that
    /// stops working cannot hide behind a log that still looks busy.
    pub unmatched_subagent_returns: u64,
    /// [`SubagentStep`]s evicted by [`Limits::max_subagent_steps`], across every card.
    pub dropped_subagent_steps: u64,
    pub orphan_argument_fragments: u64,
    pub late_deltas: u64,
    pub duplicate_results: u64,
    pub duplicate_calls: u64,
    /// Events that landed in an already-finished turn.
    pub trailing_events: u64,
    /// Answers for an approval that was already answered, or for an element that is not
    /// one.
    pub duplicate_answers: u64,
    /// ⚠️ Approvals the cap evicted **while the agent was still waiting on them**. Unlike
    /// the other counters this one is not merely informational: whoever holds the other
    /// end of the question has to fail it closed, or the agent waits forever. Counted
    /// separately from [`Stats::dropped_elements`] so "we lost a decision" can never be
    /// read as ordinary trimming.
    pub dropped_pending_approvals: u64,
}

/// The transcript: events in, ordered renderable elements out.
#[derive(Clone, Debug)]
pub struct Transcript {
    elements: VecDeque<Element>,
    turns: VecDeque<Turn>,
    by_message: HashMap<String, ElementId>,
    by_tool: HashMap<String, ElementId>,
    /// Behaviour 6's chain. A tool id a *subagent* called → the top-level [`ToolId`] whose
    /// card owns it, and the depth that subagent was running at. This is what turns a
    /// depth-2 `parent_tool_use_id` — which names a call that is only ever a step inside
    /// another card's log, never an element — into the one card a human can actually see.
    ///
    /// ⚠️ Keyed by the *nested* id and never removed on resolution: a subagent's tool that
    /// has already returned can still be the parent named by a later line, and forgetting
    /// it would turn a perfectly attributable step into an orphan. It is bounded instead by
    /// the log cap that bounds everything else — an entry whose card is evicted is swept
    /// with it.
    subagent_owner: HashMap<String, (ToolId, u8)>,
    running: Vec<ElementId>,
    pending: Vec<ElementId>,
    session: Option<String>,
    next_element: u64,
    next_turn: u64,
    limits: Limits,
    stats: Stats,
}

impl Default for Transcript {
    fn default() -> Self {
        Transcript::new()
    }
}

impl Transcript {
    pub fn new() -> Self {
        Transcript::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        Transcript {
            elements: VecDeque::new(),
            turns: VecDeque::new(),
            by_message: HashMap::new(),
            by_tool: HashMap::new(),
            subagent_owner: HashMap::new(),
            running: Vec::new(),
            pending: Vec::new(),
            session: None,
            next_element: 0,
            next_turn: 0,
            limits,
            stats: Stats::default(),
        }
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The most recently announced session id, if any.
    pub fn session_id(&self) -> Option<&str> {
        self.session.as_deref()
    }

    /// The retained elements, oldest first. Indexable and `len()`-able for a virtualised
    /// view; the container type is `std`'s so the caller is not tied to this module.
    pub fn elements(&self) -> &VecDeque<Element> {
        &self.elements
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// O(1): ids are contiguous over the retained window (module doc, "Ordering and
    /// identity"). Returns `None` for an evicted or not-yet-issued id.
    pub fn get(&self, id: ElementId) -> Option<&Element> {
        self.index_of(id).map(|i| &self.elements[i])
    }

    /// The retained turns, oldest first.
    pub fn turns(&self) -> &VecDeque<Turn> {
        &self.turns
    }

    pub fn turn(&self, id: TurnId) -> Option<&Turn> {
        self.turn_index(id).map(|i| &self.turns[i])
    }

    /// The turn new elements land in — the newest one. `None` before the first event.
    pub fn current_turn(&self) -> Option<&Turn> {
        self.turns.back()
    }

    /// **The answer to "what is running", in call order.** Each id names a [`ToolCard`]
    /// whose [`ToolState`] is `Running`; the list shrinks as results arrive, keeping the
    /// order of whatever is left. Several entries at once is the normal case: read-only
    /// tools are dispatched concurrently.
    pub fn running_tools(&self) -> &[ElementId] {
        &self.running
    }

    /// Whether anything is unresolved right now — the spinner's condition.
    pub fn is_working(&self) -> bool {
        !self.running.is_empty()
    }

    /// **The approvals a human still owes an answer to**, in arrival order. Each names an
    /// [`ApprovalBlock`] whose state is `Pending`, and behind each is an agent that cannot
    /// proceed — which is why this is a first-class query and not a filter the view is
    /// expected to remember to write.
    pub fn pending_approvals(&self) -> &[ElementId] {
        &self.pending
    }

    /// Whether the agent is blocked on a human right now.
    pub fn is_waiting(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The card for a call id, resolved or not.
    pub fn tool(&self, id: &ToolId) -> Option<&ToolCard> {
        let eid = *self.by_tool.get(id.as_str())?;
        self.get(eid)?.tool()
    }

    /// Fold one event into the transcript.
    pub fn apply(&mut self, event: AgentEvent) -> Change {
        self.stats.events += 1;
        match event {
            AgentEvent::SessionStarted { session_id } => {
                self.session = Some(session_id.clone());
                // Only break the turn if the current one has already collected something;
                // `system/init` before the first human message must not strand an empty turn.
                let needs_break =
                    self.turns.back().map(|t| t.retained > 0).unwrap_or(false);
                if needs_break || self.turns.is_empty() {
                    self.open_turn();
                } else if let Some(t) = self.turns.back_mut() {
                    t.session = Some(session_id);
                }
                Change::Meta
            }

            AgentEvent::HumanInput { text } => {
                // A human input always opens a turn — including mid-run (queued input),
                // which is why a tool from the previous turn can resolve after this.
                if self.turns.back().map(|t| t.retained > 0).unwrap_or(true) {
                    self.open_turn();
                }
                let turn = self.ensure_turn();
                let id = self.append(turn, Body::Human(HumanBlock { text }));
                Change::Appended(id)
            }

            AgentEvent::AssistantDelta { message, text } => {
                if let Some(idx) = self.index_by_message(&message) {
                    let late = matches!(&self.elements[idx].body, Body::Assistant(a) if a.complete);
                    if late {
                        self.stats.late_deltas += 1;
                        return Change::Ignored(Ignored::LateDelta);
                    }
                    let (eid, turn) = (self.elements[idx].id, self.elements[idx].turn);
                    if let Body::Assistant(a) = &mut self.elements[idx].body {
                        a.text.push_str(&text);
                    }
                    self.touch_turn(turn);
                    return Change::Updated(eid);
                }
                let turn = self.ensure_turn();
                let block =
                    AssistantBlock { message_id: message.clone(), text, complete: false };
                let id = self.append(turn, Body::Assistant(block));
                self.by_message.insert(message.0, id);
                Change::Appended(id)
            }

            AgentEvent::AssistantMessage { message, text } => {
                if let Some(idx) = self.index_by_message(&message) {
                    let (eid, turn) = (self.elements[idx].id, self.elements[idx].turn);
                    if let Body::Assistant(a) = &mut self.elements[idx].body {
                        // Replacement, not append: this is what makes the deltas pure
                        // presentation (module doc, behaviour 2).
                        a.text = text;
                        a.complete = true;
                    }
                    self.touch_turn(turn);
                    return Change::Updated(eid);
                }
                let turn = self.ensure_turn();
                let block =
                    AssistantBlock { message_id: message.clone(), text, complete: true };
                let id = self.append(turn, Body::Assistant(block));
                self.by_message.insert(message.0, id);
                Change::Appended(id)
            }

            AgentEvent::ToolCall { id, name, arguments } => {
                if let Some(idx) = self.index_by_tool(&id) {
                    let resolved =
                        matches!(&self.elements[idx].body, Body::Tool(c) if !c.state.is_running());
                    if resolved {
                        self.stats.duplicate_calls += 1;
                        return Change::Ignored(Ignored::ToolAlreadyResolved);
                    }
                    let (eid, turn) = (self.elements[idx].id, self.elements[idx].turn);
                    if let Body::Tool(c) = &mut self.elements[idx].body {
                        c.name = Some(name);
                        if let Some(args) = arguments {
                            c.arguments = Arguments { text: args, complete: true };
                        }
                    }
                    self.touch_turn(turn);
                    return Change::Updated(eid);
                }
                let turn = self.ensure_turn();
                let card = ToolCard {
                    call_id: id.clone(),
                    name: Some(name),
                    arguments: match arguments {
                        Some(args) => Arguments { text: args, complete: true },
                        None => Arguments::pending(),
                    },
                    state: ToolState::Running,
                    subagent: SubagentLog::default(),
                    detail: ResultDetail::default(),
                };
                let eid = self.append(turn, Body::Tool(card));
                self.by_tool.insert(id.0, eid);
                self.running.push(eid);
                Change::Appended(eid)
            }

            AgentEvent::ToolArgumentsDelta { id, fragment } => {
                let Some(idx) = self.index_by_tool(&id) else {
                    self.stats.orphan_argument_fragments += 1;
                    return Change::Ignored(Ignored::UnknownToolFragment);
                };
                let (eid, turn) = (self.elements[idx].id, self.elements[idx].turn);
                if let Body::Tool(c) = &mut self.elements[idx].body {
                    // Once the authoritative input has landed, a fragment is noise for the
                    // same reason a late delta is.
                    if !c.arguments.complete {
                        c.arguments.text.push_str(&fragment);
                    }
                }
                self.touch_turn(turn);
                Change::Updated(eid)
            }

            AgentEvent::ToolResult { id, output, is_error, detail } => {
                if let Some(idx) = self.index_by_tool(&id) {
                    let resolved =
                        matches!(&self.elements[idx].body, Body::Tool(c) if !c.state.is_running());
                    if resolved {
                        self.stats.duplicate_results += 1;
                        return Change::Ignored(Ignored::ToolAlreadyResolved);
                    }
                    let (eid, turn) = (self.elements[idx].id, self.elements[idx].turn);
                    if let Body::Tool(c) = &mut self.elements[idx].body {
                        c.state = ToolState::Complete { output, is_error };
                        c.detail = detail;
                    }
                    self.running.retain(|x| *x != eid);
                    self.touch_turn(turn);
                    return Change::Updated(eid);
                }
                // An orphan: the call was never seen, but the output is real content and
                // dropping it would be the silent truncation this module refuses to do.
                self.stats.orphan_results += 1;
                let turn = self.ensure_turn();
                let card = ToolCard {
                    call_id: id.clone(),
                    name: None,
                    arguments: Arguments::pending(),
                    state: ToolState::Complete { output, is_error },
                    subagent: SubagentLog::default(),
                    // ⚠️ Kept on an orphan card, and it is worth more here than anywhere
                    // else: with no call there are no arguments, so the detail's own
                    // `file_path` is the only thing that says what the tool touched.
                    detail,
                };
                let eid = self.append(turn, Body::Tool(card));
                self.by_tool.insert(id.0, eid);
                Change::Appended(eid)
            }

            AgentEvent::RunFinished { outcome, detail } => {
                let turn = self.ensure_turn();
                let id = self.append(turn, Body::RunEnd(RunEnd { outcome, detail }));
                if let Some(i) = self.turn_index(turn) {
                    // Already-finished stays finished with the newest outcome; `trailing`
                    // was set by `append` → `touch_turn` before this ran.
                    self.turns[i].state = TurnState::Finished(outcome);
                }
                Change::Appended(id)
            }

            // Behaviour 6. The only arm that *addresses* an element instead of appending
            // one: a subagent has no place of its own in the flow, and giving it one is
            // precisely the "turns belonging to nobody" §5.9.3 rule 5 refused.
            AgentEvent::SubagentActivity { parent, activity } => {
                let (owner, depth) = self.resolve_subagent_parent(&parent);
                let mut opened_card = false;
                let index = match self.index_by_tool(&owner) {
                    Some(index) => index,
                    // An orphan, on `orphan_results`' precedent exactly: the activity is
                    // real content, and the parent is missing for the same ordinary
                    // reasons — a compaction boundary, a resumed session, or a card the
                    // cap evicted out from under a subagent still working inside it.
                    // Keeping it nameless beats dropping it.
                    //
                    // ⚠️ It opens `Running` and joins the running set, which is a claim.
                    // The claim is the one behaviour 1 already licenses — running-ness is
                    // *derived from an unresolved id* — and live activity from inside a
                    // call is the strongest evidence available that the call has not
                    // finished. The alternative, opening it `Complete`, would invent the
                    // result behaviour 3 refuses to invent.
                    None => {
                        self.stats.orphan_subagent_activity += 1;
                        opened_card = true;
                        let turn = self.ensure_turn();
                        let card = ToolCard {
                            call_id: owner.clone(),
                            name: None,
                            arguments: Arguments::pending(),
                            state: ToolState::Running,
                            subagent: SubagentLog::default(),
                            detail: ResultDetail::default(),
                        };
                        let eid = self.append(turn, Body::Tool(card));
                        self.by_tool.insert(owner.0.clone(), eid);
                        self.running.push(eid);
                        self.index_of(eid).expect("just appended")
                    }
                };

                // Recorded before the borrow, because a nested call has to be attributable
                // even if its own card is the thing that later goes away.
                //
                // 🚨 **Recorded at every depth, and the depth saturates instead.** Making
                // this conditional on the depth was the first implementation and it was
                // wrong in the exact way behaviour 6 exists to prevent: past the cutoff a
                // step stopped resolving to its owner, fell through to the orphan path, and
                // **opened a new top-level card** — so a deep chain grew a fresh card every
                // few levels and the nesting hazard came back wearing a flat disguise. The
                // depth bounds what is *reported*; nothing bounds what is attributed.
                if let Subagent::Used { id, .. } = &activity {
                    let nested = depth.saturating_add(1).min(MAX_TRACKED_DEPTH);
                    self.subagent_owner.insert(id.0.clone(), (owner.clone(), nested));
                }

                let (eid, turn) = (self.elements[index].id, self.elements[index].turn);
                let mut unmatched = false;
                let mut dropped = 0u64;
                if let Body::Tool(card) = &mut self.elements[index].body {
                    match activity {
                        Subagent::Said(text) => {
                            card.subagent
                                .steps
                                .push_back(SubagentStep { act: SubagentAct::Said(text), depth });
                        }
                        Subagent::Used { id, name } => {
                            card.subagent.steps.push_back(SubagentStep {
                                act: SubagentAct::Tool {
                                    id,
                                    name: Some(name),
                                    state: StepState::Running,
                                },
                                depth,
                            });
                        }
                        // Resolves its step in place rather than appending — the same
                        // shape `ToolResult` has against a `ToolCard`.
                        Subagent::Returned { id, is_error } => {
                            let found = card.subagent.steps.iter_mut().rev().find(|s| {
                                matches!(&s.act, SubagentAct::Tool { id: sid, state, .. }
                                    if *sid == id && state.is_running())
                            });
                            match found {
                                Some(step) => {
                                    if let SubagentAct::Tool { state, .. } = &mut step.act {
                                        *state = StepState::Done { is_error };
                                    }
                                }
                                // The call was never seen — evicted from this log, or
                                // never sent. Kept as a nameless step for the same reason
                                // an orphan card is kept.
                                None => {
                                    unmatched = true;
                                    card.subagent.steps.push_back(SubagentStep {
                                        act: SubagentAct::Tool {
                                            id,
                                            name: None,
                                            state: StepState::Done { is_error },
                                        },
                                        depth,
                                    });
                                }
                            }
                        }
                    }
                    let cap = self.limits.max_subagent_steps.max(1);
                    while card.subagent.steps.len() > cap {
                        card.subagent.steps.pop_front();
                        card.subagent.dropped += 1;
                        dropped += 1;
                    }
                }
                if unmatched {
                    self.stats.unmatched_subagent_returns += 1;
                }
                self.stats.dropped_subagent_steps += dropped;
                self.touch_turn(turn);
                // ⚠️ The distinction is not cosmetic: the view re-arms its scroll-follow on
                // `Appended` and merely repaints on `Updated`. A step landing inside a card
                // already in the flow must **not** yank a reader who has scrolled up — the
                // card is where it always was. An orphan, though, really did put a new
                // element at the bottom, and reporting that as an update would leave it
                // below the fold with nothing to say it had arrived.
                if opened_card {
                    Change::Appended(eid)
                } else {
                    Change::Updated(eid)
                }
            }
        }
    }

    /// Behaviour 6's flattening, in one place: **which visible card does this belong to,
    /// and how deep was the agent that produced it.**
    ///
    /// A `parent_tool_use_id` naming a top-level call is depth 1 and resolves to itself. One
    /// naming a call a subagent made resolves to whatever *that* call was attributed to,
    /// one level deeper — which is how a chain of any length collapses onto the single card
    /// at the top of it. An id we know nothing about resolves to itself at depth 1 and
    /// becomes an orphan, because a made-up owner would be worse than a missing one.
    fn resolve_subagent_parent(&self, parent: &ToolId) -> (ToolId, u8) {
        match self.subagent_owner.get(parent.as_str()) {
            Some((owner, depth)) => (owner.clone(), (*depth).min(MAX_TRACKED_DEPTH)),
            None => (parent.clone(), 1),
        }
    }

    /// Append an inline artifact to the current turn.
    ///
    /// # Why this is a method and not a ninth [`AgentEvent`]
    ///
    /// The events are *what a harness said*, and no harness said this. An artifact is
    /// summoned by the console: today by a local composer command the view handles and never
    /// forwards (`/panel`), and next by the integrator noticing a tool call and answering it
    /// with a panel — at which point the tool card is the anchor and this call is what the
    /// handler makes. Both callers want the same thing: *put this in the flow, here, now*.
    /// Putting it in the event enum instead would oblige every harness mapping to carry an
    /// event none of them can produce, and would put the summoning path inside the fold it
    /// is supposed to be independent of.
    ///
    /// It is an ordinary element in every other respect — it takes the next id, lands in the
    /// current turn, counts against the cap, and evicts from the front like anything else.
    pub fn insert_artifact(&mut self, artifact: ArtifactBlock) -> Change {
        let turn = self.ensure_turn();
        Change::Appended(self.append(turn, Body::Artifact(artifact)))
    }

    /// Put a permission request in the flow — **where the agent is working**, which is the
    /// end of the current turn, exactly where an artifact lands.
    ///
    /// It is [`Transcript::insert_artifact`]'s sibling and for the same reason: no harness
    /// emits this. It arrives from the console's own MCP handler, off a serve thread, and
    /// making it a ninth [`AgentEvent`] would oblige every harness mapping to carry an
    /// event none of them can produce.
    ///
    /// A block inserted already `Answered` — the decision memory answering for the human —
    /// is **still an element**, deliberately: a remembered decision nobody can see is worse
    /// than being asked every time. It simply never joins [`Transcript::pending_approvals`].
    pub fn insert_approval(&mut self, approval: ApprovalBlock) -> Change {
        let turn = self.ensure_turn();
        let pending = approval.state.is_pending();
        let id = self.append(turn, Body::Approval(approval));
        if pending {
            self.pending.push(id);
        }
        Change::Appended(id)
    }

    /// Answer a pending approval. `Ignored` if it was already answered, was evicted, or is
    /// not an approval at all — the caller then knows not to send a verdict on the wire.
    pub fn answer_approval(&mut self, id: ElementId, answer: Answer) -> Change {
        let Some(index) = self.index_of(id) else {
            self.stats.duplicate_answers += 1;
            return Change::Ignored(Ignored::ApprovalAlreadyAnswered);
        };
        let open = matches!(&self.elements[index].body, Body::Approval(a) if a.state.is_pending());
        if !open {
            self.stats.duplicate_answers += 1;
            return Change::Ignored(Ignored::ApprovalAlreadyAnswered);
        }
        let turn = self.elements[index].turn;
        if let Body::Approval(a) = &mut self.elements[index].body {
            a.state = ApprovalState::Answered(answer);
        }
        self.pending.retain(|x| *x != id);
        self.touch_turn(turn);
        Change::Updated(id)
    }

    /// **Close a question nobody is waiting for any more**, without claiming it was
    /// decided.
    ///
    /// The sibling of [`Self::answer_approval`] and the reason [`ApprovalState`] has three
    /// arms: this leaves the card in the flow, in its place, saying what became of it —
    /// and takes it out of [`Self::pending_approvals`] so nothing offers to answer it.
    /// Ignored for anything that is not currently pending, which makes it safe to sweep
    /// every frame.
    pub fn abandon_approval(&mut self, id: ElementId) -> Change {
        let Some(index) = self.index_of(id) else {
            return Change::Ignored(Ignored::ApprovalAlreadyAnswered);
        };
        let open = matches!(&self.elements[index].body, Body::Approval(a) if a.state.is_pending());
        if !open {
            return Change::Ignored(Ignored::ApprovalAlreadyAnswered);
        }
        let turn = self.elements[index].turn;
        if let Body::Approval(a) = &mut self.elements[index].body {
            a.state = ApprovalState::Abandoned;
        }
        self.pending.retain(|x| *x != id);
        self.touch_turn(turn);
        Change::Updated(id)
    }

    /// Clear the `remembered` mark on an answered approval — the revocation the memory's
    /// visibility exists for.
    ///
    /// **The verdict itself never changes.** What happened, happened; only the promise to
    /// answer the same way again is withdrawn. Ignored for anything that is not an
    /// answered, remembered approval, so a second click is a no-op rather than a lie.
    pub fn revoke_approval(&mut self, id: ElementId) -> Change {
        let Some(index) = self.index_of(id) else {
            return Change::Ignored(Ignored::ApprovalAlreadyAnswered);
        };
        let Body::Approval(a) = &mut self.elements[index].body else {
            return Change::Ignored(Ignored::ApprovalAlreadyAnswered);
        };
        match a.state {
            ApprovalState::Answered(answer) if answer.remembered => {
                a.state = ApprovalState::Answered(Answer { remembered: false, ..answer });
                Change::Updated(id)
            }
            _ => Change::Ignored(Ignored::ApprovalAlreadyAnswered),
        }
    }

    // ---- internals -------------------------------------------------------------

    fn index_of(&self, id: ElementId) -> Option<usize> {
        let front = self.elements.front()?.id.0;
        let i = id.0.checked_sub(front)? as usize;
        (i < self.elements.len()).then_some(i)
    }

    fn turn_index(&self, id: TurnId) -> Option<usize> {
        let front = self.turns.front()?.id.0;
        let i = id.0.checked_sub(front)? as usize;
        (i < self.turns.len()).then_some(i)
    }

    fn index_by_message(&self, id: &MessageId) -> Option<usize> {
        self.index_of(*self.by_message.get(id.as_str())?)
    }

    fn index_by_tool(&self, id: &ToolId) -> Option<usize> {
        self.index_of(*self.by_tool.get(id.as_str())?)
    }

    fn open_turn(&mut self) -> TurnId {
        let id = TurnId(self.next_turn);
        self.next_turn += 1;
        self.turns.push_back(Turn {
            id,
            session: self.session.clone(),
            state: TurnState::Open,
            trailing: false,
            retained: 0,
        });
        id
    }

    fn ensure_turn(&mut self) -> TurnId {
        match self.turns.back() {
            Some(t) => t.id,
            None => self.open_turn(),
        }
    }

    /// Note that an event landed in `turn`, flagging it if the run there already ended.
    fn touch_turn(&mut self, turn: TurnId) {
        if let Some(i) = self.turn_index(turn) {
            if matches!(self.turns[i].state, TurnState::Finished(_)) && !self.turns[i].trailing {
                self.turns[i].trailing = true;
            }
            if matches!(self.turns[i].state, TurnState::Finished(_)) {
                self.stats.trailing_events += 1;
            }
        }
    }

    fn append(&mut self, turn: TurnId, body: Body) -> ElementId {
        let id = ElementId(self.next_element);
        self.next_element += 1;
        self.touch_turn(turn);
        if let Some(i) = self.turn_index(turn) {
            self.turns[i].retained += 1;
        }
        self.elements.push_back(Element { id, turn, body });
        self.enforce_cap();
        id
    }

    fn enforce_cap(&mut self) {
        let cap = self.limits.max_elements.max(1);
        while self.elements.len() > cap {
            let gone = self.elements.pop_front().expect("len > cap >= 1");
            self.stats.dropped_elements += 1;
            match &gone.body {
                Body::Assistant(a) => {
                    if self.by_message.get(a.message_id.as_str()) == Some(&gone.id) {
                        self.by_message.remove(a.message_id.as_str());
                    }
                }
                Body::Tool(c) => {
                    if self.by_tool.get(c.call_id.as_str()) == Some(&gone.id) {
                        self.by_tool.remove(c.call_id.as_str());
                    }
                    if c.state.is_running() {
                        self.running.retain(|x| *x != gone.id);
                        self.stats.dropped_unresolved_tools += 1;
                    }
                    // The chain has to go with the card it pointed at, or `subagent_owner`
                    // is the one map here that grows for the life of the process — a
                    // coordinator run makes an entry per tool per subagent and never stops.
                    // ⚠️ Guarded on the log being non-empty, not merely tidy: this is a
                    // scan of the whole map, and without the guard every ordinary `Read`
                    // card leaving the window would pay for it.
                    if !c.subagent.is_empty() {
                        let owner = c.call_id.clone();
                        self.subagent_owner.retain(|_, (o, _)| *o != owner);
                    }
                }
                // ⚠️ An evicted *pending* approval is the one eviction with a consequence
                // outside this module: an agent is blocked on it. The count is how the
                // pane learns to fail it closed rather than leave the question hanging.
                Body::Approval(a) => {
                    if a.state.is_pending() {
                        self.pending.retain(|x| *x != gone.id);
                        self.stats.dropped_pending_approvals += 1;
                    }
                }
                // An artifact holds no correlation and no running-ness; the view's
                // per-element widget state goes with it, keyed off an id that
                // `Transcript::get` now answers `None` for.
                Body::Human(_) | Body::RunEnd(_) | Body::Artifact(_) => {}
            }
            if let Some(i) = self.turn_index(gone.turn) {
                self.turns[i].retained = self.turns[i].retained.saturating_sub(1);
            }
            // A turn with nothing retained is not renderable; the newest is kept even when
            // empty because it is where the next element goes.
            while self.turns.len() > 1 && self.turns.front().map_or(false, |t| t.retained == 0) {
                self.turns.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn human(text: &str) -> AgentEvent {
        AgentEvent::HumanInput { text: text.to_string() }
    }

    fn delta(msg: &str, text: &str) -> AgentEvent {
        AgentEvent::AssistantDelta { message: msg.into(), text: text.to_string() }
    }

    fn complete(msg: &str, text: &str) -> AgentEvent {
        AgentEvent::AssistantMessage { message: msg.into(), text: text.to_string() }
    }

    fn call(id: &str, name: &str, args: Option<&str>) -> AgentEvent {
        AgentEvent::ToolCall {
            id: id.into(),
            name: name.to_string(),
            arguments: args.map(str::to_string),
        }
    }

    fn result(id: &str, output: &str) -> AgentEvent {
        AgentEvent::ToolResult {
            id: id.into(),
            output: output.to_string(),
            is_error: false,
            detail: ResultDetail::default(),
        }
    }

    /// The same, carrying what the tool said about the file it read.
    fn result_with_detail(id: &str, output: &str, detail: ResultDetail) -> AgentEvent {
        AgentEvent::ToolResult {
            id: id.into(),
            output: output.to_string(),
            is_error: false,
            detail,
        }
    }

    fn finished() -> AgentEvent {
        AgentEvent::RunFinished { outcome: RunOutcome::Ok, detail: None }
    }

    fn feed(t: &mut Transcript, events: Vec<AgentEvent>) {
        for e in events {
            t.apply(e);
        }
    }

    fn ids(t: &Transcript) -> Vec<u64> {
        t.elements().iter().map(|e| e.id.0).collect()
    }

    /// Deltas that deliberately disagree with the authoritative text. The final text wins,
    /// wholesale — this is the invariant that makes streaming pure presentation.
    #[test]
    fn the_complete_message_replaces_disagreeing_deltas() {
        let mut t = Transcript::new();
        feed(
            &mut t,
            vec![
                human("hi"),
                delta("m1", "Wrong "),
                delta("m1", "and duplicated "),
                delta("m1", "and duplicated "),
            ],
        );
        let eid = t.elements()[1].id;
        assert_eq!(t.get(eid).unwrap().assistant().unwrap().text, "Wrong and duplicated and duplicated ");
        assert!(!t.get(eid).unwrap().assistant().unwrap().complete);

        assert_eq!(t.apply(complete("m1", "The real answer.")), Change::Updated(eid));
        let block = t.get(eid).unwrap().assistant().unwrap();
        assert_eq!(block.text, "The real answer.", "the authoritative text must win outright");
        assert!(block.complete);
        assert_eq!(t.len(), 2, "replacement must not create a second element");
    }

    /// The other half of the same rule: once the authoritative text has landed, a straggler
    /// delta is noise, and noise is counted rather than concatenated.
    #[test]
    fn a_delta_after_the_complete_message_is_ignored_and_counted() {
        let mut t = Transcript::new();
        feed(&mut t, vec![delta("m1", "par"), complete("m1", "final")]);
        assert_eq!(t.apply(delta("m1", "tial")), Change::Ignored(Ignored::LateDelta));
        assert_eq!(t.elements()[0].assistant().unwrap().text, "final");
        assert_eq!(t.stats().late_deltas, 1);
    }

    /// Behaviour 5: subagent text never streams. A block that arrived in one burst must be
    /// byte-identical to one assembled from deltas — no flag, no marker, nothing to differ.
    #[test]
    fn a_message_that_never_streamed_matches_one_that_did() {
        let mut streamed = Transcript::new();
        feed(&mut streamed, vec![human("go"), delta("m", "Hel"), delta("m", "lo, "), delta("m", "world."), complete("m", "Hello, world.")]);

        let mut burst = Transcript::new();
        feed(&mut burst, vec![human("go"), complete("m", "Hello, world.")]);

        assert_eq!(streamed.len(), burst.len());
        for (a, b) in streamed.elements().iter().zip(burst.elements()) {
            assert_eq!(a, b, "a streamed block must be indistinguishable from a burst one");
        }
    }

    /// Behaviour 1 and 6 together: cards resolve where they sit, and the ids never move.
    #[test]
    fn a_tool_card_resolves_in_place() {
        let mut t = Transcript::new();
        feed(
            &mut t,
            vec![
                human("read it"),
                complete("m1", "I'll read the file."),
                call("t1", "Read", Some(r#"{"file_path":"a.rs"}"#)),
                complete("m2", "Here is what it says."),
            ],
        );
        let before = ids(&t);
        let card_id = t.elements()[2].id;
        assert!(t.elements()[2].tool().unwrap().state.is_running());

        assert_eq!(t.apply(result("t1", "fn main() {}")), Change::Updated(card_id));

        assert_eq!(ids(&t), before, "resolving must not move, add or renumber anything");
        assert_eq!(t.elements()[2].id, card_id);
        let card = t.elements()[2].tool().unwrap();
        assert_eq!(card.state.output(), Some("fn main() {}"));
        assert!(!card.state.is_running());
        assert_eq!(
            t.elements()[3].assistant().unwrap().text,
            "Here is what it says.",
            "text emitted after the call must still sit after it"
        );
    }

    /// Read-only tools are dispatched concurrently, so several ids are unresolved at once.
    /// Each resolves alone and the survivors keep their call order.
    #[test]
    fn several_tools_run_at_once_and_resolve_independently() {
        let mut t = Transcript::new();
        feed(
            &mut t,
            vec![
                human("survey"),
                call("a", "Read", Some("{}")),
                call("b", "Grep", Some("{}")),
                call("c", "Glob", Some("{}")),
            ],
        );
        let e = |i: usize| t.elements()[i].id;
        assert_eq!(t.running_tools(), &[e(1), e(2), e(3)][..]);
        assert!(t.is_working());

        t.apply(result("b", "hit"));
        assert_eq!(t.running_tools(), &[t.elements()[1].id, t.elements()[3].id][..], "call order survives a middle resolve");
        assert!(t.tool(&"b".into()).unwrap().state.output().is_some());
        assert!(t.tool(&"a".into()).unwrap().state.is_running());
        assert!(t.tool(&"c".into()).unwrap().state.is_running());

        t.apply(result("a", "hit"));
        t.apply(result("c", "hit"));
        assert!(t.running_tools().is_empty());
        assert!(!t.is_working());
    }

    /// Behaviour 3: `RunFinished` records an outcome, it does not close anything. A tool
    /// dispatched before the end resolves after it, in place, and the turn says so.
    #[test]
    fn a_result_does_not_close_the_transcript_to_later_events() {
        let mut t = Transcript::new();
        feed(&mut t, vec![human("go"), call("t1", "Bash", Some("{}")), finished()]);
        let turn = t.current_turn().unwrap().id;
        assert_eq!(t.turn(turn).unwrap().state, TurnState::Finished(RunOutcome::Ok));
        assert!(!t.turn(turn).unwrap().trailing);
        assert_eq!(t.running_tools().len(), 1, "a finished run does not invent evidence a tool ended");

        let card_id = t.elements()[1].id;
        assert_eq!(t.apply(result("t1", "done")), Change::Updated(card_id));
        assert!(t.running_tools().is_empty());
        assert!(t.turn(turn).unwrap().trailing, "the turn must own up to the late event");
        assert_eq!(t.stats().trailing_events, 1);

        // And more text after the end still lands, in order, in the same turn.
        t.apply(complete("m9", "one more thing"));
        assert_eq!(t.elements().back().unwrap().assistant().unwrap().text, "one more thing");
        assert_eq!(t.elements().back().unwrap().turn, turn);
        assert_eq!(t.len(), 4, "human, card, run-end, then the late text — appended, never reset");
        assert_eq!(t.stats().trailing_events, 2);
    }

    /// Behaviour 4: a fragment is text with a bit saying "not yet JSON". It never claims
    /// otherwise, and the authoritative input replaces it.
    #[test]
    fn argument_fragments_are_never_claimed_to_be_complete() {
        let mut t = Transcript::new();
        t.apply(call("t1", "Edit", None));
        assert_eq!(t.tool(&"t1".into()).unwrap().arguments, Arguments::pending());

        t.apply(AgentEvent::ToolArgumentsDelta { id: "t1".into(), fragment: r#"{"file"#.into() });
        let args = t.tool(&"t1".into()).unwrap().arguments.clone();
        assert_eq!(args.text, r#"{"file"#);
        assert!(!args.complete, "half a JSON document must never be marked complete");

        t.apply(call("t1", "Edit", Some(r#"{"file_path":"x"}"#)));
        let args = t.tool(&"t1".into()).unwrap().arguments.clone();
        assert_eq!(args.text, r#"{"file_path":"x"}"#, "the authoritative input replaces the fragments");
        assert!(args.complete);
        assert_eq!(t.len(), 1, "the card is opened once, not once per fragment");

        // A fragment arriving after that is noise and must not corrupt valid JSON.
        t.apply(AgentEvent::ToolArgumentsDelta { id: "t1".into(), fragment: "junk".into() });
        assert_eq!(t.tool(&"t1".into()).unwrap().arguments.text, r#"{"file_path":"x"}"#);
    }

    #[test]
    fn a_fragment_for_an_unknown_call_is_ignored_and_counted() {
        let mut t = Transcript::new();
        let c = t.apply(AgentEvent::ToolArgumentsDelta { id: "ghost".into(), fragment: "{".into() });
        assert_eq!(c, Change::Ignored(Ignored::UnknownToolFragment));
        assert!(t.is_empty());
        assert_eq!(t.stats().orphan_argument_fragments, 1);
    }

    /// A result with no call — a resumed session, a compaction boundary. The output is real
    /// content, so it is kept as a nameless card rather than discarded.
    #[test]
    fn an_orphan_result_is_kept_named_nothing_and_counted() {
        let mut t = Transcript::new();
        let c = t.apply(result("ghost", "output of something we never saw"));
        assert_eq!(c, Change::Appended(ElementId(0)));
        let card = t.elements()[0].tool().unwrap();
        assert_eq!(card.name, None);
        assert_eq!(card.state.output(), Some("output of something we never saw"));
        assert!(!card.state.is_running(), "an orphan is resolved on arrival, never running");
        assert!(t.running_tools().is_empty());
        assert_eq!(t.stats().orphan_results, 1);
    }

    /// **CONTRACT.** A result's structured detail lands on the card it resolves, and a card
    /// whose tool said nothing about itself carries no detail at all — the two states a
    /// reader has to be able to tell apart.
    #[test]
    fn a_results_detail_lands_on_the_card_it_resolves() {
        let mut t = Transcript::new();
        let detail = ResultDetail {
            file_path: Some("C:\\work\\demo\\fx-a.txt".into()),
            lines: Some(4),
            total_lines: Some(900),
            start_line: Some(1),
        };
        feed(
            &mut t,
            vec![
                call("t1", "Read", Some("{}")),
                result_with_detail("t1", "1\talpha\n", detail.clone()),
                call("t2", "Bash", Some("{}")),
                result("t2", "done"),
            ],
        );
        assert_eq!(t.tool(&"t1".into()).unwrap().detail, detail);
        assert!(
            t.tool(&"t2".into()).unwrap().detail.is_empty(),
            "a tool that reported nothing must not inherit the last one's numbers"
        );
    }

    /// **CONTRACT.** An orphan card keeps the detail, and this is the case where it matters
    /// most: with no call there are no arguments, so the detail's path is the only record of
    /// what the tool touched.
    #[test]
    fn an_orphan_card_keeps_the_detail_that_is_all_it_has() {
        let mut t = Transcript::new();
        let detail = ResultDetail { lines: Some(3), total_lines: Some(3), ..Default::default() };
        t.apply(result_with_detail("ghost", "output", detail.clone()));
        let card = t.elements()[0].tool().unwrap();
        assert_eq!(card.name, None, "still nameless");
        assert_eq!(card.detail, detail);
    }

    #[test]
    fn a_second_result_for_the_same_call_is_ignored() {
        let mut t = Transcript::new();
        feed(&mut t, vec![call("t1", "Read", Some("{}")), result("t1", "first")]);
        assert_eq!(t.apply(result("t1", "second")), Change::Ignored(Ignored::ToolAlreadyResolved));
        assert_eq!(t.tool(&"t1".into()).unwrap().state.output(), Some("first"));
        assert_eq!(t.stats().duplicate_results, 1);
        // And a late call for a resolved id cannot un-resolve it.
        assert_eq!(t.apply(call("t1", "Read", Some("{}"))), Change::Ignored(Ignored::ToolAlreadyResolved));
        assert_eq!(t.stats().duplicate_calls, 1);
        assert!(t.running_tools().is_empty());
    }

    #[test]
    fn a_tool_error_is_complete_and_flagged() {
        let mut t = Transcript::new();
        t.apply(call("t1", "Bash", Some("{}")));
        t.apply(AgentEvent::ToolResult {
            id: "t1".into(),
            output: "exit 1".into(),
            is_error: true,
            detail: ResultDetail::default(),
        });
        let card = t.tool(&"t1".into()).unwrap();
        assert!(card.state.is_error());
        assert!(!card.state.is_running());
        assert_eq!(card.state.output(), Some("exit 1"));
    }

    /// Turn grouping, including the case that makes it interesting: input queued while a
    /// tool is still out. The new turn opens; the old turn's card resolves where it is.
    #[test]
    fn human_input_opens_a_turn_and_old_cards_still_resolve_into_the_old_one() {
        let mut t = Transcript::new();
        feed(
            &mut t,
            vec![
                AgentEvent::SessionStarted { session_id: "s-1".into() },
                human("first"),
                call("t1", "Read", Some("{}")),
                human("second, queued"),
                complete("m1", "working on it"),
            ],
        );
        assert_eq!(t.session_id(), Some("s-1"));
        assert_eq!(t.turns().len(), 2, "init before the first human must not strand an empty turn");
        let (t0, t1) = (t.turns()[0].id, t.turns()[1].id);
        assert_eq!(t.elements()[0].turn, t0);
        assert_eq!(t.elements()[1].turn, t0);
        assert_eq!(t.elements()[2].turn, t1);
        assert_eq!(t.elements()[3].turn, t1);
        assert_eq!(t.turns()[0].session.as_deref(), Some("s-1"));

        let card_id = t.elements()[1].id;
        t.apply(result("t1", "contents"));
        assert_eq!(t.get(card_id).unwrap().turn, t0, "a card never migrates turns");
        assert_eq!(t.turns()[0].retained, 2);
        assert_eq!(t.turns()[1].retained, 2);
    }

    #[test]
    fn a_new_session_mid_transcript_opens_a_turn() {
        let mut t = Transcript::new();
        feed(&mut t, vec![human("a"), finished()]);
        t.apply(AgentEvent::SessionStarted { session_id: "s-2".into() });
        assert_eq!(t.turns().len(), 2);
        assert_eq!(t.current_turn().unwrap().state, TurnState::Open);
        assert_eq!(t.current_turn().unwrap().session.as_deref(), Some("s-2"));
    }

    /// The lookup invariant `get` depends on: every id issued is immediately appended, and
    /// eviction is front-only, so the retained ids are contiguous and ascending.
    #[test]
    fn element_ids_are_contiguous_over_the_retained_window() {
        let mut t = Transcript::with_limits(Limits { max_elements: 4, ..Limits::default() });
        for i in 0..20 {
            t.apply(complete(&format!("m{i}"), "x"));
            let seen = ids(&t);
            assert!(seen.windows(2).all(|w| w[1] == w[0] + 1), "ids must stay contiguous: {seen:?}");
            for e in t.elements() {
                assert_eq!(t.get(e.id).map(|x| x.id), Some(e.id));
            }
        }
        assert_eq!(t.get(ElementId(0)), None, "an evicted id resolves to nothing, not to a neighbour");
        assert_eq!(t.get(ElementId(999)), None);
    }

    /// The cap evicts from the front, says how much it dropped, and keeps every derived
    /// structure consistent with what is left.
    #[test]
    fn the_cap_evicts_the_oldest_and_reports_it() {
        let mut t = Transcript::with_limits(Limits { max_elements: 3, ..Limits::default() });
        assert_eq!(t.limits().max_elements, 3);
        feed(&mut t, vec![human("a"), complete("m1", "one"), call("t1", "Read", Some("{}"))]);
        assert_eq!(t.len(), 3);
        assert_eq!(t.stats().dropped_elements, 0);
        assert_eq!(t.running_tools().len(), 1);

        feed(&mut t, vec![complete("m2", "two"), complete("m3", "three")]);
        assert_eq!(t.len(), 3);
        assert_eq!(t.stats().dropped_elements, 2);
        assert_eq!(ids(&t), vec![2, 3, 4]);
        assert_eq!(t.running_tools(), &[ElementId(2)][..], "the surviving card is still running");
        assert!(t.tool(&"t1".into()).is_some());

        // Push the running card off the front: the running id goes with it, counted.
        t.apply(complete("m4", "four"));
        assert_eq!(t.stats().dropped_unresolved_tools, 1);
        assert!(t.running_tools().is_empty());
        assert!(t.tool(&"t1".into()).is_none(), "the correlation map must not outlive the card");
    }

    #[test]
    fn a_degenerate_cap_still_holds_the_newest_element() {
        let mut t = Transcript::with_limits(Limits { max_elements: 0, ..Limits::default() });
        feed(&mut t, vec![human("a"), human("b")]);
        assert_eq!(t.len(), 1);
        assert_eq!(t.elements()[0].human().unwrap().text, "b");
        assert_eq!(t.turns().len(), 1, "turns with nothing left are pruned, the newest is kept");
    }

    fn panel(title: &str) -> ArtifactBlock {
        panel_driving(title, ElementId(0))
    }

    fn panel_driving(title: &str, drives: ElementId) -> ArtifactBlock {
        ArtifactBlock {
            title: title.to_string(),
            content: ArtifactContent::Panel(PanelSpec {
                sliders: vec!["depth".into(), "bloom".into()],
                buttons: vec!["metal".into(), "glass".into()],
                drives,
            }),
        }
    }

    /// **The property the whole feature rests on**: an artifact is summoned once and then
    /// the stream keeps moving *around* it. Text streams in, a card resolves, a run ends —
    /// and the artifact keeps its index, its id and its contents, so a view holding widget
    /// state under that id finds the same element on the next frame.
    ///
    /// The failure this pins is the one that would be invisible in a screenshot and obvious
    /// in the hand: a slider that resets mid-sentence because the element it belongs to
    /// moved, was rebuilt, or was replaced by a same-keyed neighbour.
    #[test]
    fn an_artifact_holds_its_place_while_the_stream_moves_around_it() {
        let mut t = Transcript::new();
        feed(&mut t, vec![human("show me"), delta("m1", "here"), call("t1", "Read", Some("{}"))]);

        let change = t.insert_artifact(panel("◈ organon · panel"));
        let art_id = match change {
            Change::Appended(id) => id,
            other => panic!("an artifact must append: {other:?}"),
        };
        assert_eq!(art_id, ElementId(3));
        let turn = t.current_turn().unwrap().id;
        assert_eq!(t.get(art_id).unwrap().turn, turn, "it lands in the turn being written");
        assert_eq!(t.turn(turn).unwrap().retained, 4, "and is counted like any element");

        let before = ids(&t);
        let body = t.get(art_id).unwrap().body.clone();

        // Everything the stream can do to the elements on either side of it.
        feed(
            &mut t,
            vec![
                delta("m1", " is what I found"),
                complete("m1", "Here is what I found."),
                result("t1", "fn main() {}"),
                complete("m2", "…and one more thing."),
                finished(),
            ],
        );

        assert_eq!(&ids(&t)[..before.len()], &before[..], "nothing renumbered");
        assert_eq!(t.elements()[3].id, art_id, "the artifact did not move");
        assert_eq!(t.get(art_id).unwrap().body, body, "nor change under it");
        assert_eq!(t.get(art_id).unwrap().turn, turn, "nor migrate turns");
        assert_eq!(t.get(art_id).unwrap().artifact().unwrap().title, "◈ organon · panel");
        // The neighbours did all of the changing, which is what makes the check mean
        // something: an artifact that survived a *static* transcript would prove nothing.
        assert_eq!(t.elements()[1].assistant().unwrap().text, "Here is what I found.");
        assert!(!t.elements()[2].tool().unwrap().state.is_running());
        assert!(!t.is_working());
    }

    /// An artifact is an ordinary element to the cap: it evicts from the front, is counted,
    /// and its id then resolves to **nothing** rather than to a neighbour. That last part is
    /// the view's only truth source for dropping the widget state it kept under that id.
    #[test]
    fn an_artifact_evicts_like_anything_else_and_its_id_stops_resolving() {
        let mut t = Transcript::with_limits(Limits { max_elements: 3, ..Limits::default() });
        t.insert_artifact(panel("first"));
        let old = t.elements()[0].id;
        feed(&mut t, vec![human("a"), complete("m1", "b")]);
        assert_eq!(t.len(), 3);
        assert!(t.get(old).is_some(), "still retained at the cap");

        t.insert_artifact(panel("second"));
        assert_eq!(t.len(), 3);
        assert_eq!(t.stats().dropped_elements, 1);
        assert_eq!(t.get(old), None, "an evicted artifact resolves to nothing");
        assert_eq!(t.elements().back().unwrap().artifact().unwrap().title, "second");
        assert_eq!(
            t.elements().iter().filter(|e| e.artifact().is_some()).count(),
            1,
            "one in, one out — an artifact is not exempt from the cap"
        );
    }

    /// Two artifacts summoned in a row are two elements with two ids. They carry no
    /// correlation key of any sort, so nothing can make the second replace the first — the
    /// way a same-id assistant block replaces its predecessor.
    #[test]
    fn artifacts_never_replace_each_other() {
        let mut t = Transcript::new();
        t.insert_artifact(panel("one"));
        t.insert_artifact(panel("one"));
        assert_eq!(t.len(), 2, "identical descriptions are still two elements");
        assert_ne!(t.elements()[0].id, t.elements()[1].id);
        assert_eq!(t.stats().events, 0, "insertion is not an event and is not counted as one");
    }

    fn surface(look: &str) -> ArtifactBlock {
        ArtifactBlock {
            title: "◈ organon · surface".to_string(),
            content: ArtifactContent::Surface(SurfaceSpec { look: look.to_string() }),
        }
    }

    fn driver(target: ElementId) -> ArtifactBlock {
        ArtifactBlock {
            title: "◈ organon · console".to_string(),
            content: ArtifactContent::Panel(PanelSpec {
                sliders: vec!["light".into()],
                buttons: vec!["metal".into()],
                drives: target,
            }),
        }
    }

    /// **The pairing the whole feature rests on**: a surface and the panel that drives it are
    /// two elements, and the link between them is an id the transcript guarantees — not an
    /// index, and not "the one above". The stream then moves around both, and the link still
    /// resolves to the same element.
    #[test]
    fn a_panel_keeps_pointing_at_its_surface_while_the_stream_moves() {
        let mut t = Transcript::new();
        let Change::Appended(surface_id) = t.insert_artifact(surface("slate")) else {
            panic!("an artifact must append");
        };
        t.insert_artifact(driver(surface_id));

        feed(
            &mut t,
            vec![
                human("go"),
                delta("m1", "th"),
                complete("m1", "thinking"),
                call("t1", "Bash", Some("{}")),
                result("t1", "done"),
                finished(),
            ],
        );

        let panel = t
            .elements()
            .iter()
            .find_map(|e| match &e.body {
                Body::Artifact(a) => match &a.content {
                    ArtifactContent::Panel(p) => Some(p.clone()),
                    _ => None,
                },
                _ => None,
            })
            .expect("the panel is still there");
        assert_eq!(panel.drives, surface_id);
        let target = t.get(surface_id).expect("the target still resolves");
        match &target.artifact().unwrap().content {
            ArtifactContent::Surface(s) => assert_eq!(s.look, "slate"),
            other => panic!("the id resolved to the wrong kind: {other:?}"),
        }
    }

    /// The failure mode a bare index would hide: the cap evicts the surface, and the panel's
    /// target stops resolving **to anything** rather than resolving to whatever slid into
    /// that position. The view's only honest reading is "this panel drives nothing now".
    #[test]
    fn an_evicted_surface_leaves_its_panel_driving_nothing() {
        let mut t = Transcript::with_limits(Limits { max_elements: 2, ..Limits::default() });
        let Change::Appended(surface_id) = t.insert_artifact(surface("graphite")) else {
            panic!("an artifact must append");
        };
        t.insert_artifact(driver(surface_id));
        assert!(t.get(surface_id).is_some(), "still retained at the cap");

        feed(&mut t, vec![human("one more")]);
        assert_eq!(t.stats().dropped_elements, 1);
        assert_eq!(t.get(surface_id), None, "the target is gone, not reassigned");
    }

    fn pending_approval(tool: &str) -> ApprovalBlock {
        ApprovalBlock {
            tool_name: tool.to_string(),
            input: r#"{"command":"cargo test"}"#.to_string(),
            tool_use_id: "toolu_1".to_string(),
            state: ApprovalState::Pending,
        }
    }

    fn allowed() -> Answer {
        Answer { verdict: Verdict::Allow, from_memory: false, remembered: false }
    }

    /// **The state machine a blocked agent depends on.** A question is pending exactly
    /// until it is answered, it is answered exactly once, and answering does not move it.
    #[test]
    fn an_approval_is_pending_until_it_is_answered_exactly_once() {
        let mut t = Transcript::new();
        feed(&mut t, vec![human("build it"), complete("m1", "I'll run the build.")]);

        let Change::Appended(id) = t.insert_approval(pending_approval("Bash")) else {
            panic!("an approval must append");
        };
        let before = ids(&t);
        assert_eq!(t.pending_approvals(), &[id][..]);
        assert!(t.is_waiting());
        assert_eq!(t.get(id).unwrap().approval().unwrap().state, ApprovalState::Pending);
        assert_eq!(t.get(id).unwrap().approval().unwrap().tool_use_id, "toolu_1");
        assert_eq!(t.stats().events, 2, "an insertion is not an event and is not counted as one");

        let answer = Answer { verdict: Verdict::Allow, from_memory: false, remembered: true };
        assert_eq!(t.answer_approval(id, answer), Change::Updated(id));
        assert_eq!(ids(&t), before, "answering must not move, add or renumber anything");
        assert!(t.pending_approvals().is_empty());
        assert!(!t.is_waiting());
        assert_eq!(t.get(id).unwrap().approval().unwrap().state, ApprovalState::Answered(answer));

        // A second answer changes nothing: the agent was unblocked by the first.
        let flipped = Answer { verdict: Verdict::Deny, from_memory: false, remembered: false };
        assert_eq!(
            t.answer_approval(id, flipped),
            Change::Ignored(Ignored::ApprovalAlreadyAnswered)
        );
        assert_eq!(t.get(id).unwrap().approval().unwrap().state.answer().unwrap().verdict, Verdict::Allow);
        assert_eq!(t.stats().duplicate_answers, 1);

        // …and neither does answering something that is not an approval, or is not there.
        let prose = t.elements()[1].id;
        assert_eq!(
            t.answer_approval(prose, allowed()),
            Change::Ignored(Ignored::ApprovalAlreadyAnswered)
        );
        assert_eq!(
            t.answer_approval(ElementId(999), allowed()),
            Change::Ignored(Ignored::ApprovalAlreadyAnswered)
        );
    }

    /// An approval the memory answered for the human is **still an element**. A decision
    /// nobody can see is worse than being asked every time — so it renders, and it is
    /// simply never pending.
    #[test]
    fn an_approval_answered_from_memory_is_visible_and_never_pending() {
        let mut t = Transcript::new();
        let remembered = Answer { verdict: Verdict::Allow, from_memory: true, remembered: true };
        let Change::Appended(id) = t.insert_approval(ApprovalBlock {
            state: ApprovalState::Answered(remembered),
            ..pending_approval("Bash")
        }) else {
            panic!("an approval must append");
        };
        assert_eq!(t.len(), 1, "it is drawn, not swallowed");
        assert!(t.pending_approvals().is_empty(), "nothing is waiting on it");
        assert!(!t.is_waiting());
        assert_eq!(t.get(id).unwrap().approval().unwrap().state.answer(), Some(remembered));
    }

    /// Revocation: the promise is withdrawn, the history is not rewritten.
    #[test]
    fn revoking_clears_the_promise_and_keeps_the_verdict() {
        let mut t = Transcript::new();
        let Change::Appended(id) = t.insert_approval(pending_approval("Bash")) else {
            panic!("an approval must append");
        };
        t.answer_approval(id, Answer { verdict: Verdict::Allow, from_memory: false, remembered: true });

        assert_eq!(t.revoke_approval(id), Change::Updated(id));
        let answer = t.get(id).unwrap().approval().unwrap().state.answer().unwrap();
        assert_eq!(answer.verdict, Verdict::Allow, "what happened, happened");
        assert!(!answer.remembered, "the promise to answer again is gone");

        // Nothing left to revoke, and nothing else is revocable.
        assert_eq!(t.revoke_approval(id), Change::Ignored(Ignored::ApprovalAlreadyAnswered));
        let mut fresh = Transcript::new();
        let Change::Appended(open) = fresh.insert_approval(pending_approval("Bash")) else {
            panic!("append");
        };
        assert_eq!(fresh.revoke_approval(open), Change::Ignored(Ignored::ApprovalAlreadyAnswered));
        assert_eq!(fresh.revoke_approval(ElementId(999)), Change::Ignored(Ignored::ApprovalAlreadyAnswered));
    }

    /// 🚨 **A question whose asker has gone stops asking, and says what happened.**
    ///
    /// The failure this closes was on screen: the client gave up on a permission call after
    /// a minute, the tool failed with *"The operation timed out"*, and the card kept
    /// offering allow / allow-and-remember / deny for a call that no longer existed.
    #[test]
    fn an_abandoned_approval_stops_asking_and_is_not_a_verdict() {
        let mut t = Transcript::new();
        let Change::Appended(id) = t.insert_approval(pending_approval("Write")) else {
            panic!("an approval must append");
        };
        assert_eq!(t.pending_approvals(), &[id][..]);

        assert_eq!(t.abandon_approval(id), Change::Updated(id));
        let state = t.get(id).unwrap().approval().unwrap().state;
        assert_eq!(state, ApprovalState::Abandoned);
        assert!(!state.is_pending(), "nothing is waiting on it");
        assert_eq!(state.answer(), None, "and nobody decided it — it is not a deny");
        assert!(t.pending_approvals().is_empty());

        // It is spent in both directions: a late click cannot answer it, and sweeping it
        // twice is a no-op rather than a second event.
        assert_eq!(
            t.answer_approval(id, allowed()),
            Change::Ignored(Ignored::ApprovalAlreadyAnswered)
        );
        assert_eq!(t.get(id).unwrap().approval().unwrap().state, ApprovalState::Abandoned);
        assert_eq!(
            t.abandon_approval(id),
            Change::Ignored(Ignored::ApprovalAlreadyAnswered)
        );

        // An **answered** question is never re-opened by a sweep, and neither is an element
        // that is not an approval at all.
        let Change::Appended(other) = t.insert_approval(pending_approval("Bash")) else {
            panic!("an approval must append");
        };
        t.answer_approval(other, allowed());
        assert_eq!(
            t.abandon_approval(other),
            Change::Ignored(Ignored::ApprovalAlreadyAnswered)
        );
        assert_eq!(t.get(other).unwrap().approval().unwrap().state.answer(), Some(allowed()));
        assert_eq!(
            t.abandon_approval(ElementId(999)),
            Change::Ignored(Ignored::ApprovalAlreadyAnswered)
        );
    }

    /// ⚠️ The eviction with a consequence outside this module. The cap takes a question a
    /// human never answered; the count is how the pane knows to fail it closed instead of
    /// leaving an agent blocked for the rest of the session.
    #[test]
    fn an_evicted_pending_approval_stops_being_pending_and_is_counted_on_its_own() {
        let mut t = Transcript::with_limits(Limits { max_elements: 2, ..Limits::default() });
        let Change::Appended(id) = t.insert_approval(pending_approval("Bash")) else {
            panic!("append");
        };
        feed(&mut t, vec![human("hurry up")]);
        assert_eq!(t.pending_approvals(), &[id][..], "still retained at the cap");
        assert_eq!(t.stats().dropped_pending_approvals, 0);

        feed(&mut t, vec![complete("m1", "pushing it off the front")]);
        assert_eq!(t.get(id), None, "evicted, not reassigned");
        assert!(t.pending_approvals().is_empty());
        assert_eq!(t.stats().dropped_pending_approvals, 1);
        assert_eq!(t.stats().dropped_elements, 1);

        // An *answered* approval evicts like anything else, with no special count — there
        // is nobody waiting on it.
        let mut t = Transcript::with_limits(Limits { max_elements: 1, ..Limits::default() });
        let Change::Appended(id) = t.insert_approval(pending_approval("Bash")) else {
            panic!("append");
        };
        t.answer_approval(id, allowed());
        feed(&mut t, vec![human("next")]);
        assert_eq!(t.stats().dropped_pending_approvals, 0);
        assert_eq!(t.stats().dropped_elements, 1);
    }

    /// Deterministic, dependency-free — the `scroll_anchor` precedent; there is no new
    /// dev-dependency in this crate for a sweep.
    struct Lcg(u64);

    impl Lcg {
        fn new(seed: u64) -> Self {
            Lcg(seed.wrapping_mul(2862933555777941757).wrapping_add(3037000493))
        }
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }
    }

    /// The sweep: every event kind interleaved, including the pathological orderings, with
    /// the structural invariants checked after **every** apply.
    #[test]
    fn interleaved_streams_keep_every_invariant() {
        for (seed, cap) in [(1u64, 8usize), (2, 64), (3, 10_000), (4, 3)] {
            let mut rng = Lcg::new(seed);
            let mut t = Transcript::with_limits(Limits { max_elements: cap, ..Limits::default() });
            // Ids drawn from a small pool on purpose, so duplicates, orphans and
            // out-of-order correlations all actually happen.
            let pool = 6u64;
            let mut kinds: HashMap<u64, &'static str> = HashMap::new();

            for step in 0..600 {
                let n = rng.below(pool);
                match rng.below(10) {
                    0 => {
                        t.apply(AgentEvent::SessionStarted { session_id: format!("s{n}") });
                    }
                    1 => {
                        t.apply(human(&format!("ask {step}")));
                    }
                    2 | 3 => {
                        t.apply(delta(&format!("m{n}"), "frag "));
                    }
                    4 => {
                        t.apply(complete(&format!("m{n}"), &format!("final {step}")));
                    }
                    5 | 6 => {
                        t.apply(call(&format!("c{n}"), "Read", if step % 3 == 0 { Some("{}") } else { None }));
                    }
                    7 => {
                        t.apply(AgentEvent::ToolArgumentsDelta {
                            id: format!("c{n}").into(),
                            fragment: "\"x\"".into(),
                        });
                    }
                    8 => {
                        t.apply(AgentEvent::ToolResult {
                            id: format!("c{n}").into(),
                            output: format!("out {step}"),
                            is_error: step % 5 == 0,
                            detail: ResultDetail::default(),
                        });
                    }
                    _ => {
                        t.apply(AgentEvent::RunFinished {
                            outcome: match rng.below(3) {
                                0 => RunOutcome::Ok,
                                1 => RunOutcome::Error,
                                _ => RunOutcome::Cancelled,
                            },
                            detail: None,
                        });
                    }
                }

                // An artifact, occasionally, so every invariant below sweeps the element the
                // console inserts itself and not only the ones a harness produces.
                if step % 37 == 0 {
                    t.insert_artifact(ArtifactBlock {
                        title: format!("panel {step}"),
                        content: ArtifactContent::Panel(PanelSpec {
                            sliders: vec![],
                            buttons: vec![],
                            drives: ElementId(0),
                        }),
                    });
                }
                // …and an approval, on a different stride, answered a few steps later —
                // so the pending list is swept against the same invariants as `running`,
                // including being evicted mid-question at the small caps.
                if step % 23 == 0 {
                    t.insert_approval(pending_approval(&format!("Bash{n}")));
                }
                if step % 31 == 0 {
                    if let Some(oldest) = t.pending_approvals().first().copied() {
                        t.answer_approval(oldest, allowed());
                    }
                }

                let ctx = format!("seed {seed} cap {cap} step {step}");

                // 1. Order and identity: ids ascending, contiguous, addressable.
                let seen = ids(&t);
                assert!(seen.windows(2).all(|w| w[1] == w[0] + 1), "{ctx}: ids broke contiguity: {seen:?}");
                assert!(t.len() <= cap.max(1), "{ctx}: the cap was exceeded");
                for e in t.elements() {
                    assert_eq!(t.get(e.id).map(|x| x.id), Some(e.id), "{ctx}: get() disagrees with the deque");
                    // 2. An element never changes what kind of thing it is.
                    let kind = match &e.body {
                        Body::Human(_) => "human",
                        Body::Assistant(_) => "assistant",
                        Body::Tool(_) => "tool",
                        Body::RunEnd(_) => "run_end",
                        Body::Artifact(_) => "artifact",
                        Body::Approval(_) => "approval",
                    };
                    let was = kinds.entry(e.id.0).or_insert(kind);
                    assert_eq!(*was, kind, "{ctx}: element {} changed kind", e.id.0);
                }

                // 3a. `pending` is exactly the retained unanswered approvals, in order.
                let unanswered: Vec<ElementId> = t
                    .elements()
                    .iter()
                    .filter(|e| e.approval().map(|a| a.state.is_pending()).unwrap_or(false))
                    .map(|e| e.id)
                    .collect();
                assert_eq!(t.pending_approvals(), &unanswered[..], "{ctx}: pending list drifted");
                assert_eq!(t.is_waiting(), !unanswered.is_empty(), "{ctx}");

                // 3. `running` is exactly the retained unresolved cards, in call order.
                let derived: Vec<ElementId> = t
                    .elements()
                    .iter()
                    .filter(|e| e.tool().map(|c| c.state.is_running()).unwrap_or(false))
                    .map(|e| e.id)
                    .collect();
                assert_eq!(t.running_tools(), &derived[..], "{ctx}: running list drifted from the cards");
                assert_eq!(t.is_working(), !derived.is_empty(), "{ctx}");

                // 4. Correlation maps only ever point at live elements of the right kind.
                for e in t.elements() {
                    if let Some(c) = e.tool() {
                        if let Some(found) = t.tool(&c.call_id) {
                            assert_eq!(found.call_id, c.call_id, "{ctx}");
                        }
                    }
                }

                // 5. Turn bookkeeping: counts match, only the newest may be empty, and
                //    every element's turn is either retained or legitimately pruned.
                let mut counted: HashMap<u64, usize> = HashMap::new();
                for e in t.elements() {
                    *counted.entry(e.turn.0).or_default() += 1;
                }
                for (i, turn) in t.turns().iter().enumerate() {
                    let live = counted.get(&turn.id.0).copied().unwrap_or(0);
                    assert_eq!(turn.retained, live, "{ctx}: turn {} miscounted", turn.id.0);
                    if i + 1 < t.turns().len() {
                        assert!(turn.retained > 0, "{ctx}: an empty non-newest turn survived");
                    }
                }
                let turn_ids: Vec<u64> = t.turns().iter().map(|x| x.id.0).collect();
                assert!(turn_ids.windows(2).all(|w| w[1] == w[0] + 1), "{ctx}: turn ids broke contiguity");
                for e in t.elements() {
                    assert!(t.turn(e.turn).is_some(), "{ctx}: element {} lost its turn", e.id.0);
                }

                // 6. Nothing is ever both provisional and authoritative.
                for e in t.elements() {
                    if let Some(c) = e.tool() {
                        if c.state.is_running() {
                            assert!(t.running_tools().contains(&e.id), "{ctx}");
                        }
                    }
                }
            }
            assert!(!t.is_empty(), "seed {seed}: the sweep produced no elements");
            assert!(t.stats().events == 600, "seed {seed}: every event must be accounted for");
        }
    }

    /// 🚨 The shape a **persistent session** actually has, measured live on this machine:
    /// a run-finished event arrives **per turn, not per session** — two of them in one
    /// stream, one `session_id` throughout, and the second human message echoed back into
    /// the same ordered stream rather than spliced in locally.
    ///
    /// So "finished" means *this exchange* ended. The transcript must stay one continuous
    /// ordered thing across the boundary: no reset, no second transcript, ids continuing
    /// straight through, and the second turn `Open` again rather than inheriting the
    /// first's outcome.
    #[test]
    fn two_exchanges_in_one_session_are_one_continuous_transcript() {
        let mut t = Transcript::new();
        feed(
            &mut t,
            vec![
                AgentEvent::SessionStarted { session_id: "same".into() },
                human("first question"),
                complete("m1", "first answer"),
                finished(),
                // …25 seconds later, into the same process, echoed back as an event.
                human("second question"),
                call("t2", "Read", Some("{}")),
                result("t2", "contents"),
                complete("m2", "second answer"),
                AgentEvent::RunFinished { outcome: RunOutcome::Error, detail: Some("oops".into()) },
            ],
        );

        assert_eq!(ids(&t), vec![0, 1, 2, 3, 4, 5, 6], "one flow, ids straight through");
        assert_eq!(t.session_id(), Some("same"), "one session across both exchanges");
        assert_eq!(t.turns().len(), 2);

        let (a, b) = (t.turns()[0].id, t.turns()[1].id);
        assert_eq!(t.turns()[0].state, TurnState::Finished(RunOutcome::Ok));
        assert_eq!(t.turns()[1].state, TurnState::Finished(RunOutcome::Error));
        assert!(!t.turns()[0].trailing, "a new exchange is not a trailing event on the old one");
        assert!(!t.turns()[1].trailing);
        assert_eq!(
            t.elements().iter().map(|e| e.turn).collect::<Vec<_>>(),
            vec![a, a, a, b, b, b, b],
            "grouping splits at the human's next message, not at the finish"
        );
        assert_eq!(t.turns()[1].session.as_deref(), Some("same"));
        assert_eq!(t.elements()[6].run_end().unwrap().detail.as_deref(), Some("oops"));
        assert!(!t.is_working());

        // The second exchange's turn was Open while it ran — a finish does not leak forward.
        let mut mid = Transcript::new();
        feed(&mut mid, vec![human("q"), finished(), human("q2")]);
        assert_eq!(mid.current_turn().unwrap().state, TurnState::Open);
        assert_eq!(mid.stats().trailing_events, 0);
    }

    /// A whole realistic turn, end to end — the shape §5.9.1 measured on this machine:
    /// `system/init` → text → `tool_use` → `tool_result` → text → `result`.
    #[test]
    fn a_measured_claude_code_turn_folds_into_five_elements() {
        let mut t = Transcript::new();
        feed(
            &mut t,
            vec![
                AgentEvent::SessionStarted { session_id: "abc".into() },
                human("what is in a.rs?"),
                delta("m1", "I'll "),
                delta("m1", "read it."),
                complete("m1", "I'll read it."),
                call("tu_1", "Read", None),
                AgentEvent::ToolArgumentsDelta { id: "tu_1".into(), fragment: r#"{"file_pa"#.into() },
                call("tu_1", "Read", Some(r#"{"file_path":"a.rs"}"#)),
                result("tu_1", "fn main() {}"),
                complete("m2", "It defines `main`."),
                finished(),
            ],
        );

        assert_eq!(t.len(), 5);
        assert_eq!(t.elements()[0].human().unwrap().text, "what is in a.rs?");
        assert_eq!(t.elements()[1].assistant().unwrap().text, "I'll read it.");
        let card = t.elements()[2].tool().unwrap();
        assert_eq!(card.name.as_deref(), Some("Read"));
        assert_eq!(card.arguments.text, r#"{"file_path":"a.rs"}"#);
        assert!(card.arguments.complete);
        assert_eq!(card.state.output(), Some("fn main() {}"));
        assert_eq!(t.elements()[3].assistant().unwrap().text, "It defines `main`.");
        assert_eq!(t.elements()[4].run_end().unwrap().outcome, RunOutcome::Ok);

        assert!(!t.is_working());
        assert_eq!(t.turns().len(), 1);
        assert_eq!(t.current_turn().unwrap().state, TurnState::Finished(RunOutcome::Ok));
        assert!(!t.current_turn().unwrap().trailing);
        assert_eq!(t.stats().dropped_elements, 0);
        assert_eq!(t.stats().orphan_results, 0);
    }

    // -- behaviour 6: a subagent belongs to the card that spawned it --------------

    fn said(parent: &str, text: &str) -> AgentEvent {
        AgentEvent::SubagentActivity {
            parent: parent.into(),
            activity: Subagent::Said(text.to_string()),
        }
    }

    fn used(parent: &str, id: &str, name: &str) -> AgentEvent {
        AgentEvent::SubagentActivity {
            parent: parent.into(),
            activity: Subagent::Used { id: id.into(), name: name.to_string() },
        }
    }

    fn returned(parent: &str, id: &str, is_error: bool) -> AgentEvent {
        AgentEvent::SubagentActivity {
            parent: parent.into(),
            activity: Subagent::Returned { id: id.into(), is_error },
        }
    }

    fn log_of(t: &Transcript, call: &str) -> SubagentLog {
        t.tool(&call.into()).expect("a card for that call").subagent.clone()
    }

    /// 🚨 CONTRACT — the whole feature in one test. A subagent's work lands **inside** the
    /// tool card that spawned it and appends **nothing** to the flow. Before this, a
    /// coordinator that dispatched agents showed a spinner and then a wall of text; the
    /// events existed all along and had nowhere to go that was not a turn belonging to
    /// nobody (§5.9.3 rule 5).
    #[test]
    fn a_subagents_work_lands_inside_its_card_and_appends_no_element() {
        let mut t = Transcript::new();
        feed(&mut t, vec![human("find the callers"), call("tu_task", "Task", None)]);
        let before = t.len();
        feed(
            &mut t,
            vec![
                said("tu_task", "Searching the tree."),
                used("tu_task", "tu_grep", "Grep"),
                returned("tu_task", "tu_grep", false),
            ],
        );
        assert_eq!(t.len(), before, "a subagent must never append an element of its own");
        let log = log_of(&t, "tu_task");
        assert_eq!(log.len(), 2, "one `Said` and one tool, not one per event: {:?}", log.steps);
        assert_eq!(log.steps[0].act, SubagentAct::Said("Searching the tree.".into()));
        assert_eq!(
            log.steps[1].act,
            SubagentAct::Tool {
                id: "tu_grep".into(),
                name: Some("Grep".into()),
                state: StepState::Done { is_error: false },
            },
            "the return resolved the call in place"
        );
        assert!(log.steps.iter().all(|s| s.depth == 1));
    }

    /// A step landing in a card already in the flow reports `Updated`, never `Appended` —
    /// the view re-arms its scroll-follow on the latter, and a reader who has scrolled up
    /// to read something must not be yanked to the bottom because a subagent spoke.
    #[test]
    fn a_step_on_an_existing_card_reports_updated_not_appended() {
        let mut t = Transcript::new();
        feed(&mut t, vec![call("tu_task", "Task", None)]);
        let change = t.apply(said("tu_task", "still going"));
        let card = t.tool(&"tu_task".into()).expect("the card");
        assert!(
            matches!(change, Change::Updated(_)),
            "a subagent step is not new content in the flow: {change:?}"
        );
        assert_eq!(card.subagent.len(), 1);
    }

    /// ⚠️ CONTRACT — depth 2+ **flattens onto the top-level card**, carrying the depth it
    /// was produced at. A subagent can dispatch its own, and nesting cards inside cards in
    /// a scrollback has no bottom; recording the number instead keeps the fact without the
    /// hazard, so a view can say a step came from two levels down rather than implying it
    /// was direct.
    #[test]
    fn a_subagent_that_dispatches_its_own_still_reports_to_the_top_level_card() {
        let mut t = Transcript::new();
        feed(
            &mut t,
            vec![
                call("tu_task", "Task", None),
                // The depth-1 agent dispatches its own Task…
                used("tu_task", "tu_inner_task", "Task"),
                // …and the depth-2 agent's lines name *that* call as their parent.
                said("tu_inner_task", "reading the file"),
                used("tu_inner_task", "tu_read", "Read"),
                returned("tu_inner_task", "tu_read", true),
            ],
        );
        assert_eq!(t.len(), 1, "one card, however deep the chain: {:?}", t.elements());
        let log = log_of(&t, "tu_task");
        assert_eq!(log.max_depth(), Some(2), "the depth is recorded, not discarded");
        let deep: Vec<u8> = log.steps.iter().map(|s| s.depth).collect();
        assert_eq!(deep, vec![1, 2, 2], "the inner Task is depth 1; what it did is depth 2");
        assert!(
            matches!(
                &log.steps[2].act,
                SubagentAct::Tool { name: Some(n), state, .. }
                    if n == "Read" && state.is_error()
            ),
            "the depth-2 tool resolved on the top-level card: {:?}",
            log.steps[2]
        );
    }

    /// ⚠️ The chain is followed but not indefinitely. Past [`MAX_TRACKED_DEPTH`] the
    /// ownership map stops growing, so a pathological nest cannot expand it without end —
    /// and the recorded depth saturates rather than overstating a chain that stopped being
    /// followed. Everything still lands on the one visible card, which is the property that
    /// must not degrade.
    #[test]
    fn a_chain_deeper_than_the_tracked_limit_still_lands_on_the_one_card() {
        let mut t = Transcript::new();
        feed(&mut t, vec![call("tu_task", "Task", None)]);
        let mut parent = "tu_task".to_string();
        for level in 0..(MAX_TRACKED_DEPTH as usize + 4) {
            let child = format!("tu_nest_{level}");
            t.apply(used(&parent, &child, "Task"));
            parent = child;
        }
        assert_eq!(t.len(), 1, "still one card: {:?}", t.elements());
        let log = log_of(&t, "tu_task");
        assert!(
            log.steps.iter().all(|s| s.depth <= MAX_TRACKED_DEPTH),
            "a depth was recorded past what was actually followed: {:?}",
            log.steps.iter().map(|s| s.depth).collect::<Vec<_>>()
        );
        assert_eq!(t.stats().orphan_subagent_activity, 0, "nothing fell off the chain");
    }

    /// 🚨 CONTRACT — an **orphan**: activity whose parent call we never saw. Follows
    /// `orphan_results` exactly — the content is real, so it is kept on a nameless card
    /// rather than dropped — and is counted separately, because only one of the two is
    /// evidence the parent finished.
    ///
    /// ⚠️ It opens `Running` and joins the running set. That is behaviour 1's derivation,
    /// not an invention: live activity from inside a call is the strongest available
    /// evidence the call has not returned. Opening it `Complete` would fabricate the result
    /// behaviour 3 refuses to fabricate.
    #[test]
    fn activity_for_a_call_we_never_saw_is_kept_on_a_nameless_card() {
        let mut t = Transcript::new();
        let change = t.apply(said("tu_ghost", "I am working"));
        assert!(matches!(change, Change::Appended(_)), "it really is new content: {change:?}");
        assert_eq!(t.stats().orphan_subagent_activity, 1, "counted, not silent");
        let card = t.tool(&"tu_ghost".into()).expect("an orphan card");
        assert!(card.name.is_none(), "there is no name to show — the call was never seen");
        assert!(card.state.is_running());
        assert_eq!(said_texts(&card.subagent), vec!["I am working"], "the content survived");
        assert!(t.is_working(), "something inside it is demonstrably still going");
    }

    /// …and the orphan resolves normally when its parent's result finally arrives. The card
    /// was nameless, not broken.
    #[test]
    fn an_orphaned_parent_still_resolves_when_its_result_arrives() {
        let mut t = Transcript::new();
        feed(&mut t, vec![said("tu_ghost", "working"), result("tu_ghost", "found three")]);
        let card = t.tool(&"tu_ghost".into()).expect("the card");
        assert_eq!(card.state.output(), Some("found three"));
        assert!(!t.is_working(), "the running claim was retracted by evidence, as any card's is");
        assert_eq!(t.len(), 1, "the result landed on the orphan card, not beside it");
    }

    /// 🚨 CONTRACT — the question "what if the parent card has scrolled away". Scrolling
    /// alone changes nothing: the log is *inside* the element, so it scrolls with it and
    /// there is no floating overlay to keep alive. **Eviction** is the real case, and it
    /// degrades to the orphan path rather than losing the work — the subagent keeps
    /// reporting and its activity opens a fresh nameless card at the bottom.
    #[test]
    fn a_subagent_whose_card_was_evicted_falls_back_to_an_orphan_card() {
        let mut t = Transcript::with_limits(Limits { max_elements: 2, ..Limits::default() });
        feed(&mut t, vec![call("tu_task", "Task", None), said("tu_task", "one")]);
        assert_eq!(log_of(&t, "tu_task").len(), 1, "the log is on the card while it is here");
        // Two more elements push the Task card out of the window.
        feed(&mut t, vec![human("something else"), human("and again")]);
        assert!(t.tool(&"tu_task".into()).is_none(), "the card is gone");
        assert_eq!(t.stats().dropped_elements, 1, "the card itself, and nothing else yet");
        // The subagent has not stopped working just because we stopped retaining its card.
        t.apply(said("tu_task", "two"));
        let card = t.tool(&"tu_task".into()).expect("a fresh orphan card");
        assert!(card.name.is_none(), "the name went with the evicted card");
        assert_eq!(said_texts(&card.subagent), vec!["two"], "later work is not lost");
        assert_eq!(t.stats().orphan_subagent_activity, 1);
    }

    /// A return naming a step this log has no call for is kept as a nameless step and
    /// counted — the same keep-it-anyway rule an orphan card follows, one level in. Its own
    /// counter, so a correlation that quietly stops working cannot hide behind a log that
    /// still looks busy.
    #[test]
    fn a_return_with_no_matching_call_is_kept_and_counted() {
        let mut t = Transcript::new();
        feed(&mut t, vec![call("tu_task", "Task", None), returned("tu_task", "tu_gone", true)]);
        let log = log_of(&t, "tu_task");
        assert_eq!(t.stats().unmatched_subagent_returns, 1);
        assert_eq!(
            log.steps[0].act,
            SubagentAct::Tool {
                id: "tu_gone".into(),
                name: None,
                state: StepState::Done { is_error: true },
            },
            "kept without a name rather than dropped"
        );
    }

    /// ⚠️ A log is capped **per card** and evicts from the front, and says so on the log
    /// itself — a trace that silently starts in the middle reads as the whole trace. The
    /// cap is separate from `max_elements` because one `Task` must not be able to evict the
    /// conversation around it by working hard.
    #[test]
    fn a_long_running_subagent_caps_its_log_and_reports_what_it_dropped() {
        let mut t =
            Transcript::with_limits(Limits { max_elements: 100, max_subagent_steps: 3 });
        feed(&mut t, vec![call("tu_task", "Task", None)]);
        for i in 0..10 {
            t.apply(said("tu_task", &format!("step {i}")));
        }
        assert_eq!(t.len(), 1, "the flow did not grow — only the log inside one card did");
        let log = log_of(&t, "tu_task");
        assert_eq!(log.len(), 3, "capped");
        assert_eq!(log.dropped, 7, "and it says how many it is not showing");
        assert_eq!(
            said_texts(&log),
            vec!["step 7", "step 8", "step 9"],
            "the tail is kept: the question a running card answers is what it is doing now"
        );
        assert_eq!(t.stats().dropped_subagent_steps, 7);
    }

    /// `unreturned` counts open tool steps and is **not** a liveness signal. A subagent
    /// that stopped mid-tool leaves one standing forever, exactly as an abandoned call does
    /// in the main flow — nothing on the wire ever retracts it, and only the parent card
    /// being unresolved says the subagent is working.
    #[test]
    fn an_unreturned_step_survives_the_parent_finishing() {
        let mut t = Transcript::new();
        feed(
            &mut t,
            vec![
                call("tu_task", "Task", None),
                used("tu_task", "tu_a", "Read"),
                used("tu_task", "tu_b", "Grep"),
                returned("tu_task", "tu_a", false),
                result("tu_task", "done, mostly"),
            ],
        );
        let card = t.tool(&"tu_task".into()).expect("the card");
        assert!(!card.state.is_running(), "the parent came back");
        assert_eq!(card.subagent.unreturned(), 1, "and one of its steps never did");
        assert!(!t.is_working(), "which is not the same as the transcript still working");
    }

    /// Two subagents under two different cards do not mix. Obvious, and the correlation is
    /// the only thing keeping them apart, so it is pinned.
    #[test]
    fn two_dispatched_agents_keep_their_own_logs() {
        let mut t = Transcript::new();
        feed(
            &mut t,
            vec![
                call("tu_one", "Task", None),
                call("tu_two", "Task", None),
                said("tu_one", "first agent"),
                said("tu_two", "second agent"),
                said("tu_one", "first again"),
            ],
        );
        assert_eq!(said_texts(&log_of(&t, "tu_one")), vec!["first agent", "first again"]);
        assert_eq!(said_texts(&log_of(&t, "tu_two")), vec!["second agent"]);
    }

    /// The ownership map must not outlive the card it points at, or it is the one map here
    /// that grows for the life of the process — a coordinator run makes an entry per tool
    /// per subagent and never stops.
    #[test]
    fn evicting_a_card_forgets_the_chain_that_pointed_at_it() {
        let mut t = Transcript::with_limits(Limits { max_elements: 2, ..Limits::default() });
        feed(&mut t, vec![call("tu_task", "Task", None), used("tu_task", "tu_inner", "Task")]);
        assert_eq!(t.subagent_owner.len(), 1, "the chain was recorded");
        feed(&mut t, vec![human("a"), human("b")]);
        assert!(t.tool(&"tu_task".into()).is_none(), "the card is evicted");
        assert!(t.subagent_owner.is_empty(), "and the chain went with it: {:?}", t.subagent_owner);
    }

    fn said_texts(log: &SubagentLog) -> Vec<String> {
        log.steps
            .iter()
            .filter_map(|s| match &s.act {
                SubagentAct::Said(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}
