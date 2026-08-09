//! The terminal core (Shell #10 Tier 1): a real PTY + the adopted VT state
//! machine, pumped by the UI thread.
//!
//! Design (PRD v3 §8.2): the PTY is `portable-pty`, the escape-sequence state
//! machine is `alacritty_terminal` — adopted standards, not product identity. What
//! is ours here is the *shape*: a *pull* model with *no locks*. The reader thread
//! only moves bytes into a channel; the UI thread drains it once per frame and
//! advances the parser against the `Term` it owns exclusively. That keeps the VT
//! state single-threaded (no `FairMutex`, no tearing) at the cost of ≤ one frame of
//! output latency, which the continuous redraw loop already pays.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Column;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::platform::{self, Platform};

/// The grid size as `alacritty_terminal` wants it — `WindowSize` (its event-loop
/// type) does not implement `Dimensions`; this minimal carrier does.
#[derive(Clone, Copy)]
struct GridSize {
    cols: u16,
    rows: u16,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }
    fn screen_lines(&self) -> usize {
        self.rows as usize
    }
    fn columns(&self) -> usize {
        self.cols as usize
    }
    fn last_column(&self) -> Column {
        Column((self.cols as usize).saturating_sub(1))
    }
    fn topmost_line(&self) -> alacritty_terminal::index::Line {
        alacritty_terminal::index::Line(0)
    }
    fn bottommost_line(&self) -> alacritty_terminal::index::Line {
        alacritty_terminal::index::Line(self.rows as i32 - 1)
    }
}

/// The VT state machine's **answers**, on their way back to the child.
///
/// ⚠️ **This is not a nicety, it is what makes a terminal work on Windows**, and its
/// absence is why Shell rendered a blank grid there for three rounds of guessing
/// (#693, #695, #697). A terminal is not write-only: the escape-sequence machine
/// *replies* to device queries, and `alacritty_terminal` emits those replies as
/// `Event::PtyWrite` through its `EventListener`. This used to be `VoidListener` —
/// whose impl is empty, so every reply was computed and then dropped on the floor.
///
/// **Measured on organon-one, 2026-08-08** (`ORGANON_SHELL_PTY_DEBUG=1`, both a
/// `cmd.exe /C` tab and a real `powershell.exe -NoLogo` tab, byte-identical):
///
/// ```text
/// [pty] spawn argv=["powershell.exe", "-NoLogo"] cwd=None
/// [pty] read 4 (total 4): \x1b[6n
/// [pty] feed 4 to parser (grid 80x24)
/// ```
///
/// Four bytes, then silence — no output, no EOF, no error. `\x1b[6n` is **DSR-CPR**
/// (device status report, cursor position), which **ConPTY sends first and blocks
/// on**: it will not forward a byte of the child's output until the terminal answers
/// `\x1b[<row>;<col>R`. `Term::device_status(6)` builds exactly that and hands it to
/// the listener, so with `VoidListener` the handshake could never complete and every
/// Windows tab hung before its first glyph.
///
/// **A POSIX pty has no such handshake** — the shell simply writes — which is why the
/// same code was correct on macOS and why no amount of reading it there found this.
///
/// Replies go through a channel rather than a shared writer on purpose: `send_event`
/// takes `&self` and fires from *inside* `parser.advance(&mut self.term, …)`, so it
/// cannot reach `self.writer`. The channel keeps this module's stated design — no
/// locks, one owner, drained once per frame — and [`TermSession::pump`] flushes it in
/// the same call that produced it, so the answer is never more than one frame late.
#[derive(Clone)]
pub struct PtyReplies(mpsc::Sender<String>);

impl EventListener for PtyReplies {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            // A closed channel means the session is being torn down; the reply has
            // nowhere useful to go and dropping it is correct.
            let _ = self.0.send(text);
        }
    }
}

