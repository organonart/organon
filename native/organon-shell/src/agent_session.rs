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
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::agent_event::{AgentEvent, EventStream};
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
}

impl AgentSession {
    /// Spawn the agent in `cwd` (the app's own directory when `None`).
    pub fn spawn(cwd: Option<&str>) -> Result<Self, SpawnError> {
        let binary = std::env::var("ORGANON_CLAUDE_BIN")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BINARY.to_string());
        let resolved = resolve(&binary).ok_or_else(|| SpawnError::NotFound {
            binary: binary.clone(),
        })?;

        let mut cmd = launch_command(&resolved);
        cmd.args(ARGS);
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

        Ok(Self {
            child,
            stdin: Some(stdin),
            rx,
            exited: false,
            command: format!("{} {}", resolved.display(), ARGS.join(" ")),
        })
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
}
