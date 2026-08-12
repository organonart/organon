# Changelog

Organon was built in the open for about a year before this repository existed, in a
private monorepo, across ~430 changes. **This file is a reconstruction of that arc** —
what got built, roughly in order — rather than a replay of it. Individual PR entries, and
the issue numbers they reference, stayed private with the original.

From here on, this file gets an entry per meaningful change, newest first.

---

## Unreleased

### Console Spike — a live control panel as an element in a conversation

- **`/panel` in a conversation tab puts a working egui panel in the transcript** — sliders
  that move and material buttons wired to the console's real look-change path, the same
  `apply_console` a typed `organon console background <name>` reaches. The terminal host
  needs a whole protocol to put a rectangle in its page (a printed gap, a claim,
  absolute-line anchoring, surviving ConPTY); here an artifact is an element in a list that
  draws itself, and that difference is the argument for the second front-end. The transcript
  stays pure: `Body::Artifact` carries a *description* — a title and control **names** —
  while every value a hand can move lives in the view, keyed by `ElementId`, which is what
  those stable ids were for. `/panel` is a temporary seam; the agent summoning a panel with
  a tool call replaces it.

### Console Spike — a claimed rectangle has a kind, and one of them is a working panel

- **`organon console patch --up N --rows M --kind <scene|panel>`.** The writer prints its own
  gap through the ordinary PTY and then says where it is; the console records the rectangle
  and paints it, and never writes into the terminal. `scene` samples the rendered substrate
  through those rows — today's behaviour, unchanged. `panel` puts a **live egui control panel**
  there instead: sliders that move when dragged and buttons wired to the console's real
  look-change path, so clicking `metal` inside the scrollback and typing
  `organon console background metal` are the same call from `apply_console` onwards. The kind
  selects the paint and nothing before it — the claim, the anchor arithmetic and the per-pane
  ledger are common to both, which is where an error would be invisible on screen.

### Console Spike — the console grows a second front-end: a conversation, rendered natively

- **A conversation tab drives an agent over pipes and draws its event stream itself** — no
  PTY, no ConPTY, no character grid. `ORGANON_SHELL_TABS=claude-chat` opens one beside the
  terminal tabs, which are unchanged and remain the universal fallback. A tool call renders
  as a **card** (name, arguments as fields, running/ok/error, clipped output) and an `Edit`
  as a real diff, because both arrive as structure rather than as the text a terminal would
  have printed. Five modules: `agent_event` decodes the NDJSON, `conversation` folds the
  transcript, `agent_map` is the seam between them, `agent_session` owns the child process,
  `conversation_view` draws it. `SHELL_ARCHITECTURE.md` §1.1 owns the shape and the honesty
  ledger says what has and has not been seen running.

### Console Spike — the console can open a hole in its own transcript

- **`organon console block <rows>` reserves a contiguous run of blank rows** in the active
  tab, just below the cursor, and the next prompt lands underneath them. Nothing is painted
  into them yet — a GPU-rendered scene or panel pinned into exactly those rows is the next
  increment. What this one buys is that the rows genuinely **exist**, so text written
  afterwards flows below them instead of over them.
- **The mechanism is the parser the console already owns**, not a second one.
  `TermSession::feed_local` advances `vte::ansi::Processor` against bytes the console
  generated itself — the same call the PTY pump makes — so a fed `\r\n` takes exactly the
  path the shell's own newline takes. A reserved row is an ordinary scrollback row: it ages,
  scrolls, evicts at the cap and reflows on a resize, because it *is* one. Three tempting
  alternatives are wrong and are named in the code: writing newlines to the pty master
  presses Enter at the child, `insert_blank_lines` discards rows off the bottom instead of
  filing them in history, and a bare `grid_mut().scroll_up()` skips damage tracking and
  never moves the cursor.
- **The feed is bracketed like a real pump, and cannot be called any other way.**
  `TermSession::feed_local` is `pub(crate)`, so it is unreachable from the console except
  through `term_view::PaneAnchor::feed_local`, and `PaneAnchor::bracketed` is now the single
  function every parser advance routes through. An unbracketed feed against a full buffer
  can evict lines the scroll anchor never learns about, which would make every absolute line
  index recorded before it permanently wrong, silently.
