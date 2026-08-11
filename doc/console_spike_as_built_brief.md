# Console Spike — as-built brief

**Status: ANSWERED.** Phase 0 ran 2026-08-10 on organon-one: six read-only reconnaissance
agents (Opus 5, dispatched concurrently per `console_spike_execution_plan.md` §4), gathered
and cross-checked by the coordinator, plus the Tier 0 verification §2 requires. Every tier
below reads this instead of re-deriving it. Where an agent's claim contradicted another's,
the coordinator re-read the code and the resolution is recorded here — the losing claim is
named, not silently dropped.

**Convention.** Citations are `path:line` against the tree at `4ece31e`. "Could not
determine" is written where that is the honest answer.

---

## Tier 0 — verified on this machine, 2026-08-10

Both binaries built release on organon-one (cold tree, 7m07s total; warnings only).
`organon-shell --help` prints the flag surface without starting the event loop. The console
was launched under `ORGANON_SHELL_BACKDROP=1`, `ORGANON_SHELL_SCRIM=96`,
`ORGANON_SHELL_TABS=pi-wsl,shell-wsl,shell`, `ORGANON_SHELL_PTY_DEBUG=1`, and — load-bearing,
see below — `ORGANON_IPC_NS=organon-t0`. Verified with eyes and screenshots:

- **Tabs + harnesses.** Three tabs opened; **Pi v0.80.7 ran its full TUI** in the `pi-wsl`
  tab (context loaded, colored banner, update notice). Tab switching via Ctrl+2 worked.
- **The backdrop renders behind the glyphs and responds live.** The world is painted under
  the grid with the scrim at its floor (96). `organon status` read the snapshot from outside
  the process; `organon recipe lattice` (18 ops over the `cli.txt` override lane) visibly
  transformed the backdrop within a second — environment brightness and gradient changed
  under the running terminal. The self-steering loop holds on this build.
- **htop is correct, including the bottom row.** Full alternate screen, 32 CPU meters,
  colors, box glyphs, and the **F1–F10 bar exactly on the last grid row**. A live window
  resize reflowed it (`[grid] 130x42 → 105x34`, SIGWINCH through ConPTY→WSL). The PTY byte
  trace under `ORGANON_SHELL_PTY_DEBUG=1` was healthy throughout — the DSR-CPR reply fix is
  doing its job on Windows.

**Things that did not work, or bit:**

1. **The native `claude` harness tab failed** — `'claude' is not recognized`: the Claude
   Code CLI is not on this machine's Windows PATH, and not installed inside WSL either
   (`pi` is, at `/home/linuxbrew/.linuxbrew/bin/pi`). Tier 0's "Pi or Claude Code tab" was
   satisfied with Pi. If the demo wants a Claude Code tab, install the CLI first.
2. **Two instances, one namespace.** The first launch ran beside James's already-open console
   and both wrote `Shared` in the default `organon-shell` namespace for ~2 minutes — two
   seqlock writers on one mmap. No observed harm (both publish identical default bytes), but
   it is exactly the collision `edition.rs`'s namespace test exists to prevent *between*
   products. **Rule for the spike: any second console instance forks `ORGANON_IPC_NS`.** The
   re-launch used `organon-t0` and coexisted cleanly with the live default-namespace instance
   — the invariant witnessed working, not just asserted.
3. **The default look reads as a near-uniform dark wash at Tier 0.** With transport stopped
   (`beat 0.00`) and the stock camera, the cube field contributes almost nothing visually;
   both the fresh build and James's instance showed the same murk until a recipe was applied.
   The *mechanism* is proven (pixels change on command); the *default opening frame* is not
   demo-grade. The demo script's opening beat should apply a recipe (e.g. `lattice`) or start
   the transport before anyone looks.
4. **Window spawn position landed partly off-screen** on this two-monitor mixed-DPI layout
   (bottom edge below the display; the monitor runs ~225% scale). Cosmetic, pre-existing,
   not spike work — but screenshots and window automation must account for DPI
   virtualization (all coordinates from `GetWindowRect` are logical, the real surface is
   ~2.25× larger).
5. **`organon generator dna` alone did not visibly change the murk** (its `queued: gen 2`
   drained fine — the lattice recipe immediately after proved the lane). A bare generator
   switch at default lighting/camera does not necessarily *read*. Beat scripts should use
   recipes, which set the whole look.

---

## R1 — The compositing seam

**Answer.** The `World` is rendered once per frame in `Shell::render_backdrop(&mut self) ->
Option<egui::TextureId>` (`native/src/shell_main.rs:344`), called from `redraw` at `:431`
**before** `egui_ctx.run` (`:459`): gate on `backdrop_on`; size to the swapchain; recreate
texture+views on size change; `world.render_to_texture(&pane.texture, pane.size,
BACKDROP_FORMAT)` (`:375`); register-or-rebind the egui id. It is painted in
`term_view::draw(ui, session, backdrop)` (`term_view.rs:127`, called at `shell_main.rs:485`)
as an **ordinary egui textured quad** — `painter.image(texture, rect, UV 0..1, WHITE)`
(`term_view.rs:200`); no `PaintCallback` anywhere. Z-order is emission order: image → scrim
→ per-run bg rects → glyphs → cursor.

