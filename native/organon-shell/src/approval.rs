//! Answering "may I?" — the console's side of `--permission-prompt-tool`.
//!
//! Three things live here, and none of them draws or decodes anything:
//!
//! 1. **[`ApprovalGate`]** — a [`PermissionResponder`] that hands the question to the UI
//!    thread and *blocks* until a human answers it.
//! 2. **[`DecisionMemory`]** — "allow and remember", which is entirely ours to build
//!    (`doc/console_approval_protocol.md` §5 measured three identical calls producing
//!    three separate requests; there is **no upstream persistence** and no response field
//!    that caches anything).
//! 3. **[`resolve_choice`]** — what a click *means*: the wire decision that goes back to
//!    the client, and the [`Answer`] the transcript records.
//!
//! # The blocking is the design — but the waiting is not free
//!
//! 🚨 [`PermissionResponder::decide`] is synchronous, and the client is waiting on a
//! JSON-RPC response while it runs. [`ApprovalGate`] therefore posts a [`PendingApproval`]
//! and blocks on a reply channel; it runs on [`crate::mcp_http`]'s serve thread, never on
//! the UI thread.
//!
//! 🚨 **This module used to say "no timeout, on purpose". That was wrong, and a human
//! reading a card found it.** Claude Code gives up on a permission call after
//! [`crate::mcp::CLIENT_DEADLINE_SECS`] — measured, twice, to a hundredth of a second — and the tool
//! then fails with *"The operation timed out"* while the card sits there still asking. So
//! the gate does not simply block: it waits in [`HEARTBEAT`] steps and lets the transport
//! emit a keep-alive at each one, which is measured to reset that clock (a 300 s wait was
//! answered and honoured). A human now has as long as they need.
//!
//! ⚠️ **Dropping a [`PendingApproval`] denies it, by construction.** The reply `Sender` goes
//! with it, the gate's `recv` fails, and the gate answers `deny`. That is not a leak being
//! tolerated — it is the only safe reading of "the console lost track of this question", and
//! it is what makes an approval evicted by the transcript's cap fail closed instead of
//! hanging the agent forever.
//!
//! ⚠️ **And the reverse direction now exists too.** A keep-alive that cannot be written, or
//! a socket the client has closed, means the question is dead — the turn was cancelled, the
//! agent was killed, or the client gave up anyway. The gate then marks the
//! [`PendingApproval`] **abandoned** and stops waiting. That flag is what the UI reads to
//! turn a live card into a closed one: a card still offering *allow* for a call that has
//! already failed is worse than no card at all.
//!
//! # What "remember" remembers
//!
//! **The tool and its arguments, exactly** — `Bash` with *this* command, not `Bash`. Two
//! reasons, one measured and one not:
//!
//! * The measured case is the repetition §5 found: the same call asked three times. Keying
//!   on the whole call answers exactly that and nothing more.
//! * A key of "the tool" would make one click on a `Write` card authorise every future
//!   write to every path. A permission surface whose narrowest gesture is that broad is not
//!   a permission surface.
//!
//! The arguments are canonicalised ([`decision_key`]) so key order cannot make one call
//! look like two. **Scope is the session**: the memory lives in the pane and dies with the
//! tab. Nothing is written to disk, deliberately — a remembered decision that outlives the
//! window it was made in is one the human cannot find again.
//!
//! # And the widest gesture: allow everything for the rest of this session
//!
//! [`DecisionMemory::allow_everything`] is the same memory widened, not a second mechanism
//! and **emphatically not an upstream permission mode**. `bypassPermissions` is unreachable
//! (the CLI refuses it without a launch flag the console does not pass) and `dontAsk`
//! **refuses** rather than allows — both measured, both recorded in
//! `doc/console_approval_protocol.md`. This is the console answering *yes* to a question it
//! is still being asked, which is why the handler still runs, the card is still drawn, and
//! the transcript still records every call.
//!
//! ⚠️ **A per-call decision outranks it.** [`DecisionMemory::recall`] checks the entries
//! first, so a call a human explicitly denied-and-remembered stays denied under a standing
//! allow. The broad gesture is the *default* for calls nobody has decided, never an
//! overrule of a specific refusal — and the card that says "denied · from a decision you
//! already made" carries its own `forget` button, so the narrower rule is the one that can
//! be found and undone.
//!
//! 🚨 **It is session-scoped and it dies with the tab, exactly like the entries** — nothing
//! is written to disk, and a new tab starts asking again. It is also revocable from the
//! band rather than from a card, because there is no one card it belongs to: see
//! [`crate::conversation_view::StripContent::session_allow`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;

use serde_json::Value;

use crate::conversation::{Answer, AnsweredBy, Verdict};
use crate::mcp::{
    Heartbeat, PermissionDecision, PermissionRequest, PermissionResponder, HEARTBEAT,
};

