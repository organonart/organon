//! One live Claude Code process, spoken to over stdio (Console Spike §5.9.2).
//!
//! This is the conversation view's equivalent of [`crate::term::TermSession`], and it is
//! deliberately built to the same shape: a **reader thread that only moves bytes**, a
//! channel, and a **pull** drained once per frame by the UI thread. Nothing is shared and
//! nothing is locked; the cost is at most one frame of latency, which the continuous
//! redraw loop already pays.
//!
//! # There is no PTY here, and that is the point
//!
//! A terminal tab exists because a program paints a character grid. This tab exists
//! because a program emits **structured events**, so the transport is a plain pipe:
//! NDJSON out of the child's stdout, NDJSON into its stdin. No ConPTY, therefore none of
//! ConPTY's rewriting (§5.9's measurement), no VT parser, no grid.
//!
//! # One process per tab, alive across every turn
//!
//! 🚨 **Measured on this machine (§5.9.2): `-p --input-format stream-json` keeps ONE
//! session alive across many turns.** One `session_id`, and a `result` object **per
//! turn** — two of them in a two-turn run. So `result` is a *turn* terminator, the stream
//! continues past it, and anything that treats it as end-of-stream closes a live
//! conversation after its first exchange. This type therefore never reacts to `result` at
//! all: it ends when the pipe ends.
//!
//! Resume is the recovery path, not the interaction model. Spawn once per tab, write user
//! turns to stdin, and never let the process go.
//!
//! # Failing visibly
//!
//! If `claude` is not on `PATH`, or will not start, [`AgentSession::spawn`] returns a
//! [`SpawnError`] whose `Display` names the binary it looked for and what to do about it.
//! A conversation tab that silently shows an empty transcript is indistinguishable from
//! an agent that has not answered yet, which is the worst available failure.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::agent_event::{AgentEvent, ControlResponse, EventStream};
use crate::platform::{self, Platform};

/// The binary a conversation tab drives, unless `$ORGANON_CLAUDE_BIN` overrides it.
pub const DEFAULT_BINARY: &str = "claude";

/// The flags §5.9.1 and §5.9.2 settled on, and why each is not optional:
///
/// * `-p` — non-interactive; the CLI reads a program's input, not a human's terminal.
/// * `--input-format stream-json` — turns `-p` from a one-shot into a **live session**
///   fed from stdin. This is the flag that makes the process persist.
/// * `--output-format stream-json` — the structured event stream this whole front-end
///   exists to render.
/// * `--include-partial-messages` — token-level deltas. Without it, prose arrives only
///   as complete blocks and nothing streams.
/// * `--replay-user-messages` — echoes injected input back into the output stream, which
///   is what lets the composer render nothing locally (§5.9.3 rule 2).
/// * `--verbose` — required alongside `-p` with stream-json output; without it the CLI
///   refuses.
pub const ARGS: &[&str] = &[
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--include-partial-messages",
    "--replay-user-messages",
    "--verbose",
];

/// How this session reaches the console's own MCP server — the wiring that turns a
/// permission request into a card instead of a red error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpWiring {
    /// The `--mcp-config` document, written per session because it carries an ephemeral
    /// port ([`crate::mcp_http::ConfigFile`]).
    pub config: PathBuf,
    /// The handler's fully namespaced name, exactly as
    /// [`crate::mcp::McpServer::permission_tool_flag_value`] spells it.
    pub permission_tool: String,
}

/// The three flags that route approvals through the console (§2, §8).
///
/// * `--mcp-config <file>` — the `http` entry the client connects **out** to.
/// * `--strict-mcp-config` — so the user's *other* MCP servers are not pulled into this
///   session as a side effect of us adding one.
/// * `--permission-prompt-tool mcp__organon__…` — the decisive one. It is **absent from
///   `--help`** on 2.1.228 but present in the binary, it requires `--print` (which
///   [`ARGS`] already passes as `-p`), and it gates **`Bash` as well as MCP tools** — so
///   the console answers for everything the agent does, not only for its own verbs.
///
/// 🚨 Naming the handler here is also what *protects* it: Claude Code removes the named
/// tool from the model's own tool set, so the model cannot hand itself
/// `{"behavior":"allow"}` (§7). The guarantee is tied to this flag, which is why the
/// console must never serve a second approval-shaped tool.
pub fn mcp_args(wiring: &McpWiring) -> Vec<String> {
    vec![
        "--mcp-config".to_string(),
        wiring.config.display().to_string(),
        "--strict-mcp-config".to_string(),
        "--permission-prompt-tool".to_string(),
        wiring.permission_tool.clone(),
    ]
}