**The measured gamma pair, concretely** (`shell_main.rs:41-46, 354-369`): one texture, two
views. `BACKDROP_FORMAT = Rgba8UnormSrgb` — the World renders through the default (sRGB)
view, so the composite writes linear and the hardware encodes once. `BACKDROP_SAMPLE_FORMAT
= Rgba8Unorm` — a second, explicitly-formatted view in `view_formats` that egui samples, so
egui's shader linearizes exactly once. The measurement itself lives at
`wgpu_editor.rs:472-477` (single-view version washed sky `0.431 0.436 0.336` down to
`0.238 0.219 0.120`, silently). "Sample-linear" means a linear-*format* view, not linear
values.

**Size/resize:** texture sized to `(gpu.config.width, gpu.config.height)` — the full
swapchain in physical pixels (`:349`), recreated on change (`:351`); `World::on_resized` is
a documented no-op (`world.rs:9581-9589`). **Same-id rebinds:** first frame
`register_native_texture` (`:394`), thereafter `update_egui_texture_from_wgpu_texture`
(`:383-388`) against the carried `Backdrop.id` (`:54`, kept across recreation at `:370`) —
`ui_layer::register_scene_texture`'s discipline (`ui_layer.rs:154-193`) open-coded; Shell
has no `UiLayer`. **Scrim:** `term_view.rs:210-219`, a pure function of
`backdrop.is_some()`: env alpha clamped `.max(SCRIM_FLOOR=96)`, default 185, painted over
the whole grid rect. **Features:** the bin *requests* full engine features but intersects
with `adapter.features()` (`shell_main.rs:132-173`) — RT and timestamps are probed, never
guaranteed. **Submission order** is sufficient: the World submits internally
(`organon-render/src/render.rs:5792`), the egui pass submits later on the same queue
(`shell_main.rs:515-548`).

**What a second background source must satisfy** — the seam's currency is
`Option<egui::TextureId>`; nothing downstream knows what a `World` is:

1. Fill a single-sample 2-D `Rgba8UnormSrgb` texture (`RENDER_ATTACHMENT |
   TEXTURE_BINDING`, `view_formats: [Rgba8Unorm]`) at exactly the claimed size, writing
   **linear** color.
2. Borrow device/queue as arguments (`world.device()` / `world.queue()`,
   `world.rs:10238-10255`) — Shell owns no device.
3. Submit before `render_backdrop` returns (or hand back an encoder to submit ahead of
   egui's).
4. Preserve the egui `TextureId` across recreation exactly as `:380-398` does.
5. Keep `BACKDROP_FORMAT` if it ever alternates with the World frame-by-frame —
   `Gfx::out_format` rebuilds composite/FX/temporal pipelines on any format change
   (`world.rs:6049-6053`).
6. Assume nothing beyond `Limits::default()` + raised `max_bind_groups` + whatever survived
   the feature intersection.

Minimal drop-in: `fn render(&mut self, device, queue, texture: &wgpu::Texture, size)`
into the **existing** `Backdrop.texture` — then `term_view.rs`, the rebind logic and the
scrim need no change at all. The only per-frame source-choice point is `render_backdrop`'s
body (`:344-375`), driven by a new field on `Shell`; today `backdrop_on` is read once at
startup (`:113`).

**Consequences for the plan** — see *Corrections* #1–#4: the seam is confirmed and the
Tier 1 split holds, but (a) **the backdrop is already painted with a wrong aspect** —
window-sized texture, UV 0..1, into a CentralPanel 30 logical points shorter (the top
strip), invisible on a generative world and glaring on a flat plane; fix at the seam by
sizing the texture to the panel rect (precedent: `wgpu_editor::render_scene_pane`,
`wgpu_editor.rs:565-625`); (b) the scrim clamp has **no test** (only the help-text test at
`shell_main.rs:713`), and making "assert `SCRIM_FLOOR` holds" mechanical means a
`term_view.rs` edit that needs an owner; (c) **if the substrate replaces the World rather
than sitting beside it, `organon set/generator/recipe` stops changing the backdrop** — the
override lane drains inside `World::frame_body` (`world.rs:2215-2223`). Keep the World
selectable as a second source; the seam is source-agnostic. (d) Nothing in the tree tests
this seam; the leaves' headless tests are the only mechanical guard.

---

## R2 — The camera

**Answer.** The projection is assembled in exactly one place: `build_uniforms()`
(`world.rs:10551-10598`, sole call site `:6749`) — `Mat4::perspective_rh(fov, aspect,
CAM_NEAR, CAM_FAR)` with `CAM_NEAR=0.1` / `CAM_FAR=5000.0` baked (`:10521-10522`). No
`Camera` type; `organon-render` computes no projection, it receives finished matrices.
Inputs: `fov_deg` from `Shared.cam_frame[1]` (live IPC data, default 45); `yaw/pitch/
distance` private fields (init 0.7/0.45/520.0) mutable **only by relative deltas**
(`apply_camera_input`, `:10199-10221` — the entire public camera API); `cam_center`
**auto-lerps 5%/frame toward the generator field's AABB centre** (`:5270`) and is not
settable; aspect from the render target each frame.

**Near-ortho needs no orthographic matrix** — issue #3's rule (long lens, far back) is
still perspective, so DoF depth remap, SSAO/SSR/SSGI/VXGI position reconstruction, TAA
reprojection and every `inv_vp` path keep working. A true `orthographic_rh` would break
them. **Stay on narrow-FOV perspective, and say so.**

**The smallest honest change is three edits, all in `world.rs`:** (1) widen the FOV clamp
floor 10°→~4° in **both** places — `:6489-6490` and `:10597`; moving one is a silent no-op;
(2) add an absolute rig setter beside `apply_camera_input` plus a latch that suppresses the
`:5270` auto-follow; (3) let near/far follow the rig (at 10° FOV the framing distance is
~2460 for what 45°/520 frames; `near=0.1` at that distance wastes depth precision —
`Depth32Float`, non-reversed, `render.rs:318`; arithmetic estimate, not measured). A
zero-edit hack exists (Shell writes `Shared` wholesale each frame, so
`shared.cam_frame[1]=10.0` works today) but framing off ratcheted `Zoom` deltas is not a
rig.

**Three hardcoded-FOV sites a narrow lens upsets** (all `world.rs`, none in the render
crate): sun-disc NDC radius against 22.5° half-FOV (`:10495-10496`); RT photon
`pixel_scale` (`:8488`); the scenery pass rebuilding its own 45° projection off-rails
(`:7981`). Culling is a non-issue: `cull_mode: None` everywhere, no frustum culling or LOD
in the render crate.

**Integration point, exactly:** the tuple `(cam_center, yaw, pitch, distance, cam_roll,
fov_deg)` finalized at `world.rs:6480-6524` — the rails branch (`:6497-6511`) is the
precedent that overrides all six; a substrate rig is a **third arm on that same `if`**,
gated on a `World` flag set by a new public setter that `shell_main.rs` calls. It must land
there and not later: TAA post-multiplies `view_proj` at `:7924-7942`, so anything injected
downstream fights the jitter.

**Consequences** — *Corrections* #1, #5: Leaf A stays pure arithmetic (eye/target/FOV +
deviation bound), but the plan's "~5–15° FOV" names no axis and the engine takes
**vertical** — the corner-ray deviation is essentially the diagonal half-FOV (≈10.1° at 10°
vertical / 16:9), so **the documented bound must be a function of aspect**, and the test
takes aspect as an input. The FOV floor is 10° today, clamped twice. And "pointed at a
plane" is the real gap, not FOV — the auto-follow at `:5270` must be either accepted (a
plane centred at origin makes it benign — a coincidence to name, not lean on) or latched
off.