/// What a deny tells the model. It surfaces as `"non_execution_kind":"permission-rule"`
/// (§4), so the text is the only thing that says *who* refused.
pub const DENIED: &str = "denied by the human at the Organon Console";

/// What the gate answers when there is nobody to ask. Distinct from [`DENIED`] because the
/// two are different events — one is a decision, the other is the console failing to offer
/// one — and a log that cannot tell them apart hides a broken pane.
pub const UNREACHABLE: &str =
    "the Organon Console could not put this approval in front of a human, so it was refused";

/// What the gate answers when the client stopped listening mid-question.
///
/// Nothing reads it — the socket it would travel on is the one that closed — but it is the
/// value the blocked call returns, and naming it is what keeps the three ways a question
/// can end distinguishable in a log. **Fail closed** here as everywhere else: a request the
/// console can no longer answer is never an allow.
pub const ABANDONED: &str =
    "the agent stopped waiting for this approval before a human answered it";

/// Which button was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Allow,
    /// Allow, and answer the *identical* call from memory next time.
    AllowAndRemember,
    /// Allow, and stop asking about **anything** for the rest of this session.
    ///
    /// 🚨 The widest gesture the console offers, and the one the band must then carry a
    /// standing marker for — see the module doc.
    AllowEverythingThisSession,
    Deny,
}

// ---------------------------------------------------------------------------
// The thread crossing
// ---------------------------------------------------------------------------

/// One question, in flight between the serve thread and the UI.
///
/// It cannot be cloned and it answers once: [`PendingApproval::answer`] consumes it, and
/// dropping it unanswered denies (see the module doc).
pub struct PendingApproval {
    request: PermissionRequest,
    reply: Sender<PermissionDecision>,
    abandoned: Arc<AtomicBool>,
}

impl PendingApproval {
    pub fn request(&self) -> &PermissionRequest {
        &self.request
    }

    /// **Has the agent stopped waiting?**
    ///
    /// Set by the gate on the serve thread the moment its keep-alive finds the client
    /// gone. The UI polls it rather than being pushed, because it is already draining an
    /// inbox every frame and a second channel would be a second thing to forget to read.
    ///
    /// ⚠️ Once true it never goes back. A question cannot be un-abandoned, and a card that
    /// flickered between asking and closed would be worse than either.
    pub fn is_abandoned(&self) -> bool {
        self.abandoned.load(Ordering::Relaxed)
    }

    /// Answer it. A closed channel means the agent gave up first — nothing to do, and
    /// nothing worth reporting, since the question no longer exists.
    pub fn answer(self, decision: PermissionDecision) {
        let _ = self.reply.send(decision);
    }
}

impl std::fmt::Debug for PendingApproval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingApproval").field("request", &self.request).finish()
    }
}

/// The [`PermissionResponder`] the MCP server is built with: post, then wait.
pub struct ApprovalGate {
    to_ui: Sender<PendingApproval>,
    /// How long to wait between keep-alives. [`HEARTBEAT`] in the console; shorter in a
    /// test, which is the only reason it is a field and not a constant read inline — a
    /// property about waiting that took ten seconds a step to demonstrate would be a
    /// property nobody runs.
    beat_every: std::time::Duration,
}

impl ApprovalGate {
    /// Beat at this cadence instead of [`HEARTBEAT`].
    ///
    /// ⚠️ The cadence is only safe **below** [`crate::mcp::CLIENT_DEADLINE_SECS`], and with
    /// room for a slow write; see [`HEARTBEAT`] for the margin the default is chosen for.
    /// Nothing here validates that, because the deadline belongs to the client and a value
    /// this type refused would be one it had no way to check.
    pub fn beating_every(mut self, interval: std::time::Duration) -> Self {
        self.beat_every = interval;
        self
    }
}

impl PermissionResponder for ApprovalGate {
    /// Post the question, then wait — in [`HEARTBEAT`] steps, so the client is told at
    /// every step that we are still here.
    ///
    /// 🚨 **The loop is the fix.** A plain `recv()` is what shipped, and it is correct only
    /// if the client waits forever; it waits [`crate::mcp::CLIENT_DEADLINE_SECS`]. Each timeout tick
    /// asks the transport to emit one keep-alive, which the measurement says resets that
    /// clock, and to report whether the client is still on the other end.
    fn decide(&mut self, request: &PermissionRequest, beat: &mut dyn Heartbeat) -> PermissionDecision {
        let (reply, answer) = mpsc::channel();
        let abandoned = Arc::new(AtomicBool::new(false));
        let pending =
            PendingApproval { request: request.clone(), reply, abandoned: abandoned.clone() };
        if self.to_ui.send(pending).is_err() {
            // The pane is gone. Fail closed: an agent that proceeds because nobody was
            // watching is the exact failure this whole path exists to prevent.
            return PermissionDecision::deny(UNREACHABLE);
        }
        loop {
            match answer.recv_timeout(self.beat_every) {
                Ok(decision) => return decision,
                // The pane dropped the question — evicted, or the tab closed.
                Err(RecvTimeoutError::Disconnected) => {
                    return PermissionDecision::deny(UNREACHABLE)
                }
                Err(RecvTimeoutError::Timeout) => {
                    if !beat.beat() {
                        // ⚠️ Marked **before** returning, because the return value goes
                        // nowhere: the socket it would be written to is the one that just
                        // closed. This flag is the only thing that reaches the card.
                        abandoned.store(true, Ordering::Relaxed);
                        return PermissionDecision::deny(ABANDONED);
                    }
                }
            }
        }
    }
}

