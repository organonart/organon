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
//! no rect, no font, no pixel, no clock and no I/O — so it is `cargo test -p organon-shell`
//! in seconds, on any machine, with no GPU and no window. The same bar `scroll_anchor` and
//! `block_anchor` are held to, for the same reason.
//!
//! # It defines its own input type, deliberately
//!
//! [`AgentEvent`] is **not** the decoder's type and is not meant to be. Two modules cannot
//! own one type, and a transcript that spoke the wire format fluently would have to change
//! shape every time the wire did — on a CLI whose event set has already grown
//! `rate_limit_event` and `system/post_turn_summary` without telling anyone. So the
//! integrator writes a short mapping from the decoder's events onto these eight, and the
//! seam is where harness-specific knowledge stops. A second harness (Pi, §5.9.1) maps onto
//! the same eight or the model is wrong.
//!
//! # The five behaviours that make this non-trivial
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
//! # Ordering and identity
//!
//! Elements are appended in arrival order and **mutate in place**: a tool card that
//! resolves does not move, and text emitted after a tool call lands after it. Every element
//! carries an [`ElementId`] that is assigned once, never reused, and never changes — so a
//! view may key per-element state (a scroll anchor, an expanded/collapsed bit, a GPU
//! texture for an inline artifact) on it across frames.
//!
//! That is not hypothetical any more: [`Body::Artifact`] is an element the console puts in
//! the flow itself, and a live control panel is the first thing drawn for one. The element
//! carries a **description** — a title, slider names, button names — and nothing else; every
//! value a hand can move lives in the view, in a side map keyed by [`ElementId`]. This split
//! is the reason ids have to be stable rather than a nice property of them being so.
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

/// The eight events a transcript folds. See the module doc for why this is not the
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
    ToolResult { id: ToolId, output: String, is_error: bool },
    /// The run reached its final result. Records an outcome; closes nothing (behaviour 3).
    RunFinished { outcome: RunOutcome, detail: Option<String> },
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

/// One tool call and its outcome, as a view draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCard {
    pub call_id: ToolId,
    /// `None` only for an **orphan** card: a result arrived for an id whose call was never
    /// seen (a compaction boundary, a resumed session). The output is real and is kept
    /// rather than dropped; what is missing is the name.
    pub name: Option<String>,
    pub arguments: Arguments,
    pub state: ToolState,
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

/// What kind of artifact it is. One arm today; the shape is here so a second kind — an
/// engine-rendered picture, which needs a render-to-texture path and an eviction cap — is a
/// variant rather than a second [`Body`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtifactContent {
    /// A control panel: named sliders and named buttons, in draw order.
    Panel(PanelSpec),
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelSpec {
    pub sliders: Vec<String>,
    pub buttons: Vec<String>,
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
}

/// Bounds, stated rather than assumed. See "What is bounded" in the module doc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Maximum retained elements; the oldest are evicted first. Treated as at least 1 —
    /// a transcript that cannot hold the element it was just given is not a transcript.
    pub max_elements: usize,
}

impl Default for Limits {
    fn default() -> Self {
        // The same order of magnitude as `term::SCROLLBACK_LINES`, and for the same
        // reason: far past any session a human reads back through, near enough that
        // memory stays bounded on a coordinator run that fans out for hours.
        Limits { max_elements: 10_000 }
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
    pub orphan_argument_fragments: u64,
    pub late_deltas: u64,
    pub duplicate_results: u64,
    pub duplicate_calls: u64,
    /// Events that landed in an already-finished turn.
    pub trailing_events: u64,
}

/// The transcript: events in, ordered renderable elements out.
#[derive(Clone, Debug)]
pub struct Transcript {
    elements: VecDeque<Element>,
    turns: VecDeque<Turn>,
    by_message: HashMap<String, ElementId>,
    by_tool: HashMap<String, ElementId>,
    running: Vec<ElementId>,
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
            running: Vec::new(),
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

            AgentEvent::ToolResult { id, output, is_error } => {
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
        let mut t = Transcript::with_limits(Limits { max_elements: 4 });
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
        let mut t = Transcript::with_limits(Limits { max_elements: 3 });
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
        let mut t = Transcript::with_limits(Limits { max_elements: 0 });
        feed(&mut t, vec![human("a"), human("b")]);
        assert_eq!(t.len(), 1);
        assert_eq!(t.elements()[0].human().unwrap().text, "b");
        assert_eq!(t.turns().len(), 1, "turns with nothing left are pruned, the newest is kept");
    }

    fn panel(title: &str) -> ArtifactBlock {
        ArtifactBlock {
            title: title.to_string(),
            content: ArtifactContent::Panel(PanelSpec {
                sliders: vec!["depth".into(), "bloom".into()],
                buttons: vec!["metal".into(), "glass".into()],
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
        let mut t = Transcript::with_limits(Limits { max_elements: 3 });
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
            let mut t = Transcript::with_limits(Limits { max_elements: cap });
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
                        content: ArtifactContent::Panel(PanelSpec::default()),
                    });
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
                    };
                    let was = kinds.entry(e.id.0).or_insert(kind);
                    assert_eq!(*was, kind, "{ctx}: element {} changed kind", e.id.0);
                }

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
}
