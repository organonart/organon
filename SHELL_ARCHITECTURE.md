# Organon Shell — living architecture

> **What this is.** The code-grounded state of **Organon Shell** as it exists *right
> now* — not a spec, not a roadmap. It is the `MIND_ARCHITECTURE.md`-shaped sibling on
> the code side of Shell's product definition (`doc/organon_shell_prd.md`, v3.2 — the
> TUI host) and build plan (`doc/organon_shell_buildplan.md`), both of which are part
> of the private annex and do not travel with this file. **Update it in the same change
> as every Shell PR** — a Stop hook (`.claude/hooks/doc-rules.sh`) reminds you when
> `native/organon-shell/*` moves without it.
>
> **`Shell #N` in this file, and in the crate's doc comments, is a Shell work item** —
> a tree in the tracker Shell was planned in, not an issue in this repo. Bare `#N`
> means this repo's tracker. Both are kept as provenance; neither is a link.
>
> **Not auto-injected.** Open it deliberately when working on Shell, like
> `MIND_ARCHITECTURE.md` and `doc/arch/render.md`.

---

## 1. What exists right now (the terminal form — trees A/B/E Tier 1 + the landed v2 foundations)

**Organon Shell is a next-gen TUI host** (PRD v3.2 §1.2, reframed 2026-08-08): tabs of
agent harnesses — Pi first — drawn as a real GPU terminal, with the Organon engine
available behind the glyphs. The bare shell is one menu entry, not the opening
position.

