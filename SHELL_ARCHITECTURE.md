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
- **Look-epochs — the backdrop applies FORWARD, history keeps its own (#4 T4)** — a
  `background` or `rig` command closes the live look at the current absolute scrollback
  line and opens the next one **just below the cursor**, so the new look scrolls in from
  the bottom as output pushes the old text up, and every older region of the buffer keeps
  the look it was written under. Nothing is ever restyled after the fact; there is no
  restyle-everything path and building one was explicitly declined (the execution plan's
  ⚡ Sequence amendment).

  Three pieces, each of which knows nothing about the other two:

  1. **`scroll_anchor.rs` (organon-shell) — the arithmetic.** Absolute line indices:
     `abs = screen_top + grid_line`, `screen_top = dropped + history_size`, **derived every
     frame rather than accumulated**, which is what makes emission age a boundary for free,
     scrolling move the window rather than the text, and a *row* resize need no bookkeeping
     at all (`grow_lines` pulls lines out of history and the cursor follows by the same
     count). Bands partition the viewport, edges monotone, and the **alt screen is always
     exactly one band** — the alt grid is built with zero scrollback, so its geometry says
     nothing about absolute lines. No egui, no alacritty, no wgpu.
  2. **`substrate_epochs.rs` (root crate) — the ledger.** Which look ran from which line,
     `Look` = `(material, rig)` **names**, never bytes. `MAX_EPOCHS = 8`, which is a
     stateable ceiling rather than an adjective: 63.3 MiB of pane-sized RGBA8 at 1080p,
     253.1 MiB at 4K (`worst_case_bytes`, pinned by test). Past the cap the two oldest
     epochs **merge**, the **newer** of the pair surviving so the loss concentrates on the
     rows furthest from the cursor, and the loser comes back carrying the exact stderr line
     to print (`[epochs] evicted graphite/studio @ line 1200 (cap 8)`). `plan()` returns
     the texture decisions as data — releases first, the live epoch second, the rest oldest
     first. It owns no GPU object and no scroll geometry.
  3. **`shell_main.rs` — the wiring.** One `PaneLooks` per tab (anchor + ledger + texture
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
  `organon-shell` — which is where the whole console lives — it is unreachable; the only caller
  is `term_view::PaneAnchor::feed_local`, and `PaneAnchor::bracketed` is now the single function
  both it and `PaneAnchor::pump` route through. Unbracketed, a feed against a full buffer with
  the user scrolled into history evicts lines that `advance_dropped` never sees, which raises
  the true `screen_top` without raising the derived one — and every absolute index recorded
  before the feed is then permanently wrong, silently.

  `PaneAnchor::feed_local` returns the **absolute line index of the first reserved row**, taken
  from the *pre-feed* `ViewState` by the same `boundary_now` a look-epoch uses. The identity a
  painter gets: the block occupies `at ..= at + rows - 1` and the cursor rests on `at + rows`.
  `Shell::open_block` logs it unconditionally — `[block] opened 12 rows @ line 1187 (pane 0)`,
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
    `Shell::render_source` renders the substrate into the pane target and only the patch quads
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

  **A panel's buttons enter the command lane rather than imitating it.** `organon-shell` cannot
  see `substrate_materials` and must not learn to, so a `BlockPanel` carries labels handed down
  by `shell_main.rs` and reports which one was pressed; `redraw` feeds that to
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
  `cli::PatchKind` over `cli::PATCH_KIND_WORDS` — one table, read by clap's possible-values
  parser, by the console's `ArgKind::Choice` schema and by `PatchKind::from_word`, so `--help`
  cannot offer a kind the console has no way to paint. Two asymmetries on the wire, both
  deliberate: a line with **no** kind is `scene` (what a claim meant before there was a choice,
  which keeps the verified arm working byte for byte), while an **unknown** kind skips the line
  — a newer CLI naming a kind this build cannot draw must not have a guess painted into a
  rectangle someone else's output is holding open.

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

So a tab is now one of two things ([`Pane`] in `shell_main.rs`):

| | What it is | Status |
|---|---|---|
| **Terminal host** | runs any program, paints its grid, patches only by cooperation | **unchanged.** It is how `htop` runs and it is the universal fallback |
| **Conversation view** | consumes an agent's structured event stream and renders it natively | **new.** No claim protocol, no anchoring, no PTY, no ConPTY |

They share the window, the tab strip, the harness registry, the console command lane and
the backdrop. Below that they share nothing, which is why `Pane` is an enum rather than a
flag: a conversation has no grid, no cursor, no scrollback and no absolute-line
coordinate, so every terminal-only path (`open_block`, `claim_patch`, the anchor pump, the
epoch boundary) goes through `Pane::term_mut()` and skips it by construction.

**Five modules, four of them harness-agnostic.**

- **`agent_event.rs` — the decoder.** NDJSON → typed events. `EventStream::push(&[u8])`
  owns its own line buffering, because a chunk boundary mid-line is the normal case (one
  `tool_result` can carry a whole file). Events carry `session_id`, an `AgentScope`
  (`Main` / `Subagent { tool_use_id }`, decoded from `parent_tool_use_id`, whose `null` is
  meaningful) and a `kind`. An unknown *event* is never an error — it decodes to
  `Unknown` with the body preserved. Tested against three committed captures in
  `organon-shell/fixtures/`, two of them real.
- **`conversation.rs` — the transcript model.** `Transcript::apply(AgentEvent) -> Change`,
  folding into ordered `Element`s (`Human | Assistant | Tool | RunEnd | Artifact`) with
  stable `ElementId`s. Its `AgentEvent` is **its own input enum, deliberately not the decoder's**
  — two modules cannot own one type, and a transcript fluent in the wire format would
  change shape every time the wire did. No egui, no clock, no I/O.
- **`agent_map.rs` — the seam, and the only file in the tree that knows both types.** A
  second harness (Pi, §5.9.1) is written here or the model is wrong.
- **`agent_session.rs` — one live child.** The same shape as `term.rs`: a reader thread
  that only moves bytes, a channel, a pull drained once per frame. Pipes, not a PTY.
- **`conversation_view.rs` — the drawing.** Scrollback above, composer below. Returns a
  `ConversationOutput`: the buttons pressed, and the **rects its rendered surfaces landed
  in** — in points, never pixels, which is the whole of what the console needs to size a
  render target for one.

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
5. **Subagent-scoped events are dropped in milestone 1**, and counted
   (`MapStats::subagent_dropped`). They belong *inside* the tool card that spawned them.

**The process contract (§5.9.2, measured):** `-p --input-format stream-json
--output-format stream-json --include-partial-messages --replay-user-messages --verbose`
keeps **one session alive across many turns** — one `session_id`, a `result` per turn.
Spawn once per tab and never let the process go. Resume is the recovery path, not the
interaction model. There is **no attach**: every programmatic surface is a child process
you spawn, so a conversation tab cannot mirror a Claude Code session running elsewhere —
it must *be* the session.

**The inline artifact, and why it is the milestone.** A terminal receives a tool call as
whatever text the harness chose to print. The event stream carries it structured — name,
the complete input object, a correlation id, a later result — so `conversation_view` draws
a **card**: the tool's name, its arguments as fields, an accent that says running (amber)
/ ok (green) / error (red), and the output clipped with a count of what was clipped.
`Edit` goes one further and renders its `old_string`/`new_string` as a real diff, because
those arrive as *fields* rather than as a patch someone has to parse back out of prose.
"A tool is running" has no event anywhere in the stream; it is derived from an unresolved
id, and it stops being true when the result arrives.

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
| Button labels are **handed down** and come back by label | `shell_main.rs` | `organon-shell` learning about `substrate_materials`; a pressed button re-enters `apply_console`, the same call `organon console background <name>` reaches |

#### The rendered surface — a control and its consequence in one glance

Beat 7 checked the panel on screen and the check produced the finding: **its effect appeared
on a different tab from the one it was clicked in.** A conversation has no scrollback for a
backdrop to band across, so `/panel`'s buttons changed the console's backdrop and the only
place that shows is the terminal next door. A control whose consequence you cannot see from
where you are sitting is a bad instrument, and no amount of wiring fixes it.

`ArtifactContent::Surface` is the answer. **`/surface` summons two elements**: a rendered
surface, and directly beneath it a panel whose `PanelSpec::drives` names that surface's
`ElementId`. The buttons and knobs then change the picture a few rows up, in the same view,
while the hand is on them — and a driving panel's button is **consumed by its surface** and
never reaches `apply_console`, so it cannot also repaint a backdrop somewhere else.

| Question | Answer | Where |
|---|---|---|
| Where does the rect come from? | **egui layout** — `allocate_exact_size`, full column width by `SURFACE_HEIGHT` (260 pt). The terminal host derives a patch's rectangle from absolute lines, a scroll anchor, a cell height and a reflow rule; the conversation view has one call. That is the simplification the second front-end buys | `conversation_view::surface_element` |
| How is it rendered? | The **one** `World`, into a target the conversation owns — `render_to_texture` at `BACKDROP_FORMAT`, the substrate rig re-framed for that rect's aspect. `Shell::render_source`'s seam exactly: what the engine draws is not what the backdrop paints, so the window behind stays flat and James's "opens like an ordinary terminal" rule is untouched | `Shell::render_surfaces` |
| How is it sized? | `scene_input::pane_pixels_in(swapchain, rect_points, window_points)` — the rect's **fraction of the window** applied to the swapchain, so `pixels_per_point` cancels. The view hands points across the crate seam and never a scale, which is the arrangement that function's doc exists to protect | `shell_main.rs` |
| How does a look reach the engine? | `surface_shared` = `look_shared(Substrate, look)` with the knobs applied last, published through the same `Shared` channel the backdrop uses, then the console's own snapshot is put back so `organon status` never reports a surface's lane | `shell_main.rs` |
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
`Shell::apply`'s `Close` arm therefore frees **every** surface texture — one wasted re-render
against a class of bug where one conversation paints into another's rectangle.

**Summoning is deliberately a separate seam.** `/surface` typed in the composer is recognised
by `conversation_view::local_command`, acted on locally and **never written to stdin**;
`Transcript::insert_artifact` is a method rather than a ninth `AgentEvent`, because no
harness said this and putting it in the event enum would oblige every mapping to carry an
event none of them can produce. That is what makes the next step small: the agent summons
a surface with a tool call, the integrator answers it with the same `insert_artifact`, and
the local command is deleted without touching anything that draws.

🚨 **`/panel` is gone, and its machinery with it.** It summoned a panel wired to the
console's *backdrop* — which a conversation has no scrollback to band across, so the effect
landed on a terminal tab and the panel you had just clicked appeared to do nothing. Driving
one, James's reading was "the controls don't do anything… it's redundant", and it was.
`PanelSpec::drives` is therefore an `ElementId` rather than an `Option<ElementId>`: **a panel
cannot be built that does not name a target in its own transcript**, so the failure is not a
policy the view has to keep enforcing. `ConversationOutput::actions` and `ArtifactAction`
went with it — the only producer was the console-driving arm — and so did `shell_main`'s
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
| The card | allow · allow & remember · deny, with the arguments shown as fields | `conversation_view.rs` |
| The memory | `DecisionMemory`, keyed on tool **plus canonicalised arguments** | `approval.rs` |

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
hand itself `{"behavior":"allow"}`. Verified live against this server: `system/init` reported
`[{"name":"organon","status":"connected"}]` with **zero** of its 36 model-visible tools
mentioning `organon`. Any other approval-ish tool would be an ordinary model-callable one
with no such protection.

**The server serves the handler and nothing else, for now.** `McpServer` generates capability
tools from the same `CommandSpec` table the CLI is generated from, but the console constructs
it with an **empty** table: routing a console verb needs a `CommandService`, which borrows the
session log on the UI thread and cannot be moved onto a serve thread. So the model sees a
connected server with nothing it can call — the safe shape for an approvals tier, and the
seam is `NoDispatch`, named rather than implied.

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

## 2. Seams the next tiers consume

| Coming | Builds on | Issue |
|---|---|---|
| Viewport interaction + provenance (T2+) | T1's pane (`shell_main.rs::ScenePane` + `app.rs::SceneView`); camera input rides `scene_input`'s region pattern — never a second gesture vocabulary. The world gate is already `any(mind, shell)`; `World` stays unforked (#618 owns its extraction) | Shell #6 |
| Content-addressed artifact store + lifecycle UI + evidence viewers | `session::Artifact` (metadata landed in #4 T1); payloads beside the log in the session dir | Shell #4 T2+ |
| Command service T2+: core_catalog seeding + real targets | `command::CommandService` landed in #5 T1 (dispatch + catalog + the every-dispatch-leaves-a-record invariant) and is **live in the product since Console Spike T2** (`console.background` / `console.rig`, seeded from `substrate_materials`' tables, dispatched from the frame path). T2+ adds the bin-side `core_catalog`→`CommandSpec` adapter, the runtime target over the CLI override lane + snap request/reply sidecar, and the policy engine that makes `Denied`/`Requested` real — never a second vocabulary | Shell #5 |
| Conversation view milestone 2 | Milestone 1 landed the whole path (decoder → `agent_map` → `conversation` → `conversation_view`, one live child per tab), the inline artifact (`Body::Artifact`) and the rendered surface it drives (`/surface`). `/panel` has since been deleted — it drove the console backdrop, which a conversation cannot show. Next: the **agent** summoning one, via a tool call the integrator answers with `Transcript::insert_artifact`, with the tool card as the anchor. Then, in the order §5.9.3 holds them: subagent events rendered *inside* the tool card that spawned them; `tool_use_result` (the undocumented structured per-tool detail a rich card wants); then Pi as the second harness, mapped onto the same eight transcript events — never a second event vocabulary | Console Spike §5.9 |
| Approvals, next steps | The card, the in-process MCP-over-HTTP server and the session-scoped decision memory landed together (§1.1, "The approval card"). Next, in order of what a session actually costs: the console's own verbs served as capability tools (needs a `CommandService` reachable from the serve thread — `NoDispatch` is the named seam), so a card can say *"organon · background"* instead of a shell command; then a memory that survives the tab, with the audit trail a durable one obliges | `doc/console_approval_protocol.md` |
| Pi bridge / workers / PTY | T1 landed the workspace side (`mock_agent.rs` + `timeline.rs`: every `EventKind` rendered, pull-tick replay). Next: a real adapter *behind the same tick shape*, approval decisions routed back as events — never a second event vocabulary | Shell #7 T2+ |

**IPC rule inherited whole:** any new Shell channel — mmap, sidecar, socket — goes
through `ipc.rs::ns_file` under the `organon-shell` namespace. A hard-coded `$TMPDIR`
path silently breaks the three-products-simultaneously guarantee that
`edition.rs`'s pairwise-distinct-namespace test pins.

## 3. Honesty ledger

- 🚨 **The conversation view has never been run against a live agent by the session that
  wrote it.** Every rule in §1.1 is pinned by headless tests against committed captures —
  the per-block key, the replayed human turn, the recurring `init`, the per-turn `result`,
  the dropped subagent scope, the card's clipping and the `Edit` diff — and that is
  **replay, not a conversation**. What no fixture can answer: whether the CLI stays alive
  when it is spawned with no prompt and nothing on stdin yet (it prints `Warning: no stdin
  data received in 3s…` and the pane logs it, but "proceeds without it" could mean it
  exits), whether stdin's line write reaches it promptly enough to feel live, and what the
  layout looks like at real width. A person on the machine is the first to know.
- **A conversation tab REPLACES an invocation; it never observes one.** There is no attach
  in any of Claude Code's programmatic surfaces, so the tab cannot mirror a session
  already running in a terminal. This is a product consequence wearing a protocol costume
  and is recorded as such (§5.9.1).
- **Subagent output is dropped, and on a coordinator run that is most of the activity.**
  Token deltas from subagents are never forwarded by the CLI at all, so even in milestone
  2 a fan-out session arrives as complete-message bursts rather than live text. Milestone
  1 drops those messages entirely (counted, not silent). A view of a coordinator will look
  much quieter than the work actually is.
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
- **The console serves no capability tools over MCP.** `McpServer` generates them from the
  command table and the console passes an empty one, because dispatch needs a
  `CommandService` bound to the UI thread. So the legibility argument for MCP — an approval
  card naming *"organon · background"* instead of a shell command — is **built but unused**:
  `capability_label` renders the name, and nothing yet produces one.
- **The `Edit` diff is a field render, not a diff algorithm.** It prints `old_string`'s
  lines as removals and `new_string`'s as additions — there is no alignment, so an edit
  that changes one character in the middle of a ten-line block shows ten removals and ten
  additions. That is honest about what arrived; it is not `diff`.
- **`/surface` is a temporary summoning seam and is not the feature.** The feature is the
  element; the local command is scaffolding that exists because agent-summoned artifacts
  are the next step. Exact-match only (`/surfaces` and `/surface slate` go to the agent),
  because over-recognising swallows a real message while the composer clears either way.
  `/panel`, which was the other one, is **removed** — see the summoning seam above for why
  and for what came out with it.
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
  stay index-aligned so every `zip` and `get(active)` in `shell_main.rs` remains safe, so
  a conversation gets a `PaneLooks` it never uses and opens epochs at line 0. The backdrop
  is not drawn behind a conversation at all — the banding is scrollback arithmetic and
  there is no scrollback. That is now a decision rather than a gap: a rendered **surface**
  is the picture a conversation gets, in a rect of its own that a control beside it drives,
  and a full-bleed backdrop behind a transcript remains available and unclaimed.
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
  no-op by construction: it is `DEFAULT_BG` with an alpha, painted onto `DEFAULT_BG`.
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
  cache lives on the pane rather than on `Shell`.
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