/// One live terminal session: PTY + child + VT state. The unit a tab owns
/// (Shell #11 makes them plural).
pub struct TermSession {
    pub term: Term<PtyReplies>,
    parser: vte::ansi::Processor,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    /// VT replies awaiting a trip back down the line (see [`PtyReplies`]).
    reply_rx: Receiver<String>,
    /// Set when the reader hits EOF — the child is gone; the grid stays readable.
    pub exited: bool,
    cols: u16,
    rows: u16,
}

/// Scrollback depth. alacritty's own default is 10k lines; nothing here needs to
/// differ from the reference implementation yet.
const SCROLLBACK_LINES: usize = 10_000;

/// `ORGANON_SHELL_PTY_DEBUG=1` — trace the PTY byte path to stderr.
///
/// Exists because the Windows grid comes back **blank** with the child alive, and
/// three rounds of reasoning-from-source have not settled why (#693, #695). The
/// question "do bytes reach us, and what are they" is a *measurement*, and this is
/// the instrument. It reports at four points, which between them separate every
/// hypothesis still open:
///
/// | Symptom in the trace | Meaning |
/// |---|---|
/// | `read err` immediately | the handle is wrong — a spawn/plumbing bug |
/// | `read EOF` immediately | the pipe closed — the child never attached |
/// | no `read` lines at all | the child produced nothing (or ConPTY is not forwarding) |
/// | `read` lines but no `feed` | the channel or the pump is broken |
/// | `feed` lines but a blank grid | the VT/render side is at fault, not the PTY |
///
/// Default-off and allocation-free when off.
pub fn pty_debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("ORGANON_SHELL_PTY_DEBUG").is_ok_and(|v| v != "0"))
}

/// First `n` bytes, escaped so escape sequences are readable in a terminal that
/// would otherwise *interpret* them — the trace must not drive the terminal reading it.
fn peek(bytes: &[u8], n: usize) -> String {
    bytes.iter().take(n).flat_map(|b| std::ascii::escape_default(*b)).map(char::from).collect()
}