// ---------------------------------------------------------------------------
// The control protocol — the console asking the session to change
// ---------------------------------------------------------------------------
//
// `doc/console_session_control_protocol.md` §1 is the measurement this section is built
// to: the console writes `{"type":"control_request","request_id":…,"request":{…}}` on the
// **same stdin it sends turns down**, and the answer comes back on the same stdout it
// reads events from, as an `EventKind::ControlResponse` correlated only by the
// `request_id` the console invented. `set_model` acked in 272 ms, `set_permission_mode`
// in 17 ms, and **no `initialize` handshake was required first** — in one probe a control
// request was answered before that session's own first `system/init`.
//
// 📌 **Correlation is the console's whole problem here.** A response carries a
// `request_id` and *nothing else* saying which verb it answers — `set_model`'s ack has no
// body at all. That is why [`ControlDesk`] exists: it is the only place that knows an id
// means "the model change", and it is the reason `agent_map.rs` deliberately records no
// fact from an ack (it never issued the request and cannot tell).

/// A control verb the console sends. Three, because three are what the strip can act on;
/// the CLI accepts twelve (§1) and the other nine are somebody else's tier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    /// `set_model` — takes an alias (`sonnet`), a full id, or `default` to reset.
    SetModel(String),
    /// `set_permission_mode` — the one verb whose ack states its own result.
    SetPermissionMode(String),
    /// `initialize` — asked **once at spawn**, purely for its `models` array.
    ///
    /// ⚠️ Once, and at spawn, on purpose. §3 measured it safe when sent **last** in a
    /// session and explicitly did *not* establish the general mid-session case; it is
    /// also a heavy answer (23 824 bytes on one line in the capture). Asking before the
    /// first turn is the honest reading of what was measured.
    Initialize,
}

impl Control {
    /// The `subtype` this verb goes out under. Also the middle of its request id, which is
    /// what makes a raw capture of the pipe readable.
    pub fn subtype(&self) -> &'static str {
        match self {
            Control::SetModel(_) => "set_model",
            Control::SetPermissionMode(_) => "set_permission_mode",
            Control::Initialize => "initialize",
        }
    }

    /// How a log line names this request — used when one is never answered, where the
    /// whole value of the sentence is that it says *which* control went unanswered.
    pub fn describe(&self) -> String {
        match self {
            Control::SetModel(model) => format!("the model change to {model}"),
            Control::SetPermissionMode(mode) => format!("the permission-mode change to {mode}"),
            Control::Initialize => "the model-list request".to_string(),
        }
    }

    /// The `request` object, exactly as §1 and §8 quote it.
    fn request(&self) -> serde_json::Value {
        match self {
            Control::SetModel(model) => {
                serde_json::json!({ "subtype": "set_model", "model": model })
            }
            Control::SetPermissionMode(mode) => {
                serde_json::json!({ "subtype": "set_permission_mode", "mode": mode })
            }
            Control::Initialize => serde_json::json!({ "subtype": "initialize" }),
        }
    }
}

/// The exact bytes one control request goes down the pipe as.
///
/// Pure, for the same reason [`user_message_line`] is: the wire shape is then a test
/// against the sentence the protocol doc quotes from a live capture, rather than a claim.
/// A typo in a subtype produces a `Unsupported control request subtype: …` error the user
/// would see as "the picker does nothing", which is exactly the failure a pinned shape
/// prevents.
pub fn control_request_line(request_id: &str, control: &Control) -> String {
    let value = serde_json::json!({
        "type": "control_request",
        "request_id": request_id,
        "request": control.request(),
    });
    format!("{value}\n")
}

/// Request ids, unique for the life of the process.
///
/// The CLI echoes whatever it is given and a `"req-model-1"` that was not a UUID was
/// accepted (§1), so the only requirement is that two live requests never collide — an
/// id that repeated would let one verb's ack resolve another's. Process-wide rather than
/// per-session so that two conversation tabs' captures cannot be confused either.
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

