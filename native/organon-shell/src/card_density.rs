//! Card density — how much room a tool call takes **after** it has stopped being news.
//!
//! # The problem
//!
//! A completed tool card used to render its full arguments and its full output forever, at
//! full weight, inside a bordered frame. James, looking at a real screenshot: *"a typical
//! screenshot is five or six tool calls, each with a beveled border around it, so it feels
//! like a list of bevel-bordered status updates."* The diagnosis is not that any one card is
//! too tall. It is that a turn's mechanical work occupied the transcript **in proportion to
//! how much work it was**, rather than to how much attention it deserved.
//!
//! # The rule
//!
//! **Success is quiet; only a departure from normal takes weight.** Same principle the roles
//! tier proposes for *colour*, extended to *persistence*:
//!
//! | state | treatment |
//! |---|---|
//! | running | open, unchanged — the work is happening |
//! | **succeeded** | one dense line: the verb, the object, and a magnitude. Expandable. |
//! | **failed** | open, bordered, loud — **unchanged, and never collapsible** |
//! | consecutive settled successes in one turn | one row with a count, expandable to the rows |
//!
//! 🚨 **Collapse, never delete.** The console's identity is that it gates and evidences what
//! an agent did. Nothing here removes an element, edits a transcript, or drops a byte: every
//! collapsed thing is one click from the card it was. The four things that must survive are
//! `SHELL_ARCHITECTURE.md` §1.1's, and three of them are enforced here rather than by care:
//! a failure is structurally excluded from both collapse and grouping, a **gated** call is
//! structurally excluded from grouping (see [`Slot::gated`]), and the scroll stability rule
//! below is the whole reason [`DensityMap::settle`] takes a `following` argument.
//!
//! # 🚨 Scroll stability, which is the requirement most likely to be got wrong
//!
//! Changing a card's height re-lays out the transcript. If content **above** the viewport
//! changes height, everything the reader is looking at jumps. `doc/console_rewrap_measurement.md`
//! measured what a width change costs; this is the same hazard reached from the other side.
//!
//! Two rules, together, make the jump impossible **by construction** rather than by
//! compensating for it after the fact:
//!
//! 1. **An automatic density change is applied only while the view is following the live
//!    edge.** [`DensityMap::settle`] takes `following` — the pane's own `pinned` bit — and a
//!    card that completes while a reader has scrolled up simply does not settle yet. It
//!    settles the moment they return to the bottom, where `stick_to_bottom` holds the last
//!    row in place and the shrink is absorbed above it. So while anybody is reading history,
//!    nothing above them can change height at all.
//! 2. **A manual toggle changes only the height of content at or below the row that was
//!    clicked** — and that row is on screen, because it was clicked. Everything above it
//!    keeps its layout position, so the clicked row does not move and neither does anything
//!    above it. Expansion grows downward, out of the way of the reader's eye.
//!
//! ⚠️ This is why the collapse decision is **not** re-derived from `ToolState` at each draw.
//! `ToolState::Complete` arrives as a `Change::Updated` on an element that may be far above
//! the live edge — a tool that ran for two minutes while the agent wrote twenty more
//! elements. Collapsing on the state alone would yank a reader who was nowhere near it.
//!
//! # Why grouping is structural and expansion is not
//!
//! [`Slot::settled`] decides *shape*; [`DensityMap::is_open`] decides *how tall one row is*.
//! Keeping them apart is what stops a hand from restructuring the transcript: expanding one
//! member of a group does not split the group into two groups and an orphan under the
//! reader's finger. The group stays one group; one of its rows got taller.
//!
//! No egui in this module, on purpose — the same rule [`crate::conversation`] and
//! [`crate::text_diff`] are held to. Every judgment here is a pure function over plain
//! values, so what can be *wrong* is tested without a window.

use std::collections::{HashMap, HashSet};

use crate::conversation::{Body, Element, ElementId, ToolCard, ToolState, TurnId};
use crate::text_diff::LineDiff;

/// How many consecutive settled successes it takes before they become one row.
///
/// **Three, not two.** Two dense lines cost two rows and name two tools; a group row costs
/// one row and names none of them, so at two the trade is a wash and the count is worth
/// less than the verbs. The noise James described starts at "five or six".
pub const GROUP_MIN: usize = 3;