- **The command reports where the hole went**: `[block] opened 12 rows @ line 1187 (pane 0)`
  on stderr, in `[epochs]`' register, because an arithmetic error in that index is invisible
  until something is painted into the wrong rows.
- **The first console argument that is a number**, which the lane had not carried before. The
  row count is bounded twice on purpose: clap's range is where a person gets a good error
  before a byte is written, and the dispatch checks it again because `ArgKind::Int` has no
  bounds to state and a hand-written sidecar line never met clap. A count that does not parse
  is skipped like an unknown verb rather than clamped — a clamp opens a block nobody asked
  for.
- **Six limits are written down rather than fixed**, all accepted: a width change reflows and
  can drop the block's topmost row outright, eviction erodes a block from the top invisibly
  at the live edge, `\x1b[3J` wipes the scrollback silently, a resize under the alternate
  screen moves the primary grid, feeding under the alternate screen writes into a grid with
  no scrollback, and the once-per-frame sidecar drain is out of band with the PTY byte
  stream — so the index is right only while the child is idle. The in-band fix, a private OSC
  scanned in the pump, is a later increment.

### Console Spike — the parameter ranges are true, and pinned

- **Nine of the 45 scriptable parameters advertised a range the engine does not have.**
  `agent.rs::id_range` and `clip.rs::RANGES` are hand-written mirrors of `params.rs` that
  nothing checked, and they had drifted. `trans_amp_x/y/z` claimed a maximum of 200 against
  the engine's 20 — a factor of ten, and published that way in
  `doc/reference/parameters.md`. `exposure`, `bloom_intensity`, `sss_power`, `irid_scale`
  and `cam_damping` were wrong at one end or both, and `cam_path` offered a twelfth camera
  path that does not exist. All nine now read what `params.rs` says.
- **Nothing errored when they were wrong, which is why they stayed wrong.** An agent told a
  param runs to 200 when it stops at 20 gets no complaint — it gets a silent clamp and a
  look it did not ask for. `recipe.rs` was validating every built-in recipe against the same
  bad bounds, so the guard meant to catch an out-of-range recipe was holding the wrong ruler.
  It reads `id_range`, so it is corrected by the same change; every shipped recipe was
  already inside the true bounds.
- **A test now pins both tables to the parameters themselves.**
  `taper_round_trips_against_the_engine_range` reads every bound off
  `OrganicMathParams::default()` instead of restating it — a third copy of a number is just
  a third thing to drift — so the next disagreement fails the build. Only the id-to-field
  join is hand-written, and the compiler checks that.
- **It also pins the taper, over all 1372 host parameters.** Treating a range as two numbers
  is only honest while the mapping between them is linear, which is an accident of `flin()`
  today rather than a guarantee. The day someone reaches for a skewed range, two numbers
  stop describing the parameter — and this fails then, rather than after the tables have
  quietly gone wrong again.
- Two CC slots are exempt, each for a stated reason: slot 16 carries
  `inc_scale × 10^speed_exp`, an expression rather than a parameter, whose narrower CC span
  is a deliberate playable range; slot 26 has been reserved since the Pulse Depth knob was
  removed. Both are argued at the table and named in the test, so a slot that gains a
  parameter has to be joined rather than left alone.
- `doc/reference/parameters.md` regenerated.
### Console Spike — Tier 4: it scrolls, and it remembers

- **A look applies forward; history keeps its own.** `organon console background <name>`
  now closes the current backdrop look at the line the cursor is on and opens the next one
  below it, so the new look **scrolls in from the bottom** as output pushes the old text up,
  and every older region of scrollback keeps the look it was written under. Scroll up
  through a session with three changes in it and you scroll back through three looks.
  Nothing is ever restyled after the fact — there is no restyle-everything path, on purpose.