fn next_request_id(subtype: &str) -> String {
    let n = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    format!("organon-{subtype}-{n}")
}

/// How long the console waits for an ack before it stops expecting one.
///
/// 🚨 **This deadline exists so that nothing is ever gated on an ack.** The console has
/// been bitten by the opposite arrangement before — `doc/console_approval_protocol.md`
/// §4b records a 60 s client-side deadline nobody knew about, discovered because a card
/// kept offering *allow* for a call that had already failed. Here the rule is simpler: a
/// control request is **fire-and-observe**. The composer, the transcript and the strip
/// never wait on one; the only thing an ack does is *release* a pending marker, and this
/// deadline releases it anyway.
///
/// Twenty seconds, and the number is set by the **slowest** request rather than the
/// fastest: `set_model` acked in 272 ms and `set_permission_mode` in 17 ms, but
/// [`Control::Initialize`] goes out at spawn, where §6 measured a **1.3–3.3 s** band to a
/// session's first announcement while MCP servers and skills warm up. Twenty is ~6× the
/// top of that band — long enough that a cold spawn is never accused of having dropped
/// the request, short enough that a plate does not sit marked "switching" into the next
/// conversation.
pub const CONTROL_DEADLINE: Duration = Duration::from_secs(20);

/// One control request the console has written and not yet seen answered.
#[derive(Clone, Debug, PartialEq, Eq)]
struct InFlight {
    id: String,
    control: Control,
    sent_at: Instant,
}

/// The in-flight control requests, and the only thing that knows what an ack answers.
///
/// Split from [`AgentSession`] so the correlation — which is the whole hazard — is
/// testable without spawning a process: an id issued, an ack matched, an ack that belongs
/// to nobody, and a request that is never answered at all.
#[derive(Debug, Default)]
pub struct ControlDesk {
    inflight: Vec<InFlight>,
}

impl ControlDesk {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take an id for `control` and hand back the line to write.
    ///
    /// Recorded as in flight *before* the write, so a write that fails half way still
    /// leaves an entry the deadline will clear — a request that reached the CLI and one
    /// that did not are indistinguishable from this side, and the safe reading is that it
    /// might have.
    pub fn issue(&mut self, control: Control, now: Instant) -> (String, String) {
        let id = next_request_id(control.subtype());
        let line = control_request_line(&id, &control);
        self.inflight.push(InFlight { id: id.clone(), control, sent_at: now });
        (id, line)
    }

    /// Which verb this response answers, or `None` if it answers nothing this console
    /// asked for.
    ///
    /// ⚠️ `None` is **expected traffic, not a fault**: a response with no `request_id`, or
    /// one carrying an id from a request the deadline already retired, both land here. The
    /// caller logs and moves on rather than treating it as an error.
    pub fn resolve(&mut self, response: &ControlResponse) -> Option<Control> {
        let id = response.request_id.as_deref()?;
        let at = self.inflight.iter().position(|p| p.id == id)?;
        Some(self.inflight.remove(at).control)
    }

    /// Everything still unanswered past `deadline`, removed and handed back.
    ///
    /// The no-reply path, and it is a *sweep* rather than a timer: the pane already pumps
    /// every frame, there is no thread to park, and nothing anywhere is blocked waiting
    /// for what this returns.
    pub fn give_up(&mut self, now: Instant, deadline: Duration) -> Vec<Control> {
        let mut abandoned = Vec::new();
        self.inflight.retain(|p| {
            if now.duration_since(p.sent_at) >= deadline {
                abandoned.push(p.control.clone());
                false
            } else {
                true
            }
        });
        abandoned
    }

    /// How many requests are outstanding. For tests and diagnostics.
    pub fn outstanding(&self) -> usize {
        self.inflight.len()
    }
}