/// The two ends: the gate for the MCP server, the receiver for the UI to drain.
pub fn approval_channel() -> (ApprovalGate, Receiver<PendingApproval>) {
    let (to_ui, from_agent) = mpsc::channel();
    (ApprovalGate { to_ui, beat_every: HEARTBEAT }, from_agent)
}

// ---------------------------------------------------------------------------
// The memory
// ---------------------------------------------------------------------------

/// A remembered decision's identity: the tool, plus its arguments in canonical form.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DecisionKey(String);

impl DecisionKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The key for one call.
///
/// `input` is the arguments as **text**, because that is the form the transcript keeps
/// them in ([`crate::conversation::ApprovalBlock::input`]) and re-deriving the key from a
/// card is what makes revocation possible from the card itself.
///
/// ⚠️ **The arguments are canonicalised, not compared as text.** `{"a":1,"b":2}` and
/// `{"b":2,"a":1}` are the same call, and a memory that disagreed would ask again for a
/// decision the human had already made — which reads as the feature being broken. Text
/// that is not JSON at all keys on itself, trimmed: unusual, but a wrong key is better
/// than a key that collides with everything else unparseable.
pub fn decision_key(tool_name: &str, input: &str) -> DecisionKey {
    let mut out = String::with_capacity(tool_name.len() + input.len() + 1);
    out.push_str(tool_name);
    out.push('\u{1f}');
    match serde_json::from_str::<Value>(input) {
        Ok(value) => canonical(&value, &mut out),
        Err(_) => out.push_str(input.trim()),
    }
    DecisionKey(out)
}

fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*key).clone()).to_string());
                out.push(':');
                canonical(&map[key.as_str()], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// One entry in the list a human reads and revokes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remembered {
    pub key: DecisionKey,
    pub tool_name: String,
    /// One line naming what was permitted — the `Bash` command, or the arguments.
    pub summary: String,
    pub verdict: Verdict,
}

/// Why the console answered a call without asking.
///
/// The two arms are the two standing sources, and they are kept apart all the way to the
/// card because they are **revoked in different places** — see [`AnsweredBy`], which this
/// maps onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recall {
    /// This exact call was decided before, and this is what was decided.
    ThisCall(Verdict),
    /// Nothing was decided about this call; everything is allowed for the rest of the
    /// session.
    SessionAllow,
}

impl Recall {
    pub fn verdict(self) -> Verdict {
        match self {
            Recall::ThisCall(verdict) => verdict,
            Recall::SessionAllow => Verdict::Allow,
        }
    }

    /// How the card records it.
    pub fn answered_by(self) -> AnsweredBy {
        match self {
            Recall::ThisCall(_) => AnsweredBy::ThisCall,
            Recall::SessionAllow => AnsweredBy::SessionAllow,
        }
    }
}

/// The console's own decision memory. Session-scoped; see the module doc.
#[derive(Clone, Debug, Default)]
pub struct DecisionMemory {
    entries: Vec<Remembered>,
    /// The standing allow. A plain `bool` because it holds no verdict worth choosing: a
    /// standing *deny* would be a console that refuses everything while still looking like
    /// it is asking, which is `dontAsk` — a mode that already exists upstream and that this
    /// console offers a labelled row for. Nobody needs a second one.
    session_allow: bool,
}