---

## R3 — The command surface

**Answer, part 1 — `command.rs::register_spec` is seeded by nobody.** `CommandService` is
constructed only in its own unit tests; `organon-shell/src/lib.rs:29` is the sole reference
to the module outside itself; `shell_main.rs` never mentions it. The catalog is data
(`Vec<CommandSpec>`, name-sorted; `register_spec` replaces on collision — idempotent,
`command.rs:252-257`). `CommandSpec = {name, doc, target: TargetKind, args: Vec<ArgSpec>}`;
args are **named**, validated from a JSON object (`validate_args`, `:419-472`; `ArgKind` =
`Float{min,max} | Int | Bool | Text | Choice`); undeclared args pass through deliberately
(`:86-95`). Dispatch = validate → execute → record, and **every** exit funnels through the
single `log_run` (`:357-379`) — the every-dispatch-leaves-a-record invariant is structural,
pinned by `every_dispatch_leaves_a_record` (`:713-738`).

**Part 2 — `core_catalog` is mechanical but thin.** 44 entries concatenated from nine
`param_block!` slot lists (`agent.rs:181-193`; macro arm `param_table.rs:78-94`). An entry
is `CatSlot{id, kind}` — **no gloss, no range, no default, no unit**. Those live in:
`id_range(id)` (`agent.rs:533-579`), `current(&Shared, id)` (`:582-631`), `param_desc(id)`
(`:304+`), `ACTUATABLE_IDS` (45 ids, `:832-845`; four sit outside the curated blocks —
`scale_amp`, `mat_hue`, `bell_physical`, `tempo` — which is why `cli::catalog_entries()`
unions the two, `cli.rs:250-261`). Internal consistency is pinned by two tests
(`agent.rs:2727-2755`); **consistency with `params.rs` is pinned by nothing** — see R6.

**Part 3 — the `organon` CLI.** clap derive (`ctl.rs:16, 25-40`), `ACTUATABLE_IDS` become
a `PossibleValuesParser` (`:21-23`). Subcommands (`:43-191`): `status catalog describe
recipes recipe get watch set do release generator|gen surface|surf material|mat snap
record completions docs`. Logic lives in `cli.rs` (pure, tested); `ctl.rs` maps clap →
`CtlCmd` (`to_ctl`, `:206-232`) and owns I/O and exit codes (0 / 2 usage / 3 no live
Organon). **Three transports:** the fire-and-forget override lane (`ops_for` → `CliOp`
lines → append to `ns_file("cli.txt")`, drained by the **World** each frame by file-length
growth, `world.rs:9694-9733`); the eyes request/reply sidecar for `snap`/`record`
(`ctl.rs:339-370, 387-413` — bypasses `to_ctl`; **the Shell does not answer it**, so those
two hang/timeout in-console today); and direct `Shared` reads.