- **The terminal core (#10 T1, `term.rs` + `term_view.rs`)** — `portable-pty` 0.9
  spawns the session (login shell or a harness command via `zsh -lc "exec …"`,
  `TERM=xterm-256color`); `alacritty_terminal` 0.26 (vte 0.15) is the VT state
  machine, advanced by a **lock-free pull loop** (reader thread → channel → the UI
  thread owns parser+`Term` exclusively, drained once per frame). `term_view` draws
  the grid through egui as per-line same-style runs: full color stack (phosphor
  ANSI16, xterm-256 pinned by test, truecolor, inverse/dim, OSC overrides), block
  cursor, wheel scrollback, bracketed paste, the xterm key table pinned by test
  (Ctrl-C, DECCKM app-cursor). ⌘-keys are chrome, never PTY. **The one provisional
  layer**: glyph painting rides egui text runs behind the `renderable_content()`
  seam; the instanced glyph-atlas pass is #10's later polish tier.
- **A terminal answers back (`PtyReplies` in `term.rs`) — ✅ this is what made Windows
  render at all.** The VT machine is not write-only: it *replies* to device queries,
  and `alacritty_terminal` emits those as `Event::PtyWrite` through its
  `EventListener`. That listener was `VoidListener`, whose impl is empty, so every
  reply was computed and discarded. **Measured on organon-one 2026-08-08** under
  `ORGANON_SHELL_PTY_DEBUG=1` — identically for a `cmd.exe /C` tab and a real
  `powershell.exe -NoLogo` tab: `read 4 (total 4): \x1b[6n`, then silence. No output,
  no EOF, no error. `\x1b[6n` is **DSR-CPR**, which **ConPTY sends first and blocks
  on** — it forwards nothing of the child's until the terminal answers
  `\x1b[<row>;<col>R`. So every Windows tab hung before its first glyph, looking
  exactly like a render bug. `PtyReplies` forwards those answers down a channel that
  `pump` flushes in the same call that produced them (the listener takes `&self` and
  fires from inside `parser.advance`, so it cannot reach the writer directly; the
  channel also keeps this module's no-locks/one-owner design). **A POSIX pty has no
  such handshake**, which is why the identical code was correct on macOS and why three
  rounds of reading it there never found this. `echo_lands_in_the_grid` — `#[ignore]`d
  on Windows by #693 — is un-ignored and passes in 0.05 s where it used to burn its
  full 5 s deadline.
- **Tabs + the harness registry (#11 T1, `tabs.rs` + `harness.rs`)** — the
  Superconductor model: each tab is one PTY session running a registry harness
  (built-ins: Shell, Pi, Claude Code, oh-my-pi, Codex, Cursor; user
  `harnesses.json` merges by id, serde-tolerant), PATH-detected via injectable
  lookup. The strip along the top is the ONE permitted chrome (FR-T11); the **+**
  menu lists the numbered registry, installed selectable, missing greyed with
  install URLs. ⌘T/⌘W/⌘1-9/⌘⇧[] via a pure, tested key table. Default tab =
  `$ORGANON_SHELL_DEFAULT` → Pi if installed → plain shell. All sessions pump
  every frame; the active one draws; closing the last quits.
- **The living backdrop (#14 T1, in `shell_main.rs`)** — the `World` rendered
  window-sized each frame and painted UNDER the glyphs (the measured
  render-sRGB/sample-linear gamma pair, same-id rebinds). The **legibility scrim**
  is structural: `ORGANON_SHELL_SCRIM` tunes, but clamps at a floor no setting
  crosses. Summoned, never imposed: `ORGANON_SHELL_BACKDROP=1`. The bin negotiates
  the FULL engine feature set (bind groups, RT, timestamps) — a default-limits
  device opens a window and then fails to create engine pipelines.
- **Process launching is platform data, not `#[cfg]` (`platform.rs`, 2026-08-08)** —
  `Platform` is a **value**, so the Windows decisions are unit-tested from a Mac.
  `default_shell` (Unix: `$SHELL` → `/bin/zsh`, `-l`; Windows: `pwsh` → `powershell`
  → `%ComSpec%`, no `-l`), `wrap_harness` (Unix `-lc "exec …"` for the login PATH;
  Windows `cmd.exe /C` because npm harnesses are **`.cmd` shims** `CreateProcessW`
  cannot execute), `shell_dash_c`, `wrap_wsl`, and `executable_names` (PATHEXT).
  ⚠️ **This exists because Shell shipped unable to open a shell on Windows while the
  Windows CI leg was green**: the POSIX literals sat on the `command: None` branch,
  and every test passes an explicit command, so that branch had no coverage anywhere.
  Put a platform decision here with a test per variant — never a `#[cfg]` at the point
  of use.
- **WSL harnesses (Windows)** — `HarnessSpec` gains `cwd`, `wsl`, `wsl_distro`;
  `harness::launch_argv` is the single place that turns a spec into `(argv, pty_cwd)`.
  A `wsl` harness runs `wsl.exe [-d D] -- bash -lic "cd <dir> && exec <cmd>"` — **login
  *and* interactive**, because `nvm` (hence `pi`) initialises from `.bashrc`. Built-ins
  add `pi-wsl` / `claude-wsl` / `shell-wsl` on Windows only, carrying **no `cwd`** — a
  project directory is the user's, and belongs in `harnesses.json`.
  **One rule governs `cwd`: it is a path in the namespace the process actually STARTS
  in.** A WSL launch starts inside WSL, so `~` is left for bash (`bash_cd`) and
  `pty_cwd` is `None`. Every other launch — including a `wsl` spec on a non-Windows
  host, where the flag is inert — starts on this machine, so `~` is resolved by
  `expand_tilde` before the OS sees it (PRD FR-T5). Both halves are required: `~` is
  shell syntax, and neither `chdir` nor `CreateProcessW` expands it, so implementing
  only the WSL half made the documented `"cwd": "~/…"` shorthand work in one place and
  fail in the other.
- **The self-steering loop (#14 T1)** — Shell publishes the default-look `Shared`
  snapshot each frame (seqlock `ipc::Writer`, `organon-shell` namespace), so the
  `organon` CLI works from *inside* the terminal: `status`/`get`/`watch` read it,
  and the override lane (`set`/`generator`/…) drains in the world's frame path —
  verified live: a CLI generator switch changed the running backdrop. ✅ **It now
  works unattended.** Until 2026-08-08 that verification required a manual
  `export ORGANON_IPC_NS=organon-shell`: the PTY was given only `TERM` and
  `COLORTERM`, so an `organon` invocation inside a Shell tab resolved the **default
  `organic-math` namespace** and addressed a different product entirely. `term.rs`
  now injects `ORGANON_IPC_NS = ipc::namespace()` into every session. Known
  cosmetic: the CLI's op-path liveness heuristic reads the Feedback channel Shell
  does not write yet.
- **The landed v2 foundations** (session/event log with torn-tail recovery, the
  typed command service, mock-agent event cards) remain in the crate, feeding
  trees C/D.
- **Dev flags**: `ORGANON_SHELL_CMD` (one plain-command tab, headless proof),
  `ORGANON_SHELL_TABS` (comma harness ids), `ORGANON_SHELL_DEFAULT`,
  `ORGANON_SHELL_BACKDROP`, `ORGANON_SHELL_SCRIM`, **`ORGANON_SHELL_PTY_DEBUG`**.
  **`organon-shell --help` is their documentation** (2026-08-09) — the binary had no
  argument handling at all until then, so `--help` started the event loop and hung. The
  scrim line is **formatted from `term_view::SCRIM_DEFAULT`/`SCRIM_FLOOR`**, not restated:
  the first draft documented `<0..1>` when the value is a `u8` alpha, and `0.5` fails the
  parse, is swallowed by `.ok()`, and silently falls back — documented syntax that does
  nothing is worse than none. Add a flag or change those constants and update the help in
  the same change; a test fails if the two drift.

  ⚠️ **That drift test is an allow-list, so it catches a *removal* and not an *addition*.**
  `help_names_every_documented_environment_variable` asserts the help mentions each name in
  a hand-written array; a new flag that never reaches either stays invisible to it. Adding
  `ORGANON_SHELL_PTY_DEBUG` here is what caught that — the flag merged green while the help
  it is supposed to be documented by said nothing about it. Add a flag to **both** the help
  text and that array, in the same change.

  `ORGANON_SHELL_PTY_DEBUG` is an **instrument, not a log level** (trace the byte path to
  stderr: `[pty] spawn/read/feed/EOF` plus a `[grid]` line on resize). It exists because a
  blank grid has five causes indistinguishable from outside the process, and it separates
  them: `read err` (wrong handle) · `read EOF` (child never attached) · no `read` at all
  (nothing forwarded) · `read` without `feed` (channel/pump) · `feed` with a blank grid (the
  render side). The `[grid]` line covers what a byte trace cannot — a mis-measured grid puts
  output off-screen and looks exactly like no output. Bytes are escaped, because an
  unescaped VT trace drives the terminal reading it. **This is the instrument that found the
  DSR-CPR stall** described above: four bytes in and then silence, with sane `[grid]`
  metrics eliminating the render side in the very same trace.

## 2. Seams the next tiers consume

| Coming | Builds on | Issue |
|---|---|---|
| Viewport interaction + provenance (T2+) | T1's pane (`shell_main.rs::ScenePane` + `app.rs::SceneView`); camera input rides `scene_input`'s region pattern — never a second gesture vocabulary. The world gate is already `any(mind, shell)`; `World` stays unforked (#618 owns its extraction) | Shell #6 |
| Content-addressed artifact store + lifecycle UI + evidence viewers | `session::Artifact` (metadata landed in #4 T1); payloads beside the log in the session dir | Shell #4 T2+ |
| Command service T2+: core_catalog seeding + real targets | `command::CommandService` landed in #5 T1 (dispatch + catalog + the every-dispatch-leaves-a-record invariant); T2+ adds the bin-side `core_catalog`→`CommandSpec` adapter, the runtime target over the CLI override lane + snap request/reply sidecar, and the policy engine that makes `Denied`/`Requested` real — never a second vocabulary | Shell #5 |
| Pi bridge / workers / PTY | T1 landed the workspace side (`mock_agent.rs` + `timeline.rs`: every `EventKind` rendered, pull-tick replay). Next: a real adapter *behind the same tick shape*, approval decisions routed back as events — never a second event vocabulary | Shell #7 T2+ |

**IPC rule inherited whole:** any new Shell channel — mmap, sidecar, socket — goes
through `ipc.rs::ns_file` under the `organon-shell` namespace. A hard-coded `$TMPDIR`
path silently breaks the three-products-simultaneously guarantee that
`edition.rs`'s pairwise-distinct-namespace test pins.

## 3. Honesty ledger

- **The backdrop is the DEFAULT LOOK of the engine**, not a live external system:
  Shell writes the default `Shared` itself and the CLI's override lane mutates the
  world's working copy. Provenance for showing any *external* system's state in the
  backdrop or blocks is later-tier work, never implied by pixels existing.
- **The legibility scrim's floor is structural** (clamped in code) — no
  configuration can trade the glyphs away.
- **⌘-keys never reach the PTY** — a harness cannot see or shadow the host's tab
  chrome, and the host never steals bare-Ctrl from the harness.
- The mock-agent demo machinery (v2) retains its rule if ever re-homed: a replay
  is labeled a replay, on its face.
- The CLI op-path prints a cosmetic "queued" warning in-Shell (its liveness
  heuristic reads the Feedback channel Shell does not yet write) — the ops drain
  fine; silence it by writing Feedback, not by patching the CLI.
- **A WSL harness's "installed" only proves the BRIDGE.** `pi-wsl` and friends
  detect on `wsl.exe`, not on the harness existing inside the distro — probing that
  means booting WSL on every launch. So a WSL row can be selectable and still fail
  to start; the spawn error names the check to run. Do not read the + menu as a
  claim about what is installed in Linux.
- ✅ **The terminal's byte path is now MEASURED on Windows — it was a missing DSR
  reply, and neither of the two candidates this entry used to list.** The entry
  offered (a) the test's `cmd.exe /C` argv being eaten by quote-stripping, a test bug,
  or (b) Shell's reader not draining ConPTY, a product bug. **Both were wrong.** A
  trace on organon-one (2026-08-08, `ORGANON_SHELL_PTY_DEBUG=1`, #697's instrument)
  showed the reader draining perfectly and the child never speaking: `read 4 (total
  4): \x1b[6n`, then nothing — byte-identical for a `cmd.exe /C` tab and a real
  `powershell.exe -NoLogo` tab, which is what rules the argv out. ConPTY was blocked
  on the DSR-CPR reply `VoidListener` discarded; `PtyReplies` (§1) sends it.
  `echo_lands_in_the_grid` is un-ignored and passes **3/3 in 0.05 s** where it
  previously failed 3/3 at its full 5 s deadline, on an idle machine both times.

  📌 **What this still does not cover.** That test asserts one command's output
  reaches the grid. It is not a claim about interactive shells, WSL harnesses,
  resize-under-load, or anything a person would call "using a terminal" — those
  remain unexercised by CI, and the entry below stands.
- 🔬 **`bash -lic` is validated, measured on organon-one 2026-08-08.**
  `wsl.exe -- bash -lic "command -v <tool>"` resolved to a path under
  `/home/linuxbrew/.linuxbrew/bin/`. That PATH exists only because `brew shellenv`
  ran from the rc files, so a bare `bash -c` would have found nothing — which is the
  evidence for the login-*and*-interactive wrapper above. Kept because it is a
  standing fact about the WSL harness, independent of the blank-grid story it was
  gathered during.
- **Windows is BUILT and only NARROWLY RUN by this project's own verification.** The
  launch logic is unit-tested for both platforms from any host, CI builds the Windows
  leg, and the byte-path test now runs there — but nothing in CI exercises the harness
  or WSL paths, and the ConPTY handshake above went undetected for three review cycles
  precisely because a green Windows leg was read as coverage it never had. Windows
  behaviour beyond that one test is confirmed by a person on the machine; a green CI
  leg is not that confirmation.
- ⚠️ **#695 ("the reader never sees EOF on Windows") may be a phantom, and should be
  re-measured before it is fixed.** Its evidence is that `exited` never became true —
  which the DSR stall above fully explains, since a child whose output ConPTY never
  forwards also never reaches EOF. The `drop(pty.slave)` / shared-`Arc<Mutex<Inner>>`
  theory may still be correct, but nothing distinguished the two until the handshake
  worked. Re-run the observation on this fix before writing code against it.