/// One line off the child, already classified.
#[derive(Debug, Clone)]
pub enum StreamItem {
    /// A decoded event.
    Event(Box<AgentEvent>),
    /// A line that was not a decodable event. **Expected traffic, not a failure**: the
    /// first line of a real run is `Warning: no stdin data received in 3s…`, plain text
    /// on the same pipe (§5.9.3 rule 6). Surfaced so it can be logged rather than
    /// swallowed.
    Noise(String),
    /// A line the child wrote to stderr. Where an auth failure or a bad flag announces
    /// itself, so it is carried rather than dropped.
    Stderr(String),
    /// stdout closed — the process is done talking.
    Eof,
}

/// Why a conversation tab could not start. Every variant says what to do.
#[derive(Debug)]
pub enum SpawnError {
    /// No `claude` on `PATH`.
    NotFound { binary: String },
    /// It was found and would not start.
    Launch { binary: String, source: std::io::Error },
    /// The child started but gave us no pipe to talk over.
    NoPipes { binary: String },
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpawnError::NotFound { binary } => write!(
                f,
                "{binary} is not on PATH — a conversation tab drives the Claude Code CLI \
                 directly. Install it (https://claude.com/claude-code), or set \
                 $ORGANON_CLAUDE_BIN to its full path, then open the tab again."
            ),
            SpawnError::Launch { binary, source } => write!(
                f,
                "{binary} would not start: {source}. Run `{binary} --version` in a \
                 terminal tab — if that fails, the conversation tab cannot work either."
            ),
            SpawnError::NoPipes { binary } => {
                write!(f, "{binary} started without usable stdin/stdout pipes")
            }
        }
    }
}

impl std::error::Error for SpawnError {}

/// A live agent process and its event stream.
pub struct AgentSession {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<StreamItem>,
    /// Set when stdout reaches EOF. The transcript stays readable — the same contract
    /// [`crate::term::TermSession::exited`] keeps for a dead shell's grid.
    pub exited: bool,
    /// What was actually launched, for the status line and for diagnostics.
    pub command: String,
    /// The control requests this console has written and not yet seen answered.
    desk: ControlDesk,
}

impl AgentSession {
    /// Spawn the agent in `cwd` (the app's own directory when `None`), routing permission
    /// prompts through `mcp` when there is one.
    ///
    /// ⚠️ `None` is a **degraded** session, not a plain one: nothing answers approvals, so
    /// any tool that needs permission bounces as a red error card — the exact failure this
    /// wiring exists to remove. The caller says so rather than letting it look like the
    /// agent's own fault.
    pub fn spawn(cwd: Option<&str>, mcp: Option<&McpWiring>) -> Result<Self, SpawnError> {
        let binary = std::env::var("ORGANON_CLAUDE_BIN")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BINARY.to_string());
        let resolved = resolve(&binary).ok_or_else(|| SpawnError::NotFound {
            binary: binary.clone(),
        })?;