- **The picture of a look is taken when it stops being live.** The backdrop texture already
  *is* that look's rendering, so it is copied into a texture of its own the moment the look
  changes, before the next one renders. That is the whole mechanism: no past look is ever
  re-derived or re-rendered, which is what keeps the cost bounded and the history honest.
- **A small, honest, logged cap.** Eight epochs — 63 MiB of pane-sized textures at 1080p,
  253 MiB at 4K, stated rather than described. Past it the two oldest merge, the newer look
  surviving so what is lost is furthest from the cursor, and every eviction prints
  `[epochs] evicted <look> @ line <n> (cap 8)` to stderr. Eight is deliberately small enough
  that the eviction path actually runs in a long session instead of being untested safety
  code.
- **`background world` and `background off` collapse the history** instead of adding a look.
  A live world is not a still life and freezing a frame of it would be a lie labelled a
  look; `off` has no picture at all. The rows written while the backdrop was off keep their
  plain background afterwards, because the epoch that owns them has no picture to paint.
- **Two new pure modules, both testable without a GPU or a window**:
  `organon-shell/src/scroll_anchor.rs` (absolute line indices → viewport bands: emission
  ages a boundary for free, scrolling moves the window not the text, a row resize needs no
  bookkeeping, and the alternate screen is always exactly one band) and
  `native/src/substrate_epochs.rs` (the ledger, the cap, the merge, and the texture
  decisions as data).
- **Fixed on the beat check: the backdrop was sized in points, so history was magnified.**
  The first run on a 225 % display showed wide historical bands as blurred washes with the
  live band crisp. The bands were right — a cached epoch measured pixel-identical to a live
  render of the same look at the same pane size — and the *size* was wrong: the backdrop
  texture was built as `pane_points × remembered_scale`, and the value standing in for a
  scale egui had not reported yet multiplies exactly like a real 100 % display. So the live
  texture spent its first frames at 1100×690 where 2475×1553 was meant, and a look closing
  in that window filed a picture 2.25× too small — which the live texture then outgrew and
  the snapshot never could. The pane is now sized as its **share of the window** applied to
  the swapchain (`scene_input::pane_pixels_in`), so the scale cancels rather than being
  guessed, and a point-sized backdrop is unrepresentable rather than merely unlikely.
- Known and recorded rather than hidden: a resized pane **stretches** cached history into
  its bands (the live band stays exact), the eviction counter under-counts rather than
  over-counts once scrollback is full, and a column resize can slide a band edge by the
  number of wrapped rows above it. `SHELL_ARCHITECTURE.md`'s honesty ledger carries each
  one with its reason.

### Console Spike — Tier 2: a backdrop you can type at

- **`organon console background <name>` changes the console's backdrop live.** Four
  substrate materials (`graphite`, `paper`, `slate`, `metal`) and two lighting rigs
  (`studio`, `daylight`), plus the three sources `world` / `off` / `substrate` — typed in a
  console tab, applied on the next frame, with no window flicker and no terminal
  relayout. `organon console rig <name>` picks the light.
- **A third command transport, because it has a third destination.** Console verbs append
  to their own namespaced sidecar (`<ns>-console.txt`) drained by the **console**, not to
  `cli.txt`, which is drained by the World and cannot reach `Shell` state at all. The
  drain reuses the World's own file-length watermark and its construction-time seed
  verbatim, so a backlog from before the console started never replays while a command
  typed a moment after launch always does. Unknown verbs and unknown names are both
  skipped in silence — that is the format's whole versioning story.
- **The #472 material gate now admits the membrane sheet.** `render.rs` split one
  predicate into `cube_draw` (the bevel morph, unchanged) and `material_draw`, so a flat
  lit plane can carry a procedural map stack — the thing Tier 1 recorded as the reason its
  substrate had no surface variation. A uniform-value gate, not a pipeline one, and inert
  with no material configured.
- **The command service is live in the product for the first time.** `console.background`
  and `console.rig` are registered on `organon-shell`'s `CommandService`, with their
  argument schemas built from the material and rig tables themselves, and every drained op
  is dispatched through it — so each one leaves a `CommandRun` record in a real session
  log, success and rejection alike.