impl TermSession {
    /// Spawn the user's shell (or an explicit command) on a fresh PTY.
    ///
    /// `TERM=xterm-256color` is the §8.2 launch answer — our own terminfo entry
    /// waits until our extensions warrant it.
    ///
    /// ⚠️ **The default (`command: None`) branch is platform-dependent and used to be
    /// a POSIX literal.** It ran `/bin/zsh -l` unconditionally, so on Windows every
    /// tab died with `CreateProcessW "/bin/zsh -l" … os error 3` — while the Windows
    /// CI leg stayed green, because every test passes an explicit `command` and never
    /// reaches here. [`platform::default_shell`] owns the choice now and is tested for
    /// both platforms from either host.
    ///
    /// `cwd` is the PTY's working directory; `None` inherits the app's.
    ///
    /// ⚠️ **It must already be tilde-expanded** — this hands it straight to
    /// `CommandBuilder::cwd`, and neither `chdir` nor `CreateProcessW` expands `~`.
    /// [`crate::harness::launch_argv`] is the one place that resolves it, and callers
    /// with a WSL harness get `None` from it, because that harness's directory is a
    /// Linux path handled by a `cd` inside WSL instead.
    pub fn spawn(
        cols: u16,
        rows: u16,
        command: Option<Vec<String>>,
        cwd: Option<&str>,
    ) -> std::io::Result<Self> {
        let pty = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(std::io::Error::other)?;

        let argv = match command {
            Some(argv) if !argv.is_empty() => argv,
            _ => platform::default_shell(
                Platform::current(),
                |k| std::env::var(k).ok(),
                |bin| crate::harness::on_path(bin),
            ),
        };
        let mut cmd = CommandBuilder::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        // The IPC namespace must reach the child: an `organon` CLI run INSIDE a Shell
        // tab has to address this process, and it resolves the namespace from its own
        // environment. Without this it silently talks to the default `organic-math`
        // namespace — a different product's snapshot — and the self-steering loop
        // only ever worked because a human exported it by hand.
        cmd.env("ORGANON_IPC_NS", organon_core::ipc::namespace());
        match cwd {
            Some(dir) => cmd.cwd(dir),
            None => {
                if let Ok(dir) = std::env::current_dir() {
                    cmd.cwd(dir);
                }
            }
        }

        let child = pty.slave.spawn_command(cmd).map_err(std::io::Error::other)?;
        // The slave fd must drop in the parent or the reader never sees EOF when
        // the child exits — the parent's copy would keep the line open forever.
        drop(pty.slave);

        let mut reader = pty.master.try_clone_reader().map_err(std::io::Error::other)?;
        let writer = pty.master.take_writer().map_err(std::io::Error::other)?;

        if pty_debug() {
            eprintln!("[pty] spawn argv={argv:?} cwd={cwd:?}");
        }

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::Builder::new().name("pty-reader".into()).spawn(move || {
            let mut buf = [0u8; 65536];
            let mut total = 0usize;
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        if pty_debug() {
                            eprintln!("[pty] read EOF after {total} bytes");
                        }
                        break; // EOF / gone: channel drop signals it.
                    }
                    Err(e) => {
                        if pty_debug() {
                            eprintln!("[pty] read err after {total} bytes: {e}");
                        }
                        break;
                    }
                    Ok(n) => {
                        total += n;
                        if pty_debug() {
                            eprintln!("[pty] read {n} (total {total}): {}", peek(&buf[..n], 96));
                        }
                        if tx.send(buf[..n].to_vec()).is_err() {
                            if pty_debug() {
                                eprintln!("[pty] channel closed; reader stopping");
                            }
                            break;
                        }
                    }
                }
            }
        })?;

        let size = GridSize { cols, rows };
        let config = Config { scrolling_history: SCROLLBACK_LINES, ..Config::default() };
        let (reply_tx, reply_rx) = mpsc::channel::<String>();
        let term = Term::new(config, &size, PtyReplies(reply_tx));

        Ok(Self {
            term,
            parser: vte::ansi::Processor::new(),
            master: pty.master,
            child,
            writer,
            rx,
            reply_rx,
            exited: false,
            cols,
            rows,
        })
    }

    /// Drain everything the child has written since the last frame into the VT
    /// state. Returns true when anything advanced (the caller's repaint hint).
    pub fn pump(&mut self) -> bool {
        let mut advanced = false;
        loop {
            match self.rx.try_recv() {
                Ok(bytes) => {
                    if pty_debug() {
                        eprintln!("[pty] feed {} to parser (grid {}x{})", bytes.len(), self.cols, self.rows);
                    }
                    self.parser.advance(&mut self.term, &bytes);
                    advanced = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.exited = true;
                    break;
                }
            }
        }

        // Answer the child's device queries (see [`PtyReplies`]). This must run in
        // the SAME call that advanced the parser, and unconditionally rather than
        // under `if advanced`: on Windows the very first reply is what unblocks
        // ConPTY, and a terminal that never answers never receives anything more to
        // advance on — so gating this on progress would deadlock on the one byte
        // sequence that matters most.
        while let Ok(text) = self.reply_rx.try_recv() {
            if self.exited {
                break;
            }
            let _ = self.writer.write_all(text.as_bytes());
            let _ = self.writer.flush();
        }

        advanced
    }

    /// Keyboard/paste input straight down the line discipline.
    pub fn input(&mut self, bytes: &[u8]) {
        if !self.exited {
            let _ = self.writer.write_all(bytes);
            let _ = self.writer.flush();
        }
    }

    /// Resize both ends — the PTY (so the child gets `SIGWINCH` and the right
    /// `TIOCGWINSZ` answer) and the VT grid. No-op when nothing changed.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows || cols == 0 || rows == 0 {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        self.term.resize(GridSize { cols, rows });
    }

    /// Scroll the *display* (scrollback view), not the content — positive is up
    /// into history, the terminal convention.
    pub fn scroll_display(&mut self, delta: i32) {
        self.term.scroll_display(alacritty_terminal::grid::Scroll::Delta(delta));
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }
}

