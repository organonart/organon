# Organon Console — living architecture

> **What this is.** The code-grounded state of **Organon Console** as it exists *right
> now* — not a spec, not a roadmap. It is the `MIND_ARCHITECTURE.md`-shaped sibling on
> the code side of Console's product definition (`doc/organon_console_prd.md`, v3.2 — the
> TUI host) and build plan (`doc/organon_console_buildplan.md`), both of which are part
> of the private annex and do not travel with this file. **Update it in the same change
> as every Console PR** — a Stop hook (`.claude/hooks/doc-rules.sh`) reminds you when
> `native/organon-console/*` moves without it.
>
> **`Console #N` in this file, and in the crate's doc comments, is a Console work item** —
> a tree in the tracker Console was planned in, not an issue in this repo. Bare `#N`
> means this repo's tracker. Both are kept as provenance; neither is a link.
>
> **Not auto-injected.** Open it deliberately when working on Console, like
> `MIND_ARCHITECTURE.md` and `doc/arch/render.md`.
>
> ⚠️ **The binary is `organon-console`; everything else is still `organon-console`.** The
> artifact carries the public name (`cargo build --features console-edition --bin
> organon-console`); the crate `native/organon-console`, the `console-edition` feature, the
> `ORGANON_SHELL_*` variables, the `organon-console` IPC namespace and this file's name
> keep the working one, because each is read by something else. The gap is deliberate —
> issue #3 owns closing it with deprecation aliases, not find-and-replace.

---

> 📦 **The substrate modules live in `organon-scene` now** (organon#49 Tier 3, 2026-08-14).
> `substrate_scene`, `substrate_materials`, `substrate_camera`, `substrate_epochs` and
> `overlay_meta` moved out of the root crate into a crate carrying **no nih-plug, no wgpu,
> no egui** — a step on #49's route to a Console binary that is not a GPL artifact of the
> VST3 crate. Every `crate::substrate_*::…` path in the root crate still resolves through a
> re-export, so the descriptions below are unaffected; only the *home* changed.
>
> ⚠️ `scene_input` did **not** move — it reaches egui, and it travels with `world.rs` in
> Tier 4.

## 1. What exists right now (the terminal form — trees A/B/E Tier 1 + the landed v2 foundations)

**Organon Console is a next-gen TUI host** (PRD v3.2 §1.2, reframed 2026-08-08): tabs of
agent harnesses — Pi first — drawn as a real GPU terminal, with the Organon engine
available behind the glyphs. The bare shell is one menu entry, not the opening
position.

- **The terminal core (#10 T1, `term.rs` + `term_view.rs`)** — `portable-pty` 0.9
  spawns the session (login shell or a harness command via `zsh -lc "exec …"`,
  `TERM=xterm-256color`); `alacritty_terminal` 0.26 (vte 0.15) is the VT state
  machine, advanced by a **lock-free pull loop** (reader thread → channel → the UI
  thread owns parser+`Term` exclusively, drained once per frame). `term_view` draws
  the grid through egui as per-line same-style runs: full color stack (phosphor
  the theme's ANSI 16, xterm-256 pinned by test, truecolor, inverse/dim, OSC overrides), block
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
  install URLs, and **drops downward from the button's bottom edge** — the one
  place the strip's position leaks into `tabs.rs`, which is why it is written
  here. It grew *upward* for a while after the strip moved from the bottom of the
  window to the top, and rendered acceptably anyway because egui clamps an
  off-screen `Area` back into the screen rect; the anchor now derives from the
  button, so a change to the strip's height cannot re-open that. ⌘T/⌘W/⌘1-9/⌘⇧[] via a pure, tested key table — which **takes a held key's
  `repeat` flag and answers it per action**, see below. Default tab =
  `$ORGANON_SHELL_DEFAULT` → Pi if installed → plain shell. All sessions pump
  every frame; the active one draws; closing the last quits.

  🚨 **A held ⌘ chord streamed one action per repeat, and `command_key_action` now
  decides which of them may.** Holding a key produces a run of `pressed: true`
  events; `egui-winit` discards winit's own repeat flag and pushes each one plainly,
  and `InputState::begin_pass` then sets `repeat = !first_press` and **leaves the
  event in the stream** — so the frame loop's `Event::Key { pressed: true, .. }`
  saw an unbroken run of fresh presses. `action.is_none()` bounds that to one per
  frame, which is a **rate and not a total**: autorepeat is slower than the frame
  rate, so a resting finger landed roughly one action per repeat, indefinitely.
  Reproduced through a real `egui::Context` and pinned by
  `a_held_command_key_reaches_the_frame_loop_as_a_run_of_presses` — driving the
  actual library rather than a hand-set flag, because the claim under test *is*
  egui's behaviour. (Read as pre-existing during #77's review, which fixed only its
  own chord rather than fold an unrelated behaviour change into that PR.)

  ⚠️ **"Ignore repeats" is the wrong answer for half this table, so the policy is a
  property of the ACTION, not of the key.** `Switch` **honours** a repeat: ⌘⇧[/]
  cycling while held is what a cycle chord is *for*, and ⌘1-9 is idempotent to the
  point of being free — the host answers it with `strip.switch(i)`, one index write,
  so the thirtieth repeat writes the number the first one did. `New` and `Close`
  **refuse** it: `New` spawns a PTY per event, `Close` drops a session, frees its
  textures and quits the console once the last tab goes, and neither is recoverable.
  Keying on the action means a chord added later inherits the right answer without
  anyone remembering to ask, and the match is **exhaustive over `TabAction`** so a
  *variant* added later fails the build rather than defaulting silently. ⚠️ The flag
  is forwarded from the call site, never filtered there — a `repeat &&` guard around
  the call would have to re-state which chords it applies to, which is the copy that
  drifts.
- **The living backdrop (#14 T1 + Console Spike T1, in `console_main.rs`)** — a frame
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
  ⚠️ **And sized as the pane's *share of the window*, never as points times a remembered
  scale** (`scene_input::pane_pixels_in`). The scale that converts points to pixels is an
  egui frame *output*, so a caller that remembers one starts from a stand-in — and the
  stand-in that reads harmlessly (`1.0`) multiplies exactly like a real 100 % display, so
  the backdrop comes out sized in **points**: 1100×690 where 2475×1553 was meant, 2.25×
  too small in each axis on ORGANON-ONE's 225 % display. The live texture rebinds itself
  the moment a real scale lands, which is why nobody saw it — but T4's epoch cache
  *copies* that texture, so a look closing inside that window filed a picture 2.25× too
  small and every band painted from it stayed magnified for the session. A pane/window
  **ratio** applied to the swapchain (physical by definition, correct before any scale is
  known) makes the error cancel instead of being guessed.
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
  ⚠️ **This exists because Console shipped unable to open a shell on Windows while the
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
- **The self-steering loop (#14 T1)** — Console publishes the default-look `Shared`
  snapshot each frame (seqlock `ipc::Writer`, `organon-console` namespace), so the
  `organon` CLI works from *inside* the terminal: `status`/`get`/`watch` read it,
  and the override lane (`set`/`generator`/…) drains in the world's frame path —
  verified live: a CLI generator switch changed the running backdrop. ✅ **It now
  works unattended.** Until 2026-08-08 that verification required a manual
  `export ORGANON_IPC_NS=organon-console`: the PTY was given only `TERM` and
  `COLORTERM`, so an `organon` invocation inside a Console tab resolved the **default
  `organic-math` namespace** and addressed a different product entirely. `term.rs`
  now injects `ORGANON_IPC_NS = ipc::namespace()` into every session. Known
  cosmetic: the CLI's op-path liveness heuristic (`ipc::Reader::is_live`) samples
  the `Shared` seqlock for motion; Console bumps it once per redraw, so a console
  that is repainting reads live and one whose redraws have stalled can print a
  spurious "queued" warning (Phase 0 correction, 2026-08-10 — this line previously
  blamed the Feedback channel, which `is_live` never reads).
- **The console command lane (#4 T2, `cli.rs` + `console_main.rs`)** — the first typed
  sentence that changes the console: `organon console background <name>` and
  `organon console rig <name>`, joined since #38 by `organon console theme <name>` and
  `organon console posture <word>` — the console's own dressing rather than the
  substrate's, and the first two verbs on this lane that a person reaches for because of
  how the window *looks to them* rather than what it is showing. **A third transport,
  because it has a third
  destination.** `cli.txt` is drained by the `World` and the eyes sidecar is answered by
  the visual; a backdrop is `Console` state and neither of them can reach it, so routing a
  console verb over the existing lane would queue it where nothing can act on it —
  green, silent, wrong. The lane end to end — **validate · write · drain · validate ·
  apply**:

  1. clap's `PossibleValuesParser` over the material / rig / source lists rejects a bad
     name *before a byte is written*, with exit 2 and "did you mean" for free.
  2. `cli::append_console_ops` appends one line per op to `cli::console_cmd_path()` =
     `ns_file("console.txt")` — append-only UTF-8, verb first, no JSON.
  3. `Console::drain_console` reads it once per frame on the **file-length watermark**,
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
- **Look-epochs — the backdrop applies FORWARD, history keeps its own (#4 T4)** — a
  `background` or `rig` command closes the live look at the current absolute scrollback
  line and opens the next one **just below the cursor**, so the new look scrolls in from
  the bottom as output pushes the old text up, and every older region of the buffer keeps
  the look it was written under. Nothing is ever restyled after the fact; there is no
  restyle-everything path and building one was explicitly declined (the execution plan's
  ⚡ Sequence amendment).

  Three pieces, each of which knows nothing about the other two:

  1. **`scroll_anchor.rs` (organon-console) — the arithmetic.** Absolute line indices:
     `abs = screen_top + grid_line`, `screen_top = dropped + history_size`, **derived every
     frame rather than accumulated**, which is what makes emission age a boundary for free,
     scrolling move the window rather than the text, and a *row* resize need no bookkeeping
     at all (`grow_lines` pulls lines out of history and the cursor follows by the same
     count). Bands partition the viewport, edges monotone, and the **alt screen is always
     exactly one band** — the alt grid is built with zero scrollback, so its geometry says
     nothing about absolute lines. No egui, no alacritty, no wgpu.
  2. **`substrate_epochs.rs` (`organon-scene`) — the ledger.** Which look ran from which line,
     `Look` = `(material, rig)` **names**, never bytes. `MAX_EPOCHS = 8`, which is a
     stateable ceiling rather than an adjective: 63.3 MiB of pane-sized RGBA8 at 1080p,
     253.1 MiB at 4K (`worst_case_bytes`, pinned by test). Past the cap the two oldest
     epochs **merge**, the **newer** of the pair surviving so the loss concentrates on the
     rows furthest from the cursor, and the loser comes back carrying the exact stderr line
     to print (`[epochs] evicted graphite/studio @ line 1200 (cap 8)`). `plan()` returns
     the texture decisions as data — releases first, the live epoch second, the rest oldest
     first. It owns no GPU object and no scroll geometry.
  3. **`console_main.rs` — the wiring.** One `PaneLooks` per tab (anchor + ledger + texture
     cache), **index-aligned with `sessions`**, because scrollback is per tab: each pane
     records the same look change at its own cursor, in its own coordinate.

  Four things about that wiring are load-bearing:

  - **Snapshot on close is the only way a picture is ever made.** When a look stops being
    live, the backdrop texture *is* its rendering — so it is copied
    (`copy_texture_to_texture` into a `COPY_DST` twin, the same `Rgba8UnormSrgb` storage
    with an `Rgba8Unorm` sample view, `COPY_SRC` newly added to the live texture) and keyed
    by `EpochId` **before the new look's first frame renders**. This happens in the command
    drain, which runs before `render_backdrop`, i.e. in the last frame that picture exists.
    Ids, not band indices: an eviction shifts every index and would look like "all of them
    changed". One copy per change for the whole window, shared across tabs by `Rc` and
    freed when the last tab drops it.
  - **One counter, one site.** `term_view::PaneAnchor::bracketed` is the only place the
    anchor's `dropped` advances, and every parser advance the console makes goes through it —
    the redraw loop pumps every tab, `term_view::draw` pumps the active one again, and T5's
    `feed_local` advances it a third way, so "exactly one site" has to mean one *function*,
    not one call. The bracket holds the advance and nothing else: `resize` moves
    `history_size` and the wheel moves `display_offset` without a line being emitted, and
    either inside it would be counted as output.
  - **The band table drops the ledger's first boundary.** The ledger records the line every
    epoch opened at, including the oldest; `scroll_anchor` counts boundaries at or below a
    line, so it wants only the changes *between* looks (`textures.len() ==
    boundaries.len() + 1`). Handing it the list unfiltered shifts every row one epoch
    younger — uniformly and plausibly. `every_row_paints_the_look_it_was_written_under`
    checks each visible row against the ledger's independent `band_for_line`.
  - **`background world` / `off` collapse the history** rather than adding a look. A live
    World is not a still life and freezing a frame of it would be a lie labelled a look;
    `off` has no picture at all. Both keep one live epoch and log every epoch they forget.
    `off` additionally takes **no** snapshot when it later gives way to a substrate look —
    `render_backdrop` returns before rendering while it is off, so the texture still holds
    the look from *before* it, and copying that would file a picture under rows it was
    never behind. Those rows keep the plain background, which is what they were written on.
- **Reserved rows — a hole in the transcript that text flows around (#4 T5, first
  increment)** — `organon console block <rows>` opens a contiguous run of blank rows in the
  **active** tab, just below the cursor, and the next prompt lands underneath it. Nothing is
  painted into them yet; making the rows genuinely exist is the increment, and a GPU texture
  pinned into them is the next one.

  **The mechanism is the parser the console already owns.** `TermSession::feed_local` advances
  `vte::ansi::Processor` against bytes the console generated itself — the same call `pump`
  makes, so a `\r\n` fed here goes through `Handler::linefeed` → `Term::scroll_up` →
  `Grid::scroll_up` exactly as the shell's own newline does. There is no second representation
  of a row and no second set of invariants: a reserved row ages, scrolls, evicts at the cap and
  reflows on a width change **because it is an ordinary scrollback row**. Three things it is
  deliberately not:

  - not `TermSession::input`, which writes to the pty **master** — that is input *to the
    child*, and N newlines there returns N shell prompts, not N blank rows;
  - not `Handler::insert_blank_lines` (IL), which pushes rows off the bottom and discards
    them, so nothing enters history;
  - not `grid_mut().scroll_up()`, which bypasses selection rotation, the vi cursor and damage
    tracking, and does not move the cursor.

  `term::block_bytes(rows)` is the sequence, pure and pinned by a test: `rows + 1` repetitions
  of `\r\n\x1b[2K`. The `\r` because `linefeed` does not reset the column; the EL because a
  linefeed only *blanks* the row it enters when the grid actually scrolled, and reserving means
  claiming rather than passing over; the **+1** because the last linefeed is what puts the
  cursor *below* the block instead of on its final row.

  🚨 **The feed must be bracketed exactly as a real pump is**, and that is the one hard
  constraint in the increment. `TermSession::feed_local` is **`pub(crate)`**, so from outside
  `organon-console` — which is where the whole console lives — it is unreachable; the only caller
  is `term_view::PaneAnchor::feed_local`, and `PaneAnchor::bracketed` is now the single function
  both it and `PaneAnchor::pump` route through. Unbracketed, a feed against a full buffer with
  the user scrolled into history evicts lines that `advance_dropped` never sees, which raises
  the true `screen_top` without raising the derived one — and every absolute index recorded
  before the feed is then permanently wrong, silently.

  `PaneAnchor::feed_local` returns the **absolute line index of the first reserved row**, taken
  from the *pre-feed* `ViewState` by the same `boundary_now` a look-epoch uses. The identity a
  painter gets: the block occupies `at ..= at + rows - 1` and the cursor rests on `at + rows`.
  `Console::open_block` logs it unconditionally — `[block] opened 12 rows @ line 1187 (pane 0)`,
  `[block]` being the tag to grep, in `[epochs]`' register and for its reason: an arithmetic
  error here is invisible until something is painted into the wrong rows.

  **The active pane only** — the opposite of a look change, and for the same reason it is the
  opposite. A look is the window's and every tab must paint its own rows under it; a block is a
  hole in *one* transcript, asked for by someone looking at one tab.

  Known limits, all accepted rather than pending (they are written out in
  `PaneAnchor::feed_local`'s doc, in `scroll_anchor.rs`'s register):

  - **A width change reflows and the anchor drifts.** Row/height resize is exact; a width
    change re-wraps every wrapped line above the block, so its index slides by the net wrap
    delta — and `grow_columns` can drop the block's topmost row outright, because a
    `row.is_clear()` row is skipped rather than pushed and a reserved row is clear by
    construction. The policy is **a width change invalidates a block**; it is recorded here,
    not implemented yet.
  - **Eviction erodes a block from the top**, one row at a time, and at the live edge
    evictions are not observable at all — so the console can believe an eroded block is whole.
  - **`\x1b[3J` wipes the scrollback silently** (`grid.clear_history()`); `clear` and `reset`
    emit it.
  - **A resize while the alternate screen is up moves the primary grid invisibly.**
  - **Feeding under the alt screen writes into the alt grid**, which has no scrollback, so the
    returned index describes a row the feed did not touch. A block during a full-screen
    application is meaningless.
  - **The sidecar is drained once per frame and is out of band with the PTY byte stream**, so
    the index a block gets is "wherever the cursor was at drain time" — correct only while the
    child is idle. The in-band fix is a private OSC scanned in `pump`, so the console learns
    the position *from the byte stream itself*, in order with everything else on it. A later
    increment.

  On the CLI side the verb is `ConsoleOp::Block(u16)`, wire form `block <n>`, bounded by
  `cli::MAX_BLOCK_ROWS = 200`. It is the **first op on this lane whose argument is a number**,
  which costs two things: `parse_console_op` skips a count that does not parse or does not fit
  (a malformed line is skipped like an unknown verb — never clamped, since a clamp opens a
  block nobody asked for), and the range is gated **twice on purpose** — clap's `value_parser`,
  where a human gets a good error before a byte is written, and `op_from`, where a
  hand-written sidecar line meets it, because `ArgKind::Int` carries no bounds and the schema
  cannot state the range the way a `Choice` states a table.
- **Patches — a claimed rectangle, and the kind that says what fills it (#4 T5)** —
  `organon console patch --up N --rows M --kind <scene|panel>`. This, and not `block` above,
  is the mechanism: **the writer prints its own gap** as ordinary blank lines through the
  ordinary PTY — rows the shell, ConPTY and the console all agree exist, because they arrived
  the normal way — and then says where it is. The console writes **nothing** into the
  terminal, ever. `up` counts back from the line the claiming command is being run from, so a
  program that prints twelve blank lines and immediately claims `--up 12 --rows 12` names
  exactly the rows it just made.

  🚨 **`block` is not a fallback for it, and the difference is the whole mechanism rather than
  a refinement.** `block` has the console *feed* rows at the cursor — but the cursor is by
  definition the live input point, the row a prompt is waiting on and a keystroke lands in, so
  feeding there opens the hole **between the prompt and the typing**, which no terminal does.
  Measured 2026-08-11: prompt stranded above an eight-row hole with the cursor below it, worst
  precisely when the shell is idle, and against a real Claude Code tab the harness's whole
  frame shifted and it repainted over everything. `block` is kept only for a shell that is
  provably idle; there is no console-side injection that can be correct.

  **A patch has a kind, and the kind selects the paint and nothing before it.** The claim, the
  anchor arithmetic (`block_anchor`) and the per-pane ledger are common to every kind; the
  first read of the kind is at paint time. That split is where the correctness lives: an error
  in the shared half puts a rectangle at the wrong line and *looks like a rectangle*, so it
  gets one implementation and the exhaustive sweep in `block_anchor`'s tests, while an error
  in the paint is something a person can see.

  - **`scene`** — the rendered substrate, sampled through the rows by `term_view::block_quads`
    on `band_quads`' UV policy: a patch is a *window*, not a thumbnail, so it shows the slice
    of the picture that sits where it sits and the surface stays put as the transcript moves
    over it. It needs no second `World` and no `Shared` override — with the backdrop `off`,
    `Console::render_source` renders the substrate into the pane target and only the patch quads
    sample it, so the window behind the text stays the flat black of an ordinary terminal.
  - **`panel`** — a live egui control panel: `block_panel::draw` builds a child `Ui` at the
    patch's rect, with sliders that move and a row of buttons wired to the console's real
    look-change path. **Not a texture**, deliberately: the console's whole frame is already
    one egui pass, so a patch is a rect inside it and a child `Ui` is the entire mechanism —
    no `TextureId`, no readback, and the controls are alive rather than a photograph of
    controls. Content is laid out into the block's **full** rect and clipped to the **visible**
    one, so a half-scrolled panel is *cut* rather than squashed; egui intersects a widget's
    rect with the clip rect to decide what the pointer can reach, so the same rect is the paint
    boundary and the interaction boundary.

  **Where the paint sits is measured, not chosen for tidiness**: both kinds draw between the
  scrim and the glyph loop. *After* the scrim, because a patch is a hole cut **through** the
  legibility layer rather than a surface behind it — its rows carry no glyphs, so dimming them
  buys no legibility and costs the whole effect, and the visible consequence is the point (a
  patch reads brighter than the transcript around it). *Before* the glyphs and the cursor, so
  nothing a patch draws can cover a character **by construction** rather than because claimed
  rows happen to be blank. One egui layer, so call order is the entire enforcement mechanism.

  **The pointer is the one thing the terminal gives up, and only to a panel.** The console
  scrolls its transcript from anywhere in the window — there is no scrollbar to be over — so a
  panel inside the grid rect claims the pointer explicitly (`block_panel::pointer_inside` over
  `panel_placements`) or a slider drag would also scroll the block out from under the cursor.
  A **scene** patch claims nothing: it is something to look at, and the wheel over one keeps
  scrolling the page exactly as the wheel over a paragraph does. The hover test runs against
  the geometry as it stands **before** the wheel is applied, because the pointer is over what
  is on screen *now*; the draw runs after it, so a panel lands where this frame's glyphs are.
  Keyboard focus is deliberately not taken — the terminal keeps the keyboard.

  **A panel's buttons enter the command lane rather than imitating it.** `organon-console` cannot
  see `substrate_materials` and must not learn to, so a `BlockPanel` carries labels handed down
  by `console_main.rs` and reports which one was pressed; `redraw` feeds that to
  `apply_console(&ConsoleOp::Background(name))` — exactly where a typed
  `organon console background metal` lands once `drain_console` has validated it, T4 look-epoch
  record included. Clicking and typing are the same code from that call onwards.

  One ledger across both kinds (`PaneLooks::blocks: Vec<Patch>`), not a list per kind: a
  patch's index there is what the bands, the quads and the placements all mean by "which one",
  so the two paints share a z-order. `block_quads` and `placements` stay kind-blind and are
  handed the whole ledger; the filter happens after the arithmetic, which is what keeps one
  index space. Named rather than hidden: the sliders drive nothing (their values persisting
  across frames *is* the demonstration), nothing reaps an evicted patch, every panel gets the
  same controls, and `patches_want_image` is scene-only so a pane holding only panels never
  summons the engine.

  On the CLI side the verb is `ConsoleOp::Patch { up, rows, kind }`, wire form
  `patch <up> <rows> <kind>`, both counts bounded by `cli::MAX_BLOCK_ROWS`. The kind is
  `organon_core::kind::Kind` over `kind::KIND_WORDS` — one table, read by clap's
  possible-values parser, by the console's `ArgKind::Choice` schema and by `Kind::from_word`,
  so `--help` cannot offer a kind the console has no way to paint. Two asymmetries on the
  wire, both deliberate: a line with **no** kind is `scene` (what a claim meant before there
  was a choice, which keeps the verified arm working byte for byte), while an **unknown** kind
  skips the line — a newer CLI naming a kind this build cannot draw must not have a guess
  painted into a rectangle someone else's output is holding open.

  **The kind vocabulary is not this lane's, and since #48 Tier 1 it is not written three
  times.** `Kind` lives in `organon-core/src/kind.rs` — in the spine because the copies were in
  **different crates** (`cli.rs` in the root crate, `block_panel.rs` and `conversation.rs` in
  `organon-console`) and the wire copy could not import the paint ones, so core is the only crate
  all three can see; a closed set of words needs no host, GPU or UI, which is what makes it
  welcome there.

  ⚠️ **The design doc counted two copies and there were three** — `block_panel::PatchContent`,
  the paint target one layer in from the wire, is the same taxonomy again. That correction is
  what settles the shape: **one vocabulary, two payload carriers, one per placement.**
  `PatchContent` is inline-in-a-terminal, `ArtifactContent` (§1.1) is inline-in-a-conversation,
  both answer `kind()`, and each has a test that fails on an arm with no kind or a kind with no
  arm. They are not merged because their payloads are genuinely different things — a patch's
  panel owns live widget state pinned to scrollback lines, an artifact's is a description the
  view keys state off — and a `Kind` that tried to carry either would have to carry both.

  What also did *not* move is `PATCH_DEFAULT_KIND`: the "a kindless line means `scene`" rule is
  a wire-compatibility fact about this verb, so it sits in `cli.rs` beside the parser that
  needs it, and `Kind` deliberately has **no `Default`** for the other front-end to inherit.

  Three ways a word can arrive and three answers, which is the whole of what "resolved in one
  place" buys: on the **CLI** clap refuses it against the shared table before a byte is
  written; through the **command service** `Kind::resolve` refuses it with the known list
  spelled out (`` `hologram` is not a kind — known kinds: scene, panel ``), because an agent
  on that end has no other way to ask what this build can draw; and on the **sidecar** an
  unknown word skips the line in silence, because nobody is listening there and a guess would
  paint the wrong object. None of the three approximates.

  ⚠️ Unchanged by any of this: the sidecar is drained **once per frame** and is out of band
  with the PTY byte stream, so "the line you are on now" resolves at drain time. A writer that
  prints its gap and claims it in one breath is fine; one that keeps printing in between is
  not. The in-band fix is the OSC 8 claim in `doc/console_patch_protocol.md`, which resolves
  the anchor from the *cells* rather than from a clock — specified, not built.
- **The landed v2 foundations** (session/event log with torn-tail recovery, the
  typed command service, mock-agent event cards) remain in the crate, feeding
  trees C/D. **`command::CommandService` is no longer only its own tests**: #4 T2 stands
  up the product's first live instance, registering `console.background` and
  `console.rig` (`TargetKind::Viewport`, one required `name` of `ArgKind::Choice` built
  from `substrate_materials`' own tables) — joined by T5's `console.block` (one required
  `rows` of `ArgKind::Int`, the first argument on this lane that is a number rather than a
  word) — and routing every drained op through
  `dispatch`, so each one leaves a `CommandRun` record in a real `SessionLog`. Two
  shapes worth knowing: the service is built **per batch** rather than held on `Console`
  (it borrows `&mut SessionLog`, and a struct holding both would be self-referential —
  `command.rs` says as much: "the log outlives any one service"), and its target
  **banks** the validated ops for the caller to apply, because `Box<dyn CommandTarget>`
  is `'static` and cannot hold `&mut Console`. The op that gets applied is the op the
  service handed back, so dispatch is in the path, not beside it.
- **Dev flags**: `ORGANON_SHELL_CMD` (one plain-command tab, headless proof),
  `ORGANON_SHELL_TABS` (comma harness ids), `ORGANON_SHELL_DEFAULT`,
  `ORGANON_SHELL_BACKDROP`, `ORGANON_SHELL_SCRIM`, **`ORGANON_SHELL_PTY_DEBUG`**,
  **`ORGANON_SHELL_THEME`** (#38 — the one that overrides a *stored* preference, for one
  launch and out loud; §1.5 carries the amendment and its two conditions).
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

### 1.1 The conversation view — the console's SECOND front-end (Console Spike §5.9)

**Everything in §1 above assumed a character grid is the canvas. It is now one of two.**
Three measurements retired the assumption: ConPTY rewrites the byte stream (APC stripped,
a private OSC hoisted out of stream order, OSC 8's params rewritten) and a WSL tab is
`wsl.exe` under ConPTY, so there is no ConPTY-free path on this machine; console-side row
injection against a real Claude Code tab shifted the harness's whole frame and rendered
nothing; and against an *idle* shell the hole lands between the prompt and the cursor,
which is where you type.

The conclusion is not that the console owns too little. **It owns every pixel already** —
it runs the PTY, parses it and paints the grid itself. What it does not own is **the
conversation**: the grid is a lossy encoding of something that had structure before it was
flattened, and every wound above came from trying to recover that structure afterwards.

So a tab is now one of two things ([`Pane`] in `console_main.rs`):

| | What it is | Status |
|---|---|---|
| **Terminal host** | runs any program, paints its grid, patches only by cooperation | **unchanged.** It is how `htop` runs and it is the universal fallback |
| **Conversation view** | consumes an agent's structured event stream and renders it natively | **new.** No claim protocol, no anchoring, no PTY, no ConPTY |

They share the window, the tab strip, the harness registry, the console command lane and
the backdrop. Below that they share nothing, which is why `Pane` is an enum rather than a
flag: a conversation has no grid, no cursor, no scrollback and no absolute-line
coordinate, so every terminal-only path (`open_block`, `claim_patch`, the anchor pump, the
epoch boundary) goes through `Pane::term_mut()` and skips it by construction.

✏️ **Six modules, five of them harness-agnostic.**

- **`agent_event.rs` — the decoder.** NDJSON → typed events. `EventStream::push(&[u8])`
  owns its own line buffering, because a chunk boundary mid-line is the normal case (one
  `tool_result` can carry a whole file). Events carry `session_id`, an `AgentScope`
  (`Main` / `Subagent { tool_use_id }`, decoded from `parent_tool_use_id`, whose `null` is
  meaningful) and a `kind`. An unknown *event* is never an error — it decodes to
  `Unknown` with the body preserved. Tested against ✏️ **four** committed captures in
  `organon-console/fixtures/`, **two of them real** — the other two are hand-written and say
  so, which is the point of the table in that directory's README.
- **`conversation.rs` — the transcript model.** `Transcript::apply(AgentEvent) -> Change`,
  folding into ordered `Element`s
  (`Human | Assistant | Tool | RunEnd | Artifact | Approval`) with
  stable `ElementId`s. ✏️ A `Tool` element additionally carries a `SubagentLog` — the one
  thing that nests, and it nests *inside* an element rather than becoming one. Its `AgentEvent` is **its own input enum, deliberately not the decoder's**
  — two modules cannot own one type, and a transcript fluent in the wire format would
  change shape every time the wire did. No egui, no clock, no I/O.
- **`agent_map.rs` — the seam, and the only file in the tree that knows both types.** A
  second harness (Pi, §5.9.1) is written here or the model is wrong. It carries two
  read-only summaries beside the mapping: `stats() -> MapStats`, what it chose not to
  render, and `facts() -> &SessionFacts`, what the session said about itself.
- **`agent_session.rs` — one live child.** The same shape as `term.rs`: a reader thread
  that only moves bytes, a channel, a pull drained once per frame. Pipes, not a PTY.
- **`conversation_view.rs` — the drawing.** Scrollback above, then the composer, then the
  status strip along the bottom. Returns a `ConversationOutput`, which since `/panel`'s
  removal carries exactly one field: the **rects its rendered surfaces landed in** — in
  points, never pixels, which is the whole of what the console needs to size a render
  target for one.
- ✏️ **`text_diff.rs` — the alignment, and the smallest module in the set.** Two strings in,
  diff rows out. It exists because an `Edit` arrives as two whole *fields* and a card has to
  show what changed between them, and because that decision is the part that can be wrong:
  no egui, no colours, no widths, tested with plain strings the way
  `term::encode_key` is. See "The `Edit` diff is an alignment" below for its three bounds.

**The mapping's five load-bearing rules**, each from a measurement, each producing a view
that looks *nearly* right if got wrong:

1. 🚨 **An `assistant` line carries ONE content block, not a whole message.** Three
   consecutive lines in the capture share message id `msg_…0001`: prose, tool call #1,
   tool call #2. `MessageId` is unique *per rendered block* and same-id blocks replace each
   other, so passing `message_id` straight through would let the tool call overwrite the
   prose and **silently lose the assistant's text**. The key is `"{message_id}#{ordinal}"`,
   counted as blocks settle; the streamed path takes the same ordinal from
   `BlockDelta { index }`. Every block consumes one **even when nothing is rendered for
   it** (thinking, unknown kinds) or every later key shifts by one.
2. 🚨 **The human turn comes back on the stream.** `--replay-user-messages` echoes injected
   input; the composer writes to stdin and renders nothing. Ordering is then free rather
   than a splice-and-hope.
3. 🚨 **`system/init` recurs mid-stream** — a second one arrived before turn two of the
   live capture. Only the first establishes identity.
4. **`result` ends a TURN, not the stream.** Two arrived in one session. Nothing closes the
   process on it, and `result.result` is dropped rather than rendered — it is the prose the
   `assistant` lines already delivered.
5. ✏️ **Subagent-scoped events go to the tool card that spawned them.** They were dropped
   in milestone 1 and counted (`MapStats::subagent_dropped`); they are now routed as
   `AgentEvent::SubagentActivity` onto the card named by their `parent_tool_use_id`, and
   that counter is **gone rather than renamed** — its sense reversed, so keeping the name
   would have left every reader of it wrong. `subagent_routed` / `subagent_unrendered`
   replace it. See "A subagent is not a turn" below.
6. **Some lines carry facts about the session, not content for the flow** — which model,
   which cwd, what the turn cost, what the standing is. None of that is an `Element`, so
   it accumulates in `SessionFacts` and the status strip reads it from `facts()`.

**`SessionFacts` — ✏️ four retention rules now, and one list of refusals.** Each field has
exactly one rule, and the rule *is* the correctness:

| Source | Rule | Carried |
|---|---|---|
| `system/init` | **first init wins** (rule 3) | `cwd`, `cli_version`, `tools` count, `mcp_servers` as `(name, status)` |
| `system/init` (a *repeat* one) | ✏️ **latest init wins** (rule 3's amendment) | `model` (verbatim, `[1m]` suffix intact — trimming it editorialises a measurement), `permission_mode` |
| `result` | **latest wins, never summed** | `cost_usd`, `last_turn_usage`, `last_turn_duration_ms` |
| `system/post_turn_summary` | **latest wins**, all three replaced as a unit | `last_status_detail`, `needs_action`, `status_category` |
| `rate_limit_event` | **latest wins** | `rate_limit_status`, `rate_limit_type`, `rate_limit_resets_at` |

🚨 **The two `system/init` rows are the seam, and they are two rows rather than one because
the rule genuinely differs by field.** `model` and `permission_mode` are the two things a
live control can change, and a changed model is restated *nowhere but a repeat init* — so
first-init-wins there would pin the plate to a model the session had stopped using.
Everything else in a repeat init stays standing, because `tools` and `mcp_servers` grow as
deferred MCP tools finish loading rather than because anything changed. **An empty string
in a later init is absence, not a change**, so an init reporting no model leaves the
standing one alone rather than blanking the plate. Split across
`SessionFacts::record_init` and `::record_repeat_init`; the argument is under "The two
plates became controls" below.

📌 **`permission_mode` has a second source and `model` has none**, which is the asymmetry
that section is built around: `set_permission_mode` also emits a dedicated
`{"type":"system","subtype":"status","permissionMode":…}` line, taken latest-wins as well,
so the mode plate is right whichever of the two arrives first. Either alone would cover
the measured order; taking both costs nothing and removes an ordering assumption.

🚨 **`total_cost_usd` is already cumulative across the session while its sibling `usage`
is per turn.** Adding two results counts the whole session so far twice — on the live
capture, turn two's `0.0202` is exactly turn one's `0.0101` plus its own. The field names
say which is which (`cost_usd` vs `last_turn_usage`) so a reader cannot mistake one for
the other. Replacing the summary fields as a unit is the same kind of care: a later turn
that needs nothing must *clear* an earlier turn's demand, or the strip says "waiting on
you" about a question already answered.

⚠️ **What is deliberately absent, because the stream does not honestly carry it:** a
context-window percentage (the denominator lives only in the unmodelled `modelUsage`
block and the numerator lives nowhere), a quota percentage (`rate_limit_event` has a
status and a reset time, no numbers), and any session token total (only cost accumulates
on the wire; summing per-turn usage double-counts every cache read). `num_turns` is
declined for a third reason — it counts *that run's* turns and does not accumulate, so it
read `1` on both results of the two-turn capture; the view counts `Transcript::turns()`,
which it measured itself.

⚠️ Reading a fact off a `Notice` or a `RateLimit` does **not** make it mapped. Neither
renders anything into the flow, so both stay counted in `MapStats::unmapped` exactly as
before — pinned by a test, because silently changing what a counter means is its own
class of bug.

✏️ **`tool_use_result` was on that list and never belonged to `unmapped` at all.** It is a
sibling of `message` on a `user` line that always mapped to a `ToolResult`, so counting it
there would have said a line was unrendered while its tool card was drawn in full. It is now
attached to the card that line resolves, with `MapStats::tool_details` /
`tool_details_declined` as its own two numbers — see "`tool_use_result` — the sibling object
a terminal never sees" above.

#### ✏️ A subagent is not a turn — it is something a tool call is doing

**The problem it closes.** A coordinator session that dispatches agents showed a `Task`
card sitting on "running" for eight to sixteen minutes and then a wall of text. The events
were arriving the whole time; rule 5 was dropping them, because rendered as ordinary events
they become assistant turns belonging to nobody.

They are folded onto the **card that spawned them** instead, correlated by the
`parent_tool_use_id` the decoder already exposes as `AgentScope::Subagent`. One event —
`AgentEvent::SubagentActivity { parent, activity }` — carries the three shapes that mean
anything inside a card (`Subagent::Said` / `Used` / `Returned`), and `ToolCard` gains a
`SubagentLog`. It is the only transcript event that **addresses an existing element rather
than appending one**, which is precisely what stops a subagent acquiring a place in the
flow of its own.

🚨 **There is no live text here, and nothing in the view may imply there is.** §5.9.1
measured that Claude Code **never forwards token-level deltas from a subagent**. So a step
is always a *completed* burst, the gaps between bursts are real and can be minutes long,
and `Subagent::Said` carries a whole string with no completeness bit because there is no
provisional state for it to be in. What the card honestly shows is that an agent is
running, which tool spawned it, and what it did — never a live feed.
`MapStats::subagent_stream_events` is the **canary on that measurement**: it should be zero
forever, and if it is not, the path needs redesigning rather than patching.

⚠️ **Depth is flattened to one and recorded, not nested.** A subagent can dispatch its own,
and cards inside cards inside a scrollback have no bottom. `Transcript::subagent_owner`
maps a nested call id to the top-level card that owns it, so a chain of any length collapses
onto the one card a human can see, each step carrying the `depth` it happened at.
🚨 **`MAX_TRACKED_DEPTH` caps the reported number, never the attribution** — the first
implementation made *ownership* conditional on it, and past the cutoff steps fell through to
the orphan path and opened **new top-level cards**, which is the nesting hazard again in a
flat disguise. What bounds the map is eviction: entries are swept when their card leaves the
window.

| Case | What happens | Why |
|---|---|---|
| Parent card scrolled away | Nothing special — the log is *inside* the element, so it scrolls with it | There is no floating overlay to keep alive |
| Parent card **evicted** by the cap | Later activity opens a fresh nameless orphan card | The subagent has not stopped working just because we stopped retaining its card |
| `parent_tool_use_id` naming a call we never saw | Nameless card, `Running`, counted `orphan_subagent_activity` | Exactly `orphan_results`' precedent — the content is real, so it is kept rather than dropped |
| A `Returned` with no matching `Used` | Kept as a nameless step, counted `unmatched_subagent_returns` | The same keep-it-anyway rule, one level in |
| A subagent that never stops | Log capped per card by `Limits::max_subagent_steps`, evicting the **front** | One `Task` must not be able to evict the conversation around it by working hard |

⚠️ An orphan card opens **`Running` and joins the running set**, which is a claim. It is
behaviour 1's derivation rather than an invention — live activity from inside a call is the
strongest available evidence the call has not returned — and opening it `Complete` would
fabricate the result behaviour 3 refuses to fabricate.

⚠️ A step landing on a card already in the flow reports `Change::Updated`, never `Appended`.
The view re-arms its scroll-follow on the latter, so reporting it wrongly would yank a
reader to the bottom every time any subagent spoke.

📌 **Two things a nested step deliberately does not carry.** A `Returned` keeps only
`is_error`, not the tool's output: this is the one place the model declines content rather
than truncating it, because the alternative is a tool's full output nested two frames deep,
multiplied by every tool every subagent runs. And a subagent's own `result` / `system` lines
are not folded into `SessionFacts` — a subagent's cost is not the turn's.

✏️ **The fixture is a real capture now** (2026-08-13, `claude.exe` 2.1.228, this argv, two
agents in parallel and one of them dispatching its own). It replaced a reconstruction, and
it confirmed the correlation, the zero subagent `stream_event`s, and — the question the old
split left open — that a subagent emits **no `result` and no `system` line of its own**
either. `fixtures/README.md` carries all four corrections; three bear on this section:

- 🚨 **The dispatch tool is named `Agent` on the wire, while `system`/`init` advertises
  `Task`.** Both spellings sit in the one capture. Nothing here routes on the name, which
  is the only reason a fixture saying `Task` never failed — but a view that special-cased
  it would have matched nothing, and this document said "`Task` card" throughout on the
  strength of a guess.
- 🚨 **The depth-2 case measured above does not occur.** A nested dispatch appears only as
  a `tool_use` and a `tool_result` **scoped to its parent**, so it lands as an ordinary
  depth-1 step; the grandchild's own lines are never forwarded, and its `tool_use.id` is
  never once a `parent_tool_use_id`. The flattening machinery is kept — nothing promises
  the CLI will keep withholding those lines, and the hazard it closes is real the day they
  arrive — but it is `conversation.rs`'s synthetic tests that cover it, with their
  provenance declared, and no capture proves it.
- ⚠️ **`Subagent::Said` is now backed by nothing observed.** Every subagent-scoped
  `assistant` line in the capture carried a `tool_use` block and nothing else; the answer
  reached the console only as the parent's `tool_result`. The card fills with *steps*, and
  a design that assumed prose would arrive was assuming.

📌 **And the capture opened a door the design did not know was there — rule 5b, now
walked through.** Five `system` subtypes nobody had seen — `task_started`,
`task_progress`, `task_updated`, `task_notification`, `task_summary` — carry live subagent
progress: a rolling `description`, `last_tool_name`, `usage.{tool_uses,total_tokens,
duration_ms}` and a terminal `status`. They are **main**-scoped (no `parent_tool_use_id`
key at all), so rule 5 cannot see them, and reaching them took a second correlation of
their own. This does not weaken "there is no live text": these are not tokens. What it
means is that the honest liveness a card can show is larger than "a burst arrived" — a
`Task` card now says *"Reading one.txt · → Read · 1 tool · 10.3s"* while the agent works,
where it used to say nothing at all for eight to sixteen minutes.

🚨 **The correlation is `task_id`, not `tool_use_id`, and the difference is not a
detail.** Measured line by line across the capture:

| subtype | `task_id` | `tool_use_id` | what it adds |
|---|---|---|---|
| `task_started` | yes | yes | the dispatch's title, its prompt |
| `task_progress` | yes | yes | the live activity, `last_tool_name`, the counts |
| `task_notification` | yes | yes | the terminal status, the final counts |
| `task_updated` | yes | **no** | a `patch` — `status`, `end_time`, and nothing else |
| `task_summary` | **no** | **no** | a nullable `detail`, belonging to no card |

So the mapper learns `task_id → tool_use_id` from any line that states both and resolves a
`task_updated` through it; a `task_summary` is a gloss of the *session* and stays unmapped.
Keying on `tool_use_id` alone — the obvious reading of the first capture — would have
dropped every status transition, silently, on a path where silence is the failure mode.

⚠️ **A card holds ONE progress value, so a nested task's is declined rather than merged.**
The `task_*` family reaches **depth 2**, unlike every other subagent line on this wire: a
grandchild's lifecycle really is forwarded, naming a call that is only a step in its
grandparent's log. Merging it would have made that card narrate somebody else's work in its
own voice, mid-flight. Counted as `Stats::nested_subagent_progress`, because a number that
reads non-zero on a *healthy* nesting fan-out must not be mistaken for a fault — and
because it is the measure of the next increment. ⚠️ **The nested task sends four `task_*`
lines here and that counter reads 3**: its `task_started` arrives one line *before* the
`tool_use` block that creates its card, so the call is not yet known to be nested and that
one is counted by `orphan_subagent_progress`. **3 here + 1 there**, each half pinned.

⚠️ **One source per fact.** An `Agent` `tool_use_result` carries its own `totalTokens` /
`totalDurationMs` / `totalToolUseCount`. Durations and tool counts match the `task_*`
figures exactly; **the token totals do not** — 62 949 against 62 951, and 63 564 against
63 803, because the result is struck later and counts output the notification had not seen.
Both are honest. Taking both would make one card's token count jump at completion with
nothing to explain it, so only the `task_*` stream is read: it is the one that exists while
the card is otherwise silent, which is the whole reason the row exists.

**The process contract (§5.9.2, measured):** `-p --input-format stream-json
--output-format stream-json --include-partial-messages --replay-user-messages --verbose`
keeps **one session alive across many turns** — one `session_id`, a `result` per turn.
Spawn once per tab and never let the process go. Resume is the recovery path, not the
interaction model. There is **no attach**: every programmatic surface is a child process
you spawn, so a conversation tab cannot mirror a Claude Code session running elsewhere —
it must *be* the session.

#### The two bands under the scrollback — the composer, and the strip beneath it

`draw` is one `bottom_up` column, and **the order of its calls is the visual order upside
down**: `status_strip` is added first and therefore sits lowest, the composer above it, a
rule, and the scrollback takes whatever is left. The status used to be added *between* the
composer and the rule, which put it **above** the composer — where a one-line band with a
rule under it reads as a divider rather than as a readout. It belongs with the composer at
the bottom, which is where Claude Desktop puts the model affordance and where a hand
looking for it goes.

Three things in these two bands are not the obvious spelling, and each cost real time.

🚨 **egui's modifier matching is SHIFT-PERMISSIVE, so the obvious composer eats
Shift+Enter.** `Modifiers::matches_logically` returns true for a press *with* shift when
the pattern does not ask for shift. `TextEdit`'s default `return_key` therefore cannot tell
Enter from Shift+Enter, and `consume_key(Modifiers::NONE, Key::Enter)` swallows both. The
way out is an **inversion**: **Shift+Enter is declared as the `return_key`**, because a
pattern that *asks* for shift is the one case the match is strict about. The widget then
takes Shift+Enter and inserts the line break itself, a bare Enter falls through it
untouched, and the view reads that Enter out of `ui.input` with
`Modifiers::matches_exact(NONE)` — read, never consumed, since consuming would take
Shift+Enter with it. Ctrl+Enter and Alt+Enter are deliberately **neither** send nor
newline: each is "send" in some chat client and "newline" in another, and a wrong guess
sends a half-written message. None of this is visible in a green build, which is why the
contract is pinned by driving real key events through a real frame
(`enter_submits_and_shift_enter_types_a_newline`) and not only by testing the predicate —
the same reason `native/tests/egui_popup_contract.rs` exists after a 0.31→0.33 change of
exactly this kind killed a keypress here once.

⚠️ **`Response::lost_focus()` stopped being a valid submit trigger the moment the box
became multiline.** The only `surrender_focus` on Enter is in `TextEdit`'s *singleline*
branch, so the old `lost_focus() && key_pressed(Enter)` idiom would simply never fire
again — silently, with everything still compiling. The guard is `has_focus()`, and keeping
focus across a send costs nothing because nothing takes it away.

🚨 **A vertical `ScrollArea` cannot be dropped straight into a `bottom_up` column.** It
places itself at `available_rect_before_wrap().min` — the *top* of the space that is left —
while the bottom-up cursor sits at the bottom, so allocating it eats everything between:
**measured at 684 pt of a 684 pt pane, for one row of text, with `max_height` set to 100**.
`ui.vertical`, `ui.scope` and an enclosing `Frame` all inherit the failure; `ui.horizontal`
places correctly and then pins the area to one row. Both bands therefore **reserve their
height first** with `allocate_ui_with_layout`, which does go through the placer, and lay
out top-down inside the reservation. For the strip that height is **the taller of the two
faces the band actually draws** — `max(Body, Monospace)` — plus `STRIP_CHROME`, *derived*
from both plates' padding and strokes rather than rounded off, so a reserved band cannot
disagree with its own chrome. Both faces are in play and neither is decorative: the model
name and the standing are `Monospace`, the chips and the trailing log are `Body`.
⚠️ On this build the mono row is the taller — **18.125 against 17.96875** — so reserving
`Body` alone left the band a hair *under* what it held, and had been right only by accident
of which face happened to be taller, an accident that started to matter the moment the dim
half stopped being `.small()`. The max is a computation that matches what is drawn rather
than one that happens to fit. ✅ Measured against the layout test's busiest content —
permission marker, unconfirmed model change, and an eight-times-repeated overlong log line —
the band measures **36.125** against the **35.96875** the `Body`-only reservation gave it,
inside the same `44.0` one-line bound, which did not have to move.
For the composer it is the text's own height, which is why
`ConversationPane::composer_height` is carried state: it is fed from
`ScrollAreaOutput::content_size`, the **unclipped** content size and deliberately not
`Response::rect`, so the measurement cannot feed back on the band that clips it. Growth
lands one frame late — the same trade egui's own panels make.

📌 The correction generalises past this band: **a reserved height has to be computed from
the text style that is actually drawn in it.** Two styles in one reservation means taking
the max of them, or the number is right only until someone changes a font call — and it
goes wrong as a second row, in the one place that has to stay one line.

**The composer** opens at a three-row floor whether or not anything is in it (a one-line
field is what makes an input read as an afterthought), grows a row at a time to a
twelve-row ceiling and then scrolls inside itself, and sits on a framed plate whose edge
says which state it is in: grey at rest, green while focused, red-brown when the agent is
gone. The keystroke contract lives in the hint text rather than in a permanent caption,
because a caption is a row of chrome paid for on every frame and stops being news after the
first message.

**The strip** is decided by one pure function, `strip_content`, so the interesting part —
the priority ordering — is testable without spawning an agent to find out what a band says:

| Rank | Standing | Reads |
|---|---|---|
| 1 | `Dead` | the failure. Outranks everything: no other reading describes a process that exists |
| 2 | `Asking` | `◈ N permission requests — waiting on you`. The agent is *halted* on a human |
| 3 | `Working` | `● N tools running` |
| 4 | `Generating` | `● generating` — an assistant message is open and tokens are arriving |
| 5 | `Asking` | `needs_action` verbatim — the agent's own sentence about what it wants |
| 6 | `Connecting` | nothing, deliberately: the model plate already says "no model yet" |
| 7 | `Ready` | `last_status_detail`, else a bare `ready` |

⚠️ **Ranks 3 and 4 above rank 5 are the one place "waiting outranks working" is deliberately
not applied**, and the reason is that `needs_action` describes a turn that has *ended*: the
mapper only clears it when the next `post_turn_summary` arrives, so a demand the human
already answered stays set for the whole of the turn that answers it. Showing it over live
work would be a stale "waiting on you" — the failure the unit-replacement rule above exists
to avoid. Tokens arriving now are live activity by exactly the measure a running tool is, so
the rule has to cover both readings or it does not hold.

**Rank 3 above rank 4 is not a claim about urgency.** The two are true *together* for most
of a turn — a tool block opens inside the message that called it, so the bracket is still
open while the call runs — so the ordering is deciding which sentence is worth the one line
there is. `● 3 tools running` names what is happening and can be checked against the cards
above it; `● generating` only says that *something* is. The specific reading wins, and the
general one is what the band falls back to for exactly the stretch of a turn that used to
read as idle.

##### Rank 4, and the signal it is keyed off

The band could say "3 tools running" and could not say the agent was **thinking**.
`Transcript::is_working()` is derived from unresolved tool ids alone, so a model writing
prose with nothing in flight was *idle* by that test — during the stretch of a turn a person
is most likely to be watching. `EventMapper::is_generating()` closes that with a **second**
signal rather than by loosening the first, so `N tools running` still means exactly N tool
calls.

🚨 **It is the `message_start` … `message_stop` bracket, and NOT `system`/`status` =
`"requesting"`.** That choice is measured, not preferred. On the committed capture
`native/organon-console/fixtures/claude_stream_two_tools.jsonl`, which makes **two** API round
trips, `"requesting"` appears **once** — line 4, ahead of the first message; the second
`message_start` (line 27) has no status line before it. And nothing anywhere reports the
request coming *back*: there is no `"responding"`, no closing status, no counterpart of any
kind (the file's only other statuses are `connected`, `pending` and `allowed`, none of them
about a round trip). A state keyed off `"requesting"` would therefore be shown for a
session's first request, be silently absent for every one after with nothing to tell those
two cases apart, and need a clearing rule invented for it besides. The bracket has neither
problem: it is emitted once per message, and it closes itself. `"requesting"` stays
read-for-facts and rendered as nothing — a refusal, pinned by
`a_requesting_status_alone_changes_nothing`.

⚠️ **`EventMapper::streaming_message` is not that bracket, and was deliberately not
repurposed into it.** It is set on `MessageStart` and **never cleared** — `MessageStop` was a
no-op arm before this change — so `.is_some()` means "a `message_start` was seen at some
point", not "a message is open". Keeping it that way is the point: that id is what a late
text delta keys against, and clearing it at the stop would trade a stuck status for a **lost
sentence**. So `generating` is a second `bool` beside it, and the distinction is pinned by
`closing_a_message_does_not_detach_a_late_delta_from_it`.

**It lives beside `SessionFacts` rather than on it, and the reason is the retention rule
rather than the source.** Every field there is a value the session *reported*, held until a
later line replaces it; this one flips on and flips off, and a stale one would be the band
claiming the agent is still writing after it stopped. It reaches the strip through
`LiveCounts`, whose every field goes back to zero or `false` on its own.

**Which makes the clearing paths the whole of the correctness.** Four events clear it, and
the fifth exit has no event at all:

| Cleared by | Because |
|---|---|
| `message_stop` | the ordinary close |
| `result` | a turn that fails part-way — `error_during_execution`, an interrupt — ends there and never reaches a stop |
| a mid-stream `system/init` | an init means the open message is never going to close. Cleared **before** rule 3's repeat guard, deliberately: dropping the line for the flow must not also drop the only notice that the stream restarted |
| a second `message_start` | the flag is **assigned**, not counted, so two opens without a stop are one open message and there is no tally that could fail to reach zero |
| — **the process dying mid-message** | nothing clears it, because there is no event to clear on and no stream to carry one. `Standing::Dead` outranking every other reading is what answers this, which makes rank 1 and rank 4 one dependency rather than two features |

🚨 **The honest wart: between two messages of one turn the band falls through to rank 7 for a
frame or two**, and reads "ready" while the turn is plainly still going. The bracket really
has closed and the next one has not opened. Holding it across the gap would mean inventing a
turn-open state the wire does not report — and that is the version that gets stuck on when a
turn ends in a way nobody predicted. The flicker is the price of the band never lying.

**And it says nothing beyond the fact that it is happening.** No token count, no rate, no
progress bar, no ETA, no elapsed timer. The wire says a message is open; it does not say how
much is left, how fast it is arriving, or when it will stop, so each of those would have to
be invented to be shown — and there is no clock in this path at all, by design
(`conversation.rs`'s module doc owns that).
`an_open_message_reports_generating_and_nothing_more_than_that` asserts the text carries none
of `%`, `/s`, `tok`, `eta`, `left`, `of`.

📌 `Generating` shares `Working`'s amber (`RUNNING`) rather than taking a colour of its own:
busy-with-tools and busy-writing are the same answer to "can I walk away", and the split the
band's small colour budget has to protect is busy versus *blocked on you*. The text already
spells out which kind of busy it is.

Beside the standing: a **model plate**, which is the headline affordance and the first
thing a hand looks for — the reported identifier with any trailing bracketed suffix
relocated into a badge (`claude-opus-5[1m]` → `claude-opus-5` · `1M`) and **nothing else
changed**. It is not prettified to "Opus 5": the field is whatever the CLI reported, and a
table of nice names would silently mangle the first identifier not on it (an alias, a
snapshot date, a gateway's fully-qualified id), which is a strip lying about which model
you are talking to. Everything else the session said about itself — the verbatim model
string, permission mode, CLI version, cwd, tool count, MCP roster, rate limit, session id —
is on that plate's **hover**, because it is identity rather than status and a strip that
grows a second row has stopped being a strip. ✏️ **The permission mode has since been
promoted out of the hover onto a plate of its own** — it stayed on the hover as well,
because it is still identity, but a setting that can silence the console's approvals is
not something a hand should have to discover by hovering. Both plates are now **controls**;
the next subsection is what that cost. Then, dim and right-aligned, one to three
chips (session cost, remembered decisions, last turn's wall time) and the most recent
diagnostic line off the child, truncated rather than wrapped. ✏️ **And at the very end
of the row, a context ring** — the one readout on this band that is not text; the
subsection after next is why it took a different numerator to become honest.

✏️ **One to three, not zero to three: the dim half is now present from the first frame.**
The session cost is on the band from the moment the tab opens, reading `$0.0000`, and the
ring's track is drawn beside it — see "the ring's track is chrome" below for the honesty
argument, which is the interesting half of this change. `last turn` is the one element with
no truthful cold-start form and it is **omitted** until there is a last turn: nought spent
is a total and a bare ring is a container, but `last turn 0.0s` is a duration asserted about
an event that did not happen. `remembered decisions` stays conditional on separate grounds —
it is a tally of things the reader themself did, so its arrival is not something that happens
*to* the band. ⚠️ Neither omission moves the band's **height**, which is the property
`the_cold_band_reports_a_cost_and_a_ring_and_does_not_grow` pins directly.

✏️ **Dim, not small.** The chips, the trailing log and the standing have all dropped
`.small()`: the chips and the log sit at `Body`, the standing at `Monospace` — the model
name's own face and size, since it keeps `.monospace()` for the tofu reason two subsections
down. Colour is what makes that half secondary, and size was doing a second job it was never
needed for, at the cost of the only items on the band with numbers in them — a session spend
read across a desk was legibly smaller than the model name opposite it. Raising the standing
alongside them was the judgement call: left small it would have been the one shrunken item
between the plates and a now-full-size dim half, which reads as a mistake rather than as a
hierarchy. ⚠️ The trade is horizontal, not vertical — the band is still one line, and larger
text simply means fewer characters of the log before the ellipsis.

📌 **What stayed small stayed small for a reason.** The mode plate's text, the model's
variant badge, the pending `→` arrow and the cold-start placeholders all sit *inside*
bordered plates, where a smaller face reads as deliberate subordination rather than as a
mismatch. The mode marker stayed small on separate grounds: it is a whole sentence rather
than a word, and it is the one item that would eat serious horizontal budget from the log's
slack.

⚠️ **Two omissions are the view's own judgement, on top of `SessionFacts`' refusals above.**
The running-tools reading says **tools, not "thinking"** — it is derived from unresolved
tool calls only, so a model writing prose with nothing in flight is not working by that
test and a "thinking" label would be false exactly when it was most reassuring. ✏️ **The
hole that left is now closed by rank 4**, and closed the way this entry implies it had to
be: with a second measured signal (`is_generating`, the `message_start` … `message_stop`
bracket) rather than by widening this one, so both readings stay exactly as literal as they
were. And the
cost chip is labelled **`session`** while per-turn tokens are not shown at all: `cost_usd`
accumulates on the wire and `last_turn_usage` does not, so one band carrying both would
invite a reader to add them up.

##### The context ring — the readout that was declined, and the numerator that unblocked it

✏️ At the far right of the band, past the chips, sits a small ring that fills with blue as
the conversation grows and turns amber when it is three-quarters gone. **`SessionFacts`
refused this readout once**, and the entry is worth reading before this one because half of
the refusal was right and is still enforced.

The refusal read: *"the denominator appears only inside the unmodelled `modelUsage` block,
per model, and the numerator would have to be a running conversation size nothing on the
wire reports."* The first clause was a gap rather than a wall — `modelUsage.contextWindow`
was never unavailable, only undecoded, and it is now `ModelUsage::context_window` (measured
**1 000 000** for `claude-opus-5[1m]`, alongside `canonicalModel`, `maxOutputTokens` and a
per-model `costUSD`). The second clause was a mistake about what the ring had to measure.

🚨 **The obvious numerator is wrong, and it is wrong in the direction that hides.** A
`result`'s `usage` is summed across the turn's several API round trips — the `iterations`
array beside it is the proof there is more than one — so it is a *turn total* wearing the
shape of a prompt. Measured on `claude_stream_two_tools.jsonl`, whose one turn makes two
requests: the requests carry prompts of **52 556** and **54 050** tokens, and the `result`
reports `4 + 28 766 + 77 836 = 106 606`, exactly their sum and **1.97×** the conversation
that was actually in front of the model. A ring built on it would have sat at 11 % where the
truth was 5 %, filled at roughly twice the real rate, and looked entirely plausible doing it.
`a_results_usage_is_the_sum_of_the_turns_requests_not_a_prompt` and
`the_context_numerator_is_the_last_request_not_the_turns_total` pin both numbers so the
ratio is visible in the failure message.

✅ **The honest numerator is `message_start.usage`, which is a prompt size per request** and
was already decoded and thrown away by the mapper. So the ring measures **context at the
last request** — `Usage::prompt_tokens()` of the most recent `message_start` over the
context window of the model that served it. Both halves *measured*; nothing derived, nothing
projected, nothing summed. ⚠️ `prompt_tokens()` is the three **input** counts and excludes
`output_tokens`, which on a `message_start` is the placeholder `1` the module doc records —
a "total tokens" spelling would have been wrong twice, adding a completion to a prompt and
lying about the completion.

**"At the last request" is the provenance marker, not a turn of phrase.** The ring moves
**per API round trip**, not per turn: a turn making three requests steps it three times, and
a compaction that shrinks the prompt shrinks the ring at the next request. That is why
nothing accumulates — `last_prompt_tokens` is assigned, never added — and why the hover says
which request it describes and names both wire fields it came from.

⚠️ **Pairing the two halves is the remaining correctness**, and it needs two spellings: the
`modelUsage` block is keyed `claude-opus-5[1m]` while the `message_start` that names the
model says `claude-opus-5`, which is that entry's own `canonicalModel`. `context_window_for`
matches on either, and with two or more entries and no match it returns `None` rather than
picking one — the one inference it allows itself is a **sole** entry, because a turn whose
whole block names one model used one model. ⚠️ The `Vec` order is `serde_json::Map`'s
key order, which is sorted rather than as-written, so "the first entry" means nothing and
nothing reads it that way.

🚨 **"We do not know yet" is a real state. The ring's TRACK is chrome and its FILL is the
measurement, so the track is drawn throughout and the arc is not.** ✏️ This reverses an
earlier decision recorded here, and the reversal is stated rather than quietly applied.

The rule used to be that `ContextSlot::Unknown` draws **nothing at all**, on the grounds
that before the first `result` there is no window, before the first `message_start` there is
no prompt, and a ring drawn empty in either case reads as *0 % full* — a specific,
confident, false number. That reasoning was right about the **arc** and wrong about the
**circle**. A ring with no arc in it is not a needle pointing at nought; it is the container
the answer will appear in, the same way an unlit gauge face is not a reading of zero. What
outweighed the original call is a cost it never priced: the whole dim half — cost, ring,
chips — materialised at the first turn's `result`, so a band a hand had been looking at for
a minute **rearranged itself** the moment the session became interesting. James asked for
stable chrome, and the decomposition gives it up nothing: the arc still refuses.

🚨 **The remaining hazard is real rather than hypothetical, and it is handled in the track's
colour.** A `message_start` reporting a zero prompt against a known window builds a `Known`
fill whose `fraction()` is `0.0` and which therefore sweeps **no arc either** — so an
unmeasured ring and a measured nought would draw one identical picture, which is precisely
the false claim the original rule existed to prevent. `ring_track_color` gives the two
different circles: `CONTEXT_TRACK_EMPTY` (visibly fainter, sat about midway between the
band's fill and the measured track) when nothing has been read, `CONTEXT_TRACK` when
something has. And because a shade is not an answer to "which is this?",
`ring_hover_rows` carries the same distinction in words — *"context: not measured yet ·
waiting on: a window from `result`, a prompt from `message_start`"* against the measured
ring's *"0% at the last request"*. `an_unmeasured_ring_is_distinguishable_from_a_measured_nought`
pins both halves, including that the empty track is the fainter of the two and is still
visible against `STRIP_FILL`.

`ModelSlot::Connecting` says "no model yet" rather than vanishing for the reason it always
did: that plate is the headline affordance and a hole where it sits reads as broken. The
ring now belongs to the same family — present, and silent about what it has not measured.
So a session's first turn has no ring **fill** and the arc arrives at that turn's `result`;
`the_band_carries_no_ring_fill_until_both_halves_are_measured` is the renamed and narrowed
survivor of the test that used to assert the whole ring was absent. ⚠️ A session
run **without `--include-partial-messages`** never gets one at all: it has a window and no
`message_start` ever, which is exactly the shape that tempts a fallback to `result.usage`,
and `a_window_without_a_prompt_size_is_no_context_reading_at_all` asserts the unused
`last_turn_usage` is sitting right there and stays unused. The console always passes that
flag; the `live_session` fixture was captured without it, which is what makes it the
regression.

📌 **Blue, because the ring is not a standing.** Every other colour on this band is a state
the agent is in — `RUNNING` busy, `ASKING` blocked, `BAD` gone — and a resource gauge that
is true continuously must not look like one of them. The amber above the threshold is
`MODE_ALERT`'s exact value, reused rather than re-chosen: it already means "worth acting on,
not a failure" here.

⚠️ **The 75 % threshold is a display decision and says so.** Nothing on the wire states when
the CLI will compact a conversation, so any number here is the console's judgement about how
much runway a reader needs, not a measurement, and it must not borrow the authority of the
two counts around it. Seventy-five because the cheap answers — a fresh tab, a summary,
letting a long tool result go — each cost a turn or two, and a turn is not small against this
window: the capture's two requests grew the conversation ~1 500 tokens in one round trip and
had already spent 5 % of a million on the first. A warning at 90 % leaves a handful of round
trips; a quarter of the window leaves room to finish the thought. Pinned at exactly 75 by
`the_ring_turns_amber_at_three_quarters_and_not_before`, with integer arithmetic so the
boundary cannot drift with the window size.

⚠️ **The colour is a statement about the printed figure, so it is computed from it.**
`ContextSlot::is_high` reads `ContextFill::percent` rather than comparing the two counts
again, and the reading **floors** rather than rounds. Both halves came out of one defect:
with a rounding `percent()` and a second, independent threshold comparison, `7 495 / 10 000`
printed "75 % at the last request" under a ring that was still blue — `74.95` rounded up to
the threshold while the comparison saw `749 500 < 750 000`. Rounding was the deeper of the
two, because a fill gauge that rounds *overstates*: it claims a threshold the conversation
has not reached, which is the same species of error as the `result.usage` numerator this
readout exists to avoid, at a tenth the scale. **Never report a fill you have not reached.**
`the_ring_cannot_contradict_the_percentage_it_prints` holds the review's case, and the
amber test now asserts the colour against the number parsed back out of the hover — it
previously checked `is_high()` alone, which is why a ring disagreeing with its own hover
was not something it could have caught.

⚠️ **The diameter is exactly one `TextStyle::Body` row**, which is the only reason the ring
is free. The band reserves `row + STRIP_CHROME` *before* laying anything out, so the ring is
the one child of that horizontal layout that could have been taller than the reservation and
quietly made the strip two lines. `the_strip_is_one_band_and_leaves_the_scrollback_the_rest`
now builds its busiest band with a 91 %-full ring in it and still asserts the same bound and
the same "identical height with everything in it as with nothing" — **no assertion was
loosened and the band did not grow.** ✏️ That last clause carries more weight since the ring
became unconditional: the ring is now a child of the horizontal layout on **every** frame
rather than only on measured ones, so the cold band is no longer the trivially-empty case it
was when that assertion was written. Both bounds are unchanged and both still hold, and
`the_cold_band_reports_a_cost_and_a_ring_and_does_not_grow` states the same equality as its
*primary* claim rather than as a corollary — deliberately twice, because it is the property
the change exists to deliver. Drawn as a stroked **arc**, not a pie: a filled wedge
past 180° is not convex and egui's `convex_polygon` tessellation folds it over, so the naive
version would have drawn wrongly exactly as the reading became urgent.

**Still declined, and for the reasons the original entry gave.** A quota percentage —
`rate_limit_event` carries a status and a reset time, no numerator and no denominator
anywhere. A session token total — only `total_cost_usd` accumulates on the wire, and summing
per-turn usage double-counts every cache read. And a cumulative context fill, which this ring
is emphatically not.

✏️ **And one thing withdrawn: the band no longer counts the models.** Every `initialize` ack
used to write a note — *"the session offers 5 models"* — which landed on the band's single
line of diagnostic width. It is a number nobody can act on, and the list it counts is one
click away on the model plate, which is where a list of models belongs. **The list itself is
untouched**: it is what `model_rows` builds the picker from, and
`the_picker_is_built_from_the_list_the_cli_offered` is unchanged.
`the_band_says_nothing_about_how_many_models_were_offered` is the other half of that
contract. ⚠️ The note's absence is not directly unit-testable — `receive_control` needs a
live session to resolve a `request_id` — so that test guards the reachable half: no chip,
reading or identity row mentions the list.

##### The two plates became controls — and what a control had to prove first

✏️ The model plate and a new permission-mode plate beside it are now **clickable**. The
band stopped only reporting the two facts it exists for and started changing them, on the
live session, with no respawn. `doc/console_session_control_protocol.md` is the
measurement — every wire shape in it captured against `claude.exe` 2.1.228 — and this
section is what building against it cost, which is a different list from what it promised.

**The wire is the pipe the console already owns.** A `control_request` line goes down the
same stdin turns go down; a `control_response` comes back on the same stdout events come
back on. `set_model` acked in **272 ms**, `set_permission_mode` in **17 ms**, and no
handshake was needed first. `agent_session.rs::control_request_line` builds the line
through `serde_json` rather than by formatting a string, and its bytes are pinned
**byte-equivalent against the sentences the protocol doc quotes from the capture** — a
typo in a subtype comes back as `Unsupported control request subtype: …`, which a user
experiences as "the picker does nothing".

🚨 **Correlation is the entire hazard, and it is why `ControlDesk` is a separate type.** A
response carries a `request_id` the console invented **and nothing else** saying which
verb it answers — `set_model`'s ack has no body at all. So exactly one place in the
console knows that an id means "the model change", and it is testable without a process:
an id issued, an ack matched, an ack belonging to nobody, a request never answered.
📌 The other end of that seam is `agent_map.rs` recording **no fact** from an ack, on
purpose: the mapper never issued the request and cannot know what a `request_id` answers,
and two writers for one field where one of them is guessing is worse than one clean
source.

⚠️ **Nothing is ever gated on an ack. `CONTROL_DEADLINE` is 20 s and it releases a marker
rather than unblocking a wait.** The composer, the transcript and the strip never wait on
a control; the deadline exists so an unanswered request cannot leave a plate marked
"switching" forever. It is a **sweep on the pane's existing per-frame pump** — no timer,
no thread, no queue — and a request is recorded in flight *before* the write, because a
write that failed half way and one that reached the CLI are indistinguishable from this
side and the safe reading is that it might have. **Twenty seconds is set by the slowest
request, not the fastest:** the two acks above are sub-second, but `initialize` goes out
at spawn, where §6 measured a **1.3–3.3 s** band to a session's first announcement while
MCP servers and skills warm up. Twenty is ~6× the top of that.

**The model list is asked for once, at spawn, and is the only source there is.**
`Control::Initialize`'s answer carries a per-account `models` array with display names
written for humans, so no model table exists anywhere in this crate to go stale — an empty
list draws a picker that says the list has not arrived, which is the honest rendering of
"this session did not answer". 📌 Asking at spawn has a side effect worth recording on its
own: `system/init` was measured to arrive **only once input is pending**, so a tab nobody
had typed into never announced itself and the strip sat on "no model yet". The
`initialize` line *is* input — so the plate now learns its model at spawn rather than at
the first human turn.

⚠️ **Two rows of that list can both be current, and that is not a bug**: `default` and
`opus[1m]` both resolve to `claude-opus-5[1m]` in the capture, so current-ness is matched
on `resolvedModel` **and** `value` — which is what the CLI's own schema says
`resolvedModel` is for.

🚨 **The pending plate, and the reasoning is the interesting part.** `set_model`'s ack
carries **no body**, and the new model is stated only by the *repeat* `system/init` that
follows. So between the click and that init the console knows what it **asked for** and
not what it **got**. Asserting the new name would be the plate lying about the one fact it
exists to report; asserting nothing would make the click look dead. So the plate keeps the
**confirmed** model and draws the destination beside it as a dim italic `→ Sonnet`. ⚠️ It
clears when **the reported model moves at all** — deliberately *not* when it equals a
predicted string: `set_model` takes an alias, the session reports a resolved id, and the
resolution table is the CLI's, so predicting it would strand the marker on every alias
this build has not met. And selecting the row already in use is a **no-op**, because
`set_model` to the current model produces an ack and no repeat init — the marker would
have nothing to clear it and would sit there until the deadline.

**That plate only works because rule 3 was amended, and the amendment is James's.** A
repeat `system/init` is the *only* place a changed model is ever restated, and rule 3 as
written kept the first init forever — so the model would have genuinely changed while the
plate said `claude-opus-5[1m]` until the tab closed. **`model` and `permission_mode` are
now latest-init-wins; `cwd`, `cli_version`, `tools` and `mcp_servers` stay
first-init-wins.** ⚠️ Taking the whole later init would be the wrong repair and that is
measured too: between the same two inits `tools` went 33 → 128 and `mcp` 0 → 4 with
nothing asked to change, because deferred MCP tools had finished loading. **An init is a
restatement, not a change notification.** The ruling is written into
`doc/console_spike_execution_plan.md` §5.9.3 rule 3; the test that pinned the old
behaviour was **re-scoped and renamed**
(`a_second_init_does_not_overwrite_the_sessions_identity`) rather than deleted, because
what it was protecting is still true of every field the amendment did not name.

🚨 **A model switch also emits a user-role message that would have rendered as a turn the
human never typed.** The CLI narrates itself as
`<local-command-stdout>Set model to sonnet (claude-sonnet-5)</local-command-stdout>`,
and it arrives *before* the ack, so waiting cannot suppress it. It is now withheld — and
the predicate is narrow on purpose, because **swallowing a real sentence is far worse than
showing a spurious one**: exactly one text element, `strip_prefix` **and** `strip_suffix`
rather than `contains` (so a human may quote the tag inside a larger message), and
⚠️ deliberately **not** keyed on `isReplay`, which is `true` on genuine human turns too —
replay is how a human turn reaches the transcript at all — so requiring it would exclude
nothing real while letting a future unflagged narration through. The one residual false
positive is a human whose *entire* message is a verbatim wrapper pair, and
`MapStats::local_commands_suppressed` counts every suppression **separately from
`unmapped`** precisely so that number climbing while somebody is typing is how that would
be caught. `control_responses` is counted apart from `unmapped` for the same reason: "the
CLI answered us" and "we drew nothing for a stream event" are different facts.

**The permission-mode control is designed around the mode that can silence the console.**
Put a session in `dontAsk` and 🚨 **it is not a bypass that lets things through** —
prompts never reach the console's handler and gated tools come back **refused**
(`decision_reason_type: "mode"`), while the console still passes
`--permission-prompt-tool`, still holds the handler and still *looks* like the authority.
The user's experience is "the agent suddenly cannot do anything and nobody asked me why."
Three consequences, on James's brief that *"we need to make what it does unmistakable for
the don't ask policy"*:

- **Exactly three rows, each labelled by what happens rather than by the mode's name.**
  "dontAsk" tells a reader nothing; *"no approval cards at all — anything needing
  permission is refused, and the console is never asked"* tells them everything. The
  consequence is the **label, not a tooltip** — a hover puts the one sentence that matters
  behind a gesture nobody makes while deciding.
- **Three omissions, each for its own reason.** `bypassPermissions` is refused outright by
  a session the console did not launch with `--dangerously-skip-permissions`, so the row
  would be a dead button; `plan` and `auto` were never measured against the console's
  handler, and the control governing authority is the wrong place to guess.
- ⚠️ **The marker is persistent, not a confirmation.** Whenever the reported mode is not
  `default`, it sits on the band for as long as that stays true. A dialog clicked through
  at the moment of choosing is exactly the warning people stop reading, and the hazard is
  not that moment — it is the hours afterwards. It is **derived in `strip_content`** from
  the reported mode every frame, so it cannot get stuck on, cannot get stuck off, and
  cannot be dismissed. 📌 Amber, deliberately **not** `BAD`'s red: this band is looked at
  for hours, and a permanent klaxon is one the eye learns to skip — which would leave the
  console exactly where it started.

⚠️ **A mode arriving from outside the picker is still reported and still marked.** A
session spawned with `--permission-mode`, or one an unrecognised future mode reaches, gets
a marker too — the rule the band keeps is *"say so whenever the console may not be the one
being asked"*, and an unknown mode is precisely the case that cannot be ruled out. **The
shortlist governs what can be chosen, never what can be shown.**

📌 The mode plate needed none of the pending machinery, and the asymmetry is the wire's
rather than a design choice: `set_permission_mode`'s ack states its own result **and** the
CLI emits a dedicated `system/status` line carrying the new mode. The mode has two clean
sources; the model has only the repeat init. Implementing the two as though they were
symmetric is the trap the protocol doc names.

##### The tofu fix — four glyphs egui's fonts do not have

James saw two empty boxes on screen where a rule should have been, and the same class of
defect turned out to be at three sites: **egui's proportional face carries no box-drawing
or block-element glyphs**, and a missing glyph draws as tofu rather than failing. The
fixes are deliberately **not** the same fix, because the right answer depends on
what the glyph sits in:

| Site | Was | Now | Why not the other fix |
|---|---|---|---|
| the run-end rule | `──` (U+2500 ×2) | `—` (U+2014) | a rule leading into small dim **proportional** text does not want to become monospace, and an em dash is the mark a typesetter would have reached for anyway |
| the streaming caret | `▍` (U+258D) | `\|` | it is concatenated into the agent's prose, which is proportional on purpose — so the glyph changes rather than the face |
| the strip's `◈` / `●` | unchanged | `.monospace()` at the **draw** site | the strings stay exactly as they were, so every existing test still pins them |
| ✏️ a subagent step marker | `✓` / `✗` (U+2713 / U+2717) | `•` / `×` (U+2022 / U+00D7) | it was **already** `.monospace()` and drew a box anyway — Hack has no dingbats, so the face was never the problem |

📌 The precedent already existed — the approval card's `◈ may I` was monospace — and had
simply not been generalised. `the_bands_symbols_are_the_ones_the_mono_face_has_to_draw`
asserts that no character in `U+2500..=U+259F` may appear in a band reading, which
pins the strings; the `.monospace()` in `strip_box` is the other half, and only a person
can confirm that half.

🚨 ✏️ **The fourth row is why the rule is two rules, and the guard is now an allowlist.**
James's fan-out capture showed `□ Bash` inside an `Agent` card — a returned subagent step,
drawn at a site that did not exist when the three fixes above landed, and drawn
`.monospace()` from the day it was written. Measured by reading the `cmap` tables of all
four fonts egui 0.33 bundles — `Hack-Regular`, `Ubuntu-Light`, `NotoEmoji-Regular`,
`emoji-icon-font`: **U+2713 `✓` and U+2717 `✗` are in none of them.** egui does no OS font
fallback, so asking for a family only chooses *which* font is missing the glyph. The same
read confirms the third row was right about its own case (`◈` U+25C8 and `●` U+25CF are in
Hack and not in Ubuntu-Light) and that `→` U+2192 is Hack-only, which is why the
`.monospace()` at that draw site stays. So: **choose a character Hack has, then ask for
Hack.** The two replacements are in *both* faces, so they cannot regress if a later edit
drops the font call.

⚠️ **The old guard could not have caught this, and that is the more valuable half of the
fix.** It forbade one range (`U+2500..=U+259F`) at one site (a band reading); `✓` is in
neither the range nor the site. `no_symbol_the_console_draws_is_a_glyph_egui_lacks`
replaces the blocklist with an **allowlist** of every non-ASCII character the console is
measured to be able to draw, and applies it to the band readings, the chips *and* the
subagent step markers (`step_mark`, extracted from the draw site precisely so a test can
ask it). Adding a symbol anywhere those reach now means adding it to that list, which means
measuring it first. The band-only test is kept alongside for its own narrower claim.

#### The scrollback's own elements — a tool call as a card, and a control panel in the flow

**The inline artifact, and why it is the milestone.** A terminal receives a tool call as
whatever text the harness chose to print. The event stream carries it structured — name,
the complete input object, a correlation id, a later result — so `conversation_view` draws
a **card**: the tool's name, its arguments as fields, an accent that says running (amber)
/ ok (green) / error (red), and the output clipped with a count of what was clipped.
`Edit` goes one further and renders its `old_string`/`new_string` as a real diff, because
those arrive as *fields* rather than as a patch someone has to parse back out of prose.
"A tool is running" has no event anywhere in the stream; it is derived from an unresolved
id, and it stops being true when the result arrives.

##### ✏️ The `Edit` diff is an alignment, and its three bounds are three different failures

The first rendering printed `old_string`'s lines as removals and `new_string`'s as additions
with nothing between them, so **a one-character change inside a ten-line block came out as
ten removals followed by ten additions** — honest about what arrived, and useless to read.
`text_diff::line_diff` trims the common prefix and suffix, aligns what is left by longest
common subsequence, and elides long unchanged runs to `CONTEXT = 3`. Measured: one changed
character in a ten-line block is one removal and one addition, and the same change 200 lines
into a 400-line block costs the same rows — **a diff's size is the size of the change, not of
the block it sits in.**

📌 **No diff crate.** This crate's `Cargo.toml` header requires every dependency edge to earn
its line, and after the trim the changed region is small enough that a plain LCS is the whole
algorithm. `MAX_CELLS` is sized against what one alignment costs rather than against "how
large an edit could be".

⚠️ **The alignment used to be recomputed every frame, and is not any more** — see
§"An `Edit` card's diff is computed once, not once per frame" below for what that cost and
what replaced it. The paragraph that used to stand here said the repetition was what
`MAX_CELLS` was sized against; that was a defensible claim about *one* card and it never
survived being multiplied by a session.

| Bound | The failure it answers | What it leaves on screen |
|---|---|---|
| `MAX_CELLS` (20 000 DP cells, ~141 × 141 lines) | the alignment costing more per frame than the card is worth | `not aligned — N lines against M is past the diff budget`, and a block replacement |
| `MAX_RUN` (8) | one hunk filling the card | `… N more lines` inside the run |
| `MAX_ROWS` (24) | *many* hunks filling the card, which no per-hunk bound catches | `… N more lines` at the end |

🚨 **`MAX_RUN` is not redundant with `MAX_ROWS`, and dropping it is a silent regression rather
than a smaller diff.** A global row cap truncates the tail, and in a block replacement every
removal precedes every addition — so a global cap alone shows a wall of red and **no green at
all**, which is worse than the unaligned rendering it replaced. Capping each same-kind run
first is what keeps both sides of every change on screen.

⚠️ **Whitespace-only and no-change edits are named rather than drawn.** An identical pair
renders **no rows** and says `no change — old_string and new_string are identical`; printing
the block as removals *and* additions is the loudest possible way to say nothing happened. A
re-indent, a stripped trailing space or a changed line ending is named `whitespace only — no
visible character differs`, because its rows are *visibly identical* and a reader with no note
reads the card as broken. 🚨 The predicate is computed **on the whole strings, not per row**,
which is also what catches a **trailing-newline** difference: `str::lines` cannot see one, so
there is no row for it, and a per-row test would have had the card claim the two were
identical when they differ by a byte.

##### ✏️ An `Edit` card's diff is computed once, not once per frame

`tool_card` used to call `edit_diff` from inside its own body, so **every frame, for every
`Edit` card in the transcript**, `serde_json::from_str` walked the whole arguments blob and
`line_diff` re-ran the alignment — and threw both away. Three facts made that linear in the
session rather than warm-in-cache: the scrollback is **not virtualised** (`ScrollArea::show`
lays out every element, and `egui::Label` builds its galley *before* the visibility check, so
a card two thousand lines off screen paid in full), `Limits::max_elements` is 10 000, and the
result is a pure function of arguments that do not change once a card has settled.

Measured — `doc/console_edit_diff_cost.md`, instrument at
`conversation_view/edit_diff_bench.rs`, two instruments agreeing within a few percent:

| One `Edit` card | per call | 400 cards, per frame |
|---|---:|---:|
| an ordinary one-line edit | 1.5 µs | 0.12 ms — below the noise |
| a function-sized hunk | 5.6 µs | 0.52 ms |
| the largest `MAX_CELLS` allows | 43.9 µs | 3.9 ms |
| a 400-line common prefix | 78.2 µs | 6.2 ms |
| *a stated one large edit in ten* | — | **2.4 ms**, 15 % of a 60 Hz budget |

🚨 **The common case was never the problem, and that is the finding.** Had sessions been only
ordinary edits the honest answer would have been to leave it alone. The tail is what justified
a field: a session of large edits cost **61 ms per frame — 16 fps sitting still** — and after
the cache the mixed corpus is indistinguishable from a `Read`-only control.

`ConversationPane::diffs: HashMap<ElementId, Option<EditDiff>>` holds it, in the idiom the
pane already had for `artifacts`: computed in `scrollback`'s walk, *read* by `tool_card`
(which now takes the diff rather than deriving it), pruned against the transcript by a
`retain` beside the artifacts one. `Body::Tool` moved out of `draw_element` into `scrollback`'s
match for the reason `Body::Artifact` is already there — it needs state that survives between
frames. **`edit_diff` itself is unchanged and still uncached**; the cache is at the call site
so the pure function stays pure, and a test fails if anyone memoises it as well.

🚨 **Invalidation is by eviction on `Change::Updated`, and it had to be, because
`Arguments::complete` is not a promise of immutability.** A second `ToolCall` for an id that is
not yet *resolved* replaces the arguments text wholesale, so a cache keyed on "complete" would
have shown the first arguments' diff forever under a card displaying the second arguments'
path — silently, and only on a card the harness happened to re-emit. A fingerprint cheap
enough to take every frame must be shorter than the text and can collide; hashing 58 KB per
card per frame costs a large fraction of a 78 µs saving. The fold already names the element it
changed, so the exact answer is also the cheap one. It lives in `ConversationPane::absorb`,
which exists as a method rather than four lines inside the drain loop **so that the rule is
reachable by a test** — the drain loop needs a live agent process.

⚠️ **Every update evicts, not only an argument one** — a `ToolResult` drops a still-good diff.
One recomputation per card per result, accepted: narrowing it would mean the pane reasoning
about which *field* the fold touched, which is the fold's knowledge and would rot the day a new
event arm is added.

⚠️ **This does not make the transcript cheap.** `doc/console_rewrap_measurement.md` §6 stands
unchanged: layout is still O(scrollback) in every condition, and at 2 000 elements that alone
is half a frame. This removed a *second*, independent O(scrollback) cost sitting on top of it.

##### ✏️ `tool_use_result` — the sibling object a terminal never sees

The decoder always kept it (`UserTurn::tool_use_result`, `Value` verbatim because its shape
varies per tool) and nothing rendered it. It is now `conversation::ResultDetail` on the tool
card, mapped by `agent_map::result_detail`, and a `Read` card reads `4 of 900 lines, from line
40`.

🚨 **Four fields, and the list stops at what a real capture contains.**
`claude_stream_two_tools.jsonl` — a capture, not a reconstruction — carries
`{"type":"text","file":{"filePath","content","numLines","startLine","totalLines"}}`, twice,
both for `Read`. A byte count, an exit status, a truncation flag, the unified patch Pi's
`Edit` result carries: **absent because nothing has been observed sending them**, not
forgotten. This repo labels what it shows measured or derived, and a field with no capture
behind it could be labelled neither.

🚨 **A second shape is now captured, it is not readable, and the counter is how that was
found.** `claude_stream_subagent.jsonl` — the real fan-out that replaced a hand-written
reconstruction — carries two `tool_use_result` objects of a third kind: `Agent` results,
`{"status","prompt","agentId","agentType","content","usage",…}` with **no `file` sub-object
and no `type` key at all**. `result_detail` reads the `file` object whatever the line claims
to be, which was recorded above as a bet on shape-stability rather than a measurement, with
`MapStats::tool_details_declined` named as the canary on it. Swapping the fixture made the
bet's first real test: the counter went 0 → 2 and a test failed loudly, rather than a card
quietly showing numbers no tool sent. ⚠️ **`result_detail` is deliberately not widened.**
What an `Agent` card should display — a token total, a duration, a nested-agent id — is a
card-design question that no observation answers yet, and inventing one here would be the
exact move this section's four-field list exists to refuse. An `Agent` result renders no
detail today, and says so in a number.

| Decision | Why |
|---|---|
| **`content` is dropped** | it is the file's text, which the `tool_result` block already carries in numbered form. The same file twice in one card |
| **Field-detected, not `type`-dispatched** | `"type":"text"` is checked nowhere. The value is undocumented and its `type` vocabulary unknown past that one word, so matching on it would render nothing for a readable `file` object under a name we have not seen. The decoder's own feature-detect rule |
| **`numLines` is passed through, off-by-one and all** | it read **4** for a three-line file (the numbered result text ends `4\t`). That is the tool's arithmetic; a card that "corrected" it would report something no tool said |
| **The path is shown only when the arguments do not already state it** | a `Read` card prints `file_path` as an argument field. ⚠️ The case this preserves is the **orphan** card — no call means no arguments, and then the detail's path is the only record of what the tool touched |
| **Counts need both halves** | `4 lines` alone says nothing a person wants; `4 of 4` and `4 of 900` are different facts. A half-reported count renders nothing rather than being completed by a guess |

🚨 **A detail on a line carrying two `tool_result` blocks is declined, never attached to
both.** `tool_use_result` is a sibling of `message`, not of a block inside it, so nothing says
which call it describes. Every capture has exactly one result per line; two is unobserved, and
guessing would put one call's line counts on another's card. `MapStats::tool_details` and
`tool_details_declined` count the two outcomes — ⚠️ **and are separate from `unmapped`, which
never counted this in the first place** despite `agent_map`'s module doc having said so: the
`user` line a `tool_use_result` rides on always mapped to a `ToolResult`, so `unmapped` would
have claimed a line was unrendered while its card was drawn in full.

📌 A subagent's nested step **declines the detail**, on `Subagent::Returned`'s own argument: a
step says it finished and whether it failed, and a file's line counts are exactly the
per-result content that argument turns down.

**A live control panel, inline — the artifact the other front-end needed a protocol for.**
`Body::Artifact` is an element the console puts in the flow itself, and
`conversation_view::panel_element` draws it as a real egui panel: sliders that move,
buttons that act, `block_panel`'s own colours and spacing imported rather than re-chosen.
Set that against what the terminal host needs for the same rectangle — the writer printing
its own gap, a claim, absolute-line anchoring, reflow invalidation, and surviving ConPTY —
and the size of the difference is the argument for the second front-end. **There is no
character grid, so an artifact is an element in a list that draws itself.** No anchoring
exists to get wrong.

Three rules hold it up, and each names a failure it prevents:

| Rule | Where | The failure it prevents |
|---|---|---|
| The element is a **description** — a title, slider *names*, button *names*, and no value, colour, rect or closure | `conversation.rs` | the model acquiring layout, and stopping being a state machine you can test in milliseconds |
| **Live widget values live in the view**, in a `HashMap<ElementId, PanelState>` beside the transcript, pruned each frame against `Transcript::get` | `conversation_view.rs` | a slider that snaps back mid-drag, because the transcript is folded from a stream and its elements mutate as events arrive. This is what stable ids are *for* |
| Button labels are **handed down** and come back by label | `console_main.rs` | `organon-console` learning about `substrate_materials`; a pressed button re-enters `apply_console`, the same call `organon console background <name>` reaches |

#### The rendered surface — a control and its consequence in one glance

Beat 7 checked the panel on screen and the check produced the finding: **its effect appeared
on a different tab from the one it was clicked in.** A conversation has no scrollback for a
backdrop to band across, so `/panel`'s buttons changed the console's backdrop and the only
place that shows is the terminal next door. A control whose consequence you cannot see from
where you are sitting is a bad instrument, and no amount of wiring fixes it.

`ArtifactContent::Surface` is the answer. **`/surface` summons two elements**: a rendered
surface, and directly beneath it a panel whose `PanelSpec::drives` names that surface's
`ElementId`.

⚠️ **`ArtifactContent`'s arms are one per shared kind, and its two spellings are not the
terminal lane's.** #48 Tier 1 made `organon_core::kind::Kind` the single vocabulary both
front-ends resolve from; `ArtifactContent::kind()` answers it, and
`every_shared_kind_has_exactly_one_artifact_arm` fails if an arm appears with no kind or a
kind appears with no arm. What it deliberately does **not** enforce is spelling:
`ArtifactContent::Surface` answers `Kind::Scene`. Both words are user-facing and neither could
move in an inert tier — `scene` is in `--help`, in the `organon-cli` skill and on the sidecar
wire, and `/surface` is what a human types in this composer — so the tier unified the *set* of
kinds and left the two names alone. Unifying them costs a documented break of one of the two
plus the skill and the protocol doc that quote it; that is a decision for whoever adds the
third kind, and it is recorded here so they decide rather than rediscover.

**The payload asymmetry is not a wart either.** A patch names a kind and can carry nothing
more — `doc/console_patch_protocol.md`'s whole point is that a program which can print must
not be able to drive the machine — while an artifact is summoned from inside the console and
so carries a `PanelSpec` or a `SurfaceSpec`. The kind is the half that is genuinely shared;
the description is the half that genuinely differs, and forcing either shape onto the other
would mean giving the text lane a payload or throwing away what the view draws from. The buttons and knobs then change the picture a few rows up, in the same view,
while the hand is on them — and a driving panel's button is **consumed by its surface** and
never reaches `apply_console`, so it cannot also repaint a backdrop somewhere else.

| Question | Answer | Where |
|---|---|---|
| Where does the rect come from? | **egui layout** — `allocate_exact_size`, full column width by `SURFACE_HEIGHT` (260 pt). The terminal host derives a patch's rectangle from absolute lines, a scroll anchor, a cell height and a reflow rule; the conversation view has one call. That is the simplification the second front-end buys | `conversation_view::surface_element` |
| How is it rendered? | The **one** `World`, into a target the conversation owns — `render_to_texture` at `BACKDROP_FORMAT`, the substrate rig re-framed for that rect's aspect. `Console::render_source`'s seam exactly: what the engine draws is not what the backdrop paints, so the window behind stays flat and James's "opens like an ordinary terminal" rule is untouched | `Console::render_surfaces` |
| How is it sized? | `scene_input::pane_pixels_in(swapchain, rect_points, window_points)` — the rect's **fraction of the window** applied to the swapchain, so `pixels_per_point` cancels. The view hands points across the crate seam and never a scale, which is the arrangement that function's doc exists to protect | `console_main.rs` |
| How does a look reach the engine? | `surface_shared` = `look_shared(Substrate, look)` with the knobs applied last, published through the same `Shared` channel the backdrop uses, then the console's own snapshot is put back so `organon status` never reports a surface's lane | `console_main.rs` |
| Which knobs? | `light` (key azimuth, swept 360° centred on the console's own), `elevation` (`lighting[3]`, 0–90°), `exposure` (`pbr[2]`, ±3 EV). Chosen to be **orthogonal to the material buttons** — `apply_material` owns `pbr[0..2]` and `lighting[7]`, so a knob on those would appear dead the moment a button was pressed | `apply_surface_slider` |

**The cap, stated in numbers because a silently dropped texture reads as "the picture is
still there".** `MAX_SURFACE_TEXTURES = 4` live textures across every tab, evicted
least-recently-**requested** first, each eviction printing one `[surface]` line naming the
element, the pane, the size and why. Only surfaces that **overlap the viewport** are
requested at all (`conversation_view::surface_visible`), so a transcript's render list is
bounded by the screen rather than by its length. `SURFACE_RENDERS_PER_FRAME = 1` bounds the
engine work: a surface whose look has not changed is not redrawn, so an idle conversation
costs zero engine frames and a dragged slider repaints at full rate. Eviction happens
*before* allocation, `substrate_epochs`' rule, so the peak is the cap. Worst case at the
size this console actually draws one (2475×585 px) is ≈23 MB — `surface_budget_bytes`, and
a test quotes the figure so the prose cannot drift from it.

⚠️ **Rendering the World twice in one frame is a real hazard, and it is bounded rather than
hidden.** The beat clock, the camera and every sim in `frame_body` advance on a **wall-clock
`dt`**, so a second render microseconds after the first advances them by microseconds —
invisible. What double-steps is what counts *frames*: `frame_index`, which drives the TAA
jitter phase, and the temporal history beside it, both shared between the two targets. On the
still lit plane a surface draws, that is not visible. On a moving World it would be, and
intermittently — which is why the surface look is the substrate, why the budget is one, and
why this paragraph exists instead of a silence.

⚠️ **A `SurfaceKey` is `(pane index, ElementId)`.** An `ElementId` is unique only within one
transcript, so two conversation tabs both start at 0. Closing a tab renumbers the panes, and
`Console::apply`'s `Close` arm therefore frees **every** surface texture — one wasted re-render
against a class of bug where one conversation paints into another's rectangle.

**Summoning is deliberately a separate seam.** `/surface` typed in the composer is recognised
by the command registry (§1.8) as its `view.surface` entry, acted on locally and **never
written to stdin**; `Transcript::insert_artifact` is a method rather than a ninth `AgentEvent`,
because no harness said this and putting it in the event enum would oblige every mapping to
carry an event none of them can produce. That is what makes the next step small: the agent
summons a surface with a tool call, the integrator answers it with the same `insert_artifact`,
and the registry entry is deleted without touching anything that draws.

✏️ **The recogniser it used to live in has become something much larger, and §1.8 owns why.**
`conversation_view::local_command` matched the single string `/surface` and forwarded
everything else. It was described here as a temporary seam — it still is, for *this* verb —
but the mechanism it needed is the one the console needed for a different reason entirely: a
human's console command was costing an agent round trip and an approval card. The console's
whole verb vocabulary is now typeable in this composer, generated from the same table the MCP
tools are generated from.

🚨 **`/panel` is gone, and its machinery with it.** It summoned a panel wired to the
console's *backdrop* — which a conversation has no scrollback to band across, so the effect
landed on a terminal tab and the panel you had just clicked appeared to do nothing. Driving
one, James's reading was "the controls don't do anything… it's redundant", and it was.
`PanelSpec::drives` is therefore an `ElementId` rather than an `Option<ElementId>`: **a panel
cannot be built that does not name a target in its own transcript**, so the failure is not a
policy the view has to keep enforcing. `ConversationOutput::actions` and `ArtifactAction`
went with it — the only producer was the console-driving arm — and so did `console_main`'s
loop that turned such a press into `ConsoleOp::Background`. `/panel` is now an ordinary
message and reaches the agent like any other sentence.

#### The approval card — the console answers "may I?"

In James's first real session **three tools bounced on permission** and rendered as red
error cards, because nothing answered approvals. The fix is one measured flag:
`--permission-prompt-tool` names an MCP tool the client consults **for every tool the agent
calls, `Bash` included** — so a single card answers for everything, not only for the
console's own verbs. `doc/console_approval_protocol.md` is the spec; every wire shape in it
was measured against `claude.exe` 2.1.228 rather than read from documentation.

**The console serves that tool itself, over loopback HTTP, inside its own process.** That is
the architectural fork and the reason the transport is `http` rather than `stdio`: a stdio
server is a *separate process* with no access to the UI, so every approval would cross a
process boundary and come back. Over HTTP the client connects **out** to us and the
permission hook is a direct call into the state the UI is already drawing.

| Piece | What it is | Where |
|---|---|---|
| The protocol | `McpServer` — messages in, messages out. No connection, no thread, no process | `mcp.rs` |
| The transport | An accept thread, a connection thread each, one `Mutex` around the endpoint. `POST /mcp`; a **permission call is answered as `text/event-stream`** so it can be written to while it waits; the optional `GET` push stream is still **`405`**, measured fine | `mcp_http.rs` |
| The hook | `ApprovalGate` — posts the question to the UI over a channel and waits on the reply in `HEARTBEAT` steps | `approval.rs` |
| The element | `Body::Approval(ApprovalBlock)` — tool, arguments as text, `tool_use_id`, pending / answered / **abandoned** | `conversation.rs` |
| The card | allow · allow & remember · **allow everything this session** · deny, with the arguments shown as fields | `conversation_view.rs` |
| The memory | `DecisionMemory` — per-call entries keyed on tool **plus canonicalised arguments**, and one session-wide allow | `approval.rs` |
| The capability tools | every `CommandSpec` in `mcp_specs()` (= `console_specs()` + the camera read), schema generated from the spec; dispatch is `ConsoleDispatch` | `mcp.rs` · `console_main.rs` |
| The exposure audit | `ExposureAudit` — §7's withholding property, re-checked at every `system/init` from the reported tool list | `mcp.rs` |

🚨 **The serve loop must not be the UI thread, and that is the whole shape.** The hook is
synchronous and blocks for as long as a human takes. The pending question therefore holds
the endpoint mutex, so a second concurrent MCP request waits behind the card; the agent
asking it could not proceed either way.

##### 🚨 There is a deadline, and holding it open is the design

**This section used to say "a card with no timeout". That was false, and a human found it.**
An approval card sat waiting while Claude Code returned
`<tool_use_error>Error calling tool (Write): The operation timed out.</tool_use_error>` — the
write failed, and the card was left still asking a question whose answer could no longer
matter.

Measured 2026-08-12 against `claude.exe` 2.1.228, with a standalone probe server that
deliberately never answered:

| What | Measured |
|---|---|
| The client's patience on a `--permission-prompt-tool` call | **60.010 s** and **60.005 s** from the `tools/call` arriving to the socket being aborted (`WSAECONNRESET`), twice in one run |
| With `notifications/progress` against the request's own `progressToken`, every 5 s | answered at **90 s**, no abort |
| …every 10 s | answered at **300.1 s** after 29 beats, the write went through, the model reported success |

So **progress notifications reset the clock**, and the fix is to send them. `mcp.rs` states
the two numbers: `CLIENT_DEADLINE_SECS = 60` and `HEARTBEAT = 10 s`, a sixth of it. The
margin is the point — a beat is written from the thread that is already blocked on the
human, so it competes with nothing, but five consecutive beats would have to be lost before
the client gave up.

**Why the transport changed shape.** `notifications/progress` is a *server-initiated*
message, and over this transport a server-initiated message related to a request rides that
request's own response stream. The console `405`s the optional `GET` push stream, so there
is no other channel: a permission `tools/call` is answered with `Content-Type:
text/event-stream`, beats go out as events while the hook blocks, and the JSON-RPC answer is
the last event before the terminating zero-length chunk. Everything else stays plain
request/response, exactly as measured. `McpServer::handle_with` carries a `Heartbeat` down
to `PermissionResponder::decide`; `McpServer::handle` and every test keep the old signature
via `NoHeartbeat`.

🚨 **A question the agent stopped waiting for stops asking.** The other half, and the one
that was visible on screen: the beat doubles as a liveness check, so a closed socket ends the
wait. `ApprovalGate` then marks the `PendingApproval` **abandoned** and returns
`deny(ABANDONED)` — fail closed, always; the console never allows on a timeout. The pane
sweeps that flag every frame into `Transcript::abandon_approval`, and the card becomes a
third state: dimmed, no buttons, *"the agent stopped waiting — this call failed before it was
answered."* The same sweep runs when the agent process ends, which is the other way a
question dies. ⚠️ A permission call carrying **no** `progressToken` is still streamed: the
deadline cannot be held open without one, but noticing the client leave needs only an open
connection.

**Remembering is ours.** There is no upstream persistence — three identical calls produced
three separate requests, and no response field caches anything. So the console keeps its
own memory, and three properties are deliberate: it keys on the **whole call** (`Bash` with
*this* command, never `Bash`), a remembered decision **still renders a card** saying so, and
that card carries a `forget` button. An authority granted once and thereafter invisible is
worse than being asked every time. **Scope is the session** — the memory lives in the pane
and dies with the tab; nothing is written to disk.

##### Allow everything for the rest of this session

The same memory widened, not a second mechanism: `DecisionMemory` holds one extra flag, the
card offers a fourth button, and `recall` answers from it when no per-call decision covers
the call. The handler still runs, the card is still drawn, the transcript still records
every call — the console simply answers *yes* on the human's behalf.

🚨 **It is not, and must never be presented as, a permission mode.** `bypassPermissions` is
unreachable (the CLI refuses it without a launch flag the console does not pass) and
`dontAsk` **refuses** rather than allows — both measured, both in the protocol doc. Nothing
in this feature touches either.

Four properties, each a decision:

- **The band carries a standing marker for exactly as long as it is on**, derived in
  `strip_content` from the memory's own flag — the same rule as the mode marker, which
  exists because the hazard is not the click, it is the hours afterwards when the band still
  looks like the authority. It cannot stick and cannot be dismissed. [`MODE_ALERT`]'s amber,
  not red: this band is read for hours and a permanent klaxon trains the eye past it.
- **It says which of the two facts it is.** *"you allowed everything — the console is not
  asking"* is a different fact from *"you are not being asked — anything needing permission
  is refused"*, with a different remedy, and both can be true at once. The band shows both,
  as two plates; a merged warning would name the wrong cure half the time.
- **Revoked from the band, by clicking that marker** — there is no one card the grant
  belongs to, since it covers calls that have not happened yet. No confirmation: the click
  revokes an authority rather than granting one, and friction belongs on the other side.
- ⚠️ **A per-call decision outranks it.** A call denied-and-remembered stays denied under a
  standing allow; the wide grant is the default for calls nobody has decided, never an
  overrule of a specific refusal. `revoke_session_allow` leaves the entries alone, and
  `clear` takes both.

The card records **who** answered — `AnsweredBy::{Click, ThisCall, SessionAllow}` — because
the two standing sources are undone in different places, and a card that could only say "from
a decision you already made" would send a reader looking for a `forget` button that is not
there. The grant is also **not** counted in the "N remembered decisions" chip: it has no key,
no card and no entry, and folding it in would hide the wider grant inside the narrower count.

⚠️ **`updatedInput` is mandatory on an allow** and is re-validated against the called tool's
schema. `resolve_choice` echoes the input back, which is the degenerate case of a capability
worth keeping: the field is where a card could offer *"allow, but not that path"*. It is not
built, and it is deliberately not designed out.

⚠️ **Two traps that make a working feature look dead.** `echo` never prompts — safe
read-only `Bash` is auto-approved by a built-in classifier that never consults the handler.
Neither do writes inside the session's own scratchpad, because the model picks a pre-blessed
path. **Only an explicit absolute path outside it triggers a prompt**, which is what a manual
test has to aim at.

🚨 **Never serve a second approval-shaped tool.** Claude Code removes the handler from the
model's own tool set *because* `--permission-prompt-tool` names it — so the model cannot
hand itself `{"behavior":"allow"}`. Verified live against this server when it served nothing
else: `system/init` reported `[{"name":"organon","status":"connected"}]` with **zero** of its
36 model-visible tools mentioning `organon`. Any other approval-ish tool would be an ordinary
model-callable one with no such protection.

##### The console's own verbs, served as capability tools

**The server now serves both halves the protocol doc always said it would.** `McpServer`
generated capability tools from the same `CommandSpec` table the CLI is generated from —
`ToolEntry::from_spec`, `input_schema`, `set_tools`, argument checking against that same
spec — and the console constructed it with an **empty** table anyway. So it answered
permissions for everything the agent did and exposed nothing, and an agent that wanted the
portal open ran `./organon.exe console portal open` through `Bash`: a second process, to
talk to the one it was already inside, raising a card that asked *"may I run this shell
command"*. Nobody had joined the two ends.

`ConversationPane::new` now takes a `Capabilities { specs, dispatch }`, handed down by
`console_main` for the reason the button and slider tables are: the vocabulary is built from
the substrate's material and rig tables, which the compositor crate cannot see, and applying
a verb needs the `Console` that owns the backdrop. `Capabilities::none()` is the caller with
nothing to offer, and `NoDispatch` stays as its honest dispatch.

⚠️ **`ConsoleDispatch` writes onto the console's own command channel rather than applying
anything, and that is the design.** `Console::drain_console` already reads that file every
frame, routes each line through the real `CommandService` — validating against the same
`CommandSpec` the tool's schema was generated from, and leaving a `CommandRun` record either
way — and applies it. A second apply path beside the audited one is exactly what §5.9.25's
"one vocabulary, many renderings, never a hand-written second copy" forbids. The CLI and the
tool converge on one transport; what the tool removes is the process, not the discipline.

⚠️ **The tool therefore returns `{"accepted": "portal open"}`, not "applied".** The op lands
on the next frame (~16 ms), and a failure *after* validation — a name this build cannot paint
— reaches stderr via the drain rather than the model. That is the honest cost of reusing the
audited path; the alternative was blocking an MCP call on the UI thread's next frame.
⚠️ Verbs are **window**-scoped exactly as the CLI's are: `background` and `rig` dress the
window, `block` and `patch` land on the active pane, whichever tab called the tool.

🚨 **One verb on this lane is a *read*, and it is the one thing a conversation tab has that
the CLI does not.** `console.camera.read` answers in-process from `Console`'s published
viewpoint instead of writing a line nobody can collect an answer to — see §1.3, "Reading it
back". So the served table is `mcp_specs()` = `console_specs()` **+ 1**, and the difference is
a fact about transports rather than drift; a test pins that the extra verb is exactly that one,
because any *other* extra would be served, called, and refused by `op_from` as a name the
sidecar has no line for.

⚠️ **A verb that collides is announced in the pane, not only on stderr.** Two spec names that
sanitise to one tool name leave the later one unserved — the agent is simply never told it
exists, everything else works, and the only trace was an `eprintln!` that a console started
from a PATH shim writes to nobody. `start_approvals` now hands the sentence back for
`ConversationPane::new` to seed the log with, so it lands at the head of the scrollback (the
route §1.1 made real by drawing that log at all) as well as on stderr — the same rule the
exposure audit sets, for the same reason: the band's slot holds one truncated line and the
next diagnostic replaces it. `console_main`'s table is asserted collision-free, so this path is
dead today; `collision_note` is pure and pinned by test precisely because a safety net nobody
has pulled is worth exactly as much as its test.

##### 🚨 §7's withholding property is now re-measured every session, by the console

`doc/console_approval_protocol.md` §9 point 4: the guarantee that Claude Code withholds the
handler from the model is **tied to the flag** and must be re-measured **per server** —
and serving real capability tools from the same server is precisely the change that could
disturb it. `system/init` already carries the model's whole tool list, so the console checks
itself: `ExposureAudit` compares that list against the handler's namespaced name and its own
served names, and the result goes to the band's log **and** to stderr at every init.

It distinguishes three states, not two, and the third is why it exists:

| State | Line |
|---|---|
| handler absent, our tools visible | `approvals: handler withheld from the model as measured, N of M console tools visible` |
| handler **present** in the model's list | `🚨 the approval handler is in the model's own tool list … do not trust this session's cards` |
| the session reported **no** tools | `the session reported no tools — the approval handler's exposure could not be checked this init` |

⚠️ A pass read off an empty list would be the false negative the whole arrangement exists to
avoid, so silence is never reported as a clean bill. ⚠️ A served tool missing from the
model's list is reported and is **not** a fault by itself: MCP tools arrive deferred, so a
name can be reachable without being preloaded — but "the agent could not find our tool" and
"we never served it" are otherwise indistinguishable from outside.

⚠️ **This is the console auditing what the CLI reports, not an independent measurement.** A
CLI that reported its tool list wrongly would fool it. That is still strictly more than a
measurement nobody re-runs — and see the honesty ledger: the live check on *this* build has
not been performed yet.

**How a tab opens one.** `HarnessSpec::conversation` — the registry is data, so the
front-end is a field. `claude-chat` is the one built-in row that sets it, on every
platform, **beside** the terminal `claude` row rather than instead of it. `command`,
`wsl` and the whole `launch_argv` decision are inert for a conversation spec (the flags
above are the CLI's own and a user argv could silently break the persistence); `cwd` and
`detect` still apply.

```
ORGANON_SHELL_TABS=claude-chat organon-console          # one conversation tab
ORGANON_SHELL_TABS=claude-chat,shell organon-console    # a conversation beside a terminal
```

**Rule 5′ governs the split** (execution plan §6, which repealed the old harness-agnostic
rule *in writing* so nobody enforces it against the pivot later): the terminal host is
harness-agnostic in full; the conversation view is harness-specific and says which
harness; and **degrading to a terminal tab is always available** — a harness we have not
integrated is not unsupported, it is supported the old way.

#### 🚨 Where the agent works — four rules, stated out loud, never inherited

**The defect, measured 2026-08-13.** An agent in a conversation tab was asked to use the
`organon-cli` skill and answered `Unknown skill: organon-cli`. Before that, asked to show
something in the portal, it spent several approval cards running `ls` and `--help` to
rediscover a CLI with an 18 KB guide sitting in the repo. The built-in `claude-chat` row
carries no `cwd`, `AgentSession::spawn` turns `None` into *the app's own directory*, and a
console launched from Explorer or from a PATH shim is in no project at all. So the agent saw
no repo-local `.claude/skills/`, no project `CLAUDE.md`, no `CONSOLE_ARCHITECTURE.md` — and
**nothing anywhere said so.** The only symptom is an agent that seems oddly ignorant.

**Why it is worth more than one broken lookup.** Execution plan §5.9.26 records James's
direction: the console is to be extensible from inside itself, on the Pi paradigm — the agent
gets its own docs plus a skill teaching it to change the thing it lives in. An agent that
cannot see the repo cannot do any of that, and §5.9.26 names this file as the authority it
should consult *before* the tree. The paradigm was failing at its first step, silently.

`harness::conversation_cwd` is the one place that decides, and it is pure — a function of
(spec, platform, launch directory, environment, a marker test), so every rule below is a unit
test rather than a thing you find out by launching:

| # | Rule | `CwdSource` |
|---|---|---|
| 1 | `HarnessSpec::cwd`, tilde-expanded — the user's per-**tab** answer | `Spec` |
| 2 | `$ORGANON_SHELL_PROJECT`, tilde-expanded — the user's per-**launch** answer | `Env` |
| 3 | the nearest project root **at or above** the launch directory | `ProjectRoot` |
| 4 | the launch directory itself — today's behaviour, now *stated* | `LaunchDir` |

📌 **The product must not name a project, and rule 3 is how it avoids having to.** A
conversation tab is not inherently about this repo — someone may want an agent about their own
work, and an explicit `cwd` on the built-in spec would be wrong for them and unmaintainable
for us. But *"`cd` into a checkout and start the console"* is the ordinary way anyone reaches
any project, and rule 3 makes that land in the checkout's root with **no configuration at
all**, for any checkout, without a single path in the source. The console's own repo therefore
works for the same reason everybody else's does. `is_project_dir` is the marker: `.claude/`
first (the literal thing that was missing), then `CLAUDE.md`, then `.git` — existence, not
directory-ness, because a git worktree's `.git` is a file.

⚠️ **Home is never *discovered*, only inherited.** The walk stops at the home directory: a
`~/.claude` is user-global configuration that every agent gets wherever it starts, so treating
it as a project root would quietly aim a console launched from `~/Documents` at the whole home
directory — which on this machine is explicitly not a codebase. Launching *in* home still
lands there, via rule 4. The stop test is `Path::starts_with`, component-wise and
case-sensitive; a Windows launch path cased differently from `%USERPROFILE%` walks one or two
ancestors further, which costs an extra marker test and cannot produce a wrong answer.

📌 **Rule 3 is deliberately NOT applied to terminal tabs, and rules 1–2 already were.** A
shell announces its directory in the prompt and `cd` is one keystroke, so a tab that started
in `native/` because that is where you were is right — ascending to the repo root would be an
unasked-for correction, and running `cargo` is the commonest reason to be in a subdirectory.
An agent's working directory is invisible *and* decides which instructions and skills exist at
all. The two cases genuinely differ, so the resolution is conversation-only and terminal-tab
behaviour is byte-for-byte unchanged.

⚠️ **The alternatives, and what they cost.** *An explicit `cwd` on the built-in spec* — names
Organon's checkout in product data, wrong for every other user, and stale the moment the repo
moves. *The launch directory alone* — that is rule 4, i.e. today, and it does not survive
being started from a shim or from Explorer, which is exactly how the defect was reached.
*Per-tab only, via `harnesses.json`* — correct but not sufficient on its own: that file is
**machine configuration outside this repo**, so it cannot be shipped, cannot be tested here,
and a fresh clone gets nothing. It stays rule 1 because a user's explicit row must win; it is
not the default because a default that requires hand-editing JSON on every machine is not a
default. Rule 2 exists because the launch shims on this machine already set `ORGANON_SHELL_*`
and one more line there is cheaper than editing a JSON file in the app's store root.

**Nothing is silent any more, and that was the point.** `harness::cwd_notes` produces the
lines — the directory and *which rule chose it*, **always**, plus a warning when the resolved
directory satisfies no marker at all (*"no `.claude/`, `CLAUDE.md` or `.git` here — this agent
starts with no project skills and no project instructions"*, naming all three ways to fix it).
⚠️ The first line is unconditional on purpose: a diagnostic that only fires when something is
*detectably* wrong cannot cover a resolution that is wrong in a way this code cannot see — a
root found two levels above the one you meant — and stating the answer every time is what
makes that inspectable at all. `console_main` says it twice, to two different readers: `stderr`
for whoever started the console from a terminal, and `ConversationPane::note` for whoever is
looking at the console. The agent's *own* report of its cwd is on the model plate's hover
(`SessionFacts::cwd`, from `system/init`) — that is the independent confirmation, since it
comes from the process rather than from the code that spawned it.

⚠️ **`ConversationPane`'s log had never been drawn anywhere**, from the day the pane was
built. `pub fn log()` existed and no caller used it, so *"approvals are not wired — a tool
that needs permission will fail instead of asking"* has been written to a reader that does not
exist for its whole life. `scrollback` now draws those lines dimmed at the head of the
transcript. Same defect as an inherited working directory, one layer over: the console knows,
and says it to nobody.

#### Card density — success is quiet, and only a departure from normal takes weight

**The problem, from a real screenshot.** *"The console feels very busy… a typical screenshot
is five or six tool calls, each with a beveled border around it, so it feels like a list of
bevel-bordered status updates. You don't want to see all that while you're developing."* The
sharper diagnosis is that a completed tool card rendered its full arguments and its full
output, forever, at full weight — so a turn's **mechanical work occupied the transcript in
proportion to how much work it was, rather than to how much attention it deserved.**

`card_density.rs` is the seventh conversation module, and like `text_diff.rs` it has no egui
in it: the part that can be *wrong* is a set of pure functions over plain values.

**The state → weight rule.**

| state | treatment |
|---|---|
| running | open, unchanged — the work is happening |
| **succeeded** | one dense line: **the verb, the object, and a magnitude**. Expandable. |
| **failed** | open, bordered, loud — **unchanged, and structurally never collapsible** |
| a consecutive run of settled successes in one turn | one row with a count, expandable |

🚨 **Collapse, never delete.** The console's identity is that it gates and evidences what an
agent did, so nothing here removes an element, edits a transcript or drops a byte. Four things
must not be lost, and three of them are enforced by construction rather than by care:

1. **The record.** Every collapsed thing is one click from the card it was. The full
   arguments, the full output and the correlation id are all still in the model — the density
   map is a side map keyed by `ElementId`, exactly like `PanelState` and the diff cache, and
   for the same reason (the transcript is folded from a stream and would rewrite anything kept
   inside it).
2. **The `toolu_` correlation.** An approval card and the result it authorises share a tool-use
   id and nothing else (`doc/console_approval_protocol.md` §3). So **an authorised call is
   never anonymous**: `gated_calls` reads every `tool_use_id` off the approval elements in the
   flow, a gated call is excluded from grouping outright, and its dense row *draws the id*. An
   ungated call has no approval to be linked to, so its id is one click away like everything
   else.
3. **Errors.** A failure never settles, at any scroll position, so it is never collapsed and —
   because grouping consumes only *settled* successes — it can never be inside a group. **A run
   containing a failure is two runs with a failure between them.**
4. **Scroll position.** Below, because it is the requirement most likely to be got wrong.

🚨 **Scroll stability, and it is a construction rather than a compensation.** Changing a
card's height re-lays out the transcript; `doc/console_rewrap_measurement.md` measured what a
*width* change costs and this is the same hazard from the other side. Two rules make a jump
impossible instead of correcting one afterwards:

* **An automatic density change is applied only while the view is following the live edge.**
  `DensityMap::settle` takes the pane's own `pinned` bit, so a card that completes while a
  reader has scrolled up simply does not settle yet — it settles the moment they return to the
  bottom, where `stick_to_bottom` holds the last row still and the shrink is absorbed above
  it. While anybody is reading history, **nothing above them can change height at all.**
* **A manual toggle changes only the height of content at or below the row that was clicked**,
  and that row is on screen because it was clicked. Everything above keeps its layout
  position, so expansion grows downward, away from the reader's eye.

⚠️ **This is why the collapse is not re-derived from `ToolState` at each draw.**
`ToolState::Complete` arrives as a `Change::Updated` on an element that may be far above the
live edge — a tool that ran for two minutes while the agent wrote twenty more elements.
Collapsing on the state alone would yank a reader who was nowhere near it. The measured
consequence of getting this wrong is not available (see the ledger); the mechanism is pinned
by `a_card_collapses_only_while_the_view_is_following_the_live_edge`, which drives a real
frame and would pass just as happily if `scrollback` passed `true`, were it not asserting the
scrolled-up case first.

**The grouping boundary is a consecutive run, not a turn** — and the turn is a hard boundary
*on top of* that, so a group is always work done inside one turn and never a run that
straddles two. A turn interleaves prose and calls ("I'll read these three files", three calls,
"now the edit", two calls); grouping per turn would have to either reorder the transcript,
which destroys the record, or emit a group spanning the prose between its members, which
claims an adjacency that is not there. A run preserves order exactly and groups precisely the
block that *reads* as a block. `GROUP_MIN` is **3**: at two, a group row costs one row and
names no verbs where two dense lines cost two rows and name both, so the trade is a wash.

**A card lands expanded and collapses when it completes**, rather than landing collapsed. The
work is news while it is happening — that is the whole argument for the card in the first
place — and a call that opens already-summarised would make a running tool and a finished one
look the same. ⚠️ **And a hand outranks all of it, permanently.** `CardState` carries two
independent bits: `settled` is the automatic one, and `by_hand: Option<bool>` is a human's
standing instruction that nothing automatic ever clears. A card the reader opened stays open
through every later event, because *a card that re-collapses under a reader's hand is worse
than one that never collapsed.*

📌 **Grouping is structural and expansion is not**, which is what stops a hand from
restructuring the transcript. Membership is decided from the `settled` bit alone, so opening a
group — or opening one of its members afterwards — makes rows taller and never splits the
group into two groups and an orphan under the reader's finger.

**Magnitude, per tool, in provenance order** — what the *tool* measured beats what the view
can derive from what the tool sent: `ResultDetail`'s line counts (`120 lines`, `120 of 900
lines`); then the cached `Edit` alignment (`+3 -1`); then a dispatch's `task_*` progress
(`3 tools · 12.4s`, the one duration that exists because the *harness* measured it); then the
output's own line count. 🚨 **A tool with no obvious magnitude renders none** — the slot is
simply absent, never a zero and never a stand-in, the same refusal `SessionFacts` makes for a
context percentage whose denominator is not on the wire. An invented number on a row this
dense would be read as a measured one.

⚠️ **A group row carries no duration, and the design that commissioned this asked for one**
(`7 tools · 12s`). `ToolCard` carries no timing and `conversation.rs` has no clock by design —
the same refusal `SubagentProgress::duration_ms` makes for a single card, one level up. A
number the view timed itself would be the view's stopwatch wearing the agent's voice, and it
would keep counting for a session that had silently died. The row says how many and which
verbs; the seconds are not on the wire.

⚠️ **The disclosure marks are ASCII `+` and `-` on purpose.** egui does no OS font fallback
and the four fonts it bundles carry no disclosure triangle — `▸` and `▾` would be boxes,
exactly like the `✓`/`✗` that shipped in the subagent card. The allowlist in
`no_symbol_the_console_draws_is_a_glyph_egui_lacks` now covers this site too.

⏸ **Out of scope, and shaped so it can be added.** The **in-flight slot above the composer** —
an ephemeral row that is replaced rather than appended — is composer-adjacent and belongs with
whoever is editing that band. It attaches without touching this model: `Transcript::running_tools()`
already names the live ids, a running card never settles and is therefore never grouped, and
the slot would be a *second view* of those same elements rather than a state they enter. The
only thing that would have to change here is the choice to keep drawing a running card in the
flow as well, which is a display decision and not a structural one.

⚠️ **Suppressing content *within* a card is a separate, real decision and is not made here.**
A subagent card renders the harness's metadata block verbatim — *"Async agent launched
successfully… never quote any part of it"* — which is noise a reader never wants. That is a
question about what a card should elide rather than about how long it keeps its weight, and
folding it into this tier would have made one change out of two.

### 1.2 The portal — a screen-anchored, live, orbitable window onto the world

`organon console portal open`, typed at a prompt **inside the console**, floats a rendered
window over the transcript. The transcript keeps scrolling underneath it; the portal holds its
place on screen. Drag it to orbit, wheel over it to zoom, `organon console portal close` to
give the rows back.

| Piece | Where |
|---|---|
| the state machine, the rect, the pointer test — all **pure** | `organon-console/src/portal.rs` |
| the wheel claim | `term_view::wheel_scrolls_the_transcript`, fed from `term_view::draw` |
| the texture, the render, the paint | `console_main.rs::{render_portal, free_portal, paint_portal}` |
| which engine frame goes where | `console_main.rs::engine_plan` |
| the verb | `cli::ConsoleOp::Portal` + `cli::PORTAL_WORDS` → `console.portal` |

**Screen-anchored is the new thing.** Every anchor the console had before this was a *scroll*
anchor — `block_anchor` pins a rectangle to a run of lines and the picture rides them off the
top. The portal is the complement, and it is what James asked for: *"the window could float in
some way so that everything flows around it … so when it scrolls, the window doesn't scroll
away."* The rect is recomputed from the pane every frame and remembered nowhere, so it is a
function of where the window is *now*.

⚠️ **It occludes the rows it floats over.** They are drawn and then covered. That is what
floating means, and it is the other half of why the verb closes as easily as it opens.

📌 **Anchored top-right on purpose.** A terminal's live edge is the bottom — new output, the
prompt, the cursor. The top is where rows go to scroll away, so the portal covers the oldest
visible text and never the newest.

#### 🚨 It shows the WORLD, and that is correctness, not taste

An installed substrate rig overrides the camera **wholesale**: `world.rs:6526` reads
`substrate_rig` first and returns its whole six-tuple *before* `yaw`/`pitch`/`distance` are
consulted — and those three are precisely what `World::apply_camera_input` writes. A portal
showing the substrate would read a drag, convert it, apply it, and draw an identical frame:
green build, no log line, and an investigation that starts in `scene_input.rs`, which is
correct code. Showing the World clears the rig and dissolves it by construction.

✅ **And it makes "control Organon from the shell" true for free.** The CLI's parameter lane
drains inside `World::frame_body`, which is what `render_to_texture` runs, and `term.rs:195`
injects `ORGANON_IPC_NS` into every tab the console spawns. So `organon set glow 1.0`,
`organon generator dna` and `organon recipe nebula`, typed at a prompt in a console tab, drive
the world inside the portal — **with no new code at all**. That was already built and shipped;
the portal is the rectangle to see it through.

It is also *simpler* than a conversation surface: a surface has to publish its own look into
`Shared` and put the console's back afterwards or `organon status` reports a picture that is
not the window. The portal shows the console's own snapshot, so there is no dance.

#### 🚨 The portal claims the wheel — reversing a decision this file argued the other way

`block_panel::pointer_inside` is fed `panel_placements` and not every patch, on a stated rule:
*a scene patch is something to look at, so the wheel over one keeps scrolling the page exactly
as the wheel over a paragraph does.* The portal reverses that, **for itself only**, and the
argument is one sentence: **a scene patch is a picture; a portal is an instrument.** A picture
that stole the wheel would break scrolling; an instrument that did not take it would be an
instrument you cannot reach. The patch's behaviour is byte-for-byte unchanged.

⚠️ **An explicit rect test is the only mechanism that works, and this is why.** `term_view`
reads the wheel and every key from **raw input** (`i.raw_scroll_delta`, `i.events`), so egui
layer order is irrelevant to it — an `Area`, a later-registered widget and a modal are all
equally invisible. `scene_viewport` *does* consume the wheel from inside (it zeroes both scroll
deltas), which is what covers a `ScrollArea`; the two are not alternatives, they cover
different readers.

#### 📌 The state machine is also the render budget

`engine_plan(portal_open, region_holds_world, backdrop, patches_want_image) ->
(BackdropSource, Option<ViewportTarget>)` is the one place both decisions are made, and
`the_engine_is_asked_for_at_most_one_frame` proves the property over the entire input space:
**at most one `World` render per console frame, in every state.**
`SURFACE_RENDERS_PER_FRAME`'s doc rules the two-render case out — `frame_index` and
the TAA jitter phase riding on it are shared between the targets, invisible on a still lit
plane and visible-and-intermittent on a moving World. A live portal beside a live backdrop is
exactly that case. So an open portal **takes the frame**.

⚠️ **The cost, stated rather than discovered: while the portal is open, the backdrop does not
paint and a scene patch has no picture.** The promotion that renders a substrate for a patch
(`Off` + `patches_want_image`) is what the portal displaces. `backdrop_source` is never
written, so closing the portal restores everything with no remembered value to get wrong. The
alternative is a second `World` — ~50 shaders and ~62 pipelines by `render_surfaces`' own
pricing, still trading jitter phases.

✏️ **Tier 2b: there is a second claimant, and the portal still wins.** A region holding `3d`
(§1.14) wants the same one frame, so `engine_plan` gained an input and now answers *which*
presentation gets it rather than a bool. The rule is **the portal takes the frame from a region
viewport too**, and the reason is the one this section already gives: the portal is temporary and
dismissable, so the state it creates ends in one word, while the region is the persistent thing a
person arranged and is still arranged underneath — nothing is written, so closing the portal hands
the frame straight back. §1.14 carries the argument in full, including the two rejected rules and
why "the refusal reaches nobody" disqualifies both. ⚠️ The yielded region **paints a notice**
naming what holds the world and the command that releases it; it does not blank and it does not
keep showing the picture it had a moment ago.

#### ✏️ One presentation of a viewport, not the only live rectangle — Tier 2b

Everything above is unchanged: the verb, the two states, the screen-anchored rect, the wheel
claim, the "shows the World" correctness argument. What changed is the **description**, and it is
worth writing down because the code now depends on it. A **viewport** is a producer plus a camera
plus a texture. The portal is one way of presenting one — floating, summoned, dismissable — and a
region is another — placed, persistent, arranged by hand. `scene_input::SceneMode` has modelled
exactly that distinction since before either existed, and both of these are `Workstation`.

🚨 **One mechanism, two presentations — never two implementations.** One texture
(`Console::viewport`), one `render_viewport`, one `paint_viewport`, one `SceneInput` accumulator,
one `pane_pixels_in` ratio, one `pointer_inside` test widened to a list. §1.14's table is the
site-by-site account. The property this preserves is the one James asked for: **when a second
producer arrives, the portal shows it as readily as a region does, because the producer seam sits
below both.**

#### Why it is a field and not a `SurfaceKey` variant

`Console::portal: Option<SurfaceTexture>`, beside `backdrop`. Eviction is a policy for *many
things competing for few slots*; a portal is *one thing that is open or closed*. It is
requested every frame it exists, so its stamp is always `now` and `surfaces_to_evict` could
never choose it — the variant would exist only to be excluded from the one function the type
serves, and would then have to be remembered out of `free_all_surfaces` and taught to the
eviction log so it did not print a fabricated element id. The deciding argument is smaller and
harder: **the portal must work in a terminal tab**, where there are no elements and `ElementId`
means nothing. `SurfaceKey`, its tests, `SurfaceImages` and the whole `conversation_view` seam
are untouched; `SurfaceTexture` and `make_surface_texture` are reused, which is the part worth
reusing.

#### What is deliberately NOT built

**Immersive, full screen, the animated grow, and the click/double-click transitions.** This
tier is one visible beat. The seam is `portal::step` being total over `(state, event)`; adding
immersive is adding a variant and its arms, and the render-budget invariant already survives it
(in immersive the portal *is* the backdrop, so again one render).

⚠️ **Escape is not consumed, and that is a decision.** In a terminal tab the keyboard is the
child's — *"the terminal owns the keyboard, full stop"* — and `vim` needs Escape, so taking it
would have to be conditional on state. The states that need an Escape are the ones where the
portal covers the window and a prompt may not be reachable; in this tier the prompt is right
there and `organon console portal close` is the way out. Spending the console's **first
state-dependent key ownership** on the one case that does not need it is the wrong first spend.
`ctx.input_mut(|i| i.consume_key(..))` is the mechanism when it is time — it `retain`s the
event out of the same `i.events` vector `term_view` clones, so it genuinely removes it from the
PTY stream.

### 1.3 The camera — the one an agent can move, and the rule that a hand outranks it

`organon console camera --distance 40`, typed at a prompt **inside the console**, walks the
viewpoint in. Before this the CLI could choose what the world *is* — every generator, every
material, `cam_path`'s auto-orbit — and could not say where to stand to look at it. James, watching
an agent try to show him something through the portal: *"it's having trouble because the camera is
far away and I don't think the CLI has commands to move it, but it's fundamentally working."*

| Piece | Where |
|---|---|
| the bands, the defaults — **one table, four readers** | `scene_input::{YAW_LIMIT, PITCH_LIMIT, DISTANCE_MIN, DISTANCE_MAX, DEFAULT_*}` |
| the absolute write | `scene_input::CameraInput::Frame` → `World::apply_camera_input` |
| the vocabulary | `cli::{CameraFraming, CAMERA_WORDS}` + `cli::ConsoleOp::Camera` → `console.camera` |
| **who wins** — pure | `organon-console/src/camera.rs` (`arbitrate`, `HAND_HOLD`, `viewpoint_is_visible`) |
| the site that obeys it | `console_main.rs::Console::frame_camera`; the hand's stamp is in `redraw` |
| **reading it back** — pure | `camera::{Viewpoint, ViewpointCell, Mover, last_mover}`; `Viewpoint::report` is the JSON |
| the read's source of truth | `World::camera_framing()` → published in `redraw`, served by `console.camera.read` |

#### ⚠️ There are TWO cameras and this is only one of them

`cam_path` / `cam_speed` / `cam_kick` / `cam_damping` — the `organon set` lane, on the **world** —
are the *auto-orbit*: part of the composition, travelling in `Shared`, saved in a preset, and the
only camera the CLI could reach before this. This is where the **viewer stands** to watch that
composition: host state on `World`, in no snapshot and no preset, and exactly the three fields a
drag and a wheel over the portal already write.

📌 **They compose rather than compete.** The camera finalization adds the auto-orbit's offset to
this base (`let yaw = self.yaw + off.dyaw`), so a shot framed here still spins if `cam_path` says
to. Neither replaces the other and neither needed to learn about the other.

#### 🚨 The hand always wins, and it is enforced rather than remembered

This is not a new rule. It is the one the lighting renderer on this workstation already runs — it
polls the lamp for a state it did not command, and a hand on the app drops the agent's scene and
refuses new ones for a while. *A person always wins.* What makes it worth **enforcing** here is
that the portal's camera is the first thing in the console an agent can move **while a hand is on
it**: a drag and `console camera` write the same three fields, `World::apply_camera_input` cannot
tell them apart, and without arbitration the last writer in the frame wins by accident. **A control
that fights your hand is worse than no control.**

- **The stamp is taken where the two are still distinguishable** — in `redraw`, from
  `SceneGesture::inputs()`, one line before both become `CameraInput`. An idle console never
  stamps, so an agent is never held off by a hand that is not there.
- **`camera::HAND_HOLD` is two seconds**, and the number is bounded on both sides rather than
  felt. *Longer* than any gap inside one interaction (a drag stamps every ≈16 ms, a wheel-notch
  train ≈100 ms, a hand releasing to re-grab a few hundred), so a pause mid-gesture is never read
  as the end of one. *Shorter* than the time it takes to **ask** for something — a request
  reaching an agent and coming back is seconds — so a command caused by the person is not refused
  when it arrives. Two properties, both pinned by a test.
- ⚠️ **The refused command is dropped, never queued.** A deferred framing would arrive after the
  hold expires, as a jump into a shot the person has since composed — the same failure the hold
  exists to prevent, delayed.

#### Absolute, and why that is forced rather than chosen

The console command lane is fire-and-forget with **no return path** (`cli::console_cmd_path`'s
doc). A caller on it can never read back where the camera is, so it can never compute a delta —
absolute is the only shape the transport carries. That is also why `--reset` exists and is not a
convenience: it is the one framing a caller can name **without knowing the current one**, so
`--reset --distance 40` ("the default view, then pull in") is a complete workflow with no
read-back anywhere in it. `scene_input::DEFAULT_*` are `World::new`'s own initial values, named
rather than repeated, so reset is provably *the framing the window opened with*.

#### Reading it back — `console.camera.read`, on the one lane that can answer

An agent in a conversation tab can now ask **where the camera is**. It gets the three axes as
measured this frame, whether anything on screen is showing them, who moved them last, and
whether a hand is holding them right now:

```json
{"yaw":0.699999988079071,"pitch":0.44999998807907104,"distance":520.0,
 "portal_open":false,"backdrop_shows_world":false,"visible":false,
 "moved_by":"nobody","hand_holds":false}
```

**The problem it ends, measured 2026-08-13.** Asked to frame an object, an agent set a distance
blind, shelled out to `organon snap`, read the PNG back off disk, judged it, and went round
again — five round trips, each costing a human approval prompt, to compose one shot. The verbs
are absolute *because* nothing could read; a read is what makes a relative move computable.

🚨 **The read is served over MCP and has no CLI spelling, deliberately.** The MCP server runs
**inside the console process** (`McpHttp::start`, from a conversation tab), so it can hand back
the console's own state. `organon console …` still cannot: giving it a read needs the
request/reply sidecar §2 names, which is not built and is not half-built here. So
`console.camera.read` lives in `mcp_specs()` and **not** in `console_specs()` — the latter's
totality (every entry has a `ConsoleOp`, a sidecar line and a clap subcommand) is what `op_from`
and the round-trip test depend on, and a read has none of the three.

🚨 **It reports the camera, never the last command.** `Console::redraw` publishes a `Viewpoint`
into a `ViewpointCell` from `World::camera_framing()` — the world's own three fields, *after its
own clamps* — at the one point in the frame where **both** writers have run: the agent's framing
(drained at the top of `redraw`) and the hand's gesture (applied a few lines above the publish).
Anything remembered on the console side would be an echo, and an echo is exactly wrong here:
a hand outranks an agent, so the value an agent last set is routinely not where the camera is,
and handing it back as current would be a lie the console told confidently.

- ⚠️ **`hand_holds` is settled at *read* time, not publish time.** The hold is two seconds and a
  snapshot can be older than that. It is the field that closes half of §1.3's "the refusal
  reaches nobody" gap: an agent whose framing vanished can now ask why and be told a hand has it.
- ⚠️ **The axes are widened exactly, never rounded.** `f64::from(0.7f32)` is `0.699999988079071`,
  and that is the only spelling a caller can write straight back and land on the same `f32`.
  Rounding would also let a value sitting on a clamp boundary read as outside its own band.
- ⚠️ **A non-finite axis is omitted, not serialised.** `serde_json` renders one as `null`, which
  a model will try to use. `apply_camera_input` filters non-finite input, so this is a belt.
- ⚠️ **An unpublished cell is a tool *failure*, not an empty object.** Before the first frame
  there is genuinely no measurement, and `{"yaw":0,…}` is a viewpoint a caller would act on.
- ⚠️ **A write is still one frame behind a read.** A framing travels the sidecar and lands on the
  next drain, so a read issued in the same breath may answer from the frame before it. The read
  is honest about what it is — the last measured frame — and it is not a synchronous echo of a
  write that has not happened yet.
- 📌 **`moved_by` is derived from two stamps** (`hand_camera_at`, `agent_camera_at`) by
  `camera::last_mover`; the agent's is taken **after** the arbitration, so a framing the hand
  held off never claims to have moved anything. A tie goes to the hand, as every other decision
  in that module does. `nobody` is a real answer and a different fact from "an agent set it to
  the default".

**A separate verb, not a zero-argument `console.camera`.** Every axis on the write is already
optional, so `{}` is a shape it can be called with — and it earns *"needs at least one of […] —
a framing that names no axis would move nothing"*, which is the right answer to a model that
forgot its arguments. Overloading would turn that mistake into a silent success returning
something nobody asked for, give one tool two descriptions to be chosen by, and give the
approval layer one name for two acts that plainly deserve different answers to "may I?".

#### Refused, not clamped — and the asymmetry with the hand is deliberate

`World::apply_camera_input` **clamps**, because a hand physically cannot ask for more than the band:
a wheel that reaches the ceiling simply stops. A typed `--distance 9000` is not a gesture that
overshot, it is a number that means something else, and silently answering 4000 would let the
mistake look like it worked. So the band is a **gate** on the command lane — `ArgKind::Float`'s
range in the schema, `CameraFraming::in_range` at the clap boundary — and the world's clamp stays
underneath as the belt. Both read `scene_input`'s constants; a second copy is how an agent comes to
be refused a viewpoint the drag can reach, and that reads as the camera being broken rather than as
two constants disagreeing.

#### 🚨 The axes are optional, and "optional" had to be taught to the validator

`console.camera` is the first spec on this lane whose arguments are **not required**, and that
turned out to be a new capability rather than a new value in an existing field.
`op_args` serializes the whole slot list — `{"reset": false, "yaw": null, "pitch": null,
"distance": 40.0}` — so the dispatch record a reader of `events.jsonl` sees names every axis,
including the ones nobody set. `CommandService::dispatch` runs `validate_args` **before**
`ConsoleTarget::execute` ever reaches `op_from`, and `validate_args` matched on key *presence*:
a `null` reached the `Some(value)` arm, `ArgKind::Float`'s `as_f64` returned `None`, and the
answer was `yaw: expected a number, got null`. **Every partial framing was refused, `--distance
40` first among them.**

The rule now lives in `validate_args`: **a declared argument present as `null` is absent**, and
absence is only *permitted* where the schema says the argument is optional (a required argument
spelled `null` is still refused, and now reports "missing" rather than naming a type). Two
reasons it belongs there rather than in `op_args`. It is a general property of optional
arguments, so the next verb with one does not re-discover the trap; and the same reading was
already the intent one level up — `args: null` has always meant "no arguments", and a slot of
`null` meaning "no value" is that same sentence, one level in.

⚠️ **The comment in `op_args` asserting this behaviour was written before the behaviour
existed.** It described `validate_args` as reading `null` as absent, which was simply not true
of the code it named, and nothing failed — the contract was written down, believed by its own
author, and depended on by the first caller to need it. That is the defect class worth
remembering here: *a comment describing a collaborator has no test behind it unless someone
writes one*, and the test that catches it must cross the boundary the comment spans.
`an_optional_arg_present_as_null_is_absent_and_a_required_one_is_missing` (command.rs) pins the
rule in the pure crate; `a_partial_framing_survives_the_real_dispatch_and_reaches_the_target`
(console_main.rs) pins the whole lane, because every other camera test calls `op_from`/`op_args`
directly and **that is exactly why none of them saw it**.

#### It says so when it moves something nobody is looking at

An installed substrate rig overrides the whole camera tuple, and `off` draws nothing at all — so a
framing applied with the portal closed and the backdrop on `substrate` succeeds, moves real state,
and changes not one pixel. That is §1.2's silent trap met from the other side. The console cannot
fix it (the camera really did move, and it will be there the moment something shows the world), so
`camera::viewpoint_is_visible` decides whether to say so on stderr instead.

### 1.4 The theme — every colour the console paints, in one owned value

**`organon-console/src/theme.rs` holds `Theme`: a plain struct of `Color32` fields, one per
semantic role, grouped by area (transcript, cards, status strip, composer, terminal, patch
panels, timeline, tabs).** `Theme::organon()` is the phosphor-green look the console has
always had and is still the **default**. The extraction is meant to be pixel-identical, and
`theme_organon_is_the_look_that_shipped` is what backs that: it pins every field against the
literal RGB the corresponding `const` held on `main` before the move. ⚠️ No window has been
opened on it — see the ledger.

**Three more palettes stand beside it: `light`, `dark` and `chocolate`**, each a constructor,
each specified by James as ~10 named roles. `Theme::by_name(&str) -> Option<Theme>` resolves a
name against `Theme::NAMES`, and `None` for anything else — never a panic, never a substitute,
because a picker wants to say "unknown" while `prefs.rs` wants `unwrap_or_default()`.
`Theme::resolve` is `by_name` with a refusal that carries the known set, and `Theme::named`
returns the palette *with its canonical name*, which is what a store records.

#### Reaching one: `organon console theme <name>`

**A person can now select a palette three ways, and the precedence is
`ORGANON_SHELL_THEME` → `preferences.json` → `organon`.** `theme::select(env, stored)` is that
order, as one pure function returning a `Selection` — the palette, its canonical name, a
`ThemeSource` saying which rung won, and `notes` the console prints on its own stderr.
`Console::new` reads the two sources and hands them in; the resolution is testable without a
process environment or a store on disk, which is why it lives in `theme.rs` and not inline.

**The verb is live, and live is the requirement rather than a convenience.** `organon console
theme light` reaches `Console::apply_console` on the console sidecar and repaints on the next
frame. Four palettes compared by relaunching four times is not a comparison: what is being
judged is a wash of colour across a window full of real text, and the judgement is made by
looking back and forth, which a relaunch destroys.

🚨 **`Console::set_theme` re-issues `set_visuals`, and it must.** `redraw` borrows `&self.theme`
afresh every frame, so the fields need nothing but an assignment — but `Visuals` is set *on the
context* and held there, so without the second call roughly half the window (sliders, popup
frames, the `TextEdit` selection wash, scrollbars) would keep the outgoing palette. That is the
same asymmetry that made `Theme::visuals` necessary at all, met from the other direction.

⚠️ **The pick is stored, and `set_theme` deliberately does not early-return when the palette
asked for is already the one painted.** Launch under `ORGANON_SHELL_THEME=chocolate` over a
stored `light`, then type `organon console theme chocolate`, meaning "yes, keep this one": an
early-out on the *painted* palette would repaint nothing (correct) and store nothing (wrong),
and the choice would evaporate at exit exactly as it did before any of this existed. The
**store** decides whether there is work to do. The write is load-modify-save, never a fresh
`Preferences { theme }` — that struct will grow fields, and constructing one here would
silently discard preferences this call knows nothing about.

⚠️ **A refusal is loud at both ends and it never approximates.** `bin/ctl.rs` restricts the
word at the clap boundary from `Theme::NAMES` *itself* rather than a copy of it, so a fifth
palette reaches `--help` and tab completion in the commit that adds it; `Console::set_theme`
resolves again on arrival, because a line written straight onto the sidecar never met clap. An
unknown name at startup **falls through to the next source** rather than resetting to
`organon` — a typo in a launch shim must not silently discard a stored choice — and says so
with the known set. There is no case folding and no nearest-match anywhere: a palette
substituted for the one asked for is indistinguishable from success, which defeats the point
of being able to tell which one you are looking at.

**One owner, still.** `Console` gains `theme_name: &'static str` beside `theme` — carried rather
than reverse-looked-up, because §1.4's own rule is that palettes may share field values, so a
`Theme` is not a reliable key to its own name.

**One owner: `Console` in `native/src/console_main.rs`**, one field, borrowed into `redraw`'s
closure beside `sessions` and `strip` and passed down as `&Theme` to every draw call —
`tabs::tab_bar`, `term_view::draw`, `conversation_view::draw`, `block_panel::draw`,
`paint_portal`, and (for the dormant v2 compositor) `app::ui` → `timeline::show`. There is
no `static`, no `thread_local!` and no `OnceCell`. That is the property the tier is for: a
palette reachable from anywhere stops being state, and a per-tab theme or a live preview
while one is being chosen becomes a rewrite instead of a second value. `ShellApp` gets no
theme field for the same reason — its `ui` takes one, so the process still has exactly one
owner however many front-ends draw.

🚨 **Roles that share bytes today are separate fields, and merging them is the mistake this
shape exists to prevent.** `term_fg`, `human_text`, `tab_active`, `tab_menu_installed` and
`panel_text` are all `#c8e6c8`; `context_arc_high` is `mode_alert`'s amber (it was literally
written `= MODE_ALERT`); `timeline_status_denied` equals `timeline_status_failed` and
`timeline_status_cancelled` equals `timeline_status_pending`. Deduplicating by value welds
two decisions together — a lighter palette almost certainly wants a terminal foreground and
a human's typed line to part company — so `roles_that_share_a_value_are_still_separate_fields`
asserts the coincidences *and* the separation on purpose.

**Ten roles specified, fifty fields to fill: the derivation rules.** Each spec names about ten
colours; `Theme` has about fifty. Every field a spec does not reach is derived by a rule
written at the site, never by an invented pigment, and the rules are four:

1. **The surface ladder.** A spec gives a page and a panel; the second raised step is the
   spec's own **hairline** colour, which is by construction exactly one step further from the
   page. (`chocolate` names all four of its steps — `#191919 → #1F1F1F → #262626 → #303030` —
   so none of this applies to it.)

   🚨 **`light`'s whole surface ladder is NOT its spec's, and this is the one place in any
   palette where that is true.** The spec's ladder is `#ffffff → #f7f8f9 → #e2e5e9 → #c9ced6`.
   It has been moved down twice, both times on James's instruction after looking at a running
   console, and the second move is a **correction of the first, not a contradiction of it**:

   | step | spec | 2026-08-14 · the page | 2026-08-14 later · the ladder | V now |
   |---|---|---|---|---|
   | page (`LIGHT_PAGE`) | `#ffffff` | `#fafbfc` | **`#d7d8d9`** | **0.851** |
   | panel (`LIGHT_PANEL`) | `#f7f8f9` | `#f7f8f9` | **`#d4d5d6`** | 0.839 |
   | hairline (`LIGHT_HAIRLINE`) | `#e2e5e9` | `#e2e5e9` | **`#bfc2c6`** | 0.776 |
   | strong (`LIGHT_STRONG`) | `#c9ced6` | `#c9ced6` | **`#a6abb3`** | 0.702 |

   The first move turned the page down 1.18 % of HSV value; James looked at it and said *"the
   white part is too white. Move it down to about a 0.85 V in the HSV system"*. 🚨 **The result
   is a light GREY page, not a white one** — `V = 0.851` is pale grey card stock, and that is
   what was asked for. Do not "fix" it back toward white because it stopped looking like paper.

   ⚠️ **The page could not move alone.** The steps are 3/3/3 (page→panel), 21/19/16
   (panel→hairline) and 25/23/19 (hairline→strong) — the page's own step is the whisper of the
   four, so a page dropped to `217` against a panel of `249` sits **32 units below it** and
   *inverts* the ladder. The failure is silent: every plate drawn on the page — the composer,
   the status strip, a bubble — would read as raised **out of** the paper rather than recessed
   into it, which is the opposite of the printed-publication metaphor. So the move is a
   **uniform −35 on every channel of every step**: all three inter-step distances survive to
   the unit, every step keeps its own cool tilt, and `217/255 = 0.8510` is the nearest a `u8`
   gets to `0.85 × 255 = 216.75`.

   ⚠️ **Uniform subtraction does not weaken the ladder — it strengthens it slightly**, because
   sRGB's transfer curve makes an equal code-value step a larger luminance ratio lower down.
   Measured WCAG contrast between adjacent steps *rises*: panel-on-page 1.026 → 1.030,
   hairline-on-page 1.220 → 1.253, strong-on-page 1.526 → 1.617. `strong` at `#a6abb3` is a
   more visible border than `#c9ced6` was, not a mid-grey one — the "floor" worry does not bite.

   🚨 **What the move genuinely costs is the TEXT ladder, which deliberately did not move, and
   nothing in the surface ladder can repay it.** `primary #0f1114` is untouched and still
   13.3:1 on the new page. The two weaker text roles are not:

   | foreground | on | before | after the move | ✏️ repaired |
   |---|---|---|---|---|
   | `primary #0f1114` | page | 18.25 | **13.25** | 13.25 — untouched |
   | `secondary` | panel | 5.70 | **4.12** ⚠️ under AA 4.5 | `#555b64` → **4.66** ✅ |
   | `faint` | page | 3.06 | **2.22** ⚠️ | `#737983` → **3.07** ✅ |
   | `faint` (`tab_menu_missing`) | hairline plate | 2.51 | **1.77** ⚠️ | `#737983` → **2.45** ⚠️ still under AA |
   | `success #1a6b46` | page | 6.26 | 4.55 | 4.55 — untouched |
   | `error #a32020` | page | 7.28 | 5.28 | 5.28 — untouched |
   | `accent #1440c4` | page | 7.92 | 5.75 | 5.75 — untouched |

   These numbers are set by a page James asked to lower against text he did not ask to darken,
   so **there is no compression of the surface ladder that fixes them** — the only repair is on
   the text, which is the other side of the fraction. ✏️ **That repair has now been taken**, on
   the two roles that fell under AA and on no others: `LIGHT_SECONDARY` and `LIGHT_FAINT`, each
   a **uniform per-channel subtraction** — the same method the ladder itself moved by, so both
   keep their cool tilt rather than being re-picked. `faint`'s −24 was chosen to land on exactly
   the 3.06 it held before the move; `secondary`'s −8 is the smallest that clears 4.5.

   ⚠️ **One role a single value cannot rescue everywhere.** `faint` on a hairline plate reaches
   2.45, not 4.5. `tab_menu_missing` labels a thing that is *absent*, and darkening it far
   enough to clear AA there would make "not mapped yet" heavier on the page than live secondary
   text — the wrong sentence. Recorded rather than fudged, and **asserted two-sided** so that a
   later drift in either direction is noticed.

   🚨 **The nine assignment sites are now two named constants**, for the reason the surface
   ladder already learned: `faint` was four repeated literals and `secondary` five, so a
   correction spelled as nine hand-edits is a correction that lands on eight of them. Fields
   still assign independently — no role is merged — only the *value* is stated once.

   📌 **And the table above is now a test.** `every_light_text_role_is_measured_against_the_
   surface_it_is_drawn_on` computes WCAG luminance and asserts each ratio against the surface
   the role is really drawn on. This table was prose for a day, and prose is precisely what a
   later edit is not obliged to keep true — the ladder move changed every number here without
   touching one text colour, because the two ladders are two sides of one fraction and only one
   was edited. ⚠️ **Still nobody's eyes.** These are ratios against a standard, not an
   observation; §3 carries it and only James closes it.

   **Four fields are functions of these steps and moved with them** (rule 4 below).
   ⚠️ **Two of them had already gone stale**: `composer_edge_dead` and `timeline_scripted_fill`
   are mixes "into the page", were computed against the spec's `#ffffff`, and the first
   correction left them behind — a written derivation quietly false for a day. `panel_fill`
   moved only because a test pinned it. New test
   `every_light_plate_mixed_from_a_surface_is_recomputed_from_it` pins all three of the mixes
   *and* the invariant that would have caught them without re-deriving anything: a plate mixed
   into a surface may never be brighter than that surface.

   **They are named constants rather than fifteen repeated literals.** The panel is five
   fields, the hairline six, the strong border four; a third correction spelled as fifteen
   hand-edits is a correction that lands on fourteen of them. Sharing a constant welds nothing
   — every field still assigns independently and a fifth palette can part any of them — it only
   stops one *step* of one ladder being two colours by accident. `term_bg` and
   `term_scrim_tint` are the original reason the pattern exists: the scrim is laid over the
   live backdrop at up to `SCRIM_FLOOR_LIGHT`, so a scrim left brighter than the page would
   cover a *larger* area than the terminal with the exact glare being removed.

   `the_light_page_stays_a_step_above_the_panel` pins the ordering, the minimum step, the
   uniform offset and rule 4's premultiply; the ordering needed no guard while the page was
   `#ffffff`, since nothing can be brighter than white.
2. **States.** A state the spec names takes its named colour; a state it does not name comes
   from the palette's **text ladder**, never from a hue the spec never introduced. ⚠️ That is
   why **none of the three has an amber**: "a tool is running" is primary text, not
   `organon`'s orange. `running`, `timeline_status_running` and `mode_alert` are the fields
   this decides.
3. **The one exception to 2.** "Blocked on a human" (`asking`), the context ring
   (`context_arc`), the approval accent and the non-default-permission note (`mode_alert`) take
   the palette's **accent**. They are what an accent is for — present, not an outcome — and
   `mode_alert`'s field doc already forbids drawing it red. ⚠️ `context_arc_high` breaks
   `organon`'s `== mode_alert` coincidence and takes the **error** colour instead: it is the
   one reading on the band that becomes a failure if ignored.
4. **Tinted plates** are a stated linear mix: the user bubble is the accent at **1:5 over the
   panel**, the scripted-replay banner is the **error at 20 % over the page** (a replay must
   never pass as live, so it is drawn from the warning family), `composer_edge_dead` is the
   **error 1:1 into the page**, and `panel_fill` is the **page premultiplied at `organon`'s own
   `0xe6`**.

⚠️ **`ansi16` is CHOSEN for all three and is marked so in the source.** All three specs were
written against the conversation view and say nothing about a terminal. `light`'s follows the
**GitHub Light** lineage (the widest-read light terminal palette, so most likely to be what a
TUI's own scheme was checked against) — note it maps *white* and *bright white* to dark
colours, or the text vanishes on paper. `dark`'s and `chocolate`'s reuse each spec's own
accent/success/error as blue/green/red and match a yellow, magenta and cyan to that weight.
⚠️ `chocolate`'s ANSI is the one place its monochrome discipline is deliberately not applied:
a program asking for red is asking for red, and rendering `git diff` in graphite would be the
console overpainting what a program said.

**Chocolate's two spec details that look like mistakes and are not.** `Theme::ok` is the
secondary grey `#8F8F8F`, because that field colours the literal word `ok` beside a finished
tool call and the spec says that word is not green. ⚠️ **The green the spec does name lives on
`timeline_status_ok`** — the console draws the word *and* its marker from one field, so "grey
word, green dot" cannot be expressed without a new field plus a `conversation_view` change;
that is out of scope for a palette and is recorded rather than fudged.
`chocolate_stays_neutral_and_keeps_its_ladder` asserts the rest: every neutral has its three
channels equal, so no dark surface can pick up a blue cast, and no surface is the accent.

**What a palette still must not touch.** Two things stay outside it, each because it is not
taste:

- **The xterm 256-colour cube and greyscale ramp.** `ansi16` is the theme's — a light
  console beside a black grid is two products in one window — but indices 16..=255 are a
  *standard*: a program asking for 196 is asking for xterm's red. **Truecolor and OSC
  overrides** likewise belong to the program that sent them.
- **`Color32::WHITE` at five `painter.image` calls** (`term_view`'s band and patch quads,
  `conversation_view`'s surface, `paint_portal`, `app`'s scene pane). That argument is a
  per-channel multiplier, not a colour; white is the identity, and a theme reaching it would
  tint the engine's own render. Each site carries a one-line comment saying so.

#### The scrim floor is the palette's now, and it still is not the setting's

🚨 **`SCRIM_FLOOR = 96` structurally forbade a light theme, and James's decision (2026-08-13)
was to make the floor theme-aware.** `Theme::scrim_floor` is an alpha byte on the palette;
`term_view::scrim_alpha(env, floor)` takes it and clamps `ORGANON_SHELL_SCRIM` up to it.
`term_view` keeps two constants — `SCRIM_FLOOR = 96` for a dark page (`organon`, `dark`,
`chocolate`) and `SCRIM_FLOOR_LIGHT = 192` for a light one.

**The rule was always "the glyphs stay legible". What was dropped is the assumption that
legibility means darkness.** A scrim is `term_scrim_tint` over the live backdrop and the floor
is how much backdrop it may leave showing. 96/255 of near-black stops a bright frame washing
out pale glyphs; the same 96 under a *white* tint protects nothing, because the glyphs are dark,
the danger is a *dark* backdrop, and the composite is a mid-grey that dark text disappears into.
A light palette was therefore not reachable by swapping colours at all — it would have sat under
a compulsory near-black veil however its fields were set.

⚠️ **The half that must not weaken has not weakened: no setting can cross a floor.**
`no_scrim_setting_can_cross_the_floor` now runs the whole byte scale, the parse failures and the
unset case against **every palette's own floor**, and asserts the two floors by name so a change
that quietly equalised them would still fail. What changed is *who names* the floor — a
compiled-in palette, which is a coherent instrument including the terms on which its own glyphs
stay readable, rather than a value typed into a launch shim.

⚠️ **`SCRIM_FLOOR_LIGHT = 192` is CHOSEN, not measured.** It is not derivable from 96: a strict
"same worst-case contrast" mirror returns a *lower* number, because sRGB gamma makes dark text
on mid-grey read better than pale text on the same grey — which would produce a light console
under a 40 % grey wash, technically legible and not a page at all. The reasoning used instead:
96 lets the world keep ~62 % of the frame, which a near-black page affords because its glyphs
are pale; against a light page and dark glyphs the world may have roughly a quarter, so the
floor is the complement. ⚠️ Note `SCRIM_DEFAULT = 185` is itself *below* it, so on `light` every
unset launch is lifted to the floor — the floor working, not a bug, since the default was chosen
against a dark page. 📌 **`--help` used to quote `SCRIM_FLOOR` as "the" floor, and the note
here said that would become a lie the day a picker could choose `light`. That day was
`organon console theme`, so it is fixed rather than merely predicted**: the scrim line quotes
both floors and names which palettes carry the light one — derived by filtering `Theme::NAMES`
on `scrim_floor`, so a fifth palette with a light page is covered without an edit.

#### egui's own chrome — `Theme::visuals()`

🚨 **`console_main.rs` called `egui_ctx.set_visuals(egui::Visuals::dark())` once, hardcoded, and
that one call coloured the sliders, popup frames, `TextEdit` selection wash and scrollbars.** A
palette assembled only from `Theme`'s fields left roughly half the window dark-hardcoded —
survivable for three dark palettes, fatal for a light one, which would have read as broken
rather than as light. `Theme::visuals()` derives `Visuals` from the palette; `console_main.rs`
calls `set_visuals(theme.visuals())`.

**`ChromeSource` has three answers, not two.** `DerivedLight` and `DerivedDark` choose which
egui base the derivation starts from; `EguiDark` means *not derived at all*, and it is
`organon`'s. 🚨 **`Theme::organon().visuals()` returns `Visuals::dark()` byte-for-byte**, so
adding three palettes cannot silently restyle the console that already ships;
`organon_chrome_is_still_egui_dark_to_the_byte` is what catches anyone tidying the special case
away, and `a_derived_chrome_carries_the_palette_rather_than_eguis` catches the opposite failure
— a `visuals()` that returns its base unchanged would pass the `organon` pin and leave `light`
in egui's dark chrome.

**The mapping**, all of it from existing fields: `panel_fill` ← `term_bg` (the page);
`extreme_bg_color` ← `composer_fill` (with `text_edit_bg_color` left `None`, i.e. "follow
`extreme_bg_color`", so the composer's plate is stated once); `code_bg_color` ←
`surface_empty`; `faint_bg_color` ← `strip_fill`; `window_fill` ← `tab_menu_fill` with
`window_stroke` ← `model_edge`; `hyperlink_color` ← `asking`; `warn_fg_color` ← `mode_alert`;
`error_fg_color` ← `bad`; and the five widget states ride the console's own surface ladder
(untouched → the status band's weight, hovered → the model plate, active → that plate's edge).
The **selection wash is `timeline_bubble_user`** — the palette already had to answer "the accent
tinted far enough into a surface that text stays readable on it", which is exactly what a
selection asks.

⚠️ **Colour only.** Corner radii, widget expansion, stroke *widths* and shadows all come from
the egui base and are left alone — `bg_stroke`/`fg_stroke` have their `.color` assigned rather
than being replaced, so each keeps its width. Form is a separate axis with a separate owner, and
`the_chrome_derivation_never_touches_form` fails if a colour change ever grows a geometry change
beside it.

### 1.5 Preferences — the console's first thing that remembers what a person chose

`prefs.rs` writes `preferences.json` at the store root, beside `harnesses.json`. It is a
`Preferences` struct — today one field, `theme: Option<String>` — loaded with
`Preferences::load_default()` and written with `Preferences::save_default()`.

**Until this landed, the console persisted nothing a user chose.** Measured 2026-08-13 by
reading the crate: the only writer was `SessionLog` (`session.rs`), and an append-only event
corpus is evidence of what happened, not a statement of what is wanted. The only
user-*configuration* path was a **read with no matching write** — `harness::load` over
`harnesses.json` — and every other knob was an `ORGANON_SHELL_*` variable sampled once at
startup. A picker could therefore offer a choice and lose it at exit, which is what makes a
picker pointless. The colour theme is the immediate consumer and the reason this exists.

**The store root is `SessionLog::store_root()`, called — not re-derived.** The one-resolver
rule in `organon-console/Cargo.toml` is usually read as "always use `dirs`", and
`dirs::data_dir().join("OrganonShell")` here would satisfy that letter while still being
wrong: two resolvers that *can* disagree eventually do, and the failure is a preferences file
written beside a `harnesses.json` the console reads from somewhere else.

**The failure posture is `harnesses.json`'s, exactly.** Missing, unreadable, or malformed ⇒
"no stored preferences" — never an error, never a crash. A corrupt preferences file must cost
you your preferences and never your ability to open the console. Growth is additive:
container-level `#[serde(default)]` loads an older file missing a newer key, and serde's
unknown-field tolerance means a newer file does not break an older binary, so adding a
preference is one field rather than a migration.

⚠️ **A write must not be able to destroy what is already stored.** `save` writes a temp file
in the *same directory* and renames it over the target — same directory because a rename is
only atomic within one volume and `%TEMP%` is routinely on another, and `std::fs::rename`
replaces an existing target on Windows too (it is `MoveFileExW` with
`MOVEFILE_REPLACE_EXISTING`, not the bare `MoveFileW` that fails when the target exists). The
reason this is worth doing in version one rather than later: a half-written file would fail to
parse, and the total read posture above would then *silently reset every preference the user
had*. A stranded temp is cleaned up on a failed rename, and a test pins that the store holds
one file rather than a litter.

⚠️ **Never written with a UTF-8 BOM.** `serde_json` refuses a BOM outright, and the total read
posture turns that refusal into silence — a file that is present, looks right in an editor, and
does nothing. That exact failure is on record on this machine for `harnesses.json`, from a
PowerShell `Set-Content -Encoding utf8`. Both halves are pinned: what we write has no BOM, and
a BOM'd file reads as defaults rather than panicking.

📌 **AMENDED (#38): `ORGANON_SHELL_THEME` does override the stored palette — for one launch,
out loud, and without writing.**

The decision this replaces read *"no environment variable overrides a stored preference, and
that is a decision, not an omission"*, and its reasoning still stands where it applies: an
override baked into a launch shim wins **silently**, which is indistinguishable from the
evaporation this file exists to end, and `organon-console.cmd` already demonstrates the failure
by setting `ORGANON_SHELL_TABS` unconditionally so the documented way to override it is ignored
without a word. That paragraph also named its own escape hatch — *"a one-launch override, if
ever wanted, belongs in a CLI flag that can say so in the console's own output"*.

**What changed is that the console can now be told a palette at all, and the objection turns
out to be to the silence rather than to the precedence.** The escape hatch as written cannot be
taken: `organon-console` has no flags. It is launched by shims, and an environment variable
*is* its argument surface — `--help` documents variables for exactly that reason. So the
override is granted and the original objection is answered head-on, in two properties that are
pinned by tests in `theme.rs` rather than left as intent:

1. **It announces itself, every launch**, naming the variable, the palette it chose and the
   stored palette it is standing in front of. `Selection::notes`, printed by `Console::new` on
   the console's own stderr.
2. **It never writes.** Only `organon console theme` stores anything, so an override cannot
   destroy the choice underneath it — unset the variable and the stored palette is back.

Together those make it a **loan, not a takeover**: it can only win while it is set, and it
cannot win quietly. ⚠️ **This licenses `ORGANON_SHELL_THEME` and nothing else.** The rest of
the `ORGANON_SHELL_*` family still overrides nothing, because none of it has a stored
counterpart to override; the day one does, it inherits both properties above or it does not get
the precedence.

**`theme` is read at startup, and written by `organon console theme`.** `Console::new` calls
`Preferences::load_default()` and hands `theme.as_deref()` to `theme::select`; `Console::set_theme`
loads, sets the field, and calls `save_default()`. ⚠️ **Load-modify-save, never
`Preferences { theme }`** — this struct grows by adding fields, and constructing a fresh one at
a write site silently discards every preference that site does not know about, which is a bug
that arrives later and looks like somebody else's. The field stays a plain `String` name so an
unresolvable value (a palette written by a newer console) costs the *preference* and not the
console: `select` reports it, names the known set, and falls through to the default.

### 1.6 Posture — how the console holds itself, on an axis of its own

**`organon-console/src/posture.rs` holds two types: `Posture`, a scalar `t ∈ [0,1]`, and
`Form`, the fourteen form tokens resolved at that `t`.** `Posture::TERMINAL` (`t = 0`) is the
console exactly as it has always drawn; `Posture::DESKTOP` (`t = 1`) is James's desktop form —
the transcript **centred, with a 90-point margin down each side**, roomier, ruled down the
left instead of boxed, with registration ticks at the corners of the conversation area.
**Every console still opens at `Posture::TERMINAL`, and `organon console posture <word>` is
what moves it.** ⚠️ The desktop end has now been seen **once**, and that sighting is why the
margin is symmetric rather than a left gutter — but nothing has been drawn at any intermediate
`t`; see the ledger for exactly what was seen and what has not been.

#### Reaching it: `organon console posture <terminal|desktop|0.0-1.0>`

`Posture::resolve` takes either end's word **or a bare scalar**, and the scalar is accepted
because a `Posture` *is* a scalar: refusing `0.5` would mean the CLI could not say a thing the
type represents and `Form::at` draws. ⚠️ **Refused, never clamped** — a typed `90` is degrees
where the axis wanted a fraction, and answering `desktop` would let the mistake look like it
worked. `Posture::new` still clamps, because it is fed by code where an overshoot really is a
rounding error; `from_scalar` is the brace on that belt, `CameraFraming::in_range`'s
arrangement exactly.

🚨 **It SNAPS, and that is a decision rather than a stage on the way to a tween.** The axis
exists so a later tier *can* animate it — that is why every token is a scalar — but a tween
moves the transcript's wrap width continuously, and §1.7 prices a single width change at
~7.6 ms at 400 elements, with five options and no decision taken. A snap pays that cost **once**,
in a frame nobody perceives as a jump; an unconsidered tween pays it every frame of the motion
and reflows a wall of text under someone's eyes to do it. Tier C still owns the tween, its
`request_repaint` discipline and what a moving layout does to the scroll anchor.

⚠️ **It is NOT remembered, and this is the deliberate asymmetry with the palette.** §1.4's
palette is *what the console is made of* — a person who picks one means it, and it should be
there tomorrow, which is why it reaches `preferences.json`. A posture, at this tier, is *how it
stands right now*: a view you take to look at something. The desktop end has never been drawn
on a real screen (§3), so a stored `desktop` would mean every console from then on opens into
an unaudited layout, recoverable only by typing the verb back or editing JSON — while closing
the window is a free undo. **Revisit when the tween lands.** An animated posture somebody has
actually lived in *is* a preference, and it is one field on `Preferences` away.

⚠️ **Where the refusal happens is not where the other console verbs put it, and the reason is
the scalar.** Every other named argument on `organon console` is gated by clap's
`PossibleValuesParser`; this one cannot be, because `Choice` and `Float` are separate kinds and
neither says "one of these two words, or a number in this band". So `bin/ctl.rs` calls
`Posture::resolve` in `run_console` instead — a human still gets the good error before a byte
reaches the sidecar, at the cost of `<POSTURE>` not tab-completing. The console's dispatch
catalog (`console.posture`) takes the opposite trade and declares `ArgKind::Choice` over
`POSTURE_WORDS` **only**: the words are what a caller reaching for a posture means, the scalar
is for a hand exploring the axis, and a hand has a terminal.

🚨 **Posture is orthogonal to the palette, and that is the whole reason it is a second value
rather than more fields on `Theme`.** §1.4 answers *what the console is made of*; this
answers *how it stands*. `organon` at desktop posture and a light palette at terminal posture
are both real consoles, and neither is a variant of the other — a palette that also decided
the padding would make "the phosphor look, but roomier" unsayable, which is the exact request
this axis exists to answer.

**Why a scalar and not two modes: every form token is a scalar, and scalars lerp.** The
desktop state is not a second renderer and not a second set of draw calls; it is the same
draw code reading different numbers. A mode enum would have made every intermediate value
unrepresentable and every draw site a branch. `Form::at(t)` interpolates componentwise, and
Tier C's animation is therefore a change to *one field on `Console`* rather than a rewrite of
the drawing.

**One owner, no globals** — `Console` holds `posture: Posture` beside its `Theme` and
`theme_name`, `redraw`
resolves `let form = &self.posture.form()` **once per frame**, and `&Form` is passed down
beside `&Theme`. No `static`, no `thread_local!`, no `OnceCell`. Resolving per draw call
would be cheap and wrong for a reason that outlives this tier: two calls in one frame could
disagree, which is precisely the tearing a tween would make visible.

**The tokens, both ends, and where the terminal value was read from.** Every terminal value
was read out of the code as it stood on `main` before a line of it moved, and
`form_at_terminal_is_the_form_that_shipped` pins each one with its source in the assertion
message — that test is the tier's entire safety net, because a wrong number compiles and
draws.

| Token | terminal | desktop | terminal value read from |
|---|---|---|---|
| `margin` | `0` | `90` **each side** | there is no margin today |
| `card_radius` | `6` | `8` | `CornerRadius::same(6)` at all five card frames |
| `nested_radius` | `4` | `6` | `CornerRadius::same(4)` — `surface_element`'s waiting plate |
| `card_pad_x` / `card_pad_y` | `8` / `8` | `18` / `18` | `Margin::same(8)`; `block_panel::PAD` is `8.0` |
| `human_pad_x` / `human_pad_y` | `10` / `6` | `18` / `18` | `Margin::symmetric(10, 6)` — the one asymmetric card |
| `line_height` | `1.0` | `1.6` | no `line_height` call exists, i.e. the font's own row |
| `card_gap` | `8` | `18` | `ui.add_space(8.0)` after each element in `scrollback` |
| `label_tracking` | `0` | `0.13` em | egui's default `extra_letter_spacing` |
| `card_border` | `1.0` | `0.0` | `Stroke::new(1.0f32, accent)`, at full alpha |
| `left_rule` | `0.0` | `1.0` | there is no left rule today |
| `tick` | `0.0` | `1.0` | there are no ticks today |
| `tick_len` | `8` | `8` | James's spec; the same at both ends on purpose |

🚨 **`card_radius`'s terminal end is `6`, and the specification says `0` (square). They cannot
both be honoured, and this is the one place the design and "change nothing on screen"
disagree.** The console's cards have had `CornerRadius::same(6)` since they were written, so
a terminal end of `0` would square every card in the flow the moment this tier landed — a
visible change, at the posture that is supposed to be today's console, verifiable only by
somebody looking at a window, which this tier explicitly did not have. It is therefore
resolved in favour of the no-change constraint and **recorded rather than silently
reconciled**: the intended terminal end is square, the shipped one is `6`, and flipping it is
one number in `Form::TERMINAL` plus one in
`form_at_terminal_is_the_form_that_shipped`. Whoever flips it should expect the flow to look
different at `t = 0` and should be prepared to say so.

Two desktop numbers are **derived rather than given**, and both say so in `Form::DESKTOP`'s
doc: `card_gap` takes the padding's number because the spec said "roomier" and no figure, so
the space between two cards matches the space inside one; and `nested_radius`'s `6` keeps the
terminal end's two-point step below `card_radius`, making the two ends a translation rather
than a re-proportioning.

🚨 **`margin` is symmetric, and it began as a left-only `gutter` — this is the one token the
first cut got wrong, and it took a window to see it.** The tier was built from a written
specification that said "add a narrow empty left margin column, about 90px wide", and it was
implemented exactly as written: `Margin { left: 90, right: 0, .. }`. **That sentence was
written to prompt an image generator into restyling a screenshot** — it was describing a
picture, not specifying a layout, and nobody noticed the difference because nobody had drawn
it. Rendered, it reads as a window shoved off its own left edge rather than as a centred
document. What was actually wanted is Claude Desktop's shape: content centred, ~90 points
clear on both sides. The token is now `margin` and `Form::content_margin` answers
`Margin::symmetric(margin, 0)`. ⚠️ **Renaming it was not cosmetic**: a field called `gutter`
that produces a symmetric inset is a lie a reader has no way to catch, and the *next* person
to add a token would have copied its shape.

⚠️ **Note what the tests did not catch, because it generalises.** Every posture test passed
the whole time. They all asserted the **scalar** — `f.gutter == 90.0` — and not one of them
asserted the `Margin` that scalar became, which is where the asymmetry lived. The token was
right and its spelling was wrong.
`the_content_margin_is_symmetric_at_every_posture` now walks the axis and checks the shape at
each step (`left == right`, no vertical inset, the value is the token), which is the assertion
that would have failed on day one.

⚠️ **A margin, not a measure — and the difference matters on a wide window.** This is an
*inset*: the text column is whatever the pane has left, so at 1100 points (the console's
default) the measure is 920, and at 2500 points it is 2320 — long prose at that width is
genuinely hard to read, and Claude Desktop does not do this. Claude Desktop caps the **measure**
and lets the margins absorb the rest. The cap was considered and deliberately not taken, for
three reasons, the third of which is the one worth recording:

* **James asked for 90 on both sides**, at the window he was looking at. A 720-point measure
  would have given him 190-point margins on that same window — visibly *not* the number he
  named, on the first thing he looked at.
* **This pane is not a prose document.** It holds diffs, tool cards with JSON, and rendered
  surfaces, all of which want width. A cap sized for paragraphs shrinks those too.
* 🚨 **A measure cap cannot be one more scalar lerping between the two ends, because
  "uncapped" is not a width.** Every combination was worked through: lerping a `measure` token
  0 → 720 makes the *midpoint* narrower than the desktop end (a non-monotone tween, i.e. the
  content pinches and reopens); starting it at a large finite "effectively infinite" number
  makes the terminal end depend on the window size, which breaks the no-change guarantee on a
  wide enough one; and a proportional margin (a fraction of available width) preserves the
  terminal end but does not cap anything. The honest formulation is
  `inset = t · max(margin, (available − measure) / 2)`, which needs `t` itself — so it needs
  either a fourth presence token redundant with `margin`, or `t` stored on `Form`, plus
  `content_margin` taking the available width. That is a real design, and it is a bigger one
  than it looks. **It should be taken when somebody has looked at the desktop posture on a
  wide window**, which nobody yet has.

🚨 **The card-edge decision: posture owns the scalar, the PALETTE owns whether the edge is
visible.** The four-sided border fades out and a left rule fades in over **one shared lerp**
(`card_border + left_rule == 1` at every `t`, pinned by test), and no draw site branches on a
theme. A palette that separates surfaces by fill alone gives `Theme::card_left_rule` **zero
alpha** and gets nothing; one that wants a hairline gives it a real colour. `organon` takes
the first answer, which is what keeps this tier invisible — its cards are four-sided boxes,
so there was no rule to preserve. **The rejected alternative was a `Box | LeftRule | None`
enum per theme**: it puts a branch in every card draw, and it makes the tween *discontinuous*
exactly where the enum flips, which an alpha cannot be.

⚠️ **The border carries a tool card's state, and the rule does not.** At desktop posture the
accent-coloured box is gone, so a card's running/ok/error reading no longer comes off its
edge — which is why the state *word* beside the tool's name is not optional and must never
become a colour alone. Whether the rule should instead be drawn in the card's own accent is a
real question and a one-line change; it needs somebody who can look at a window, so it is
named here rather than guessed at.

⚠️ **`card_stroke` answers `Option<Stroke>`, not a transparent stroke, and `content_margin` and
`body_line_height` answer `Option` for the same reason.** `Frame` reserves a stroke's width
whether or not it can be seen, so a zero-alpha border would leave a point of invisible inset
around every card; and at the terminal end the scrollback's walk runs directly in the scroll
area's own `Ui` with the text laid out by the font, rather than inside a zero-margin wrapper
with an explicit line height that *ought* to equal the one egui would have computed. Those
`None`s are the no-change guarantee, not an optimisation — identical by construction instead
of identical by arithmetic.

🚨 **Two tokens genuinely do not interpolate, and they are LEFT OUT rather than faked.**
**Font family** (mono ↔ sans) and **label case** (`command:` ↔ `COMMAND:`) are the two form
decisions in the design that are not scalars: there is no half-mono face and no half-capital
letter. They are **not fields on `Form` at all** — not a threshold, not a snap — and the
console's font choices and label spellings are exactly what they were. The alternative
considered was snapping both at some `t`; it is cheap to write and wrong twice over, because
a snap inside a tween reads as a lurch precisely when the tween is meant to read as one
motion (Tier C's problem, created here), and it puts a *threshold constant* in the same struct
as thirteen honest continuous values, so a reader can no longer tell by looking which fields
mean what they say. Nothing draws a sans face or a capitalised label today, so leaving them
out costs nothing and leaves the decision with whoever can look at a window. ⚠️ Fonts are not
a blocker when that day comes and need no new asset: `organon-console` installs no
`FontDefinitions`, so it inherits egui's built-ins, which carry a proportional sans beside
the mono.

**What posture does not govern.** The **terminal host** (`term_view`): a character grid's
form is the font's — cell size, baseline and wrap are all consequences of the glyph metrics,
and there is no padding, corner or gap in it for a scalar to move. Within the conversation
view, the **tab strip, the composer plate, the model plates and the status band** keep their
own constants: they are chrome, not cards, and a tier that wants them to breathe should say
so out loud rather than inherit it from a token named "card". And the **margins' contents**:
this tier claims the two 90-point columns and leaves both empty, because the reflow is the
part that can be wrong and is worth seeing on its own before Tier D draws turn ordinals into
the left one. And, since §1.12, **whether the window covers the display**: that is a third
orthogonal axis, not a third value on this one, and this axis could not have expressed it
anyway — it has no slots to add to. §1.12 owns the argument, including why a full-screen
window's extra width is deliberately not allowed to feed back into a `Form`.

⚠️ **`Posture::new` and `Form::at` clamp, and NaN resolves to the terminal end.** These
numbers reach `Margin`'s `i8` and `CornerRadius`'s `u8` through `as` casts, where a `NaN`
converts to `0` silently — one bad float would square every corner in the window and report
nothing. Resolving it where it enters is the only place it is still a number anyone could
notice.

### 1.7 The re-wrap measurement — what a width change costs the transcript

`conversation_view/rewrap_bench.rs` is an instrument rather than a feature, and it is in §1
because the *instrument and its finding* exist right now even though both things waiting on
them do not. **`doc/console_rewrap_measurement.md` is the document; this is the pointer.**

⚠️ **This section was written against a tree where posture did not exist**, and said so — the
measurement was taken on a branch off `main` while `posture.rs` was still unmerged. Posture
landed in the same integration, so §1.6 now describes it and this is no longer a stand-in for
a missing section. What has not changed is the part that matters: **the number was taken
before the tween was built rather than after** — the order the portal's "immersive is nearly
free" claim did not get, and the reason that claim had to be retracted.

⚠️ **The tween is still unbuilt, and this measurement is why `organon console posture` snaps.**
§1.6 opens at `t = 0.0` and the verb moves it in one step, so posture pays the animating column
**once per command** and never per frame — the single-change row below, priced at exactly one
frame. The console already pays the animating column on a window-resize drag, which is this
section's actual finding.

The one line: **egui's galley cache is keyed on the wrap width** (`epaint-0.33.3/src/text/fonts.rs:884`
→ `text_layout_types.rs:439`), so a width that moves by a whole point is a **total** miss across
the entire retained scrollback — and nothing culls, because `egui::Label::ui` builds its galley
*before* it tests `is_rect_visible`. Measured on this machine, release: **≈ 7 µs to lay out a
wrapped galley against ≈ 0.9 µs to reuse one — 6–9× per frame**, which is 9.1 ms per frame at a
400-element session and 308 ms at the 10 000-element cap. A single change (pane splitting, or
snapping a tween at its end) costs exactly one such frame and nothing after it.

📌 **The larger finding is in the steady column.** The transcript is not virtualised, so its
layout cost is linear in scrollback length *with nothing animating*: 8.1 ms per frame at 2 000
elements, 51.6 ms at the cap. Posture's tween does not create that — it multiplies it by eight
and makes it visible eight times sooner. Window-resize drags already pay the animating column
today, on `main`, with no posture and no panes.

Two tests hold the parts that can rot silently, both in the default suite: one pins epaint's
cache keying (a pinned-dependency contract, the `native/tests/egui_popup_contract.rs` argument),
and one pins that the whole scrollback is laid out rather than the visible slice — if a future
egui culls, every figure in the document is about the wrong thing and that test says so. The
benchmark itself is `#[ignore]`d; §8 of the document is the list of what it did **not** measure.

### 1.8 The command registry — one table, four front doors

✏️ **Four again.** #98 Tier C briefly made it five — a command line in every region, dispatching
this whole table — and **that was cut back to a two-word control**, which is not a front door onto
the vocabulary and is not counted as one. §1.14's Tier C subsection owns the reversal and the
argument for it. The panel column's control still *expands onto* `console.stack` and reaches the
same dispatch, so it is not a second vocabulary either; it simply is not a way of reaching the
table.

**The console's own verbs now have one vocabulary and several spellings, and the newest
spelling is a slash command a human types into the composer.** `organon-console/src/registry.rs`
is the table; `console_main.rs`'s `console_specs()` / `mcp_specs()` is what fills it.

🚨 **The measurement that forced this.** On 2026-08-13 James typed
`organon console posture desktop` into a conversation tab. The text was sent to the agent as a
message; the agent ran inference to decide what it meant; it made a tool-search call to *find*
the tool; it called the tool; and the console then raised an approval card asking James to
approve his own command. **About thirteen seconds and a chunk of context for a command he had
already decided on.**

Nothing in that chain was a bug. It is what the console's older architecture forced: it
composited *around* a harness it did not own — Claude Code in a PTY, Pi in WSL — and therefore
had no way to hear a human's intent except through that harness. That is exactly why the MCP
tools and the `organon console …` CLI exist, and both remain right for the callers they were
built for. What changed is that §1.1's front-end **owns the composer**, and nobody revisited
the assumption afterwards.

#### The four doors, and why none of them is redundant

| Door | Who is talking | Route |
|---|---|---|
| `organon console background slate` | a terminal, a script, a harness with no other way in | clap → `cli::ConsoleOp` → the sidecar |
| `mcp__organon__console_background` | an agent, on its own initiative | MCP tool → `ConsoleDispatch` → the sidecar |
| `/background slate` | **a human, in the composer** | `Registry::resolve` → `ConsoleDispatch` → the sidecar |
| `add surface` in a panel column | **a human, in the rectangle it acts on** — ✏️ **two words only**, not a door onto this table | `region_line` → expands onto `console.stack` → `Registry::resolve` → the same `ConsoleDispatch` (§1.14, Tier C) |
| a pie-menu wedge | a human, with a pointer | **not built** — the registry is shaped for it |

They already converged on `Console::apply_console(&ConsoleOp)`, which is what makes several
entry points safe in the first place. This tier makes the **vocabulary** converge too: the
slash surface is *generated* from the same `Vec<CommandSpec>` the MCP schemas are generated
from, so a verb cannot exist for an agent and not for the person in front of the console.
`every_console_verb_is_typeable_as_a_slash_command` and
`every_surface_of_a_verb_produces_the_same_console_op` (both in `console_main.rs`) are what hold
that to more than an intention — the second one asserts that `/camera reset distance 40`, the
agent's tool arguments, and the CLI's own `ConsoleOp` are one value.

📌 **The typed line, minus its slash, *is* the sidecar line.** `/camera reset distance 40` and
`camera reset distance 40` are the same words in the same order, because the slash grammar is
derived to match `cli::console_op_to_line`: **required arguments positional in declared order,
optional ones keyword-tagged, an optional `Bool` a bare flag.** So `queued: camera reset
distance 40` printed by the CLI tells a human exactly what to type in the composer, with no
translation table between them.

#### What a slash command costs, and what it still owes

🚨 **No message, no inference, no tool search, no approval card, no tokens.**
`ConversationPane::submit` resolves the line *before* the session is even looked up.

⚠️ **The approval model is untouched and still correct**, because it answers a different
question — *may this agent act on my behalf* — and a person's own keystroke was never that
question. That is why `Capabilities` now carries **two** dispatches: `dispatch`, which the MCP
server moves onto its serve thread behind the gate, and `local`, which the composer reaches
directly. Both are `ConsoleDispatch`, built in `console_main` from one type.

⚠️ **It is still audited.** The console lane applies nothing locally: it hands the validated
call to the same dispatch the agent's tools use, which writes the console's sidecar, which
`Console::drain_console` drains next frame through the real `CommandService` — leaving a
`CommandRun` record either way. A slash command skips the agent, not the discipline. It also
inherits the honest consequence: the receipt says **accepted**, not applied.

#### The hierarchy, and how a pie menu generates itself from it

An `Entry` is a **group**, a **verb** and its **arguments**, never a flat string. The dotted
catalog name is split once, here, on the **first** dot: `console.background` is group
`console`, verb `background`; `console.camera.read` is group `console`, verb `camera.read`
(typed `/camera.read`). Splitting on the last dot instead would invent a `console.camera` ring
with one wedge in it.

A radial menu is then three reads and no new table:

1. `Registry::groups()` → the root ring (`console`, `view`).
2. `Registry::verbs_in(group)` → the second ring, each wedge labelled `Entry::verb()` and
   described by `Entry::doc()`.
3. `Entry::args()` → the third. **An argument whose `ArgKind` is `Choice` already *is* a ring
   of wedges** — one per option, closed and validated, because those options were built from
   `substrate_materials`' own tables rather than restated. `Float` carries its band, so it is a
   dial. `Int`/`Text` have no closed value space and are the one case needing a typed field.

A wedge press builds the same `(name, args)` pair `Registry::resolve` builds from a typed line
and hands it to the same dispatch. **The menu is a second renderer of this table, never a
second table** — which is the whole reason the hierarchy is carried explicitly rather than left
implicit in a dotted string.

#### Two lanes, because two different things answer

`Lane::Console` verbs are handed down by `console_main` and act on the console. `Lane::View`
verbs are answered inside the conversation view and never leave it: `view.surface` (`/surface`,
unchanged in spelling and behaviour), `view.help`, `view.media`, `view.organon` and — since
#117 — `view.trace`, which decides whether *this pane* narrates itself (§1.9). They share the
registry because a human types them in one box and a menu should draw them in one tree; they are
marked because the code that runs them is not the same code.

⚠️ **The slash namespace is flat while the registry is not**, so two groups can collide on a
verb word. The first claimant wins and the loser is **reported** — into the pane's log, where a
human is — rather than being silently untypeable. That is `mcp::McpServer::name_collisions`'
discipline one layer out, and `every_console_verb_is_typeable_as_a_slash_command` pins that the
real table has no collision.

#### Recognising a command without swallowing a message

The predecessor of this was `conversation_view::local_command`: an exact match on the single
string `/surface`, forwarding everything else. Its stated reason was that the two mistakes are
wildly asymmetric — failing to recognise a command sends a slash-word to the agent, which is
merely odd, while over-recognising one **swallows a real message**, and the composer cleared
either way.

That reason survives; the instrument does not, because forwarding `/surfaces` to an agent is
its own silent failure — the console knows the verb does not exist and says nothing. **A
refusal is what makes both properties hold at once**: it names what would have worked, and the
caller does **not** clear the composer, so nothing a person typed can vanish. A refusal is
recoverable; a swallow is not.

The rules, in order:

1. A line that does not begin with `/` is a message. **This alone is what keeps a *mention*
   reaching the agent** — "what does `/surface` do?", "use `/theme` for that" — because a
   sentence about a command has words in front of it.
2. `//` is the escape: the rest goes to the agent with one slash removed.
3. A bare `/` names no verb, so there is nothing to refuse. A message, exactly as before.
4. Otherwise the first word must be a verb in the table and the rest must satisfy that verb's
   own schema. Anything else is a refusal naming the alternatives.

⚠️ **Every value is checked in `registry.rs` even though the dispatch checks it again**, and
that is not redundancy for its own sake: this is the only gate with a human in front of it, so
it is the only one whose message can name the alternatives *while the words are still in the
composer to be edited*. By the time `validate_args` sees them the line has been sent.

⚠️ **`/help` is generated from the table**, so a verb added to `console_specs()` documents
itself in the composer with no edit. It lands in the pane's log — which is drawn at the **head**
of the scrollback, so in a long conversation it is a scroll away. That is a real limitation and
it is the log's, not this tier's: the transcript has no element for "the console said this",
and inventing one is a change to the conversation model rather than to the command registry.

⚠️ **What is *not* generated is the CLI's clap surface.** `bin/ctl.rs` still declares its
subcommands by hand; what binds it to this table is that its `PossibleValuesParser` lists are
built from the **same word tables** (`substrate_materials::MATERIAL_NAMES`, `cli::PORTAL_WORDS`,
`kind::KIND_WORDS`) the `Choice` options are, and its own tests pin that. So the *values*
cannot drift and the *verb list* still can. Generating clap from `CommandSpec` is the remaining
quarter of "one vocabulary" and is not done.

### 1.9 The command panel — see your choices while you type, and see what happened after

**The precedent is NeoVim's `which-key`**: press a prefix, a panel shows every valid
continuation *with its description*; press another key and it narrows. It is fast because it
never asks you to remember — it shows you, and once you know, the showing costs nothing
because you are already past it. James's own framing, 2026-08-14: *"when I type slash, I want
to see something pop up… a pop-up full-width display that lists all my choices."*

`/` shows every verb. `/c` leaves `camera` and `camera.read` — type the next letter to
narrow. `/theme ` shows the palettes. **Values complete exactly like verbs**, which is what
makes the surface feel finished, and it is free: an `ArgKind::Choice` already *is* the list,
built from `Theme::NAMES` and `substrate_materials`' tables rather than restated.

#### The panel is one row. The list is a mode.

**The primary panel is a single full-width row of words**, and that is what a `/` opens:

```
[background] | rig | theme | posture | block | patch | portal | camera | camera.read | surface | help
```

James, 2026-08-14, having used the first version: *"when the commands pop up, it is not what
I had in mind, but I'm glad you did it this way because I think it is good for it to start
this way. This should be available as a verbose mode, But I want the primary mode to be more
compact and I want it to be simply a list of the available terms."* He wrote the row out
himself, pipes and all.

🚨 **The words are `Registry::candidates`' own and nothing restates them**, which is why the
row narrows as letters are typed for free and why it gained `block`, `camera.read` and `help`
— three verbs James's sketch omitted — without anybody deciding to add them. Curating that
list is exactly the second vocabulary §1.8 exists to prevent, reached from the friendliest
possible direction. The brackets mark the word Tab would take; a bracket rather than a colour
alone, because colour is a weak signal in a row of same-sized words and dies in a screenshot,
and rather than the verbose list's `>`, which reads as a bullet when there is only one row.
`conversation_view::compact_line` is that row as a plain string, so it can be read in a test
rather than looked at. ✏️ **`compact_join` was split out of it for #98 Tier C**: a panel column's
control builds its words from its own two-verb palette rather than from `Registry::candidates`,
and it gets
the same fitting rule, the same ` | ` separator and the same `+N` count by calling the same
function. Two producers of words, one row — a second joiner is exactly how the two would come to
disagree about what a hidden count looks like.

⚠️ **Where the row would list options and there are none, it reads the hint instead** —
`rows: a whole number` for `/block `, `distance: a number from 5 to 4000` for `/camera
distance `. `Palette::hint` was written for exactly the kinds with no closed value space
(`Float`, `Int`, `Text`) and the compact row is the first surface to draw it.

🚨 **A line Enter would run says so, because a blank row reads as a broken one.** James, on a
running build: *"slash surface shows no options."* `surface` takes no arguments, so there
genuinely are none — and the panel said nothing at all, which is indistinguishable from a
panel that has failed. The row now leads with **`Enter runs`** whenever `Palette::runnable`
holds: `Enter runs` alone for `/surface`, `Enter runs | [reset] | yaw | distance` for
`/camera `. ⚠️ It leads rather than trails because `compact_fit` drops from the tail, so last
would be the first thing a narrow pane hid. ⚠️ **Two spellings failed differently and both are
pinned**: `/surface` had a redundant one-item list the renderer dropped, leaving an empty row,
while `/surface ` had no candidates at all, so `Palette::is_empty` was true and there was no
panel to draw — that third term (`&& !runnable`) now lives in `is_empty` rather than at the
call site, exactly as `hint()` already did. The `None` path is untouched: a line that is not a
command line still opens nothing, so nothing pops up over prose. ⚠️ **One consequence worth
stating**: a panel being open is what gives the arrow keys to the panel (`arrow_owner`), so on
a runnable line with no candidates Up and Down now move a highlight that is not there instead
of the caret — which is already what they do on a hint-only line like `/block `, and on a
one-line command there is no other row for the caret to reach.

⚠️ **The row counts what it could not fit rather than truncating it** (`… | +9`).
`compact_fit` measures in **characters**, which is exact because the row is drawn entirely in
the mono face, and it is a count rather than an ellipsis because egui's own truncation
appends `…` — U+2026, in none of its four bundled fonts, the very defect the glyph allowlist
exists to catch.

**The verbose list is the old panel, whole**, behind `ORGANON_PALETTE_VERBOSE=1`, read once at
tab construction the same way `ORGANON_PALETTE_AUTORUN` is. ⚠️ **An env var rather than a
key.** James asked for the list to *"be available as a verbose mode"* and said nothing about
how to reach it; a keybinding invented on his behalf is a standing claim on a key in a box
that is also where he talks to an agent. Which key it eventually gets is his.

#### A lone candidate completes itself

🚨 **One continuation left is not a choice, it is an answer already given.** James:
*"when I type slash p [Tab] d so that it narrows down to just one choice, 'desktop', Do not
show me the single choice like you currently do. Simply complete the completion because it's
the only option."* `Palette::sole_completion` is that rule, and `palette_complete` is the
loop; the panel additionally declines to draw a one-item list whose candidate is already the
whole line, which is the same statement seen from the drawing side —
`conversation_view::drawn_palette`, one pure function so the row a test reads and the row a
human sees cannot be two derivations. ⚠️ Dropping the redundant *word* is not the same as
dropping the *row*: `/surface` is this case and also a whole command, so what survives is the
`Enter runs` marker above.

🚨 **Completing is not running, and they have separate switches.** Completion is **on by
default** and only ever rewrites the composer — a line in the box is not an action.
`Palette::autorun` **submits**, and additionally requires `Candidate::completes` **and**
`Candidate::fires`. That a completion may hand autorun a line it then runs is a *chain*, not a
merge: `/su` completes to `/surface` and stops there, because a surface is not something
autorun may fire. Both rules are pinned in
`registry.rs` and again through real frames in `conversation_view.rs`.

⚠️ **`completion != line` is what makes it terminate**, not the loop bound. `/surface` is
already its own sole completion, so a rule that counted candidates alone would rewrite the
line to itself on every frame for ever. The bound (`PALETTE_COMPLETE_STEPS`, four) is there
for a cycle the registry has no way to produce today and a future `Choice` table has no way
to be trusted not to.

⚠️ **`/camera` must not complete**, because `camera` and `camera.read` both match it — a
prefix that is also a whole verb is two candidates, and a count is the whole trigger.

⚠️ **What this buys beyond the keystroke, and it is the larger half.** `candidates` reads a
line with no trailing whitespace as *"still typing this word"*, so `/portal` put `portal` in
the **verb** slot and offered the verb back; only `/portal ` reached the value slot.
A command whose arguments are its entire point therefore appeared to offer no argument
completions at all, which is what James reported. `verb_candidate` gives a verb-with-arguments
a trailing space in its completion, so taking the lone candidate is what opens the ring:
typing exactly `/portal` now leaves `/portal ` in the box with `[open] | close | toggle`
above it.

⚠️ **Escape suppresses it, for free and correctly.** `ConversationPane::palette` answers
`None` while the panel is dismissed, so a human who has shut the panel is not having their
line rewritten behind it.

🚨 **THE RULE: complete on insertion, NEVER on deletion — and a reader who does not know it
will reintroduce the worst defect this panel has had.** James, on a running build:
*"once I have typed slash surface, I am no longer able to backspace out of it."* Deleting
from `/surface` leaves `/surfac`, whose only candidate is still `surface`, whose completion
is `/surface` — so the deletion was undone on the frame it happened. Every verb with a unique
prefix was a trap (`/background`, `/rig`, `/theme`, `/posture`, `/help`) and so was every
value once its prefix was unique, and select-all-and-retype was the only way out of a typo.
⚠️ It was worse than an undo: accepting rewrites the whole line and puts the caret at its end,
so the characters that *did* come out came from the middle of the word — measured through real
frames, eight backspaces on `/surface` gave `/surface`, `/surfae`, `/surface`, `/surfce`,
`/surfc`, `/surface`, `/surace`, `/surac`.

`conversation_view::completion_held` is the rule and it is a **latch, not a per-frame test**.
The frame *after* a backspace is a frame in which nothing changed at all, so refusing only
*shrinking* frames would re-complete on that next one — the same bug at 60 Hz, presenting as a
flicker rather than as a line that will not shorten, and invisible to any single-frame test.
A deletion therefore holds completion off until an **insertion** lets it go. It reads the
shadow copy `notice_edit` already keeps (`composer_seen`, the line at the start of the frame)
against the line the `TextEdit` has just written — one source of truth, no second observer.
⚠️ **`Palette::autorun` obeys the same latch**, where the stake is higher — and now that it is
on by default the stake is real rather than conditional: backspacing `/theme dark` to
`/theme dar` leaves one candidate that completes *and* is recoverable, so without the latch the
keystroke trying to erase the command would execute it.

⚠️ **What the rule measures is the line's length in bytes**, which answers *"did this frame
add text"* and nothing finer. Three cases are therefore classified by their effect rather than
their intent, each stated rather than defended: a **paste that replaces a long line with a
short one** reads as a deletion; **select-all then type one character** reads as a deletion;
a **same-length replacement** leaves the latch as it was. None can get stuck — the next
inserted character releases it, so the cost is bounded at one keystroke — and a composer set
wholesale (a test, a history recall) arrives unchanged within its frame and completes as
before.

🚨 **The caret moves on the same frame as the rewrite, and it did not always.** This used to
be a one-frame window recorded here as a known price: typing `/`, then `h`, completed the line
to `/help` — and the next character produced **`/hxelp`**. `ConversationPane::want_caret` was
drained by `composer_box`, which runs *before* the completion does, so the box could only ever
honour the *previous* frame's request, and by the time it ran, that frame's keystroke had
already been placed at the stale index after `/h`. The window was one frame — ~16 ms at 60 fps,
inside a fast burst.

**It is closed by an ordering, not by a second flag.** `want_caret` is drained at the **end**
of `conversation_view::composer`, after `palette_complete` and `palette_autorun` have both had
their say, and `put_caret_at_end` writes egui's cursor state there. Writing it *after* the
widget has stored its own is what makes it stick: the next frame's `TextEdit` loads a caret
already at the end, so the next character appends. ⚠️ The earlier note here said closing the
window would mean setting the cursor *before* the widget runs, and would entangle the box with
the registry — **both halves of that were wrong**. Before is the one place it cannot go, since
the widget overwrites it; and `composer_box` only has to hand back its `egui::Id`, which is the
one fact about the widget its caller cannot derive. The box still knows nothing about
completions.

⚠️ **One flag serves four rewrite sites on either side of the box** — the arrows' history walk
and Tab's accept before it, self-completion and autorun's accept after it — precisely *because*
the drain is last. `want_caret` therefore never survives a frame; a request that outlived its
frame is exactly what `/hxelp` was.

#### The candidate model, and the three renderers of it

`registry.rs::Registry::candidates(line) -> Option<Palette>` is a **pure function returning
structured values** — no egui, no formatted rows, testable headless. A `Candidate` is:

| Field | What it is |
|---|---|
| `label` | the word — `theme`, `chocolate`, `distance` |
| `doc` | one line, off the table. Empty for a `Choice` option, which stands for itself |
| `completion` | 🚨 **the whole composer line accepting it would produce**, never the fragment |
| `kind` | `Verb { group, lane }` / `Keyword` / `Value` |
| `completes` | whether that line is a complete, valid command — asked of `resolve`, so it cannot drift from what Enter does |

🚨 **`completion` being the whole line is what makes one generator serve every renderer.**
Accepting is `line = candidate.completion`; asking `candidates` again with it yields the next
ring. That two-step loop is the entirety of what a renderer implements, and it is the same
loop whether the accept came from Tab, from a wedge, or from a click.

**Three surfaces draw this list and there is one generator**: the panel above the composer;
the **pie menu**, whose three rings are `groups()` → `verbs_in()` → an argument's `Choice`
(§2, still unbuilt); and `/help`. A renderer that needed its own generator would be a second
vocabulary, which is the failure §1.8 exists to prevent, reached from the other end.

The `Palette` around them carries `slot` — which word is being narrowed — `typed`,
`candidates`, and `runnable` (the line **as it stands** already resolves). ⚠️ `Slot::Value`
carries the whole `ArgSpec`, not a list of options, because the arguments with *no* closed
value space are precisely the ones a renderer must treat differently: `Float` is a dial with
its band already stated, `Int` and `Text` need a typed field. `Palette::hint()` is the
sentence for a human; the `ArgKind` on the slot is the fact for anything else.

#### Prefix, not fuzzy

Matching is a **case-insensitive prefix**, in table order. Subsequence matching (`/pst` →
`posture`) is faster on a long list, and this list is nine verbs long, so that speed is not on
offer. What it would buy instead is the ability for a line that reads like a typo to match a
distant verb — and with auto-execute available, a surprising *match* becomes a surprising
*action*. Prefix is also what makes "press another key and it narrows" literally true, which
is the property being copied. **Fuzzy is not reachable and is not built**; `registry::narrows`
is the one function that would have to change.

#### Tab completes, Enter runs, and they are never the same key

The composer is also where a human talks to the agent, so the send key must mean one thing
always. **Tab accepts** the highlighted candidate and cannot send anything at all; **Enter
submits the line as it stands** and never accepts.

⚠️ **Enter with exactly one candidate left is deliberately not an accept.** `/theme` names one
verb and is *not* runnable, so an Enter that accepted would have to either run an incomplete
command or silently rewrite the line and wait for a second Enter — one key doing two different
things one keystroke apart. Instead Enter reaches `Registry::resolve`, which refuses it by
name (*"`/theme` needs `name`"*) and **does not clear the composer**, so the words are still
there and Tab is one key away. That is §1.8's rule unchanged, and it is what makes "Enter
never accepts" affordable.

Arrows move the highlight, wrapping — but see the history below, which is the other claimant
on that key.

🚨 **Escape's dismissal is a fact about an EDIT, and getting that wrong shipped a bug.** It
was the composer's *text* at the moment Escape was pressed, compared for equality on every
frame — and content equality cannot express "has changed since", because a line becomes equal
to a dismissed string again by ordinary retyping. Press Escape once at `/p` and every future
`/p` was silently refused a panel for the life of the tab, with nothing on screen to explain
it; James hit exactly that (*"Now my tab completion broke. When I type slash p, nothing comes
up"*). `ConversationPane::notice_edit` now watches the composer change against a shadow copy
of the previous frame's text, once per frame, before anything asks whether the panel is open.
⚠️ The rule lives there rather than in `palette()` deliberately: `palette` is the *question*,
and a `&self` read that quietly rewrote state to answer itself would put the rule in the place
that is asked rather than the place that knows. ⚠️ The one case it does not catch is a line
replaced by an identical line *within a single frame* — select-all then paste the same text,
both landing on one pass. Every ordinary route to retyping a string passes through a frame in
which it is shorter.

#### Up walks the commands you have already sent

James asked for it in one line: *"Add a slash command scroll back buffer on the up key."* Up
recalls the previous slash command into the composer, Down comes back forward, and stepping
forward past the newest returns to the empty box the walk started in. **It does not wrap**,
where the panel's highlight does: a ring of eleven verbs has no end worth feeling, and a
history that silently rolled from the oldest to the newest would be indistinguishable from
having lost your place.

🚨 **Up already meant two other things, and the rule that picks one is a pure function** —
`conversation_view::arrow_owner`, three booleans in, one owner out — because the wrong pick
costs a message somebody was writing:

1. **A walk in progress keeps them.** Recalling `/theme dark` puts a command line in the box,
   which opens a panel; without this the second Up would move a highlight and the walk would
   be one step deep for ever. A walk ends by *editing* the recalled line, which is asked of
   the composer (`walking()`) rather than tracked, so there is no second flag to keep in step.
2. **An open panel takes them next**, unchanged.
3. **An empty box hands them to history**, because an empty text box has no caret motion to
   perform: Up there can only mean "what did I type before".
4. **Otherwise the text box keeps them.** Prose, a half-written paragraph, and a command line
   whose panel was dismissed with Escape are all this case. ⚠️ A multiline `TextEdit` gives Up
   a real meaning — moving the caret between lines — and taking it unconditionally would break
   ordinary typing in the box a human talks to an agent in, which is the same constraint that
   made Tab and Enter separate keys. Escape means "stop showing me this", not "hand my arrow
   keys to something else".

⚠️ **The raw key is carried alongside the act through the consumption pass**, because
`palette_key` maps **Shift+Tab** to `Prev` — the same act ArrowUp produces — so routing on the
act alone would hand Shift+Tab to the history.

**What earns a place**: `Resolved::Run` and `Resolved::Refused`, most recent first, no
consecutive duplicates. ⚠️ **A refusal is remembered and that is the case the buffer is most
for** — a command that ran is one you no longer need back, while one the registry refused is a
line with a typo in it you want in front of you again to fix. Prose is not remembered (he
asked for a *command* buffer, and a walk that stepped over three paragraphs would not be a
recall surface), and `Resolved::Escaped` is not a command at all. ⚠️ **In memory, for the life
of the tab**: the session log already records every command that ran, so a durable recall
surface would be a second record of the same fact and the two would disagree the first time
one was pruned. Reading back the session log is the honest way to make it survive a restart.

✏️ **All four keys are now conditional on one more thing: whether this composer owns the keyboard
at all** (#98 Tier C). `composer_keys` and `theme_edit_keys` both read from the raw event list, and
`arrow_owner` hands Up to the history unconditionally on an empty box — safe while the console had
exactly one command input, and not once a panel column has an input of its own. ✏️ **Narrower than
it was**: Tier C gave every non-`agent` region a command line, and only a `panel` region has one
now, so a console with no panel column never leaves the composer.
`ConversationPane::set_keys` is the gate, written each frame from
`region_line::Lines::composer_owns_keys`, which is a **measurement** of which widget had egui focus
last frame rather than a policy. It is `true` for every console that has not divided its pane, so
nothing above changes for the console this section describes. §1.14's Tier C subsection owns the
arbitration.

🚨 **`lock_focus(true)` on the composer is load-bearing, not a preference.** egui's focus
manager reads Tab out of the **raw input** in `Focus::begin_pass`, before any console code
runs, so consuming the event is too late to stop focus leaving for whatever button the
scrollback drew — and the keystrokes after it would go somewhere invisible. `lock_focus` sets
`EventFilter::tab`, which is the flag that pass tests. ⚠️ Visible consequence when the panel
is shut: Tab indents the message instead of moving focus, which is what a text box does
everywhere else.

⚠️ **Escape's hazard is real here but it is NOT the terminal's.** In a terminal tab Escape
belongs to the child (`vim` needs it) and must be consumed before `term_view` clones the event
vector; the conversation front-end has no child reading keys, so that hazard does not apply.
A different one does, one layer down: the same `begin_pass` **drops the focused widget** on
Escape, and `TextEdit` exposes no setter for `EventFilter::escape`. So Escape cannot be
prevented from blurring the composer — it is *repaired*, by re-requesting focus in the frame
the panel is dismissed. One frame passes with nothing focused, during which no keystroke can
arrive. All four keys are matched with `matches_exact`, never `matches_logically`, for the
shift-permissive reason `composer_key` already documents.

#### Auto-execute, and the two guards on it

James asked for it: *"it will just execute the thing as soon as it knows what we want"* — and
again on 2026-08-15, for the default: *"when we reach the end of a tab completion hierarchy …
it automatically executes and we don't press enter. I would limit this so that if there are any
things that would be irreversible or dangerous, it should not do that, but should instead
display a final completion that says something like press enter."*

🚨 **`Palette::autorun` fires on three terms, all of them provable.** (1) Exactly one
continuation remains, so there is nothing else the line could have meant. (2) That
continuation **completes** the command — `/t` leaves `theme`, which still needs a value, so it
does not fire; firing there would run a command while the hand is still typing its argument.
(3) The command it completes to is **recoverable**. Pinned by test in both `registry.rs` and
`conversation_view.rs`.

🚨 **THE RULE, and it is recoverability rather than severity: a verb may run without an Enter
when the console can be put back the way it was.** A setting has an inverse (another value of
the same verb) and a read changes nothing; both fire. **A verb that puts a new element into the
transcript does not** — the transcript only ever grows, and there is no verb that takes an
element back out of it. Nothing in this vocabulary formats a disk, so a severity scale would
have one rung and say nothing; what a hand needs protecting from here is the edit it cannot
undo.

| Runs unasked | Completes, then waits for Enter |
|---|---|
| `background`, `rig`, `theme`, `posture`, `screen`, `portal`, `camera`, `camera.read`, `help` | `block`, `patch`, `surface`, `organon` |

⚠️ **`help` is the one that looks like it belongs on the right and does not**, and the pair
`help`/`surface` is what the rule has to get right: both are view-lane, both take no arguments,
both are reached the same way. `/help` writes through `note` — the capped diagnostic log — and
reads a table; `/surface` calls `Transcript::push`. That is a difference in the code, not a
judgement call. A rule spelled "view-lane verbs are dangerous" or "argument-less verbs are
dangerous" would have got one of the two wrong.

🚨 **The declaration is `command::Reversal`, on `CommandSpec` and on `registry::Entry`** — one
per verb, in the place that verb is declared, never a list in a renderer (the house rule that
put roles on the spec). ⚠️ **It has no `Default`**, so a verb added later cannot answer by not
answering: adding a `CommandSpec` is a compile error until it says which it is, and the quiet
answer would have been the one that runs. `Candidate::fires` is derived from it in the same
`Registry::resolve` call that derives `Candidate::completes`, so neither can drift from what
Enter would actually do, and a name the table cannot find answers `false`.

📌 **The MCP catalog deliberately does not restate it.** An agent's tool call never reaches
this rule — the question at that door is *"may this agent act on my behalf"*, which
`start_approvals` answers with a real prompt per call, a stronger mechanism than an Enter key
rather than a weaker one. Emitting the flag as a tool annotation would be a second claim about
the same verb with nothing reading it. It lives on the shared spec so both doors can read one
fact when the approval model wants it.

**What the ask looks like: the `Enter runs` marker that already existed.** A verb on the right
of the table still *completes* — `/su` becomes `/surface` under the hand — and then the compact
row says `Enter runs`, because `Palette::runnable` holds. No second phrasing was invented for
this; the marker introduced for `/surface` showing an empty panel turned out to be exactly the
"final completion that says press enter" the request asks for.

🚨 **A command does not run on the frame its last character landed.** `palette_autorun` takes
`edited` — whether the composer changed on *this* frame, read before `palette_complete`
rewrites it — and refuses while it is true, so the earliest a fire can happen is the first
frame in which nothing was typed. Two reasons, the second the larger: the completed line is
**drawn at least once** before it disappears, and a keystroke arriving while a fire is pending
**cancels** it rather than racing it. ⚠️ **A settled frame
has to be made to happen** — egui repaints on input, so a deferred fire explicitly
`request_repaint`s; without that the command would run whenever something else next moved the
mouse, which is worse than either extreme. It is requested only when a fire is pending, never
unconditionally. ⚠️ A composer set **wholesale** (a test, a history recall) is settled already
by this definition: nothing was typed, so there is no hand to wait for.

✏️ **Correcting what pinning this once measured.** Typing `h` on `/` completes to `/help`, and
a character arriving on the very next frame used to land at the caret index the completion had
not yet moved — `/hxelp`, not `/helpx`. The wait was recorded here as not fixing that, which
was true and remains true: what it fixed was that `/hxelp` then ran nothing at all. **The
window itself is now closed** — the caret moves on the rewrite's own frame, so the character
lands as `/helpx` — and the two mechanisms stay separate on purpose. The wait is about *when a
command runs*; the drain is about *where the next character goes*. `a_command_waits_for_one_
frame_in_which_nothing_was_typed` reads `/helpx` today and still asserts the same thing it
always did about the receipt.

⚠️ **`Palette::autorun` still obeys `completion_held`**, where the stake is higher than a
rewritten line: backspacing `/theme dark` to `/theme dar` leaves one candidate that completes
*and* is recoverable, so without the latch the keystroke trying to erase the command would
execute it.

**On by default**, which is the substantive half of the request. `ORGANON_PALETTE_AUTORUN=0`
is the escape hatch and restores the Enter-for-everything console for a session, read once at
tab construction rather than per frame. ⚠️ **`=1` still means ON** — the variable's existing
spelling keeps its existing meaning, so nobody's shell profile quietly came to mean the
opposite of what they wrote. `conversation_view::autorun_enabled` is that rule as a pure
function of the value, so a test can pin it without writing to the process environment.

#### The panel only exists for a command line

🚨 **A panel that appeared while prose was being typed would be intolerable**, so the test is
`Registry::resolve`'s own and no other: the line must begin with `/`, and `//` is an escape
meaning the line is a message. A sentence *mentioning* a command has words in front of the
slash and answers `None`. ⚠️ A bare `/` answers `Some` with the whole table even though
`resolve` calls it a message — those are not in conflict: showing the choices is what `/` is
*for*, and nothing runs until the line is a command.

⚠️ **The verbose list is capped at eight rows with a count of the remainder, rather than
scrolled.** `console.background` offers more materials than fit, so it genuinely overflows —
but a vertical `ScrollArea` dropped into this bottom-up column takes the whole pane (684 pt of
a 684 pt pane, measured; see §1.1's composer). "Type another letter to narrow" is also the
faster route to the one you want. The compact row has no such cap: it is one row and it
counts what did not fit along the width.

#### The panel's bottom edge, and why it was on top of the text

🚨 **The panel painted over the composer, and the cause was `ui.horizontal`.** James: *"Line
the bottom of it up so it sits just atop the top line of the text box. Your current box
extends lower than that and covers a bit of the text."* `Ui::horizontal` seeds its child with
`spacing().interact_size.y` — 18 pt on egui's default style, on the assumption that a
horizontal row holds something interactive — and `allocate_ui_with_layout_dyn` then advances
by `frame_rect.union(final_child_rect)`, so a row of 15.125 pt text still costs the whole 18.
The band was arithmetic over *text* heights. **Measured at 2.875 pt of overflow per row**, by
putting `ui.horizontal` back and reading `plate`'s own return.

⚠️ **And the overflow goes downward, which is why it was visible rather than merely wrong.**
`plate` reserves its band in a bottom-up column but lays out top-down inside it, so rows that
outgrow the reservation are painted past its *lower* edge — over the composer, which was
placed there first, rather than pushing the scrollback up. Ten rows (a head, eight verbs and a
`+N` line, which is what a bare `/` drew against the real table) put ~29 pt of panel across
the top line of the text box.

`palette_row` allocates each row explicitly at `palette_row_height`, so the arithmetic and the
drawing are one statement. ⚠️ **Posture is in that height and it is not decoration**: `body`
applies `Form::body_line_height`, which at the desktop end is strictly greater than the text's
own height, so a band measured from `text_style_height` alone is short at every posture but
the terminal one. `plate` returns how far it outgrew its reservation — **zero by
construction** — and the test harness asserts that on *every* frame it runs rather than in one
test of its own, because the failure reappears whenever a row is added, a font changes or a
posture widens the line.

#### The same region is where a command answers

🚨 **The defect this closes.** A slash command's receipt goes to the pane's log, and the log
is drawn at the **head** of the scrollback — so in any conversation longer than a screen the
confirmation lands far above the live edge and is, in practice, invisible. James typed
`/posture desktop` on 2026-08-14, the console obeyed, and nothing he could see said so. §1.8
recorded that as a limitation on the grounds that the transcript has no "the console said
this" element and inventing one is a change to the conversation model. **The panel needs no
such element**: it is already full-width, already appears and disappears with the command
line, and is already where the eye is.

- ⚠️ **A receipt and a candidate list share one region and mean opposite things** — "here is
  what happened" against "here is what you may do" — so they are distinguished
  **structurally**: a receipt is a single band with a coloured word marker (`ok` / `refused`);
  candidates are a headed list with `>` on the highlighted row. Only ever one of the two.
- 🚨 **A refusal outlives a success**, which is `card_density`'s asymmetry one layer out: a
  confirmation nobody reads cost nothing, because the command ran; a refusal nobody reads
  cost the command *and* the knowledge that it did not happen. So a success ages out after
  eight seconds and a refusal never does. Both go the moment the line is edited, which is the
  honest signal that the human has moved on — and it is what hands the region back to the
  candidates. `receipt_holds` is that rule as a pure function.
- `registry::Receipt { ok, text }` is the structured value; `registry::receipt` formats the
  log's line **from it**, so the band and the log cannot come to disagree about what happened.
  The marker is the word `ok` rather than a glyph — 🚨 **and the log shipped with `✓` anyway,
  for four hours, photographed on a running console drawing `☐ /rig daylight` in the pane log
  and again in the status band.** The glyph allowlist guard existed and did not catch it: it
  walks an enumerated list of *draw sites*, and a string built in `registry.rs` and drawn in
  `conversation_view.rs` fell straight between them. That is the fourth time this exact defect
  has shipped and every earlier fix was site-local, so the guard now checks
  `registry::receipt`'s **output** from the file that draws it.

##### 🚨 …and the console is quiet about all of it by default — #117 Tier 1, `/trace`

James, 2026-08-20: *"I don't want to see any of the things presently visible in the status
panel. Or anything like that. My working model here is Claude Desktop. That's the level of
interactivity I want to default to in terms of showing your process as the agent or harness.
**Consider you are building this for me, not for some unknown user.**"*

Almost everything the pane printed above the first message was explaining the console to a
stranger — which directory the tab started in, that `/viewport center agent` was accepted, that
an empty transcript is empty. **The rule that replaced it, and the only rule:**

> **A refusal is always seen. An acceptance is seen only under `/trace on`.**

- `conversation_view::Remark { text, always }` carries it **per line**, and
  `Remark::seen(tracing)` is the one predicate — used by both surfaces that draw the log (the
  head of the scrollback and the status band's slot), because a band saying something the
  scrollback above it is hiding reads as a bug in whichever one you distrust.
- 🚨 **The default is deliberately the loud one.** `ConversationPane::note` keeps its signature
  and its meaning, so a line written by somebody who did not think about this is **seen**;
  `ConversationPane::trace` is the opt-in for the quiet half. A surface whose default is silence
  eventually swallows the one message that mattered — which is the defect this tree keeps
  finding, at every scale.
- **The test for the quiet half is not "is this routine".** It is whether the thing the line
  describes is visible some *other* way. A console command's acceptance is: the layout moves, the
  panel appears, the palette repaints. A refusal is not, and neither is `⚠ no .claude/, CLAUDE.md
  or .git here` — so `harness::cwd_notes` now answers a `CwdNote` per line and only the
  *resolution* is traced. **stderr keeps both either way**, which is what stops the quiet default
  from costing a diagnostic rather than a distraction.
- **A successful receipt band is held but not drawn.** `pane.receipt` is still set and still
  ages; only the drawing is gated, so `/trace on` mid-receipt shows one already in hand. The
  refusal band is untouched — `receipt_holds`' asymmetry above is extended by this, not replaced.
- **What else went quiet**: `no messages yet — type below and press Enter…` (an empty transcript
  is self-evidently empty and the composer below it has its own hint), and `/organon`'s
  *"Surface → the panel stack in `left`"*.

⚠️ **`/trace` is a *view-lane* verb, so it is per TAB, not per console.** Everything it un-hides
is one pane narrating itself; a console-lane spelling would mean a sidecar line and an MCP tool
for a preference about how loudly the console talks to the person in front of it. Two
conversations can therefore disagree about it, which is the honest consequence of that scope
rather than an oversight. `ORGANON_TRACE=1` opens every tab tracing, the same escape hatch
`ORGANON_PALETTE_VERBOSE` has.

⚠️ **`on` and `off`, no toggle** — `console.screen`'s rule: a verb meaning "the other one" cannot
be read back and answers differently depending on a state the person typing it may be wrong
about. Switching **on** echoes and switching **off** does not, which falls out of the rule rather
than being a second decision: the acknowledgement goes to the quiet half, whose visibility the
mode it just set decides.

⚠️ **One thing this cost, recorded because it is a vocabulary fact rather than a bug**: `/t` no
longer settles. `theme` and `trace` share the letter, so the completion cascade needs `/th`.

⚠️ **`/help` is now the third-best way to find a verb**, behind typing `/` and behind the pie
menu that will read the same table. Its body still lands at the head of the scrollback, which
is the limitation §1.8 named; this tier routes around it for *receipts* rather than fixing it
for *output*, because a twenty-line help text in a band above the composer is a different
thing from a one-line answer.

⚠️ **ASCII throughout the panel**, deliberately. The obvious characters — `▸`/`▾` for the
highlight, `…` for a `Float`'s band or for a row that overran its width — are in none of
egui's bundled fonts and would ship as boxes, which is exactly how `✓` reached a fourth draw
site. The glyph allowlist test in `conversation_view` walks every string the panel can draw,
**including the ones derived from a schema** and the compact row as a whole assembled line at
three widths, since a range, an option list or a separator is where a stray glyph hides.

##### 🚨 …and #120 finished the job: the instructional chrome, the harness telemetry, and a band that painted over itself

#117 removed the *receipts*. What it left behind was everything shaped like a **caption** —
text whose only reader is somebody who does not already know how this console works. James,
looking at the build the morning after and quoting the first thing on it:

> `message the agent — Enter sends, Shift+Enter for a new line`

and then, circling the block at the head of the transcript: *"We don't want this status
information showing here. … **it should not feel like part of the conversational flow.** …
**When everything is moving right, I generally don't care about this stuff unless there is some
exception or problem.**"*

**The rule, which is #117's own rule pointed at a wider class:**

> **Delete text whose only reader is a stranger. Keep text carrying a fact he cannot already
> have — a failure, a refusal, or a state no pixel states.**

**What went, and why each one is not a fact:**

| Was | Now | Because |
|---|---|---|
| `COMPOSER_HINT` — *"message the agent — Enter sends, Shift+Enter for a new line"* | nothing | An empty box under a conversation reads as *ready*. He knows Enter sends. |
| `COMPOSER_HINT_DEAD` — *"the agent is not running"* | `not running` | The fact survives; the sentence was a label wearing a verb. |
| `PALETTE_KEYS` — *"Tab completes - Enter runs"*, right of the command panel's head row | nothing | On screen for as long as the panel was, teaching two keystrokes to the person typing them. |
| `NOT_TRANSPLANTED` — *"this panel is named in Organon's editor but has not been transplanted into the console yet"* | `no controls yet` | Twenty-four of twenty-five panels are `Declared`; it was one sentence about the console's own construction, repeated two dozen times down a 320 pt column. |
| `— turn complete`, after every successful turn | nothing | Precisely *"showing your process as the agent or harness"*. The reply is on the page and the composer is live; both say it. |
| `approvals: handler withheld …, 14 of 14 console tools visible (47 offered)` | only when anomalous | See below — this one is a **security property** and could not simply go. |
| `session $1.18 · last turn 5.1s` | `/trace on` | Harness telemetry. Claude Desktop tells you which model you are talking to, not what the turn cost. |
| `◈ What are we working on?` | `/trace on` | The agent's own closing line, echoed a few pixels below the transcript that already has it. |
| `/theme edit - editing 'dark' live; nothing is stored until you save` | `/trace on` | The editor is on screen. An acceptance, under #117's existing rule, reaching a `note()` that should always have been a `trace()`. |
| *"the palette changed while the theme editor was open, so the editor closed"* | `/trace on` | An explanation of a normal outcome of a thing he just did. |
| `Surface → the panel stack in 'left'` | `/trace on` | This file already claimed it was quiet; the code still said `note`. Now it is true. |

⚠️ **Nothing is discarded — only undrawn.** Every line above still enters `pane.log` as a
`Remark`, still ages out of it at `LOG_LINES`, and still comes back whole under `/trace on`.
That is deliberate: James's *"I like the idea that there is a status log somehow, but it should
not be present normally"* is a surface nobody has built yet, and a change that dropped the
entries at the source would make building it harder rather than easier.

🚨 **The approvals audit is the one that could not simply be hidden, and the rule it earned is
worth keeping.** It reports §7's withholding guarantee — whether the permission handler is
absent from the model's own tool list — and a **silent** failure of that is the exact class of
defect this tree exists to prevent. It is now **loud when anomalous, quiet when the property
holds**, decided by `mcp::ExposureAudit::confirms_withholding` rather than by reading the
summary string:

- handler **on** the model's list → always seen (the console's authority is decorative);
- the session reported **no tools** → always seen (an audit that proved nothing is not a pass);
- served names the model cannot see → **not** an anomaly; that is ordinary deferred MCP loading,
  and folding it in would make every cold start read as a fault;
- otherwise → recorded, and drawn only under `/trace on`.

⚠️ **stderr keeps it unconditionally.** The screen is where a repeated confirmation costs
something; a launch log is where an audit trail lives, and a security property recorded only
when it fails is a record nobody can check afterwards. ⚠️ There were **two** render sites, not
one — the head of the scrollback and the status band's slot — and both are fed from the same
`Remark`, which is why one predicate fixed both. A string removed from one surface and left on
the other is the shape of bug a per-site fix produces.

🚨 **And the band had no width budget at all, which is a defect rather than a matter of taste.**
James saw `◈ What are we working on?ession $1.18 · last turn 5.1s` — the echo's tail painted
*under* the chips. The cause is egui's ordinary idiom used with an unbounded left-hand item:
the reading is added to the horizontal first and `Label::truncate` truncates to
`available_width`, which at that moment is *everything*; the right-aligned group added after it
is then handed a zero-width rect and lays out leftwards over what is already there. **Truncating
"to what is left" is only a bound when something has already been taken.**

`strip_right_reserve` measures the fixed right-hand items first — the ring (a Body-height square
allocated every frame, measured or not) and the chip run, laid out through the painter with
`CHIP_SEP`, now a constant because it is measured as well as drawn — and `reading_room` hands
the flexible reading the remainder, floored at nought. The reading is then drawn inside an
allocation of exactly that width, so `truncate` has something real to truncate against; it
shrinks to the text when the text is short, so an ordinary band does not move. The **log** is
deliberately not reserved for: it is the *other* flexible item and already truncates into its
slack. ⚠️ **This was fixed rather than hidden.** Both offending segments are now trace-only, so
the overlap would only have surfaced under `/trace on` — and would have come back the moment
anything was added to that band.

**How the three visibility rules are spelled**, each next to the thing it decides, none of them
re-derived at a draw site:

| Carrier | Decides | Predicate |
|---|---|---|
| `Remark { text, always }` | one line of the console's own log | `Remark::seen(tracing)` |
| `StatusReading { …, narration }` | the band's standing text | `StatusReading::seen_text(tracing)` |
| `Chip { text, narration }` | one dim right-hand chip | `StripContent::chips_seen(tracing)` |
| — | one transcript element | `element_seen(&body, tracing)` |

⚠️ **`narration` is set where the value is built, never inferred downstream.** Two of the seven
status readings are echoes — `needs_action` and `last_status_detail` — and both produce the same
`Standing` as live conditions do (`Asking`, `Ready`), so nothing downstream could tell them
apart; a caller re-deriving it from the *text* would be a second rule to keep in step. Same for
the chips: `2 remembered decisions` stays on the quiet band because it reports how far the
console has delegated its own authority, which is the mode marker's class rather than the
spend's.

⚠️ **`element_seen` returns `true` by falling through, not by listing the other variants** — a
new `Body` is visible until somebody decides otherwise, which is the safe default for a surface
whose failure mode is swallowing something. `RunEnd::detail` rides on the element it belongs to:
drawn on the `Error` and `Cancelled` arms where it carries a reason, gone with the success.

**Judged and kept, so the next sweep does not re-litigate them:** `region_line`'s `> add |
remove` (the vocabulary you would type, not a description of the control); `PALETTE_RUNS`
(*"Enter runs"* — it appears **only** where the panel would otherwise be blank, and a blank
panel reads as a broken one); `theme_edit`'s `EDIT_KEYS` and its two button hovers (a modal ring
driven by Tab/arrows/Esc with no on-screen affordance, present only while `/theme edit` is open
— and removing the row would mean changing the band's reserved row count, i.e. a relayout);
`/help`'s body (a direct answer to a typed request); the child's own `stdout:`/`stderr:` lines
(a line the *harness* printed is not the console explaining itself, and swallowing it is the
silence-on-failure defect); and *"the console is asking again — everything-for-this-session
revoked"* (a change in the console's permission authority, and the acknowledgement of a click
whose own doc argues for it).

📌 **`app.rs` is not touched and is not an oversight.** The Tier-1 skeleton carries the most
explanatory text in the crate — *"The agent workspace — conversation, plans, tool events, diffs,
artifacts, and live surfaces land here."*, `⌘K — command palette`, `The primary agent connects
in Console #7` — and `ConsoleApp` is constructed by nothing outside its own tests. It is not on
James's screen, so it is not chrome; it is dead UI, and deleting it is a different change.

🚨 **Nothing here has been seen on a screen.** Every claim is about how quiet something feels.
What is established is that the rules hold and the arithmetic is pinned — that the audit is loud
in both anomalous arms, that a failed turn keeps its caption, that the reading never asks for
width the fixed items need. Whether a band holding a model chip and a mode plate reads as calm
or as broken is James's to judge.

### 1.10 The window icon — the aperture mark, and the two defaults it replaces

The Console opened with the operating system's default icon. `native/src/console_icon.rs`
is the whole of the fix: two PNGs `include_bytes!`d into the binary, decoded once in
`ApplicationHandler::resumed`, handed to winit through `console_icon::apply`.

The artwork is `native/assets/chrome/aperture-mark-on-dark.svg` — two concentric rings and
a centre dot in warm gold on near-black, ticked at N/E/S/W. It is the **source**; the
rasters beside it are generated from it, and `assets/chrome/README.md` carries the command
that regenerates them.

🚨 **"The Console shows the default icon" was two different defaults, set by two unrelated
APIs, and only one of them is portable.** `with_window_icon` is winit's cross-platform
call, and on Windows it reaches `ICON_SMALL` alone — title bar and Alt-Tab. The **taskbar
button**, the most visible of the three, is `ICON_BIG`, reachable only through
`WindowAttributesExtWindows::with_taskbar_icon`, which exists on Windows and nowhere else.
Setting just the portable one would have looked like a fix and left the taskbar untouched.
Both live behind one `apply` call so the platform story has a single home. (On macOS
`with_window_icon` does nothing at all — the icon there comes from an `.app` bundle, and
the Console has none.)

⚠️ **The rasters are committed rather than built, and the drift that buys is paid for
explicitly.** Rasterising at build time with `resvg` would make the SVG the only source,
but the root crate has **no build script today**, and adding one is not a local change: it
builds the plugin cdylib, the standalone, the visual, the CLI and three editions, and every
one of them would grow a build script plus ~20 build-dependency crates so that one window
could have an icon. Committing the pixels costs **nothing** — `image` is already a
dependency here, for the overlay's formula PNGs. The price is that the PNGs can fall out of
step with the SVG, and the mitigation is that the SVG sits beside them with the regeneration
command written down, plus two tests (`console_icon::tests`) that pin the rasters' sizes and
opacity so a broken or resized asset fails at test time rather than shipping a window whose
icon silently did not load.

⚠️ **Drawing the circles and lines in code was the third option and is the one to avoid.**
It looks like less machinery; it is a transcription of a design asset that stops matching
the day the asset changes, with nothing to say so.

⚠️ **winit takes one bitmap per slot — it does not accept a set and pick**, which is what a
Windows `.ico` resource does. So the sizes are a choice about what scales best rather than a
spread: **48×48** for the window slot (`SM_CXSMICON` is 16 px at 100 % scaling and 36 px at
this workstation's 225 %; 48 divides exactly by 3 into 16 and by 2 into 24 and reduces into
36, so every common slot is a downsample) and **256×256** for the taskbar.

⚠️ **The mark does not survive 16×16, and that is a property of the artwork, not of this
code.** Measured, magnified, and looked at: the outer ring's 3 px stroke lands on 0.4 px and
the inner ring's 1.4 px stroke on 0.19 px; the ticks and the centre dot vanish and what is
left is a dark square with a grey smudge in it. **32×32 is the floor.** No 16 px raster is
committed, deliberately — shipping one would only give Windows an illegible bitmap to prefer
over a downsample of a good one. A legible small size needs a *hinted* variant of the
drawing (thicker strokes, no inner ring), which is an artwork decision.

📌 **This is the window icon, not the executable icon.** What Explorer draws on
`organon-console.exe`, and what a pinned shortcut shows, is a Win32 `RT_GROUP_ICON` resource
linked into the binary — a `.ico` plus a build script. Not done, and `assets/chrome/README.md`
records what it would cost.

⚠️ **Scoped to the Console alone.** `console_icon` is gated on `console-edition`, so the
plugin cdylib does not carry an icon it can never draw. Organon and Organon Mind are separate
products with their own identity; the mechanism is reusable by them and is deliberately not
wired up.

### 1.10 The live colour editor — tune the palette while looking at it

`/theme edit` (or `/theme adjust`) opens an editor for the palette being painted, in §1.9's band
above the composer. James's framing, 2026-08-14: *"a little dialog, much like the place where we
do our command completions… it shows the theme and the theme colors, and each one has a little
HSV editor on it."*

**The loop it replaces.** Every colour in `theme.rs` was chosen against a described intent and
then written as a hex literal and compiled. That is a fine way to *state* a palette and a
hopeless way to *judge* one — the only way to find out whether `light`'s whitest white is too
bright is to look at it, and until now that meant edit, rebuild, relaunch, look.

#### One vocabulary: the fields are enumerated once

🚨 **`theme.rs`'s `colour_fields!` macro is the only list of the palette's colours.** It
generates `Theme::fields`, `Theme::fields_mut`, `Theme::SCALAR_FIELDS` and `Theme::GROUPS` from
one grouped declaration, so a colour added later is editable, diffable, storable and on a ring
with no second place to remember. This is §1.8's rule reached from a new direction: a
hand-listed editor would silently stop covering a field somebody added, and nothing would say
so.

⚠️ **Rust has no reflection, so the list is hand-written** — which makes the guard the actual
work. `every_colour_a_palette_can_differ_in_is_reachable` copies field-by-field *through the
accessor* between every ordered pair of the four palettes, adds the two non-colour fields
(`scrim_floor`, `chrome`) by hand, and asserts the result equals the source; a field the
accessor cannot reach keeps the destination's value and the comparison fails by name. Its
residual blind spot is stated in the test: **a colour on which all four palettes agree to the
byte** is invisible to it, and a fifth palette closes that gap for that field.

⚠️ `ansi16` is one array field, so the macro cannot name its members; `ANSI16_NAMES` supplies
the sixteen and `Theme::editor_groups` folds them onto the terminal's ring. `TERMINAL_GROUP`
names the heading they attach to, so renaming it cannot silently orphan them.

#### 🚨 The HSV is the truth, not the RGB

**RGB → HSV → RGB does not round-trip, and it is not a rounding error.** A grey has no hue: drag
saturation to nought and the hue is gone from the bytes, so an editor re-deriving HSV every
frame would show hue 0 (red) the moment a colour went neutral, and dragging saturation back up
would return red rather than the blue it was. Value does the same at nought. So the editor holds
the `Hsva` of every field a hand has touched and derives the `Color32` from it — never the
reverse. An untouched field is not in the map and is read straight off the palette, which keeps
the state proportional to the editing rather than to the sixty-eight colours.

⚠️ **This is why it does not use `egui::color_picker::color_picker_color32`.** That function
solves the same problem with a cache in egui's context memory keyed by the **`Color32`** — and
§`theme`'s module doc explains at length why this palette deliberately keeps four fields holding
`#c8e6c8` (`human_text`, `tab_active`, `tab_menu_installed`, `term_fg`) apart. Keyed by value
they share one entry; keyed by field they do not.

⚠️ **`set_hsva` is on the drag path — every frame, per field being dragged — so it interns its
field name against `Theme::SCALAR_FIELDS` and `ANSI16_NAMES`, the two `&'static` tables, rather
than by constructing a palette and asking it.** Both answer the same question; the second built a
whole `Theme` and a sixty-eight-entry `Vec` per tick to learn a compile-time fact. The general
rule this is an instance of: `Theme::fields`/`fields_mut` allocate, which is right for the
once-per-action callers (a save's diff, a startup override) and wrong for anything inside a
gesture.

⚠️ **A second, subtler round-trip lives in the row itself and is pinned by test.** The drags are
in degrees and percent because that is how a hand thinks, and `h * 360.0 / 360.0` is not `h` in
binary floating point — so writing the scaled values back unconditionally made every field
differ from itself on the first frame, reporting a change nobody made and marking a freshly
opened palette `unsaved`. The comparison is on what the widget was *given* against what it
*returned*, which is the only form that can be quiet when nothing moved.
`an_untouched_editor_asks_for_nothing` is the test that found it.

#### The seam: the editor cannot assign the palette, and should not be able to

`conversation_view::draw` is handed `&Theme`; the one owner is `console_main`'s `Console`, which
is `theme.rs`'s "one owner, no globals" rule and the thing that makes a per-tab or preview
palette a second value rather than a rewrite. So an edit leaves as a value —
`ConversationOutput::theme: Option<ThemeChange>` — and `Console::apply_theme_change` assigns it
after the frame closure's borrow has ended.

🚨 **`Some` only on the frames something moved.** `Visuals` is held on the egui context rather
than read per frame, so `console_main` re-derives and re-uploads the whole chrome for every
change it is handed; answering `Some` unconditionally would do that sixty times a second for a
palette nobody was touching.

`theme_name` is now threaded into `draw` because this crate is given the palette's *values* and
cannot recover its label — once an override has been laid over it, the live palette equals none
of the compiled ones, and filing a saved override under the wrong name would apply a
light-theme correction to a dark palette.

#### What persistence means

Three things, deliberately not gated on each other:

| | Writes | Says |
|---|---|---|
| a drag | nothing | the head row's `unsaved` count |
| **save** | `theme_overrides[name]` = the **diff** from the compiled palette | a stderr line naming the count |
| **revert** | removes that entry | the palette returns to what this build ships |

🚨 **Unsaved is always visible.** A tuning session that evaporates at exit without having said so
is worse than no editor: the tuning felt finished. It is drawn in `mode_alert`, not `bad` — it is
not an error.

⚠️ **Overrides are keyed by palette**, because an override is a judgement about one palette.
⚠️ **Only the difference is stored**, so a later build that improves a shade nobody tuned is not
silently overruled by a file its owner believed recorded three edits. ⚠️ A stored colour naming a
field this build lacks, or a string that is not eight hex digits, is **skipped with a note** —
losing nine good edits over a tenth that aged badly is the wrong trade. ⚠️ Hex is **eight
digits always**: `panel_fill` is premultiplied at `0xe6` and a six-digit form would silently make
every saved panel opaque.

Startup applies overrides **after** `theme::select` has settled which palette won — an override
corrects a named palette and cannot resurrect a different one. ⚠️ It applies to an
environment-selected palette too: the variable is a loan of *which palette*, and the tuned
colours are part of what that palette now looks like on this machine.

#### The band, and the keys

The editor takes the region outright from both the receipt and the candidate list while it is
open. Those two answer a line and are gone in seconds; this is a surface a hand is *working in*,
and one that vanished because a keystroke reached the composer would be unusable.

🚨 **It claims Tab, the arrows and Escape — and no printing key, and not Enter.** The composer is
still live underneath, so a message stays sendable without closing the editor, and Enter keeps
meaning exactly one thing, which is §1.9's rule unchanged. Only one of the editor and the panel
reads a frame's keys, since both want the same three.

🚨 **The editor closes itself if the palette changes underneath it.** `/theme chocolate` typed
elsewhere, the CLI, or an agent's tool call can all repaint while an editor is open on `light`;
its held HSV would then describe colours that are no longer there. Comparing the incoming
palette against what the editor last painted is one `PartialEq` per frame and is the only signal
available — this crate is not told when the palette is reassigned.

⚠️ **`edit` and `adjust` are values of `console.theme`'s argument, not a verb.** That is what
makes them complete for free from the same `Choice` §1.9 already draws, with no second table and
no new ring. Two consequences: they must be in the `Choice` or `Registry::resolve` refuses
`/theme edit` during validation before the view sees it; and they are refused **on the sidecar**
by name, because the CLI and the MCP lane have no band above a composer to draw a dialog in.
This is the one place a console-lane verb is answered locally, and it is the lane's edge rather
than a violation: the palette really is console-wide (which is why the *edits* leave on
`ConversationOutput`), but the editor is a panel in this transcript.

⚠️ **ASCII throughout**, like the rest of the band. `theme_edit::drawn_strings` enumerates every
string the editor can draw — including the **group headings**, which are hand-written prose in
the field macro, and the **field names** — and §1.9's glyph allowlist test walks it. Sampling
rather than enumerating is how `✓` reached a third draw site.

⚠️ `ThemeEditor::open` takes a `focus` field name and **no command produces one yet**:
`/theme`'s schema carries a single argument, so `/theme edit human_text` is not a line the
registry can build. It exists because landing on a named colour is a one-line change the moment
a second argument is worth adding. Nothing claims the command exists.

### 1.11 `/organon` — the console's rings ARE Organon's UI hierarchy

James's framing, and the whole design in one sentence: *"the first thing we will see is the
choices `generator | motion | environment | look | synth | audio | settings | mind` because
those are the top level tabs… and these choices will map to the panels that are available in
Organon."* Not a command tree beside the instrument — **the instrument's own shape, walked from
the composer.**

`/organon` → the eight tabs. `/organon look` → the Look tab's twenty-five panels. `/organon
look surface` → that panel, as an element in the flow.

✏️ **That last arrow now ends somewhere else: the panel goes into a region's PANEL STACK, and
there is no transcript home at all** (§1.14, #98 Tier A). *A transcript is a log and a control is
not a log entry.* Both **rings are unchanged** — the tab, the panel, the two tables they read,
the dependent hook, the refusals — and everything below about *which* panel and *why it can be
drawn at all* is untouched. What changed is the **destination**, and it is called out here and
corrected in place at the two subsections it invalidates rather than being rewritten away.

**Neither ring is a list this console wrote.** The tabs are `organon_core::tabs::UiTab::ALL`,
which that module already calls "the single source of truth the editor's tab bar iterates". The
panels are the new `organon_core::panels::PANELS` — and the arrow between it and the editor
points the way that cannot rot: **`lib.rs` reads its card headings out of the table**, written
`card(&mut c[0], panels::LOOK_SURFACE.title, |ui| …)` at all twenty-five Look-tab call sites. A
renamed panel is one edit and the compiler finds the other end. ⚠️ **Only the Look tab is joined
that way**; the other seven are *absent* from the table rather than transcribed into it, because
an entry whose title nothing reads is exactly the un-joined copy the table exists to prevent. A
tab joins by converting its `card()` sites, one tab at a time.

#### 🚨 Seven of the eight tabs lead nowhere, and the ring says so in its own line

James typed `/organon generator 2` on a running build and was told *"`2` is not one of surface |
colour | material | …"* — the Look tab's twenty-five panels, on a line that said `generator`.
He read it as the console failing to register the word, which is the only reading available: a
Look-shaped answer to a Generator-shaped question. Two separate defects made that one sentence,
and both are the same failure — **a surface that knew and did not say**.

**The tabs stay, all eight, and the empty ones are marked.** The alternative was to offer only
the tabs with panels, which is honest and self-maintaining and was rejected: `UiTab::ALL` *is*
Organon's hierarchy, and a first ring showing one wedge of it would misrepresent the product as
having one section. So `look` carries `25 panels` and the other seven carry `not mapped yet — no
panels in the table`, **counted off `panels::in_tab` rather than listed**, so a tab stops being
marked on the day its `card()` sites are converted and no line here changes. That is the same
honesty `Status::Declared` already uses one ring down: named, offered, and truthful about what
choosing it opens.

⚠️ **An empty ring must never be silent, and the type is what enforces it.** `NarrowFn` answered
`Option<Vec<(label, doc)>>`, so a tab with no panels answered `Some(vec![])` — truthful, and
invisible: the band drew empty, which is indistinguishable from a band that is broken, and
`Palette::is_empty` then threw the panel away entirely. The hook now answers a `Ring`, whose
`Empty` arm **cannot be constructed without the sentence that explains it**. That sentence is
`registry::unmapped_tab`, written once and read three times — by the band (through
`Palette::hint`, so both renderers already draw it), by the refusal, and by the view lane.

⚠️ **The refusal consults the hook, so it names the ring it is refusing against.** `coerce`
refused against the declared `Choice`, which for a dependent argument is the union across tabs —
hence twenty-five slugs for a tab that has none. It now asks the hook first wherever the parent
word is in hand: `/organon generator 2` answers with the unmapped-tab sentence, and `/organon
look 2` answers *"`/organon look`: `2` is not one of surface | colour | …"*, the head carrying
the words that **chose** that list. ⚠️ **This does not fix itself as tabs are joined — it gets
worse**: the union today happens to be Look's, and a second joined tab would have that refusal
reading out two tabs' panels at once.

**Slug and title are different words**, and the rule that binds them is not cosmetic: no slug
may be a prefix of another slug on the same tab (`panels::no_slug_is_a_prefix_of_another`).
`Palette::sole_completion` takes a lone remaining candidate, so `surface` alongside a
`surface-fx` would make the shorter one permanently ambiguous — the second panel would silently
switch the first one's auto-completion off. `fx` is the answer, not a longer prefix.

#### The one registry extension: a ring that depends on the ring above it

`Registry::candidates` was already the whole machine — `Candidate::completion` is the *entire
line*, so accepting one and asking again yields the next ring, which is how `/organon look
surface` falls out of what shipped in §1.8 with no new walk. What it could not do is make ring
two a function of ring one: `ArgKind::Choice(Vec<String>)` is fixed when the table is built.

⚠️ **The obvious fix — a dependent `ArgKind` variant — was rejected on measurement.** That enum
is matched exhaustively at **~30 sites** across `command.rs`, `mcp.rs`, `conversation_view.rs`
and `registry.rs`, so a new arm is a change to the MCP schema generator, the dispatch validator
and three renderers, for one verb. Instead an `Entry` may carry a `NarrowFn` — a plain `fn`
pointer, `fn(arg, positional) -> Option<Ring>` — consulted by `value_candidates` **and by
`coerce`**, and by nothing else. `CommandSpec` is untouched, so a console verb still cannot have
one and the agent-facing vocabulary is unchanged.

Three consequences worth stating:

- **`Ring::Empty` beats `None` for an unjoined tab, and beats an empty list.** `None` falls
  through to the declared `Choice`, which would offer *every* slug on a tab that has none of
  them; an empty list draws a blank band and refuses with nothing to say. `Empty` carries the
  reason, and the enum is what makes carrying it unavoidable.
- **`Entry`'s `PartialEq` is hand-written now**, to exclude the hook. `derive` compared it and
  rustc warns that function-pointer equality is not meaningful. Excluding it is also the right
  meaning: an entry is its vocabulary, and the hook is how a ring is drawn.
- 🚨 **The declared value space is still the union across tabs, and stays so.** It is what the
  MCP schema and `/help` are generated from, and neither has a parent word in hand — one value
  list per argument is all a schema has. What changed is that the two paths *with* a parent word
  in hand now use it: a **typed** `/organon motion surface` is refused in the composer, naming
  the tab. The `(tab, panel)` check in `summon_organon` is therefore no longer the only gate,
  but it is not dead either — it is the door a caller that never touched the composer arrives
  through, and it is pinned by a test that calls it directly.

#### The element, and why it is not an artifact

`Body::Organon(OrganonBlock)`, a sixth body — **not** a third `ArtifactContent` arm. That enum
is one arm per `organon_core::kind::Kind`, pinned by
`every_shared_kind_has_exactly_one_artifact_arm`, and an Organon panel is not in that vocabulary
because it *cannot* be: a `Kind` has to be placeable on a text lane, and this is a live egui
panel with dropdowns and typed numeric entry. Forcing it in would have meant two arms answering
`Kind::Panel` — which that test calls "a kind this view cannot address" — or widening the shared
kinds with one the terminal front-end can never honour. **Being unable to be an artifact is the
evidence that it is its own body.**

The block carries the panel **resolved**, as a `&'static Panel`, not as a `(tab, slug)` pair: the
pair is checked once, at the command, and an element holding it would push that check into every
frame that draws it.

✏️ **`Body::Organon` no longer exists** (§1.14, #98 Tier A): a panel is not an element of a
transcript at all, so there is no sixth body and no `OrganonBlock`. **Everything above stays
because the argument is still correct and would be needed again if a panel ever came back** —
an Organon panel could not have been an `ArtifactContent` arm, and being unable to be an artifact
was the evidence it was its own body. `conversation.rs` carries the same paragraph as a comment
where the variant stood, so a reader of the code meets it too. ✏️ The **second** paragraph
survives verbatim and is now `panel_stack::Entry`'s: the panel is still carried resolved, and the
reason is still that a `(tab, slug)` pair would push a check into every frame that draws it.

#### The seam: a callback, not a render list

`conversation_view::OrganonDraw` — `&mut dyn FnMut(&mut egui::Ui, &'static Panel)`, passed into
`draw`. **The opposite shape to `SurfaceRequest`, and the difference is forced.** A surface is a
*picture*: the view says what it laid out, `console_main` renders into a texture, the answer
arrives next frame, and deferral costs one frame of "rendering…". A panel is *widgets*: a
dropdown must open where it was clicked and a drag must be read in the pass it was drawn. There
is no texture to hand back later, so the console's drawing has to happen **inside** this crate's
layout, at the point in the flow the element occupies.

The contract is otherwise identical and deliberately so: `organon-console` knows a panel by its
tab, slug and title, and cannot see `OrganicMathParams`, a `ParamSetter` or a `World`.

✏️ **The type moved to `panel_stack::OrganonDraw` and its address changed with the destination;
the argument above did not.** It is still a callback rather than a render list, still for the
reason that a dropdown must open where it was clicked, and still the opposite shape to
`SurfaceRequest`. What is no longer true is the phrase *"at the point in the flow the element
occupies"* — a panel occupies a point in a **stack**, and `conversation_view::draw` no longer
takes this parameter at all. What travels into the conversation view now is the *destination*
(`panel_stack::Home`), not a way to draw, which is what lets `/organon` refuse **in the composer**
when nothing holds a stack rather than on a stderr nobody reads.

##### ✏️ …and in #117 it widened from the BODY to the whole CARD

`panel_stack::OrganonDraw` is now `&mut dyn FnMut(&mut egui::Ui, &'static Panel) -> bool`, and it
is handed **every** panel in the column rather than only the `Live` ones.

James, holding the Console's panel column up against Organon's own editor stack: *"I want us to
adopt the styling … so that it looks just like it does here in terms of the padding and the fact
that it just has the one word for the panel and not all of the words you have now. **In fact, it
should use the same exact code somehow.**"*

The chrome he is pointing at is `lib.rs`'s `card()` — `theme::framed`, the three-stop silver
header band, an `egui::CollapsingHeader` — and it lives in the **root crate**. There are exactly
two ways to run one copy of it:

| option | why not / why |
|---|---|
| move the chrome down into a crate both can see | **rejected.** There is no such crate. `organon-core` is host-free *by acceptance test* (`cargo tree -p organon-core` must show no egui), and `theme.rs` reaches `nih_plug_egui`, `theme_config` and the paint helpers. Creating one is a topology change to run a 25-line function. |
| widen the seam so the crate that already owns the chrome draws it | **taken.** The seam already existed and already pointed the right way — the body crossed it every frame. |

- **`panel_surface::OrganonPanels::card` is the new caller of `crate::card`**, and `lib.rs`'s
  Look tab is the other. One card function in the tree, two products. ⚠️ So a change to `card()`
  is visible in both, and the Console has no compile-time way to notice.
- **`NOT_TRANSPLANTED` is still `organon-console`'s sentence**, and it is placed *inside* the
  card by the caller. It has to be: most of the twenty-five panels are still `Declared`, and a
  declared panel that skipped the chrome would be the one card in a column of twenty-five wearing
  something else. `panel_stack::absent_body` is the single place that decides which panels get
  it — `console_main` asks it rather than matching on `Status` a second time.
- 🚨 **The `bool` is the whole reason `organon-console` still compiles and draws on its own.**
  `false` = this build drew no card, and the stack falls through to `panel_stack::plain_card` —
  the frame and heading it drew before this widened. Every test in this crate takes that arm, and
  `a_console_with_no_organon_behind_it_still_draws_the_panel` reads the frame's own text shapes
  rather than a return value, because the claim is about pixels.
- ✏️ **`panel_stack::heading` is now `panel.title` and nothing else** — `Surface`, where it said
  `◈ organon · look · Surface`. The breadcrumb was answering a question this surface no longer
  asks: it was written when a panel was an element scrolling past in a **transcript**, where
  "which of Organon's tabs is this from" is real. In a panel column every card is one of
  Organon's and the column is the answer.
- ⚠️ **The inter-card gap moved with the chrome.** `card()` ends on its own `add_space`, so
  `panel_stack::draw` no longer adds one — two would have been twice the editor's, which is the
  padding this tier is about. `panel_stack::GAP` survives as `plain_card`'s own trailing space.
  ✏️ Both numbers are 0 since #120, below.

##### ✏️ …and in #120 the PALETTE stopped coming with it

James, on the live build: *"I didn't want to adopt the blue, gray color theme for all of these. I
want them to adapt to the current theme colors. I would like to create a theme that does have
these colors, but then the whole UI should have those colors too. Right now, the colors stay
fixed no matter how I set the theme of the panels I'm talking about. Also, we need to remove that
spacing. See the dark spacing between them? We need to tighten it up and stack them more tightly
together."*

#117 bought the padding, the corners and the one-word heading by calling `crate::card` — and
every colour in that function resolved through `theme.rs`'s palette accessors, which read
Organon's own `theme_config`. So `/theme dark` moved the terminal, the composer, the status
strip and the tab bar, and left the panel column blue-slate. **The card is now drawn through a
`theme::CardStyle`**: pigment is a parameter, geometry is not.

| what | who decides it | why |
|---|---|---|
| body gradient, header band's three stops, header rule | `CardStyle` — the caller | pigment. Organon passes `CardStyle::organon()` (the live `theme_config`, unchanged); the Console passes `panel_surface::console_card_style` |
| border, title | `CardStyle` — and for the Console they are **read verbatim** off `Theme::panel_edge` / `Theme::panel_title` | the palette already has an opinion about both |
| corner radius, inner margin, band bleed, collapsing header, grain, bevel | `theme.rs`, shared, no parameter | this is what *"it should use the same exact code"* was asking for. A second card body is the drift #117 existed to prevent |
| the trailing gap | `CardStyle::gap` | the one geometry the two surfaces genuinely differ on — see below |

- 🚨 **The two gradients are DERIVED from `Theme::panel_fill`, not read from new `Theme`
  fields.** The rejected alternative was four new palette fields (`card_body`, `card_header`, …):
  more explicit, and it costs more than it buys — all four shipped palettes would have to answer
  four questions they have no opinion about, `every_colour_a_palette_can_differ_in_is_reachable`
  would need a fifth palette disagreeing on each to see them, and **a palette James writes later
  would have to fill them to look right**, when the complaint is that the column ignores the
  palette he already wrote. Derivation means a new palette needs only the fields it already sets.
  If a palette ever *wants* a card that does not follow its panel plate, that is the day the
  fields earn their place.
- ⚠️ **The steps are neutral — the same number on all three channels.** Organon's own table tilts
  blue as it lightens (`card 0x212A30` → body top `0x242E35`, `+3,+4,+5`) because §2's rule is
  `blue − red ∈ 8..=15`. Carrying those per-channel deltas onto a green or a warm plate would
  drag it back toward blue slate, which is the exact complaint. The **magnitudes** are Organon's,
  averaged across its three channels, so a cut card keeps the reference's compressed 5–15-level
  tonal steps (§14) in the caller's own hue.
- ⚠️ **`panel_fill` is premultiplied glass and the card is opaque.** The column paints that plate
  over whatever is behind it; a card in the same translucent colour would darken the region twice
  and sink into its own background. The premultiplied components are exactly what the column
  contributes over black, which is what makes them the right base to step from.
- ⚠️ **A card is not separated from the column by its fill, and never was.** `palette.panel` and
  `palette.card` are *the same value* in blue slate: what separates a card is its gradient, its
  border and its bevel highlight. So passing the column's own background as the plate is right
  rather than something to correct with an arbitrary lightening.
- 🚨 **The gap needed two numbers because the two surfaces run different `item_spacing`.**
  Measured on a headless context rather than reasoned about: Organon's editor sets
  `item_spacing.y = 6` (`theme::install`) and the Console never touches spacing, so it takes egui's
  default 3. Card-to-card that was **12 pt in the editor and 9 pt in the column** — the column
  was already the tighter of the two, and what made it *read* as a dark band was that the seam
  showed near-black `panel_fill` beside a blue-slate card. `panel_stack::draw` now zeroes its own
  contribution around the loop and each card restores it inside its own scope (so rows *within* a
  panel are untouched), and `panel_surface::PANEL_COLUMN_GAP` is 0 — **so the column's seam is
  0 pt and the editor's is unchanged at 12.**
- ⚠️ **`panel_stack::GAP` is `pub` now**, and equals `PANEL_COLUMN_GAP` by assertion rather than
  by comment: `panel_surface::the_fallback_leaves_the_same_gap_organons_card_does` compares them.
  The root crate is the only place that can see both, since this crate cannot look upward.
- 📌 **Both halves of the seam are pinned on the side that can see them.**
  `panel_stack::the_column_contributes_no_space_between_two_cards` lays a real stack out headless
  and measures what the *column* adds (0; delete the zeroing line and it reports 3, drop the
  restoration and it says so in the same run);
  `panel_surface::two_editor_cards_leave_twelve_points_between_them` measures the *other product*
  and holds Organon's editor at 12.
- ⚠️ **What is still Organon's palette inside a Console card**: the Surface panel's own row
  labels, which `param_sink` draws in `theme::TITANIUM()`. Only one of the twenty-five panels is
  `Live`, so it is one card's rows — but it is real, and it is the next thing to notice if the
  column still looks half-themed.

#### 🚨 The wall: an Organon parameter cannot be written from outside `nih_plug`

**Look ▸ Surface is `Status::Live` and every other panel is `Status::Declared`.** The ring lists
them all; Surface opens Organon's real controls, the other twenty-four open a line saying they
have not been transplanted yet. ✏️ **Four panels are `Live` now** — Surface, Cast Shadows,
Lighting and Bloom (organon#124, and the list itself is
`panels::only_the_transplanted_panels_are_live`, never a sentence here). Nothing else in this
subsection moved: the wall below is still a property of `nih_plug`, and the mirror is still how
a panel gets a write path at all. What follows is why that took a mirror rather than a setter, and
it is a property of `nih_plug` rather than a gap in this crate.

Every panel widget is `srow(ui, w, "node bevel", &params.bevel, setter)` over a `ParamSetter`,
which calls `GuiContext::raw_set_parameter_normalized(ParamPtr, f32)`. Implementing `GuiContext`
is trivial; **honouring it is impossible**. Checked, not assumed:

| Route | Verdict |
|---|---|
| `ParamPtr::set_normalized_value` | `pub(crate)` (`params/internals.rs:77`) |
| the `ParamMut` trait — every setter | `pub(crate)`, and its doc says so on purpose |
| `FloatParam`'s value fields | private; only `pub smoothed` is reachable |
| `Params::deserialize_fields` | `#[persist]` fields only, not params |
| `wrapper::state::deserialize_object` | `pub(crate)` |
| nih-plug's standalone `Wrapper` (the one non-host `GuiContext`) | in a **private** module inside `wrapper/standalone.rs` |

`nih_plug` is an upstream git dependency, not a fork. **A panel drawn without a write path is a
panel whose knobs do nothing — which is precisely why `/panel` was retired** ("its controls
changed something you could not see"). So the panel needed a different place for its writes to
land before it could be drawn at all.

#### The way through: a writable mirror, and an identity join at the widget

Three facts, none of them guessed, and all three now load-bearing:

1. **The console already owns a `World` in-process** (`console_main.rs`), and every picture —
   backdrop, surface, portal — is that one World rendered into a different target from a
   `Shared` the frame path publishes. **So there is no second process and no IPC bridge**: a
   panel that can produce a `Shared` drives what you are looking at, immediately.
2. **`look_shared` builds its snapshot from `OrganicMathParams::default().to_shared()`** — 1372
   params constructed headlessly, with no host, no audio thread and no GPU. So a console-owned
   params object is free, and that is what the panel reads its ranges, units, value strings and
   enum variant names off. **It is metadata and is never written.**
3. 🚨 **`PresetValues` is a plain, freely-writable mirror of the params with a `to_shared()`**,
   and `param_table.rs`'s macro states the convention that makes it usable: *"the param-side
   field and the preset-side field are assumed to share the same identifier."*

⚠️ **What was actually missing was not a writer — it was an identity.** `&params.bevel` is a
`&FloatParam`; nothing about it tells a writer that its mirror is `pv.bevel`. `param_sink.rs`
supplies that join by naming the field **once**: `srow!(ui, w2, "node bevel", sink, p, bevel)`
expands to both `&p.bevel` and `|pv| &mut pv.bevel`. A rename on either side is a compile error,
the same way `param_table!`'s slot lists are.

`Sink` is the two-armed destination — `Host(&ParamSetter)` for Organon's editor, gesture-wrapped
and automation-recordable exactly as before, and `Mirror(&mut PresetValues)` for the console.
Five row helpers branch on it (`scalar_row`, `check_row`, `choice_row`, plus `read`/`write` for
values a panel computes rather than drags), and `Mirrored` converts a mirror field to and from
the normalized 0..1 domain through the param's *own* `preview_normalized`/`preview_plain`, so a
skewed float, an integer's rounding and an enum's variant index all stay the engine's
arithmetic.

⚠️ **The field accessor is a `fn(&mut PresetValues) -> &mut T`, not a `&mut`, and that is
forced.** The field lives *inside* the sink, so passing `&mut Sink` and `&mut pv.bevel` together
is two mutable borrows of one value. Handing the row a way to *take* the borrow, at the moment
it needs it, is the shape that compiles.

#### What landed, and the two costs of it

**Look ▸ Surface is one function.** `panel_surface::surface_card` — `lib.rs`'s Look tab calls it
and so does `console_main` through `OrganonDraw`. There is no second rendering to keep in step,
which is the point: `/organon`'s claim is *this is the same instrument*, and a copied body would
make that false within a week.

⚠️ **The extraction was the work, not the plumbing.** `editor_ui` is one ~4,700-line pass over
105 cards and a card in the middle of it cannot be called from anywhere else; lifting one out is
what "transplanting a panel" consists of. Measured on Surface exactly as it stood: 457 lines, 190
helper call sites, 24 conditional `.value()` reads. **Every one of its 167 distinct parameter
fields exists in `PresetValues`** — checked field by field before any of this was written, since
one that did not would have been a control with no writable mirror and a reason to stop.

**The mirror reaches the world as a diff, not as a snapshot.** `OrganonPanels::overlay` compares
`mirror.to_shared()` against the mirror's own *starting* snapshot and copies only the lanes that
disagree, through `organon_core::ipc::overlay_changed`. Two things follow, and both are why it is
a diff:

- 🚨 **An untouched panel is byte-inert over any snapshot whatsoever** — invariant #4 made
  structural rather than checked. A console in which nobody opens a panel publishes the bytes it
  published before this existed, and `overlay_changed_is_inert_when_nothing_moved` pins it.
- 🚨 **No lane manifest.** Naming which lanes the Surface card owns would be a second list
  beside the panel body, and the failure mode is the quiet one: a param added to the card would
  keep working in the editor and silently stop reaching the world here. The difference between
  two snapshots is that list, derived rather than maintained.

⚠️ **Lane granularity, not byte granularity.** A changed `f32` differs in one to four of its
bytes; copying only the differing ones would splice two floats into a value neither side held.
`Shared` is `Pod` and every field is a `u32`, an `f32` or an array of them, so a 4-byte word
*is* a lane — and `shared_is_a_whole_number_of_lanes` fails rather than corrupting one if that
ever stops being true.

⚠️ **The panel opens on Organon's defaults, not on the console's current look.** Against
`BackdropSource::World` — the portal, and the backdrop when it is showing the world — that is
*exactly* faithful, because the console's snapshot there is those same defaults. Against a
dressed substrate it is faithful only about what it has been told: a row reading `0.35` while
the substrate renders something else is saying **"I have not asked"**, not "the world is at
0.35". There is no honest alternative available — `Shared` → `PresetValues` is not invertible,
so the panel cannot be seeded from what is on screen.

⚠️ **`material_gen` does not ride the mirror**, because a preset stores the material's *path*
rather than a counter. "Load Material…" bumps an `AtomicU32` on a background thread and
`overlay` folds it into the published snapshot by hand; without that the folder picker writes a
sidecar the renderer never re-reads.

⚠️ **One mirror per console, not one per element.** Two `/organon look surface` cards in a
transcript are two views of one instrument; reading different values off each would make the
claim the command exists to make false on sight. ✏️ **The cards are in a stack now, not in a
transcript, and the rule and its reason carry over unchanged** — §1.14 makes the same argument
one level up to keep the *stack* itself console-wide rather than one per region, and the two are
the same sentence about the same instrument.

⚠️ **The write lands one frame later**, because the conversation is drawn after the snapshot is
published. That is the same arrangement `surface_requests` and `pane_points` already use, for
the same reason, and the alternative is publishing twice per frame.

#### ⚠️ Where the Console's Surface panel is not the editor's

Faithful: every range, unit, value string, enum variant name, help text and grid line, all read
off the real `OrganicMathParams`; and the disclosure logic — which rows appear under which
surface mode — because those conditions route through `param_sink::read` rather than through
`params.….value()`.

Two differences, both consequences rather than choices:

- **The slider fill.** `Sink::Host` draws nih-plug's `ParamSlider`; `Sink::Mirror` draws an
  `egui::Slider` over the same domain with the same formatter. `ParamSlider` takes a
  `ParamSetter` in its constructor — writing is not something it does, it is something it *is* —
  so no mirror can drive one. Same grid lines, same readout, different bar.
- **↑/↓ inside an open dropdown.** The editor's combo live-applies as you move, so the look
  scrubs; the shared one commits on the following frame. The popup borrows the child `Ui` while
  a write needs the sink, so the choice has to come back out of the closure.

#### The pattern, for the other twenty-four

✏️ **Superseded by the subsection after this one** (organon#124): twenty-one of the twenty-four
are now *declared* rather than converted, and the four steps below apply only to a panel whose
body has to stay hand-written. The paragraph is kept because its two warnings are the ones that
still bite, and both survive the change.

A panel converts in four mechanical steps, and the compiler checks three of them: lift the
`card()` body into its own module; turn each helper call into the matching macro (`srow!`,
`crow!`, `combo!`); turn each `params.x.value()` into `rd!`; flip its `panels::Status` to `Live`
and add its slug to `only_the_transplanted_panels_are_live`. ⚠️ **The `.value()` reads are the
half that fails silently** — a missed one compiles perfectly and pins the Console's panel to
Organon's defaults, so the checkbox ticks and the rows underneath it never appear. ⚠️ And each
panel's fields must be checked against `PresetValues` first: Surface's 167 were all present, but
that is a measurement of Surface, not a property of the editor.

#### ✏️ …and in organon#124 the pattern stopped being a pattern and became a table

Everything above is what a transplant *was*. It is kept because the mechanism it describes —
`param_sink`, the `Sink`, the `rd!`/`wr!` identity join, the wall it works around — is unchanged
and load-bearing. What changed is the **shape of a panel body**: twenty-one of the twenty-four
remaining panels are no longer written at all.

James, on what a preset should be able to do: *"we can tell from the preset what values we have
adjusted from the default … we could construct custom panels or even a single custom panel with
sections and sliders and dropdowns that are **tailored to the exact changes that we made on that
preset**."*

🚨 **That and the transplant are one problem approached from opposite ends.** The transplant
hand-writes, panel by panel, a mapping of **field → section → widget**; a preset-built panel
needs exactly that mapping **as data**. Twenty-four imperative bodies first and a table extracted
afterwards is the mapping written twice and then reconciled. So the table came first, and both
renderers read it: **`native/src/panel_table.rs`**.

##### What the table carries, and the three things it deliberately does not

Measured across all **519** rows of the Look tab's twenty-five cards before a line of it was
written:

| | in the table? | why |
|---|---|---|
| panel, section, order | **yes** | it exists nowhere else — a `PresetValues` field is a name and a type |
| the row's label | **yes** | 361 of 519 differ from `Param::name()`, *systematically* |
| the control kind | **no** | derivable, **519/519**, zero exceptions |
| a dropdown's options | **no** | `choice_row` already walks the param's own steps |
| range, unit, formatting, ⟲ | **no** | read off the real `OrganicMathParams` |

🚨 **The control kind is a measurement, not a convention.** `FloatParam`/`IntParam` → a slider
(399 rows), `BoolParam` → a checkbox (83), `EnumParam` → a dropdown (37), and the editor
disagrees with the param's Rust type in **no case anywhere on the tab**. So
`param_sink::AutoRow` — one trait, four impls beside `Mirrored` — reproduces the editor's choice
from the param alone, and a fifth param kind is a compile error naming the missing impl rather
than a control quietly drawn as a slider.

⚠️ **The dropdown gap organon#124 opened with does not exist.** The issue recorded *"a `u32`
that renders as a dropdown needs its variant names, and those live at the `combo!` call sites"*.
They do not: `choice_row` walks `param.step_count()` and labels each option through
`param.normalized_value_to_string`. A `combo!` call site passes a label and a width and nothing
else. **Both of that section's "genuinely missing" items therefore reduce to one** — field →
panel → section.

⚠️ **The label is not the param's name, and the reason is structural rather than sloppy.**
`kal_spin` is `"Kaleido Spin"` to a DAW's flat automation list and `"spin"` inside a card already
headed *Scene Kaleidoscope*; `bevel` is `"Bevel"` and `"node bevel"`. **The label is a function
of the grouping** — which is why one table owns both, and why deriving it from the param would
be wrong twice: redundant on screen, and it would change what the plugin draws.

##### 🚨 A macro list, not an array — the identity join survives

`&[Row { field: "bevel", … }]` throws away the one property `param_sink` exists to provide: a
string in an array is checked by nothing, while `row bevel` in the list expands to `&p.bevel`
**and** `|pv| &mut pv.bevel`, so a rename on either side is a compile error. That is also the
idiom this tree already uses twice — `preset.rs`'s `for_each_tab_field!` and `param_table.rs`'s
`param_block!` — rather than a new one invented here.

The list expands three ways, from one source:

1. **the body** — drawn by Organon's editor with `Sink::Host` and by the Console with
   `Sink::Mirror`, exactly as `panel_surface::surface_card` already was;
2. **`ITEMS`** — `&'static [Item]`, the data a preset's diff is grouped by;
3. **`draw_one(name)`** — one control, chosen at runtime by field name, which is the seam a
   preset-built panel is drawn through.

⚠️ **`Item::Section` is a marker, not a container.** A section owns the rows that follow it until
the next one — how the editor draws it. A filtered panel therefore emits a heading *lazily*,
before the first surviving row under it, so a section whose rows were all filtered away draws
nothing rather than an empty heading.

⚠️ **egui ids come from the field, never from widget order.** This section already records that
bug being fixed twice, and a filtered panel is where it returns: a preset panel drawing rows 3, 7
and 40 of a card must give each the row id it would have had drawing all forty.
`stringify!($f)` is what every generated control is keyed on.

##### Two kinds of panel, and the count is the honest number

- **`panel!(@generated …)`** — the body **is** the list. A `free` item in a generated panel is a
  `compile_error!`, so it provably contains no hand-written fragment.
- **`panel!(@labelled …)`** — the body is hand-written because the panel has control flow, a file
  dialog or a capability gate. The table still owns its labels and grouping; only the *order*
  lives in the body.

Measured, the twenty-four un-transplanted panels are **352 rows, 40 help texts, 36 section
headings, six conditionals and three hand-written fragments** — so twenty-one of them qualify for
`@generated`, and the exceptions are Ray Tracing, Liquid and Post. Surface stays hand-written and
holds nearly all of the tab's complexity by itself: fifteen disclosure reads, two file dialogs and
a material-graph loader. `grep -c '@labelled'` is what "how much of this tab is still written by
hand" answers to.

##### 🚨 Eleven controls have no writable mirror, and it is deliberate

Of the **514 distinct fields** the twenty-five panels draw, **503 are in `PresetValues`** and
eleven are not — Temporal's seven (`taa_*`, `motion_blur`, `mb_*`, `stochastic_glass`) and four of
Ray Tracing's (`rt_debug`, `pathtrace_enable`, `spectral_secondaries`, `pt_caustic_photons`).
These are not gaps. The Temporal card's own help text says *"per-display, not preset-captured"*
and `params.rs` repeats it: *"**Per-display, NOT preset-captured** (like HDR/MSAA/rt_debug)"*.

⚠️ **So Look ▸ Temporal cannot be transplanted at all through the mirror** — every one of its
seven controls would draw, drag and move nothing, which is precisely why `/panel` was retired.
Ray Tracing converts minus four of its sixty-one rows.

📌 **Decided (James, 2026-08-21): a third `panels::Status`.** `Live` / `Declared` / **`HostOnly`**
— Temporal stays named and offered, and says *"per-display, not preset-captured — these controls
exist only in Organon's own editor"* rather than *"not transplanted yet"*, which would be a lie:
it is not pending, it is impossible through the mirror. Ray Tracing's four rows take the same
treatment per row, drawn disabled with the reason on hover. **It generalises**, which is why it
beat the alternatives — the Settings tab is largely this the day it joins.

The two rejected options are worth keeping: drawing them **read-only** from Organon's defaults
was refused on §1.11's own principle (a control that looks live and is not is worse than an
absent one), and **adding the eleven to `PresetValues`** reverses a deliberate decision about
what a preset means in order to make a transplant tidier.

⚠️ **`HostOnly` is not built yet** — it lands with the panel that needs it, per invariant #4, and
an unused enum arm would widen every exhaustive `match` on `Status` for no caller. Until then
`panel_table::every_row_the_table_names_has_a_writable_mirror` is what stops one of these panels
being declared by accident.

##### What a preset-built panel can reach, stated rather than implied

The Look tab, fully joined, homes **503 of `PresetValues`' 1,333 fields — 37.7%**. A preset whose
diff is mostly Generator fields gets a mostly-empty panel until that tab joins. **A field with no
table row is drawn anyway**, from its param alone, with `Param::name()` for a label and no
section: the control kind and the range are derivable, and only the short label is not. A field is
never silently absent from a panel claiming to show what a preset changed, and that
visibly-second-class rendering is itself the argument for joining the next tab.

Two related numbers worth not confusing: **49 Look-tagged fields are drawn on no Look panel**, and
**103 fields drawn on Look panels are tagged to another tab** (95 Generator, 8 Environment —
`palette` and `surface_mode` live in the Surface card). `preset.rs`'s `for_each_tab_field!` tag and
this table answer different questions and neither substitutes for the other.

##### Landed so far

**Cast Shadows, Lighting (Direct) and Bloom** — ten controls between them, `@generated`, drawn by
Organon's editor and Organon Console from one declaration. `panels::Status::Live` is now four
panels rather than one, and `panel_table::a_declared_panel_is_exactly_a_live_one` asserts the pair
in both directions: a `Live` panel with no declaration is a card that opens to nothing, and a
declared panel still marked `Declared` is a body nobody can reach.

⚠️ **A card the editor hides, the Console still offers, and that is not new.** Organon's Look
tab wraps Cast Shadows in `if !kifs` and Surface in `if !raymarch`, so those cards vanish when a
generator has no use for them; a Console panel column has no such gate, because the stack is a
column somebody assembled rather than a tab the editor lays out. The same asymmetry already
applied to Surface before this change. It is worth knowing rather than fixing: the honest gate
would be on the *mirror's* generator, and a panel that disappeared out of a column somebody built
by hand would be worse than one whose controls happen not to apply.

⚠️ **Surface is not in the table yet, on purpose.** Joining it is a 167-site rewrite of the one
file another workstation is actively changing, and it does not belong in the change that
establishes the mechanism. It is the next one.

⚠️ **Three witnesses in `panel_stack.rs` and one in `registry.rs` named Bloom as their example of
a `Declared` panel**, and transplanting Bloom failed all four for a reason that had nothing to do
with what they assert. They now ask the table for *a* panel of each status rather than naming one,
and each `expect`s loudly on the day a status has no members left. That is the shape every
per-status witness here needs as the `Live` set grows.

### 1.12 The screen — whether the window covers the display, on a THIRD axis

**`organon-console/src/screen.rs`.** `organon console screen <full|windowed|toggle>` puts the
console's window into borderless full screen and back, and **F11 flips it from inside the
window**. That is the whole feature; almost everything below is about why it is a third thing
rather than a value of an existing one.

#### 🚨 It was asked for as a posture, and it cannot be one

James's words were *"adds a new posture, which is full screen"*. The obvious implementation —
a third slot beside `terminal` and `desktop` — **is not available**, and the reason is §1.6's
own design: `Posture` is a **scalar**, not an enum. `Form::at(t)` lerps componentwise between
two ends and `Posture::from_scalar` accepts anything between them, so `organon console posture
0.5` is a real drawable console and the CLI takes it. There are no slots to add a third to;
there is an axis.

And full screen is not a point on it. Every one of `Form`'s fourteen tokens is a margin, a
corner, a padding, a line height, a gap, a tracking, or the presence of a border, a rule or a
tick. **Full screen changes none of them.** It changes the rectangle the window occupies, which
no token in that struct describes.

It also passes §1.6's own orthogonality test verbatim. That section argues theme and posture are
orthogonal because *"`organon` at desktop posture and a light palette at terminal posture are
both real things, and neither is a variant of the other"*. Apply it here: a **full-screen
terminal** is the oldest thing in computing and a **full-screen desktop document** is what every
reader app opens into. Both are real. So this is a third orthogonal state, and all four
(posture × screen) combinations are consoles somebody would want.

#### ⚠️ Why the form is NOT nudged when the window fills the display

The tempting version — full screen also opens the margins out, because a 2560-wide window wants
more air than an 1100-wide one — couples to the **wrong variable**, and that is worth stating
precisely because the argument *for* it is a good one. A **maximized** 2560-wide window and a
**full-screen** 2560-wide window are the same width and want the same margins. A full-screen
window on a 1280-wide laptop is *narrower* than a maximized one on this workstation's display,
and wants today's desktop margins unchanged. "Is it full screen" is simply not the question the
margin wanted asked — "how wide is it" is. If width-responsive form is ever wanted, its input is
`available_width`, and it is a change to how a `Form` is *resolved* rather than a third state
feeding into it.

The coupling would also be the part that is regretted: with it, `organon console posture
terminal` typed while full screen either draws something that is not the terminal posture, or is
refused. A person who typed a posture would not get the posture they typed.

#### ⚠️ No state is held — the window is the answer

**There is no `screen` field on `Console`.** `winit::window::Window::fullscreen()` *is* the
state, so `Screen` is derived from the window at the moment a command arrives rather than
remembered beside it. A remembered copy would be a second source of truth for one boolean, and
the failure it forecloses is concrete rather than hypothetical: this verb is not the only way a
window gets resized (macOS's green button, a tiling window manager, a platform restoring a
session), and after such a divergence a remembered `Windowed` would make `toggle` send an
already-full-screen window *into* full screen — the one word whose entire meaning is "the other
one" doing nothing visible and reporting nothing.

`Fullscreen::Borderless(None)` — the window's current monitor — and never `Exclusive`, which
takes a video mode from the display and is a projector's business. Only the *discipline* is
shared with `organon-visual`'s `sync_fullscreen` (`organon-visual/src/main.rs`) — touch the
window only on a real change — and
the **two differ exactly where it matters**: that one holds a `fullscreen_applied` bool and
compares against it, because its intent arrives from `World::wants_fullscreen` on every frame
and it needs an edge; this one has no periodic intent to debounce, so it can ask the window and
avoid keeping the bool at all. **No code is shared, deliberately.** `World::wants_fullscreen` is
a field that travels in `Shared`, written by the visual's own `F` key and its projector launch
logic; the console's `World` renders only into a backdrop texture and never owns a swapchain, so
reaching for it would mean the console writing into the visual's IPC state to set a flag on its
own window. Two lines of winit is a far smaller price than that coupling.

**Not remembered across launches**, on §1.6's rule for posture — the console opens windowed
however you left it. A window that reopens covering the display with no title bar is the state
that most needs an undo and has the fewest ways to get one.

#### 🚨 The way out is F11, and choosing it needed an argument

A borderless window has no close button, so a full-screen console with no key would be a trap.
The verb is reachable — the console is full of terminals — but that is a way out for somebody
who remembers the verb.

**Escape is not available**, and §1.2 owns why: in a terminal tab the keyboard is the child's,
`vim` needs Escape, and taking it would have to be conditional on state — unbuilt work with a
designed mechanism reserved for it (§2's portal row: `consume_key` `retain`ing out of the same
`i.events` vector `term_view` clones). Spending that here would be borrowing a subsystem to pay
for one window flag.

**F11 is free, and that is measured rather than assumed.** `term::encode_key` returns `None` for
every function key, so the console has never sent it to a child under any modifiers;
`conversation_view::palette_key` and `theme_edit::edit_key` both answer `Ignore` for it. All
three are pinned by tests in `screen.rs`, so the day one stops being true it fails there rather
than fighting silently. Claiming it takes nothing from anybody — exactly what Escape could not
say — and it is the convention every full-screen window on this platform already uses.

The claim is therefore **unconditional** (every tab, every state, whatever has focus) and needs
none of the state-dependent machinery Escape would. It is read at the same site as `⌘T`/`⌘W`/
`⌘1-9` — `redraw`'s raw frame events, before any panel is laid out — which is what makes it
work while the composer has focus, and it is **not** consumed out of `i.events`, because nothing
downstream wants it. The chord and `organon console screen toggle` funnel into one call, so the
key cannot drift from the verb.

🚨 **A key-repeat is not a press, and that filter is in `screen_key` rather than left to be
observed.** Holding a key streams `pressed: true` events, which egui marks
(`Event::Key::repeat`). Without the filter a resting finger flips the window once per repeat and
the state on release is decided by the parity of however many arrived — indistinguishable from
the chord being broken, and worst on the one chord that is the *way out* of a window with no
title bar. A toggle is also the worst verb to repeat; an absolute `full` would just be
re-applied and swallowed by the change guard. ⚠️ The `⌘` chords read the same event and were
deliberately **not** fixed here: existing behaviour on a different key table, whose right answer
may not be "ignore the repeat" (an autorepeating `⌘1` is arguably fine), and folding an
unrelated behaviour change into this one would hide it. ✏️ **That reservation was right, and
PR #83 has since settled it the opposite way for half its chords**: `tabs::command_key_action`
now takes the flag and streams `Switch` on repeat — holding `⌘⇧]` should keep cycling, and
repeating `⌘1` means nothing — while refusing it for `New` and `Close`. So one event, two key
tables, and genuinely opposite right answers. The lesson is the arrangement rather than either
verdict: **resolve a shared input flag per key table, not once at the read site**, or the first
table to need a rule imposes it on every table that comes later.

#### Reaching it, and why the schema can state the whole value space here

Three words, no scalar: a window either covers the display or it does not, and there is nothing
between for a number to address. That is the one place this verb is *simpler* than posture and
it shows up in the CLI — `console posture` cannot be a clap `PossibleValuesParser` (its value
space is two words **or** a float, which clap cannot state, so its gate moves to `run_console`
and its words do not tab-complete), while `console screen` can be, and is. `SCREEN_WORDS` is the
one table read by `bin/ctl.rs`, by `console_specs()` and by `ScreenCmd::resolve`'s refusal —
`POSTURE_WORDS`' arrangement, for its reason.

⚠️ **The verb is named for the window it moves, and not "fullscreen", on purpose.** §2's portal
row reserves the phrase *full screen* for a still-unbuilt portal state — the portal taking the
whole window, after `immersive`. Two different rectangles can each be described as going full
screen, so this one says which.

### 1.13 The exhibit — a picture and a document, from a path a human typed

`/media <path>` puts a file in a conversation tab. Two kinds — `image` (PNG, JPEG) and
`markdown` — added to `organon_core::kind::Kind` beside `scene` and `panel`, so the console's
one vocabulary grew rather than being sidestepped. Several paths in one line make **one exhibit
with several items**: `organon_core::exhibit::Exhibit` is a kind, a non-empty list of items, and
nothing else. That shape is day-one rather than speculative — three generated candidates
arriving as one three-item exhibit is the case it exists for, and retrofitting "several items"
later means touching every kind written before it (#56 T4).

#### 🚨 A media kind names no file, and that is why it could join `Kind` at all

The patch wire is `patch <up> <rows> [kind]` — three positional fields, no payload slot
(`console_ops::parse_console_op`). That is not an omission; it is the patch protocol's central
property, that *a program which can print can ask for a rectangle without being able to drive
the machine*. **A kind carrying a path would end that outright**: anything able to append a line
to `$TMPDIR/<ns>-console.txt` could make the console open any file the user can read.

So `Kind::Image` does not mean "this file". It means **the exhibit the human loaded** — exactly
as `Kind::Scene` means "the scene the console is rendering" rather than naming a generator, and
exactly as a `panel` patch's `BlockPanel` is constructed console-side from a wire word carrying
no description. Both placements build their payload from console-side state.

⚠️ **`/media` is therefore a view-lane verb and must stay one.** It is deliberately absent from
`console_specs()` and so from the MCP catalog: an agent cannot call it. #56 leaves *how an
exhibit reaches the console* open between an agent verb and the console recognising a path in a
tool result; **this tier picks neither**, because both hand path selection to something that is
not the person at the keyboard. The absence is the decision, not an oversight, and
`registry::VERB_MEDIA` carries the reason at the definition.

#### The terminal placement is honest rather than complete

`organon console patch 0 4 image` claims its rows and draws one line: *an image exhibit is shown
in a conversation tab, not a terminal patch*.

📌 **That is not a media-shaped exception.** The invariant
`every_shared_kind_has_exactly_one_patch_arm` defends is that the CLI cannot accept a kind word
this front-end then **silently** ignores — its own doc says the failure is one that "dispatches,
records a success, and paints nothing". A notice in the claimed rows is not that failure. A
companion test, `every_kind_either_draws_itself_or_says_why_not`, now pins the other half: a
media arm answering `None` there would claim rows and draw nothing in them, satisfying the first
test on paper and breaking it in the pane.

⚠️ **Why the picture is a conversation placement in this tier.** A scene patch works because
there is exactly *one* scene texture and every scene quad samples it through its rows
(`term_view::draw`'s `patch_image`). An exhibit has a texture **per item**, so painting one in a
character grid needs a per-patch texture ledger keyed on something a terminal pane does not have
— there is no `ElementId` in a grid — plus a second eviction budget and a `draw` signature
change. That is #56 T5/T6 work. T4's stated bar is *"an image and a markdown document render
inline in a conversation tab"*, and that is what landed.

#### Nothing touches the frame thread

Opening a file and decoding a JPEG are both unbounded in the only sense that matters — they
depend on a disk and on somebody else's bytes — so `console_main::service_exhibits` never calls
either. One thread per item, results on an `mpsc` channel, and a frame that only ever
**collects**. This is the first place in the console that rule has needed enforcing; everything
else here is synchronous and in-process.

The visible consequence is one or more frames of `reading...`, the same deferral a conversation
surface already shows. 🚨 **`Failed` is a state of its own, not a missing entry.** A blank
rectangle and a file that will never decode must not look alike; collapse them and a bad path
reads as "still loading" for the rest of the session. The two plates differ in wording and in
colour, and a `Failed` entry is what stops a broken file being re-read every frame forever.

#### Refusals name the file, and known-but-unbuilt is its own answer

`exhibit::KNOWN_UNBUILT` is the table that makes this more than an extension check. An `.mp3` is
refused **by name** with its real reason — *audio needs a playback device and a player, not just
a decoder* — rather than getting the answer a typo gets. PDF, video and LaTeX carry their own.
A refusal that cannot tell *"I do not know this extension"* from *"I know exactly what this is
and have not built it"* is a dead end for the person reading it, and both those sentences are
now pinned by tests.

⚠️ **There is deliberately no `media` kind**, and `kind.rs`'s refusal test asserts the word
resolves to nothing. "images/mp3/pdf/etc" is three unrelated engineering problems wearing one
word; a kind named after their union would promise all three from the arm that delivers one.

#### The two tables that can silently disagree

`image` is built `default-features = false`, so an extension in `exhibit::IMAGE_EXTENSIONS` with
no matching cargo feature is **not a compile error anywhere** — it is a file the composer
accepts, dispatches, reads off the disk, and only then fails to decode.
`native/tests/exhibit_formats.rs` encodes and decodes every offered extension **in memory** and
fails the build if the two drift. No fixture is committed, on #56 T4's own bar: a repository that
gains sample media never loses it.

#### The budget is the surfaces', not a second one

`surfaces_to_evict` is now **generic over its key** — a conversation surface is keyed
`(pane, element)`, an exhibit item `(element, item)`, and the policy (least-recently-*requested*
first, ties broken down the request list) is a fact about how a person reads a scrollback that is
identical for both. `MAX_EXHIBIT_TEXTURES` is a separate *ceiling* from `MAX_SURFACE_TEXTURES`
because the two ledgers fill differently — an exhibit can arrive with several items in one
command, and a pooled cap would let a three-item gallery evict the surface a panel is driving —
but there is only one policy.

🚨 **Documents are budgeted too, and by bytes rather than by count.** The first cut of this tier
capped only pictures, reasoning that a `String` costs no GPU. That is true and beside the point,
and #86's review caught it: a document that is never evicted is held for the rest of the session,
so a long conversation that opened a dozen READMEs keeps every one alive behind cards nobody can
see. `documents_to_evict` is the weighed twin of `surfaces_to_evict` — same rule, same tie-break,
**pure and tested** — and it is a separate function rather than a cap computed for the counting
one because *how many entries fit* is unanswerable in advance when the entries are different
sizes: dropping the two oldest might free 4 KB or 8 MB, and only the running total knows when to
stop. The property that falls out, and that a test pins, is that **one oversized document goes
alone** rather than taking its small, freshly-read neighbours with it.

⚠️ **`ExhibitContent::Document` holds an `Arc<str>`, and that is a frame-cost decision.** The
console hands the whole `ExhibitContents` map to the view on *every* frame, exactly as it hands
over `SurfaceImages` — but that map holds `TextureId`s, which are `Copy`. A `String` here meant a
moderately-sized README being deep-copied sixty times a second for as long as it was held, in a
file whose §1.7 measurement exists precisely because frame time is load-bearing. The `Arc` makes
that clone a refcount bump. Also from #86's review.

Every eviction prints a line naming what went and why (`[exhibit]`), on `free_surface`'s rule and
for its reason: *a silently dropped texture reads as "the picture is still there"*. **A document
says so too**, because a re-read nobody was told about is how a document that quietly reloads on
every scroll looks like a console that is merely slow. It drops the
**entry**, not only the texture, which is what makes the next frame ask again — an item is **a
reference, never bytes**, so an eviction costs a re-read and never costs the picture. Pictures
are scaled to a 2048 px long edge before upload (a phone photograph is 4000 px and would be a
64 MB texture, against a conversation-surface budget of ~23 MB), and a file over 64 MB is refused
**before** the decoder sees it, because a decoder handed a 500 MB PNG allocates its full pixel
buffer before anything can object.

### 1.14 Regions — how the one pane is divided, on a FOURTH axis

**`organon-console/src/region.rs`**, plus the walk in `console_main.rs`'s `draw_regions`.
`organon console viewport <region> <content>` — `/viewport` in a conversation composer — divides
the console's single pane into up to six rectangles and says what each one holds. James asked
for *"split the viewports … into four or two and two or one on one side and two on the other"*,
and those three shapes are the module's own acceptance test.

✏️ **Up to SIX, on three columns — #98 Tier B.** The grid was 2×2 through Tier 1, 2b and A; the
subsection *"Three columns"* below owns the change, including the two words in this file's own
vocabulary that now mean something different.

**Tier 1 is the model, the geometry, the lane and the seam. It is not yet the content.** Only
`agent` draws something live — the tab the console is already showing. `panel` is a **named
placeholder**: the region says an Organon editor panel belongs there and that a later tier gives
it a body. `3d` and `media` are not in the vocabulary at all. What this tier buys is the thing
only a hand can judge — whether a half-height conversation is any good — and it buys it without
touching the engine.

✏️ **`panel` has a body now — see "a scrolling stack" below.** The sentence above is left standing
because it is what Tier 1 shipped and what the two tiers after it were scoped against; the
placeholder is gone, `3d` landed in Tier 2b, and `media` is still absent.

#### 🚨 A fourth axis, and the argument is §1.12's word for word

James's earlier framing folded this into the posture, and he changed his mind to this explicitly.
The reasons are the ones §1.12 already had to make for the screen. `Posture` is a **scalar**, so
there is no third slot to add — there is an axis, and `organon console posture 0.5` is a real
drawable console. And a split is not a point on it: every one of `Form`'s tokens is a margin, a
corner, a padding, a line height, a gap, a tracking, or the presence of a border, and **a split
changes none of them**. It changes how many rectangles there are.

It passes §1.6's orthogonality test verbatim — a split terminal-posture console and a split
desktop-posture one are both real, and neither is a variant of the other — so this is a fourth
orthogonal state. All (posture × screen × layout) combinations are consoles somebody would want,
and each of the three verbs means exactly what it says in every one of them.

#### 🚨 Flat, never nested — and the reason is the vocabulary, not the geometry

`Region` is twelve words over a 3×2 grid: `full`, the two rows, the three columns, the six cells.
**A region holds one thing and never splits again.** The tree is the obvious model and it is the
wrong one here because **a tree has no names**. `/viewport left agent` is a sentence a person says
and an agent writes; the same intent in a tree is a path through splits that must already exist,
and the console lane is fire-and-forget with no return path
(`console_ops::console_cmd_path`) — so a caller cannot ask what the tree currently looks like in
order to describe a place in it. Twelve fixed words are addressable from a line that gets no
answer, which is the only transport this verb has.

What that costs is stated rather than hidden: **no uneven splits and no dragging a divider.**
Those are real wants and they are a later tier's. The seam for them is that `region_rect` is the
only place a rectangle is computed.

#### 🚨 Three columns, and the vocabulary is DERIVED — #98 Tier B

`topcenter` was never a missing word; it was a missing **rectangle**. Four bits over a 2×2 grid
has no middle column to name, so the target layout — two control columns flanking the instrument,
with the agent beneath it — could not be spelled at all. Tier B is **four cells to six**, three
columns by two rows, and every property survives verbatim: a region is a set of cells,
coexistence is disjointness, refusals name the obstacle, `only_one_because` still attributes the
3D limit to Organon, the last `agent` cannot be evicted, and a console nobody has typed
`/viewport` at is unchanged.

**The twelve words are a cross product, not a list**, which is what makes the count defensible
rather than curated. A region has to be an axis-aligned run of cells or `region_rect` has no
rectangle to return; over a 3×2 grid there are eighteen such runs (six column-spans × three
row-spans). The discriminator is the module's own rule that **a region is a word a person says**:
the column-spans English names are *left*, *center*, *right* and *all three*, and the row-spans
are *top*, *bottom* and *both*.

| row span ╲ column span | all three | left | center | right |
|---|---|---|---|---|
| **both rows** | `full` | `left` | `center` | `right` |
| **top** | `top` | `topleft` | `topcenter` | `topright` |
| **bottom** | `bottom` | `bottomleft` | `bottomcenter` | `bottomright` |

The six excluded runs are the **two-column** ones — *left and centre*, *centre and right*. Naming
them (`leftcenter`?) would mint a word nobody says to complete a table, and nothing breaks by
leaving them out: `plan`'s vacancy walk reports a two-column gap as two vacant regions, exactly
as the 2×2 grid already reported a three-quarter gap as two.

⚠️ **`left` and `right` CHANGED MEANING, and that is the intended change rather than a
regression.** They were **half** the pane; they are now the **outer column** of three, at a fixed
`SIDE_COLUMN` width. Anyone with muscle memory gets a narrower column than they got yesterday, and
`/viewport left panel` does not look the way it looked before. It is said out loud here, in
`region.rs`'s own header and in the changelog fragment, because a word quietly meaning something
new is exactly the drift this axis spends its refusals preventing.

#### 🚨 Every region word also answers to its INITIALS, at all four front doors

`REGION_ALIASES` in `region.rs` pairs each word with a short form — `f t b l c r` for the first
six, `tl tc tr bl bc br` for the six cells — so `/viewport tl panel` and
`console viewport tl panel` are `topleft` exactly. Regions only: the content words, the stack
actions, the panels, `screen`, `posture`, the patch kinds and the verbs themselves have none, and
that is a scope decision rather than an oversight — a region word is the long one, and `3d` is
already two characters.

The four doors, and then the one gate they all arrive at:

| where | what it accepts | what it **lists** |
|---|---|---|
| door — `/viewport` composer | long + short (`ArgKind::ChoiceAliased`, `registry::coerce`) | the twelve, each carrying its short form in `Candidate::doc` |
| door — MCP `console.viewport` | long + short (`command::check_kind`) | the twelve in `enum`, the short forms in `description` |
| door — `organon console viewport` | long + short (clap `PossibleValue::alias`) | the twelve — a clap alias is *hidden* by construction |
| door — tab completion | nothing to accept; it only offers | the twelve (the palette's ring, and the generated shell completions) |
| gate — `Region::resolve` | long + short | the twelve, plus one clause saying short forms exist |

🚨 **All four, because a word that exists for one caller and not another is exactly what
`registry.rs`'s header forbids.** A region word passes three independent gates before `resolve`
ever sees it — the `ArgKind` the composer and the MCP schema share, the CLI's own
`PossibleValuesParser`, and the completion ring reading the first — so a composer-only
abbreviation would not have been a small feature with a gap in it. It would have been a second
vocabulary.

🚨 **`REGION_WORDS` stays canonical-only, and that is the constraint the whole shape is built
around.** It is what `--help`, the MCP `enum`, the palette's rings and every refusal *display*;
twelve more entries in it would present a vocabulary with twelve shapes as one with twenty-four
words. So the short forms are **accepted everywhere and listed nowhere**, and shown *beside* their
word instead: the palette puts one in each candidate's doc slot, and `/help`, the refusals and the
MCP `description` share one clause built from the table's first and last pairs — *"each has a
short form: `full` is `f`, `bottomright` is `br`"*. Two examples, because the rule is legible from
one short word and one compound and twenty-four in a refusal is a wall nobody reads.

🚨 **The table is DECLARED and the rule is a test, which is the inverse of the arrangement the
twelve words themselves have.** The vocabulary is derived (a cross product of spans, above); the
abbreviations are written out. Deriving them would be an algorithm nobody could contradict — a
future region word whose initials collided with an existing short form would silently shadow it,
and one whose natural abbreviation is not its initials would have nowhere to say so. So
`region.rs` holds the pairs and three tests hold the pairs to the rule: no alias equals a region
word and no two collide; every compound word's short form is its parts' joined, with the parts
read off the grid rather than listed; and every short form resolves to the same variant its long
word does.

⚠️ **A declared short form is a second exact word, never a prefix rule.** `l` resolves; `le`,
`lef`, `L` and `TL` still refuse. That is `Region::resolve`'s no-approximation rule intact — the
alias is rewritten to its long word *before* the search, so there is one matching rule rather than
two, and a short form cannot come to resolve to something the long form does not.

⚠️ **The word travels as TYPED and is expanded once, at the console.** clap's
`PossibleValuesParser` returns the string it matched rather than the canonical name
(`clap_builder`'s `parse` yields `value`, not `v.get_name()`), and the composer passes the typed
word through for its own reasons — so both doors put `tl` on the sidecar line and
`Region::resolve` is the single place it becomes a region. Expanding at one door and not the other
would make one command read two ways in the session log depending on which door it came through.
`Region::as_word` is untouched, so nothing is ever *written* short: a saved layout, a displacement
notice and a refusal all still say `topleft`.

📌 **`ArgKind::ChoiceAliased` is additive, and the arithmetic is why.** Counted on `main`:
`ArgKind::Choice` appears **43** times across the tree, **21** of them constructions, and
`ArgSpec { … }` **50** times — so widening `Choice`'s payload or adding a field to `ArgSpec` is a
change to every vocabulary in the console, deferred ones included. The new variant converts one
construction and leaves the other twenty untouched and inert (`CLAUDE.md` invariant #4), and
because every reader matches `ArgKind` exhaustively the compiler named the **seven** renderers
that had to learn it rather than leaving one to surface on a running console.
`ArgKind::choices()` is the one place the two choice variants are read as the same thing, for
callers that only want "what words may go here".

#### 🚨 Fixed side columns, and the centre takes the remainder — NOT equal thirds

`region_rect` grows **two vertical cuts**, and their positions come from a width rather than from
a ratio: `SIDE_COLUMN = 320.0` points in from each edge, with the centre absorbing what is left.
The **row** split is still the exact midpoint — two rows, nothing to choose.

James's decision, and it stands on what this tree already does rather than on taste. Organon's own
editor sizes control columns absolutely and lets the subject take the rest: `SidePanel::right`'s
`default_width(320.0)` for the theme dock, `exact_width(150.0)` for the presets rail, and
`mind_shell::DockSizes::default()`'s `left: 260.0, right: 300.0` beside a viewport that absorbs the
remainder. Equal thirds would pin the instrument to a third of the window whatever the window is.
**320 is the widest fixed control column in the tree**, chosen so a panel that fits Organon's own
side dock fits a console `panel` region without the region being what decides.

📌 **It is a width, never a ratio**, so it does not scale with the pane — which is the point, and
is also why the next paragraph has to exist.

#### ⚠️ The narrow-pane rule: the columns vanish, the rows survive

A fixed side means a pane can be too narrow to seat two of them. Below **`MIN_COLUMNS_WIDTH` =
2 × `SIDE_COLUMN` + `MIN_SIDE` = 688 pt** the two cuts would cross and `left` and `right` would
overlap in the middle. The rule is **decided rather than discovered**:

> **The side columns keep their width or there are no columns.** A region that needs a cut —
> anything not spanning all three columns — returns `None`. A region that spans all three
> (`full`, `top`, `bottom`) needs no cut and is unaffected.

So a narrow console still splits into **rows**, and every column word refuses until the window is
wide enough; `plan` then answers `None` for a column layout and the seam says the window is too
small for it, naming the command that undoes it. That is actionable, where a 20-point `left` is a
sliver somebody has to guess about. Pinned at the boundary from both sides — at exactly 688 the
centre is `MIN_SIDE` wide and stands; one point under, every column word refuses.

⚠️ **The rejected rule was "the sides shrink"**, and `mind_shell::layout_workstation` is the
precedent for it — its docks yield proportionally so the viewport survives. Right there, wrong
here: those docks are *chrome* around a subject, while every region here is somebody's assigned
content and none outranks the others. Shrinking would also make `left` mean a different width at
different window sizes with no word explaining it, and a side column narrowed to `MIN_SIDE` can no
longer hold what it exists to hold — which is §1.3's "refused, not clamped" arriving as geometry.

🚨 **What green does NOT say.** Nobody has looked at a three-column console. Whether 320 is the
right width, whether the instrument reads as the subject at 1100 points, and whether two control
columns beside a live transcript look like Organon's editor or like a cramped imitation of it are
all questions a hand and a screen answer — §3's ledger carries them.

#### The overlap rule, which is a bitmask and nothing else

Every region is a set of the six **cells**. Two may be held at once **iff their cell sets are
disjoint** — that is the whole geometry model, so there is no layout arithmetic to get wrong. An
assignment that meets something already held is resolved by containment:

| Relation | Answer |
|---|---|
| Disjoint | both stand — `left` and `right`, or the four corners |
| One contains the other | the other **gives up its place**, and the displacement is reported |
| Partial overlap | **refused by name**, quoting both regions |

⚠️ **The containment arm is the one place this module acts rather than refusing, and it is not a
convenience.** The console opens holding `full`, so a rule that refused every overlap would
refuse the first word of every split — and `full off` cannot be the way out, because it is
refused by the last-agent rule below. Measured in `region.rs`'s tests: without it, no split is
reachable at all. It is safe where a partial overlap is not, because containment has exactly one
reading — `left` is the only held region `topleft` can be displacing, and it is displaced whole.
A partial overlap (`top` asked for while `left` is held) has no unambiguous thing to take away,
so it is refused, and the refusal names both. **`left` and `topleft` can therefore never both be
held**, which is the invalid state the whole rule exists to prevent; a test walks every ordered
pair of assignments and asserts no two held regions overlap by any route.

#### 🚨 Two refusals about meaning rather than geometry

**The last `agent` region cannot be evicted.** A console with no agent region is a window with
nothing to talk to, and the way back is not obvious from inside it because the verb that would
fix it is typed *at* an agent. So any command whose **result** would hold no agent is refused —
`full off`, `full panel` and `left panel` from a default console are all the same eviction by
different names, and one invariant checked once on the resulting layout closes all three. That
shape is deliberate: a per-verb special case is how the second route comes to be the one nobody
remembered.

**A region that already holds nothing cannot be cleared.** A command that changes nothing and
says nothing is indistinguishable from one that never arrived.

#### 📌 The uniqueness rule, and whose limit it turned out to be — Tier 2b

A content kind that may exist **only once** is, on a second assignment, **refused by name, saying
what already holds it** — never moved. That follows §1.3's "refused, not clamped": moving a thing
somebody can see because they named a second place for it is a guess about which of the two they
meant. Tier 1 stated the rule and built no machinery for it, because an unreachable arm is an
untested branch pretending to be a design. `3d` makes it reachable, and building it changed where
the limit is *attributed*.

🚨 **The limit belongs to the producer, not to the idea of a viewport.** Tier 1 pre-committed to
it as a property of the content kind — *"at most one region can hold the live World"* — and under
James's ordering that is backwards. A region holding `3d` is a rectangle a producer draws into.
Today the only producer is Organon's `World`, and it is **Organon** that cannot be drawn twice in
a frame: `engine_plan` renders it once because `frame_index` and the TAA jitter phase riding on it
are shared between targets. A future producer — the simplified real-time engine James describes —
might fill four regions happily, and would otherwise inherit a refusal it has no reason to obey,
which would read as a rule about viewports rather than as an accident of one engine's temporal
history.

So `Content::only_one_because` is the single site that decides, and **it answers with a reason
rather than a bool**, because the reason is what carries the attribution: the refusal a person
reads names Organon and its shared jitter phase, not "viewports are singular". ⚠️ **What is not
available is attributing it in the type system**, and that is stated rather than worked around: a
`Producer` enum with one variant, invented so the limit could hang off it, is exactly the untested
branch Tier 1 declined to build. The attribution is a reason string, a doc and a test that asserts
the refusal quotes it — and the seam for a second producer is that one function.

⚠️ **A displacement is still allowed to move it.** `full 3d` while `left` holds `3d` displaces
`left` and stands: the check is asked of what **survives** the assignment, not of what is held
now, so widening one copy is not the same act as asking for a second. That is the same
"invariant on the resulting layout" shape the last-agent rule already uses, and for its reason —
a per-verb special case is how the second route comes to be the one nobody remembered.

#### 🚨 `3d`, and why the obvious word was taken — Tier 2b

The content word is **`3d`**. Three candidates were real:

| Word | For | Against |
|---|---|---|
| **`3d`** | §1.14 already promised this word in print; general; names no renderer; it is James's own phrase | reads cruder than `agent`/`panel`/`media` |
| `scene` | the best register of the three | **collides** — `organon-scene`'s own header is *"the substrate, below the plugin"*, and in this tree "scene" already means the thing painted **behind** the glyphs |
| `world` | matches `organon-world` | 🚨 names *Organon's renderer*, which is the one thing the word must not do |

`scene` lost on the collision and `world` lost on the ordering that decides this whole tier:
**the generalized 3D viewport is what is being built, and Organon is a particular application of
it.** A region says *a 3D picture belongs here*; which engine draws it is the producer's business,
and `world` would have baked today's only answer into the vocabulary a person types.

⚠️ The Rust spelling is `Content::ThreeD` because an identifier cannot begin with a digit. The
**word** is `3d`, and the word is what travels — on the wire, in `--help`, in the ring and in
every refusal.

#### 🚨 The producer seam — where the generality actually lives

> **A producer yields a texture the console can sample, at a size the console asks for.**

That is the whole boundary, and it is deliberately not *"a function that draws into our device"*.
The in-process producer satisfies it trivially (`World::render_to_texture` into the console's own
target). An out-of-process one satisfies it later by importing a shared texture, **without
restructuring the region model** — which is the accommodation James asked for, and it costs one
sentence today rather than a layer.

🚨 **There are no speculative arms behind it, and that is the point.** No `Producer` enum with one
variant, no trait methods nothing calls, no vocabulary word for choosing a producer. §1.14's own
rule holds: an unreachable arm is an untested branch pretending to be a design. **The generality
is in where the boundary is drawn, not in machinery behind it.** What a second producer will
change is `Content::only_one_because` and the site that renders — not `Region`, not `Layout`, not
`plan`, not the lane, and not the two presentations below.

#### 🚨 Two claimants for one frame: the portal wins, and the loser says so

An open portal and a region holding `3d` both want the one World render. `engine_plan` is widened
to `(portal_open, region_holds_world, backdrop, patches_want_image) -> (BackdropSource,
Option<ViewportTarget>)` and arbitrates between them; `the_engine_is_asked_for_at_most_one_frame`
is widened with it, to the full 2 × 2 × 3 × 2 cross product. ⚠️ Widening the function while
leaving the proof at its old arity is the exact shape of a test that keeps reporting green about a
space it no longer covers, so the loop's arity moved in the same commit as the signature.

**The portal takes the frame.** The argument is §1.2's own rather than a new one: the portal is
**temporary and dismissable**, so the state where it holds the frame ends with one word
(`organon console portal close`) that sits in the same ring as the word that got you there. The
region is the persistent thing a person arranged and it is *still arranged* — nothing is written,
nothing is remembered, and closing the portal hands the frame straight back. That is the same
"no remembered value to get wrong" property the backdrop already had, extended to one more
claimant rather than re-argued for it.

The two rejected rules, named so nobody has to re-derive them:

- **The region wins.** Then `/portal open` is a command that appears to do nothing whenever a
  `3d` region exists — and this lane is fire-and-forget with no return path (§1.3, "the refusal
  reaches nobody"), so there is nowhere to say why. A verb that silently no-ops is the defect
  this file keeps a running tally of.
- **Refuse the second by name**, on §1.3's "refused, not clamped" precedent. Same problem: the
  refusal reaches a reader of stderr and nobody else, so from the composer `/portal open` looks
  broken rather than declined.

🚨 **The loser paints a notice and never a stale texture.** §1.14's vacancy rule applies with more
force to a picture than to an empty quarter: a rectangle that *was* rendering a world and now is
not is precisely what a broken viewport looks like. The yielded region says the portal has the
world, says why (Organon renders one frame per console frame) and names the command that gives it
back. It also registers no interaction region at all, which is what keeps `scene_viewport`'s
single interned egui id to one claimant per frame.

#### 🚨 One mechanism, two presentations — what "keep it in sync" actually means

The portal is unchanged from a person's point of view: same verb, same state machine, same
screen-anchored rect, same wheel claim. What changed is that it is no longer the only live
rectangle, and **nothing is implemented twice**. A **viewport** is a producer plus a camera plus a
texture; a region is one way of presenting one and the portal is another. `SceneMode` in
`scene_input` has modelled that distinction since before either existed — `Workstation` is *"a
pane inside the workstation, a widget among widgets"*, `Immersive` is *"the scene is the window
and the interface floats over it"* — so it is the seam, rather than a parallel notion invented
here. Both presentations are `Workstation` today, which is the honest answer: a floating rectangle
and a region are both bounded panes inside an interface.

| Was the portal's | Is now | Serving |
|---|---|---|
| `Console::portal` | `Console::viewport` — **one** texture | one target is live per frame, so a second could only ever hold a picture nobody may refresh |
| `portal_input` | `viewport_input` — **one** `SceneInput` | one camera, because there is one `World` |
| `portal_points` | `viewport_points` — `portal_rect.or(region_rect)` | `or`, and the order **is** the precedence above, not a second copy of it |
| `render_portal` | `render_viewport` | gated on `engine_plan`'s answer, never on a second reading of the state |
| `free_portal` | `free_viewport` | **the single release site**, in `render_viewport`'s gate |
| `paint_portal` | `paint_viewport(…, mode)` | two call sites, one implementation |
| `portal::pointer_inside` | `+ pointer_inside_any(&[Rect])` | both presentations claim the wheel the same way |

⚠️ **The texture release moved, and that is a consolidation rather than an omission.** Closing a
portal used to free it on the spot. A `3d` region can stop being live by three more routes — it is
cleared, it is displaced, or `viewport full agent` resets the layout — and a release per route is
how the one nobody remembered comes to leak 2.5 MB. `render_viewport`'s gate is now the one site,
reached every frame, and it is total over every route by construction because it asks
`engine_plan` rather than asking what just changed. The gesture accumulator is reset with it, or a
latch stranded mid-drag would have the next viewport claiming the wheel with no drag behind it.

📌 **What this buys the next tier, in one sentence:** when a second producer arrives, the portal
shows it as readily as a region does, because the producer seam sits **below** both presentations.
That property is cheap to preserve now and expensive to retrofit, which is the whole reason the
portal was kept and generalised rather than left beside a copy of itself.

#### ⚠️ The wheel is region-aware now — the second consumer §1.14 predicted

Tier 1 recorded that `term_view` reads the wheel and every key from **raw input**, and that this
was inert because there was exactly one live tab and so no second consumer. **A viewport region is
that second consumer.** `term_view::draw`'s `portal: Option<Rect>` becomes `viewports: &[Rect]`,
tested by `portal::pointer_inside_any` — `pointer_inside` over a list, not a second mechanism, for
the reason this section already gives about not inventing a second gesture vocabulary. Two
rectangles go in the list: the portal's and the `3d` region's.

⚠️ **Both, even though at most one is live.** A yielded region is showing a notice and is still
not the transcript, so a wheel over it must not scroll text that is nowhere near the pointer.

⚠️ **The rect is computed before anything is drawn, not read back from the region walk.** The walk
visits regions in `Region::ALL` order, so a viewport that happened to be visited first would have
consumed the scroll from inside `scene_viewport` and one visited second would not — relying on
that is relying on the layout's alphabet. Only `term_view`'s explicit rect test is order-free.
This does **not** widen `scene_viewport`'s contract: keys are untouched, and the terminal still
owns the keyboard.

#### The camera: one, and a region does not get a second

**One camera, console-wide.** §1.3 owns it and nothing there needed widening: `World` holds a
single yaw/pitch/distance, there is one `World`, so a viewport region and the portal are two
windows onto the same viewpoint rather than two viewpoints. There is one `SceneInput`
accumulator, drained once per frame, and the hand-outranks-an-agent arbitration therefore did not
have to learn anything — a drag is a drag, whichever rectangle it landed in.

A second `3d` region is refused (above), so "what happens to the camera when a second viewport
region exists" has no state to answer for. If a producer ever lifts that refusal, *then* per-region
cameras become a real question; it is not one today, and inventing an answer now would be
inventing the machinery this section just declined to build.

⚠️ **`camera::viewpoint_is_visible` gained a `region_3d` argument, and leaving it out would have
made the predicate lie** — §1.3's "it says so when it moves something nobody is looking at" would
have shouted *"nothing on screen is showing the world"* at somebody watching a live picture. The
truth table is now checked over its whole input space rather than by example. `console.camera.read`
reports `region_3d` as its own key beside `portal_open` — **a separate fact, not folded in**,
because an agent acts on them differently: the portal is something an agent may open and close, a
region viewport is something a hand arranged. Reporting a region as `portal_open: true` would
invite `console.portal close` aimed at a rectangle it cannot touch. ⚠️ It reports the **layout**,
not the frame: a region can hold `3d` while the portal has the world, and `visible` is true either
way. And nothing was appended to `Shared` — §2 already refuses that, and this is host state that
dies with the window.

#### ⚠️ Unassigned space is a sentence, never a blank

`plan` returns every occupied region **and every unassigned one**, each with its rectangle.
§1.9's `Ring::Empty` argument at the scale of a quarter of a window: a region that draws nothing
is indistinguishable from one that is broken. Vacancy is **coalesced largest-first**, so a layout
holding only `topleft` reports one vacant `bottom` rather than three vacant cells — the word in
the notice is then the word a person would type. A pane too small for the layout (any region under
`MIN_SIDE`, or any column word under the narrow-pane rule) yields no plan at all, and the console
says so across the whole pane with the command that undoes it, rather than drawing slivers.

✏️ **A gap two columns wide is two vacant regions, and that is the honest answer rather than a
hole in the coalescing.** The vocabulary has no word for *left and centre* (above), so a layout
holding only `right` reports a vacant `left` **and** a vacant `center`. The 2×2 grid already
behaved this way for the three-quarters case; three columns simply make it commoner.

#### ⚠️ Only one region shows the live tab, and that is the borrow checker

`conversation_view::draw` takes the pane `&mut`, so a second live copy of one tab is not
something this seam *declines* to draw — it is something it cannot express. A second `agent`
region therefore says so and names what would fix it (Tier 2's per-region tab). Recording it here
because it reads like a limitation and is really a property: the first agent region in
`Region::ALL` order gets the tab, deterministically.

#### The seam, and why invariant #4 is structural here

The whole shipping layout is one `CentralPanel` whose first act is
`ui.available_rect_before_wrap()`, then a `match` on the active pane, then `paint_portal` last in
the same layer. **That `match` — one active pane filling one rect — was the single-column
assumption.** It is now a closure called at most once, and the pane walk chooses where.

🚨 **A console that has had no `/viewport` typed runs the identical code**, not merely equivalent
code: `redraw` compares the layout against `region::Layout::default()` — the value `Console::new`
starts from — and on a match calls the closure with the `CentralPanel`'s own `ui`. No child
`Ui`, no id salt, no clip rect, no separator. And `region_rect(pane, Full)` returns the pane bit
for bit, so nothing about that claim rests on a float comparison.

Three things the split deliberately does **not** touch. The **backdrop** is still rendered once
at the whole pane's size and every region is drawn over the same picture — which is also why a
`viewport` op folds into no look and opens no Tier-4 epoch (§1.12's argument, one level in). The
**portal** is still screen-anchored to the whole pane and floats over everything. And the layout
is **not** written to `preferences.json`, on the posture's rule: a console opens undivided however
it was left, so a stored layout can never make a launch look broken with no command having been
typed.

#### ⚠️ What a split does NOT yet change: where input goes

`term_view` reads the wheel and every key from **raw input**, which §1.2 already records as the
reason the portal needs an explicit rect test rather than egui's layer order. That property does
not become wrong under a split, but it does become *visible*: a wheel anywhere in the window
still reaches the live tab, because nothing tells it which region the pointer is in. The clip
rect on each child `Ui` bounds what is **painted** and what egui's own widgets reach; it does not
bound a reader of raw events.

Nothing in this tier needed to fix that — there is exactly one live tab, so there is no second
consumer for the wheel to be stolen from. It becomes real the moment a region holds something
scrollable, and the mechanism is already built and tested: `block_panel::pointer_inside` and
`portal::pointer_inside` are the two precedents, and a region test is the same shape.

#### 🚨 `panel` — a scrolling stack, and where an Organon panel now lives — Tier A

**`organon-console/src/panel_stack.rs`.** A region holding `panel` holds a **scrolling column of
Organon's own editor panels**, added and removed by verb. James's framing, 2026-08-20: *"instead
of popping up a panel right above us like we do now in the agent panel, we should be able to pop
up panels in one of the viewports we have assigned as a panel … so we could create our own stacks
that would scroll. And that means even if a viewport took up only the top left or top right, we
could still scroll many panels with the same scrolling mechanism."*

🚨 **The stack REMOVES the blocker rather than working around it.** §2's row recorded the
obstacle as *"a third word naming which panel, since two rings cannot say it"* — `/viewport
<region> <content>` already spends both argument rings. A stack dissolves it by splitting the
sentence in two: `/viewport left panel` declares the region and a **different command** puts a
panel in it. Nothing here ever needs three rings, and that is a property of the split, not of a
longer grammar.

📌 **Region size is independent of panel count.** One `egui::ScrollArea` per stack, `auto_shrink`
off on both axes, sized to the region and nothing else — so a top-left corner scrolls twenty
panels exactly as a full-height column does. That is what makes assigning a *small* region worth
doing at all, and it is the property the whole tier exists for.

📌 **`panel_stack::draw` takes its target, and that is a spelling choice made on purpose.**
`fn draw(ui: &mut egui::Ui, region, stack, theme, form, organon)` — it paints into the `Ui` it is
handed, through that `Ui`'s own painter, and never reaches for `ctx`, a named layer or the
window. James's standing note is that *"the entire surface of the console is still itself a 3D
surface that can have physically based rendering applied to it"* (#17), and `egui → texture` is
the half the console does **not** have today, unlike `World → texture` which the backdrop, the
portal and `/surface` all use. 🚨 **Nothing was built toward it** — no offscreen path, no texture
per region, no producer machinery, because a texture and a copy per region per frame is a real
cost for a capability nothing uses. What this buys is only that a stack does not *assume* the
window, so whoever builds #17 has nothing here to unpick. It cost one parameter that was already
going to be there.

#### 🚨 A panel lives ONLY in a stack — the transcript is not a home for one

James, 2026-08-20: *"I don't know if we would ever want to put an Organon panel in the transcript
if we have this new paradigm. Would we ever want a panel inline? A panel should not scroll away.
That doesn't make sense."* The sharp form: **a transcript is a log, and a control is not a log
entry.** A panel is used *while* watching what it changes, and a control that scrolls off
mid-drag was never usable. The inline route was the answer to "where does a panel go" from before
regions existed.

So `/organon <tab> <panel>` targets the stack, and with **no** region holding one it is **refused
by name**, carrying the command that makes one. It does not fall back to the transcript and it
does not silently do nothing — `Ring::Empty`'s rule, at the scale of a verb.

⚠️ **Retired deliberately, not left unreachable.** `Body::Organon`, `OrganonBlock`,
`Transcript::insert_organon` and `organon_element` are **deleted**, and
`conversation_view::draw` no longer takes an `OrganonDraw` at all — the type moved to
`panel_stack`, where the only caller now is. An arm nothing can select is an untested branch
pretending to be a design, which is this file's own phrase. 📌 What made it cheap is in §3's
ledger: the inline panel was recorded as *"reached, not seen"* — **no human has ever looked at
one** — so this retires something never validated rather than removing something known to work.
⚠️ **`/surface` is untouched and is not what this decision is about**: a rendered surface with its
own controls beneath it is an *artifact travelling with its panel*, a different thing that keeps
`Body::Artifact` and `ArtifactContent::Panel`.

#### 🚨 A stack PER REGION — and the two reasons that used to say otherwise — Tier C

✏️ **This subsection said the opposite through Tier A and Tier B.** It is rewritten rather than
annotated, and `panel_stack.rs`'s header with it, because a doc asserting a constraint the code no
longer has is worse than one merely out of date: somebody reads it and declines to do the thing
that already works.

**Reason (a), the mechanical one, is dissolved.** It read: *"the add verb has two rings
(`<action> <panel>`) and no room for a region word, so a per-region stack would give every region
after the first a column nothing could ever put anything into."* Tier C's control makes the
region **context** — a line typed inside a rectangle names that rectangle by being typed there —
and the one dispatch path carries it as an **optional** third word. Every column is fillable, so
no arm is unreachable. ✏️ **Narrowing Tier C did not weaken this**: `add`/`remove` is exactly the
part of that line the dissolution rests on, and it is the part that survived.

**Reason (b) conflated two objects, and only one of them was ever console-wide.** It read: *"two
`panel` regions are two views of one instrument"*, on §1.11's `OrganonPanels` precedent (*"one
mirror per console, not one per element"*). 🚨 **The mirror and the composition are different
things.** The *mirror* — the `PresetValues` a control writes, which is what reaches `Shared` —
stays **one per console** and must: two mirrors would be two claims about one instrument's state.
The *composition* — which panels a column lists — is a property of the **column**, and nothing
ever argued that two columns must list the same panels. Tier A could not tell them apart because
there was only one column.

📌 **Measured, not argued.** On 2026-08-20 James built the four-region layout on a running console
(`left` panel · `topcenter` 3d · `right` panel · `bottomcenter` agent — the first layout ever
written to `layouts.json`), typed one `stack add surface`, and **both** side columns rendered an
identical `organon · look · Surface`. His verdict: *"you can see why we need to have the update for
the stacks because it addressed both of them."* One stack, two views, is now a thing somebody has
looked at, and it is not what a person assembling two control columns means.

⚠️ **Two copies of one panel still share the instrument.** Surface in the left column and Surface
in the right column drive the same parameters through the same mirror; only which panels each
column *holds* differs. `/organon`'s claim — *this is the same panel* — survives verbatim.

⚠️ **A column belongs to the region, not to what the region currently holds.** `Stacks` is an
array over the whole region vocabulary indexed by `Region::slot`, exactly as `Layout`'s own array
is, so `viewport left off` then `viewport left panel` finds the column where it was left. Only
`stack remove` empties one. A column whose region holds nothing is kept and not drawn.

⚠️ **The scroll position was already per-region** and still is; it is now the smaller half of the
story rather than the whole of it, and it is still why the id namespace below carries the region.

**The destination rule, and it is doing real work now.** Three of §1.8's four front doors have **no
region to be typed into**: the CLI's `organon console stack add surface`, an agent's MCP tool, and
a `/organon look surface` typed at a conversation. `panel_stack::Home` answers for all three —
**the first region holding `panel` in `Region::ALL` order**, largest first, the same determinism
that decides which `agent` region gets the live tab. `panel_stack::resolve_target` is the one place
a named region and an unnamed one meet, so the rule cannot be spelled twice and drift. A named
region that does **not** hold `panel` is **refused by name**, never redirected: putting a panel in
a column somebody did not name is §1.3's "refused, not clamped" at the scale of a rectangle.

🚨 **The optional word is what keeps the four doors one vocabulary.** `console.stack` gains a
**third, optional** `region` argument — `ArgKind::Choice` over `region::REGION_WORDS`, the same
table `/viewport`'s first ring is built from, and the slot name is `CMD_REGION`, shared with
`viewport` because both name a region drawn from one list. Optional rather than required, because
requiring it would break the two doors that cannot supply one; and a keyword rather than a bare
third word, because the slash grammar fills optional arguments **by name** (`registry::parse_args`)
and a bare word would make the typed line and the sidecar line disagree. Absent, the wire is
byte-identical to every `stack` line ever written, so a line from an older build still means what
it meant. ⚠️ Half of it is malformed rather than defaulted: `stack add surface region` with no word
after it is skipped, because the caller *did* name a column and the line lost the answer.

#### 🚨 The id namespace: WHERE it is drawn, never WHAT is drawn — the third instance

§1.11 records this bug fixed twice. `organon_element` scoped its widgets by the panel's **slug**,
which separates two *different* panels (something that could never collide) while merging two
elements of the **same** panel. And the typed-value box's key was absolute —
`Id::new("om_value_edit")` plus the param pointer — correct in the editor where a param appears
once, and wrong with two Surface elements over one params instance: clicking one value box opened
a text field in **both**.

A stack is that case from a third direction, and from two at once — **two regions showing one
stack**, and **two copies of one panel inside a stack**. Both fixes say the same thing and it is
now stated as the rule: **an Organon panel's egui namespace is its position on screen, never its
identity.** `panel_stack::draw` pushes `("organon-panel-stack", region.as_word())` and then, per
panel, the entry's **serial** — a number issued once at push and **never reused**, so removing the
third panel cannot hand its open dropdown or half-typed value box to the fourth.

⚠️ **Both pushes happen inside `draw`, not in the caller's `Ui`.** `console_main`'s region walk
*does* salt each child `Ui` with the region word, and if the property rested on that it would be
pinned by a line in another crate that nothing in `organon-console` can see. Pushing it here makes
it testable where it is implemented.

🚨 **The test is a mutation test, not an assertion that happens to hold.**
`two_surface_panels_never_share_a_widget_namespace` draws **four** Surface bodies in one headless
frame — one stack shown by two regions, two copies inside it — under parent `Ui`s given the
**same** id salt so the parent can separate nothing, and asserts that the exact key
`param_sink::value_box` builds (`ui.id().with("om_value_edit").with(<param ptr>)`, with one
pointer standing behind all four) is distinct at every site. `the_region_and_the_serial_are_both_
doing_work` then removes each half of the key in turn and requires the namespaces to collapse.
"They both drew" would have proved nothing.

#### What is in the stack, and what is still an honest line

**Only Look ▸ Surface has a body** — `panels::Status::Live` is Surface and nothing else, and **no
second panel was transplanted here**: §1.11 requires a hand to confirm the first one moves the
picture and nobody has done that. The other twenty-four draw their existing *"named in Organon's
editor but has not been transplanted into the console yet"* line **inside the stack**, so a column
of them is twenty-four named things rather than twenty-four empty boxes.

**The verb is `organon console stack <action> <panel>`** — `add` or `remove`, then a slug —
**two required `Choice` rings, `/viewport`'s shape exactly**. `remove` takes out the **last**
copy of a slug, because the gesture a person means is *undo the one I just added*.

🚨 **Emptying the column is `stack remove all`, and "clear" is deliberately not a third action.**
The slash grammar fills *required* arguments positionally and *optional* ones by keyword
(`registry::parse_args`), so an optional trailing panel would make the typed line `/stack add
panel surface` while the CLI stayed `organon console stack add surface` — one verb with two
spellings, which is the drift this tree spends most of its refusals preventing. Both words are
therefore required, and the emptying word rides the **panel** ring as `all`. That is
`region::CLEAR_WORD`'s own arrangement one module over, on the precedent `console.background`'s
three backdrop *sources* set: a clearing word travelling in the same argument as the real values.
⚠️ `add all` is **refused by name** rather than read as "every panel" — filling a column from a
word somebody typed meaning the opposite is not a convenience.

⚠️ **It is not spelled `panel`.** `/panel` is a *retired* word this console refuses by name, with
a test pinning the refusal, and re-minting it for a different meaning is how somebody comes to
type it expecting the old thing. `stack` also names what the verb actually edits: `panel` says
what a *region* holds.

⚠️ **`Reversal::Permanent`, and the classification is the argued one.** Nothing lands in the
transcript, which is `viewport`'s case for `Recoverable` — but `clear` discards a column somebody
assembled and **no single command rebuilds it**, which is `block`'s case for `Permanent`. The
conservative reading wins, and the practical effect is that autorun can never fire it.

⚠️ **The stack is uncapped, deliberately.** A cap needs a refusal and the refusal could not reach
the person who caused it: `/organon look surface` is answered in a conversation pane one frame
*before* the console applies the push, so a full stack would answer "added" and then quietly not
add. A long column is recoverable with one word; a receipt that lies is not.

#### ⚠️ The wheel over a stack — the consumer §1.14 predicted, arriving

Tier 1 recorded that `term_view` reads the wheel from **raw input** and that this was inert
because there was one live tab; Tier 2b made the `3d` region the second consumer. **A panel stack
is the third, and it is the one Tier 1 named**: *"it becomes real the moment a region holds
something scrollable, and the mechanism is already built and tested."* So every region holding
`panel` joins the portal and the `3d` region in the rectangle list `term_view::draw` tests
against — `portal::pointer_inside_any` over a longer list, never a second gesture vocabulary.

⚠️ **Every panel region, not just the first.** They all show the stack and all of them scroll, so
listing only the one `/organon` names would leave a wheel over the second scrolling text nowhere
near the pointer. ⚠️ The conversation front-end needs nothing: its scrollback is an
`egui::ScrollArea` and the region `Ui`s are clipped, so egui's own hover test already answers.

#### 🚨 A panel column's add/remove control — Tier C, narrowed after use

**`organon-console/src/region_line.rs`.** A region holding `panel` gets a one-line control along
its bottom edge. It takes exactly two words — `panel_stack::STACK_ACTIONS`, i.e. `add` and
`remove` — each followed by a panel name from `panel_stack::panel_words()`, with `remove all`
emptying the column. **A region holding `agent` or `3d`, and a region holding nothing, gets no
line at all.**

##### 🚨 It is literally one line now — #117 Tier 2

✏️ **`BAND_ROWS` was 4 and is 1.** The band drew a candidate row, the box, and two rows for a
note; James, 2026-08-20, having used it: *"completely remove the status lines in the add remove
inputs on the panels. It should just be a single line. If we need to pop something up over top
of it, we can do that, but it's way too messy. We have to clean all this up."*

So the band holds the `TextEdit` and nothing else. The candidate row and the refusal became
`region_line::popover` — a floating `egui::Area` anchored `LEFT_BOTTOM` to the box's own top-left,
so it grows **upwards over** the column rather than out of it. `region_line::overlay` is the pure
rule that decides what it says, and it has exactly one asymmetry:

| state | what floats |
|---|---|
| nothing typed, no focus | **nothing** — the resting state of a panel column |
| focused, words to offer | the candidates, one row, `compact_join` |
| a refusal outstanding | the refusal, **whether or not the line has focus** |

**A success is discoverable; a refusal is news.** Candidates answer a hand already in the box, so
focus is the right gate for them. A refusal answers something that did not happen, and the person
who has clicked away is exactly the one who would otherwise never learn it. A refusal also
outranks the candidates outright, on `command_panel`'s rule — one rectangle cannot say two things
at once, and an edit clears the note, so the candidates return the moment you type.

🚨 **The third row went too: `console_main` no longer writes an accepted dispatch back to the
line.** `console.stack — {"accepted":"stack add surface region left"}` restated a line still on
screen and announced a panel that had just appeared above it. The write is now gated on
`receipt.ok`, so a **refusal** from dispatch still reaches the box that produced it — that is the
only route by which one can.

⚠️ **The hint text changed with the row that used to carry the words**: `> add | remove`, not
`> add or remove a panel in this column`. A sentence describing the control has to earn its width
against the words you would actually type, and against a person who built the console.

**`add surface` in a panel column does what `organon console stack add surface` does**, in that
column. The line expands onto `/stack add surface` and `Registry::resolve` validates it, so the
control and the CLI stay one vocabulary with one set of refusals — but the control is **not a
front door onto the table** (§1.8): the two words are all it takes.

##### ✏️ This REVERSES what Tier C shipped, and the reversal is recorded rather than quietly applied

#98 Tier C shipped a command line in **every** region, dispatching the **whole** console registry,
under the rule **prune discovery, never capability**: the band listed a region's own verbs and
`region_line::act` accepted every verb in the catalog, so `/theme dark` typed into a panel column
ran. It had a test defending exactly that and a review round behind it. **It no longer runs, by
design.**

James used it and rejected the scope: *"I don't want every region to have commands like this. At
least that wasn't my original intention. … What I envisioned is not that I want to be able to
have slash commands to set each region to be what it could be. I only particularly wanted to be
able to add and remove panels from a panel section."*

⚠️ **The old rule is not wrong; it is about a different object, and that is the whole
reconciliation.** "Prune discovery, never capability" guards a **general command surface** against
becoming a jail: a surface that offers ten verbs and secretly refuses the eleventh has taught the
person using it that its list is a lie, and loosening a refusal later is far harder than widening
an offer. This is no longer a general command surface. It is a **dedicated control for one job** —
the way a scrollbar is not a crippled command line and a volume knob is not a pruned synthesiser.
A control that does one thing is not a pruned version of a thing that does everything, so the rule
does not reach it: there is no hidden capability to discover, and the list **is** the vocabulary.

⚠️ **What was given up, said plainly.** `/theme dark`, `/posture`, `/background` and every other
console verb no longer run in a region. Assigning a region is `/viewport <region> panel` at an
agent, or `organon console viewport …` from a terminal — which is what it was before Tier C. An
unknown first word is refused **by name**, and the refusal says what the control does take and
where the rest of the vocabulary lives. Nothing was made unreachable; it was made un-typeable
*here*.

⚠️ **What went with it, so nobody looks for it**: the `Shed` table and the `Context::content`
field (there is no content to branch on when only one kind gets a line), the `RegionPalette::
elsewhere` sentence (nothing is pruned any more, so there is nothing for it to name), the
view-lane refusal (subsumed by the one refusal), and the `dismissed` latch with its Escape
handling — the two words are the control's own label and are always shown, so there is no band to
shut. **Escape is now left alone on purpose**, which means egui's `TextEdit` does what it does
everywhere else with it: surrender focus, handing the composer its keys back.

##### 🚨 The defect this also fixed: egui gave the box a new id every time the palette opened

James, on the shipped Tier C build: *"When I type slash in one of the regions, I just get a list
of choices and some text below it, and I can't type or select anything."*

**The cause was `egui::TextEdit`'s id, and it had nothing to do with key consumption.** The box
was added with no explicit id, so egui derived one from `Ui::next_auto_id` — a counter over the
widgets allocated before it. The palette's rows are drawn *above* the box, so the instant a `/`
opened the band two more labels existed ahead of it, the box's id changed, egui saw a **different
widget**, and focus was stranded on an id nothing drew any more. The first keystroke landing and
none of the rest is the signature of that, and it is what a headless probe measured. `BOX_ID` is
the fix: an explicit salt, so the box is the same widget whatever is drawn around it.

⚠️ **A second, independent defect was measured on the way, and stopping at the first would have
left it.** In a 320 pt side column the old band's `elsewhere` sentence wrapped to several rows and
pushed the box **35.6 pt past the band's own clip rect** (measured: content `529.5 → 635.6` in a
band ending at `600.0`) — invisible and unclickable even with a stable id. Two faults, one
symptom.

✏️ **The fix was the row order, and #117 replaced it rather than inheriting it.** That paragraph
read: *candidate row, then the box, then the note* — the note last because it is the only
unbounded row, so an overflow costs the tail of an explanation and never the input. The reasoning
was right and there are no other rows now.

🚨 **What holds the guarantee up today.** The band holds exactly one widget and it is a
*single-line* `TextEdit`, which does not wrap — so its height is one row whatever is typed, and
nothing above it can move it. Everything unbounded is in a floating `Area`, which allocates
nothing in the band's `Ui` and therefore cannot displace the box at any height. **The property is
structural where it used to be ordinal — and structural only while nothing else is put in the
band.** `BAND_ROWS`' doc carries that rule for whoever next wants a row.
`the_box_stays_inside_the_band_however_long_the_refusal_is` drives a real refusal at 320 pt and
asserts `band.contains_rect(box)`; **mutation-measured**, drawing the note as a label in the band
puts the box at `y 657.9…673.0` against a band of `574.9…600.0` — **73 pt outside**, twice #112's
original, because a one-row band has less slack to absorb a wrap than a four-row one did.

⚠️ **The mutation table is in `a_note_appearing_does_not_take_the_box_with_it`'s doc, and it
corrects the obvious assumption**: on today's code, deleting `BOX_ID` does **not** fail the suite,
because nothing is drawn in the band above the box at all and the widget count never varies. Only
"salt deleted **and** note moved into the band above the box" fails. Both fixes are kept because
each covers the other's gap — and that test still *passes* under the mutation the geometry test
catches, which is why there are two.

##### 🚨 The hard part is still focus, not parsing

`conversation_view::composer_keys` consumes Tab and the arrows out of the **raw event list**, not
out of a focused widget — and two of them unconditionally: `arrow_owner` hands Up to the history
whenever the composer is empty. That was safe while the console had exactly one command input.

**The owner is measured, never asserted.** `region_line::Lines::owner` is the region whose line had
egui focus **last frame**, recorded by `region_line::draw` from the `TextEdit`'s own
`Response::has_focus` — the same fact `composer_box` already reads to decide whether Enter sends.
Nothing invents a focus state and nothing fights egui's: clicking a box moves focus, egui
guarantees at most one focused widget, and this only *observes* which. `Lines::composer_owns_keys`
is the one value `console_main` hands `ConversationPane::set_keys`, and `composer` returns early
from both key readers when it is false.

⚠️ **One frame behind, deliberately and boundedly.** The walk visits regions in `Region::ALL`
order, so a line drawn *after* the agent region cannot tell the composer anything before the
composer has read the frame's keys. Both sides therefore gate on the previous frame's
measurement — the same one-frame-behind arrangement the `3d` region's rectangle already uses. The
cost is that the frame on which focus *moves* is arbitrated by the old owner, and that frame
carries no keystroke: **focus moves by a click, and a click is not a key.**

⚠️ **`Lines::begin` is called every frame whether or not any line draws**, which is what hands the
keys back when the last line goes away. A latch cleared only by an explicit blur would leave the
composer's keys held by a rectangle that no longer exists.

⚠️ **A console with no panel column has no region line at all**, so invariant #4 holds by
construction: the default console — one region, holding the agent — draws nothing here, and the
composer behaves exactly as it did before this module existed.

##### 📌 What an empty column and a vacant region show

An empty **panel column** names the two words its own control takes (*"panel — an empty column.
Type `add surface` in the line below; `remove all` empties it again"*), and the control's box says
the same thing in its hint on an empty line — a control that does exactly two things should say
which two the moment you look at it.

✏️ **A vacant region's notice went back to naming the other doors**, because it no longer has a
line of its own to point at: *"empty — `/viewport <region> panel` at an agent, or `organon console
viewport …` from a terminal, fills it"*. Tier C's version named a control that is no longer drawn
there, which is exactly the status line that cannot be right — the two arms it used to have (one
for "a line was drawn", one for "too short") collapse back into one.

##### ⚠️ What the control deliberately does NOT do

**No self-completion and no autorun.** Both are §1.9 rules of the composer's and both rest on
`completion_held` — the latch that keeps a completion from undoing a backspace on the frame it
happens, read off a shadow copy taken at the top of the frame. Reproducing them here without that
measurement would reintroduce the worst defect the command panel has had (*"once I have typed
slash surface, I am no longer able to backspace out of it"*). **Tab accepts, Enter runs**, which is
the pair §1.9 says must never be one key, and every candidate answers `fires: false`.

**No history.** Up and Down move the highlight.

⚠️ **The leading slash is tolerated, not required.** `add surface` is the natural thing to type at
a control; but Tier C shipped a slash line and taught one, so `/add surface` still works — one
grammar with an optional ornament, rather than a refusal aimed at the only spelling anybody has in
their fingers.

##### The lane, and the row

Full console lane. `region_line::Act::Run` carries a catalog name and arguments and `console_main`
hands them to the **same** `ConsoleDispatch` a `/`-command in the composer reaches, which writes
the console's sidecar, which `drain_console` drains next frame through the real `CommandService`.
A line typed in a column leaves a `CommandRun` record exactly as one typed in the composer does,
and the receipt says **accepted**, never applied.

⚠️ **The column this control lives in is supplied, never offered — and never forced.**
`panel_stack::REGION_ARG` is dropped from the ring, because offering it inside a control whose
premise is that it edits *this* column would invite a second, contradicting one. It is still
*accepted* if typed in full: `add surface region right` typed in the `left` column reaches
`right`, because the caller named it. The supplied word only ever fills an empty slot.

⚠️ **The band's row is `conversation_view`'s own.** `compact_join` was split out of `compact_line`
so a second *producer* of words gets the same fitting rule, the same ` | ` separator and the same
`+N` count. Two producers, one row. ⚠️ The band is a **fixed four rows** — candidate row, box, and
two for a note — rather than a measured one: a note longer than that is clipped by the region's
own clip rect, which the row order makes survivable.
#### What Tier A leaves to B, C and D

✏️ **Three columns landed, and so did Tier C — then Tier C was cut back to a two-word control**;
the two subsections above own both. What survives of it is the half James asked for: a per-region
stack is addressable from the column it acts on. ⚠️ **What it no longer answers is "what does an
unassigned region show"** — a vacant region has no line again and its notice names the composer
and the CLI instead, which is a smaller answer and an honest one. **A tab per agent region** is
Tier D, still blocked on the borrow and nothing else, and Tier C never touched it: an `agent`
region gets no line, now because only a `panel` region does and formerly because it already had a
composer. Saved layouts landed early (§1.15); animated transitions and drag-to-resize stay after Tier D,
for §1.14's own reason — and Tier B sharpened one of them: a divider a hand can move is now a change
to *two* cuts whose positions come from a constant, so it is the constant that would become state,
not a ratio that already exists.

#### The lane

Full console lane, not the view lane — `/organon`'s shape would not do, because Tier 2 must
change `engine_plan`, which lives in the root crate while `organon-console` is the lower one. So:
`CommandSpec` in `console_specs()` → `ConsoleOp::Viewport { region, content }` → the
`viewport <region> <content>` sidecar line → clap's `Viewport` subcommand → `spec_name`/`op_from`
→ `Console::set_viewport`. Both arguments are `ArgKind::Choice` built from `region.rs`'s own two
tables, so the MCP schema, the slash palette's two rings, the CLI's `--help` and tab completion
are four renderings of one vocabulary.

⚠️ **`off` is a content *word* and not a content *kind*.** It empties a region; no region holds
it, and giving `Content` such a variant would put a value in the enum the draw path must then
match and refuse to draw. The precedent for a clearing word riding the same argument as the real
values is `console.background`, whose `Choice` carries the three backdrop *sources* beside the
materials.

🚨 **`set_viewport` is the only gate on an assignment, and it has to be.** clap restricts both
words and `op_from` resolves them again, but neither can answer the question that decides the
command: *may this region hold this, given what the console is holding right now?* Overlap, the
last agent and "there is nothing there to clear" are facts about the **current layout**, which
lives on `Console` and nowhere else — and the lane gets no answer back, so a caller cannot read
it before writing. Every refusal is therefore spoken at the console end, by name.

### 1.15 Saved layouts — an arrangement with a name, and the load that refuses rather than half-applies

**`organon-console/src/layout.rs`**, plus `Layout::from_placements` in `region.rs` and
`Console::set_layout` in `console_main.rs`. `organon console layout <save|load|delete> <name>` —
`/layout save desk` in a composer — writes the console's whole arrangement down under a name and
brings it back. The listing is a separate verb, `/layout.list`, and the next heading says why.

#### 🚨 Why this is not a convenience

`doc/organon_is_the_product.md` §4 is the reframe this is built against: **a layout is the unit of
product identity.** "Claude Code Desktop", "Organon standalone" and "an LLM visualiser" become
*named arrangements of one program* rather than three programs. What is written to disk here is
not a window position; it is what somebody means when they say which program they are running.

⚠️ **That document is a proposal, not a ratified change.** Nothing here touches `Edition`, no
edition is collapsed, and no preset is named — see "the library ships empty" below. The mechanism
is worth having either way; the naming is not this tier's to do.

#### 🚨 A load is a TRANSACTION, and that constraint decided the whole design

§4 again, and it is not optional: *"a layout that cannot be drawn must say so and leave the
current one standing, never half-apply."* A saved arrangement is an assignment that arrives **all
at once, from a file, possibly written by someone else** — so it needs every refusal `/viewport`
already has, and one more.

`layout::resolve` validates the whole thing and answers **either one finished `region::Layout` or
one sentence**. `Console::set_layout` then applies it in a *single assignment*. There is no loop
over placements and there must never be one: "never half-apply" is a property of the signature
rather than discipline at the call site, and a partial apply that had evicted the last `agent`
region is a console with nothing to type into — recoverable only by a verb typed at an agent.

Eight checks, in the order a reader meets the file:

| Refused | Because |
|---|---|
| a region word this build does not have | ⚠️ refused **whole**, never dropped — an arrangement missing a region is not the one that was saved, and nothing on screen would say so |
| a content word this build does not have | the same rule, stated the same way |
| `off` stored as what a region holds | it is what a *command* says to **empty** a region; a region holding nothing is simply absent from the file. Its own sentence, because "not a content" is a poor answer to a word the neighbouring verb takes |
| two regions whose cell sets intersect | the grid cannot be drawn, and **neither region asked**, so neither can be the one that gives way |
| two regions holding `3d` | `Content::only_one_because`, quoted — the limit is Organon's, not the idea of a viewport's |
| nothing holding `agent` | §1.14's last-agent invariant, met from the other end |
| the pane is too **narrow** to seat the fixed side columns, and the layout uses a column word | `MIN_COLUMNS_WIDTH` (688 pt) is quoted with the region that needs the cut — and the sentence points at rows, which need none and work at any width |
| some region would be under `MIN_SIDE` **right now** | the pane it was measured against is named, and the sentence says the layout is fine and the window is not |

📌 **The last two are facts about the window rather than about the file**, which is why they are
checked only when a pane has been measured (`Console::pane_points`, one frame behind by
construction) and why the same file loads once the window is bigger.

🚨 **They are two rules, not one, and telling them apart is a should-fix this tier earned in
review.** `plan` returns `None` for either — and the first version of this refusal quoted only
`MIN_SIDE`, so a three-column arrangement against a 500-point pane was told *"every region needs
48 on a side"*: true, irrelevant, and misleading about what to do. The coarser rule is asked
first, the sentence names the threshold that actually tripped, and the predicate deciding it is
`Region::needs_column_cut` — extracted out of `region_rect` so the explanation and the geometry
cannot come to disagree. ⚠️ Both can be true at once; the column rule wins, because widening is
what makes the layout expressible at all and the height refusal is still there afterwards if it
applies. One refusal at a time, each of them true.

⚠️ **The edge case that fix could have missed is pinned rather than argued.** `plan` measures the
*vacant* regions it fills in as well as the occupied ones, while `resolve` inspects only what the
layout holds — so a column-shaped **gap** would fail the plan with no occupied region needing a
cut, and the refusal would fall through to the wrong sentence. Today that is unreachable (the only
regions needing no cut are `full`, `top` and `bottom`, and a layout built from those leaves a
filler that is itself `top`, `bottom` or nothing) — but that is a fact about *this* grid, and
Tier B is the proof that the grid moves. So the property is a test stated as a **counterfactual**:
whenever the answer is the `MIN_SIDE` refusal, widening the pane must not rescue the layout,
because if it did the width was the real reason. ⚠️ It is mutation-checked — reverting the split
makes it fail, naming the case (`Top`/`BottomLeft` at 80×400) rather than merely going red. The draw path's own *"the
window is too small for this layout"* sentence remains the backstop for every other route.

✏️ **The first two checks stopped being hypothetical during this tier's own review.** #98 Tier B
landed while it was open and made `topcenter` a real region — so the word this module's test used
as its example of *"a region a newer build has and this one does not"* resolves now. The test was
re-pointed at `middleleft` (what a future 3×3 would call its middle row) and gained the other
half: a file naming `topcenter` **loads** here, because this build has the word. Both directions
of the forward-compatibility story are therefore measured rather than argued, by an event nobody
staged.

🚨 **`Layout::from_placements` is where the rules live, not `layout.rs`.** It is the whole-layout
counterpart of `Layout::assign`, and it exists because a set of placements has **no order**: there
is no "asked", so an overlap inside it is a contradiction rather than a displacement. Containment
is the case worth naming — `full` beside `left` *displaces* when a command asks for one of them,
and is simply refused when both arrive in a file. Reusing `Refusal` would have meant inventing an
`asked` region out of iteration order, which is a guess about which half of a contradiction was
meant, so the fault type is its own (`LayoutFault`). Disjointness is still `Region::cells`,
uniqueness is still `Content::only_one_because` and the agent rule is still `Layout::has_agent` —
one implementation, two doors. ✏️ **That accessor was `quadrants` over four bits when this was
written and became `cells` over six under #98 Tier B, and nothing else here moved** — which is
the payoff of reading the geometry off one function instead of restating it.

#### 🚨 Three actions, and why the listing is a verb of its own

`registry::parse_args` fills **required** arguments positionally and **optional** ones by
keyword. So a verb whose name argument were optional would be typed `/layout save name mine`
while the CLI stayed `console layout save mine` — one verb with two spellings, which is the drift
this tree spends its refusals preventing. `panel_stack::StackCmd` hit exactly this and is why
`/stack`'s two rings are both required.

Both words here are therefore required, which settles `save` / `load` / `delete` — all three name
a layout — and rules `list` out: a name is not a thing a listing takes, and there is no honest
word for that slot. `all` works in the panel ring because `all` genuinely names a value there;
no layout name means "every layout".

So the listing is **`console.layout.list`**, on the precedent `console.camera.read` set: a *read*
is answered in-process, because `organon console …` is fire-and-forget with no return path.
⚠️ **It differs from the camera's read in one way, stated rather than glossed:** the library is a
*file*, so this read needs nothing from the running console and a CLI could answer it out of the
same file. It does not today, because the CLI has no spelling for a dotted verb and inventing a
second one (`console layouts`) would be one verb with two names. The gap is real and small:
`layouts.json` is legible by design.

⚠️ **`Reversal::Permanent`, and each action earns it separately.** `delete` takes a layout out of
a file and nothing puts it back. `save` replaces what was stored under that name and nothing
rebuilds it. `load` is the one worth arguing: it puts nothing in the transcript, which is
`viewport`'s whole case for `Recoverable` — but what it *displaces* is the arrangement on screen,
and no second command restores that unless it too was saved. `/viewport full agent` returns to the
**default**, not to what you had. "Only if you had already saved it" is not the same as yes, so
the conservative reading wins and autorun can never fire the verb — the right outcome for one
that writes to a file.

#### The file, and whose precedent it follows

`layouts.json` at the store root, beside `harnesses.json` and `preferences.json` — one resolver
(`SessionLog::store_root`), reused, never re-derived.

`harness.rs` is the precedent §4 names — *built-ins seeded in code, a user's file merged over them
by id, serde defaults, unknown fields tolerated* — and this follows the **discipline** rather than
the literal shape: an object with a `layouts` array has room to grow a top-level key, which a bare
array has not. Three properties come from `prefs.rs` instead, because unlike `harnesses.json` this
file is one the product **writes**:

1. **A write cannot destroy what is stored** — temp file in the *same directory*, then rename.
2. **Reading is total** — missing, unreadable or malformed is "nothing saved". A corrupt library
   must cost you your layouts, never your console. ⚠️ Never a BOM: `serde_json` refuses one
   outright and a total reader turns that refusal into silence.
3. 🚨 **Unknown fields survive a round trip, which is stronger than tolerating them.**
   `harnesses.json` may tolerate-and-drop because nothing writes it back; `save` and `delete`
   rewrite this file whole, so dropping would silently strip a newer console's fields off every
   layout in the library — including the ones this build never touched.

📌 **No environment variable overrides any of it**, on `prefs.rs`'s stated rule: a variable baked
into a launch shim years ago silently outranking the file is indistinguishable from the
evaporation a stored choice exists to end.

⚠️ **The library is re-read per command rather than held in memory.** It is small and it is the
truth; a copy cached at startup would fight a hand-edited file and win silently. The cost is
stated: two consoles saving at once resolve as last-writer-wins — never a torn file, but the
loser's layout is simply not there.

#### 🚨 The library ships EMPTY, and that is the scope rather than an omission

`builtin()` returns nothing. Naming the presets — "desktop", "standalone", "mind" — is **James's
call**, and a preset nobody has looked at on a screen is worse than none: it would be a shape the
product asserts is good, arrived at by a machine that cannot see it. The **seam** is built and
tested — `merge_over` and `save_over` both take the built-ins as a parameter, `builtin_for`'s
arrangement, precisely so the merge and the don't-write-an-unchanged-built-in rule are exercised
against a seeded list rather than against nothing.

#### What a layout does NOT record

⚠️ **The panel stack.** A saved arrangement records that a region holds `panel`, never which
panels are in the column. The stack is documented as not remembered across a launch (§1.14, Tier
A), and changing that is a decision about the stack rather than about layouts. Also absent, and
for the same "it is not part of the arrangement" reason: the theme, the posture, the screen state,
the tabs and the camera.

⚠️ **Tier C made that gap MORE visible without changing it, and the omission is knowing.** With
one console-wide stack, restoring `organon` — James's own saved layout, `left` panel · `topcenter`
3d · `right` panel · `bottomcenter` agent — brought back two rectangles showing one column that
was itself empty at launch. With a column per region it brings back **two empty columns**, and the
work of refilling them is now per-column rather than shared. That is a stronger case for
recording composition than existed before, and it is still a decision about the *stack*: what a
layout would have to carry is a list of slugs per region, which is content rather than
arrangement, and the first question it raises — does loading a layout replace a column somebody
has assembled, or refuse — is `set_layout`'s transaction argument all over again at a smaller
scale. Not built, deliberately, and recorded here so nobody reads the empty columns as a
regression.

#### The lane

`CommandSpec` in `console_specs()` → `ConsoleOp::Layout { action, name }` → the
`layout <action> <name>` sidecar line → clap's `Layout` subcommand → `spec_name`/`op_from` →
`Console::set_layout`. The action ring is `ArgKind::Choice` over `layout::LAYOUT_ACTIONS`; the
name ring is `ArgKind::Text` — **the first console verb whose two rings are not both closed
lists**, because a layout's name is whatever a person called it.

🚨 **The name is checked at the clap boundary and again at dispatch, and that is a real gate
rather than a belt.** `ArgKind::Text` states no value space, so `validate_args` has nothing to
check — and the sidecar line is whitespace-delimited, so `layout save my desk` would arrive at the
console as `layout save my` with the rest silently dropped, having saved something nobody named.
`check_name` refuses whitespace, control characters, an empty name and anything over 64
characters. ⚠️ Matching is **exact**: `Desk` and `desk` are two layouts, and the refusal for an
unknown name lists every name that exists, which is what makes a case mismatch visible rather
than silently folded. ✏️ **That list backticks each name, and the reason came out of review:** a
comma is a legal name character — `check_name` refuses whitespace because the *wire* cannot carry
it, and deliberately refuses nothing else about what somebody may call their arrangement — so a
bare comma-join would render a library holding `a,b` and `c` as `a,b, c`, indistinguishable from
three layouts. The fix belongs in the joining rather than in `check_name`: narrowing the names a
person may choose to suit a separator would be a rule about the *display* leaking into the data.

#### ✏️ The names ARE a ring now — for `load` and `delete`, and never for `save`

📌 **The deferral above was "nobody has measured it", so the measurement is what closed it.**
`registry::layout_options` is a `NarrowFn` on `console.layout`: `/layout load ` and
`/layout delete ` complete to the layouts that exist, each option carrying what choosing it
holds (`left agent, right panel`), and an unknown name is refused **in the composer**, while the
words are still there to be edited, instead of after a round trip.

🚨 **`save` is not narrowed, and that asymmetry is the whole reason the ring is chosen by the
*action* rather than declared on the *argument*.** `load` and `delete` name something that must
already exist, so the library is their value space. `save` takes a name a person is **inventing**
— offering the existing names there would read as a closed list, and `coerce` would then refuse
every new name in the world, which is the one thing `save` is for. So the hook reads the action
word and answers `None` for `Save` **before the store is touched at all**, and the declared
`ArgKind::Text` answers as usual.

⚠️ **The declared kind stays `Text` and must.** It is what the MCP schema and `/help` are
generated from, and neither has the action word in hand — one value list per argument is all a
schema has. Same rule as `/organon`'s declared union being wider than any one ring.

**What it costs, measured before it was wired** (`layout::tests::library_read_cost`, release,
organon-one, 2026-08-20 — the instrument is in the tree so the numbers can be re-taken rather
than believed):

| layouts | read + parse | one `stat` | cached | per call (n+1 asks) |
|---|---|---|---|---|
| 1 | 24.2 µs | 12.0 µs | 0.08 µs | 0.05 ms → 0.000 ms |
| 10 | 24.5 µs | 12.1 µs | 0.05 µs | 0.27 ms → 0.001 ms |
| 100 | 100.2 µs | 12.6 µs | 0.05 µs | **10.1 ms** → 0.005 ms |

Medians of three runs; the spread is 23.1–27.3 / 24.0–27.1 / 98.8–101.4 µs. ⚠️ **A fourth run,
taken first and while other builds were on the machine, is kept here as the warning rather than
discarded**: it reported 75.2 µs at n=1 against 43.6 µs at n=10 — a 138-byte file costing more
than a 1182-byte one — and put n=100 at 16.4 ms, i.e. *over* frame budget. The impossible ordering
is the tell that the run measured contention, not the file. **Distrust any run whose n=1 is not
the cheapest row.** 📌 The real library here is **197 bytes, one layout, four regions**, so what
ships today sits between the first two rows; the hundred-layout row is the one the cache is for.

🚨 **Two things in that table changed the design.** The first is the **n + 1**:
`value_candidates` asks the ring once and then calls `settled` per candidate, and `settled` runs
`resolve` → `coerce`, which asks the same hook again — so a library of a hundred is a hundred and
one reads. The second is *when*: `ConversationPane::palette` is called while the composer band is
**drawn**, so this is per **frame**, not per keystroke. Straight from disk that is 10.1 ms against
a 16.7 ms frame, and it would be spent while somebody is typing. ⚠️ **A `stat`-per-call cache is
not the fix either** — 12 µs × 101 is still 1.2 ms, several times a frame.

So the ring goes through `Library::for_completion`, which holds one `Arc<Library>` keyed on the
store root. ⚠️ **What invalidates it, both halves needed:** `Library::save_over` — the one path
every write takes, which is why a `delete` (a `remove` and then a rewrite) needs no second call
site — **forgets it outright**, so a name just saved is in the ring on the very next frame and a
name just deleted is gone from it. Everything else (a hand-edited file, a second console) is
covered by a **200 ms TTL**, which is the answer to `console.layout.list`'s standing objection
that *"a cached copy would fight a hand-edited one and win silently"*: it can only win for a fifth
of a second, and the two **reads** — the listing and `set_layout`'s own load — deliberately do not
use it at all. They are asked once, by a person, and for them the file is simply the truth.

⚠️ **The empty library is `Ring::Empty` carrying `layout::NOTHING_SAVED`** — the same sentence
`console.layout.list` answers an empty library with, moved into `layout` so both surfaces read one
string rather than each keeping its own. A band with nothing in it is indistinguishable from a
broken one; that is what `Ring::Empty` exists for. ⚠️ And **no data directory answers `None`, not
`Empty`** — telling somebody whose layouts are merely *unreachable* that they have saved nothing
is the mistake the listing already refuses to make.

⚠️ **A comma survives.** Each option is its own `(label, doc)` pair all the way to the popup row,
so the names are never joined; the only place they meet a separator is `coerce`'s refusal, which
joins with ` | ` — unambiguous precisely because `check_name` makes whitespace impossible in a
name. And matching stays exact: `Desk` and `desk` are two options, shown as two, because folding
them would suggest a name that then fails to load.

## 2. Seams the next tiers consume

| Coming | Builds on | Issue |
|---|---|---|
| Viewport interaction + provenance (T2+) | T1's pane (`console_main.rs::ScenePane` + `app.rs::SceneView`); camera input rides `scene_input`'s region pattern — never a second gesture vocabulary. The world gate is already `any(mind, shell)`; `World` stays unforked (#618 owns its extraction) | Console #6 |
| Content-addressed artifact store + lifecycle UI + evidence viewers | `session::Artifact` (metadata landed in #4 T1); payloads beside the log in the session dir | Console #4 T2+ |
| Rich media — **placement and promotion**, the gallery, and the expensive kinds | **§1.13 landed T4**: two media kinds on the one `Kind` vocabulary, `/media` on the view lane, off-thread reads, `Failed` as its own state, and the surfaces' own eviction policy made generic over its key. What is left is #56 T5 (the ladder *inline / docked / full screen*, on T3's animator) and T7's expensive kinds — audio, PDF, LaTeX, video, each behind an opt-in feature on the `--with-llm` precedent. ⚠️ **Two things this tier deliberately did not decide.** *How an exhibit reaches the console* is still open and still picks neither of #56's two options — a path comes from a typed `/media` line and nothing else, which is why the verb is absent from the MCP catalog; anything that changes that is changing a security property, not adding a convenience. And a multi-item exhibit currently draws its items **stacked**, because a scroller or a tap-to-maximise grid is a *placement* decision and placement is T5. ⚠️ The terminal patch placement of a media kind is a notice, not a picture — §1.13 says what painting one there would actually cost | #56 T5, T7 |
| Command service T2+: core_catalog seeding + real targets | `command::CommandService` landed in #5 T1 (dispatch + catalog + the every-dispatch-leaves-a-record invariant) and is **live in the product since Console Spike T2** (`console.background` / `console.rig`, seeded from `substrate_materials`' tables, dispatched from the frame path). T2+ adds the bin-side `core_catalog`→`CommandSpec` adapter, the runtime target over the CLI override lane + snap request/reply sidecar, and the policy engine that makes `Denied`/`Requested` real — never a second vocabulary | Console #5 |
| Conversation view milestone 2 | Milestone 1 landed the whole path (decoder → `agent_map` → `conversation` → `conversation_view`, one live child per tab), the inline artifact (`Body::Artifact`) and the rendered surface it drives (`/surface`). `/panel` has since been deleted — it drove the console backdrop, which a conversation cannot show. Next: the **agent** summoning one, via a tool call the integrator answers with `Transcript::insert_artifact`, with the tool card as the anchor. ✏️ Subagent events rendered *inside* the tool card that spawned them has since **landed**, and so has ✏️ `tool_use_result` (the undocumented structured per-tool detail a rich card wants — four measured fields, no more). Then, in the order §5.9.3 holds them: `Notice`/`post_turn_summary` and `RateLimit` rendered into the flow rather than only read for facts, and **thinking blocks**, which are decoded and drawn nowhere and are waiting on a capture that contains one; then Pi as the second harness, mapped onto the same nine transcript events — never a second event vocabulary | Console Spike §5.9 |
| Approvals, next steps | The card, the in-process MCP-over-HTTP server and the session-scoped decision memory landed together (§1.1, "The approval card"). Next, in order of what a session actually costs: 🚨 **`system/permission_denied` carrying `decision_reason_type: "mode"` rendered as its own thing** rather than as a generic red tool error — the band now says a non-default mode may be silencing approvals, but the individual refusal it causes still looks like an ordinary tool failure, and that line is the only place a human learns *which of their clicks* caused it; the console's own verbs are now **served** as capability tools (`Capabilities` handed down, `ConsoleDispatch` onto the audited drain, plus the one in-process read §1.3 adds) so a card can say *"organon · background"* instead of a shell command — but nothing has called one yet, and **§7's withholding property has not been re-measured against a server that serves them**, which is the first thing to read off a live run; then a memory that survives the tab, with the audit trail a durable one obliges | `doc/console_approval_protocol.md` · `doc/console_session_control_protocol.md` §10 |
| The portal's other states | §1.2 landed the portal itself and §1.3 its camera; **immersive, full screen and the animated grow are still unbuilt**, in James's own order. ⚠️ **"Immersive is nearly free" is the one claim in the recon that does NOT survive contact, and the correction matters before anyone scopes it.** The recon reads immersive as the existing backdrop, which is true of the *rendering* and false of the *painting*: `paint_portal` paints the portal **over** the front-end (that is what floating means), and immersive needs it **under** the glyphs with the scrim over it — and the scrim lives inside `term_view::draw`'s `Some(bands)` arm, fed from the epoch ledger. So immersive is a **new integration** (a single-band `BandedBackdrop` carrying the portal's texture, and deliberately *not* opening a look epoch, or the first screenful is striped), not a variant added to `portal::step`. It is also a terminal-tab-only route as things stand: the conversation front-end has no backdrop path at all. Then **full screen**, genuinely new (no path suppresses the tab strip, the glyph grid or the scrim), then the **animated grow** between the three rects. ⚠️ **This "full screen" is the PORTAL's, and it is not what §1.12 landed** — that is the *window* covering the display, which suppresses nothing inside it and shares no code with this. The two are independent and compose: a full-screen portal inside a full-screen window is the state this row is ultimately reaching for. §1.12's verb is named `console screen` rather than `console fullscreen` precisely so this row keeps the phrase. Three things must land with them and are already argued: `scene_viewport` widened by a `Sense` parameter (clicks in Portal, drag-only in Immersive — never a second `ui.interact` on the same rect); **Escape consumed state-conditionally** (`consume_key` `retain`s out of the same `i.events` vector `term_view` clones — the console's first state-dependent key ownership, and the new states are exactly the ones that need it); and the allocation rule for the animation — **allocate at the destination size, scale the quad, reallocate once on settle**, because a size change today is free + realloc + re-register + one unconditional log line, i.e. ~15 of each per 250 ms transition. That same settle rule closes the window-resize-drag churn with it. ✏️ **Tier 2b re-homes half of this row.** `SceneMode::Immersive` now clearly belongs *here* rather than being a spare variant — §1.2's viewport/presentation split is what makes "immersive" a third presentation of the one mechanism instead of a fourth thing, so the work is a `SceneMode` value at a call site plus the `BandedBackdrop` integration above, and `paint_viewport` already takes the mode. 🚨 **And the row gains a second, larger future James named explicitly: the EXTERNAL-PROCESS portal** — opening a portal launches a **separate process in its own window**, exactly as Organon's visual already works. Four facts, recorded so whoever picks it up does not rediscover them: `spawn_visual()` probes for the `organic-math-visual` binary by **file name** (which is why `CLAUDE.md` forbids renaming that one and permits renaming the front-of-house binaries); the two processes talk over the **`Shared` mmap**; `ipc.rs::ns_file` namespaces every channel so a Console session and an Organon session coexist; and **`$ORGANON_IPC_NS` is the runtime override** that lets one visual binary serve any edition — `term.rs:195` already injects it into every tab the console spawns. ⚠️ **Deferred deliberately and nothing was built toward it** — James: *"Portals will come after we get the viewports working."* No stub, no arm, and today's portal was **not** changed in anticipation of it. ⚠️ Note the ordering it implies: an external portal is a *second producer process*, so it lands on §1.14's producer seam ("a producer yields a texture the console can sample, at a size the console asks for") rather than on `portal::step` | Console Spike §5.9 · `doc/console_portal_recon.md` — the site-by-site investigation these follow from, now merged, carrying this correction as its own §1.1 amendment so the two cannot drift apart |
| A **read** path for the console's own state | **The camera half has landed, on the MCP lane only** — `console.camera.read` (§1.3, "Reading it back"), answered in-process from the viewpoint `redraw` publishes. What is left is the *other* transport and the *other* verbs. `organon console …` is still fire-and-forget with no return path, so the CLI reads nothing; the honest fix there is the request/reply sidecar §5.9.25 already names for the command service — a nonce out, an answer back, on the `eyes.txt` pattern the World lane already runs. ⚠️ **Do not generalise the camera's shape to reach it.** A published cell works because the camera is one small `Copy` tuple owned by the frame path; "the console's state" at large is panes, transcripts and textures, and a cell per fact is a second state tree that will drift from the first. ⚠️ The other tempting shortcut is to append yaw/pitch/distance to `Shared` so `organon status` reports them; do not. `Shared` is append-only with pinned goldens and a `LAYOUT_VERSION`, and this is **host** state that dies with the window — putting it there would make it a param, which is the one thing it is not (§1.3, the two cameras) | Console Spike §5.9.25 |
| The pie menu, and the context menu | §1.8's `Registry` is the table both read: `groups()` is the root ring, `verbs_in(group)` the second, and an argument's `ArgKind::Choice` the third — already a closed, validated value space, because those options were built from `substrate_materials`' own tables rather than restated. A wedge press builds the same `(name, args)` pair a typed line builds and hands it to the same dispatch, so the menu is a **second renderer of one table, never a second table**. ⚠️ The one thing it needs that the slash surface did not: `Int` and `Text` arguments have no closed value space (`block`'s row count, `patch`'s two counts), so a wedge for those has to open a field rather than a ring — and `patch`'s anchor arithmetic makes it a poor menu candidate at all. ⚠️ Do **not** give the menu its own vocabulary for "what the console can do"; the failure that costs is the one §1.8 exists to prevent | James's own framing: *"mirror the command hierarchy of the slash commands on the context menu, pie menu that we have in the works"* |
| Posture's tween, and pane splitting | Both change the transcript's available width, and **the cost of that is now measured rather than assumed** — §1.7, in full at `doc/console_rewrap_measurement.md`, with five priced options and no decision taken. The two things the design has to answer before either is scoped: whether the tween moves the *wrap width* at all (option B holds it fixed for free), and whether the scrollback is virtualised first (option E, the only one that also fixes the steady-state cost §1.7 found underneath). ⚠️ Do not scope a smooth 0 → 90 pt tween against a ten-card transcript — the number that decides it is the 2 000- and 10 000-element row | #38 · `console_view_paradigm.md` §2, §9 |
| The other twenty-four Organon panels | **Look ▸ Surface landed**, and with it the whole mechanism: `param_sink::Sink` (the two-armed write destination), the `srow!`/`crow!`/`combo!`/`rd!`/`wr!` identity join, and `OrganonPanels::overlay`'s difference-not-snapshot route into `Shared`. §1.11's "The pattern, for the other twenty-four" is the four-step recipe, three steps of which the compiler checks. ⚠️ **The two that do not check themselves**: a missed `.value()` → `rd!` conversion compiles and silently pins the Console's copy to Organon's defaults, and each panel's fields need their own `PresetValues` census — Surface's 167 were all present, which is a fact about Surface. ⚠️ Do **not** convert a second panel to prove the pattern generalises before a hand has confirmed the first one moves the picture; a reviewable single panel is worth more than a broad half-transplant | §1.11 |
| Regions, Tier 2 — the content | §1.14 landed the axis in T1, **`3d` in T2b** and **`panel` in #98 Tier A**: T2b brought the content word, the producer seam, the widened `engine_plan` (the portal wins, the loser paints a notice), the uniqueness rule attributed to Organon rather than to viewports, region-aware wheel ownership, and the portal's machinery *shared* rather than copied; Tier A gave `panel` a body — **a scrolling stack**, one console-wide, with `console stack add|remove <panel>` (and `remove all` to empty it), and the wheel claim T1 predicted for "the moment a region holds something scrollable". ✏️ **The blocker this row used to name is gone rather than solved**: it read *"what is missing is a third word naming which panel, since two rings cannot say it"*, and the stack removes the need for one — the region and the panel are named by **different commands**. ✏️ **And a panel now lives only in a stack**: the transcript route (`Body::Organon`) is retired, because a transcript is a log and a control is not a log entry. What is left is **a tab per agent region**, which is what makes a second `agent` region draw something: today it cannot, and the reason is the borrow (§1.14) rather than a policy. Then **`media`**, which waits on §1.13's placement question. ✏️ **Tier B has landed**: four quadrant bits are now **six cells**, three columns by two rows, so `topcenter` is expressible and James's editor layout can be typed. The side columns are a **fixed `SIDE_COLUMN` = 320 pt** with the centre taking the remainder (Organon's own docks are absolute, not thirds), and below 688 pt of pane the column words refuse while the rows keep working. ⚠️ **`left` and `right` therefore mean the outer COLUMN now, not the half** — the one word-level break in this axis's vocabulary, deliberate and recorded in §1.14 and the changelog. ✏️ **Tier C landed and was then cut back**: it shipped as a command line inside each region dispatching the whole registry, James rejected that scope on a running console, and what remains is a two-word `add`/`remove` control in `panel` regions only — which is still what makes a *per-region* stack addressable, and is all it was wanted for (§1.14). ✏️ **Saved layouts have since landed out of that deferral order, and the promotion is argued rather than assumed** — §1.15: `doc/organon_is_the_product.md` §4 reframes a layout as the unit of *product identity* rather than a convenience, which is a different weight from the one this row deferred, and the work needed none of B/C/D — it records whatever arrangement exists and derives every word from `Region::ALL`, which is why **Tier B landing under it changed nothing in it**. What it leaves behind is small and named: ✏️ **the name ring has since become a `NarrowFn` over the library** — measured first, which is what the deferral asked for, and the measurement is why it is cached rather than read straight (§1.15: the candidate walk runs on the *draw* path and asks n + 1 times per call, so a hundred layouts is 10.1 ms against a 16.7 ms frame) — and **the CLI has no `list`**, because a read has no return path on this lane and the dotted verb `console.layout.list` has no CLI spelling. ⚠️ **Tier B's word-level break is the first real test of a layout's forward compatibility, and it is the expected behaviour rather than a bug**: a `layouts.json` written before it still loads (`left` and `right` resolve, and now mean the outer column), and one naming `topcenter` is refused **by name** in an older build rather than half-loaded — which is exactly the story §1.15's refusal table is arranged to give. ⚠️ Animated transitions and drag-to-resize are still after Tier C — a divider a hand can move is a change to `region_rect`'s contract (it reserves no gutter and computes from the pane alone), and it wants §1.7's re-wrap measurement first, exactly as the posture tween does; what Tier B changed about that is only *which* number would become state — the side width, rather than a ratio. 📌 **The one thing neither `3d` nor the stack settles is whether either is any good**: whether a 3D viewport in half a window earns its half, whether two scrolling control columns beside a live transcript read as Organon's editor or as a cramped imitation of it, and whether orbiting beside a live transcript feels right, are James's calls and no amount of green or of captured frames answers them (§3) | §1.14 · #98 |
| Pi bridge / workers / PTY | T1 landed the workspace side (`mock_agent.rs` + `timeline.rs`: every `EventKind` rendered, pull-tick replay). Next: a real adapter *behind the same tick shape*, approval decisions routed back as events — never a second event vocabulary | Console #7 T2+ |

**IPC rule inherited whole:** any new Console channel — mmap, sidecar, socket — goes
through `ipc.rs::ns_file` under the `organon-console` namespace. A hard-coded `$TMPDIR`
path silently breaks the three-products-simultaneously guarantee that
`edition.rs`'s pairwise-distinct-namespace test pins.

## 3. Honesty ledger

- 🚨 **THE CONSOLE HAS NEVER BEEN RUN ON macOS. Not once, by anyone.** No window has opened
  on a Mac, no glyph has been drawn there, no PTY has been spawned there by this binary.
  Everything below is about whether it *compiles*, which is a different sentence, and
  this work was scoped to exactly that.
  ✅ **IT COMPILES, AND THE SUITE PASSES — measured on a real `macos-latest` runner, not
  inferred.** `build (macos)` (PR #96) is **green on its first run**: `cargo build --release
  --features console-edition --bin organon-console` in **9 m 30 s**, then `cargo test --release
  --workspace --features console-edition --no-fail-fast` → **2138 passed, 0 failed, 5 ignored**
  across 30 test binaries, zero errors. Whole job 17 m 46 s cold, and it did not queue. That is the
  answer to "can we build Organon Console for macOS": **yes**, with the caveat this entry opens on.
  ✅ **Also measured, from a Linux container with no Mac in reach**, because the fast loop matters
  more than the answer once: nine of the workspace's eleven members type-check clean for
  **`aarch64-apple-darwin`**, `--all-targets` (lib *and* tests), in well under a minute —
  `cargo check --target aarch64-apple-darwin --all-targets --workspace --exclude
  organic-math-native --exclude organon-visual`, exit 0. That is the compositor compiler-verified
  for Apple silicon without a Mac. `platform.rs` is why it costs nothing to believe: macOS folds
  into `Platform::Unix`, its `/bin/zsh` fallback was written for a Mac, and both arms are exercised
  by tests on every host, so a Mac-shaped launch decision is not a `#[cfg]` nobody can run.
  ⚠️ **The exclusion is a deny-list on purpose, and the allow-list version failed exactly as
  predicted while this was being written.** The first recipe was a `-p` list typed from CLAUDE.md's
  repository map, and it silently missed five real members — `organon-agent`, `organon-visual`,
  `organon-world`, `xtask`, the vendored `egui-wgpu`. `cargo metadata --no-deps` is the authority on
  who the members are; a prose list, this one included, is not. Only **two** members reach nih_plug:
  the root crate, and `organon-visual`, which is on the deny side solely because it *depends on* the
  root crate rather than being a nih_plug crate itself.
  🚨 **The root crate is NOT in that list, and the reason is the single most surprising fact
  establishing this turned up.** `cargo check --target aarch64-apple-darwin --features console-edition
  --bin organon-console` **fails**, and not on our code — it never reaches our code:

  ```
  error: failed to run custom build command for `coreaudio-sys v0.2.18`
  coreaudio.h:1:10: fatal error: 'AudioUnit/AudioUnit.h' file not found
  ```

  ⚠️ **This is the exact inference the Windows story invites, and it is wrong.** `ci.yml`'s
  header establishes that the Windows cross-check needs no system packages because `native/`
  has no `build.rs` and `cargo check` does not link. Both remain true for Apple targets, and
  the check still dies — because the build script belongs to a **dependency**, and `cargo
  check` runs those. `nih_plug` (`features = ["standalone"]`) → `cpal` → `coreaudio-rs` →
  `coreaudio-sys`, which runs bindgen against the macOS SDK headers. That is `CONSOLE_ARCHITECTURE.md`'s
  version of the crate's own note that the Console **binary** lives in the root crate: the
  compositor lib is nih_plug-free by acceptance test, and the moment you ask for the *binary*
  the whole plugin host stack arrives with it, Apple frameworks included. ⚠️ Do not "fix" the
  wrong `-sys` crate on the strength of the name: **`jack-sys`, also Unix-graph, also a `-sys`
  crate, cross-checks clean** — it is `dlopen`-based and needs no headers (measured, exit 0).
  `coreaudio-sys` and `coremidi-sys` are the bindgen ones.
  📌 **So this is cause (c) — it needs a real Mac — and it is a property of the TOOLCHAIN, not
  a defect in our source.** Nothing was found to fix: no missing macOS arm, no unsupported
  upstream crate. The blocker is the absence of an Apple SDK on a Linux box, and the answer to
  it is `build (macos)` on a `macos-latest` runner, which is the first macOS
  coverage this repository has ever had for any edition. `ci.yml`'s macOS block owns the
  reasoning — including why there is deliberately **no** cheap Apple cross-check leg beside it
  and what the leg does not prove.
  🚨 **What the green `build (macos)` means and does not mean.** It means *green and ready to
  deploy* — it compiles, and the workspace suite passes on macOS. It does **not** mean verified
  working, and the list of what stays unknown until somebody opens the window on a real Mac is
  long and is the interesting part: whether wgpu picks Metal and the surface configures;
  whether the backdrop's sRGB/linear gamma pair (measured on Windows and Vulkan) holds on
  Metal; whether the glyph grid is legible at Retina scale factors, which no other platform's
  scaling exercises; whether ⌘T/⌘W/⌘1-9 arrive as `Modifiers::COMMAND` through winit on the
  platform where ⌘ is the *native* modifier rather than the borrowed one; whether a login
  `/bin/zsh -l` tab inherits the PATH a Homebrew- or nvm-installed harness needs; and whether
  the window has **any icon at all** — `with_window_icon` does nothing on macOS (§ the icon
  note above), the icon there comes from an `.app` bundle, and the Console has none.
  📌 **Explicitly out of scope and NOT attempted**, so nobody reads a green tick as more: no
  `.app` bundle, no `Info.plist`, no code signing or notarization, no dock/menu-bar
  integration, no `deploy.sh` path (that script is the *plugin's*, and the Console is
  standalone-only with no bundle and no plugin identity — permanently). A macOS *build* is not
  a macOS *product*, and this entry is only about the first.
  ⚠️ **Organon and Organon Mind still have no macOS CI coverage.** The Console leg builds the
  root crate's lib, so most shared macOS ground is compiled incidentally; the
  `cfg(not(feature = "console-edition"))` arms — the plugin's export macros, `standalone.rs`,
  `mind_main.rs` — are not. Unlike the Windows/Mind asymmetry, which `ci.yml` argues for, this
  one is a gap rather than a considered trade.

- 🚨 **THE COLUMN'S CONTROL IS NOW DRIVEN HEADLESS, BUT STILL NOBODY HAS TYPED INTO IT ON A
  SCREEN** (#98 Tier C, narrowed). ✏️ **This entry used to read "nobody has typed a character into
  a region command line", and that was the entry that mattered**: James then typed into one and
  found it would not accept input at all. The suite has since gained real egui frames —
  `typing_survives_the_palette_opening`, `a_note_appearing_does_not_take_the_box_with_it` and
  `tab_takes_the_highlighted_candidate` drive `egui::Event::Text` and `Event::Key` through
  `region_line::draw` at a real 320 pt column width — so *"a character reaches the box"* and
  *"Tab takes the highlighted candidate"* are now measured rather than asserted, and the
  measurement is what found the defect. ⚠️ **What is still unmeasured is everything about the
  surface as a surface.** No band has been looked at, no box has been clicked with a mouse, and
  the arbitration is exercised only as a value: `Lines::begin`/`owner`/`composer_owns_keys` are
  unit-tested, and the thing they are *for* — clicking from the composer into a column's line and
  back, and Tab meaning the right thing on each side — needs a hand. ⚠️ **The failure mode this
  names: the focus story is the part most likely to be subtly wrong and the part a green suite
  says least about.** The one-frame-behind gate is argued from "focus moves by a click and a
  click is not a key"; if some route moves focus *with* a keystroke in the same frame, that frame
  goes to the wrong reader, and the symptom is one lost Tab rather than anything that looks
  broken. ⚠️ Nor has anyone judged the band: four fixed rows against `MIN_SIDE`-adjacent regions,
  a note that clips rather than wraps, and whether a control that is always showing two words
  reads as a control or as clutter — all taste calls, all James's.
  ⚠️ **And the general lesson, which cost this tier a round trip**: a headless suite that never
  drives the framework's own input path can be entirely green about a widget nobody can type
  into. The three tests above are cheap and were available the whole time.
  ⚠️ **The per-region stack is one measurement extended by argument.** James saw one `stack add
  surface` fill both side columns and said the columns should differ; that is a real observation
  and it is the whole of what forced the change. **Nobody has assembled two genuinely different
  columns and used them**, which is the thing the change is *for*, and it is where the remaining
  doubt sits — in particular whether two Surface panels over one mirror in two columns read as
  one instrument or as a confusing duplicate.
  ⚠️ **A restored layout now comes back with two empty columns** where it used to come back with
  two views of one empty column. Nothing regressed — §1.15 never recorded stack contents — but the
  cost of that gap is higher than it was, and it is recorded there rather than fixed here.

- 🚨 **THREE COLUMNS ARE GREEN AND HAVE NEVER BEEN LOOKED AT — and the one number the tier turns
  on is a taste call nobody has judged** (#98 Tier B). The geometry is arithmetic and the tests
  cover it: four cells to six, twelve region words, two vertical cuts, the narrow-pane boundary
  pinned from both sides. **None of that says the layout is any good.** `SIDE_COLUMN = 320.0` was
  chosen from precedent — the widest fixed control column in this tree, Organon's own theme dock —
  and precedent is an argument, not an observation. Whether a 320-point column beside a
  460-point instrument reads as Organon's editor or as a cramped imitation of it, whether the
  instrument still looks like the subject at 1100 points, and whether a `panel` stack is legible
  at that width are questions a hand and a screen answer. **Nobody has typed `/viewport topcenter
  3d` on a running console.** ⚠️ The failure mode this entry exists to name: the tier is *most*
  convincing exactly where it is least verified — a fixed width produces a clean, confident,
  reproducible rectangle whether or not the rectangle is the right size.
  ⚠️ **`left` and `right` mean something new, and no test can tell you whether that is a problem.**
  They were halves; they are the outer columns. The suite proves the new meaning is consistent
  everywhere, which is a different claim from "somebody who typed `/viewport left panel` yesterday
  will not be surprised today". §1.14 and the changelog say it out loud because saying it is the
  only mitigation available without a hand.
  ✅ **What IS measured**, on `main` @ `6443ba4`: `organon-console --lib` **719 passed, 3 ignored**
  (717 before — the two are Tier B's, the editor layout and the narrow-pane boundary);
  `organon-core` **594** unchanged; the console bin **62** and the `organon` bin **16**, both
  unchanged, and both **run** rather than type-checked, on the ledger entry below's rule. Both
  `cargo check` legs clean.
  📌 **No root-crate test needed editing**, which is worth recording because the entry below
  predicts the opposite: the region ring's *value space* grew from nine words to twelve, and
  `the_compact_panel_shows_the_real_table` counts **verbs**, not values, so its string, its
  character count and its hidden `+N` were all untouched. The prediction was right about the
  hazard and wrong about this instance — checked by running both bins, not by reasoning about them.

- ✅ **Tier 2b was BUILT, RUN AND LOOKED AT on a GPU — this is the first region tier that is not
  "green and ready to deploy".** A release `organon-console` from the worktree was launched on
  ORGANON-ONE (RTX 5090, 225 % scaling) under a forked IPC namespace, driven with the CLI and a
  real pointer, and captured at every step. **What was seen**, each with a frame:
  a `3d` region **renders the live World** beside a working transcript — not a blank rect, not the
  placeholder, not a stale texture; the transcript **re-wraps into its half** (filenames break
  mid-word at the region's width, which is what proves `term_view`'s grid sized itself from the
  child `Ui`'s rect rather than the window's); **drag inside the region orbits** and **wheel inside
  it zooms**; **wheel over the transcript scrolls the transcript and does not move the camera**,
  and **wheel over the viewport zooms and leaves the transcript in exactly the same scroll
  position** — both directions, with real scrollback on screen; `organon console camera` drives it
  from a prompt; the **portal still opens, orbits and closes unchanged**, and while it is open the
  region paints its yielded notice and gets the frame back on close.
  ✅ **The precedence rule does what it says.** With both claimants, the portal renders and the
  region reads *"3d — the portal has the world. Organon renders at most one frame per console
  frame, so the floating portal takes it while it is open; `organon console portal close` gives it
  back to this region."*
  ✅ **The uniqueness refusal was seen live**, not merely unit-tested: asking a second region for
  `3d` printed the refusal naming both regions, attributing the limit to Organon and its shared
  jitter phase, and naming the way out.
  ✅ **Nothing leaked.** Six `[surface] released …` lines across the session, each naming its
  cause, and every allocation matched — including the two that matter most: the texture
  reallocating when the frame changes hands (`the viewport changed size`, 1375×1725 ⇄ 1155×650)
  and the single release site firing on the way out (`nothing is showing the world — the portal is
  closed and no region holds 3d`). The console returned to `full agent` with no texture held.
  ⚠️ **What a running console still does NOT settle, and these are James's calls, not mine.**
  Whether a 3D viewport in half a window is *useful*; whether this is the right split; whether
  orbiting beside a live transcript feels right. No amount of green or of captured frames answers
  any of them, and nothing here should be read as claiming otherwise.
  ⚠️ **One observation that is a real finding rather than a defect: the yielded notice's
  legibility depends on the layout.** The portal is anchored **top-right** and floats over the
  whole pane, so with `3d` on the `right` it covers most of the notice and only its first few
  characters are readable; with `3d` on the `left` the notice is fully legible. The notice is
  painted and correct either way — what varies is whether you can read it. That is the collision
  §1.2's "it occludes the rows it floats over" already describes, met by a region rather than by
  text, and it is a placement question for the tier that gives the portal its other states.
  ⚠️ **Measurement honesty about the drive itself.** Two early drag attempts on the region did
  **not** register, and the reason was window-activation state rather than the wiring: the first
  ran before the window had ever been activated, and the wheel — which needs only hover — worked
  throughout. The decisive check was running the *same* gesture against the portal, which orbited;
  after the window had been clicked into, the region orbited too, repeatably. I did not isolate
  precisely which activation step fixed it, so it is recorded as observed rather than explained.
  ⚠️ **Captured with `PrintWindow`, not a screen grab**, because the window could not be brought
  to the foreground from a background process and stealing focus mid-session was not worth it.
  ⚠️ **`GetWindowRect` returns LOGICAL points to a DPI-unaware process** — at 225 % this window is
  2782×1888 physical against ~1236×839 logical, and a bitmap sized from the logical rect silently
  crops the right edge in a way that reads exactly like a text-wrapping bug. The capture script
  declares itself DPI-aware, which is the fix rather than oversizing and hoping.
  ✅ **Measured:** `organon-console --lib` **699 passed, 3 ignored**; `organon-core` **593**;
  the console bin target **61 passed**; `--bin organon` **15 passed**; both `cargo check` legs
  clean. ⚠️ **The `--lib` baseline in circulation was 696 and the true one is 697** — counted from
  the test attributes on `main` rather than from the number the brief carried, because a stale
  baseline turns a delta of two into a delta of three and then somebody goes looking for a test
  that was never written. The two added are `region`'s uniqueness refusal and `portal`'s
  wheel-claim-over-a-list; the console bin's one is the portal-versus-region precedence.
  🚨 **`--bin organon` caught a real failure that the four-leg bar structurally cannot see, and it
  is the third instance of the same defect class.** `console_viewport_takes_a_region_and_a_content_
  and_defaults_neither` asserted that `viewport left 3d` is **refused** — true when it was written,
  false the moment `3d` joined `CONTENT_WORDS`. It compiles perfectly either way, because the
  assertion is about a *value* and the leg that covers it only type-checks. Its console-side twin
  (`the_viewport_verbs_rings_…`) carried the identical stale line. **Both moved to `media`** rather
  than being deleted: a word table whose refusals are never exercised stops being a closed value
  space the day somebody adds a word to one of its four renderings and not the others.

- ✅ **CORRECTED — the regions HAVE now been seen split.** This entry said *"never been seen
  split"*, and that stopped being true twice over: James ran a divided console with
  `topleft agent` and confirmed the vacancy notices read correctly, and Tier 2b (below) was built
  and driven on a GPU. What the original entry got right is kept below, because a claim that was
  once unobserved and is now observed should say **which** parts got observed rather than being
  deleted wholesale.
  ✅ **Observed since:** a divided pane draws; `term_view`'s glyph grid does lay out correctly in
  a clipped child `Ui` rather than merely compiling; the vacancy notices name themselves and the
  command that fills them. **The wheel is region-aware as of Tier 2b** — that sentence below is
  superseded, and §1.14's "the second consumer" subsection is what replaced it.
  ⚠️ **Still unobserved, and still the point of the whole axis:** whether a half-height
  conversation is *useful* is James's call and is unanswered by any amount of green. Whether the
  hairline separators read as separators at 225 % scaling or as artifacts has not been judged.
  Whether the portal, which still floats over the **whole** pane, looks right straddling two
  regions is likewise unknown, and is a taste call as much as a correctness one.
  📌 The original entry, kept for the record: it was **green and ready to deploy**, which is a
  different sentence from verified working — and this is the tier where that distinction stopped
  being the only thing on offer.
  ✅ What *is* measured: `organon-console --lib` is **696 passed, 3 ignored** (684 before — the
  twelve are `region`'s), `organon-core` is **593** unchanged (the new wire assertions live inside
  existing tests), the console bin target is **60 passed** (59 before) and the CLI's own bin is
  **15 passed** (14 before). Both `cargo check` legs are clean.
  ⚠️ **Four root-crate tests were edited, and the ledger entry below is why that is worth
  flagging rather than mentioning.** `the_compact_panel_shows_the_real_table` (its string, its
  character count and its hidden `+N`), `the_real_table_says_which_verbs_may_run_without_an_enter`
  (the reversal column of the whole vocabulary), `a_capability_call_becomes_the_sidecar_line_the_
  cli_would_have_written` (an args exemplar) and `every_op_round_trips_through_its_catalog_name`.
  All four live in the bin target that the four-leg bar only *type-checks*, so they were **run**
  rather than trusted. The two list-shaped ones were re-derived from the tables, never appended
  to — the discipline the entry below paid for.
  📌 **`panel` content is a labelled placeholder and says so on screen**, which is the honest
  shape for a tier that divides the pane before it has things to put in it. It is not a stub
  pretending to be a feature: the region names itself, names what belongs there, and names that a
  later tier fills it. `3d` and `media` are absent from the vocabulary entirely rather than
  present and inert, because a word an agent can type and nothing can honour is worse than a word
  that is refused with the list that would have worked.

- 🚨 **`main` @ `2018d41` did not compile, and it stayed that way until somebody built it.**
  Not a test and not an edition — the `organon-console` **library**, on
  `error[E0063]: missing field reversal` at `registry.rs:544`. Two of the bar's four legs were
  red on it, so every branch cut from `main` inherited a tree that would not build.
  ⚠️ **Neither contributing branch was wrong, and neither could have been red.** `/media`
  joined `view_entries()` on the exhibit branch (`94e26c7`); `Entry::reversal` was added to
  every entry *that existed at the time* on the autorun branch (`8307e5c`). The hunks are a few
  lines apart in one `vec![]`, so git merged them with no conflict to review and produced an
  initializer missing a field that did not exist when it was written. The defect exists only in
  the combination and was authored by the merge.
  📌 **The gap was not a missing test — it was that nobody built after merging.** A missing
  struct field is a compile error; no test catches it earlier or better. Every count this ledger
  carried above was therefore a claim about some branch, never about `main`: measured on the
  repaired tree, `organon-console --lib` is **682 passed, 3 ignored** and `organon-core` is
  **593 passed**. The `675`/`580` pair in circulation before this was stale on both halves.
  ✅ What a test *can* add is the value rather than the presence, and
  `the_view_lane_states_what_can_be_taken_back_and_an_exhibit_cannot` now pins all four
  view-lane verbs. A missing field is loud; a wrong one is silent, and the wrong one here would
  have let a keystroke place an exhibit unasked.
  🚨 **The same merge broke a second thing, and the four-leg bar structurally cannot see it.**
  With the tree building again, `the_real_table_says_which_verbs_may_run_without_an_enter`
  (`console_main.rs`) failed: it pins the reversal column of the console's *whole* vocabulary as
  a literal list, and that list had never run against a table containing `media` — `/media`
  joined the view lane where `Reversal` did not exist, the test arrived where `/media` did not.
  ⚠️ **A missing field is `E0063`; a stale vocabulary list is a failing test, and only something
  that RUNS it can catch one.** It lives in the root crate, which the bar's fourth leg
  type-checks and never executes — the entry below has said so for some time — so all four legs
  were green while this was red. **CI caught it.** On any change that adds or moves a console
  verb, a green local bar is not evidence about this test. 📌 `compact_line`'s hidden `+N` count
  carries the same warning from two earlier merges; the discipline for both is to **re-derive
  the list from `view_entries()`**, never to append and assume the order.

- 🚨 **The exhibit (§1.13) is code that compiles, runs, and has never been looked at.**
  `cargo test -p organon-console --lib` is **678 green** (672 before), `cargo test -p organon-core`
  is **593 green** (580 before), `native/tests/exhibit_formats.rs` adds 2, and both `cargo check`
  legs are clean — the warning count on the console binary is **179 before and after**, so this
  change adds none.
  ✅ **The binary was built and launched**, which the four-leg bar does not do and which is in this
  ledger because a change once passed all four and then died on startup with a stack overflow. It
  ran a steady frame loop for 40 s under Xvfb on software Vulkan (lavapipe) with no panic and no
  error output. That clears *startup*, and only startup.
  🚨 **No picture has ever been on a screen.** This session had no GPU and no hand: nothing typed
  `/media`, no PNG was decoded by the running binary, no texture was uploaded or evicted, and
  nobody has seen whether an image in half a window is useful — which is a question only James can
  answer. The decode path is exercised only by `exhibit_formats.rs`, which proves the *codec* is
  present in this build, in memory, and says nothing about the console.
  ⚠️ **The design makes this un-drivable from outside on purpose, and that is worth knowing before
  someone tries.** `/media` is a view-lane verb absent from the sidecar and the MCP catalog
  precisely so that no process other than the person at the keyboard can hand the console a path
  (§1.13). The consequence is that there is no headless route to summon an exhibit — the first
  real test is a hand in a window, and that is the cost of the security property rather than a
  gap in the tests.
  ✏️ **Measured since, and the decoder is off the unverified list.** `exhibit_formats.rs` is now
  **6 green** (2 before): a **64 × 48** four-quadrant fixture — red top-left, blue top-right,
  green bottom-left, an amber with alpha 128 bottom-right — is encoded to every extension in
  `IMAGE_EXTENSIONS`, written to a temp file, and read back through `image::open` → `to_rgba8()`,
  the console's own two calls. Each corner's colour lands at each corner's coordinates, the raw
  buffer's first and last four bytes are the top-left and bottom-right pixels, the `thumbnail`
  branch above `MAX_EXHIBIT_EDGE` is checked too, and the sRGB-storage / UNORM-sample pair is
  pinned rather than merely commented. ⚠️ **The 2×2 test that was already here could not have
  caught either failure**: a fixture symmetric under a flip cannot detect a flip, and one read
  only as a *length* cannot detect a channel swap — it would have passed just as green against a
  decoder returning the picture upside down in BGRA. Verified by mutation rather than assumed: a
  vertical flip injected after the decode fails three of the six, a red/blue swap fails the same
  three, and a crossed storage/view spelling fails the pin alone. JPEG's tolerance is **measured**
  — worst channel drift 1, bar set at 2 — and JPEG *discards* alpha rather than compositing, so
  the alpha assertion there is 255 by the format's own rule. Dimensions differ on purpose (a
  transpose changes them); the probes sit at quadrant centres because JPEG's 16 × 16 chroma block
  makes the boundary pixel the dishonest place to sample.
  🚨 **This is the decoder and nothing after it, and the sentence above still stands.** What is
  measured ends at the RGBA buffer `ExhibitLoad::Picture` carries. `upload_exhibit`'s
  `write_texture` row order, `register_native_texture`, the `(0,0)–(1,1)` UV rect at the paint
  site and every GPU sampling behaviour are all downstream: **a flip introduced after
  `to_rgba8()` passes all six.** And pinning that the two format constants agree is **not** a
  measurement of gamma — whether sRGB storage sampled through a UNORM view linearizes exactly
  once is a property of a GPU, and none was run. The pin is a source-text one because the
  constants live in a `[[bin]]` and `doc/arch/topology.md` forbids `organon-console` `wgpu`
  outright, so a library that both could import is not available.
  📌 Still specifically **unverified**: that a decoded picture *looks* right on a screen — its
  gamma, and every link between the buffer and the glass; that the eviction line fires at the cap
  in a real session; that `reading...` is brief enough to feel like loading rather than breakage;
  and that Markdown's four line kinds are legible at `EXHIBIT_HEIGHT` in a real card.

- 🚨 **`/organon look surface` opens Organon's real Surface controls; the other twenty-four
  panels still open a line saying they have not been transplanted.** ✏️ **Corrects the entry
  this replaces, which said `/organon` opens none of them.** §1.11 carries the mechanism: a
  parameter still cannot be written from outside `nih_plug`, so the panel writes a
  `PresetValues` mirror and the world is driven from the difference between it and its own
  starting state. What is verified is code: `cargo test -p organon-console --lib` is **659
  green** and `cargo test -p organon-core` is **570 green** (565 before, plus five on
  `ipc::overlay_changed`); both `cargo check` legs are clean.
  🚨 **A green build proves the widgets compile, not that dragging one moves the picture.** That
  needs a GPU and a hand, and this session had neither.
  **That is the whole claim: it compiles and the tests pass.** What it does not establish, in
  order of how much it matters: (0) 🚨 **that a Surface control moves the picture** — nothing
  below is worth much if it does not, and the check is one motion: open the portal (`engine_plan`
  forces the backdrop `Off` while it is open, so a substrate backdrop proves nothing), type
  `/organon look surface`, drag `node bevel`, watch the cubes round; (1) **that the two rings
  feel like Organon's own hierarchy when
  a hand walks them** — James described `l` → `look` completing while the ring beneath it
  changes as one motion, and whether twenty-five candidates in the second ring reads as a menu
  or as a wall is a question about a running window; (2) that the slugs are the words a person
  reaches for — `lmat` for Liquid Material and `fx` for Surface FX exist to satisfy the
  no-prefix rule, and a slug nobody guesses is worse than a longer one; (3) that the remaining
  twenty-four panels' "not transplanted yet" line reads as *honest* rather than as broken —
  sharper now that it sits beside a panel which **is** transplanted, since the contrast is
  either reassuring or damning and nobody has seen which; (4) that §1.9's eight-row candidate cap
  is survivable at twenty-five — the Look ring overflows it by seventeen and nobody has seen
  "+N more" against a list that long.
- ✏️ **`OrganonDraw` carries a widget now.** It was retained through the tier that built it as a
  seam nothing filled — `console_main` passed a closure that ignored its arguments, and the
  `Status::Live` branch beside the placeholder had never been reached. Both are live:
  `console_main` passes `|ui, panel| organon_panels.draw(ui, panel)`, and Surface routes through
  it. ⚠️ **Reached, not *seen*** — no human has looked at a panel drawn through this seam, so
  whether an editor card reads as an element in a conversation flow is untested. The two
  frames are deliberately different objects drawn to the same spec, and that is exactly the
  kind of claim only a screen settles.
  ✏️ **The seam moved and the question it was asking has been withdrawn.** `OrganonDraw` now
  lives in `panel_stack`, and a panel is no longer an element in a conversation flow at all —
  §1.14 carries the argument (*a transcript is a log and a control is not a log entry*). So
  "whether an editor card reads as an element in a conversation flow" is not a thing anybody
  has to answer any more; **"still not seen" survives verbatim**, now about a card in a
  scrolling column beside the transcript instead of one inside it.
- ✏️ **The panel stack has now been looked at — 2026-08-20, two columns of it, beside a live
  transcript and a `3d` region.** This bullet opened *"has never been looked at either"*; that is
  retired. What the sitting proves is narrow and worth stating exactly: two `panel` regions each
  drew a complete `organon · look · Surface` body — mode, palette, node bevel, material maps,
  procedural, the two graph buttons, the plexus toggle — **simultaneously, in one frame, with no
  visible id collision**. That is the first evidence outside the headless test below that the
  `param_sink` id namespacing holds when two Surface bodies are on screen at once. ⚠️ It is
  evidence about *drawing*, not about *writing*: §1.11's item (0) is still unchecked, because
  nobody has yet turned a knob in one of those columns and watched the picture move.

  🚨 **And the sitting produced a verdict against the current design.** Both columns rendered the
  **same** panel, because one `/stack add surface` populates the single console-wide stack that
  every `panel` region is a view of. James, seeing it: *"you can see why we need to have the
  update for the stacks because it addressed both of them."* That is what settled #98 Tier C's
  per-region-stack question — by observation rather than by argument, which is the outcome this
  ledger exists to make possible.

  What remains open is the part no green and no screenshot answers, and it is still James's
  call: whether a column of Organon's controls **beside** a live transcript reads
  as the instrument's own editor or as a cramped imitation of it; whether the gap between cards,
  the scroll bar and the region's own hairline settle into one object or three; whether
  twenty-four "not transplanted yet" lines stacked in a column read as an honest inventory or
  as a broken panel — sharper here than in §1.11's version of the same worry, because in a
  column they are *adjacent* rather than summoned one at a time; and whether `stack` is the word
  a hand reaches for after typing `/viewport left panel`. **No amount of green answers any of
  them**, and the id-namespace test, which is the strongest thing in this tier, proves only that
  four Surface bodies get four distinct egui ids — not that a knob in one moves the picture,
  which §1.11's item (0) still records as unchecked.
- 🚨 **#117 is the largest purely-visual change this console has had and NOTHING in it has been
  seen on a screen.** Three tiers, all of which are claims about how something *looks* or how
  quiet it *feels*, and every one of them is green rather than verified:
  - **Tier 1, quiet by default.** The tests pin the *rule* — which line is `always`, that a
    refusal survives a quiet console, that a held receipt comes back under `/trace on`. What no
    test can say is whether a conversation with the narration removed reads as calm or as
    broken: an acceptance really does disappear, and the argument that "the layout moving is the
    receipt" is a claim about James's eye, not about the code.
  - **Tier 2, one line.** `band.contains_rect(box)` at 320 pt is measured. Whether a popover
    floating over a panel column is *better* than three rows under it — whether it lands where
    the eye is, whether it obscures the card it sits on, whether a refusal that persists without
    focus is reassuring or nagging — is entirely unlooked-at. James permitted the popover
    (*"if we need to pop something up over top of it, we can do that"*); he has not seen one.
  - **Tier 3, Organon's card.** This is the one where "green" says least. The Console now calls
    Organon's real `card()`, so the padding, the corner radius, the silver header band and the
    collapsing header are the editor's **by construction** — but a card sized for a 3-column
    editor pass is now drawing in a region column of unknown width, and nothing checks that it
    survives that. The `CollapsingHeader` is also new here: **a stacked panel can now be folded**,
    which nobody has asked for or tried. And the twenty-four `Declared` panels now wear the full
    chrome around one italic sentence, which the bullet above already flags as the worry that is
    *sharper* in a column than in a transcript.

  **Only James can say whether it now looks right, and none of this claims it does.**
- ⚠️ **The egui-id collision, third instance — reasoned and now *tested*, but still not
  reproduced.** §1.11's two fixes were read out of the code rather than found by running
  anything, and this one was too: a stack plus two regions is the same case from a third
  direction, worked out before the draw call was written. What is different is that it is pinned
  by a headless egui frame that draws four Surface bodies and compares the exact key
  `param_sink::value_box` builds, plus a companion that removes each half of the key and requires
  the collision back. That is a real test of the property; it is **not** a person clicking one
  value box and watching whether a text field opens in another.
- ✏️ **A layout has now been saved and loaded by a running console — 2026-08-20, by James, on the
  workstation.** This bullet used to open *"No layout has ever been saved, loaded or deleted by a
  running console. Not once, by anyone."* That is no longer true and the correction is recorded
  rather than the sentence quietly deleted, because what it proves is narrower than "layouts
  work" and the difference is the whole point of this ledger.

  **What a hand actually did.** Four regions typed one at a time — `topcenter` `3d`, `left`
  `panel`, `right` `panel`, `bottomcenter` `agent` — then `/layout save organon`, then
  `/layout load organon`. The library came back through `/layout.list` and the console rendered
  the receipt `ok /layout load organon — {"accepted":"layout load organon"}`. So **three of this
  bullet's open questions are answered by that single run**: `Console::set_layout`'s assignment
  *does* reach a window; the **store path is confirmed** at
  `C:\Users\james\AppData\Roaming\OrganonShell\layouts.json` (197 bytes, written 21:12), which
  retires the separate "unconfirmed on this machine" note that stood here; and `layouts.json`
  **is** legible — pretty-printed, one `regions` object of plain word pairs, obviously
  hand-editable. ⚠️ The receipt question is answered in the *opposite direction to what this
  bullet predicted*: it warned the lines are `eprintln!` and "in a GUI launched from Explorer
  goes nowhere", but the conversation front-end renders them in its own transcript, where they
  were read. The prediction was about the terminal lane; it did not survive contact with the
  front-end the verb is actually typed into.

  🚨 **`delete` is still untouched by a hand** — the verb has three actions and two of them have
  now been exercised. Do not round this up to "saved layouts are verified".

  ⚠️ **Still unmeasured, and the live run did not answer them**: whether a load *looks*
  instantaneous or whether the pane visibly re-lays-out under the eye (§1.7's ~7.6 ms re-wrap at
  400 elements is still the only number, still inferred rather than observed — the run confirms
  the arrangement arrives, not what the arrival looked like); and forward compatibility against a
  build predating Tier B's `left`/`right` word change, which no older binary has been asked to
  load. The headless suite below is unchanged and still carries the transactional property:
  **`resolve` returns `Err` and no `Layout`, so the refusal is a property of the signature.**

  📌 **The run produced one finding that belongs to the stack, not to layouts.** With `left` and
  `right` both holding `panel`, a single `/stack add surface` populated **both** columns with an
  identical `organon · look · Surface` — one stack, two views, exactly as `panel_stack.rs`'s
  header describes. Seeing it is what settled #98 Tier C's open question in favour of per-region
  stacks; James: *"you can see why we need to have the update for the stacks because it addressed
  both of them."* Recorded here because the layout run is where it was observed, and argued where
  it belongs, in §1.14 and the tier's own work.
  📌 **Two absences are deliberate and are not gaps**: the library ships empty because naming the
  presets is James's call, and a layout records no panel stack, theme, posture, screen state, tab
  or camera. Both are stated in §1.15 rather than left to be discovered.
- ✏️ **The `PresetValues` route is built and the counts are confirmed.** It was recorded as *"a
  reading of the code, not a working path"* with the caveat that no field had been checked for
  preset capture. All 167 of Surface's distinct parameter fields are present in `PresetValues`,
  checked field by field before anything was written; the 457-line, 190-call-site,
  24-conditional-read measurements held. ⚠️ **That is a measurement of Surface, not a property
  of the editor** — the next panel needs its own census, and a Look-tab param with no
  `PresetValues` counterpart is still a control that cannot be driven this way.
- ⚠️ **Two things about the Console's Surface panel are known not to match the editor's**, both
  consequences rather than choices, both argued in §1.11: the slider *fill* (nih-plug's
  `ParamSlider` cannot be driven by anything but a `ParamSetter`), and ↑/↓ inside an open
  dropdown committing on the next frame instead of live-scrubbing. Everything else — ranges,
  units, value strings, variant names, help text, grid lines, and which rows appear under which
  surface mode — is read off the real params and is the same by construction.
- ⚠️ **The panel opens on Organon's defaults, and against a dressed substrate that is a real
  gap.** Its untouched rows report "I have not asked", not what the world is showing; only
  against `BackdropSource::World` are the two the same thing. `Shared` → `PresetValues` is not
  invertible, so seeding the panel from what is on screen is not available today — this is a
  limitation with a known cause, not an oversight.
- ✏️ **Two egui-id collisions between duplicate panel elements were latent and became
  reachable, and both are fixed.** `organon_element` scoped its widgets by the panel's *slug*,
  which separates two different panels — something that could never have collided — while
  putting two `/organon look surface` elements, the case that can, in one namespace; it uses the
  element id now. And the typed-value box's key was absolute (`Id::new("om_value_edit")` plus
  the param pointer), which is correct in the editor, where a param appears in exactly one card,
  and wrong in a console holding two Surface elements over one params instance: clicking a value
  box would open a text field in both. `param_sink`'s copy folds in `ui.id()`; `lib.rs`'s is
  deliberately unchanged. ⚠️ Neither was *found* by running anything — both were read out of the
  code while checking what "drawing widgets for the first time" newly exposed, so the fixes are
  reasoned rather than reproduced.
- ⚠️ **`every_branch_of_the_card_reaches_the_snapshot` is a spot-check, one field per
  disclosure branch, not per field.** Rust cannot enumerate a struct's fields, so per-field
  coverage would mean a 167-name list beside the panel body — the second copy this repo keeps
  learning to avoid. It catches a whole region falling out of the packers, which is how this
  breaks in practice; it would not catch a single field.
- ⚠️ **`cargo check --tests` does not catch everything `cargo test` does, and this change found
  it twice.** An out-of-bounds constant index into a `Shared` array (`s.membrane[5]`, length 4)
  type-checked clean on the local bar's fourth leg and failed the real build with
  `deny(unconditional_panic)` — that lint fires during codegen, which `check` skips. And three
  *in-bounds but wrong* lane indices in the same test were found only by reading them back
  against `param_table!`'s slot lists by hand: `mat_scale` is `material[2]` not `[1]`,
  `splat_radius` is `splat[0]` not `[1]`, and `tube_profile` is a tail-appended scalar rather
  than a member of `tube[4]` at all. All three compile, and all three would have asserted
  against a lane nobody moved. ⚠️ **The perturbations are deltas (`+= 1.0`), never literals** —
  a chosen constant silently tests nothing on the day it equals the field's default.
- ⚠️ **The root crate's own tests were never *run* on this machine**, only type-checked.
  `cargo test -p organic-math-native --lib` was started and abandoned after ~25 minutes with a
  single 3 GB `rustc` still going; the bar names that trap and CI is where those nine tests
  (six in `panel_surface`, three in `param_sink`) first execute. Everything above about them is
  a claim about code that compiles and has been read, not about a green run.
- ⚠️ **Only the Look tab's twenty-five `card()` titles are joined to `panels::PANELS`.** The
  other seven tabs draw string literals as before, so the table cannot yet claim to be Organon's
  whole panel taxonomy — and `panels::the_look_tab_is_whole` guards a count, not a join: a
  twenty-sixth Look card added without a table entry fails that test, but a *renamed* title on
  any other tab is invisible to everything here.
  ✏️ **What has changed is what the surface says about it, not how much of it is joined.** The
  first ring still offers all eight tabs; the seven unjoined ones are marked `not mapped yet — no
  panels in the table`, their second ring carries the sentence `registry::unmapped_tab` writes
  instead of drawing blank, and a refusal names the tab that was given rather than the union
  (§1.11). James found both halves within a minute of first use — a Look-shaped refusal to a
  `generator` query, and an empty band under `/organon generator ` — which is the sixth
  the-console-knew-and-said-nothing defect this surface has produced. **The fix is the saying,
  not the mapping**; seven tabs are still dead ends and now admit it in three places.
  ⚠️ **Unverified in the same way everything else here is**: the marked ring and both refusal
  sentences exist as strings pinned by test. Nobody has read them on a running console, and
  whether "not mapped yet" reads as honest or as broken is the same open question the element's
  "not transplanted yet" line already carries.
- 🚨 **Nothing has been seen full screen, and "the window fills the display" is a claim no
  test on this machine can make.** §1.12 was written and verified in a session with no way to
  open a window: `cargo test -p organon-console --lib` is **655 green** (nine of them
  `screen.rs`'s), `cargo test -p organon-core` is green with four more of its own — the three
  screen words and the `screen full` byte-pin riding the wire-format round trip, which this
  verb reaches only because Tier 5a moved that test into a crate this bar executes rather than
  only type-checks — `cargo check --features console-edition --bin organon-console` is clean,
  and `cargo check --tests -p organic-math-native --features console-edition` is clean.
  ⚠️ The counts are quoted without a "before", deliberately: two merges have moved the
  baseline under this entry since it was written, and a delta against a number nobody can
  reconstruct is worse than no delta. **That is the whole claim: it
  compiles and the tests pass.** ✏️ **One item has left this list by being made impossible
  rather than watched for**: the automated review asked whether a held F11 would rapid-toggle
  and suggested eyeballing it on hardware. It is now filtered in `screen_key` and pinned by
  `a_held_key_flips_the_window_once_not_once_per_repeat` — a repeat stream is exactly what does
  not show up in a screenshot, and this is the chord somebody reaches for when they cannot get
  out. What remains genuinely needs a display, in order of how much it matters: (1) that
  `set_fullscreen(Borderless(None))` actually fills the display on this Windows box rather than
  producing a maximized-but-bordered window or a black band — and **full-screen behaviour is
  exactly the kind that differs between one display and two**, which is the configuration James
  runs; (2) that **F11 arrives**, which is the only part whose failure is a trap rather than a
  disappointment. The three tests prove only that nothing else in this crate *claims* the key.
  ✏️ The **translation layer has since been read and is not a risk**: `egui-winit` 0.33.3 maps
  both `NamedKey::F11` and `KeyCode::F11` to `egui::Key::F11` (`src/lib.rs:1160`, `:1284`), so
  a keystroke that reaches winit reaches this code as the key the chord tests. What is still
  unverified is everything *upstream* of winit — whether Windows or another resident hook eats
  it first, which is a live concern on this machine specifically: it runs push-to-talk tools
  that install `WH_KEYBOARD_LL` hooks, and a low-level hook decides before delivery. If it does
  not arrive, the way out is
  `organon console screen windowed` typed in any tab, and that should be the first thing tried
  before anything is diagnosed; (3) that the **tab strip and the scrim look right** at a
  display's full width, since neither has been drawn wider than a 1100-point window; (4) that a
  **posture change while full screen** does what §1.12 argues it does — the two axes are
  independent in the code and have never been moved at once with anyone watching. Both
  behaviours the two `⚠️`s in §1.12 describe (the divergence `toggle` recovers from, the
  platform putting the window full screen by another route) are reasoned, not observed.
- 🚨 **Nobody has seen the compact command panel, so whether it *feels* fast is unverified —
  and "fast" is the entire claim being made for it.** ✏️ **The verbose panel HAS now been
  seen**, which is where the compact one came from and what the six defects §1.9 records were
  found by: James used it on 2026-08-14, and the panel he used painted over the composer,
  refused to reappear after an Escape, offered no arguments for `/portal`, and wrote a check
  mark that drew as an empty box. Those are fixed and each is pinned; the row that replaced it
  has still only been read as a string in a test. Everything §1.9 asserts is a claim about
  code: as of 2026-08-15 `cargo test -p organon-console --lib` is **683 green** (3 ignored),
  `cargo test -p organon-core` is **593 green**, `cargo check --features console-edition --bin
  organon-console` is clean, and `cargo check --tests -p organic-math-native --features
  console-edition` is clean. **That is the whole claim: it compiles and the tests pass.**
  ⚠️ **That includes the caret fix**, which is the one thing in §1.9 whose *symptom* a human
  reported (`/hxelp`) and whose *cure* no human has typed at: the ordering is pinned by three
  tests driving egui's real `TextEdit`, and nobody has watched a character land after a
  completion in a running window. What
  it does not establish, in order of how much it matters: (1) that eleven verbs in one row is
  legible at a glance rather than a wall of words — which-key is fast because the panel is
  glanced at and then outrun, and a row that has to be *read* is slower than the list it
  replaced; (2) that the brackets read as a selection rather than as punctuation, which
  matters because Tab still takes what they mark; (3) that a line completing itself under the
  hand feels like help rather than like interference — where the caret ends up is arithmetic
  and §1.9 now settles it, but "the box moved while I was typing" is a feeling; (4) whether
  `+9` at a
  narrow width is useful or merely honest, since nobody has seen the row at a width that
  cannot hold it. ✏️ The old ledger's second worry — a band whose height changes with the list,
  jumping the transcript on every keystroke — is **closed by construction**: the compact row is
  one row whatever it holds, and a test pins that. ⚠️⚠️ **Auto-execute has still never been used
  by a human, and as of 2026-08-15 it is ON BY DEFAULT — so this is the most load-bearing line
  in the ledger.** It used to be a switch nobody had flipped, which made it a claim about code
  that cost nothing; it is now what happens to James the first time he types a slash. The
  recoverability rule (§1.9) is what made the default defensible, and it is a *design* argument
  supported by tests, not evidence: every verb that fires is one another command undoes, which
  bounds the cost of being wrong at one command — it does not establish that being wrong is
  rare, or that a command running under the hand feels like help rather than like the console
  jumping the gun. In order of how much it matters: (1) whether a fire on the first settled
  frame is *soon enough to be worth having* and *late enough not to startle* — 16 ms is
  arithmetic, the feeling is not; (2) whether the split lands where a hand expects, in
  particular `/screen full` firing without an Enter (recoverable by F11, but it is the largest
  visible change in the set) and `/help` firing while `/surface` does not; (3) whether
  `Enter runs` reads as *"press Enter"* to somebody who has just watched three other commands
  run themselves — the marker was written for a different question and is being reused for
  this one. ⚠️ `ORGANON_PALETTE_AUTORUN=0` is the escape hatch, and the first thing to reach for
  if any of the three turns out badly. ⚠️ **The command history has never been walked
  by a hand either** — `arrow_owner`'s four cases are pinned as a pure function and driven
  through real frames, but "Up did what I expected" is exactly the kind of claim a test cannot
  make. ✏️ **Correcting this entry, because what it recorded as completion's cost was the
  smaller half of it.** The one-frame caret window was named here as the known price of
  self-completion, and it was true — ✏️ it is **now closed** (§1.9: the caret moves on the
  rewrite's own frame, so `/help` then `x` gives `/helpx`), which is worth saying because the
  entry twice called it a price worth paying and it turned out to be one nobody had to pay.
  What went unrecorded is that **deletion was impossible** —
  every backspace on a uniquely-prefixed verb was undone on the frame it happened, and a
  mistyped command could only be corrected by selecting the whole line. James found it in
  minutes on the first running build (*"once I have typed slash surface, I am no longer able to
  backspace out of it"*). It is fixed and pinned (§1.9's insertion-only rule); the lesson worth
  keeping is that the ledger listed the defect that had been *reasoned about* and missed the one
  a hand meets first, which is the failure mode a ledger written without a user exists to have.
- 🚨 **Nobody has seen the colour editor, and it is a tool for judging colours by eye — so the
  one thing it exists to do is exactly the thing not verified.** Everything §1.10 asserts is a
  claim about code. What it does not establish, in order of how much it matters:
  (1) **that three numeric H/S/V drags are enough to judge a colour by** — the alternative was
  egui's 2D picker, which is a far better instrument per colour and shows one colour at a time,
  and James asked for a list where each row has an editor, so the row won; whether a row is
  enough is a question about a hand on a mouse; (2) that eight rows is the right window, and
  that a highlight-following window pages the way a hand expects on the timeline's eleven and
  the terminal's twenty — the arithmetic is pinned by test, the *feel* is not; (3) that the
  editor's band, which is taller than the candidate list's, does not push the transcript around
  disruptively — §1.9's own ledger already flags band height as an open question and this makes
  the band bigger; (4) that `unsaved` in `mode_alert` on the right of the head row is actually
  noticed, which is the entire defence against a tuning session evaporating at exit; (5) that a
  drag at 60 fps through `set_visuals` on every change is smooth — the change is gated to frames
  where something moved, and the automated review found and removed the one per-tick allocation
  that was on that path (`set_hsva` was building a whole `Theme` plus its sixty-eight-entry field
  list to intern a compile-time constant), but **nothing has measured a sustained drag** and the
  remaining per-change cost — deriving `Visuals` and re-uploading egui's chrome — is real and
  unpriced. ⚠️ **Nothing has been saved
  and reloaded by a human**, so the round trip through `preferences.json` is pinned by unit test
  and by nothing else. ✏️ **The entry as written on `console/theme-editor` said the light page
  was "still `#ffffff`, deliberately". That was true of that branch and is false here** — this
  merge also carries `console/light-white`, so `Theme::light`'s page is `#fafbfc` and the
  editor is the general answer *beside* a specific correction rather than instead of one. It is
  the exact failure mode this document warns about: a claim that was accurate about one branch
  and wrong about the tree it landed in.
- 🚨 **Nobody has seen the window icon rendered, at any size, in any slot.** §1.10 is a
  claim about code and about PNG files, and nothing more: `cargo test -p organon-console
  --lib` is unchanged by it (the icon lives in the root crate), `cargo test -p
  organon-core` is **556 green**, `cargo check --features console-edition --bin
  organon-console` is clean, and `cargo check --tests -p organic-math-native --features
  console-edition` is clean. **That is the whole claim: it compiles and the tests pass.**
  Only James has the display. What has *not* been established, in order of how much it
  matters: (1) that the icon appears **at all** — `decode` returns `None` on failure by
  design, so a window that opens with the OS default is indistinguishable from one where
  nothing was wired up, and the two tests only prove the bytes are a well-formed 48×48 and
  256×256 RGBA, never that winit accepted them or that Windows drew them; (2) that
  `with_taskbar_icon` actually changes the **taskbar button** — that reading comes from
  winit 0.30.13's `platform_impl/windows/window.rs` (`set_taskbar_icon` → `IconType::Big`),
  which is the right call by inspection but has not been watched happen; (3) how the
  **opaque `#0e0d0b` tile reads against a light taskbar** — the artwork is "on-dark" by
  design and was deliberately not changed, but on a light theme it will be a near-black
  square with a gold mark in it rather than a mark, and whether that is right is James's
  call; (4) whether the **48 px raster downscaled by Windows to 16 px** is any better than
  the 16 px render that was rejected as illegible — the arithmetic says a reduction beats a
  direct render at that size, and Windows' own downscaler was never watched do it. The one
  thing that *is* measured rather than argued: the 16 px render was produced, magnified 16×
  and looked at, and it is not recognisable as the aperture mark.
- ✅ **The `#fafbfc` prediction came true, and it is worth recording as a hit rather than
  quietly deleting.** That entry said: *"what it most likely does not establish is that the
  change is big enough to matter … if James still sees glare, the honest answer is not to
  squeeze the remaining units but to take the whole light ladder down together, which is a
  re-spec of four roles he named and therefore his call."* He looked at it the same day and
  said *"the white part is too white. Move it down to about a 0.85 V in the HSV system"* — the
  exact remedy the ledger had named, at the exact scale it had declined to choose. The ledger
  was right about the **direction** and right to leave the decision with him; what it got wrong
  was the framing that made "2.35 % of HSV value" sound like a budget. It was never a budget on
  the *page* — it was the distance to the panel, and the panel was always free to move.
  ⚠️ **The general lesson: a headroom figure computed against a fixed neighbour is a fact about
  the neighbour, not a limit on the thing being measured.** Quoting it as "the room available"
  is what made a 1.18 % move look like half of everything possible when it was 3 % of what was
  actually wanted.
- 🚨 **Nobody has seen `#d7d8d9` on James's display either, and he is the only person who can
  say whether it is right.** The whole light ladder is down a uniform 35 per channel, the page
  landing at **V = 0.851**. Unlike `#fafbfc` this is arithmetic against a number he named
  rather than a value someone reasoned toward, which is a better starting point and still not
  an observation. Everything claimed for it is a claim about code: `cargo test -p
  organon-console --lib` is **647 green** (646 before, plus
  `every_light_plate_mixed_from_a_surface_is_recomputed_from_it`), `cargo test -p organon-core`
  is **557 green**, `cargo check --features console-edition --bin organon-console` is clean, and
  `cargo check --tests -p organic-math-native --features console-edition` is clean. **That is
  the whole claim: it compiles and the tests pass.**

  ⚠️ **The most likely complaint this time is the opposite one: that the page reads as grey
  rather than as paper.** `V = 0.851` is pale grey card stock, not white, and §1.4 says so in
  as many words so that nobody later "fixes" it back toward white. If it is too far, the number
  to move is `LIGHT_PAGE` and the other three follow by the same offset — the constants exist
  for exactly that.

  ⚠️ **The one thing that is measured and unfavourable is text contrast**, and it is recorded
  in §1.4 as a table rather than left to be discovered. `secondary #5d636c` on the panel falls
  5.70 → **4.12**, under AA's 4.5 for normal text; `faint #8b919b` falls 3.06 → **2.22** on the
  page and 2.51 → **1.77** on a hairline plate (it was already sub-AA before this change).
  Primary text is unaffected at 13.3:1. **This cannot be fixed from the surface side** — the
  repair is a darker text ladder, which is three more roles James specified, so it is named
  here and not taken. `#737983` is what would restore `faint` to what it had.
  ✏️ **Taken since, and the costed number was the right one.** `LIGHT_SECONDARY #555b64`
  (uniform −8) puts secondary on panel at **4.66**, and `LIGHT_FAINT #737983` (uniform −24) puts
  faint on page at **3.07** — the 3.06 it held before the ladder moved, to two decimal places.
  Only those two roles moved; `primary`, `success`, `error` and `accent` are untouched and all
  clear AA. `faint` on a hairline plate reaches only 2.45 and is knowingly left there, because
  the role labels something *absent* and darkening it to AA would make "not mapped yet" heavier
  than live secondary text.
  📌 §1.4's table is now `every_light_text_role_is_measured_against_the_surface_it_is_drawn_on`,
  which computes WCAG luminance and asserts each ratio — including the sub-AA exception,
  bounded on **both** sides. The table was prose for a day, and the ladder move had already
  demonstrated what prose is worth here: it changed all seven ratios without touching a single
  text colour, because the two ladders are two sides of one fraction and only one was edited.
  🚨 **Still nobody's eyes on any of it.** Ratios against a standard are not an observation, and
  the darkening is as unlooked-at as the page it repays. The complaint to watch for is the
  opposite of the last one: text that now reads *heavy* on pale card stock.

  Also unverified: (1) that the 3-unit page→panel step, unchanged in absolute terms, still
  reads on a real display now that both sides are darker — the arithmetic says the *ratio*
  improved (1.026 → 1.030) but nobody has looked; (2) that `panel_fill`'s matching move is
  invisible, since a Tier 5 patch panel only appears over a live backdrop and none was running;
  (3) that a TUI's own light colour scheme still reads correctly against a page 35 units below
  the pure white `ansi16`'s GitHub Light lineage was chosen against — the foregrounds did not
  move, and this shift is an order larger than the 3 units the last entry called negligible, so
  here it is a genuine open question rather than a note; (4) that the recomputed
  `timeline_scripted_fill` still reads as a warning banner — its mark's contrast falls
  5.30 → 3.84 against it.
- 🚨 **Nobody has seen a collapsed transcript, so whether it actually *reads* better is
  unverified — and that is the entire point of the change.** Card density was designed against
  a screenshot and a sentence, and everything claimed for it here is a claim about code:
  `cargo test -p organon-console --lib` is **563 green** (539 before, plus twenty in
  `card_density` and four in `conversation_view`), `cargo check --features console-edition --bin
  organon-console` is clean, and `cargo check --tests -p organic-math-native --features
  console-edition` is clean. **That is the whole claim: it compiles and the tests pass.** What it
  does not establish, in order of how much it matters: (1) that six calls collapsed to six
  dense lines is *calmer* rather than merely smaller — a wall of one-line rows is its own kind
  of noise, and the only way to know is to look at a real session; (2) that the **scroll
  stability** construction holds in a window. The two rules are pinned by pure functions and by
  one real-frame test, but "the reader's eye does not move" is a statement about pixels, and no
  card has ever collapsed under a live scroll on this machine; the failure mode, if the
  argument is wrong somewhere, is a jump nobody would catch in a test. (3) That `GROUP_MIN = 3`
  is the right threshold, which is a taste call made without ever seeing a group. (4) That the
  dense row is legible at all — the verb is `prose` and everything after it is `dim`, chosen so
  that colour stays reserved for a failure, and a row that turns out to be too faint to scan is
  a one-token change nobody can make from here.
- ⚠️ **The desktop posture has never been drawn, and neither has the terminal one since the
  wiring.** §1.6 ships at `t = 0.0`, which is meant to be today's console to the point:
  `form_at_terminal_is_the_form_that_shipped` pins all fourteen tokens against values read out
  of `main` *before* a line moved, `nothing_is_wrapped_or_overridden_at_the_terminal_end`
  pins the two `None`s that make the no-change claim structural rather than arithmetical, and
  the midpoint and quarter tests stop a `Form::at` that ignored `t` from passing.
  `cargo test -p organon-console --lib` is **534 green** and `cargo check --features console-edition
  --bin organon-console` is clean. **That is the whole claim: it compiles and the tests pass.**
  ⚠️ **#38 removed the excuse, not the gap**: `organon console posture desktop` moves the
  scalar from a running console, so "nothing can reach `t > 0`" is no longer true and the
  first honest test of the axis is one command away. What
  it does not establish: that `t = 0.0` really is pixel-identical — the wiring moved five card
  frames onto `Form`, threaded `&Form` through nine functions, added a closure around the
  scrollback's walk and now paints two things (the left rule, the ticks) that return before
  touching the painter, and only a running window can say the flow still looks like itself.
  Nothing has ever been rendered at any `t > 0`, so the 90-point gutter, the 1.6 line height,
  the 0.13em tracking, the border/rule exchange and the corner ticks are *specified and
  compiled*, not seen. The first honest test of the axis is somebody moving the scalar and
  looking.
  - 🚨 **UPDATED — the desktop end has now been drawn, once, and the first thing it showed was
    a bug.** James put the console at desktop posture on the evening of 2026-08-13, at roughly
    the default 1100-point width, and reported the margin: *"you can see there's a border on
    the left, a margin, but not on the right."* So the sentence above is no longer wholly
    true, and this is exactly what it was written to invite. **What was seen:** the inset
    itself, on a real window, and that it was left-only — which is what §1.6's `margin` token
    now fixes. **What was still not seen, and what this entry now claims instead:** the
    symmetric margin — `cargo test -p organon-console --lib` is **535 green**, one more than
    before it (`the_content_margin_is_symmetric_at_every_posture`), and both `cargo check`
    legs are clean, so the fix is compiled and pinned and **not looked at**; the line height, the
    tracking, the border→rule exchange and the corner ticks, none of which drew a reported
    observation either way; whether `t = 0.0` is pixel-identical to the console before the
    wiring; any `t` strictly between the two ends; and the desktop end **on a window wide
    enough for the measure to be the problem** — §1.6's margin-versus-measure argument turns
    on that, and it is still an argument rather than a sighting.
    ⚠️ **The general shape, since this ledger keeps finding it:** one look at one window at
    one width falsified one claim and left the rest of the paragraph standing. "Has been seen"
    is not a boolean, and an entry that flips wholesale on the first screenshot is being
    written too coarsely.
- ⚠️ **`Theme::card_left_rule` has never drawn a pixel, though two palettes now ask it to.**
  `light` sets `#c9ced6` and `dark` sets `#363b43`, while `organon` and `chocolate` set it
  fully transparent — and in both of those that is the palette's *answer*, not a placeholder:
  `organon` has four-sided boxes and `chocolate` separates by surface tone alone, so each
  declines the rule by declining to colour it.
  `a_transparent_palette_rule_stays_invisible_at_every_posture` pins that a declining palette
  cannot be made to show one. But the left-rule half of the card-edge exchange only draws at
  `t > 0`, and **nothing has ever been rendered at any `t > 0`** — so the mechanism is
  specified, compiled and pinned, and still unseen.
  ⚠️ **This entry said "a colour no palette in this build makes visible" until the palettes
  and posture were merged together**, which was true of each branch alone and false of the two
  combined. It is recorded rather than quietly rewritten because it is the exact shape this
  ledger exists to catch: a claim that stays true right up until an integration, and that
  nothing in either branch's own tests could have falsified.
- ⚠️ **The kind registry unification (#48 T1) has not been seen on screen at all, and it is a
  refactor whose whole claim is that nothing changed.** `cargo test -p organon-console --lib` is
  502 green (two more than before — one arms-match-the-vocabulary test per placement), `cargo test
  -p organon-core` is 544 green (four more — the round trip, the no-approximation rule, the
  refusal carrying the known list, and the happy arm of the refusing path), and `cargo check
  --features console-edition --bin organon-console` is clean. **That is the whole claim: it
  compiles and the tests pass.** No patch has been claimed in a running terminal since the
  move, no `/surface` has been summoned, and the refusal sentence has never been read by a
  human off a real console — it is pinned by a test that asserts the known words appear in it,
  which is a different thing from anyone having seen it.

- ⚠️ **No preferences file has ever been written on a real machine — and #38 changed the
  reason, not the fact.** §1.5 is pinned by ten headless tests against temp directories — round
  trip, missing file, malformed file, a BOM'd file, an unknown key, a missing key, the atomic
  replace, the stranded-temp check, first-run directory creation, and that the store root is
  literally `SessionLog::store_root()`. `cargo test -p organon-console --lib` is **534 green** and
  `cargo check --features console-edition --bin organon-console` is clean. **That is the whole
  claim: it compiles and the tests pass.** What changed: this used to be "a writer with no
  writer", because nothing called the module. `Console::new` now calls `load_default()` at
  startup and `Console::set_theme` calls `save_default()`, so the caller exists — but it has
  never run. Nothing has written a real `%APPDATA%\OrganonShell\preferences.json`, no console
  has read one back, and no preference has survived an actual exit, because no console has been
  opened on this code at all. ⚠️ **The first `organon console theme` anybody types is the first
  honest test of the durable promise, and it exercises the whole chain at once** — store root
  resolution on Windows, `create_dir_all`, the temp-then-rename, and the read on the *next*
  launch. Read the console's stderr while doing it: a failed save says so there and nowhere
  else.

- ⚠️ **The theme extraction has not been seen on screen, and "the look did not change" is a
  claim about a test rather than about a window.** `theme_organon_is_the_look_that_shipped`
  compares every field against the literal RGB read out of `main` before the move, `cargo
  test -p organon-console --lib` is green (484 tests, one more than before — the new one checks
  that `indexed_256`'s first sixteen come from the theme), and `cargo check --features
  console-edition --bin organon-console` is green. That proves no *value* drifted. It does not
  prove the console still draws it in the same place: the extraction also moved four `&Theme`
  borrows through the draw path and rewrapped a dozen call sites, and only a running window
  can say the strip, the composer and the grid still look like themselves.
- 🚨 **Nobody has ever seen `light`, `dark` or `chocolate` — but as of #38 somebody CAN, in one
  command, and that is the change: the obstacle is no longer the code.** These are ~150 colour
  values and two structural decisions, and the entire claim is still: `cargo test -p
  organon-console --lib` is **534 green** and `cargo check --features console-edition --bin
  organon-console` is clean. **It compiles and the tests pass.** What that establishes is narrow
  and worth naming: every hex James specified is on the field he specified it for, `organon` is
  byte-unchanged including its chrome, names resolve and fall back, no scrim setting crosses any
  palette's floor, and the chrome derivation moves no geometry. What it cannot establish is
  anything a palette is *for*. No window has been opened on one. ⚠️ **This entry said "indeed
  nothing can open one, because nothing selects a palette yet" until the verbs landed** —
  `organon console theme light` in a running console is now the whole procedure, and there is
  no longer any excuse for this entry to still be here at the next revision. Specifically
  unverified, in the order they are most likely to be
  wrong: whether `SCRIM_FLOOR_LIGHT = 192` actually leaves a legible page over a live
  backdrop (it is reasoned, not measured, and the reasoning is in the constant's doc);
  whether **dropping amber** leaves "a tool is running" distinguishable from prose at a glance
  when the only difference is which field it came from and both are primary text; whether the
  derived `Visuals` covers *every* egui surface a real session shows — sliders, scrollbars and
  popup frames were reasoned about, never looked at; whether `light`'s GitHub-Light ANSI reads
  correctly under an actual TUI; and whether `chocolate`'s greys land as "warm graphite" rather
  than as flat neutral, which is the one thing a channels-equal assertion explicitly cannot
  answer.
- 🚨 **No agent has ever called `console.camera.read`, and the number it would return has never
  been checked against a picture.** Built on this machine without launching the console:
  `cargo test -p organon-console --lib` (486 pass, 11 of them this module's) and `cargo check
  --features console-edition --bin organon-console` are green, and the pure half — the JSON shape,
  the non-finite omission, the provenance rule, read-time `hand_holds`, the unpublished cell —
  is pinned by test. **What that does not establish:** that the three axes an agent reads back
  correspond to the shot on screen; that the publication point in `redraw` really lands after
  every camera writer in a *live* frame rather than only in the source order I read; and that
  `moved_by: "hand"` appears after an actual drag on an actual portal. The first real use is
  the measurement — frame something, read, compare — and it needs a window.
- ⚠️ **`backdrop_shows_world` is what the console is *rendering*, not proof anything is
  legible.** It is `render_source() == World`, the same predicate `frame_camera` warns from, so
  it inherits that predicate's whole meaning and no more: a world backdrop rendered at a scrim
  the glyphs sit on top of still reports `true`. `visible` answers "would a move show up
  anywhere", never "can you see it".
- ⚠️ **The read cannot see a framing an agent posted moments earlier.** A write travels the
  sidecar and lands on the next frame's drain; the read answers from the last *published* frame.
  So set-then-immediately-read can return the previous framing, and that is not a bug the read
  can fix from its side — it is the write lane's fire-and-forget shape, one frame wide. Nothing
  papers over it; the reading is labelled for what it is.
- ✅ **RESOLVED 2026-08-13: the second reason was CRLF line endings, and the fix is a
  `.gitattributes` pin.** Claude Code parses a skill's YAML frontmatter from between two
  `---` fences and its parser does not accept `---\r\n`, so on a Windows working tree the
  frontmatter fails to parse and the skill degrades silently: `name` falls back to the
  directory (which is why it was in `slash_commands` and looked installed), `description`
  falls back to the body's first heading, and it is **never offered to the model**. Measured
  against a real `claude -p` session with three sibling skills as controls — `organon-cli`
  was the only one of four with CRLF (LF=318, CR=318; the others CR=0) and the only one
  missing from `skills`. Converting a **byte-identical** copy to LF took the offered count
  22 → 23 and restored its real description.
  ⚠️ **The junction hypothesis recorded here was tested and FALSIFIED** — a real directory
  copy fails identically. So were file size (a 23 KB sibling loads), description length (a
  607-byte sibling loads), a BOM, duplicate copies, a colliding slash command, a disabling
  setting, and the skill's own name. Every one of those was ruled out by experiment, and the
  measurement that mattered was a byte count rather than a `grep -c`, which had reported the
  siblings as CRLF too.
  ⚠️ **The index was always LF** (`git ls-files --eol` → `i/lf w/crlf`): nothing was ever
  committed wrong, the file is correct in the repository and broken on disk, for Windows
  checkouts only — which is what put it out of reach of review. Note this was the *third*
  fix in the same place: the skill was a git symlink (unusable on a Windows checkout), then
  a real tracked file, which is precisely what gave it a CRLF working copy. Each fix
  uncovered the next failure. The working-directory fix this entry originally qualified was
  necessary and is now also sufficient.
- ⚠️ **Nothing in this change has been seen running.** No GPU here, and the console was
  deliberately not launched. The four resolution rules, the home stop, the notes and the
  bare-directory warning are pinned by ten headless tests; `cargo check --features
  console-edition --bin organon-console` is green. That the notes actually *appear* at the head
  of a live conversation tab's scrollback — the right colour, the right place, not colliding
  with the empty-transcript placeholder — is unverified.
- 🚨 **Nobody has seen the portal. Not one pixel of it, and not one interaction.** It was
  built in a cloud session with no GPU: the state machine, the rect arithmetic, the wheel
  claim, the CLI round trip and the one-render-per-frame invariant are pinned by headless
  tests, and `cargo check --features console-edition --bin organon-console` is green. **None of
  that is evidence that anything appears on screen.** Specifically unanswered, and each needs
  James at the machine: whether a 42 %-width 16:9 rect at the top right reads as *floating*
  rather than as a hole in the terminal; whether the phosphor hairline is enough to make it an
  object; whether the World at the console's default snapshot is *legible* at that size (the
  console publishes `OrganicMathParams::default()`, which nobody has looked at through a
  window this small); whether the drag orbits at a rate a hand likes; whether the wheel claim
  feels right or merely correct; and whether one frame of `Theme::panel_fill` before the first render
  reads as a beat or as a flicker. The verb is the only part with a cheap self-check —
  `organon console portal open` prints `queued: portal open`, which says the line was written,
  not that anything drew.
- 🚨 **Nobody has seen the camera verb either — not one frame of it moving.** §1.3 was built in
  the same shape as §1.2 and with the same limit: `cargo test -p organon-console --lib` is 438 green
  and `cargo check --features console-edition --bin organon-console` is clean, and **neither is
  evidence that a picture moved.** What is genuinely proven is the arithmetic and the policy: the
  hand-hold's boundary behaviour, the visibility predicate, the wire round trip in every
  combination of the four flags, the malformed set, and that the schema's three bands are
  literally `scene_input`'s constants. What is **not** proven, and each needs James at the
  machine: whether `--distance 40` frames the default world *usefully* (520 is where it opens and
  nobody has looked at that world through a 42 %-width rect to know what "close" means in it);
  whether `--reset` looks like a return or like a glitch; whether two seconds of hand-hold feels
  protective or obstructive in an actual back-and-forth; and whether the stderr advisory ever
  fires when it should, since nothing on this branch has run with a substrate backdrop and a
  closed portal. The cheap self-check is the same one the portal has — `organon console camera
  --distance 40` prints `queued: camera distance 40`, which says the line was written.
- ⚠️ **A refused camera command reaches nobody but a reader of the console's stderr.** The lane
  has no return path by design, so an agent that is held off by the hand learns nothing at all: it
  issued a command, the command was accepted by clap, written to the sidecar, validated by the
  service, recorded — and then dropped. From the agent's side that is indistinguishable from a
  camera that does not work. The console prints why; nobody may be reading. This is the concrete
  cost of the missing read path in §2 and it is the strongest argument for building it.
- ⚠️ **Two-thirds of §1.3's tests cannot be RUN on a Windows workstation session**, and the split
  is worth knowing before trusting a green line. `camera.rs`'s six live in `organon-console` and run
  in 0.17 s. The `CameraFraming` round-trip and range tests live in `src/cli.rs` and the schema
  tests in `src/console_main.rs`; both need the root package's test binary, which is ~45 minutes of
  codegen here, so they were **compiled and not executed** (`cargo check --profile test`, three
  targets, exit 0). CI runs them. The pure-crate half was deliberately given the safety-critical
  decision — who owns the camera — so the part that can be *proved* here is the part that matters
  most. ⚠️ **That split has a cost, and review found it**: the schema tests all called `op_from`
  or `op_args` directly, so none of them crossed `CommandService::dispatch`, and the optional-arg
  bug above lived in exactly that gap. The fix is pinned where it can be run — `command.rs`, in
  the pure crate, 480 tests in 0.17 s — with the whole-lane test beside the ones it joins in
  `console_main.rs`. **When a bug lives between two components, put the test in whichever of them
  can actually be executed here**, and let the crossing test be the compiled-only one.
- ⚠️ **The wheel claim is enforced in the TERMINAL front-end only.** `term_view` reads
  `raw_scroll_delta` directly, so it gets the explicit rect test; a **conversation** tab's
  `ScrollArea` reads egui's smoothed delta in its `end()`, which has already run by the time
  `paint_portal` registers the region and zeroes it. So in a conversation tab a wheel over the
  portal zooms **and** scrolls the transcript. The drag is fine in both (registering after the
  content wins the tie, which is `scene_input`'s own tested property). Fixing it means
  registering the region before the scroll area, which costs the tie for drags and needs
  `SceneMode::Immersive`'s hit-test walk to give it back — a real design step, not a patch, and
  out of this tier's one beat.
- ⚠️ **A scene patch shows nothing while the portal is open**, by construction (§1.2, the
  render budget). It returns when the portal closes. This is a documented cut in service of the
  one-render invariant, not a bug, but it is a visible regression in an unrelated feature and
  belongs here rather than only in a doc comment.
- 📌 **The portal does not resize while the window does.** A window-resize drag changes the
  pane every frame, so the portal frees and reallocates its texture — and logs one `[surface]`
  line — on every frame of the drag, exactly as an open conversation surface already does. The
  fix is the settle rule recorded in §2, and it was deliberately not built here: the animated
  grow is the thing that makes it urgent, and building the rule without the animation would be
  guessing at what it has to serve.

- 🚨 **The conversation view has never been run against a live agent by the session that
  wrote it.** Every rule in §1.1 is pinned by headless tests against committed captures —
  the per-block key, the replayed human turn, the recurring `init`, the per-turn `result`,
  ✏️ the subagent scope, the card's clipping, the `Edit` diff's alignment and the result
  detail's four measured fields — and that is **replay, not a conversation**. What no fixture can answer: whether the CLI stays alive
  when it is spawned with no prompt and nothing on stdin yet (it prints `Warning: no stdin
  data received in 3s…` and the pane logs it, but "proceeds without it" could mean it
  exits), whether stdin's line write reaches it promptly enough to feel live, and what the
  layout looks like at real width. A person on the machine is the first to know. ✏️ **All
  three of those have since been answered on screen** (demo script beat 7, 2026-08-12): a
  real two-turn conversation with a tool card in it, and the composer re-checked by James
  after it was rebuilt. The entry stays because what it describes is a *method* — the
  fixtures are still replay, and everything added to this view since is unseen until
  somebody looks — which is what the composer and status-strip entries below record.
- **A conversation tab REPLACES an invocation; it never observes one.** There is no attach
  in any of Claude Code's programmatic surfaces, so the tab cannot mirror a session
  already running in a terminal. This is a product consequence wearing a protocol costume
  and is recorded as such (§5.9.1).
- ✏️ **Subagent output is rendered now, but it is still not live, and that half is
  permanent.** Milestone 1 dropped it entirely; it is now folded onto the tool card that
  spawned it (§1.1, "A subagent is not a turn"), so a coordinator run shows what its agents
  are doing instead of a spinner. 🚨 **What did not change is the measurement underneath:**
  Claude Code never forwards token deltas from a subagent, so activity still arrives as
  complete bursts minutes apart, and no amount of rendering can make it a live feed. The
  card reports *counts and completed steps*, never liveness, because counts are what the
  wire honestly carries. A view of a coordinator will still be quieter than the work is —
  it is now quiet in a way that says what is happening.
- ✏️ **The subagent fixture is a real capture now — and nobody has still seen the card on
  screen.** The two halves of this entry came apart on 2026-08-13. The *wire* was measured:
  a real two-agent fan-out was driven through the console's own argv and replaced the
  reconstruction (`fixtures/claude_stream_subagent.jsonl`, and `fixtures/README.md` for
  what it corrected). The correlation held; three of the reconstruction's shape claims did
  not — the tool is called `Agent` and not `Task`, the wire stops at depth 1, and no
  subagent in the capture said anything at all. 🚨 **The pixels are still unverified.** The
  card has never been drawn from a real fan-out by anyone who looked at it, and a capture
  cannot answer that any more than the other fixtures could answer the conversation view's
  first entry above. Same class, same remedy: somebody runs one and looks.
- ✅ **Somebody looked, and looking found a defect no replay could.** James ran a real
  fan-out in the console on 2026-08-13. The structure was right — `subagent · 2 steps` with
  a nested step beneath it, the shape the path predicted — and the **step marker was tofu**,
  a missing-glyph box where the mark belonged. That is the tofu section's fourth row, and it
  is this ledger paying for itself: no headless test could have caught an absent glyph, and
  the entry above said in as many words that only somebody looking would. 🚨 The fix was not
  the one assumed either — the draw site already asked for a monospace face, and reading
  egui 0.33's four bundled `cmap` tables showed the *character* was in none of them.
- ✏️ **The subagent lifecycle is rendered now — the gap above is closed, and closing it
  corrected the description of it.** ~~The console renders none of the subagent lifecycle
  the CLI actually sends… a **gap, not a decision**: it was never weighed, because until
  this capture nobody knew the lines existed.~~ It has been weighed. A dispatch card now
  carries a `task` row: what the agent said it is doing, the tool it last ran, its tool
  count, the harness's elapsed and its tokens, and a terminal status — thirteen lines of
  the capture that used to draw nothing (rule 5b, `agent_map.rs`).

  🚨 **Two of the five do not correlate the way that entry said, and building on its
  description would have lost work silently.** `task_updated` carries a `task_id` and a
  `patch` and **no `tool_use_id`** — so a correlation keyed on `tool_use_id`, which is what
  "each naming its card by a `tool_use_id` field" licenses, drops every status transition
  in the stream. And `task_summary` carries **neither** key, only a nullable `detail`: it
  is a gloss of what the *session* is doing and belongs to no card at all, so it stays
  unmapped. The key is `task_id`, learned against a card from any line stating both.

  📌 **And the family reaches depth 2, where every other subagent line stops at depth 1.**
  A nested agent's `task_*` lines *are* forwarded, naming a call that exists only as a step
  inside its grandparent's log. They are **declined and counted**
  (`Stats::nested_subagent_progress`): a card holds one progress value with nowhere to
  record a depth, so merging them would have made the outer card read "Reading one.txt ·
  1 tool · completed" — the grandchild's work in the parent's voice, while the parent was
  still going. That is the honest next increment, and the number is the measure of what is
  being given up. ⚠️ Four `task_*` lines on the capture, counter reads **3**: the
  `task_started` lands one line before the `tool_use` block that creates its card and goes
  to `orphan_subagent_progress` instead. **3 here + 1 there**, each half pinned.

  ⚠️ **What has *not* changed is the liveness measurement**, and the row is built so it
  cannot drift into implying otherwise: no caret, no partial text, and the elapsed is the
  harness's own stopwatch as of its last line, frozen between them — `conversation.rs` has
  no clock, and a ticking number would be the view's arithmetic in the harness's voice,
  still counting for an agent that had died. `MapStats::subagent_stream_events` still reads
  **0** on this capture and is still the canary.

  ⚠️ **Nobody has seen this on screen.** Same class as the entry above it, which took a
  human looking to find a tofu glyph no replay could: the row is pinned by tests against
  the real capture, and every glyph in it is one `step_mark` already measured present in
  Hack — but the pixels are unverified. Somebody runs a fan-out and looks.
- **The card's clipping is the VIEW's, and it says what it hid.** `conversation.rs` leaves
  per-element text unbounded on purpose — a tool result can be a whole file, and
  truncating it in the model would misrepresent the tool's output while looking like the
  tool's output. So the card draws ten lines and then says "+N more lines"; the full text
  is still in the transcript. Same for an argument value: flattened to one line with a
  character count, never quietly cut.
- **Permissions are answered, and a human has now driven the card — which is how the
  deadline was found.** The path was verified end to end against the real CLI before
  anyone clicked, and every wire shape held; what a test responder could not show was that
  the client stops waiting after 60 s. It does, and the card outlived the call. That is
  fixed above. What is still only reasoned about: whether three buttons in a scrollback are
  the right affordance, and whether the auto-scroll puts a question in front of someone who
  is reading back.
- ⚠️ **The keep-alive is verified against a probe, and the abandoned card against a socket —
  not against a slow human at the real console.** `mcp_http`'s two live-socket tests drive
  the whole path headlessly (progress out, answer back, hangup closes the question), and the
  60 s / 300 s numbers come from `claude.exe` 2.1.228 answering a standalone probe server.
  What nobody has yet done is leave a real card unanswered for five minutes and then click
  it. The mechanism is measured; the sitting-there is not.
- ⚠️ **The decision memory is session-scoped and unaudited.** It lives in the pane, dies
  with the tab, and is written nowhere — so a decision cannot be reviewed after the fact,
  and closing a tab silently forgets everything it was told. That is the honest trade for
  this tier (a remembered decision that outlives the window it was made in is one the human
  cannot find again), but "the console remembered" currently means "until you close it".
  **The session-wide allow inherits every word of that**, deliberately: it is the same
  memory widened, and a blanket allow that survived a restart is the one grant that must
  not be quietly inherited by a session nobody was watching.
- 🚨 **The §7 re-measurement has NOT been performed against the server as it now stands, and
  that is the one thing in this section that is asserted rather than measured.** The console
  now serves capability tools from the same server as the permission handler, and §9 point 4
  is explicit that the withholding guarantee must be re-checked per server. The session that
  built this could not launch the console or produce a release binary, so no `system/init`
  from this build has been read. What exists instead is the machinery to answer it in one
  glance: `ExposureAudit` prints its verdict to stderr and to the band at every init. **The
  first person to run this build should read that line before trusting a card.** It is one
  line, it names all three states, and if it opens with 🚨 the model can answer its own
  approvals and the console's authority is decorative.
- ⚠️ **Nothing has driven a capability tool end to end.** The dispatch is unit-tested
  (the tool call becomes the line the CLI would have written, on the same sidecar, and an
  out-of-range `block` is refused before anything is written), and the drain that consumes
  that line was already tested — but no agent has yet called `mcp__organon__console_portal`,
  so the card naming a capability instead of a shell command has been *built* and not *seen*.
  `capability_label` renders the name; whether the resulting card reads better than the Bash
  one it replaces is still a claim.
- 📌 **A capability tool costs the agent a `ToolSearch` before it can be called** — MCP tools
  arrive deferred in the measured build. So the first use of a console verb in a session is
  slower than the `Bash` call it replaces, and only the second onwards is cheaper. Measured
  in the protocol doc, not by this change.
- ✏️ **The `Edit` diff is an alignment now, and what is left unverified is the *look*.** This
  entry used to read *"a field render, not a diff algorithm — no alignment, so an edit that
  changes one character in the middle of a ten-line block shows ten removals and ten
  additions"*, and that is no longer true: `text_diff::line_diff` aligns it, and the
  one-character case is pinned by the test the change was written for. 🚨 **What no test can
  answer** is whether three lines of context is the right amount at the console's real width,
  whether `… N unchanged lines` reads as a summary or as a missing row, and whether a
  `MAX_ROWS`-capped diff looks bounded or looks broken. Nobody has seen a single row of it on
  screen. ⚠️ Two things are named rather than claimed: the alignment is **recomputed every
  frame** and is bounded rather than cached, so `MAX_CELLS` is a per-frame budget and not a
  statement about how large an edit can be; and past that budget the rendering **is** the old
  field render, so the failure mode this entry used to describe still exists — it is now
  labelled on the card instead of being the only behaviour.
- ✏️ **`tool_use_result` is rendered, and the fields it does not render are the honest part.**
  Only `filePath`/`numLines`/`startLine`/`totalLines`, because those are what a real capture
  contains; every richer field a card would want is absent because nothing has been observed
  sending it. ⚠️ **Two shapes are captured and only one is readable**: the `two_tools`
  capture's `Read` results, and the subagent capture's `Agent` results, which carry no `file`
  object and are declined. What a `Bash`, a `Write` or an `Edit` puts here is still *unknown
  on this machine*. 📌 This entry used to say both captured details were `Read`s and that
  `MapStats::tool_details_declined` was the counter that *would* catch a third shape — it
  has, on the first capture that contained one, which is a designed safeguard reporting
  rather than a regression.
- 🚨 **Thinking blocks still render nothing, and that is a refusal with a date on it.** The
  decoder reads them (`ContentBlock::Thinking`, text plus the opaque signature) and the
  transcript spends a block ordinal on them per rule 1, so the wiring is in place and the
  view simply draws nothing. **No real capture on this machine contains one** — the only
  fixture that has a thinking block is `claude_stream_edges.jsonl`, which declares itself
  hand-written — so rendering them would mean building a second path against an unobserved
  shape, which the subagent entries above already record the cost of once. Re-scope it the
  first time a capture shows one; until then the honest state is that a model's reasoning is
  invisible in this front-end.
- **`/surface` is a temporary summoning seam and is not the feature.** The feature is the
  element; the command is scaffolding that exists because agent-summoned artifacts are the next
  step. ✏️ **What has changed is everything around it**: it is now one entry in the command
  registry (§1.8) rather than a two-arm match, and `/surfaces` is **refused with the known
  list** instead of being forwarded to the agent — a refusal leaves the words in the composer,
  so the "over-recognising swallows a real message" hazard the old rule was defending against
  is closed more tightly than exact-matching closed it. `/panel`, which was the other one, is
  still **removed** — see the summoning seam above for why and for what came out with it.
- ⚠️ **The slash surface has not been driven by a hand.** It is green, unit-tested end to end
  in `registry.rs` (parse, refuse, help) and pinned against the real verb table in
  `console_main.rs` — but no one has yet typed `/background slate` into a running console and
  watched the backdrop move, and `/help`'s placement at the head of the scrollback is a
  judgement made from reading the layout rather than from looking at it. The tests that
  matter most here are the ones a green build cannot supply: whether the receipt lands where
  a person's eye is, and whether refusing an unknown command reads as helpful or as fussy.
- ✅ **The composer has been driven by a human, and the keystroke contract holds on real
  hardware.** Checked 2026-08-12 on organon-one by James at the keyboard, in a live
  conversation tab: three rows at rest, the hint carrying the contract, and **Enter sends
  while Shift+Enter inserts a newline, confirmed by keypress** — the one thing a green
  build could not have told anyone, because egui's shift-permissive matching fails
  silently. His words were *"it's all working beautifully."*
- ✏️ **The status strip HAS now been seen on screen — and only the strip that was there
  when he looked.** This entry used to read *"the status strip has never been seen on
  screen by anyone"*, and that is no longer true: James drove a live conversation tab on
  organon-one on 2026-08-12 with the strip and the composer under it, and his words were
  *"It's a great start. It's working very well."* So the two questions this entry existed
  to ask are answered for the band as it stood — the model plate reads as an identity, and
  the band is legible at real width.

  🚨 **Everything added to the strip since is unseen, and the list is longer than the seen
  part.** None of these has been drawn in front of a human: **`Standing::Generating`**
  (and with it the question only a person can settle — whether the documented fall-through
  to "ready" between two messages of one turn reads as an honest gap or as a flicker,
  since a frame-or-two blink is exactly what a fixture cannot have an opinion about); the
  **model picker** and its `→ Sonnet` pending annotation; the **permission-mode plate**,
  its picker, and the persistent amber marker whose whole design claim is that it stays
  legible across hours without becoming wallpaper; and the **tofu fix**, of which only the
  first of the sites was ever confirmed broken on screen and none has
  been confirmed *fixed* on screen. **421 green tests in the compositor lib are not a
  substitute for having looked once** — and the strings being pinned is explicitly only
  half of the tofu fix, since the `.monospace()` that makes them draw is at the draw site
  where no test can reach it.

  ✏️ **Two more join that list, and the first is the one a person has to settle.** The
  **empty ring track** is drawn from the first frame at `CONTEXT_TRACK_EMPTY`, and the whole
  argument for it rests on a claim no test can make: that a fainter circle beside a
  brighter one reads as *"waiting for a reading"* rather than as *"a reading of nought"* —
  or worse, as a ring someone forgot to finish. A test can prove the two colours differ; it
  cannot prove the difference is legible at the edge of the eye, on this display, at this
  scaling, which is exactly where the ring lives. If it is not, the fix is a wider gap
  between the two values, not a return to drawing nothing. And the **cold-start cost chip**
  (`session $0.0000`) is unseen: nought spent is true, but whether four decimals of nothing
  reads as a meter at rest or as clutter is a judgement about a band, not about a number.
  ⚠️ Also unseen: the **new subagent step markers** `•` / `×`, which replace the two
  dingbats that drew as boxes — the glyphs are measured present in egui's fonts, but "present
  in the font" and "reads as *returned* and *failed* at `.small()`" are different claims.

  ⚠️ Two questions from the original entry also remain open, because nothing since has
  addressed them: whether the model plate's **hover is discoverable at all** when nothing
  on screen suggests hovering — a question that now costs more, since the mode plate's
  hover is where the full consequence sentence for the mode in use lives — and whether a truncated
  diagnostic line beside three chips is legible when the band is also carrying two plates
  and a marker. ⚠️ And the coordinator cannot find out — its synthetic clicks do not reach
  this app and its synthetic keystrokes leak into another window (demo script beat 7), so
  this needs a person at the keyboard exactly as the surface does.
- 🚨 **No control has been clicked by a human, and the two failure modes that would show
  up first are the two nothing headless can reach.** `set_model` and `set_permission_mode`
  are pinned byte-for-byte against the protocol doc's captured lines, the correlation is
  tested with an ack matched, an ack belonging to nobody and a request never answered, and
  the deadline sweep is tested against a clock — but **no request has gone down a real
  pipe from a real click**. What that leaves genuinely open: whether the pending `→ Sonnet`
  annotation clears in a time a person reads as *responsive* (the repeat `system/init` is
  the only thing that clears it, and nothing measured says how long after the ack it
  arrives), and whether `initialize` at spawn is answered at all before the session's first
  init — a case the protocol doc measured for `set_permission_mode` and **not** for
  `initialize`, whose failure is a silently empty model picker.
- 🚨 **The rendered surface has never been drawn on screen by the session that wrote it.**
  The model link, the visibility test, the panel→surface join, the cap's eviction order and
  every knob's lane are pinned headless. Nothing headless can answer the questions that
  decide whether it is a good instrument: whether the picture reads as a *surface* at 260 pt
  rather than as a stripe, whether one frame of latency is perceptible on a drag, whether
  the surface and its panel are comfortably on screen together at real width, and whether
  the substrate at this framing is interesting enough to be worth looking at. And the
  coordinator's synthetic mouse input is measured **not to reach this app** (demo script
  beat 7), so only a person at the keyboard can answer any of them.
- **A surface's rig is not the console's rig.** `organon console rig daylight` changes the
  backdrop and leaves every surface alone, deliberately: a surface is meant to be answerable
  to the controls beside it, and inheriting a value typed into another tab is the coupling
  the element exists to remove. The consequence is that the two can look different, and
  nothing on screen says why.
- **A conversation pane carries an inert look-epoch ledger.** The three per-tab vectors
  stay index-aligned so every `zip` and `get(active)` in `console_main.rs` remains safe, so
  a conversation gets a `PaneLooks` it never uses and opens epochs at line 0. The backdrop
  is not drawn behind a conversation at all — the banding is scrollback arithmetic and
  there is no scrollback. That is now a decision rather than a gap: a rendered **surface**
  is the picture a conversation gets, in a rect of its own that a control beside it drives,
  and a full-bleed backdrop behind a transcript remains available and unclaimed.
- **The backdrop is the DEFAULT LOOK of the engine**, not a live external system:
  Console writes the default `Shared` itself and the CLI's override lane mutates the
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
  `substrate_materials::MATERIAL_NAMES` / `RIG_NAMES`, and `console_main.rs`'s command
  schema is built from those same constants and asserted to accept exactly what its
  resolver does. ⚠️ **The three source words are not.** `world`/`off`/`substrate` are
  `BackdropSource`'s value space, which lives in `console_main.rs` — another `[[bin]]`, and
  no `bin` can import another — so the literal appears twice (`CONSOLE_SOURCES` there,
  `BACKDROP_SOURCE_WORDS` here) with each side asserting it against its own resolver.
  Change one and that side's test fails naming the other: two smoke alarms, one missing
  wire. The fix is a `pub const` in `cli.rs` beside `parse_console_op`, already the
  declared home of "both ends speak one vocabulary from one place"; it was left undone
  because `cli.rs` is another leaf's file this tier only reads.
- **History is STRETCHED after a pane resize, and that is the honest choice.** A cached
  epoch picture is frozen at the size the pane was when that look closed; a resize leaves
  it at the old resolution and `band_quads` samples it into the new band anyway. The
  alternative is re-deriving a past look's `Shared` and re-rendering the world once per
  epoch, which is precisely the unbounded GPU cost the cap exists to prevent — and it is
  what "do not build a restyle-everything path" rules out. The **live** band, the one the
  eye is actually on, is always exact; only history is approximate, and only after a
  resize. `EpochLedger::plan` still reports those epochs as `Rerender`; the integrator
  declines that arm on purpose, in a comment that says so.
  ✏️ **That cut is only honest while the live texture is the size it claims to be.** The
  first beat check found wide historical bands rendering as blurred washes with the live
  band crisp — the same stretch, but from a size nothing had resized: the backdrop was
  built as `pane_points × remembered_scale` and spent its first frames in points (see
  §"sized to the terminal pane" above). Measured on this machine: an epoch picture of
  1100×690 painted across a 2475×1553 pane. So "history is stretched **after a resize**"
  now means what it says; a picture that was never the right size was a bug, and
  `scene_input::pane_pixels_in`'s regression test is what keeps it one.
- **A closed World epoch's band is one frozen frame of something that was moving.**
  Switching *to* `world` collapses history, for the reason the plan gives — a live world is
  not a still life. Switching *away* from it (`world` → `graphite`) snapshots like any other
  close, so the rows written while the World was live keep a single frame of it. That is
  true about what was behind those rows and it is still a still of a moving thing; the live
  band, which renders every frame, is the only one that animates.
- **A band whose look has no picture paints nothing.** An epoch that closed while the
  backdrop was `off`, or that predates the tab it is in, has no texture and no way to get
  one. Its rows show the panel's own background — which is what was behind them at the
  time — rather than borrowing the neighbouring look's picture. The scrim over it is a
  no-op by construction: it is `Theme::term_scrim_tint` with an alpha, painted onto `Theme::term_bg` — the same three channels.
- **The `dropped` counter UNDER-counts in two regimes, never over-counts.** Once
  scrollback is full (10 000 lines), an eviction is only observable through the display
  pin — so at the live edge (`display_offset == 0`), and parked against the top of a full
  buffer, the counter stalls. History then looks *older* than it is, band edges stop
  ageing until the pin comes free, and the count is exact again the moment it does. The
  two honest fixes are outside this tier: raise `SCROLLBACK_LINES`, or count
  `Grid::scroll_up` at the source (a counter inside `alacritty_terminal`, or a `Handler`
  decorator around `Term`). The demo is three orders of magnitude away from the cap.
  Separately, a **column** resize re-wraps rows and genuinely renumbers lines, so a band
  edge can slide by the number of wrapped rows above it; row changes are exact.
- **Look history is per tab, and a tab opened later has none.** Each pane's ledger starts
  collapsed at line 0 with whatever the console is wearing, because it has no rows from
  before it existed. `EpochId`s are unique only *within* a pane, which is why the texture
  cache lives on the pane rather than on `Console`.
- ✏️ **The banding has now been seen on a GPU, and it cost one bug.** Every claim above is
  still arithmetic pinned by headless tests — the row→band mapping, the tiling, the length
  law, the beat as a state machine — and the beat check (ORGANON-ONE, RTX 5090, 225 %
  display) confirmed the parts only a screen can answer: the boundaries are row-aligned,
  they stay pinned to the text through a full scroll, and the new look scrolls in from the
  bottom. What it also found is the sizing defect recorded three entries above — wide
  historical bands reading as blurred washes while the live band stayed crisp. Instrumented
  at the capture site, a cached epoch's band was then measured **pixel-identical** to a live
  render of the same look at the same pane size, twice (`paper` and the undressed
  substrate), which is what located the fault in the size rather than in the bands. The
  second beat check answered the seam question: a boundary reads as a clean, deliberate
  material edge — row-aligned, no bleed into the neighbouring band. Still unjudged: whether
  a stretched history after a genuine window resize looks acceptable.
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
- The CLI op-path can print a cosmetic "queued" warning in-Console — the ops drain
  fine. ✏️ **Phase 0 (2026-08-10) corrected this entry:** `is_live()` probes the
  `Shared` seq counter for motion (`organon-core/src/ipc.rs`) and never reads the
  Feedback channel, and Console **does** write `Shared` each redraw
  (`console_main.rs` — the publish under `redraw`). The warning is a redraw-cadence
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
  or (b) Console's reader not draining ConPTY, a product bug. **Both were wrong.** A
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
