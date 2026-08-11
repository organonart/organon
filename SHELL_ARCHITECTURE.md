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
>
> ⚠️ **The binary is `organon-console`; everything else is still `organon-shell`.** The
> artifact carries the public name (`cargo build --features shell-edition --bin
> organon-console`); the crate `native/organon-shell`, the `shell-edition` feature, the
> `ORGANON_SHELL_*` variables, the `organon-shell` IPC namespace and this file's name
> keep the working one, because each is read by something else. The gap is deliberate —
> issue #3 owns closing it with deprecation aliases, not find-and-replace.

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
- **The living backdrop (#14 T1 + Console Spike T1, in `shell_main.rs`)** — a frame
  rendered each redraw and painted UNDER the glyphs (the measured
  render-sRGB/sample-linear gamma pair, same-id rebinds). Summoned, never imposed, and
  now with **two sources** chosen by `ORGANON_SHELL_BACKDROP`: unset/`0` off, `1` the
  live `World` exactly as before, `substrate` a flat lit plane. The World stays
  selectable on purpose — the CLI's override lane drains inside `World::frame_body`, so
  a substrate that *replaced* it would silently kill `organon set`/`generator`/`recipe`.
  The substrate is `substrate_scene::apply_substrate_look` written **once** into the
  `Shared` the console already publishes every frame, framed by
  `substrate_camera::SubstrateRig` at a 10° **vertical** lens and re-framed each frame
  from the pane's aspect. That rig reaches the engine through a **third arm on
  `world.rs`'s camera finalization** (beside rails), overriding all six of
  `(centre, yaw, pitch, distance, roll, fov)` and latching off the `cam_center`
  auto-follow while installed; the FOV clamp floor moved 10° → **4°** at *both* sites that
  clamp it (the finalization and `build_uniforms` — moving one is a silent no-op). The
  key azimuth is the one look value the shell overrides, because "above-left" is a fact
  about the camera and not about the light: under this top-down rig screen-up is world
  −Z, which puts above-left at −135°, not the −10° a 40°-yaw camera wanted.
  **The texture is sized to the terminal pane, not the window** — Console Spike T1's bug
  fix. It is painted at UV 0..1 into a `CentralPanel` already 30 points shorter than the
  swapchain, so a window-sized texture was vertically squashed; invisible on a
  generative world, glaring on a flat plane. Sized one frame behind and clamped,
  `wgpu_editor::render_scene_pane`'s pattern. **This changes `=1`'s rendering too**, and
  is meant to.
  The **legibility scrim** is structural: `ORGANON_SHELL_SCRIM` tunes, but
  `term_view::scrim_alpha` clamps at a floor no setting crosses — now a pure function
  with that floor pinned by a test rather than an unguarded expression inside `draw`.
  The bin negotiates the FULL engine feature set (bind groups, RT, timestamps) — a
  default-limits device opens a window and then fails to create engine pipelines.
  **Console Spike T2 dressed that plane and made the choice live.** Four materials and
  two lighting rigs (`substrate_materials.rs`: `graphite` / `paper` / `slate` / `metal`;
  `studio` / `daylight`), each a pure delta on the T1 snapshot, written by
  `apply_material` / `apply_rig`. They are only visible because the **#472 map gate
  moved in the same tier** — `render.rs` split one predicate into `cube_draw` (the bevel
  morph, unchanged) and `material_draw`, which now admits the Membrane sheet; see
  `doc/arch/render.md` §"Which draws sample the #472 material set". The snapshot is
  recomputed from one pure function of `(source, material, rig)` — `look_shared`, order
  **look → material → rig → the camera's key azimuth** — so every change is a fresh
  derivation rather than a patch on the last one, and `world`/`off` publish exactly
  today's default bytes however the console is dressed. Startup applies **no** material:
  `ORGANON_SHELL_BACKDROP=substrate` is byte-identical to T1 until a command arrives.
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
  cosmetic: the CLI's op-path liveness heuristic (`ipc::Reader::is_live`) samples
  the `Shared` seqlock for motion; Shell bumps it once per redraw, so a console
  that is repainting reads live and one whose redraws have stalled can print a
  spurious "queued" warning (Phase 0 correction, 2026-08-10 — this line previously
  blamed the Feedback channel, which `is_live` never reads).
- **The console command lane (#4 T2, `cli.rs` + `shell_main.rs`)** — the first typed
  sentence that changes the console: `organon console background <name>` and
  `organon console rig <name>`. **A third transport, because it has a third
  destination.** `cli.txt` is drained by the `World` and the eyes sidecar is answered by
  the visual; a backdrop is `Shell` state and neither of them can reach it, so routing a
  console verb over the existing lane would queue it where nothing can act on it —
  green, silent, wrong. The lane end to end — **validate · write · drain · validate ·
  apply**:

  1. clap's `PossibleValuesParser` over the material / rig / source lists rejects a bad
     name *before a byte is written*, with exit 2 and "did you mean" for free.
  2. `cli::append_console_ops` appends one line per op to `cli::console_cmd_path()` =
     `ns_file("console.txt")` — append-only UTF-8, verb first, no JSON.
  3. `Shell::drain_console` reads it once per frame on the **file-length watermark**,
     reusing `agent::cli_drain_step` + `agent::cli_seed` verbatim (the CLI is never an
     IPC writer, so there is no `seq` to watch). Seeded at construction from ONE read: a
     backlog from before the process existed never replays, a command typed a moment
     after launch always drains.
  4. `CommandService::dispatch` re-validates against a `Choice` schema built from the
     same tables, and records the run.
  5. `console_step` folds the op into `(source, look)`; `look_shared` recomputes the
     published `Shared`.

  It drains **immediately before** the per-frame `Shared` publication, so a command
  reaches the World on the frame it arrives, not the next one. **Versioning is the
  verb**: `parse_console_op` returns `None` for anything it does not know and the drain
  skips the line, so a newer `organon` against an older console degrades to "that op did
  nothing" instead of poisoning the rest of the drain — and `console_step` carries the
  same contract down to the *argument*, so an unknown material leaves the backdrop
  exactly as it found it. Nothing else moves on a switch: the backdrop texture is keyed
  on the pane size, which a command does not change, so no texture is recreated, no egui
  id is rebound and no glyph is re-laid-out.
- **The landed v2 foundations** (session/event log with torn-tail recovery, the
  typed command service, mock-agent event cards) remain in the crate, feeding
  trees C/D. **`command::CommandService` is no longer only its own tests**: #4 T2 stands
  up the product's first live instance, registering `console.background` and
  `console.rig` (`TargetKind::Viewport`, one required `name` of `ArgKind::Choice` built
  from `substrate_materials`' own tables) and routing every drained op through
  `dispatch`, so each one leaves a `CommandRun` record in a real `SessionLog`. Two
  shapes worth knowing: the service is built **per batch** rather than held on `Shell`
  (it borrows `&mut SessionLog`, and a struct holding both would be self-referential —
  `command.rs` says as much: "the log outlives any one service"), and its target
  **banks** the validated ops for the caller to apply, because `Box<dyn CommandTarget>`
  is `'static` and cannot hold `&mut Shell`. The op that gets applied is the op the
  service handed back, so dispatch is in the path, not beside it.
- **Dev flags**: `ORGANON_SHELL_CMD` (one plain-command tab, headless proof),
  `ORGANON_SHELL_TABS` (comma harness ids), `ORGANON_SHELL_DEFAULT`,
  `ORGANON_SHELL_BACKDROP`, `ORGANON_SHELL_SCRIM`, **`ORGANON_SHELL_PTY_DEBUG`**.
  **`organon-console --help` is their documentation** (2026-08-09) — the binary had no
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
| Command service T2+: core_catalog seeding + real targets | `command::CommandService` landed in #5 T1 (dispatch + catalog + the every-dispatch-leaves-a-record invariant) and is **live in the product since Console Spike T2** (`console.background` / `console.rig`, seeded from `substrate_materials`' tables, dispatched from the frame path). T2+ adds the bin-side `core_catalog`→`CommandSpec` adapter, the runtime target over the CLI override lane + snap request/reply sidecar, and the policy engine that makes `Denied`/`Requested` real — never a second vocabulary | Shell #5 |
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
- **The substrate backdrop is a LOOK, not a system** (Console Spike T1) — one flat lit
  plane, written into the same default `Shared` the console publishes. ✏️ **T2 lifted the
  gate this entry used to end on**: `render.rs` no longer forces `mtl[0] = 0` on the
  Membrane path, so the four materials really do reach the sheet. What carries the read
  is still the narrow lens (the frustum's diagonal half-angle **is** the shading gradient
  on a flat plane — ≈10.1° at 10°/16:9), now plus a procedural map stack. It is a
  surface, never a readout: no material or rig name means anything about a system.
- 🚨 **No GPU has seen Tier 2's materials.** The layer shapes, channel routing and
  sampling rates are derived from the bake shader and `params.rs`'s range table and are
  pinned by tests; the *taste* — how dark graphite is, how much sheen metal keeps — is
  chosen and unverified. Each material names its one dial in its doc comment for exactly
  that reason (`substrate_materials.rs`). The suite is green; that is not "verified
  working", and the coordinator's beat check is the first time any of it is seen.
- **The material texel rate is tied to the pane's pixel height.** The baked channel maps
  are 512² with **no mip chain**, and `MATERIAL_UV_SCALE` is already at its declared range
  floor (0.02), so a 1080-px pane samples at ≈1.20 texels/px and a 540-px pane at ≈2.4,
  where the fine layers will sparkle. No `Shared` field can fix it; the fixes are mipmaps
  on `MaterialBaker::make_target` or a coarser `mp_scale`, both outside this tier.
- **The console's name lists are guarded in three places and *bound* in two.** The
  materials and rigs are bound: `bin/ctl.rs`'s clap lists are asserted equal to
  `substrate_materials::MATERIAL_NAMES` / `RIG_NAMES`, and `shell_main.rs`'s command
  schema is built from those same constants and asserted to accept exactly what its
  resolver does. ⚠️ **The three source words are not.** `world`/`off`/`substrate` are
  `BackdropSource`'s value space, which lives in `shell_main.rs` — another `[[bin]]`, and
  no `bin` can import another — so the literal appears twice (`CONSOLE_SOURCES` there,
  `BACKDROP_SOURCE_WORDS` here) with each side asserting it against its own resolver.
  Change one and that side's test fails naming the other: two smoke alarms, one missing
  wire. The fix is a `pub const` in `cli.rs` beside `parse_console_op`, already the
  declared home of "both ends speak one vocabulary from one place"; it was left undone
  because `cli.rs` is another leaf's file this tier only reads.
- **A console command applies whether or not it can be recorded.** Every drained op goes
  through `CommandService::dispatch`, so in the normal case it leaves a `CommandRun` in a
  real `SessionLog`. If the store cannot be opened there is no service to dispatch
  through, and the console **still applies the command** — the shortfall is announced once
  on stderr at startup and recorded here rather than hidden. It is not a silent
  equivalent: the apply path is total over its own vocabulary (an unknown name changes
  nothing), so schema validation is defence in depth, not the only gate.
- **The session log is per console process and nothing prunes it.** Each launch opens
  `<data dir>/OrganonShell/sessions/console-<unix ms>-<pid>/events.jsonl`. Per-process is
  required — two consoles co-existing is the whole point of the IPC namespace fork, and
  sharing one file would give them two independently-advancing `seq` counters — but it
  means a directory per launch, with no retention policy yet.
- **A command that arrives while the console is occluded waits for the next frame.**
  `redraw` returns early on an `Occluded` surface, before the drain, so a `background`
  typed at a hidden window applies when it is shown again. Same behaviour the World has
  for `cli.txt`, and invisible by construction — there is nothing to see while occluded —
  but it is a real "under a second" caveat if anything ever scripts this.
- **`Issuer::Worker("organon-cli")` is what is KNOWN, not who acted.** A line on the
  console sidecar could have been typed by a person in a tab or written by an agent in
  another process, and the console cannot tell. The record names the transport rather
  than guessing a person; do not read it as attribution.
- **The backdrop's framing is verified by arithmetic, not by eyes.** `SubstrateRig`'s
  coverage guarantee is a headless test; whether the plane actually fills the pane also
  depends on the texture being **pane-sized**, and nothing in the tree tests that seam
  (Phase 0 R1 said so). The two landed in the same tier for exactly that reason — the
  beat check is what closes it, not the suite.
- **⌘-keys never reach the PTY** — a harness cannot see or shadow the host's tab
  chrome, and the host never steals bare-Ctrl from the harness.
- The mock-agent demo machinery (v2) retains its rule if ever re-homed: a replay
  is labeled a replay, on its face.
- The CLI op-path can print a cosmetic "queued" warning in-Shell — the ops drain
  fine. ✏️ **Phase 0 (2026-08-10) corrected this entry:** `is_live()` probes the
  `Shared` seq counter for motion (`organon-core/src/ipc.rs`) and never reads the
  Feedback channel, and Shell **does** write `Shared` each redraw
  (`shell_main.rs` — the publish under `redraw`). The warning is a redraw-cadence
  artifact, not a missing channel. Silence it by keeping `seq` moving while ops
  are pending (or teaching the probe about event-driven writers) — not by writing
  Feedback, and not by patching the CLI. Measured during Tier 0: with the
  backdrop animating, in-console `status`/`recipe` ran with no spurious warning.
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