/// The disclosure marks, and they are **ASCII on purpose**.
///
/// 🚨 egui 0.33 does no OS font fallback, and the four fonts it bundles carry no disclosure
/// triangle — `▸` U+25B8 and `▾` U+25BE are in none of them, exactly like the `✓`/`✗` that
/// shipped as boxes in the subagent card. The allowlist in `conversation_view`'s
/// `drawable` is the guard; ASCII sails through it because ASCII is in everything.
pub const MORE: &str = "+";
/// The other half of [`MORE`] — this row is already showing everything it has.
pub const LESS: &str = "-";

/// One tool card's density state.
///
/// Two independent bits, and the independence is the design (see the module doc).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CardState {
    /// The **automatic** bit: this call succeeded, and the view was following the live edge
    /// when it did, so its collapse could be applied without moving anybody's reading.
    /// Once true, always true — a tool call does not un-complete.
    settled: bool,
    /// The **hand's** bit. `None` means "however this card settled"; `Some` is a human's
    /// standing instruction, and nothing automatic ever clears it.
    ///
    /// 🚨 This is the field that answers *"a card that re-collapses under a reader's hand is
    /// worse than one that never collapsed."* A hand-expanded card stays expanded for the
    /// life of the element, through every later event.
    by_hand: Option<bool>,
}

/// What the view remembers about how densely each card is drawn.
///
/// Side state keyed by [`ElementId`], never in the transcript — the same argument
/// `PanelState` makes in `conversation_view`: the transcript is folded from an event stream
/// and its elements mutate as events arrive, so a density living there would be rewritten by
/// the fold. Pruned against the transcript by [`Self::retain`], so an element the cap evicted
/// takes its density with it.
#[derive(Clone, Debug, Default)]
pub struct DensityMap {
    cards: HashMap<ElementId, CardState>,
    /// The groups a hand has opened, keyed by the id of the run's first card. Absent is the
    /// default and the point: a group is collapsed unless somebody asked otherwise.
    opened: HashSet<ElementId>,
}

impl DensityMap {
    /// Bring the map up to date with the transcript, once per frame, **before** anything is
    /// laid out.
    ///
    /// `following` is the pane's `pinned` bit — whether the view is glued to the live edge.
    /// See the module doc: it is the whole of rule 1, and passing `true` unconditionally
    /// would reintroduce the jump this design exists to prevent.
    pub fn settle<'a>(&mut self, elements: impl Iterator<Item = &'a Element>, following: bool) {
        for element in elements {
            let Body::Tool(card) = &element.body else { continue };
            let state = self.cards.entry(element.id).or_default();
            // ⚠️ A failure never settles, whatever the scroll is doing. That asymmetry is
            // what makes the silence elsewhere safe to read: if nothing shouted, nothing
            // failed.
            if following && settles(card) {
                state.settled = true;
            }
        }
    }

    /// Is this card drawn at full weight?
    ///
    /// The hand's answer if it gave one; otherwise "yes, unless it has settled".
    pub fn is_open(&self, id: ElementId) -> bool {
        match self.cards.get(&id) {
            Some(state) => state.by_hand.unwrap_or(!state.settled),
            // Never seen — a card being drawn before its first `settle`. Open, because a
            // card nobody has decided about is news.
            None => true,
        }
    }

    /// Has this card settled — i.e. may it take part in a group?
    pub fn is_settled(&self, id: ElementId) -> bool {
        self.cards.get(&id).is_some_and(|state| state.settled)
    }

    /// A hand toggled this card. Records the new state **as the hand's**, so no later event
    /// can undo it.
    pub fn toggle_card(&mut self, id: ElementId) {
        let open = self.is_open(id);
        self.cards.entry(id).or_default().by_hand = Some(!open);
    }

    /// Is this group showing its members?
    pub fn is_group_open(&self, key: ElementId) -> bool {
        self.opened.contains(&key)
    }

    /// A hand toggled this group.
    pub fn toggle_group(&mut self, key: ElementId) {
        if !self.opened.remove(&key) {
            self.opened.insert(key);
        }
    }

    /// Drop state for elements the transcript no longer holds. The same line
    /// `conversation_view`'s other side maps get, and the same one-line answer to "does this
    /// leak on a session that runs all day".
    pub fn retain(&mut self, keep: impl Fn(ElementId) -> bool) {
        self.cards.retain(|id, _| keep(*id));
        self.opened.retain(|id| keep(*id));
    }
}

/// Does this card qualify for automatic collapse?
///
/// **Completed, and not an error.** A running card is live work and stays open; a failed card
/// is the one thing on the page that must keep its weight.
fn settles(card: &ToolCard) -> bool {
    matches!(card.state, ToolState::Complete { is_error: false, .. })
}