**Where `console` attaches:** five files in order — `ctl.rs:43` (a `Console` variant + a
`ConsoleAction` enum beside `RecordAction`), `to_ctl` (`:206`), `cli.rs:22` (`CtlCmd`),
`cli.rs:119` (`ops_for`), `agent.rs:767-828` (`CliOp`) — *if* routed over the existing
lane. See *Corrections* #7 for why Tier 2 should instead use a new `ns_file("console.txt")`
sidecar drained in `shell_main.rs`: **there is no transport from the CLI to Shell state
today** — `cli.txt` is drained by the World, and a background swap is `Shell` state, not
`World` state.

**Where `--discover`/`--describe --at` attach:** the `Cli` struct (`ctl.rs:37-40`). `cmd`
is **required** today, so `organon --discover` is a clap usage error — it must become
`Option<Cmd>` with top-level `#[arg(long, global)]` flags, handled **before** `to_ctl`
beside the existing early exits (`:375-413`). Naming collision: `organon describe <query>`
already exists and prints prose (`ctl.rs:63-66`); the schema's `--describe` prints JSON.
They can coexist; decide wording deliberately.

**The `is_live()` story — SHELL_ARCHITECTURE.md §3 had the wrong channel, and the recon
initially had the wrong writer.** `is_live` (`organon-core/src/ipc.rs:3446-3460`) probes
the **`Shared` mmap's `seq` counter for motion** (up to ~150 ms) — it never reads the
`Feedback` mmap (`:3483-3544`), so the ledger's "silence it by writing Feedback" remedy
could never work (corrected in this change). The recon agent then concluded nothing writes
`Shared` in the Shell namespace — **wrong**: `shell_main.rs:425-427` publishes it every
redraw (its comment says exactly why), James's in-console `organon status` works, and Tier
0 re-verified both. Reconciled: liveness in-console depends on the *redraw cadence* — a
continuously-repainting console (backdrop on) reads live; the warning can fire when redraws
stall. During Tier 0 with the backdrop on, no spurious warning appeared. One real cost
stands either way: **every read-path invocation pays the probe before printing**
(`ctl.rs:452` runs it unconditionally), so `--discover` must skip it — the strip must never
block on ~150 ms of liveness theatre.

---

## R4 — Grid geometry and PTY sizing

**Answer: there is no row arithmetic anywhere in the tree.** The whole mechanism is one
`floor()` fed by an egui rect that Context-level panels have already shrunk:

1. Cell metrics: `FontId::monospace(14.0)` hard-coded in `term_view::draw`
   (`term_view.rs:128-130`); `cell_w = glyph_width('M')`, `cell_h = row_height`, in points.
2. Grid rect: `ui.available_rect_before_wrap()` (`:132`) — the CentralPanel's rect, no
   margins, no `allocate_*`.
3. `cols/rows = floor(rect/cell).max(2) as u16` (`:133-134`) — **the one points→rows
   conversion in the product**.
4. `session.resize(cols, rows)` (`:153`) — **the single site that decides what the child
   believes**; `TermSession::resize` (`term.rs:318-326`) guards no-op/zero, resizes the PTY
   (`PtySize{pixel_width: 0, pixel_height: 0}`) then the VT (`Term::resize` resizes **both**
   grids — alt screen included, one path, no special case).
5. Painting culls `vrow >= rows` (`:270-271`) and the cursor uses the same bound (`:309-313`).

**The pattern Tier 3 copies is already on screen:** `shell_main.rs:473-491` declares
`TopBottomPanel::top("tab-strip").exact_height(30.0)` *then* `CentralPanel` — egui
subtracts panels from `ctx.available_rect()` in declaration order, so the grid is already
window-minus-30pt and nothing but `term_view.rs:133-134` ever knew. **A
`TopBottomPanel::bottom` declared before the CentralPanel reserves its rows for free, in
`term_view.rs` and `term.rs` alike — under this approach neither needs an arithmetic
change.** (In-repo precedent: `app.rs:172-178`; the repo's own statement of the rule:
`lib.rs:2196-2201`.)

**Verified live in Tier 0:** `[grid] 130x42 cell=8.43x16.31 rect=1100x690` →
`105x34 … rect=888x555` on a real resize, with htop's F-bar on the final row both times.

**The integrator's real checklist** (full detail in the R4 report; the load-bearing rows):

- Declare the bottom panel before the CentralPanel (`shell_main.rs:459-492`); height =
  `strip_rows × cell_h` from the pure module — **never a magic constant**.
- **`cell_h` must escape `term_view::draw`** — the panel is sized before the CentralPanel
  exists, so metrics need a public `term_view::cell_metrics(...)` (or `FONT_ID` const).
  *This is the one structural change the tier forces, and no plan document mentioned it.*
- Auto-hide = don't `.show()` the panel; rows grow back and `:153` fires. But **auto-hide
  is not artifact-free**: every toggle is a real `Term::resize`, and alacritty's
  `grow_lines` decrements `display_offset` (`grid/resize.rs:66` in alacritty_terminal
  0.26), so the view jumps a row if toggled while scrolled into history. Recommend:
  suppress auto-hide while `display_offset != 0`.