- **One table, one guard.** The `organon` CLI's clap value lists are now asserted equal to
  the tables the renderer draws from, so a name that completes is a name that can be drawn.
  (The three *source* words are pinned by a literal on each side rather than bound — the
  two ends are separate `[[bin]]`s; recorded in `SHELL_ARCHITECTURE.md`'s honesty ledger.)
- Launching with `ORGANON_SHELL_BACKDROP=substrate` still publishes Tier 1's snapshot byte
  for byte: no material is applied until one is named.

### Console Spike — the binary is `organon-console`

- **The artifact is renamed; nothing an identifier is read by changes.** The `[[bin]]`
  target `organon-shell` is now **`organon-console`** — so it is
  `cargo build --release --features shell-edition --bin organon-console`, and the old
  spelling now fails with "no bin target named `organon-shell`". Explicitly *not* renamed,
  because each of these is read by something outside this crate: the **crate**
  `native/organon-shell` (`-p organon-shell` still names it), the cargo **feature**
  `shell-edition`, every **`ORGANON_SHELL_*` environment variable**
  (a shipped flag surface), the **`organon-shell` IPC namespace** (a wire identifier the
  `organon` CLI joins on), `%APPDATA%\OrganonShell`, and `SHELL_ARCHITECTURE.md`'s filename.
  Collapsing those needs deprecation aliases and a coordinated change, not find-and-replace.
- **The console introduces itself as Organon Console.** The window title, the `--help`
  header and usage line, `--version`, the startup banner and the `organon-console:`
  diagnostic prefixes now read the public name from a local `PRODUCT_NAME`, deliberately
  shadowing `EDITION.product_name()` — which stays "Organon Shell" because `organon-core`'s
  `Edition` is shared spine. A test pins the split: the header must not say "Organon Shell",
  and `ORGANON_SHELL_BACKDROP` must still be there.

### Console Spike — Tier 1: the lit substrate

- **A second backdrop source for Organon Shell.** `ORGANON_SHELL_BACKDROP=substrate` puts
  one flat, still, lit plane behind the glyphs instead of the generative world: a pure
  `Shared`-state builder (`substrate_scene`) drawn through the existing
  `RenderPath::Membrane` — no new shader — and framed by a pure narrow-lens camera rig
  (`substrate_camera`) at a 10° vertical FOV, re-framed on every resize. `=1` still selects
  the world, unchanged, because the `organon` CLI's override lane drains inside the world's
  frame path and replacing it would kill the console's live response.
- **The world gained an absolute camera rig.** A third arm on the camera finalization
  overrides centre/yaw/pitch/distance/roll/FOV as a set and latches off the auto-follow while
  it is installed; the FOV clamp floor moved 10° → 4° at *both* of the two sites that clamp it
  (moving one alone does nothing).
- **Fixed: the backdrop was vertically squashed.** The texture was sized to the window and
  then painted at UV 0..1 into a panel 30 points shorter. It is now sized to that panel, one
  frame behind, with the same clamps the Mind editor's viewport uses — which also changes, and
  corrects, the existing `ORGANON_SHELL_BACKDROP=1` rendering.
- The legibility scrim's alpha is now a pure `term_view::scrim_alpha`, with its structural
  floor pinned by a test against hostile input.

---

## Before this repository

### The instrument

The first thing that existed was a plugin. Organon began as a faithful reimplementation
of *Organic Math*, a cube-field visualizer, and the reimplementation immediately raised
the question the whole project has been answering since: what else can this generator do?

- A **VST3/CLAP plugin plus a standalone**, built on nih-plug, with the fullscreen visual
  as a **separate process** reading a memory-mapped snapshot. Two processes was an early
  and load-bearing decision: a host's audio thread can't be blocked by a renderer.
- Host **tempo sync** via a PLL, MIDI CC routing, clip export, and audio-reactive band
  analysis — the parameters move to the music, which is the point of it being a plugin.
- The algorithm itself, isolated as pure unit-tested functions: rotate-then-translate
  composition (mirroring OpenGL's `glRotatef`/`glTranslatef` order, which is what makes
  the motion organic rather than mechanical) and a fourth accumulating strand that
  compounds transforms without reset — the source of the tentacle and helix families.

### The renderer

What began as instanced cubes became a full real-time stack, because each new generator
asked for a way to be *seen*:

- PBR materials, image-based lighting, punctual lights, HDR output with true EDR on
  macOS, tone mapping, palettes, SSAO.
- Screen-space reflections, bounced GI, spectral glass with dispersion, a hardware
  ray-traced path, voxel GI, temporal accumulation, and a post stack.
- 27 generators and a matching set of surface modes — how nodes become geometry: swept
  tubes, metaballs, membranes, voxels, neural tissue, plexus, splats.
- Later arrivals: a time-marched **field engine** (PDEs on a grid), a **kaleidoscope**
  pass, a **creature engine**, a **neural-network generator** with a gallery of
  synthesized graphs, and an HDR starfield driven by the embedded Yale Bright Star
  Catalog.
- An in-app **production recorder** for capturing HDR clips, and a **frame harness**
  (`native/verify`) that turns rendering into pass/fail against committed goldens — the
  only test in the project that can see a picture.

### Organon Mind

Then the engine was pointed at something other than music: a language model.

- A **GGUF reader** that parses a model file's header and tensor directory and draws the
  model's **true wiring** — layers, heads, experts, the residual stream — as a structure
  in the same 3-D engine.
- An **activation ring** for live inference, an **embedded llama.cpp runtime** behind an
  opt-in feature (the default build stays C++-toolchain-free), and a synthetic frame
  writer that exercises the whole live path with zero inference.
- A set of **lenses** — quantitative instrumentation, an inference-geometry atlas,
  concept views — and the commitment that governs all of them: **every displayed quantity
  is labeled with its provenance** (measured / derived / proxy / projection), with a
  standing ledger recording which is which and what is still a proxy.

### The workshop around the code

Roughly half the effort that does not show up on screen:

- The **`organon` CLI** — drive the running instrument from a terminal: read state, set
  parameters, apply recipes, take a snapshot.
- **Preset and clip machinery**, an app-support store, network galleries.
- **Documentation discipline** that is enforced rather than hoped for: architecture docs
  updated in the same change as the code, session hooks that measure doc drift, structure
  drift and the context each session costs.
- **CI** running the full edition matrix, and an automated review agent on every PR.

### Taking the engine apart

With three products sharing one codebase, the monolith was split — carefully, in tiers,
with the acceptance test written down first:

- **`organon-core`** — the host-free spine: math, IPC, params, GGUF, editions. Its
  acceptance test is a dependency check: no plugin framework, no GPU crates, no UI.
- **`organon-render`** — the renderer as its own crate: the surface modules, the shaders,
  the star catalog.
- **`organon-mind`** — Mind's own code, free of the plugin framework.
- The `World` god-struct partitioned into ownership clusters, then made
  compiler-enforced rather than conventional.
- A measured answer to "are these crates actually independent?" — cross-crate churn,
  computed by a script rather than asserted, because a number nobody can re-derive rots.

### More platforms

- A **Windows port**: bundle and deploy scripts, DLL-lock handling, HiDPI for the
  standalone, true HDR through the scRGB swapchain, CUDA for the embedded runtime, and
  Windows legs in CI.
- A **WebGPU port** of the same `math.rs` compiled to WASM — built, then deliberately
  **parked**. It is not in this repository: development is Rust-native only, and shipping
  a parked port would have meant publishing a second, staler answer to the same question.

### Organon Shell

The third product: an agent-operating workstation. Founded, built as a five-panel
workspace, falsified by actually using it, and re-founded as a **GPU-composited
terminal** — its own crate and a third `Edition` over the same engine.

### Open-sourcing

The work that produced this repository: an audit of the whole tree, a licensing decision
(permissive engine, GPL plugin — see [`LICENSING.md`](LICENSING.md)), and an export tool
that materializes this public tree from the private monorepo with a byte-identity gate
and a fatal privacy scan, so that what is published is exactly what was reviewed.