/// One element as the grouping arithmetic sees it — position, turn, and the two bits that
/// decide whether it may disappear into a count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub id: ElementId,
    pub turn: TurnId,
    /// A settled, non-error tool card. False for prose, for an approval, for a running card,
    /// for a failure, and for a success whose collapse has not been applied yet.
    pub settled: bool,
    /// 🚨 **A human was asked to authorise this exact call**, so its `toolu_` id is the only
    /// thing linking an approval card to its result — and a group would hide it. A gated call
    /// therefore keeps a row of its own forever. **An authorised call is never anonymous.**
    pub gated: bool,
}

/// What the walk draws, in transcript order.
///
/// Indices rather than ids, because a group's members are always **contiguous** — that is
/// what "a consecutive run" means — and because `Transcript::get` is a linear scan, so
/// resolving ids per frame would make the walk quadratic in the scrollback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// Draw this element as itself, at whatever density [`DensityMap::is_open`] gives it.
    One(usize),
    /// A run of settled successes drawn as one row. `len >= GROUP_MIN`, always.
    Group {
        /// The run's first element, which is the group's identity across frames.
        ///
        /// ⚠️ A run can grow at its **head** as well as its tail — an earlier card that was
        /// still running settles later — and the key then changes, so a hand-opened group
        /// re-closes. That can only happen while the view is following (rule 1), which is
        /// where the live edge is glued and the shape change is absorbed.
        key: ElementId,
        start: usize,
        len: usize,
    },
}

/// The grouping boundary, as a pure function.
///
/// **A maximal run of consecutive same-turn settled successes**, and the choice of
/// *consecutive run* over *per turn* is deliberate: a turn interleaves prose and tools —
/// "I'll read these three files", three calls, "now the edit", two calls. Grouping per turn
/// would either have to reorder the transcript, which destroys the record, or emit a group
/// that spans the prose between its members, which claims an adjacency that is not there. A
/// run preserves order exactly and groups precisely the block that *reads* as a block.
///
/// The turn is still a hard boundary on top of that, so a group is always "work done inside
/// one turn" and never a run that happens to straddle two.
///
/// A run shorter than [`GROUP_MIN`] stays as its own rows.
pub fn plan(slots: &[Slot]) -> Vec<Row> {
    let mut rows = Vec::with_capacity(slots.len());
    let mut i = 0;
    while i < slots.len() {
        let head = &slots[i];
        if !head.settled || head.gated {
            rows.push(Row::One(i));
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < slots.len() {
            let next = &slots[end];
            if !next.settled || next.gated || next.turn != head.turn {
                break;
            }
            end += 1;
        }
        let len = end - i;
        if len >= GROUP_MIN {
            rows.push(Row::Group { key: head.id, start: i, len });
        } else {
            rows.extend((i..end).map(Row::One));
        }
        i = end;
    }
    rows
}

/// What a group row says.
///
/// ⚠️ **There is no duration here, and the brief that commissioned this asked for one**
/// (`7 tools · 12s`). [`ToolCard`](crate::conversation::ToolCard) carries no timing, and
/// [`crate::conversation`] has no clock by design — the same refusal
/// `SubagentProgress::duration_ms`'s doc makes for a single card, one level up. A number the
/// view timed itself would be *the view's* stopwatch wearing the agent's voice, and it would
/// keep counting for a session that had silently died. So the group says how many, and which
/// verbs; the seconds are simply not on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupLine {
    /// `7 tools`.
    pub count: String,
    /// The distinct verbs in the run, in order, clipped — `Read, Grep + 2 more`. `None` when
    /// nothing in the run had a name (every member an orphan card).
    pub verbs: Option<String>,
}

/// How many distinct verbs a group row names before it starts counting them instead.
const GROUP_VERBS: usize = 3;

/// Build a group's one line from its members.
pub fn group_line<'a>(members: impl Iterator<Item = &'a ToolCard>) -> GroupLine {
    let mut n = 0usize;
    let mut verbs: Vec<&str> = Vec::new();
    for card in members {
        n += 1;
        if let Some(name) = card.name.as_deref() {
            if !verbs.contains(&name) {
                verbs.push(name);
            }
        }
    }
    let count = format!("{n} {}", if n == 1 { "tool" } else { "tools" });
    let verbs = if verbs.is_empty() {
        None
    } else if verbs.len() <= GROUP_VERBS {
        Some(verbs.join(", "))
    } else {
        let shown = verbs[..GROUP_VERBS].join(", ");
        Some(format!("{shown} + {} more", verbs.len() - GROUP_VERBS))
    };
    GroupLine { count, verbs }
}