impl DecisionMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// The verdict for this exact call, if one was remembered. **Entries only** — see
    /// [`Self::recall`] for the answer that also honours a standing allow.
    pub fn lookup(&self, tool_name: &str, input: &str) -> Option<Verdict> {
        let key = decision_key(tool_name, input);
        self.entries.iter().find(|e| e.key == key).map(|e| e.verdict)
    }

    /// **The whole auto-answer.** A per-call decision if there is one, else a standing
    /// allow if one is active, else `None` — which is the only case that reaches a human.
    ///
    /// ⚠️ The order is the policy, not an implementation detail: a specific decision wins
    /// over the blanket one. The module doc carries the argument.
    pub fn recall(&self, tool_name: &str, input: &str) -> Option<Recall> {
        if let Some(verdict) = self.lookup(tool_name, input) {
            return Some(Recall::ThisCall(verdict));
        }
        self.session_allow.then_some(Recall::SessionAllow)
    }

    /// Is the standing allow active? **The band reads this every frame** and derives its
    /// marker from it, so this is the single fact behind that marker rather than a second
    /// copy of it.
    pub fn session_allow(&self) -> bool {
        self.session_allow
    }

    /// Stop asking about anything for the rest of this session.
    pub fn allow_everything(&mut self) {
        self.session_allow = true;
    }

    /// Start asking again. `true` if there was a standing allow to revoke.
    ///
    /// **Per-call entries are untouched.** The two are different grants and revoking the
    /// wide one must not silently discard the narrow ones a human made deliberately —
    /// those are revoked from their own cards, one at a time, which is where they are
    /// visible.
    pub fn revoke_session_allow(&mut self) -> bool {
        let was = self.session_allow;
        self.session_allow = false;
        was
    }

    /// Remember a verdict for this exact call. Re-deciding the same call **replaces**
    /// rather than appends, so the list a human reads never shows one call twice with two
    /// answers.
    pub fn remember(&mut self, tool_name: &str, input: &str, verdict: Verdict) {
        let key = decision_key(tool_name, input);
        let entry = Remembered {
            key: key.clone(),
            tool_name: tool_name.to_string(),
            summary: summarise(tool_name, input),
            verdict,
        };
        match self.entries.iter_mut().find(|e| e.key == key) {
            Some(existing) => *existing = entry,
            None => self.entries.push(entry),
        }
    }

    /// Revoke one. `true` if there was anything to revoke.
    pub fn forget(&mut self, key: &DecisionKey) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| &e.key != key);
        self.entries.len() != before
    }

    /// Forget everything — the entries **and** the standing allow. Total on purpose: a
    /// "clear the memory" that left the widest grant standing would be the one call here
    /// that could quietly do the opposite of what its name says.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.session_allow = false;
    }

    /// Everything remembered, oldest first — the list the UI shows so a decision is never
    /// invisible.
    pub fn entries(&self) -> &[Remembered] {
        &self.entries
    }

    /// How many **per-call** decisions are held. ⚠️ Not affected by the standing allow,
    /// which is not an entry and has no card: the band carries it as its own marker rather
    /// than as one more in a count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no per-call entries. See [`Self::len`] — a session with a standing
    /// allow and no entries is still `true` here, and the band still says so.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One line describing a call, for the memory list and the card's headline.
///
/// `Bash` is special-cased because its whole meaning is in one field, and a human deciding
/// whether to permit it is reading *the command* — not a JSON object that contains one.
/// Everything else is rendered as its fields, which is as close to "the capability and its
/// arguments" as this crate can get without knowing what any tool means.
pub fn summarise(tool_name: &str, input: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(input) else {
        return one_line(input);
    };
    if tool_name == "Bash" {
        if let Some(command) = value.get("command").and_then(Value::as_str) {
            return one_line(command);
        }
    }
    match value {
        Value::Object(map) if map.is_empty() => String::from("(no arguments)"),
        Value::Object(map) => {
            let fields: Vec<String> = map
                .iter()
                .map(|(k, v)| match v {
                    Value::String(s) => format!("{k}={s}"),
                    other => format!("{k}={other}"),
                })
                .collect();
            one_line(&fields.join(" "))
        }
        other => one_line(&other.to_string()),
    }
}

fn one_line(text: &str) -> String {
    const LIMIT: usize = 120;
    let flat: String = text.replace(['\n', '\r'], "⏎");
    if flat.chars().count() <= LIMIT {
        return flat;
    }
    flat.chars().take(LIMIT).chain("…".chars()).collect()
}

// ---------------------------------------------------------------------------
// What a click means
// ---------------------------------------------------------------------------

/// Turn a click into the decision that goes back on the wire and the [`Answer`] the
/// transcript records. Pure but for the memory it writes, which is the point of extracting
/// it: the arithmetic of a permission decision is tested headlessly, and the view does
/// nothing but call this with the button that was pressed.
///
/// ⚠️ **An allow echoes the input back unchanged.** `updatedInput` is *mandatory* on an
/// allow and is re-validated against the called tool's schema (§4). The echo is the
/// degenerate case of a capability worth keeping: the field is where a card could one day
/// offer *"allow, but not that path"*, which is why the decision carries a value at all
/// rather than being a bare boolean.
pub fn resolve_choice(
    choice: Choice,
    request: &PermissionRequest,
    memory: &mut DecisionMemory,
) -> (PermissionDecision, Answer) {
    let input = request.input.to_string();
    match choice {
        Choice::Allow => (
            PermissionDecision::allow_unchanged(request),
            Answer { verdict: Verdict::Allow, by: AnsweredBy::Click, remembered: false },
        ),
        Choice::AllowAndRemember => {
            memory.remember(&request.tool_name, &input, Verdict::Allow);
            (
                PermissionDecision::allow_unchanged(request),
                Answer { verdict: Verdict::Allow, by: AnsweredBy::Click, remembered: true },
            )
        }
        // ⚠️ **This call is answered by the click, not by the grant it just created.** The
        // card reads `allowed` with no "from a decision you already made", because there was
        // none when the question was asked — the standing allow starts with the *next* call.
        // `remembered: false` for the same reason and a sharper one: nothing about this call
        // went into the per-call memory, so its card must not offer a `forget` that would
        // find nothing to forget. The revocation for what was just granted is on the band.
        Choice::AllowEverythingThisSession => {
            memory.allow_everything();
            (
                PermissionDecision::allow_unchanged(request),
                Answer { verdict: Verdict::Allow, by: AnsweredBy::Click, remembered: false },
            )
        }
        Choice::Deny => (
            PermissionDecision::deny(DENIED),
            Answer { verdict: Verdict::Deny, by: AnsweredBy::Click, remembered: false },
        ),
    }
}