        let extra: Vec<String> = mcp.map(mcp_args).unwrap_or_default();
        let mut cmd = launch_command(&resolved);
        cmd.args(ARGS);
        cmd.args(&extra);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        // The console is a windowed process, so a console child would otherwise flash
        // (and on some shells, steal) a window of its own.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn().map_err(|source| SpawnError::Launch {
            binary: binary.clone(),
            source,
        })?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            return Err(SpawnError::NoPipes { binary });
        };

        let (tx, rx) = mpsc::channel::<StreamItem>();

        // stdout: bytes → whole lines → events. `EventStream` owns the line buffering,
        // so a chunk split mid-line (the normal case — one `tool_result` can carry a
        // whole file) cannot produce a wrong answer.
        let out_tx = tx.clone();
        std::thread::Builder::new()
            .name("agent-stdout".into())
            .spawn(move || {
                let mut stream = EventStream::new();
                let mut reader = stdout;
                let mut buf = [0u8; 65536];
                loop {
                    use std::io::Read;
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            for outcome in stream.push(&buf[..n]) {
                                let item = classify(outcome);
                                if out_tx.send(item).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
                // A final line with no trailing newline is a short line, not an
                // unfinished one, once the pipe has closed.
                if let Some(outcome) = stream.flush() {
                    let _ = out_tx.send(classify(outcome));
                }
                let _ = out_tx.send(StreamItem::Eof);
            })
            .map_err(|source| SpawnError::Launch {
                binary: binary.clone(),
                source,
            })?;

        let err_tx = tx;
        std::thread::Builder::new()
            .name("agent-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { break };
                    if err_tx.send(StreamItem::Stderr(line)).is_err() {
                        return;
                    }
                }
            })
            .map_err(|source| SpawnError::Launch {
                binary: binary.clone(),
                source,
            })?;

        let mut session = Self {
            child,
            stdin: Some(stdin),
            rx,
            exited: false,
            command: format!("{} {} {}", resolved.display(), ARGS.join(" "), extra.join(" "))
                .trim_end()
                .to_string(),
            desk: ControlDesk::new(),
        };
        // **Ask for the model list once, here, and never again** — see
        // [`Control::Initialize`] for why "once at spawn" is what the measurement
        // supports. Ignored on failure: a session whose list never arrives still runs, and
        // the picker says the list has not arrived rather than inventing one.
        //
        // 📌 A side effect worth knowing rather than discovering: §6 measured that
        // `system/init` is emitted only once input is pending, so a tab nobody has typed
        // into never announced itself at all. This line is input, so the strip now learns
        // its model at spawn instead of at the first human turn.
        let _ = session.send_control(Control::Initialize);
        Ok(session)
    }

    /// Everything the child has said since the last call. Never blocks.
    pub fn pump(&mut self) -> Vec<StreamItem> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(StreamItem::Eof) => {
                    self.exited = true;
                    out.push(StreamItem::Eof);
                }
                Ok(item) => out.push(item),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.exited = true;
                    break;
                }
            }
        }
        out
    }

    /// Send one human turn.
    ///
    /// ⚠️ The caller renders **nothing** for this — the CLI echoes it back on the output
    /// stream under `--replay-user-messages`, and that echo is what the transcript
    /// draws (§5.9.3 rule 2). Rendering it locally as well would double every message
    /// and invite an ordering bug the replay exists to prevent.
    pub fn send_user(&mut self, text: &str) -> std::io::Result<()> {
        let line = user_message_line(text);
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "agent stdin is closed",
            ));
        };
        stdin.write_all(line.as_bytes())?;
        stdin.flush()
    }

    /// Ask the session to change something about itself. Returns the `request_id` the
    /// request went out under.
    ///
    /// ⚠️ **Nothing waits on the answer.** The ack arrives later as an ordinary
    /// [`crate::agent_event::EventKind::ControlResponse`] on the stream everything else
    /// arrives on, and is matched back to this verb by [`resolve_control`](Self::resolve_control).
    /// If it never arrives, [`give_up_on_controls`](Self::give_up_on_controls) retires the
    /// request at [`CONTROL_DEADLINE`] and the caller un-marks whatever it marked. There
    /// is no blocking path here to get wrong.
    pub fn send_control(&mut self, control: Control) -> std::io::Result<String> {
        let (id, line) = self.desk.issue(control, Instant::now());
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "agent stdin is closed",
            ));
        };
        stdin.write_all(line.as_bytes())?;
        stdin.flush()?;
        Ok(id)
    }

    /// Which verb `response` answers — `None` when it answers nothing this session asked
    /// for.
    pub fn resolve_control(&mut self, response: &ControlResponse) -> Option<Control> {
        self.desk.resolve(response)
    }

    /// Control requests that have gone unanswered long enough to stop expecting an answer.
    pub fn give_up_on_controls(&mut self, now: Instant) -> Vec<Control> {
        self.desk.give_up(now, CONTROL_DEADLINE)
    }

    /// How many control requests are outstanding. Diagnostics only — nothing gates on it.
    pub fn controls_outstanding(&self) -> usize {
        self.desk.outstanding()
    }
}

impl Drop for AgentSession {
    fn drop(&mut self) {
        // Closing stdin is the polite ask; the kill is what guarantees a closed tab does
        // not leave an agent running against the user's repository.
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn classify(outcome: Result<AgentEvent, crate::agent_event::DecodeError>) -> StreamItem {
    match outcome {
        Ok(event) => StreamItem::Event(Box::new(event)),
        Err(error) => StreamItem::Noise(error.to_string()),
    }
}

/// The exact bytes one human turn goes down the pipe as.
///
/// Pure, so the wire shape is a test rather than a claim — and built through
/// `serde_json` rather than by formatting, because a message containing a quote or a
/// newline would otherwise produce a line the CLI cannot parse, and the symptom would be
/// silence.
pub fn user_message_line(text: &str) -> String {
    let value = serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{ "type": "text", "text": text }],
        },
    });
    format!("{value}\n")
}