/// A settled card as one line: **the verb, the object, and a magnitude**.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DenseLine {
    /// The tool's name, or the same `(call not seen)` an open orphan card shows.
    pub verb: String,
    /// What it acted on. `None` when the arguments name nothing a person would recognise —
    /// and then the row is the verb alone, which is still the whole truth about the call.
    pub object: Option<String>,
    /// How big it was. See [`magnitude`] — **`None` is a real answer** and the common one.
    pub magnitude: Option<String>,
}

/// The argument keys that name a call's object, in the order they are preferred.
///
/// ⚠️ **A list, not a guess.** Every key here is one a real tool on this wire sends; the
/// order is "what a person would say the call was about". A tool whose arguments contain
/// none of them has no object, and the row says the verb alone rather than picking the first
/// key alphabetically and hoping.
const OBJECT_KEYS: [&str; 8] =
    ["file_path", "notebook_path", "path", "command", "pattern", "url", "description", "prompt"];

/// The keys whose value is a **path**, where the informative end is the tail.
const PATH_KEYS: [&str; 3] = ["file_path", "notebook_path", "path"];

/// How many characters of an object a dense row shows.
const OBJECT_LIMIT: usize = 64;

/// One settled card, as the line that replaces it.
///
/// `diff` is the cached alignment `conversation_view` already holds for this card — handed
/// in rather than recomputed, for exactly the reason
/// [`ConversationPane::diffs`](crate::conversation_view::ConversationPane) exists.
pub fn dense_line(card: &ToolCard, diff: Option<&LineDiff>) -> DenseLine {
    DenseLine {
        verb: card.name.clone().unwrap_or_else(|| "(call not seen)".to_string()),
        object: object_of(card),
        magnitude: magnitude(card, diff),
    }
}