impl Drop for TermSession {
    fn drop(&mut self) {
        // The child dies with the session — a terminal window closing is SIGHUP
        // territory, and portable-pty's kill is the portable spelling of it.
        let _ = self.child.kill();
    }
}

/// Encode one key press the way an xterm-family terminal would put it on the wire.
///
/// Pure and separately tested — this table is the input half of "it must BE a
/// terminal" (PRD v3 §4.1), and every entry is a behavior someone's muscle memory
/// depends on. `app_cursor` is `TermMode::APP_CURSOR` (DECCKM): arrows/Home/End
/// switch from CSI to SS3 encodings when a full-screen app asks.
pub fn encode_key(key: egui::Key, mods: egui::Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    use egui::Key as K;

    // Ctrl+letter → C0 control byte (Ctrl-C = 0x03, Ctrl-Z = 0x1a, …). egui does
    // not deliver these as Text events, so they must be produced here.
    if mods.ctrl && !mods.alt {
        let ctrl = match key {
            K::A => 0x01, K::B => 0x02, K::C => 0x03, K::D => 0x04, K::E => 0x05,
            K::F => 0x06, K::G => 0x07, K::H => 0x08, K::I => 0x09, K::J => 0x0a,
            K::K => 0x0b, K::L => 0x0c, K::M => 0x0d, K::N => 0x0e, K::O => 0x0f,
            K::P => 0x10, K::Q => 0x11, K::R => 0x12, K::S => 0x13, K::T => 0x14,
            K::U => 0x15, K::V => 0x16, K::W => 0x17, K::X => 0x18, K::Y => 0x19,
            K::Z => 0x1a,
            K::OpenBracket => 0x1b,
            K::Backslash => 0x1c,
            K::CloseBracket => 0x1d,
            _ => 0,
        };
        if ctrl != 0 {
            return Some(vec![ctrl]);
        }
    }

    let arrow = |csi: u8| -> Vec<u8> {
        if app_cursor { vec![0x1b, b'O', csi] } else { vec![0x1b, b'[', csi] }
    };
    let seq: Vec<u8> = match key {
        K::Enter => vec![b'\r'],
        K::Tab => {
            if mods.shift {
                b"\x1b[Z".to_vec() // back-tab
            } else {
                vec![b'\t']
            }
        }
        K::Backspace => vec![0x7f],
        K::Escape => vec![0x1b],
        K::ArrowUp => arrow(b'A'),
        K::ArrowDown => arrow(b'B'),
        K::ArrowRight => arrow(b'C'),
        K::ArrowLeft => arrow(b'D'),
        K::Home => arrow(b'H'),
        K::End => arrow(b'F'),
        K::PageUp => b"\x1b[5~".to_vec(),
        K::PageDown => b"\x1b[6~".to_vec(),
        K::Delete => b"\x1b[3~".to_vec(),
        K::Insert => b"\x1b[2~".to_vec(),
        _ => return None,
    };
    Some(seq)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire encodings people's fingers depend on, pinned.
    #[test]
    fn key_encoding_matches_the_xterm_table() {
        let none = egui::Modifiers::NONE;
        assert_eq!(encode_key(egui::Key::Enter, none, false), Some(vec![b'\r']));
        assert_eq!(encode_key(egui::Key::Backspace, none, false), Some(vec![0x7f]));
        assert_eq!(encode_key(egui::Key::ArrowUp, none, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode_key(egui::Key::ArrowUp, none, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(
            encode_key(egui::Key::C, egui::Modifiers::CTRL, false),
            Some(vec![0x03]),
            "Ctrl-C is not optional"
        );
        assert_eq!(
            encode_key(egui::Key::Tab, egui::Modifiers::SHIFT, false),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(encode_key(egui::Key::F1, none, false), None, "unmapped keys pass through");
    }

    /// ⚠️ **These two tests spawn a REAL process, so they cannot assume a POSIX
    /// shell.** They hard-coded `/bin/sh` and were red on every Windows CI run from
    /// the day the terminal landed — `CreateProcessW "/bin/sh …"` fails with "the
    /// system cannot find the path specified" (os error 3), because Windows has no
    /// such path and `portable-pty` does not translate one. `cmd.exe /C` is the
    /// equivalent there; `ping -n` is the idiomatic sleep, since `timeout /t` wants
    /// a console it does not have under ConPTY.
    ///
    /// Keep the two argv builders below platform-split rather than reaching for one
    /// clever cross-platform command: the *shells* differ, not just the paths, so a
    /// single string would be wrong on one of them either way.
    ///
    /// 📌 **SETTLED on organon-one, 2026-08-08 — and it was neither candidate.** The
    /// note here used to offer two: (a) `cmd.exe /C` quote stripping eating the echo,
    /// a test bug; or (b) Shell's reader not draining ConPTY, a product bug. A trace
    /// under `ORGANON_SHELL_PTY_DEBUG=1` showed the reader draining perfectly and the
    /// child never speaking: **4 bytes, `\x1b[6n`, then silence.** ConPTY was blocked
    /// on a DSR-CPR reply that [`PtyReplies`] now sends and `VoidListener` used to
    /// discard. So it was a product bug, but not the one (b) describes — and the argv
    /// was never at fault, which is why this test comes back unmodified rather than
    /// rewritten. [`PtyReplies`] carries the measurement.
    ///
    /// The lesson worth keeping: `resize_round_trips` passing on Windows never
    /// implied this one would, because it asserts geometry and never reads a glyph.
    /// This is the only test that walks PTY → parser → grid end to end, which is
    /// exactly why it was `#[ignore]`d with a written reason instead of deleted.
    #[cfg(not(windows))]
    fn print_then_linger() -> Vec<String> {
        vec!["/bin/sh".into(), "-c".into(), "printf 'organon-ok'; sleep 0.3".into()]
    }
    #[cfg(windows)]
    fn print_then_linger() -> Vec<String> {
        vec!["cmd.exe".into(), "/C".into(), "echo organon-ok&& ping -n 2 127.0.0.1 >nul".into()]
    }

    #[cfg(not(windows))]
    fn linger() -> Vec<String> {
        vec!["/bin/sh".into(), "-c".into(), "sleep 1".into()]
    }
    #[cfg(windows)]
    fn linger() -> Vec<String> {
        vec!["cmd.exe".into(), "/C".into(), "ping -n 2 127.0.0.1 >nul".into()]
    }

    /// The whole loop, headless: spawn a real PTY running a real command, pump
    /// until it lands in the grid, and read the glyphs back out of the VT state.
    /// This is the cloud-runnable proof that PTY → parser → grid works end to end.
    #[test]
    fn echo_lands_in_the_grid() {
        let mut s = TermSession::spawn(40, 10, Some(print_then_linger()), None)
            .expect("spawn a shell on a pty");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut screen = String::new();
        while std::time::Instant::now() < deadline {
            s.pump();
            screen = grid_text(&s);
            if screen.contains("organon-ok") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(screen.contains("organon-ok"), "grid never showed the output: {screen:?}");
    }

    /// Resize must not panic and must stick on the VT side.
    #[test]
    fn resize_round_trips() {
        let mut s = TermSession::spawn(80, 24, Some(linger()), None).expect("spawn");
        s.resize(100, 30);
        assert_eq!(s.size(), (100, 30));
        s.resize(0, 30); // degenerate sizes are refused, not applied
        assert_eq!(s.size(), (100, 30));
    }

    fn grid_text(s: &TermSession) -> String {
        let content = s.term.renderable_content();
        let mut out = String::new();
        let mut line = None;
        for cell in content.display_iter {
            if line != Some(cell.point.line) {
                line = Some(cell.point.line);
                out.push('\n');
            }
            out.push(cell.cell.c);
        }
        out
    }
}