/// Is a conversation tab launchable on this machine? The + menu's detection probe.
pub fn available() -> bool {
    let binary = std::env::var("ORGANON_CLAUDE_BIN")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_BINARY.to_string());
    resolve(&binary).is_some()
}

/// Find `binary` as an actual file.
///
/// ⚠️ **`Command::new("claude")` is not good enough on Windows.** `CreateProcessW`
/// searches `PATH` but appends only `.exe`, so an npm-installed `claude.cmd` — the shape
/// the harness registry already had to learn about ([`crate::harness::on_path`]) — would
/// never be found. [`platform::executable_names`] expands through `PATHEXT`, which is the
/// same probe the terminal side uses, so the two cannot disagree about what "installed"
/// means.
///
/// An absolute or relative path (from `$ORGANON_CLAUDE_BIN`) is taken as given.
fn resolve(binary: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(binary);
    if direct.components().count() > 1 || direct.is_absolute() {
        return direct.is_file().then_some(direct);
    }
    let names = platform::executable_names(Platform::current(), binary, |k| std::env::var(k).ok());
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        names.iter().map(|n| dir.join(n)).find(|candidate| candidate.is_file())
    })
}

/// How to actually run the resolved file.
///
/// A `.cmd`/`.bat` shim cannot be handed to `CreateProcessW` directly — it is a script,
/// not an image — so it goes through `cmd.exe /C`, which still gives us the child's real
/// stdio. Everything else is executed as itself.
fn launch_command(resolved: &std::path::Path) -> Command {
    let script = resolved
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
        .unwrap_or(false);
    if script {
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/C").arg(resolved);
        cmd
    } else {
        Command::new(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flags §5.9.2 measured a persistent session on. A silent drop here turns the
    /// tab back into a series of one-shots, which looks like "the agent forgot".
    #[test]
    fn the_persistent_session_flags_are_all_present() {
        for flag in [
            "-p",
            "--input-format",
            "--output-format",
            "--include-partial-messages",
            "--replay-user-messages",
            "--verbose",
        ] {
            assert!(ARGS.contains(&flag), "{flag} is not optional");
        }
        assert_eq!(ARGS.iter().filter(|a| **a == "stream-json").count(), 2);
    }

    /// The wire shape of a human turn, and the reason it is built rather than formatted:
    /// quotes and newlines must survive, and the line must be exactly one line.
    #[test]
    fn a_user_turn_is_one_escaped_json_line() {
        let line = user_message_line("say \"hi\"\nand stop");
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1, "a literal newline would split the line");
        let value: serde_json::Value = serde_json::from_str(line.trim_end()).expect("valid json");
        assert_eq!(value["type"], "user");
        assert_eq!(value["message"]["role"], "user");
        assert_eq!(value["message"]["content"][0]["type"], "text");
        assert_eq!(value["message"]["content"][0]["text"], "say \"hi\"\nand stop");
    }

    /// A missing binary must produce a message a human can act on — the failure this
    /// module refuses to make silent.
    #[test]
    fn a_missing_binary_names_itself_and_the_fix() {
        let error = SpawnError::NotFound { binary: "claude".into() }.to_string();
        assert!(error.contains("claude"));
        assert!(error.contains("PATH"));
        assert!(error.contains("ORGANON_CLAUDE_BIN"), "the escape hatch must be in the message");
    }

    /// A path that does not exist resolves to nothing rather than to a `Command` that
    /// fails later with a worse message.
    #[test]
    fn an_explicit_path_that_is_not_a_file_does_not_resolve() {
        assert!(resolve("./no/such/claude-binary").is_none());
    }

    /// The three flags that make an approval a card. `--permission-prompt-tool` needs
    /// `--print`, so the two sets are checked together — passing one without the other is
    /// a session where nothing prompts and nothing says why.
    #[test]
    fn the_approval_flags_name_the_config_and_the_handler() {
        let args = mcp_args(&McpWiring {
            config: PathBuf::from("/tmp/organon-console-mcp-1-8931.json"),
            permission_tool: "mcp__organon__approve_tool".into(),
        });
        assert_eq!(
            args,
            vec![
                "--mcp-config",
                "/tmp/organon-console-mcp-1-8931.json",
                "--strict-mcp-config",
                "--permission-prompt-tool",
                "mcp__organon__approve_tool",
            ]
        );
        assert!(ARGS.contains(&"-p"), "--permission-prompt-tool only works with --print");
        // The namespaced spelling is the client's, not ours to invent.
        assert!(args.last().unwrap().starts_with("mcp__"));
    }

    // -----------------------------------------------------------------------
    // The control protocol
    // -----------------------------------------------------------------------

    /// Decode a control response the way the stream does, so a test can hand
    /// [`ControlDesk`] the same value a live ack produces.
    fn ack(line: &str) -> ControlResponse {
        let event = crate::agent_event::decode_line(line).expect("a decodable line");
        match event.kind {
            crate::agent_event::EventKind::ControlResponse(response) => response,
            other => panic!("expected a control response, got {other:?}"),
        }
    }

    /// 📌 CONTRACT: the bytes are **exactly** the line
    /// `doc/console_session_control_protocol.md` §1 quotes from a live capture. A typo in
    /// a subtype is answered `Unsupported control request subtype: …` and reads to a user
    /// as a picker that does nothing, which is why this is pinned against the quote rather
    /// than against itself.
    #[test]
    fn a_control_request_is_the_line_the_protocol_doc_quotes() {
        let line = control_request_line("req-model-1", &Control::SetModel("sonnet".into()));
        assert!(line.ends_with('\n'), "one line, terminated");
        assert_eq!(line.matches('\n').count(), 1);
        let sent: serde_json::Value = serde_json::from_str(line.trim_end()).expect("valid json");
        // §1, verbatim.
        let quoted: serde_json::Value = serde_json::from_str(
            r#"{"type":"control_request","request_id":"req-model-1","request":{"subtype":"set_model","model":"sonnet"}}"#,
        )
        .expect("the doc's own line");
        assert_eq!(sent, quoted, "the request must be byte-equivalent to the measured one");

        // §8's, the same way — and note the key is `mode`, not `permission_mode`.
        let mode = control_request_line("req-perm-1", &Control::SetPermissionMode("acceptEdits".into()));
        let sent: serde_json::Value = serde_json::from_str(mode.trim_end()).expect("valid json");
        let quoted: serde_json::Value = serde_json::from_str(
            r#"{"type":"control_request","request_id":"req-perm-1","request":{"subtype":"set_permission_mode","mode":"acceptEdits"}}"#,
        )
        .expect("the doc's own line");
        assert_eq!(sent, quoted);

        // §3's takes no arguments at all.
        let init = control_request_line("req-init-1", &Control::Initialize);
        let sent: serde_json::Value = serde_json::from_str(init.trim_end()).expect("valid json");
        let quoted: serde_json::Value = serde_json::from_str(
            r#"{"type":"control_request","request_id":"req-init-1","request":{"subtype":"initialize"}}"#,
        )
        .expect("the doc's own line");
        assert_eq!(sent, quoted);
    }

    /// 📌 CONTRACT: two requests never share an id. An id that repeated would let one
    /// verb's ack resolve another's — and since `set_model`'s ack carries no body at all,
    /// nothing downstream could notice.
    #[test]
    fn every_request_gets_its_own_id() {
        let mut desk = ControlDesk::new();
        let now = Instant::now();
        let (a, _) = desk.issue(Control::SetModel("sonnet".into()), now);
        let (b, _) = desk.issue(Control::SetModel("sonnet".into()), now);
        let (c, _) = desk.issue(Control::SetPermissionMode("default".into()), now);
        assert_ne!(a, b, "the same verb twice must still be two ids");
        assert_ne!(b, c);
        assert!(a.contains("set_model"), "an id says which verb it is: {a}");
        assert!(c.contains("set_permission_mode"), "{c}");
    }

    /// 📌 CONTRACT: an ack resolves the request that carries its id, and only that one.
    #[test]
    fn an_ack_resolves_the_request_it_names() {
        let mut desk = ControlDesk::new();
        let now = Instant::now();
        let (model_id, _) = desk.issue(Control::SetModel("sonnet".into()), now);
        let (mode_id, _) = desk.issue(Control::SetPermissionMode("acceptEdits".into()), now);
        assert_eq!(desk.outstanding(), 2);

        let resolved = desk.resolve(&ack(&format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{mode_id}","response":{{"mode":"acceptEdits"}}}}}}"#
        )));
        assert_eq!(resolved, Some(Control::SetPermissionMode("acceptEdits".into())));
        assert_eq!(desk.outstanding(), 1, "the other request is still in flight");

        let resolved = desk.resolve(&ack(&format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{model_id}"}}}}"#
        )));
        assert_eq!(
            resolved,
            Some(Control::SetModel("sonnet".into())),
            "set_model acks with no body at all — the id is the only correlation there is"
        );
        assert_eq!(desk.outstanding(), 0);
    }

    /// 📌 CONTRACT: an ack for something this console never asked for is ignored, not
    /// mistaken for the oldest outstanding request. Expected traffic, not a fault.
    #[test]
    fn an_ack_this_console_did_not_ask_for_resolves_nothing() {
        let mut desk = ControlDesk::new();
        desk.issue(Control::SetModel("sonnet".into()), Instant::now());
        let stranger = ack(
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"somebody-else-1"}}"#,
        );
        assert_eq!(desk.resolve(&stranger), None);
        let anonymous =
            ack(r#"{"type":"control_response","response":{"subtype":"success"}}"#);
        assert_eq!(desk.resolve(&anonymous), None, "no id means no correlation");
        assert_eq!(desk.outstanding(), 1, "and neither one consumed the real request");
    }

    /// 🚨 CONTRACT: **a request nobody answers wedges nothing.** It is retired at the
    /// deadline, named so the caller can say which control went unanswered, and the desk
    /// is left clean — there is no queue to drain and nothing was ever blocked on it.
    #[test]
    fn an_unanswered_request_is_given_up_on_and_wedges_nothing() {
        let mut desk = ControlDesk::new();
        let sent = Instant::now();
        desk.issue(Control::SetModel("sonnet".into()), sent);
        desk.issue(Control::SetPermissionMode("dontAsk".into()), sent + CONTROL_DEADLINE);

        // Just short of the deadline nothing is retired: a busy session is not accused of
        // having dropped a request.
        assert!(desk.give_up(sent + CONTROL_DEADLINE / 2, CONTROL_DEADLINE).is_empty());
        assert_eq!(desk.outstanding(), 2);

        let abandoned = desk.give_up(sent + CONTROL_DEADLINE, CONTROL_DEADLINE);
        assert_eq!(abandoned, vec![Control::SetModel("sonnet".into())]);
        assert_eq!(desk.outstanding(), 1, "the younger request is still waiting, not swept");
        assert!(
            abandoned[0].describe().contains("sonnet"),
            "the sentence must name what went unanswered: {}",
            abandoned[0].describe()
        );

        // An ack that arrives *after* the console gave up resolves nothing and is not an
        // error — it is simply late.
        let late = ack(
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"organon-set_model-1"}}"#,
        );
        assert_eq!(desk.resolve(&late), None);
    }

    /// The CLI's own refusal survives to the caller. Measured against `bypassPermissions`,
    /// which is why the picker never offers it — but a mode can be refused for reasons
    /// this build has not met, and the sentence is written for a human.
    #[test]
    fn a_refusal_comes_back_as_the_cli_wrote_it() {
        let mut desk = ControlDesk::new();
        let (id, _) = desk.issue(Control::SetPermissionMode("bypassPermissions".into()), Instant::now());
        let refused = ack(&format!(
            r#"{{"type":"control_response","response":{{"subtype":"error","request_id":"{id}","error":"Cannot set permission mode to bypassPermissions because the session was not launched with --dangerously-skip-permissions"}}}}"#
        ));
        assert_eq!(
            desk.resolve(&refused),
            Some(Control::SetPermissionMode("bypassPermissions".into()))
        );
        assert!(!refused.is_success());
        assert!(refused.error().unwrap().contains("was not launched with"));
    }
}