/// What a call acted on, from its arguments.
///
/// ⚠️ Only once the arguments are **complete**. While a call streams, its text is genuinely
/// half a JSON document — [`Arguments`](crate::conversation::Arguments)' contract — and this
/// function is never asked for a streaming card anyway, because a streaming card has not
/// settled.
fn object_of(card: &ToolCard) -> Option<String> {
    if !card.arguments.complete {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&card.arguments.text).ok()?;
    let map = value.as_object()?;
    for key in OBJECT_KEYS {
        let Some(found) = map.get(key) else { continue };
        let text = match found {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let flat = text.replace(['\n', '\r'], " ");
        let flat = flat.trim();
        if flat.is_empty() {
            continue;
        }
        return Some(clip(flat, PATH_KEYS.contains(&key)));
    }
    None
}

/// Cut a value down to [`OBJECT_LIMIT`] characters, keeping the end that carries the meaning.
///
/// A path's tail is its filename and a command's head is its verb, so the cut goes at
/// opposite ends and the `…` says which — never a silent truncation.
///
/// `…` U+2026 is in both of egui's text faces; the ellipsis is not a glyph risk the way a
/// disclosure triangle is.
fn clip(value: &str, keep_tail: bool) -> String {
    if value.chars().count() <= OBJECT_LIMIT {
        return value.to_string();
    }
    let chars: Vec<char> = value.chars().collect();
    if keep_tail {
        let tail: String = chars[chars.len() - (OBJECT_LIMIT - 1)..].iter().collect();
        format!("…{tail}")
    } else {
        let head: String = chars[..OBJECT_LIMIT - 1].iter().collect();
        format!("{head}…")
    }
}

/// **How big was it** — and there is no universal number, which is the whole of this
/// function's difficulty.
///
/// The order is a provenance order, not a preference: what the *tool* measured beats what
/// this view can derive from what the tool sent.
///
/// 1. `ResultDetail` — the tool's own line counts, the one **measured** magnitude on the
///    wire (`Read`'s `numLines`/`totalLines`).
/// 2. The cached `Edit` alignment — `+3 -1`, derived from the two strings the call carried.
/// 3. A dispatch's `task_*` progress — `3 tools · 12.4s`, and it is the one place a duration
///    exists, because the *harness* measured it.
/// 4. The output's own line count — derived, and the last resort.
///
/// 🚨 **`None` is a real answer and the common one.** A `Bash` that printed nothing, an MCP
/// verb that returned an empty string, a tool nobody has a number for: the magnitude slot is
/// simply absent. It is never a zero and never a stand-in — the same refusal `SessionFacts`
/// makes for a context percentage whose denominator is not on the wire. An invented number
/// on a row this dense would be read as measured.
pub fn magnitude(card: &ToolCard, diff: Option<&LineDiff>) -> Option<String> {
    let detail = &card.detail;
    if let (Some(lines), Some(total)) = (detail.lines, detail.total_lines) {
        return Some(if lines == total {
            format!("{lines} lines")
        } else {
            format!("{lines} of {total} lines")
        });
    }
    if let Some(diff) = diff.filter(|d| d.has_changes()) {
        return Some(format!("+{} -{}", diff.added, diff.removed));
    }
    if let Some(progress) = progress_magnitude(card) {
        return Some(progress);
    }
    let output = card.state.output().unwrap_or_default();
    let lines = output.lines().count();
    if lines == 0 {
        return None;
    }
    Some(format!("{lines} {}", if lines == 1 { "line" } else { "lines" }))
}

/// A dispatch card's magnitude, from what the harness said about the agent it ran.
///
/// ⚠️ The elapsed time is the **harness's** stopwatch, frozen at its last line — see
/// `progress_summary`, which makes the same point for the open card. It is reported here
/// because the harness measured it, and for no other reason.
fn progress_magnitude(card: &ToolCard) -> Option<String> {
    let progress = &card.progress;
    let mut parts = Vec::new();
    if let Some(uses) = progress.tool_uses {
        parts.push(format!("{uses} {}", if uses == 1 { "tool" } else { "tools" }));
    }
    if let Some(ms) = progress.duration_ms {
        parts.push(elapsed(ms));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

/// A duration a person can read. The twin of `conversation_view::elapsed`, and deliberately
/// a copy of nothing: that one is private to the drawing module and this one is used by a
/// pure function tested without egui. Both truncate rather than round up, which is the rule
/// that matters and is pinned on both sides.
fn elapsed(ms: u64) -> String {
    if ms < 60_000 {
        format!("{}.{}s", ms / 1000, (ms % 1000) / 100)
    } else {
        let seconds = ms / 1000;
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

/// Every `toolu_` id a human was asked to authorise, from the approval cards in the flow.
///
/// One pass over the transcript per frame, the same order of cost as the walk that follows
/// it. See [`Slot::gated`] for what it buys.
pub fn gated_calls<'a>(elements: impl Iterator<Item = &'a Element>) -> HashSet<String> {
    elements
        .filter_map(|element| element.approval())
        .map(|block| block.tool_use_id.clone())
        .collect()
}

/// Build the grouping input for one frame.
pub fn slots<'a>(
    elements: impl Iterator<Item = &'a Element>,
    density: &DensityMap,
    gated: &HashSet<String>,
) -> Vec<Slot> {
    elements
        .map(|element| {
            let card = element.tool();
            Slot {
                id: element.id,
                turn: element.turn,
                // Both halves are required: `settled` alone would let a *failure* into a
                // run the moment somebody made failures settle, and `settles` alone would
                // let a success collapse the structure while a reader was scrolled up.
                settled: card.is_some_and(settles) && density.is_settled(element.id),
                gated: card.is_some_and(|c| gated.contains(c.call_id.as_str())),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{Arguments, ResultDetail, SubagentLog, SubagentProgress, ToolId};

    fn card(name: &str, state: ToolState) -> ToolCard {
        ToolCard {
            call_id: ToolId(format!("toolu_{name}")),
            name: Some(name.to_string()),
            arguments: Arguments { text: "{}".into(), complete: true },
            state,
            subagent: SubagentLog::default(),
            detail: ResultDetail::default(),
            progress: SubagentProgress::default(),
        }
    }

    fn ok(name: &str) -> ToolCard {
        card(name, ToolState::Complete { output: String::new(), is_error: false })
    }

    fn failed(name: &str) -> ToolCard {
        card(name, ToolState::Complete { output: "boom".into(), is_error: true })
    }

    fn element(id: u64, turn: u64, body: Body) -> Element {
        Element { id: ElementId(id), turn: TurnId(turn), body }
    }

    fn tool(id: u64, turn: u64, card: ToolCard) -> Element {
        element(id, turn, Body::Tool(card))
    }

    fn prose(id: u64, turn: u64) -> Element {
        element(
            id,
            turn,
            Body::Assistant(crate::conversation::AssistantBlock {
                message_id: crate::conversation::MessageId("m".into()),
                text: "hello".into(),
                complete: true,
            }),
        )
    }

    fn settled_map(elements: &[Element]) -> DensityMap {
        let mut map = DensityMap::default();
        map.settle(elements.iter(), true);
        map
    }

    fn planned(elements: &[Element], gated: &HashSet<String>) -> Vec<Row> {
        let map = settled_map(elements);
        plan(&slots(elements.iter(), &map, gated))
    }

    // ---- the state model -------------------------------------------------------------

    /// 🚨 CONTRACT: a failed card is never collapsed, by any path, ever. This is the
    /// asymmetry that makes the silence around it safe to read.
    #[test]
    fn a_failure_never_settles_and_never_collapses() {
        let elements = [tool(1, 1, failed("Bash"))];
        let map = settled_map(&elements);
        assert!(!map.is_settled(ElementId(1)));
        assert!(map.is_open(ElementId(1)), "a failed card is drawn open");
    }

    /// 🚨 CONTRACT: and it is never inside a group either — a run of successes with a
    /// failure in it is not a success group.
    #[test]
    fn a_failure_breaks_a_run_rather_than_joining_it() {
        let elements = [
            tool(1, 1, ok("Read")),
            tool(2, 1, ok("Read")),
            tool(3, 1, failed("Bash")),
            tool(4, 1, ok("Read")),
            tool(5, 1, ok("Read")),
        ];
        let rows = planned(&elements, &HashSet::new());
        // Two runs of two, neither long enough to group, with the failure standing between
        // them — five rows, no groups.
        assert_eq!(rows, vec![Row::One(0), Row::One(1), Row::One(2), Row::One(3), Row::One(4)]);
    }

    #[test]
    fn a_running_card_stays_open_and_a_settled_one_does_not() {
        let elements = [tool(1, 1, card("Bash", ToolState::Running)), tool(2, 1, ok("Read"))];
        let map = settled_map(&elements);
        assert!(map.is_open(ElementId(1)), "running work keeps its weight");
        assert!(!map.is_open(ElementId(2)), "a settled success is one line");
    }

    /// 🚨 CONTRACT: rule 1 of the scroll-stability argument. A card that completes while a
    /// reader has scrolled up does not change height until they come back.
    #[test]
    fn nothing_settles_while_a_reader_is_scrolled_up() {
        let elements = [tool(1, 1, ok("Read"))];
        let mut map = DensityMap::default();
        map.settle(elements.iter(), false);
        assert!(!map.is_settled(ElementId(1)));
        assert!(map.is_open(ElementId(1)), "height is unchanged while somebody is reading");
        // Back at the live edge, where `stick_to_bottom` absorbs the shrink.
        map.settle(elements.iter(), true);
        assert!(!map.is_open(ElementId(1)));
    }

    /// 🚨 CONTRACT: a hand outranks the machine, permanently. A card the reader opened stays
    /// open through every later event — re-collapsing under their hand is worse than never
    /// having collapsed.
    #[test]
    fn a_hand_expanded_card_stays_expanded_across_later_events() {
        let elements = [tool(1, 1, ok("Read"))];
        let mut map = settled_map(&elements);
        assert!(!map.is_open(ElementId(1)));
        map.toggle_card(ElementId(1));
        assert!(map.is_open(ElementId(1)));
        // Every subsequent frame, with more of the session arriving each time.
        for _ in 0..5 {
            map.settle(elements.iter(), true);
            assert!(map.is_open(ElementId(1)), "the hand's answer survives the fold");
        }
    }

    #[test]
    fn a_hand_collapsed_card_stays_collapsed_too() {
        let elements = [tool(1, 1, card("Bash", ToolState::Running))];
        let mut map = settled_map(&elements);
        map.toggle_card(ElementId(1));
        map.settle(elements.iter(), true);
        assert!(!map.is_open(ElementId(1)));
    }

    #[test]
    fn density_state_dies_with_the_element_it_describes() {
        let elements = [tool(1, 1, ok("Read")), tool(2, 1, ok("Read"))];
        let mut map = settled_map(&elements);
        map.toggle_group(ElementId(1));
        map.retain(|id| id == ElementId(2));
        assert!(map.is_open(ElementId(1)), "an unknown id is open, i.e. forgotten");
        assert!(!map.is_group_open(ElementId(1)));
        assert!(!map.is_open(ElementId(2)));
    }

    // ---- the grouping boundary -------------------------------------------------------

    #[test]
    fn a_run_of_three_becomes_one_group_and_the_count_matches_its_contents() {
        let elements =
            [tool(1, 1, ok("Read")), tool(2, 1, ok("Grep")), tool(3, 1, ok("Read"))];
        let rows = planned(&elements, &HashSet::new());
        assert_eq!(rows, vec![Row::Group { key: ElementId(1), start: 0, len: 3 }]);
        let Row::Group { start, len, .. } = rows[0] else { unreachable!() };
        let members: Vec<&ToolCard> =
            elements[start..start + len].iter().filter_map(|e| e.tool()).collect();
        assert_eq!(members.len(), len, "every index in the run resolves to a card");
        let line = group_line(members.into_iter());
        assert_eq!(line.count, "3 tools");
        assert_eq!(line.verbs.as_deref(), Some("Read, Grep"));
    }

    #[test]
    fn a_run_of_two_stays_two_rows() {
        let elements = [tool(1, 1, ok("Read")), tool(2, 1, ok("Read"))];
        assert_eq!(planned(&elements, &HashSet::new()), vec![Row::One(0), Row::One(1)]);
    }

    /// The turn is a hard boundary: a group is always work done inside one turn.
    #[test]
    fn a_run_never_crosses_a_turn() {
        let elements = [
            tool(1, 1, ok("Read")),
            tool(2, 1, ok("Read")),
            tool(3, 2, ok("Read")),
            tool(4, 2, ok("Read")),
            tool(5, 2, ok("Read")),
        ];
        assert_eq!(
            planned(&elements, &HashSet::new()),
            vec![Row::One(0), Row::One(1), Row::Group { key: ElementId(3), start: 2, len: 3 }]
        );
    }

    /// Consecutive, not per-turn: prose between two sets of calls keeps them apart, because
    /// grouping across it would claim an adjacency the transcript does not have.
    #[test]
    fn prose_between_two_sets_of_calls_keeps_them_apart() {
        let elements = [
            tool(1, 1, ok("Read")),
            tool(2, 1, ok("Read")),
            tool(3, 1, ok("Read")),
            prose(4, 1),
            tool(5, 1, ok("Edit")),
            tool(6, 1, ok("Edit")),
            tool(7, 1, ok("Edit")),
        ];
        assert_eq!(
            planned(&elements, &HashSet::new()),
            vec![
                Row::Group { key: ElementId(1), start: 0, len: 3 },
                Row::One(3),
                Row::Group { key: ElementId(5), start: 4, len: 3 },
            ]
        );
    }

    /// 🚨 CONTRACT: an approval and the result it authorises share a `toolu_` id and nothing
    /// else. A gated call therefore keeps its own row, where that id is drawn — so a human
    /// can always get from the question to the answer.
    #[test]
    fn a_gated_call_is_never_swallowed_by_a_group() {
        let elements = [
            tool(1, 1, ok("Read")),
            tool(2, 1, ok("Bash")),
            tool(3, 1, ok("Read")),
            tool(4, 1, ok("Read")),
        ];
        let gated: HashSet<String> = ["toolu_Bash".to_string()].into_iter().collect();
        let rows = planned(&elements, &gated);
        assert_eq!(rows, vec![Row::One(0), Row::One(1), Row::One(2), Row::One(3)]);
        // …and without the gate, the same four are one group — so it really is the
        // authorisation doing the work and not the shape of the run.
        assert_eq!(
            planned(&elements, &HashSet::new()),
            vec![Row::Group { key: ElementId(1), start: 0, len: 4 }]
        );
    }

    /// The other half of the same contract, at the source: the ids come off the approval
    /// cards in the flow, so nothing has to remember to register one.
    #[test]
    fn the_gated_set_is_read_off_the_approvals_in_the_transcript() {
        let approval = element(
            9,
            1,
            Body::Approval(crate::conversation::ApprovalBlock {
                tool_name: "Bash".into(),
                input: "{}".into(),
                tool_use_id: "toolu_Bash".into(),
                state: crate::conversation::ApprovalState::Pending,
            }),
        );
        let elements = [approval, tool(1, 1, ok("Bash"))];
        assert_eq!(
            gated_calls(elements.iter()),
            ["toolu_Bash".to_string()].into_iter().collect::<HashSet<_>>()
        );
    }

    #[test]
    fn a_group_names_its_first_verbs_and_counts_the_rest() {
        let cards = [ok("Read"), ok("Grep"), ok("Glob"), ok("Bash"), ok("Read")];
        let line = group_line(cards.iter());
        assert_eq!(line.count, "5 tools");
        assert_eq!(line.verbs.as_deref(), Some("Read, Grep, Glob + 1 more"));
    }

    #[test]
    fn a_group_of_nameless_cards_names_none() {
        let mut anon = ok("Read");
        anon.name = None;
        let cards = [anon.clone(), anon.clone(), anon];
        assert_eq!(group_line(cards.iter()).verbs, None);
    }

    // ---- the dense line --------------------------------------------------------------

    /// 🚨 CONTRACT: whatever else it drops, a collapsed card still names the tool and what it
    /// acted on.
    #[test]
    fn a_collapsed_card_names_the_tool_and_its_object() {
        let mut read = ok("Read");
        read.arguments =
            Arguments { text: r#"{"file_path":"C:\\src\\foo.ts"}"#.into(), complete: true };
        read.detail = ResultDetail {
            file_path: Some("C:\\src\\foo.ts".into()),
            lines: Some(120),
            total_lines: Some(120),
            start_line: None,
        };
        let line = dense_line(&read, None);
        assert_eq!(line.verb, "Read");
        assert_eq!(line.object.as_deref(), Some("C:\\src\\foo.ts"));
        assert_eq!(line.magnitude.as_deref(), Some("120 lines"));
    }

    #[test]
    fn a_partial_read_says_how_much_of_the_file_it_covered() {
        let mut read = ok("Read");
        read.detail = ResultDetail {
            file_path: None,
            lines: Some(120),
            total_lines: Some(900),
            start_line: Some(40),
        };
        assert_eq!(magnitude(&read, None).as_deref(), Some("120 of 900 lines"));
    }

    #[test]
    fn an_edit_reports_its_own_size() {
        let edit = ok("Edit");
        let diff = crate::text_diff::line_diff("a\nb\nc\n", "a\nB\nc\nd\n");
        assert_eq!(magnitude(&edit, Some(&diff)).as_deref(), Some("+2 -1"));
    }

    #[test]
    fn a_dispatch_reports_what_the_harness_measured() {
        let mut agent = ok("Agent");
        agent.progress = SubagentProgress {
            tool_uses: Some(3),
            duration_ms: Some(12_400),
            ..SubagentProgress::default()
        };
        assert_eq!(magnitude(&agent, None).as_deref(), Some("3 tools · 12.4s"));
    }

    #[test]
    fn a_tool_that_only_printed_is_measured_by_what_it_printed() {
        let bash =
            card("Bash", ToolState::Complete { output: "one\ntwo\n".into(), is_error: false });
        assert_eq!(magnitude(&bash, None).as_deref(), Some("2 lines"));
    }

    /// 🚨 CONTRACT: a tool with nothing to measure renders **no** magnitude. Never a zero,
    /// never a stand-in — an invented number on a row this dense reads as a measured one.
    #[test]
    fn a_tool_with_no_magnitude_renders_none() {
        let quiet = ok("mcp__organon__background");
        assert_eq!(magnitude(&quiet, None), None);
        let line = dense_line(&quiet, None);
        assert_eq!(line.verb, "mcp__organon__background");
        assert_eq!(line.magnitude, None);
    }

    #[test]
    fn an_orphan_card_says_so_rather_than_inventing_a_name() {
        let mut orphan = ok("Read");
        orphan.name = None;
        orphan.arguments = Arguments::pending();
        let line = dense_line(&orphan, None);
        assert_eq!(line.verb, "(call not seen)");
        assert_eq!(line.object, None);
    }

    #[test]
    fn a_long_path_keeps_its_filename_and_a_long_command_keeps_its_verb() {
        let mut read = ok("Read");
        let path = format!("C:\\{}\\the_actual_file.rs", "very_long_directory".repeat(4));
        read.arguments = Arguments {
            text: serde_json::json!({ "file_path": path }).to_string(),
            complete: true,
        };
        let object = object_of(&read).unwrap();
        assert!(object.starts_with('…'), "{object}");
        assert!(object.ends_with("the_actual_file.rs"), "{object}");
        assert_eq!(object.chars().count(), OBJECT_LIMIT);

        let mut bash = ok("Bash");
        bash.arguments = Arguments {
            text: serde_json::json!({ "command": format!("cargo test {}", "-p organon ".repeat(20)) })
                .to_string(),
            complete: true,
        };
        let object = object_of(&bash).unwrap();
        assert!(object.starts_with("cargo test"), "{object}");
        assert!(object.ends_with('…'), "{object}");
    }

    #[test]
    fn a_streaming_call_has_no_object_because_its_arguments_are_half_a_document() {
        let mut streaming = card("Edit", ToolState::Running);
        streaming.arguments = Arguments { text: r#"{"file_pa"#.into(), complete: false };
        assert_eq!(object_of(&streaming), None);
    }

    #[test]
    fn the_disclosure_marks_are_ascii() {
        for mark in [MORE, LESS] {
            assert!(mark.is_ascii(), "{mark:?} — egui bundles no disclosure triangle");
        }
    }
}