- The strip sits **outside** the grid rect: the scrim doesn't cover it (give the panel its
  own fill), the global wheel handler (`:191-194`) will scroll the terminal from over the
  strip, and the keyboard drain (`:158-190`) ships every keystroke to the PTY with no focus
  check — the strip's interactivity sits on top of both.
- The `[grid]` debug line must keep reporting the **PTY's** rows or the canary goes blind.
- Leaf C owns both directions — `grid_rows(avail_h, cell_h, strip_rows) -> u16`
  (saturating, floor ≥ 2) *and* `strip_height(strip_rows, cell_h) -> f32` — or points and
  rows are computed twice and diverge at fractional DPI. `u16` underflow wraps in release.
  Precedent for the shape: `mind_shell.rs:100-150` (`layout_workstation`).
- If the strip is ever drawn **inside** the grid rect instead (the literal "patch on the
  bottom row"), exactly three sites change together — `:153` (PTY rows), `:271` (cull),
  `:310` (cursor) — and **only `:153` produces the one-line-off symptom**; the other two
  fail invisibly. The failure is asymmetric; that is why it looks like a rendering bug.

**Also found:** no mouse hit-testing or click-to-cell mapping exists anywhere (`term_view`
senses nothing); initial PTY size is hard-coded `80,24` (`shell_main.rs:305`) and corrected
on the first frame; `GridSize`'s `Dimensions` impl lies about `total_lines`/`history_size`
(inert today — alacritty reads only cols/screen_lines; do not consult it for history);
`tabs.rs:5` says the strip is "along the bottom" while it is at the top, and the `+` menu
anchors upward (`tabs.rs:173-176`) — Leaf D fixes both while extracting the widget.
`doc-rules.sh` does **not** trigger on `native/src/shell_main.rs`, so a tier done wholly
there would dodge the SHELL_ARCHITECTURE.md reminder — the discipline is on us, not the
hook.

---

## R5 — Material and surface reuse

**Answer: reuse `RenderPath::Membrane` + `cube.wgsl` + the existing lighting/composite
rig. Write no new shader.** `Surface.mem_pos/mem_norm/mem_col/mem_idx` is an arbitrary
world-space triangle mesh (`render.rs:1443-1490`) drawn through the cube pipeline with one
identity instance and per-vertex color (`render.rs:1912-1918, 3425-3458, 4478-4483`), with
a two-sided-lighting line put there for exactly this geometry (`cube.wgsl:1384-1385`). A
plane through Membrane inherits the full material_type branch set, split-sum IBL, key+fill
lights, shadow map, and the whole HDR→bloom→exposure→tonemap→dither post stack.

**A flat plane is reachable from `Shared` bytes alone, today:** `math::draw_membrane` with
`loop_count_q=0`, `rot_amp=0`, `loop_count=(nx,1,nz)` degenerates to one tessellated flat
quad in the world x–z plane (`math.rs:11833-11836, 11884-11893`; gates `world.rs:2334`,
`:2656-2657`) — which is precisely `mat_uv`'s world-planar-XZ projection. The Shell already
owns and republishes a full `Shared` every frame, so Tier 1's "scene" is substantially a
**params builder, not a renderer feature**.

**The lighting rig, concretely:** `Shared.lighting[8]` = ambient, key, fill, key_elev,
key_azim, glow, opacity, material_type (`ipc.rs:29`, decoded `world.rs:10625-10641`); IBL
via `env.rs` (procedural sky / .hdr / Nishita → irradiance + prefiltered specular + BRDF
LUT) driven by `pbr[3]`/`pbr[4]`/env tint; exposure `2^pbr[2]` (`world.rs:10617`). **The
fill light's direction is derived, not settable** (`world.rs:10637`) — a "rig" is key
elev/azim/intensity, fill intensity, ambient, env intensity/rotation/tint, exposure,
tonemap. Two distinct looks are reachable; "aim the fill" is not.

**The procedural material library already exists** (#472): `material_bake.wgsl` composites
up to 2 layers × 16 pattern kinds into six `Rgba16Float` channel maps
(albedo/normal/rough/metal/AO/height) with triplanar/world-planar UV and height
displacement — all `Shared`-addressable (`ipc.rs:1864-1926`). 🚨 **But it is gated off the
Membrane path:** `render.rs:3640-3654` forces `u.mtl[0]=0.0` for everything but plain
instanced cubes. It is a **uniform-value gate, not a pipeline gate** (group(5) is bound at
the membrane draw sites), and the tree already patches copies of `u` five times for other
layers (liquid/plexus/scenery/water, `render.rs:3667-3705`) — `WaterLayer`
(`render.rs:1617-1652`) is literally "a flat sheet with its own material and uniform copy".
Lifting the gate for the substrate is small and precedented — **and graphite / paper /
slate need it** (they are map-driven). Brushed metal alone is reachable without it
(`MaterialType::Anisotropic`, `Shared.aniso[4]`).

🚨 **The finding that decides the Tier 1 beat:** a perfectly flat plane + uniform material
+ near-zero FOV yields **one constant color across the whole backdrop** — N, V and L are
identical at every fragment, so Fresnel, the specular lobe and env variation all collapse.
Tier 1 must ship at least one of: **(1) a real (narrow) FOV** — which makes Leaf A's
view-vector deviation bound *the shading gradient itself*, the load-bearing deliverable,
not a nicety; (2) normal variation (blocked behind the `mtl[0]` gate, or geometric relief);
(3) non-uniform albedo (`mem_col` is per-vertex and free; `apply_hsv` with saturation 0
gives neutral slate). **Recommendation: (1) + (3) for Tier 1; (2) lands with the gate lift
in Tier 2.**

**Why not a bespoke pass:** code volume is not the argument (`chamber.wgsl:257-306` is a
complete ~180-line PBR+IBL shade; `chamber.rs:40-67`'s `ImpostorFrame` is already the
"one plane, one material, one rig" uniform) — the argument is downstream: a bespoke pass
into the `Rgba8UnormSrgb` backdrop must re-implement exposure/tonemap/bloom/dither or ship
a raw-linear image that looks flat next to a stock terminal, exactly the Tier 1 beat check;
and `mod env` is **private** inside `render` (`render.rs:12-13`), so even the IBL needs a
`render.rs` change to reach.

**A lone plane gets nothing from SSAO/SSR/SSGI/VXGI/RT** (no occluders) — spend no Tier 1
budget there. **Could not determine** (needs eyes/tracing, recorded honestly): whether the
result *looks* good; how the bounds-driven auto-framing behaves on a large thin sheet; and
which `Shared` toggles quiesce the auto-orbit and the "Breath" scene-scale clocks —
`rot_amp=0` stills the geometry, those two clocks are separate. Tier 0's murk observation
(above) is adjacent evidence: the default look's motion clocks idle at beat 0.

---

## R6 — The parameter model, and what a descriptor can honestly say

**The premise correction first:** `param_table.rs` declares **no ranges at all** — it is
the slot-packing macro table. Ranges live in **`native/src/params.rs`** (all 1372 host
params: 944 Float + 145 Int + 150 Bool + 133 Enum). The schema doc's 📌 note pointed at the
wrong file. (`organon-core/src/params.rs` owns only `FuncName`/`ParamValues` and is
nih-plug-free on purpose.)

**Taper: the engine is 100% linear.** `FloatParam::new` is called exactly once in the
tree — inside the private helper `flin()` (`params.rs:7517-7520`), which hard-codes
`FloatRange::Linear`; every `IntParam` is `IntRange::Linear`; `Skewed` /
`SymmetricalSkewed` / `Reversed` / `with_step_size` / `with_unit` are **unused anywhere**.
The one logarithmic *control* is modelled as two linear params (`inc_scale` ×
`10^speed_exp`, `params.rs:8547-8548`) whose product is a computed slot with no id of its
own — describing the two linear params separately is honest and correct. **The schema's
taper set needs no widening.** `log`/`skewed{factor}` stay as headroom for foreign emitters,
and the round-trip test is what catches a future skewed range.

**What the schema actually could not say — two real gaps, neither taper (schema amended in
this change):**

1. **Display formatting.** Every float goes through `v2s_va()` (`params.rs:7529-7553`):
   *magnitude-dependent* decimals (0 at |v|≥1000, 1 at ≥100, 2 at ≥10, else 3), trailing
   zeros trimmed, `-0`→`0`. `format:{decimals:N}` cannot express it → the schema gains
   `format.style: "fixed" | "magnitude"`.
2. **`unit` has no machine-readable source.** `Param::unit()` is `""` for all 1372; units
   exist only as prose in names ("Exposure (EV)") and glosses. The honest emitter output is
   `null`; I2 forbids inventing one at the emitter → schema says so now.

🚨 **The real I2 hazard: three hand-maintained range tables, already disagreeing.**
`agent.rs::id_range` (`:533-579`) and `clip.rs::RANGES` (`:17-31`) both claim to mirror
`params.rs`; no test pins either. **9 of 45 actuatable ids are wrong today** —
coordinator-verified on the flagship cases:

| id | `id_range` says | `params.rs` truth | damage |
|---|---|---|---|
| `trans_amp_x/y/z` | (0, 200) `agent.rs:539` | 0..20 `params.rs:7597-7599` | **10× on max**; `clip.rs:21` identical |
| `exposure` | (−8, 8) `:550` | −8..4 `:8724` | max 2× |
| `bloom_intensity` | (0, 2) `:553` | 0..1 `:8727` | max 2× |
| `sss_power` | (0, 8) `:558` | 1..16 `:8733` | both ends |
| `irid_scale` | (0, 8) `:559` | 0.1..6 `:8735` | both ends |
| `cam_damping` | (0, 1) `:563` | 0.01..0.99 `:8610` | both ends |
| `cam_path` | (0, 11) `:568` | 11 variants, 0..=10 (`:113-137`) | admits a 12th that doesn't exist |

The published `doc/reference/parameters.md` already ships the wrong `trans_amp` range, and
`recipe.rs:214-231` validates recipes against the wrong bounds. This is exactly the
"approximately right, survives review" failure I2 names — **fix it as its own change before
Tier 3** (see *Pre-Tier-3 work item*). (`clip.rs:23` also shows `inc_scale (0, 0.1)` vs
params 0..1 — possibly a deliberate CC-range compression; not counted, verify when fixing.)

**Current value: readable from any process, in the display domain**, two ways —
`ipc::Reader` over `Shared` (plain human-unit `f32`s; packers write `p.field.value()`,
never normalized — `param_table.rs:98`, confirmed by `clip.rs:92-99` normalizing *from*
them) via `agent::current`; and `OrganicMathParams::default()` (`params.rs:3982-3983`),
which constructs all 1372 real params **with no host, no audio thread, no GPU** — what
makes the round-trip test headless.

**Descriptor field → source (the emitter joins three sources; one is unreliable):**

| field | source | note |
|---|---|---|
| `id` | `core_catalog()` ∪ `ACTUATABLE_IDS` (= `cli::catalog_entries()`) | ~48 ids, 45 actuatable |
| `label` | the param object's `.name()` | not in the catalog |
| `help` | `param_desc(id)` | compile-guarded, reliable |
| `kind` | the slot lists' `SlotKind` | ⚠️ **not** `catalog_entries()` — it hard-codes `"num"` for the four union ids (`cli.rs:257`); `bell_physical` is a Bool |
| `value` | `agent::current(&Shared, id)` | display domain, live |
| `default` | `Param::default_plain_value()` | no catalog source |
| `range`+`taper` | **the param object — never `id_range`** | `preview_plain(0.0)` / `preview_plain(1.0)`; taper `"linear"` |
| `unit` | none — emit `null` | honest |
| `format` | `v2s_va`'s rule (floats) / `fixed:0` (ints) | schema amendment A |
| `variants[]` | `<E as Enum>::variants()` | label = `#[name]`; id = lowercased label (`cli.rs:81-101, 292-303`) |

**The join has a missing bridge.** Two disjoint id namespaces exist — Rust field names
(`trans_amp_x`; produced by `param_block!`'s `stringify!`, spoken by the whole
catalog/CLI/docs stack) and nih-plug wire ids (`"tax"`; produced by `#[id]`, returned by
`param_map()`, spoken by presets/hosts/MIDI) — **and no table joins them**. The Tier 3
adapter must build that bridge **generated from the `param_table.rs` slot lists** (extend
the `@cat` arm to carry the `#[id]`, or generate field accessors) — a hand-written table
would be the fourth copy of the thing that already drifted. One precedence rule needs
documenting: `cam_path` is `SlotKind::Enum` *and* has an `id_range` (`param_table.rs:
204-205` + `agent.rs:568`) — Enum wins (render as `Choice`); and `palette`'s / `cam_path`'s
variant lists must come from `params.rs` enums directly (the CLI exposes variant lists only
for generator/surface/material, `cli.rs:301-303`).

**The `taper_round_trips_against_the_engine_range` test, concretely:** iterate
`OrganicMathParams::default().param_map()`; for each `ParamPtr::FloatParam(fp)` deref
(sanctioned by `param_map`'s validity contract — the plain-domain methods are **not**
forwarded by `ParamPtr`) and assert, for a normalized sweep: `descriptor.min ==
fp.preview_plain(0.0)`, `max == preview_plain(1.0)`, `default == default_plain_value()`,
linear law `min + n·(max−min) ≈ preview_plain(n)` within ~1e-5 relative; same over
`IntParam`; Bool/Enum carry no range. Add the format law to the same walk: `format(v) ==
normalized_value_to_string(preview_normalized(v), false)`. It lives in the root crate
beside `generated_reference_is_current` (`cli.rs:1023`). **Written today it fails on 9
ids — the correct outcome, and why it gets written first.**

**Coverage arithmetic for `--discover`:** "~1,370 parameters" is right as a count of host
params (1372), but only **45 are actuatable / ~48 catalogued** — a strip's coverage line
reads `of 45` (or whatever the emitter's node exposes), not `of 1370`. The other 1327 have
no gloss, no CLI id and no actuation route.

---

## Contradictions and surprises

1. **R3 vs R1/R2 on who writes `Shared` in the Shell** — R3 claimed nobody does and built
   two conclusions on it (descriptor `value: null` in-console; the warning's cause).
   **Resolved against R3** by code (`shell_main.rs:425-427`), by James's live `organon
   status`, and by Tier 0 (namespaced `status` + a recipe landing). What survives from R3:
   `is_live()` probes `Shared` seq motion and never reads `Feedback`, so
   SHELL_ARCHITECTURE.md §3's "write Feedback" remedy was wrong — corrected in this change.
   Descriptor `value` in-console is therefore **available**, not null, whenever the console
   is redrawing.
2. **R1 and R4 found the same latent defect from two directions:** the backdrop texture is
   window-sized but painted UV 0..1 into a CentralPanel already 30pt shorter — vertically
   compressed today, worse with a bottom strip, glaring under a flat plane. One fix, at the
   seam, in Tier 1 (size the texture to the panel rect; `render_scene_pane` precedent).
3. **The plan's protected-file list was too short.** `world.rs` (R2, R3, R5), `render.rs`
   (R5), and the CLI seam files `ctl.rs`/`cli.rs`/`agent.rs` (R3, R6) are all merge
   surfaces tiers will contend for. §6 amended; per-tier ownership declared in §5.
4. **Tier 2's output line conflated two registries, one of which is constructed nowhere.**
   clap gives `--help`; `CommandService` gives specs — and no `CommandService` exists in
   the product. Tier 2 must stand one up (owned by `shell_main.rs`) or its "registered
   spec" is dead code that looks green.
5. **`snap`/`record` cannot work in-console** (the eyes sidecar has no reply side in
   Shell) — known seam (SHELL_ARCHITECTURE §2), now stated where tier planners will read
   it, so nobody demos it by accident.
6. **The default look is not demo-grade at rest** (Tier 0 §3 above): the opening beat needs
   a recipe applied or the transport running. A mechanism being proven and a frame being
   showable are different claims; the demo script should encode the difference.
7. **The murk also hid a lane subtlety:** a bare `generator` switch may not visibly change
   anything at default lighting/camera. Beat scripts use recipes.

---

## Corrections to the execution plan

*(All applied to `console_spike_execution_plan.md` in this change.)*

1. **Tier 1 Leaf B re-scoped** — "substrate scene + shader" → "substrate `Shared`-state
   builder (pure) drawn via `RenderPath::Membrane`; no new shader; per-vertex albedo +
   narrow FOV carry the read" (R5).
2. **Tier 1 ownership** — integrator additionally owns `world.rs` (camera arm + FOV clamps
   ×2 + auto-follow latch; R2) and `term_view.rs` (extract a testable `scrim_alpha`; R1);
   `doc/arch/render.md` joins `SHELL_ARCHITECTURE.md` in the integrator's doc duty when
   `world.rs`/`render.rs`/shaders move (hook `doc-rules.sh:29,31`).
3. **Tier 1 keeps the World selectable** as a backdrop source beside the substrate —
   otherwise `organon set/generator/recipe` visibly dies (R1). The source switch is a field
   on `Shell`, decided per frame in `render_backdrop`.
4. **Leaf A's deviation bound is a function of aspect** (vertical FOV is what the engine
   takes), and its test takes aspect as an input (R2).
5. **Tier 2 transport decided:** a new `ns_file("console.txt")` sidecar drained in
   `shell_main.rs` — not a `CliOp` through `world.rs` (R3). Tier 2 also stands up the
   product's first `CommandService` instance, owned by the integrator.
6. **Tier 3 Leaf B relocated and split:** it cannot live in `organon-shell` (nih-plug
   firewall; `Cargo.toml` header + `cargo tree` acceptance) — it is a new **root-crate**
   module called from `shell_main.rs`, budgeted as two pieces: the field-name↔wire-id
   **namespace bridge** (generated from the slot lists), then the adapter. Its `pub mod`
   line in `lib.rs` is the integrator's, added up front (R6).
7. **Tier 3 integrator brief corrected:** the strip is a bottom `TopBottomPanel` declared
   before the CentralPanel — `term_view.rs` and `term.rs` need **no arithmetic change**;
   the centre of gravity is `shell_main.rs`; the one forced structural change is `cell_h`
   escaping `term_view::draw` (assigned to the integrator); Leaf C owns rows↔points **both
   directions** plus the empty-payload→0-rows rule; auto-hide is suppressed while scrolled
   into history (R4).
8. **Leaf C additionally owns click→cell mapping** (nothing exists; the strip needs taps)
   — or the strip uses egui widgets in the panel and the mapping is dropped explicitly.
   Integrator decides at dispatch; default is egui widgets, mapping dropped.
9. **§6 protected list extended** with `world.rs` and `organon-render/src/render.rs`;
   `ctl.rs`/`cli.rs`/`agent.rs` get one-writer-per-tier declarations in §5.
10. **A pre-Tier-3 work item added** (below).
11. **Tier 0 §2 gains the namespace rule:** a second console instance always forks
    `ORGANON_IPC_NS`.

### Pre-Tier-3 work item — make the range tables true

Its own small change, landed before Tier 3 starts, conflict-free with Tiers 1–2:

- Write `taper_round_trips_against_the_engine_range` (it fails on 9 ids today — that is
  the point).
- Make `agent.rs::id_range` (and `clip.rs::RANGES`, and `recipe.rs`'s bounds) derive from
  or be pinned to `params.rs`; regenerate `doc/reference/`.
- Then Tier 3's emitter builds on a table that is already true.

---

## Schema doc amendments

Applied to `doc/console_discover_schema.md` in this change, per R6:

- **(A)** `format` gains `style: "fixed" | "magnitude"` (default `fixed`); the
  `"magnitude"` law is `v2s_va`'s, stated verbatim in the doc; round-trip law folded into
  the taper test.
- **(B)** `unit` documented as `null` for every Organon parameter until a unit is declared
  at the parameter; inventing one at the emitter violates I2.
- **(C)** I2 gains the named source of truth: descriptor `min`/`max`/`default` come from
  the parameter object (`preview_plain(0.0/1.0)`, `default_plain_value()`), **never**
  `agent.rs::id_range` — with the 9-of-45 drift of 2026-08-10 recorded as the reason.
- The 📌 verify-at-implementation note replaced with the verified finding: the engine is
  all-linear; `log`/`skewed` stay as foreign-emitter headroom.