/// The decision a remembered verdict produces, for the auto-answer path.
pub fn decision_for(verdict: Verdict, request: &PermissionRequest) -> PermissionDecision {
    match verdict {
        Verdict::Allow => PermissionDecision::allow_unchanged(request),
        Verdict::Deny => PermissionDecision::deny(DENIED),
    }
}

/// The whole auto-answer, as the pane needs it: the wire decision and the [`Answer`] the
/// card records, from one [`Recall`].
///
/// Paired with [`resolve_choice`] deliberately — the two ways a question can end are the
/// two functions here, and neither of them draws anything.
pub fn resolve_recall(recall: Recall, request: &PermissionRequest) -> (PermissionDecision, Answer) {
    let verdict = recall.verdict();
    (
        decision_for(verdict, request),
        Answer {
            verdict,
            by: recall.answered_by(),
            // Only a per-call decision is "remembered" in the sense the card's `forget`
            // acts on. A standing allow has no entry to revoke — see [`Answer::remembered`].
            remembered: matches!(recall, Recall::ThisCall(_)),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::NoHeartbeat;
    use serde_json::json;

    fn request(tool: &str, input: Value) -> PermissionRequest {
        PermissionRequest {
            tool_name: tool.to_string(),
            input,
            tool_use_id: "toolu_test".into(),
        }
    }

    fn bash(command: &str) -> PermissionRequest {
        request("Bash", json!({ "command": command }))
    }

    /// **The shape the whole feature rests on**: the hook blocks on a thread that is not
    /// the UI's, the UI answers whenever it likes, and the blocked call returns that
    /// answer. Modelled with a real thread, because the property is a threading one.
    #[test]
    fn the_hook_blocks_until_the_ui_answers_it() {
        let (mut gate, inbox) = approval_channel();
        let asked = std::thread::spawn(move || gate.decide(&bash("cargo test"), &mut NoHeartbeat));

        let pending = inbox.recv_timeout(std::time::Duration::from_secs(5)).expect("a question");
        assert_eq!(pending.request().tool_name, "Bash");
        assert_eq!(pending.request().input["command"], "cargo test");
        assert_eq!(pending.request().tool_use_id, "toolu_test");
        let echo = PermissionDecision::allow_unchanged(pending.request());
        pending.answer(echo);

        let decision = asked.join().expect("the serve thread returned");
        assert_eq!(
            decision.to_wire(),
            r#"{"behavior":"allow","updatedInput":{"command":"cargo test"}}"#
        );
    }

    /// ⚠️ Dropping a question denies it. This is what makes an approval the transcript's
    /// cap evicted fail **closed** rather than hang the agent for the rest of the session.
    #[test]
    fn a_dropped_question_is_denied_not_hung() {
        let (mut gate, inbox) = approval_channel();
        let asked = std::thread::spawn(move || gate.decide(&bash("rm -rf /"), &mut NoHeartbeat));
        let pending = inbox.recv_timeout(std::time::Duration::from_secs(5)).expect("a question");
        drop(pending);
        let decision = asked.join().unwrap();
        assert_eq!(decision, PermissionDecision::deny(UNREACHABLE));
    }

    /// A heartbeat that counts, and can be told the client has gone.
    struct Fake {
        beats: Arc<std::sync::atomic::AtomicUsize>,
        /// The beat on which the client disappears. `usize::MAX` for one that never does.
        dies_at: usize,
    }

    impl Heartbeat for Fake {
        fn beat(&mut self) -> bool {
            self.beats.fetch_add(1, Ordering::Relaxed) + 1 < self.dies_at
        }
    }

    fn fast(gate: ApprovalGate) -> ApprovalGate {
        gate.beating_every(std::time::Duration::from_millis(10))
    }

    /// 🚨 **The bug, headless.** Claude Code gives up on a permission call after
    /// [`crate::mcp::CLIENT_DEADLINE_SECS`], so a question that outlives its keep-alives is a
    /// question nobody will read the answer to. The gate must then resolve **once**, in the
    /// **deny** direction, and — the part a human sees — mark the pending so the card can
    /// stop asking.
    #[test]
    fn a_question_the_client_gave_up_on_resolves_once_and_fails_closed() {
        let (gate, inbox) = approval_channel();
        let beats = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fake = Fake { beats: beats.clone(), dies_at: 3 };
        let mut gate = fast(gate);
        let asked = std::thread::spawn(move || gate.decide(&bash("rm -rf /"), &mut fake));

        let pending = inbox.recv_timeout(std::time::Duration::from_secs(5)).expect("a question");
        let decision = asked.join().expect("the serve thread returned");

        assert_eq!(decision, PermissionDecision::deny(ABANDONED));
        assert!(
            !matches!(decision, PermissionDecision::Allow { .. }),
            "a request the console can no longer answer is never an allow"
        );
        assert_eq!(beats.load(Ordering::Relaxed), 3, "it beat until the client stopped answering");
        assert!(pending.is_abandoned(), "the card must be able to stop asking");

        // …and it resolved **once**. A late click cannot produce a second decision: the
        // gate has already returned, so this is a no-op rather than a second verdict on a
        // wire nobody is reading.
        pending.answer(PermissionDecision::deny(DENIED));
    }

    /// The other half of the same fix: while the client is still there, a slow human is
    /// simply waited for — past the deadline, for as long as they take.
    #[test]
    fn a_slow_human_is_waited_for_and_their_answer_is_the_one_that_counts() {
        let (gate, inbox) = approval_channel();
        let beats = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fake = Fake { beats: beats.clone(), dies_at: usize::MAX };
        let mut gate = fast(gate);
        let asked = std::thread::spawn(move || gate.decide(&bash("cargo build"), &mut fake));

        let pending = inbox.recv_timeout(std::time::Duration::from_secs(5)).expect("a question");
        // Take longer than several heartbeats — the shape of a human reading a card.
        while beats.load(Ordering::Relaxed) < 4 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!pending.is_abandoned(), "nothing has gone wrong; the human is just slow");
        pending.answer(PermissionDecision::allow_unchanged(&bash("cargo build")));

        let decision = asked.join().expect("the serve thread returned");
        assert_eq!(
            decision.to_wire(),
            r#"{"behavior":"allow","updatedInput":{"command":"cargo build"}}"#,
            "the answer a human gave late is still the answer"
        );
    }

    /// And so is a question nobody is listening for at all — a pane closed while its agent
    /// was still running.
    #[test]
    fn a_gate_with_no_listener_denies_immediately() {
        let (mut gate, inbox) = approval_channel();
        drop(inbox);
        assert_eq!(gate.decide(&bash("ls"), &mut NoHeartbeat), PermissionDecision::deny(UNREACHABLE));
    }

    /// The measured repetition (§5): three identical calls, one decision. And the bound on
    /// it — a *different* call is a different question.
    #[test]
    fn remembering_answers_the_identical_call_and_only_that_one() {
        let mut memory = DecisionMemory::new();
        let first = bash("cargo test -p organon-shell");
        assert_eq!(memory.lookup("Bash", &first.input.to_string()), None);

        let (decision, answer) = resolve_choice(Choice::AllowAndRemember, &first, &mut memory);
        assert!(matches!(decision, PermissionDecision::Allow { .. }));
        assert_eq!(answer, Answer { verdict: Verdict::Allow, by: AnsweredBy::Click, remembered: true });

        assert_eq!(memory.lookup("Bash", &first.input.to_string()), Some(Verdict::Allow));
        let other = bash("cargo test -p organon-core");
        assert_eq!(memory.lookup("Bash", &other.input.to_string()), None, "a different command is a different question");
        assert_eq!(memory.lookup("Write", &first.input.to_string()), None, "and so is a different tool");
    }

    /// Key order is not identity. A memory that thought otherwise would re-ask a decision
    /// the human had already made, which reads exactly like the feature not working.
    #[test]
    fn argument_order_does_not_make_one_call_look_like_two() {
        let a = decision_key("Write", r#"{"file_path":"a.txt","content":"x"}"#);
        let b = decision_key("Write", r#"{"content":"x","file_path":"a.txt"}"#);
        assert_eq!(a, b);
        assert_ne!(a, decision_key("Write", r#"{"content":"y","file_path":"a.txt"}"#));
        // Nesting and arrays canonicalise too, and unparseable text keys on itself.
        assert_eq!(
            decision_key("T", r#"{"o":{"b":1,"a":[2,{"d":4,"c":3}]}}"#),
            decision_key("T", r#"{"o":{"a":[2,{"c":3,"d":4}],"b":1}}"#)
        );
        assert_ne!(decision_key("T", "not json"), decision_key("T", "other junk"));
        assert_eq!(decision_key("T", " not json "), decision_key("T", "not json"));
    }

    /// A remembered **deny** auto-denies. Remembering only allows would make the list
    /// half a memory and the card half a decision.
    #[test]
    fn a_remembered_deny_is_a_deny() {
        let mut memory = DecisionMemory::new();
        let call = request("mcp__probe__echo_probe", json!({ "text": "ALPHA" }));
        memory.remember(&call.tool_name, &call.input.to_string(), Verdict::Deny);
        let verdict = memory.lookup(&call.tool_name, &call.input.to_string()).unwrap();
        assert_eq!(verdict, Verdict::Deny);
        assert_eq!(decision_for(verdict, &call), PermissionDecision::deny(DENIED));
        assert_eq!(
            decision_for(Verdict::Allow, &call).to_wire(),
            r#"{"behavior":"allow","updatedInput":{"text":"ALPHA"}}"#,
            "updatedInput is mandatory on an allow, and echoing is the ordinary case"
        );
    }

    /// Revocation, and the list it works from. A remembered decision a human cannot see or
    /// undo is worse than being asked every time.
    #[test]
    fn the_memory_is_a_list_a_human_can_read_and_revoke_from() {
        let mut memory = DecisionMemory::new();
        let build = bash("cargo build");
        let write = request("Write", json!({ "file_path": "C:/tmp/notes.txt", "content": "hi" }));
        memory.remember(&build.tool_name, &build.input.to_string(), Verdict::Allow);
        memory.remember(&write.tool_name, &write.input.to_string(), Verdict::Deny);

        assert_eq!(memory.len(), 2);
        assert_eq!(memory.entries()[0].tool_name, "Bash");
        assert_eq!(memory.entries()[0].summary, "cargo build", "a Bash entry reads as its command");
        assert_eq!(memory.entries()[1].tool_name, "Write");
        assert!(memory.entries()[1].summary.contains("file_path=C:/tmp/notes.txt"));

        // Re-deciding the same call replaces it; the list never shows one call twice.
        memory.remember(&build.tool_name, &build.input.to_string(), Verdict::Deny);
        assert_eq!(memory.len(), 2);
        assert_eq!(memory.lookup("Bash", &build.input.to_string()), Some(Verdict::Deny));

        let key = decision_key(&build.tool_name, &build.input.to_string());
        assert!(memory.forget(&key));
        assert!(!memory.forget(&key), "forgetting twice is not an error, it is a no-op");
        assert_eq!(memory.lookup("Bash", &build.input.to_string()), None);
        assert_eq!(memory.len(), 1);

        memory.clear();
        assert!(memory.is_empty());
    }

    /// A plain allow or deny remembers **nothing** — the third button is the only thing
    /// that writes to the memory, which is what keeps "remember" a deliberate act.
    #[test]
    fn only_the_third_button_writes_to_the_memory() {
        let mut memory = DecisionMemory::new();
        let call = bash("git push --force");

        let (decision, answer) = resolve_choice(Choice::Allow, &call, &mut memory);
        assert_eq!(decision.to_wire(), r#"{"behavior":"allow","updatedInput":{"command":"git push --force"}}"#);
        assert!(!answer.remembered);
        assert_eq!(answer.by, AnsweredBy::Click);
        assert!(memory.is_empty());

        let (decision, answer) = resolve_choice(Choice::Deny, &call, &mut memory);
        assert_eq!(decision, PermissionDecision::deny(DENIED));
        assert_eq!(answer.verdict, Verdict::Deny);
        assert!(!answer.remembered);
        assert!(memory.is_empty(), "a deny is not remembered unless it was asked to be");
    }

    // -- allow everything for this session ---------------------------------

    /// **The feature, in one test.** A call that would have raised a card is answered
    /// instead — and the console can still say, in the transcript and on the band, that it
    /// was the standing allow and not a decision about that call.
    #[test]
    fn a_session_allow_answers_a_call_that_would_otherwise_ask() {
        let mut memory = DecisionMemory::new();
        let call = bash("cargo build --release");
        assert_eq!(memory.recall("Bash", &call.input.to_string()), None, "it would have asked");
        assert!(!memory.session_allow());

        let (decision, answer) =
            resolve_choice(Choice::AllowEverythingThisSession, &call, &mut memory);
        assert!(matches!(decision, PermissionDecision::Allow { .. }));
        assert!(memory.session_allow(), "the grant is standing from here");
        assert_eq!(
            answer,
            Answer { verdict: Verdict::Allow, by: AnsweredBy::Click, remembered: false },
            "the call in front of the human was answered by the click that granted it"
        );

        // The next call — any call, any tool — is answered without asking.
        let other = request("Write", json!({ "file_path": "C:/tmp/x", "content": "y" }));
        let recall = memory.recall(&other.tool_name, &other.input.to_string()).expect("answered");
        assert_eq!(recall, Recall::SessionAllow);
        let (decision, answer) = resolve_recall(recall, &other);
        assert_eq!(
            decision.to_wire(),
            r#"{"behavior":"allow","updatedInput":{"content":"y","file_path":"C:/tmp/x"}}"#
        );
        assert_eq!(answer.verdict, Verdict::Allow);
        assert_eq!(answer.by, AnsweredBy::SessionAllow);
    }

    /// 🚨 **The two standing sources must stay distinguishable, all the way to the card.**
    /// They are revoked in different places — a per-call decision from its own card, a
    /// session allow from the band — so a card that could not say which would send a reader
    /// looking for a `forget` button that is not there.
    #[test]
    fn a_session_allow_is_recorded_differently_from_a_per_call_remember() {
        let mut memory = DecisionMemory::new();
        let remembered = bash("cargo test -p organon-shell");
        resolve_choice(Choice::AllowAndRemember, &remembered, &mut memory);
        memory.allow_everything();

        let by_call = memory.recall("Bash", &remembered.input.to_string()).unwrap();
        let by_session = memory.recall("Bash", &bash("cargo doc").input.to_string()).unwrap();
        assert_eq!(by_call, Recall::ThisCall(Verdict::Allow));
        assert_eq!(by_session, Recall::SessionAllow);
        assert_ne!(by_call.answered_by(), by_session.answered_by());

        // …and the difference survives into the answer the transcript keeps: only the
        // per-call one is `remembered`, which is what puts a `forget` button on its card.
        assert!(resolve_recall(by_call, &remembered).1.remembered);
        assert!(!resolve_recall(by_session, &remembered).1.remembered);
        assert_eq!(memory.len(), 1, "the standing allow is not an entry and is not counted");
    }

    /// ⚠️ **A specific refusal outranks the blanket grant.** Clicking "allow everything"
    /// must not silently overturn a deny a human made deliberately about one call — that
    /// call keeps its own card, its own verdict and its own `forget`.
    #[test]
    fn a_remembered_deny_survives_a_session_allow() {
        let mut memory = DecisionMemory::new();
        let dangerous = bash("git push --force");
        memory.remember(&dangerous.tool_name, &dangerous.input.to_string(), Verdict::Deny);
        memory.allow_everything();

        let recall = memory.recall("Bash", &dangerous.input.to_string()).unwrap();
        assert_eq!(recall, Recall::ThisCall(Verdict::Deny));
        assert_eq!(resolve_recall(recall, &dangerous).0, PermissionDecision::deny(DENIED));
        // Everything else is still allowed — the narrow rule bounds one call, not the grant.
        assert_eq!(
            memory.recall("Bash", &bash("git status").input.to_string()),
            Some(Recall::SessionAllow)
        );
    }

    /// **Revocation, from the band.** The grant is the one thing here with no card of its
    /// own, so it needs a revoke that does not require finding one — and revoking it must
    /// leave the per-call decisions a human made alone.
    #[test]
    fn revoking_a_session_allow_restores_prompting_and_keeps_the_entries() {
        let mut memory = DecisionMemory::new();
        let kept = bash("cargo build");
        memory.remember(&kept.tool_name, &kept.input.to_string(), Verdict::Allow);
        memory.allow_everything();

        assert!(memory.revoke_session_allow());
        assert!(!memory.session_allow());
        assert!(!memory.revoke_session_allow(), "revoking twice is a no-op, not an error");
        assert_eq!(
            memory.recall("Bash", &bash("rm -rf /").input.to_string()),
            None,
            "a call nobody decided asks a human again"
        );
        assert_eq!(
            memory.recall("Bash", &kept.input.to_string()),
            Some(Recall::ThisCall(Verdict::Allow)),
            "the narrow decisions are a different grant and are untouched"
        );

        // `clear` is the total one — it takes the standing allow with it.
        memory.allow_everything();
        memory.clear();
        assert!(!memory.session_allow());
        assert!(memory.is_empty());
    }

    /// A card has to show *what* is being permitted, so the summary must survive the
    /// shapes that would otherwise make it useless — a multi-line command, a huge one, and
    /// a call with no arguments at all.
    #[test]
    fn a_summary_stays_one_readable_line() {
        assert_eq!(summarise("Bash", &json!({"command":"a\nb"}).to_string()), "a⏎b");
        let huge = json!({ "command": "x".repeat(500) }).to_string();
        let line = summarise("Bash", &huge);
        assert!(line.ends_with('…'));
        assert_eq!(line.chars().count(), 121);
        assert_eq!(summarise("Read", "{}"), "(no arguments)");
        assert_eq!(summarise("Bash", "not json"), "not json", "unparseable input is shown, not hidden");
        assert_eq!(
            summarise("mcp__organon__background", &json!({"arg":"metal"}).to_string()),
            "arg=metal",
            "an MCP capability reads as its named arguments"
        );
    }
}
