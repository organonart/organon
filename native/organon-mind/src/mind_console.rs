//! #367 Tier 2 — the in-plugin **Mind console**.
//!
//! The plugin spawns the embedded `organic-math-mind-runtime` helper as a
//! *managed child process* (mirroring `spawn_visual`, but with piped stdio),
//! captures its stdout+stderr into a bounded scrollback the Mind card renders,
//! and forwards typed commands to the child's stdin (the line REPL the runtime
//! parses — `gen` / `load` / `temp` / `status` / `stop` / …).
//!
//! Two things this buys over "run it yourself in a terminal":
//!  1. **No separate terminal** — the plugin owns the process lifecycle, so the
//!     runtime starts/stops from the Mind tab and can never become a stale
//!     6-day zombie reading the wrong `Shared` layout.
//!  2. **Generation without audio.** The command *trigger* rides stdin, not the
//!     audio-thread-stamped IPC counters (`prompt_gen`/`model_gen`), so Mind
//!     inference fires even with the transport stopped.
//!
//! The struct lives behind an `Arc<Mutex<_>>` held by the plugin and cloned into
//! the editor closure (the same pattern as the `model_gen`/`mind_topo` atomics),
//! so the runtime keeps running while the editor window is closed + reopened.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

/// Bounded scrollback — the runtime is chatty on model load (per-tensor lines),
/// so keep enough to see a load + a generation without unbounded growth.
const LOG_CAP: usize = 800;

#[derive(Default)]
pub struct MindConsole {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    log: VecDeque<String>,
    /// Bumped on every log push / lifecycle change so the UI can auto-scroll
    /// only when new output actually arrived.
    revision: u64,
}

impl MindConsole {
    pub fn shared() -> Arc<Mutex<MindConsole>> {
        Arc::new(Mutex::new(MindConsole::default()))
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn lines(&self) -> impl Iterator<Item = &String> {
        self.log.iter()
    }

    pub fn clear_log(&mut self) {
        self.log.clear();
        self.bump();
    }

    /// Push a console-side notice (e.g. "runtime not bundled") into the log.
    pub fn note(&mut self, msg: impl Into<String>) {
        self.push(msg);
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn push(&mut self, line: impl Into<String>) {
        self.log.push_back(line.into());
        while self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
        self.bump();
    }

    /// Launch the runtime at `exe` if it isn't already running. Reader threads
    /// drain the child's stdout+stderr into `shared`'s scrollback. Takes the
    /// `Arc` (not `&mut self`) because the reader threads need their own handle.
    pub fn start(shared: &Arc<Mutex<MindConsole>>, exe: &Path) {
        {
            let mut c = shared.lock().unwrap();
            if c.is_running() {
                c.push("mind-console: runtime already running");
                return;
            }
        }
        let mut child = match Command::new(exe)
            // #483 Tier 1 — hand the runtime OUR IPC namespace, exactly as `spawn_visual`
            // does. The runtime is built once (feature-off, so its own `EDITION` is
            // `Full`); this is what points it at the Organon Mind ring + sidecars when an
            // Organon Mind session launched it. Under full Organon it sets the variable to
            // `organic-math` — what the child would have resolved anyway.
            .env("ORGANON_IPC_NS", organon_core::ipc::namespace())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let mut c = shared.lock().unwrap();
                c.push(format!("mind-console: failed to launch {}: {e}", exe.display()));
                return;
            }
        };
        let stdin = child.stdin.take();
        if let Some(out) = child.stdout.take() {
            spawn_reader(shared.clone(), out);
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(shared.clone(), err);
        }
        let mut c = shared.lock().unwrap();
        c.child = Some(child);
        c.stdin = stdin;
        c.push(format!("mind-console: launched {}", exe.display()));
    }

    /// Send one command line to the runtime's stdin REPL. Returns false (and logs)
    /// if the runtime isn't running or the pipe is broken.
    pub fn send_command(&mut self, cmd: &str) -> bool {
        let line = cmd.trim_end();
        if line.is_empty() {
            return false;
        }
        if let Some(stdin) = self.stdin.as_mut() {
            if writeln!(stdin, "{line}").and_then(|_| stdin.flush()).is_ok() {
                self.push(format!("> {line}"));
                return true;
            }
            // Broken pipe → the child died under us; fall through to the notice.
            self.stdin = None;
        }
        self.push("mind-console: runtime not running — press Start first");
        false
    }

    /// Stop the runtime: ask it to `quit`, drop its stdin (EOF), then reap it.
    pub fn stop(&mut self) {
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = writeln!(stdin, "quit");
            let _ = stdin.flush();
        }
        self.stdin = None; // dropping the pipe signals EOF to the reader thread
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            self.push("mind-console: runtime stopped");
        }
    }

    /// Reap the child if it exited on its own (e.g. a load failure or `quit`),
    /// so the UI flips back to "stopped". Call once per editor frame.
    pub fn poll_liveness(&mut self) {
        if let Some(child) = self.child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                self.push(format!("mind-console: runtime exited ({status})"));
                self.child = None;
                self.stdin = None;
            }
        }
    }
}

impl Drop for MindConsole {
    fn drop(&mut self) {
        // Don't leave an orphaned inference process when the plugin unloads.
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_reader<R: Read + Send + 'static>(shared: Arc<Mutex<MindConsole>>, pipe: R) {
    std::thread::spawn(move || {
        let reader = BufReader::new(pipe);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if let Ok(mut c) = shared.lock() {
                        c.push(l);
                    }
                }
                Err(_) => break,
            }
        }
    });
}
