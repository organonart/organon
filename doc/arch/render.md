# Organon — the world & render pipeline

> **What this is.** The depth of the native **render pipeline**: how a frame is built,
> what each pass does, and the seams to extend it through. Split out of the repo-root
> `ARCHITECTURE.md` in organon#590 Tier 3, where it was 905 lines — 40% of a document
> that a SessionStart hook injects into *every* session.
>
> **This file is NOT auto-injected. Open it deliberately** when you are working on the
> renderer — `ARCHITECTURE.md` §9 carries the altitude version and points here, and §19's
> file map names the module you want.
>
> **Keep it current in the same change** as any render-pipeline shift. A Stop hook
> (`.claude/hooks/architecture-doc-check.sh`) reminds you when anything in
> `native/organon-render/src/`, `world.rs`, `rt.rs`, `bin/visual.rs` or an upstream
> `.wgsl` moves without it. As of #626 Tier 4 that rule follows the **crate boundary**
> rather than a file list, so a new render module is covered the day it is added.

> 🧱 **#626 Tier 4 — the renderer is its own crate, `organon-render`.** `render` and its
> 36 surface submodules, plus `axes` and `chamber` and all 50 of their shaders, moved to
> `native/organon-render/src/`. Paths in this document that read `src/render.rs` mean
> `native/organon-render/src/render.rs`; the **module** paths are unchanged, because
> `organic-math-native` re-exports the crate and `world.rs` still writes `render::…`.
>
> 🚚 **UPDATE (organon#49 T4c-ii): `world.rs` HAS moved** — to
> `native/organon-world/src/world.rs`, behind that crate's default-off `world` feature,
> with its nine `#[path]` submodules and the `capture.wgsl` / `overlay.wgsl` /
> `rt_debug.wgsl` shaders. `bin/visual.rs` went with it as
> `native/organon-visual/src/main.rs`, its own package. The paragraph below is #626's
> reasoning, kept because it is still right about *why it could not move then*: what
> unblocked it was organon#49 T1–T4c-i moving everything `world.rs` reached upward for —
> the enums, the substrate, the window leaves, and finally the agent — not a change of
> mind about the coupling.
>
> **`world.rs` did not move [in #626], and the reason is the interesting part.** #626 scoped "the
> render set" as the 17 files in `doc-rules.sh`, `world.rs` among them — and every hard
> coupling it attributed to the renderer was `world.rs`'s. The world owns the agent chat
> client (41 refs), the CLI reply protocol, `egui_platform`, `frame_ring` and
> `scene_input`; `agent` needs `OrganicMathParams` and `preset`. Those are host-side and
> cannot descend. Extracting `world.rs` is **#618's `World` decomposition**, not #626's.
> With it set aside the renderer needed exactly one upstream module —
> `organon-core::math` — and none of the six wire-format enum splits #626 specced.
>
> **One real consequence for this document's subject:** `world.rs` is `#[path]`-included
> by *both* `lib.rs` (behind `mind-edition`) and `bin/visual.rs`, so the renderer used to
> be **compiled twice** in a Mind build. As a crate it compiles once and links into both.
>
> **Scope.** Everything the old §9 covered. Its neighbours stay in `ARCHITECTURE.md`:
> §5 (the two-process model), §8 (the generator system), §10 (the world layer —
> terrain/stars/clouds/ocean), §11 (motion), §14 (the editor UI).

---

## The world & render pipeline (`world.rs`, `bin/visual.rs`, `render.rs`)

The world **owns** the animation clocks, camera, and any generator state; it **reads**
everything else from the IPC `Shared` snapshot.

**Where it lives (#572, the world hoist).** All of that used to *be* `bin/visual.rs`: 13 000
lines of binary, including `render.rs` and its GPU siblings, which a binary's own `#[path]`
modules made unreachable from the library. Route C needs the opposite — Organon Mind's editor
becomes a wgpu surface on the window nih-plug hands it, and that editor lives in `lib.rs`. The
hoist ran in three stages: **(1)** the renderer as a library module tree (#574); **(2)** `App`
→ **`World`** in `src/world.rs`, the state and the frame (#575); **(3)** the window seam — the
caller hands in a target per frame and `World` stops owning a swapchain (#582). **All three are
done.**

The split is: **`world.rs` = the world**, ~12 900 lines, everything that draws or
decides what to draw, plus its `#[path]` module tree (`render`, `capture`, `overlay`, `axes`,
`chamber`, `hdr_macos`, `rt`, `metal_island`, `gpu_timer`, `recorder`, `snap`, `ui_layer`,
`winit_platform`);
**`bin/visual.rs` = the window**, ~625 lines — create it, pick its display, own the surface and swapchain, run winit's event
loop, forward. Two rules make that work and are easy to break:

- **`World` is a library type, so the binary cannot `impl ApplicationHandler` for it** (orphan
  rule). It owns a `VisualApp { world: World }` wrapper instead, and forwards to `attach_gpu`
  (adopts a device the host built), `on_window_event` (the whole old handler body — keys, camera,
  the UI layer's pointer routing; since #621 its two gesture arms **delegate** the maths to
  `apply_camera_input` rather than holding it), `render_into` and `present`. The handler *body* moved with the
  world rather than staying in the binary, which keeps `World`'s public surface small instead of
  twenty `pub` fields. Two things it cannot do come back as an **`EventResponse`**: ending the
  event loop (`ActiveEventLoop` belongs to whoever called `run_app`) and **drawing** (`Redraw` —
  only the host can acquire and present).
- **A host may also draw its *own* late pass over the frame**, which is what `World::device()`
  and **`World::queue()`** (added by #593 Tier 2) exist for. `wgpu_editor.rs` passes
  `ui_window: None` — so the world draws no interface — and then runs its own `egui-wgpu` pass
  into the same swapchain image with `LoadOp::Load` before calling `present`. The winit host
  never needed the queue because its UI pass runs *inside* the frame, via `ui_layer`; both
  hosts still share one device, and the device is still the world's.
- **Stage 3 took the window out entirely.** `Gfx` is device-side only; `WindowSurface` lives in
  `bin/visual.rs`. The host acquires a swapchain image, states its size / format / `presented` /
  EDR headroom / gamut on a **`FrameTarget`**, and applies the **`FrameRequests`** (title, inner
  size) the frame returns. `resolve_output` is gone — with no surface to reconcile against, the
  caller simply says what its texture is (`FrameOutput::of`). What used to be *five window
  couplings inside the frame* is now zero. The one left was `FrameTarget::ui_window`, a
  `&winit::Window` that existed only because `ui_layer` translated input through `egui-winit`.
- **#593 Tier 3 removed that one too, so `world.rs` names no `winit::window::Window`.**
  `ui_layer` is generic over `egui_platform::EguiPlatform`; the frame states
  `ui_scale_factor: Option<f32>` (`None` = draw no interface, exactly as `ui_window: None` did)
  and the *host* builds the `UiLayer` it hands to `attach_gpu`, because only a host knows which
  backend it has — `winit_platform` here, baseview in route C's editor. `WindowGeometry` carries
  the geometry as data since `baseview::Window` answers neither size nor scale.
  `winit::event::WindowEvent` stays on `World::on_window_event`: that is the winit host's entry
  point and holds the visual's keymap, and a baseview host never calls it.
- **#617 Tier 1 gives the world a second destination in the Mind editor.** `render_to_texture`
  into a pane-sized offscreen target, which egui then paints as a widget — so the same `World`,
  the same passes, now serves both a full-window immersive scene and a bounded viewport pane. The
  render path is untouched; what changed is only *which texture* the host hands it and how the
  result gets on screen. ⚠️ **The pane is two formats, not one**: rendered through an
  `Rgba8UnormSrgb` view so the hardware encodes once, sampled through a plain `Rgba8Unorm` view so
  egui — whose shader assumes every texture is already gamma-encoded — does not linearize it a
  second time. Getting that wrong renders, animates, and is simply too dark.
  ⚠️ **The frame borrows the layer, it never moves it out.** `ui_geometry` decides whether an
  interface is laid out this frame; the paint site then does `gfx.ui.as_mut()`. It briefly did
  `(gfx.ui.take(), ui_geometry(..))` as one tuple pattern instead — and a tuple evaluates left to
  right *before* matching, so every offscreen frame took the layer out and then failed to match,
  skipping the line that put it back. One frame-mirror or recorder frame destroyed the interface
  for the life of the process: no HUD, and **U** dead, since its handler reaches for that same
  `Option`. The four `ui_geometry` tests shipped green alongside it because the predicate was
  never the thing that was wrong — the call site was, and it needs a GPU, so nothing covers it.
- **#621 gives the world a second *input* entry point, and only the camera crosses it.**
  `apply_camera_input(CameraInput::{Orbit, Zoom})` is backend-neutral; `on_window_event`'s
  `CursorMoved` / `MouseWheel` arms translate winit's shapes and delegate, so both viewports orbit
  and zoom through one implementation. The keymap stays winit-typed and stays with the visual
  (see `scene_input.rs` for why, and for why `mind_shell::PointerRouter` cannot supply the events
  in a host that draws a `CentralPanel`). Nothing in the render path is aware of any of this — it
  moves the same `yaw`/`pitch`/`distance` the auto-orbit already rides on.
  ⚠️ **The one unit trap: egui reports drags in *points*, the winit arm in *physical pixels*.**
  `scene_input::orbit_pixels` converts, and skipping it would orbit the editor at half rate on
  every Retina display — working, so nobody would look for it.
- **A third `CameraInput` variant, absolute, on the same seam** (Console Spike, the portal's
  camera). `CameraInput::Frame { yaw, pitch, distance }` writes the base orbit directly, `None`
  per axis meaning "leave it". It rides #621's entry point rather than becoming a fourth method
  because it is the *same three fields a drag writes* — it just says where instead of how far,
  which is the only shape a lane with no return path can carry. `cam_path` still orbits around
  the result, exactly as it orbits around a hand-dragged viewpoint, so nothing in the
  finalization changed.
  ⚠️ **Its clamps are now named constants — `scene_input::{PITCH_LIMIT, DISTANCE_MIN,
  DISTANCE_MAX}` — read by the Orbit and Zoom arms, by the finalization's pitch clamp, and by
  Organon Console's command schema.** Four readers, one number: a hand and an agent must not
  disagree about where the instrument ends. `World::new`'s three initial values are likewise
  `scene_input::DEFAULT_*`, which is what makes the console's `camera --reset` provably the
  framing the window opened with.
  ⚠️ Non-finite input is **dropped, not clamped**: `f32::clamp` panics on a NaN bound and
  returns NaN for a NaN input, and a NaN yaw poisons `view_proj` into a black window with no
  error at all.
  📌 **And the read half: `camera_framing() -> (yaw, pitch, distance)`.** All four writers land
  on those three fields and the world *clamps* on the way in, so a host that remembered what it
  last asked for would report a value the camera may never have held — and would be blind to
  every move a hand made. Nothing in the render path reads it; Organon Console publishes it once
  per frame and serves it to an agent (`CONSOLE_ARCHITECTURE.md` §1.3). ⚠️ It is the **base
  orbit**, not the camera the frame is drawn with: `cam_path`'s offset is added downstream and
  an installed substrate rig (below) overrides all six wholesale. That is the right answer for a
  caller computing a delta to write back, and the wrong one for anybody asking what the pixels
  were rendered from.
- **A fourth camera entry point, and it is absolute in a much stronger sense** (Console Spike
  Tier 1, for Organon Console's substrate backdrop). `set_substrate_rig(Option<(center, yaw,
  pitch, distance, roll, fov_deg)>)` installs the whole tuple; the camera finalization then
  selects it as a **third arm** on the same `if` the rails branch already overrode all six
  from, and latches off the `cam_center` auto-follow (the 5 %/frame lerp toward the generator
  field's AABB centre) for as long as it is installed. It has to land *there* and not later:
  TAA post-multiplies `view_proj`, so anything injected downstream fights the jitter.
  ⚠️ **The FOV clamp floor moved 10° → 4° at BOTH sites** — the camera finalization and
  `build_uniforms`' `perspective_rh` call clamp the same number twice, and moving one alone is
  a silent no-op. `CAM_NEAR`/`CAM_FAR` are deliberately unchanged: a 127-unit plane frames at
  ≈408 world units at 10° and ≈1023 at 4°, both comfortably inside 0.1..5000.
- ✅ **`world.rs` is compiled ONCE** since organon#49 T4c-ii — the paragraph below describes
  the arrangement that replaced. It was not redundancy but the *mechanism*: a `#[path]`
  include is not a cargo feature, so including the source was how the visual binary got a
  world the shipping cdylib did not get. Now `world` is a feature of `organon-world` and the
  visual is a package that turns it on, so one compilation serves both. Nothing here needs a
  path that resolves in two crate roots any more.
- ~~**`world.rs` is compiled twice**~~ in a `mind-edition` build: once as `organic_math_native::world`,
  once via the binary's `#[path = "../world.rs"]`. Build time, not correctness, and it keeps one
  source of truth. It also means `render.rs` has two hosts, which is why its sibling references
  are spelled `super::axes` / `super::chamber` (`rt.rs` moved to `super::render` for the same
  reason) — the one form that resolves under `world::` in both. Conversely `crate::math` resolves
  to `pub mod math` in the library and to `bin/visual.rs`'s root `use organic_math_native::math`
  in the binary, so **that import is load-bearing** even though nothing in the binary names it
  (deleting it is eight `unresolved import` errors — measured).

**Gated on `mind-edition`, by measurement.** Ungated, `pub mod world` grows the plugin cdylib
12 749 728 → 13 250 704 bytes (+490 KB) with zero wgpu/naga dynamic symbols either way. Gated,
a default build measures **12 749 528** — unchanged, and `ui_layer`'s egui-wgpu stack still
reaches only the visual binary, never Ableton's process. Organon Mind ships no VST3, so under
the feature the growth costs nothing.

**Launch display (`resumed` + `pick_launch_monitor`).** When 2+ displays are connected the
window is created **borderless-fullscreen on the non-primary display** (the projector in the
live rig) rather than a windowed default — chosen at create time so there's no windowed flash.
A lone display stays windowed. This is zero-config auto-detect (not an env var) so it also works
when the plugin's "Open Visual Window" button spawns the visual as a host GUI child, which does
not inherit the shell environment. `ORGANON_VISUAL_DISPLAY` overrides:
`off`/`none`/`windowed` = force windowed; a 1-based index or `primary` = pin a display; any
other text = first display whose name contains it (case-insensitive). **F** still toggles
fullscreen on the current display; **Esc** exits.

**Don't steal focus on launch (macOS).** `main` builds the winit event loop with
`with_activate_ignoring_other_apps(false)`. winit's default `activateIgnoringOtherApps(true)`
force-activates this freshly-spawned process, which deactivates the host (Ableton) and hides its
floating plugin-editor window; `false` lets the visual order-front on the projector without
taking focus, so the host and its editor stay put. (This aggressive activation of a non-bundled
child also appears to be behind the intermittent "have to click *Open Visual Window* twice"
first-launch flake.) The window becomes key normally on first click.

**…and therefore, the launch watchdog (#588, `launch_macos.rs`).** Coming up unactivated is
also the condition under which AppKit sometimes never delivers
`applicationDidFinishLaunching:` to a bare (LaunchServices-less) executable — and winit
dispatches `Resumed` *only* from that delegate method, so **nothing below ever runs**: no
window, no adapter, no device, no `about_to_wait`. What the user sees is an invisible process
at 100 % CPU with empty stderr and `organon snap` timing out; what it looks like is a crash or
a window on a display they can't see, and it is neither. `main` therefore calls
`launch_macos::arm()` before `run_app`: a run-loop timer that, if `Resumed` hasn't happened
within `GRACE` (0.75 s), calls `applicationDidFinishLaunching:` **on winit's delegate** and
says so on stderr. It is inert on every healthy launch — the notification arrives before the
run loop pumps its first timer, so the first tick sees the flag set and takes itself down
without touching AppKit — and it does **not** activate anything, so the no-focus-stealing
behaviour above is preserved exactly. The alternative (revert to `true`, or activate on a
timer) would trade this bug back for the disappearing-editor one.

**World vs window (`FrameTarget`, #541 S2 T3).** `World::render` used to assume it owned a
window and a swapchain, reaching into `gfx.config` / `gfx.surface` / `gfx.window` directly.
That assumption is now factored out, because #541 T4 needs the same scene drawn into an egui
pane's texture: `WindowSurface` holds the window-only state (surface, config, the winit
handle), `Gfx` holds what a frame needs regardless of destination, and **`FrameTarget`**
selects between them — a presented window or a caller-supplied offscreen texture.
`resolve_output` yields the `FrameOutput` (size + format + whether it is `presented`) that the
rest of the frame reads instead of the swapchain, and `render_into(target)` / `render_to_texture`
are the two entry points. **The standalone window path is unchanged by design** — this tier
ends with the scene renderable offscreen and *no* behaviour difference to the separate window.

> ⚠️ **`render_to_texture` has no caller in a `mind-edition` build, as of #593 Tier 4.** Its one
> consumer was the #554 T1 frame mirror (`pump_mirror`), which is now
> `#[cfg(not(feature = "mind-edition"))]` along with the rest of that subsystem — Organon Mind's
> editor renders straight into its own swapchain image through `render_into`, so it never needs
> a texture of its own. The method deliberately **stays ungated**: it is this seam, the shape
> `render_into` was factored out of, and what any future offscreen consumer (a lens pane, a
> thumbnail, an export) calls. It carries `#[cfg_attr(feature = "mind-edition", allow(dead_code))]`
> — scoped to the one build where it is dead, so the default build still reports it if its last
> real caller ever goes. Gating the mirror itself is `world.rs`'s `Mirror` / `pump_mirror` /
> `MIRROR_*` / `drop_mapped`; see `MIND_ARCHITECTURE.md` §2.5.

The subtle part is HDR, and it is pinned by tests rather than left to inspection. EDR headroom
and the Rec.2020 wide-gamut tag (#119) are negotiated with a **display**, so they are meaningful
only for a presented window and must never be applied to an offscreen texture: `frame_hdr_max`
and `frame_gamut` both gate on `FrameOutput::presented`, which reduces to exactly the old
`hdr_enabled` / `hdr_enabled && hdr_wide` conditions on the window path. Five unit tests fix
this contract (window draws at the swapchain's size/format, offscreen at the caller's, offscreen
needs no window at all, headroom reaches the window but never an offscreen texture, gamut
expansion only against a tagged surface).

#### Where `hdr_max` comes from — the platform seam (organon#658 Tier 4)

Making headroom an *input* is what lets a second platform arrive without the composite noticing.
`bin/visual.rs`'s **`set_hdr_output`** is a compile-time shim over two routes to the same pair of
numbers — `(headroom, wide_gamut_granted)` — and **nothing downstream of it is per-platform**:
`hdr_max` / `hdr_knee` / `hdr_reexpand` / `hdr_vivid` in `composite.wgsl` are byte-for-byte
unchanged.

| | macOS | Windows | elsewhere |
|---|---|---|---|
| File | `hdr_macos.rs` | `hdr_windows.rs` | — |
| How | objc, **behind wgpu's back**: hunt the `CAMetalLayer` on the `NSView`, set `wantsExtendedDynamicRangeContent` + an extended-linear colorspace | **through wgpu 30**: `SurfaceConfiguration::color_space = ExtendedSrgbLinear` (scRGB) on the `Rgba16Float` swapchain | nothing |
| Headroom | `NSScreen`'s `maximum[Potential]ExtendedDynamicRangeColorComponentValue` | `Surface::display_hdr_info().tone_map_headroom()` → DXGI `IDXGIOutput6::GetDesc1` `MaxLuminance` ÷ `DISPLAYCONFIG_SDR_WHITE_LEVEL` | `1.0` (the SDR tone-map, unchanged) |
| Rec.2020 | granted (`extendedLinearITUR_2020`) | **not granted** | n/a |

Three things about that table are load-bearing:

- **Windows needs no raw DXGI and no `windows-sys`.** wgpu 30 exposes the scRGB swapchain colour
  space *and* the display query as first-class API, and `wgpu-hal` shares the DXGI read between
  its DX12 and Vulkan backends — so the "which backend gets ray tracing" question does not fork
  the HDR path. `hdr_windows.rs`'s header documents the investigation in full.
- **`MaxLuminance` alone is not headroom.** It is absolute nits; the composite wants a multiple
  of SDR white, which moves with the Windows HDR brightness slider. The division is the whole
  measurement, and a unit test pins it (a 1000-nit panel at 200-nit SDR white is 5×, not 1000×).
- **Rec.2020 is a genuine gap on Windows, not an omission.** macOS grants it with an
  *extended-linear* Rec.2020 container; DXGI has no such colour space — its only Rec.2020
  swapchain is HDR10/**PQ**, and our composite writes linear. Reaching it means a PQ encode in
  `composite.wgsl`, which is exactly the shared-composite change #658 T4 promised not to make.
  So `bin/visual.rs` reports the *granted* gamut to the frame and `hdr_vivid` stays inert there.

The Mac route is untouched by all of this; unifying it onto the same wgpu API is a separate
change that needs Mac verification (#658 says so explicitly).

### Per-frame data flow (`World::render`)

1. **Read** the `Shared` snapshot from IPC.
2. **Advance the clocks** the visual owns: `gen_phase` (generic animation phase),
   `angle`/`wind_phase` (rotation), the **PLL beat clock** (`advance_beat_clock`), the
   **auto-orbit camera** (`advance_camera`), colour/ripple phases. The beat clock's BPM
   comes from `active_bpm` — a **tempo source** (#307: Host transport PLL-lock / a BPM
   **detected from the audio** by `audio::TempoEstimator` and published in `cam_audio` /
   the manual dial); only Host PLL-locks its phase, Audio + Manual free-run, and Audio
   **holds the last detected BPM** through a breakdown. The **camera shot sequencer**
   (`SeqState`) advances off the **bar clock** (`beat_pos / beats_per_bar`), cycling the
   `SEQ_PATHS` moves on each `bars_per_shot` boundary and crossfading between the outgoing
   and incoming `camera_path_offset` (Glide) or snapping (Cut); the **decoupled dolly**
   (`dolly_factor`) breathes the radius on its own bar period; `cam_beat_momentum` gates
   the per-beat kick (off = smooth, no wiggle with the audio). **Tier 2 (#307):**
   `camera_path_offset` returns a **`CamOffset`** (yaw/pitch/dist + **roll + lateral truck
   + fov_mul**), so the extra moves (Boom/Pendulum/Truck/Push-Pull/Over-the-Top/Drift)
   drive **roll** (dutch — an up-vector Rodrigues rotation in `build_uniforms`), **FOV** +
   **dolly-zoom**, and a lateral frame slide (`cam_frame`); the sequencer adds
   **Shuffle**/**Weighted** order, a **hold probability**, and **phrase-locked facing**.
   **Tier 3 (#307):** a **`StoryState`** plays an authored **storyboard** (`cam_story` —
   a header + 4 shot slots of `[path, bars, radius]`) that **overrides** the auto
   sequencer when enabled; each shot holds for its own bars, playback is
   Series/Random/Shuffle/Weighted seeded for reproducibility, and a bar-quantized
   **"next shot"** trigger (`cam_story[4]`, a `story_next_gen` atomic edge-detected like
   `hdr_gen`) advances on demand.
3. **Pulse & modulation:** compute the beat/audio pulse envelope; route it to up to 2
   target params (`apply_mod` — geometry targets land on a local `ParamValues`, look
   targets on the local `Shared` copy); apply the logarithmic **Speed Pulse** and the
   universal **Breath** scene-scale.
4. **Dispatch on `GeneratorMode`** (the big match): build `instances` + `tints`
   (node-field generators) or set flags for the raymarch siblings; report bounds for
   camera framing. Build the membrane mesh or metaball/voxel node set if needed.
5. **Build uniforms** (view/proj scaled by Breath, lighting, material, SSR/SSAO/GI)
   and assemble a **`RenderFrame`**.
6. **`Renderer::render(device, queue, view, &frame)`** runs all passes.
7. **Present**; write the `Feedback` (resolution/fps) reverse channel.

### `RenderFrame` / `RenderPath` (issue #104)

`render()` was an 81-line, ~55-argument call. PR #113 collapsed it to
`render(&mut self, device, queue, view, frame: &RenderFrame)`:

```rust
struct RenderFrame<'a> {
    // size / render_scale / uniforms / sky_uniforms / post_params at top level
    background: Background<'a>,      // terrain + stars
    surface:    Surface<'a>,         // path + the geometry payload + particles
    light:      LightTransport<'a>,  // ssao / ssr / gi / reaction-diffusion
}
enum RenderPath { Instanced, Membrane, Metaball, Volume, Voxel, Mandelbulb, Creature, MinimalSurface, Kifs, NeuralField, Lens, Splat }
```

`RenderPath` makes the render-mode mutual exclusivity a **type invariant** (you can't
pass two). The ~600-line body is unchanged — it derives the old local bools from
`path`, so behaviour is structurally identical.

### Render passes (in order)

0. **Compute prep** (as needed): metaball voxelize, voxel splat, terrain half-res,
   reaction–diffusion step, particle-aura sim (optionally the Navier–Stokes Fluid
   solver — which also runs, with an **RGB dye transport** (#182 Tier 1: inject →
   semi-Lagrangian or MacCormack advect along the projected velocity), whenever
   the **Fluid Ink** is on, even with the aura Off). #182 Tier 3a adds the
   **MLS-MPM liquid** (`liquid.rs` + `liquid.wgsl`, `Shared.liquid[16]`): a
   free-surface particle liquid (P2G fixed-point-atomic scatter → grid
   gravity/walls/colliders → G2P/APIC gather, substeps ×1–4) in an invisible
   tank centred on the smoothed field centre, with the generator's nodes as
   moving no-slip colliders (the T2 occupancy machinery, at the liquid's grid
   res). Its density splats into a SECOND `MetaField`'s 3D texture, and the
   scene pass draws that with the EXISTING metaball isosurface raymarch —
   full material stack, so **Material = Glass renders it as water** (route A
   of the tier; the screen-space hero surface + whitewater are Tier 3b). The
   sim math is mirrored + unit-tested CPU-side (`math::MpmSim`), which also
   seeds the GPU buffers (deterministic — integer atomics commute).
   **Gravity defaults to 0** (weightless — dialling it up pools the liquid), and
   the Liquid card's **"Reset pool" button** re-pours the seed distribution via a
   live counter in `Shared.liquid[14]` (stamped in `process()` like `hdr_gen`;
   not a param, never preset-captured — the sim edge-detects it and reseeds).
   **#182 Tier 4 — "one world, one light"** (`Shared.fluidgi[4]` +
   `Shared.caustic[4]`, all inert at 0) couples the medium into the light
   transport: (1) **fluid → GI** — `vxgi.wgsl::cs_resolve` samples the ink dye
   texture + the liquid `MetaField` per voxel (bindings 4–6 on the voxelize
   group; `VoxU` carries the source AABBs) and folds radiance + occupancy into
   the bounce volume; (2) **shadows both ways** — `fluidlight.rs`/`.wgsl` (a
   256² light-space RGBA16F map on the cube pipeline's **group 4**, r =
   Beer–Lambert dye transmittance marched from the key light, g = caustic):
   `cube.wgsl::fluid_light` multiplies the key radiance by it
   (`ShadowU.params2` carries the amounts, zeroed whenever the pass didn't
   run), while the ink march gains the scene shadow map + light matrix
   (`InkU.light_vp`, march bindings 8/9) so geometry shades the smoke;
   (3) **caustics** — `cs_caustic` fires one ortho key ray per texel, finds the
   liquid isosurface, refracts at the field-gradient normal (`refract_dir`,
   CPU-mirrored) and splats the landing texel — undeviated rays ≈ 1, so the
   resolve keeps `max(raw−1, 0)^sharpness`, applied as a light-space gobo;
   (3b) **the fluid receives GI** (no params — the existing toggles): the ink
   march binds the probe `GiUniform` + the VXGI radiance volume and adds both
   as per-step in-scatter; the metaball isosurface (incl. the liquid) gains a
   **group(4)** = the cube pipeline's GI bind group, adding the probe L0 term
   to its ambient; the VXGI voxelize also runs with the generator hidden when
   the ink is on (`screen_geo || ink.enabled`), and fluid-only injection
   survives an empty node list;
   (3c) **liquid material + ghost light** (`Shared.liqmat[8]`): the metaball
   raymarch now implements the FULL material branch set (Chrome/Glass ported
   from cube.wgsl — purity, clarity, IOR Fresnel, dispersion/thin-film — plus
   the `many_lights` point-light loop), and the liquid draws with its OWN
   group-0 uniform copy (`liquid_ubuf`/`liquid_bind`, the scene uniforms with
   `liqmat`'s type/metallic/roughness/IOR patched in; selector 0 = follow the
   scene). `liqmat[4]` = **ghost light**: hide-generator keeps probe GI + the
   VXGI voxelize + the emissive-cube lights alive — a pure GI/light emitter;
   (3d) **refractive water** (`liquidsurf.rs`/`.wgsl`, `liqmat[5]` render
   mode): a post-scene fullscreen pass — snapshot the resolved HDR (the buffer
   gains COPY_SRC + `Post::hdr_texture()`), march the liquid field, Fresnel
   split at the live IOR, Snell-refract and fetch the scene at the bent ray's
   landing (env fallback off-screen), Beer–Lambert over the marched thickness
   (`liqmat[6]` absorption). The in-scene isosurface draw is skipped in this
   mode; the pass runs after SSGI, before the ink march. The Liquid Material
   card carries the FULL dial set (`liqmat2[8]`: purity/clarity/F0/dispersion/
   glass-caustic/thin-film + glow), patched into the liquid's uniform copy;
   (4) **two-way coupling** — `sway.rs`/`.wgsl`: one compute dispatch after the
   instance upload samples the fluid velocity (NS grid, else the MPM `grid_v`)
   at each node and integrates a per-node damped displacement spring in a
   persistent GPU state buffer, displacing the instance translations IN PLACE
   (no readback; depth/shadow/scene all see the swayed structure; state zeroed
   on a count change). CPU mirrors: `math::optical_depth` / `refract_dir` /
   `sway_step`.
   `Shared.liquid2[4]` (the follow-up block — `liquid[16]` is full) carries the
   tank **vertical offset** (`liq_offset_y`, ±10 world units off the smoothed
   field centre), the **container shape** (`LiqShape`: Box / Sphere — a
   free-slip shell, gravity pools into a curved bowl / Cylinder / **Boundless**
   — no hard wall, a soft absorbing shell fades outward motion and pulls strays
   back so the liquid trails off into space), and the **render reveal** (0..1,
   a soft spherical window on the splatted density applied in
   `cs_resolve_field` — the isosurface closes into blobby trailing edges, no
   tank face ever shows). Shape walls live in `math::mpm_wall`, mirrored 1:1 in
   `liquid.wgsl::cs_grid`; sphere/cylinder also push penetrating material back
   inside (so the pool reshapes even at gravity 0) — 3 mirror tests.
   #182 Tier 2 (`Shared.fluid2[8]`)
   upgrades the solver: **solid boundaries** (node occupancy → moving no-slip walls
   in advection + a Neumann-masked pressure solve — wakes shed off the structure;
   `source.w` carries the CPU occupancy mark, `math::fluid_project_masked` is the
   tested mirror), **buoyancy** from a heat scalar riding `dye_a.w` (injected with
   the dye, cooling at its own rate), a radial **beat-splash** impulse + a
   **beat-gated dye** injection, a sim-res override (to 128³) and **substeps**
   (1–4, each a full solver pass).
0b. *(SSAO note, #174 T3)* `ssao.wgsl::fs_ao` is now **GTAO** (horizon-based
   visibility integration, Jimenez 2016) instead of the hemisphere kernel — same
   uniforms/target, better quality per tap; the blurred AO also feeds **specular
   occlusion** in `cube.wgsl` (Lagarde SO on the env-specular/mirror lobes, bound
   as group 3 binding 2 with a white no-op dummy when AO wasn't computed).
   **#195 Tier 3 adds an AO source switch** (`rt_ao.rs` + `rt_ao.wgsl`,
   `Shared.rt2[5..6]`): with SSAO enabled and source = Ray Traced, 1–4
   cosine-weighted SHORT hemisphere rays per pixel (t_max = the shared radius
   dial) trace the TLAS and fill the SAME raw-AO target GTAO would
   (`Post::ao_raw_target`), then the existing blur runs (`Post::blur_ao`) —
   the composite multiply + specular occlusion never know which source wrote
   it. Ground truth at short range: no screen-space haloing, off-screen
   geometry occludes; hits weight by distance falloff (t/radius) for GTAO's
   soft look; TAA integrates the low ray count. GTAO stays the default and
   the fallback (non-RT machines, sway live).
1. **Depth prepass** (single-sample) — only if SSAO / SSR / SSGI / DoF / TAA / VXGI /
   Fluid Ink is on (they share it). Rasterizes the instanced cube/tube geometry; **Membrane** normally
   skips it (so those screen-space effects are inert on membrane) unless the `membrane_fx`
   opt-in is on, which draws the membrane mesh into the prepass so they apply to it too.
   **Contiguous (welded) Swept Tubes** (`draw_swept`) likewise clears `instances` and draws one
   dynamic welded mesh — it is now rasterized into the prepass too (`screen_geo` includes `draw_swept`),
   so SSAO/SSR/SSGI/DoF/TAA/VXGI/screen-refraction **and the RT shadow/reflect/GI masks** apply to it
   (without this the RT shadow mask left the welded tubes fully shadowed → they went dark).
   All depth-only pipelines (this, early-Z, the shadow map) run a slim position-only
   `vs_depth` (#174 T2) — `@invariant`, mirroring `vs_main`'s position math, so the
   opaque scene pass's `Equal` test still matches.
   ⚠️ **`vs_depth` is not uniforms-only.** Mirroring `vs_main`'s position means calling
   the same `mat_displace_world` (#472 Tier 5), which samples the **group(5)** material
   height map — so `make_depth_prepass_pipeline`'s layout is
   `[group(0), –, –, –, –, group(5)]` (groups 1–4 are fragment-only and stay `None`
   holes) and **every** prepass draw site binds 0 **and** 5. Anything new that
   `vs_depth` reads has to be added to that layout and to all three draw sites, or
   `Renderer::new` aborts at `create_render_pipeline` before the first frame — a
   shader↔layout mismatch that naga can't see, so `cargo test` stays green and only
   the Mac catches it (this shipped broken on `main` once).
1b. **Hardware-RT shadow mask** (optional, #195 Tier 1 — `rt_shadow.rs` + `rt_shadow.wgsl`):
   when RT shadows are on (and the prepass ran), a fullscreen pass reconstructs each
   pixel's world position from the prepass depth, offsets along the derivative
   geometric normal, and fires one **any-hit ray** toward the key light (plus an
   optional fill ray) against the Tier-0 TLAS — into a screen-space visibility mask
   (r = key, g = fill) at the render resolution. `cube.wgsl::shadow_factor` samples
   the mask (group 4 binding 5, `ShadowU.params2.z/w` = the RT key/fill strengths)
   **instead of** the PCF shadow map — ground-truth occlusion, no bias/frustum
   tuning; the fill light gains shadows the map never had. Softness = the light's
   angular size via per-pixel cone jitter, resolved by TAA. Strengths are zeroed and
   a 1×1 white dummy stays bound whenever the pass didn't run (stale masks are never
   read). The shadow-map pass still runs independently (the fluid "receive" coupling
   reads it).
2. **Opaque early-Z prepass** (MSAA depth) — for the instanced opaque path. **At
   MSAA 1× with screen-space FX on, this pass is skipped** (#174 T2): the FX prepass
   renders the scene depth directly with the early-Z (cull-Back) pipeline, and the
   screen-space effects read that same texture — one less full-field rasterization.
   Cached FX bind groups are keyed by a `depth_epoch` + route bit so depth-texture
   recreation invalidates them.
2b. **Shadow-map pass** (optional, #152 Tier 3 — `shadow.rs`): when Cast Shadows is on,
   render the instanced geometry depth-only from the **key light** into a 2048² depth
   map (reusing the single-sample depth-prepass pipeline with a light-matrix group-0
   uniform). The ortho frustum is fit to the field AABB's 8 corners in light space with
   a texel-snapped origin (#174 T1 — the old padded sphere fit wasted ~85% of the map).
   `cube.wgsl`'s `shadow_factor` (group 4) PCF-samples it with a slope-scaled bias to
   darken the key light's direct term where occluded. Instanced path **and Contiguous welded Swept
   Tubes** (`draw_swept` casts the welded mesh, so the tubes self-shadow + shadow the scene); off → a
   dummy map + the shader's `enabled` flag = no change. A captured Look.
3. **Scene pass** → a **linear `Rgba16Float` HDR buffer** (MSAA target resolves into a
   single-sample HDR buffer):
   - **Background:** terrain (raymarched landscape + sky) **or** skybox (procedural/HDR
     env); then the additive **starfield + sun disc**.
   - **Geometry** (one `RenderPath`): instanced cubes/cylinders (PBR), or membrane mesh,
     or metaball isosurface, or **emissive volume** (#152 — the metaball field
     raymarched as a glowing medium, alpha-over), or **voxel DDA** (crisp grid-snapped
     cubes, now **physically shaded** — the DDA hit's flat face + splatted albedo feed
     the SAME metallic-roughness PBR + IBL + Material card as `cube.wgsl`, with the
     voxel AO folded into the indirect term and the soft-shadow threaded into the key
     light; `voxel.wgsl` binds group 0 = uniforms + group 1 = IBL + group 2 = the
     field), or Mandelbulb DE, or TPMS minimal-surface isosurface, or KIFS fullscreen
     field.
   - **Particle aura:** additive HDR billboards (depth-tested, no write).
   - **Capture decoration** (optional, #135 P5 — `axes.rs` + `axes.wgsl`): the XYZ axes are
     shaded **tubes with conical arrowheads** (a lit triangle surface, thickness-sliderable);
     the bounding box is gridded **back walls only** (the 3 faces away from the camera — a
     hidden-line "room", no busy interior lattice). Two pipelines (surface + line) share the
     camera + depth so they sit in 3-D among the geometry; alpha-blended. The X/Y/Z labels
     are 2-D — they project the axis tips world→screen and draw via the overlay text pass
     (§ step 8). Per-display.
4. **Voxel GI** (optional, #152 Tier 3 #10 — `vxgi.rs` + `vxgi.wgsl`): a compute
   **scatter** (one thread per node, fixed-point atomics into a ±3-cell window; a
   resolve pass normalizes per cell — #174 T2, replacing the 67M-`exp` gather)
   voxelizes the node field into a 32³ colour volume, then a fullscreen pass
   reconstructs world position from the prepass depth, marches the volume, and **adds**
   the world-space bounce **directly into the resolved HDR buffer** (`Post::hdr_view`,
   additive RGB-only) — before bloom, so it blooms + tonemaps with the scene. Sees
   off-screen/occluded emitters (the volume holds the whole field), unlike SSGI.
   Touches neither `cube.wgsl` nor the composite. Instanced path only; a captured Look.
   Runs **before SSR/SSGI** (#174 T1) so the screen-space passes, which sample the same
   HDR buffer, see the voxel bounce. March opacity is Beer–Lambert per unit length
   (energy independent of the step count), and the injected node colour is scaled by a
   radiance estimate (`glow + 0.3·key`, computed in `render.rs`) — the tint is an
   albedo, not an emission. **VXGI specular** (#163 Tier 3, `Shared.vxgi_spec[4]`): the
   same `fs_vxgi` pass also reflects the view ray about the reconstructed normal and
   cone-marches the SAME volume, so cubes reflect the actual scene (other cubes /
   off-screen emitters — no screen-edge dropout, unlike SSR), added on top of the
   diffuse bounce. `strength = 0` → skipped (byte-identical); needs the VXGI master
   toggle on (shares the voxelize + gather pass).
4b. **SSR** (optional) — superseded by RT reflections (#195 Tier 2, below) while they
   run — marches the prepass depth + resolved HDR (step-scaled hit band
   + bisection refinement, #174 T1; linear-z per-step reconstruction + stochastic
   GGX-cone roughness jitter resolved by TAA, #174 T3). Outputs `rgb` premultiplied by
   a confidence weight that is ALSO stored in alpha — the composite **blends** by it
   (see step 6).
4c. **SSGI** (optional, #152 Tier 2 — `ssgi.wgsl`, `Post::compute_ssgi`) — a sibling of
   SSR sharing the depth prepass: gathers one diffuse bounce from neighbours into a
   buffer the composite adds (exposed). Noisy at low ray counts; pairs with TAA.
   **Superseded by RT GI** (#195 Tier 4 — `rt_gi.rs` + `rt_gi.wgsl`) when it's on: a
   per-pixel cosine-hemisphere gather against the TLAS shaded from each hit's instance
   (glow + a GI fraction of its direct key, optional traced shadow) into the SAME
   buffer (`Post::ensure_ssgi_target`) — real inter-cube bleed with off-screen emitters,
   which the screen-space march can't reach; miss → 0.
4d. **Fluid Ink** (optional, #182 Tier 1 — `fluidvis.rs` + `fluidvis.wgsl`): the
   fluid solver's dye buffer is blitted into a 3D texture, then a fullscreen pass
   **raymarches the medium** into the resolved HDR buffer pre-bloom (premultiplied
   over-blend, RGB-only write) — Beer–Lambert extinction, Henyey–Greenstein
   key-light scatter with a short self-shadow light-march, ambient in-scatter from
   the **IBL irradiance map** (group 1 reused), and an emissive-dye dial. The march
   clamps at the prepass depth when it ran (ink composites against geometry; no
   prepass — raymarch modes / `hide_generator` — marches the whole volume). A
   **half-res march + depth-aware joint-bilateral upsample** is the perf dial
   (full-res = a pass-through blend). A **reveal** density knee culls the dilute
   haze (the vector-field reveal, for ink) so the dense filaments show through;
   the dye advection uses an **open domain boundary** (outside = clean water),
   so inflow faces flush instead of plating the AABB with opaque ink walls.
   #182 Tier 2 adds render-time **curl-noise micro-detail**: the march's sample
   positions are perturbed by an analytic divergence-free swirl scaled by the
   local |vorticity| (the blit folds a soft-mapped |ω| from the solver's curl
   buffer into `dye_tex.a`), so a coarse grid reads finer than it is.
   Off → all three passes skipped.
   *(All four depth-based passes — SSAO/SSR/SSGI/VXGI — reconstruct the normal as
   `cross(dpdy, dpdx)`: wgpu's framebuffer origin is top-left, so the GL winding
   `cross(dpdx, dpdy)` points away from the camera — the #174 T1 headline bug.)*
4b. **Scene Kaleidoscope** (optional, #361 Tier 1 — `kaleido.rs` + `kaleido.wgsl`,
   `Shared.kaleido[16]`). When enabled, a `liquidsurf`-style pass snapshots the fully-
   lit HDR buffer to a scratch and folds it back **in place**: for each output pixel it
   maps the screen coordinate through **N-fold kaleidoscopic symmetry** and samples the
   scene there, so the reflected shards are the real, moving, PBR-lit generator (ANY
   generator + surface — it consumes the render, unlike the KIFS *generator* which
   replaces it). Runs **before bloom**, in HDR-linear, so highlights + the EDR headroom
   stay physical and the fold rides the existing bloom/beat stack. `mode` picks
   **FullFrame** (each slice = the whole frame, mirror-tiled — swimmy) or **Wedge** (the
   classic optical kaleidoscope — identical slices); `spin` × the animation clock rotates
   the fold (so it rides Speed/beat); `zoom`/`center` frame the source, `twist` adds a
   log-polar spiral, `tint_*` hue-grade, `seam` supersamples the mirror lines, `mix`
   crossfades against the untouched scene. Off → the HDR buffer is untouched
   (byte-identical). A captured **Look**. (Tier 1 of #361; Tiers 2–3 — temporal/depth,
   then true 3-D reflection-group folding — are follow-ups.)
5. **Bloom** — soft-knee bright-pass → downsample/upsample chain (`post.rs`). The
   first downsample applies a **Karis average** (#174 T3 — per-quad 1/(1+maxRGB)
   weights) so sub-pixel HDR fireflies don't flicker-bloom, and the composite
   normalizes bloom energy by the mip count (window/DRS size no longer changes
   bloom brightness).
6. **Composite** → surface — exposure (EV) → add SSGI → **AO multiply (scene + SSGI
   only)** → **SSR blend by its confidence weight** (it replaces the env specular it
   supersedes, instead of double-counting) → add bloom → **tone-map** (#174 T1
   ordering; byte-identical when AO/SSR/SSGI are off).
   Branches on `hdr_max`: `≤ 1` = SDR via a chosen `ToneMap` operator (ACES/AgX/
   Reinhard/Neutral/**ACES Fitted** — Hill's RRT+ODT fit, #174 T3) plus a ±½-LSB
   triangular **output dither** (#174 T3, kills 8-bit banding; HDR output is float
   → no dither); `> 1` = **true-HDR** (macOS EDR / Windows scRGB — "Where `hdr_max`
   comes from" below) using the *same* operator for
   the diffuse range, then re-expanding highlights past `hdr_knee` into the display
   headroom (`hdr_reexpand`, #119) — so HDR is "the SDR look + brighter highlights",
   not a flat near-linear ramp (the old "shoulder" looked washed out vs SDR). The
   environment backdrop keeps its own `bg_tonemap` in both modes. Finally, when **wide
   gamut** is on (`hdr_wide` → the EDR surface is tagged **Rec.2020**, `hdr_macos.rs`),
   the HDR output is expanded from Rec.709 into Rec.2020 by **`hdr_vivid`** (#119): 0 =
   colour-accurate (`rec709_to_2020`), 1 = full stretch of the spectrum to the wide
   primaries — the lever that makes HDR look *more* vivid than SDR on a wide-gamut
   (triple-laser) projector. Off / SDR → output stays Rec.709 (unchanged). The tag is a
   *grant*, not a request: `bin/visual.rs` passes `wide_gamut` only when the surface
   really carries it, which on Windows is never yet (below).
6b. **Temporal pass** (optional, #152 Tier 2 — `temporal.rs` + `temporal.wgsl`). When TAA
   or motion blur is on, the composite (step 6) renders into the temporal **source
   texture** instead of the view; this pass then does **motion blur** (smear along the
   reconstructed camera velocity) + **TAA** (reproject the previous frame via the
   camera velocity, neighbourhood-clamp, blend) and writes onward (to the FX source if
   6c follows, else the view) *and* a **history texture** (MRT) used next frame. Velocity
   is reconstructed from the depth prepass via camera reprojection (current world pos →
   previous clip via the prev view-proj — camera motion only). **#174 T3 made it a real
   jittered supersampler**: while TAA is on the visual applies a Halton-(2,3) sub-pixel
   jitter to the scene view-proj (prepass + scene rasterize identically, so the Equal
   test holds), this pass reprojects with the UNJITTERED matrices, the history clamp
   runs in **YCoCg**, and the velocity source is **depth-dilated** to the closest 3×3
   neighbour (foreground motion wins at silhouettes). Leaves `composite.wgsl` untouched;
   off → composite writes straight to the next stage (byte-identical, and no jitter).
   Per-display (not preset-captured). Also gates **stochastic glass** (cube.wgsl
   dither-discard, the order-independent-transparency item — needs TAA to resolve) and
   denoises the #174 T3 stochastic-roughness SSR.
6c. **Post-composite creative FX** (optional, #152 Tier 1 — `fx.rs` + `fx.wgsl`). When the
   "Post FX" master is on, the composite (step 6) — or the temporal pass (6b) if it ran —
   feeds an FX **source texture**; this pass then applies the screen-space stack —
   pixelate → DoF (scene depth) → chromatic aberration → NPR style
   (Toon/Outline/Halftone/Dither) → colour grade → **halation + lens flares** (#167 Tier 1,
   `Shared.finishing[8]`) → vignette → film grain → feedback
   trail — and writes the result to the **view** *and* a **history texture** (MRT)
   sampled next frame for the trails. Halation is a wide, warm, red-weighted bright-pass
   halo around highlights (≠ bloom); lens flares add screen-space ghosts + a halo ring +
   an anamorphic streak keyed off the bright points — both additive, inert at amount 0.
   Leaves `composite.wgsl` untouched (the
   HDR/EDR/gamut path is unchanged); off → the upstream stage writes straight to the view
   (byte-identical). Runs at the full output resolution. Preset-captured (a Look).
   Per-frame history ping-pong (two textures, frame parity). **Chain when both are on:
   composite → 6b temporal → 6c FX → view.**
7. **Capture letterbox blit** (optional, #135 — `capture.rs` + `capture.wgsl`). With a
   fixed **Output Aspect** (`Shared.capture`), steps 3–6 render+composite into a
   **fixed-resolution production texture** (its size, not the window's, drives the
   projection aspect + render dims — so an OBS capture is pixel-exact and a 4K display
   stays 4K: `long_edge 0` = match the display). A final pass clears the swapchain to the
   backdrop colour and blits the production texture into the centred aspect-fit rect
   (`set_viewport`/`set_scissor`; pure pass-through, no clamp, so EDR survives), with an
   optional safe-area frame guide. **Native** (default) skips this — the composite writes
   straight to the swapchain. Per-display (not preset-captured), like HDR/MSAA.
   **In-app recorder** (#430 — `recorder.rs`, visual-only): the production texture
   (given `COPY_SRC`) is also the frame-exact **readback source** for recording. While
   armed (the **R** key; the **B** key cycles the length — Free / 8 / 16 / 32 / 64 bars,
   `next_record_bars`, `record_bars = 0` = free-run / manual toggle; N-bar takes auto-stop off
   `beat_pos`) the visual forces the
   production path even in Native, reads the texture back to the CPU right after the scene
   composite (before the letterbox blit), and pipes it to a spawned **ffmpeg** — **SDR** →
   H.264 Rec.709, **HDR** → the `Rgba16Float` radiance PQ-encoded on the CPU to Rec.2020
   10-bit HEVC (HDR10). So the file is the render itself, not a re-tone-mapped screen
   grab. **Tier 2 audio:** the plugin streams its post-synth stereo output through the
   `audio_ring` mmap channel (a continuous sample ring, off `Shared`); the recorder drains
   it per frame and muxes it in at stop (a second ffmpeg pass — video copied, audio → AAC).
   **HDR mastering target:** while recording HDR the composite's `hdr_max` is driven by
   `recorder::record_headroom()` (mastering peak ÷ SDR-white) instead of the display's EDR
   headroom, so the file holds the generated highlight range even beyond what the panel can
   show (the on-screen preview may clip brighter during a take). **Perfect capture**
   (Shift+R): a **fixed-timestep** mode — the visual drives the whole animation at exactly
   `1/fps` per frame (`recording_fixed` → the render `dt` override + `advance_beat_clock`'s
   PLL bypass) and the recorder captures **every** frame 1:1 (no CFR wall-clock resampling,
   blocks for a slot rather than drop, video-only). So motion is perfectly even and readback
   latency can't jitter it — the deterministic "offline render" that matches the viewport
   frame-for-frame, decoupled from wall-clock (the file's timeline is animation time).
   **Selectable record rate** (the **V** key, idle only): `recorder::Fps` is an exact
   rational, so 23.976 reaches ffmpeg as `24000/1001` rather than a rounded 24 — a clip cut
   into an NLE sequence at a broadcast rate doesn't drift a frame against it every ~42 s.
   Choices 23.976 / 24 / 25 / 29.97 / 30 / 60; **60 is the default** (the historic behaviour).
   **Phrase chunk mode** (the **C** key; **Shift+B** cycles the phrase, in *beats*: 4/8/16/32):
   record continuously and roll to a **new file on every musical phrase boundary**, so one
   pass over a song yields a folder of grid-aligned clips that butt-join on a music-video
   timeline. Arming lays a phrase grid in continuous `beat_pos` space **phase-aligned to the
   host's `pos_beats`** at that instant (the host value wraps mod 1024, which every
   power-of-two phrase divides, so the grid survives the wrap); the first clip is spawned
   **warm but gated** and its shutter opens on the boundary, so the ffmpeg fork/exec happens
   *before* the downbeat rather than on it. Each clip's length is an **exact frame quota**
   (`chunk_frames`) rounded from *absolute* musical position (`round(k·L)`), never
   accumulated — the phrase is almost never a whole number of frames (8 beats at 172 BPM is
   66.977 frames at 24 fps), so quotas alternate between the bracketing integers and every
   cut stays within ±½ frame of true **forever** instead of walking off the grid. While
   chunking, the CFR pacer is **beat-driven** (`ideal_frame`) rather than wall-clock, so the
   file's timeline is the *song's* timeline. Rolling is only possible because
   **`finish_async`** moved the blocking tail (worker drain, ffmpeg wait, WAV write, mux
   pass, rename — seconds of work) off the render thread onto a `Finalizer` thread; the
   handles are joined at exit so a session can't die mid-mux. Disarming discards the partial
   clip (`Recorder::discard`). Chunk mode and perfect capture are mutually exclusive (one
   rides the host's real-time transport, the other deliberately decouples from it). Off by
   default / byte-identical when idle. Overlay bake and the editor Record card are later
   #430 tiers.
8. **Capture overlay** (optional, #135 P2 — `overlay.rs` + `overlay.wgsl` + `overlay_meta.rs`).
   The maths-account text card — title / description / formula / a live readout panel /
   handle — alpha-blended on top of the final image, laid out **inside the production rect**
   (so it tracks the letterbox). A self-contained renderer (no `glyphon`/`wgpu`-coupling): a
   CPU `ab_glyph` glyph atlas + bundled pre-rendered TeX **formula PNGs** (MathJax→SVG→PNG,
   per-variable `\textcolor` baked in), all drawn through one `LoadOp::Load` pass. The
   per-generator **metadata** (`overlay_meta.rs`, pure + unit-tested) supplies the title /
   description / symbol colours / readout layout and a per-generator `eval(clocks)` that
   computes the live "formula plugged in" values each frame (6 flagship generators with a
   bespoke eval — incl. **Synchrotron** (#150 P2): R / β / γ / κ_min / beam-angle; the other
   ten a key-param readout). All 16 carry a bundled formula PNG.
   Text is written at display-white with a drop shadow
   (EDR-safe). Per-display, not preset-captured; **T** toggles it.

### Geometry, materials, MSAA, resolution

- Mesh bank — `cube_mesh`, a unit `cyl_mesh` (open, radial normals), the Boids
  `creature_meshes`, and (#260 Tier 1) two Neural-Tissue meshes: `soma_mesh` (a
  subdivided-octahedron **icosphere**, radius 0.5, for cell bodies + boutons) and
  `capsule_mesh` (a **capped** cylinder — hemispherical caps folded into z∈[-0.5,0.5]
  so the tube is closed, no open pipe). The `RenderPath`/`tube` flag picks the cylinder
  for Swept Tubes; `neural_capsule` swaps in the closed capsule for the Neural Tissue
  surface on non-graph generators. `cull_mode = None`, so most swaps need no winding
  care (all four extra meshes are still wound outward for the opaque early-Z path).
- Instanced: one static mesh + per-instance model matrix + per-instance colour tint.
- **Which draws sample the #472 material set** — `render()` scopes it with `material_draw`,
  a predicate **separate from** the bevel's `cube_draw` even though they were one until the
  Console Spike's Tier 2. They scope different things: `cube_draw` protects the shared cube
  *mesh* from a morph meant only for the generator's cubes, while the material set is a
  surface *response* any draw shading through `cube.wgsl` on the main uniform can carry.
  `material_draw = cube_draw || (membrane && !membrane_arms)`, so the **Membrane** path's
  lofted sheet (and its optional boundary strands, which share every other material dial)
  now sample the maps — that is what lets the Organon Console's flat substrate backdrop wear
  a procedural material at all. It is a **uniform-value** gate, never a pipeline one: group(5)
  was already bound at both membrane sites (the scene branch and the depth prepass, which
  read the same uniform, so a height-displacing material stays consistent between them).
  Byte-identical by default — `u.mtl.x` comes from `material[0] || material_layer[16]`
  (`world.rs`), both 0 at the stock defaults. Everything else still zeroes `mtl[0]`: the five
  patched uniform copies (plexus overlay, liquid, scenery, scenery water, demo sub-batches)
  do it because their geometry has a material of its **own**; the membrane has none, it
  shares the generator's.
- **Neural Tissue multi-mesh draw** (#260 Tier 1): when `Surface.neural_batches` is set
  (the Neural Network generator under the Neural Tissue surface), the ONE instance/tint
  buffer holds three **contiguous sub-batches** — somata `[0,soma)`, capsules
  `[soma, soma+cap)`, boutons `[…, total)` — and `Renderer::render` issues one instanced
  draw per sub-batch (`draw_neural_batches`), binding `soma_mesh`/`capsule_mesh`/`soma_mesh`
  and slicing the shared buffers at the sub-batch byte offset. **No new pipeline** — it
  reuses the instanced cube/tube pipeline (per-instance model loc 3–6 + tint loc 7). The
  same sub-batch draws run in the FX depth prepass, the opaque early-Z prepass, the shadow
  map, and the scene pass, so screen-space FX + shadows are correct. The lowering
  (`math::neural_tissue_lay`) is the closed-primitive sibling of `neural_net_lay`.
- **Neuron morphology** (#260 Tier 2): `neural_tissue_lay` grows a real neuron per node —
  `grow_neuron` sprouts a dendritic arbor via `dendrite_tree` (a **seeded, frame-stable**
  recursive bifurcation with a **monotone radius taper** to the tips — child radius =
  parent × `dendrite_taper`), plus a **hillock axon** (a thinner process off a distinct
  soma point, toward the node's primary downstream neighbour) ending in a small **terminal
  bouton arbor**. `NeuronType` selects pyramidal (apical trunk + basal skirt) / stellate
  (bushy radial) / by-degree. Arbor rods extend the **capsule** sub-batch; terminal bulbs
  the **bouton** sub-batch (both stay contiguous). A per-neuron branch budget bounds cost.
  Dials ride `neural_surface[5..10]` (density/length/taper, type, spines); `density = 0` →
  no arbor (byte-identical to Tier 1). Unit-tested (determinism, taper monotonicity,
  boundedness, budget, type divergence).
- **The living synapse + tissue context** (#260 Tier 4, the final tier): `neural_tissue_lay`
  gains three synapse dials (`neural_surface[13..16]`) + two tissue-context dials
  (`neural_surface2[0..2]`). `synapse_cleft` pulls each edge's terminal **bouton** back off
  its post-synaptic target (the `pa.lerp(pc, 0.86)` bulb → `0.86 − 0.12·cleft`) so a visible
  **synaptic cleft** gap opens. `synapse_vesicles` releases a **deterministic vesicle burst**
  — a few tiny short-lived instances marching across the cleft — on each spike **arrival**
  (the cascade sim's `edge_pulse ≥ 0.82` deposit event, the same one Tier 3 flashes the
  bouton on); the crossing fraction advances with the pulse position, so the burst is a pure
  function of sim state (no per-frame flicker), emitted into the **bouton** sub-batch.
  `synapse_glow` finalizes the neural material: it lights each soma's **cytoplasmic interior**
  (`cyto = 1 + glow·1.5·activation`, exactly 1 when the dial is 0) from within, layered on
  the Tier-1 `membrane_sss`/`membrane_irid` waxy-membrane path (`build_uniforms`). Tissue
  context: `glia` sprouts faint sparse **astrocyte scaffolding** (short branching stubs off a
  seeded subset of somata) and `capillary` routes a few dim wandering **capillary threads**
  across the tissue volume — both into the **capsule** sub-batch, counts scaling with the
  dial. Every Tier-4 feature is inert at its default 0 (byte-identical to Tier 3). Unit-tested
  (cleft gaps the bouton, vesicle burst deterministic + only on arrival, context counts scale,
  all-inert). Follow-ups (need a shader/pipeline change): extracellular-medium volumetric fog
  + a wet-fresnel membrane rim.
- The cube pipeline blends **premultiplied alpha** (#174 T1): `fs_main` multiplies the
  attenuable terms by alpha itself — Standard/Chrome premultiply their whole output
  (byte-identical to the old SrcAlpha blend at any opacity), while Glass fades only the
  transmitted body (refraction/SSS/GI bounce) and composites its Fresnel reflection /
  specular / emissive at full strength.
- `cube.wgsl` branches on a `material_type` uniform: **Standard** PBR, **Chrome**
  (sharp prefiltered-env mirror + Fresnel), **Glass** (Fresnel reflect+refract by
  `ior`, F0 derived from the IOR, alpha-blended; spectral dispersion/thin-film/caustic
  from the Jewel Box work), and **Refractive** (the Glass path plus **Beer–Lambert
  absorption** over the measured chord through the instance body — the liquid's
  see-through-water optics on the generators: the vertex stage passes the mesh-local
  position + the instance frame's inverse, `instance_thickness` slab-tests the ±0.5
  unit box along the refracted ray with a world-distance parametrization, σ per
  channel = `(1 − albedo) × Shared.refrmat[0]` so the node's own colour survives, and
  the murk lifts alpha so thick bodies occlude the scene behind. Inherits every glass
  dial; absorption 0 = Glass byte-identical. The raymarch siblings
  (metaball/minimal) have no per-instance body and fall back to Glass; the membrane
  sheet draws with an identity instance, which the local-position bound rejects →
  thickness 0).
- **Physical thin-film interference** (#258 Tier 1, `Shared.thinfilm[4]` =
  `[film_thickness, film_thickness_var, film_ior, film_drainage]`): a real soap-film /
  bubble iridescence model that replaces the cosine-hack `thin_film_tint` on the
  Glass branch (and the Foam/Bubble raymarch's intrinsic sheen in `minimal.wgsl`)
  when `film_thickness` (base thickness in nm) > 0. `thin_film_physical` evaluates a
  wavelength-resolved low-finesse **Airy interference** reflectance (phase
  Δφ = 4π·n·d·cosθ_t/λ at three RGB wavelengths, Fresnel at the air/film interface),
  driven by an **actual thickness field**: base thickness + a **gravity-drainage
  gradient** (thin at the top → thick at the bottom, along world-space up) via
  `film_drainage`, plus value-noise **marbling** via `film_thickness_var`; `film_ior`
  = the film's index. `film_thickness = 0` → the shader keeps the existing
  cosine-hack path, so the appended block is **inert at the default** and the default
  snapshot only gains zero-thickness bytes (golden re-pinned, no `LAYOUT_VERSION` bump). Shader-
  only; captured as a **Look**. Actual bubble look unverified without the Mac.
- A **refraction overlay** (`refrmat[1..2]`: checkbox + blend dial)
  weaves that same measured-chord transmission into the OTHER three types on top of
  their own shading — Standard's diffuse body opens into (roughness-frosted)
  refraction on the (1 − ks) energy split while its PBR specular/direct stay, Chrome
  yields to a refracted core face-on by the IOR Fresnel and stays mirror at grazing
  angles, and Glass gains the murk scaled by blend. `ior` + `absorption` drive the
  overlay; overlay 0 = every branch byte-identical; redundant on Refractive itself.
  A fifth type, **Anisotropic** (#214 Tier 1, `Shared.aniso[4]`), is Standard PBR with
  an **elliptical GGX** specular lobe (Burley NDF + Heitz height-correlated visibility)
  instead of a round one — brushed metal / satin / hair. The tangent frame comes free
  from the instance frame's local +Z long axis (passed as the `brush` varying; the
  `cyl_mesh` axis, so Swept Tubes comb along their length), a rotation dial re-aims it,
  and the env reflection uses a bent-reflection-vector approximation against the same
  prefiltered mips (no new textures). It's also an **overlay** (`aniso[2..3]`: checkbox
  + blend) on Standard/Chrome — brushed chrome is the showpiece. The Anisotropic enum id
  (4) falls through to the Standard branch (the Glass/Refractive branch is bounded
  `< 3.5`); amount 0 = isotropic, byte-identical. Raymarch siblings stay isotropic.
  Two more (#214 Tier 2, `Shared.coat[8]`): **Clearcoat** (id 5) adds a thin smooth
  dielectric lobe (F0 = 0.04 ≙ IOR 1.5) over the Standard base — a second env reflection
  + sharp glint at the coat's own roughness, and the base attenuates by the coat Fresnel
  (car paint / lacquer / ceramic / wet); **Velvet** (id 6) adds a **sheen** lobe (Charlie
  NDF + Neubelt visibility) that blooms at grazing angles with a white→albedo tint
  (velvet / dust / moss). Both are also **overlays** (`coat[2]`/`coat[3]` checkboxes) on
  Standard/Chrome — lacquer a brushed metal, dust any surface — computed once and woven
  into the Standard + Chrome returns (`base·base_scale + coat_spec + sheen_add`). Ids 5/6
  fall through to the Standard branch; Glass/Refractive are excluded (their transmission
  owns the look). All lobes off → byte-identical.
  All of these reflect the env/HDR only (no inter-cube SSR except the optional `ssr.wgsl`).
  A **reflection-look** block (#163 Tier 1, `Shared.reflect[4]` → the `reflect_ctl`
  uniform, all 0 = today's look) layers on top: `chrome_purity` drives Chrome to a pure
  neutral mirror, `glass_clarity` drives Glass to colourless clear glass, `f0_override`
  lifts Standard's reflectance toward a mirror without forcing metallic, and
  `reflect_tint` mixes the palette into the reflection (0 = neutral, >1 = override).
  A **reflection source** (#163 Tier 2, `Shared.refl_probe[4]` → the `refl_box_min/max`
  uniforms): `EnvOnly` (default, today — a pure direction lookup) or `Parallax`, which
  box-projects `reflect(-v,n)` against the field's live AABB (`parallax_correct`) so the
  reflection shifts with a cube's *position*, not just its orientation — removing the
  "painted-on sky" flatness. The visual fills the box from the field bounds at the
  uniform-patch site; `source = 0` leaves the box off → the reflection is unchanged.
- **MSAA** (1/2/4/8): `set_sample_count` rebuilds the scene/sky pipelines + depth +
  multisample targets. **Dynamic resolution:** the scene + post render at `size·scale`
  (manual 0.25–1.0 or auto-targeting 60 FPS); the composite upscales to the full-res
  swapchain. The auto scale is quantized to 1/16 steps with hysteresis (#174 T2 —
  every distinct scale rebuilds the whole target set, so the old smooth lerp
  reallocated hundreds of MB per frame during transitions). SSR/SSGI targets
  allocate lazily on first use; the GI/many-light uniforms and the RD sim skip
  their work entirely while off.

### Per-instance emission — the glyph ring's phosphor (organon#217 T1)

The instanced cube/tube pipeline carries **four** per-instance buffers, not three: the
model matrix (loc 3–6), the tint (loc 7), and since organon#217 T1 an **emission**
`vec4` at **loc 8** — linear RGB radiance in `xyz`, gain in `w`. `cube.wgsl`'s emissive
term is `albedo * (glow + env_tint.w) + ripple + rd + emit.rgb * emit.w`: the new term
**bypasses albedo**, because a terminal cell's colour is display-referred — a phosphor
behind a near-black faceplate, not a reflectance — and the existing `tint` path would
multiply it to nothing (`doc/pbr_text_engine.md` §4).

🚨 **Inert by construction (invariant #4).** `Surface.emits` is `&[]` on every frame the
glyph ring is not driving, and the renderer then binds an all-zero buffer: `make_emit_buf`
creates it and wgpu zero-initialises a fresh buffer, nothing writes it until a glyph frame
does, and the range a glyph frame lit is zeroed back the frame after (`emit_len`). With
`emit == vec4(0)` the added term is exactly `vec3(0.0)` and the expression reduces to the
one it replaced — so the frame is byte-identical, with no `Shared` field and no
`LAYOUT_VERSION` move. A non-empty `emits` is honoured only when it is exactly
`instances.len()` long; any other length is treated as "no emission", never as a partial
upload.

⚠️ **A fourth layout in a pipeline is a fourth buffer at every draw against it**, or wgpu
fails validation at draw time — and no leg of the bar has a GPU. So: `emit_vertex_layout()`
is built in one place and listed by both `make_cube_pipeline` and
`make_depth_prepass_pipeline` (the prepass's `vs_depth` never reads loc 8, but the scene
pass and the prepasses share draw code, so it takes the same four buffers and ignores the
fourth); every `set_vertex_buffer(2, …)` has a `set_vertex_buffer(3, …)` twin — `emit_buf`
beside `tint_buf` (sliced at the same sub-batch byte offsets), and `zero_emit` beside
`white_tint`, the scenery's and the plexus overlay's tints, regrown by `ensure_zero_emit`
whenever any of those could draw more instances than it covers. Grep the two counts and
they must agree.

⚠️ **The zeroing is a high-water mark, not the previous frame's length.** Glyph frames
shrink as an effect animates (fewer live cells), so a 100-instance frame followed by a
50-instance one leaves `[50, 100)` lit unless the shrink itself zeroes it; the review on
#224 caught the first version trusting the last length, which a later 80-instance
generator draw would have read. `emit_upload_plan(high, lit)` — pure, tested without a
GPU — returns the dirty range beyond this upload, `[lit, high)`, and the new mark, so
after any sequence of frames the possibly-non-zero set is exactly `[0, last lit)`.

📌 **The ray-traced passes read it too (organon#217 T8).** T1 named the gap — the
hardware-RT and path-trace passes took `inst_buf`/`tint_buf` as storage and shaded a hit
from the tint alone, so a ray-traced reflection of the glyph grid was a reflection of dark
faceplates and the T5 dwell (below) converged to **black**, measured on the first GPU look
(2026-09-02, `doc/pbr_text_engine.md` §15). T8 closes it for the three passes that shade a
hit: `rt_pathtrace`, `rt_reflect` and `rt_gi` bind `emit_buf` as a read-only storage
buffer beside the instance and tint buffers (`emits` at `@binding(7)` in the tracer — after
the caustic map and the cache weights — and `@binding(5)` in the other two), index it by
the same `instance_custom_data` the hit reports, and add `instance_emission(idx)` = **the
same expression `cube.wgsl` adds**, `emit.rgb * emit.w`, into the hit's radiance — pinned
textually identical across the three shaders by test, so raster and traced agree on what a
lit cell is worth (§9's second law). The tracer treats an emissive hit as a light: the
radiance is added and the path **terminates**, in both the RGB and the hero-wavelength
loops (the "lights are emitters" simplification — a lit tile's tint is the near-black
faceplate, so the dropped continuation is ≤ albedo × incident, and a fullscreen grid then
costs one ray per pixel instead of `bounces`); the gate is the emission's *value*, so a dark
tile with `emit == 0` keeps bouncing and shows the room. In GI-add mode the primary-hit
emission is skipped like the other primary terms (the raster already shows it) **and the
path continues** — the tracer owes that pixel its indirect light, so the termination sits
inside the same guard (#232 review). Its
next-event estimation reaches only the key and fill directions — there is **no light list
and no light selection** — so a lit tile is found by the cosine bounce landing on it, which
converges over the dwell but is noisier than NEE; an emitter list would ride T10's
brightest-N selection and is a documented hook, not built. **What does not read it, and
why:** `rt_shadow` and `rt_ao` trace visibility only (a hit is a boolean, never shaded), and
`rt_caustic` shades hits for the *photon's* BSDF, where the landing surface's emission plays
no part — emitters as photon *sources* would need a per-frame CDF over instances and is a
tier of its own (its layout comment names the binding it would take). 🚨 **Inert by
construction still holds**: the all-zero buffer every non-glyph draw binds makes every added
term exactly zero and the termination gate false, so each pass's output and RNG stream is
byte-identical. ⚠️ The binding is a bind-group entry, not a vertex slot, and wgpu validates
it at *draw* time — a layout entry with no matching `create_bind_group` entry is a runtime
panic CI cannot reach — so each pass's layout is a pure `layout_entries()` its unit test
holds: index, read-only storage, fragment visibility, and that the WGSL declares `emits` at
the **same** `@binding`. 🚨 And the buffer itself must be *created* with `STORAGE`, or
wgpu refuses the bind group at creation — `make_emit_buf` was `VERTEX | COPY_DST` only,
and nothing but a GPU would have said so (#232 review). Every buffer an RT layout binds as
storage — instances, tints, emission, in `new` and on every regrow path — is now created
with one `RT_HIT_BUFFER_USAGE`, and `rt_hit_buffer_tests` walks every `BufferDescriptor`
in `render.rs` and fails naming the label if one of those is created any other way.
Nothing here has been looked at on a GPU: what a session must see
is the dwell converging to a *lit* still, and a glossy backplane reflecting lit glyphs.
This is the **cube pipeline's** emission only: the capsule impostors have their own
per-instance emission in `particles.wgsl` (the `ArmInstance` colour), which is what T6's
coaxial glass capsule shows through its shell (the "Shaders" entry below) — the `bottled` /
`cathode` presets will ride that path, not this attribute.

The producer of the only non-empty `emits` today is `world.rs`'s `glyph_grid_geometry`
(the glyph ring, `organon-core/src/glyph_ring.rs::lower_grid` — see `ARCHITECTURE.md`'s
`$TMPDIR` channel list). Its look — tile depth, gap, gain, faceplate, backplane — is
`GlyphLook::DEFAULT`, one `const` in core that **T3 lifts onto the param chain**. Whether
a look still *reads* is T2's question, not this section's: the legibility harness (its own
section below) takes the same cell grid this ring carries as its fixture and scores the
render against it, which is what makes "is this preset still readable" a number rather
than a matter of taste.

#### Converge on hold (organon#217 T5)

The path tracer restarts its progressive accumulation on a camera move, a buffer resize, or
a change to the settings that decide what the buffer holds — the **content key**,
`world.rs::pt_content_key` — and deliberately **not** on geometry change: the TLAS rebuilds
every frame, so "the geometry moved" is true of nearly every frame, and a moving field would
smear the average. T5 adds one geometry counter to that key, and it is the exception that
proves the rule: the glyph ring's `GlyphFrame.generation` is bumped by the *producer* only
when the cell payload differs from its last publish, and a dwell heartbeat republish keeps
it. So keying on it restarts accumulation exactly when the glyphs move and accumulates
exactly while they are held (`doc/pbr_text_engine.md` §8). ⚠️ Keying on the frame's `seq` or
`tick` instead would restart every 250 ms heartbeat and never converge. The key carries a
`live` bit beside the generation, so "no ring" and "ring at generation 0" cannot collide,
and a producer going silent (the world hands the frame back to the generator after 3 s) is
itself a content change.

**The handover is one pure predicate**, `world.rs::pathtrace_active(preset_pt, glyph)`:

> the preset's own toggle (`pathtrace_on` — the editor checkbox or the 'P' key),
> **OR** a glyph frame is drawing this frame **AND** it carries `FRAME_SETTLED`.

A preset that already path-traces is untouched (the OR is already true). A preset that
rasters rasters through every frame of an effect's motion — it is `GlyphPtState.live &&
!settled` — and hands the frame to the tracer for the dwell, where it sharpens over the
hold into a converged still and drops back to raster the instant the next effect's first
payload arrives (generation bumps → key changes → count to 0; `settled` clears → tracer
off). A session with no ring reduces to the toggle alone and is byte-identical to before
T5. Every other gate the tracer already had — `hide_generator`, a boids creature, the
render path being `Instanced` with instances, ray-query support — still applies on top.
The restart itself (`pathtrace_restarts`) is keyed on that live answer rather than the
toggle, so during motion the count is held at 0 and the dwell's first traced frame starts
from a clean buffer. **Silence is not settle**: a ring whose producer exited may still carry
`FRAME_SETTLED` on its last frame, but `live` is false once the world stops drawing it, so
a stale grid is never traced as though it were held. All of it is pinned in
`organon-world`'s tests (`the_dwell_converges_and_the_next_effect_restarts_it` walks one
whole motion → settle → dwell → next cycle).

⚠️ **Two things this did not do, and what T3 did about them.** It does not touch TAA: the
jitter is `Shared.temporal[0]`, and that block is **param-only** (`pack_temporal` declares
one packer), so no preset — `faceplate` included — can switch it; §8's warning
(`temporal.rs` reprojects by camera only; teleporting glyphs ghost) is met by TAA's default
of OFF, and a session that turned it on keeps it on through a recall. And it did not still
the camera: `pt_moved` compares the unjittered view-proj, so a preset whose auto-orbit is
running restarts accumulation every frame and the dwell never converges — measured on the
first GPU look (2026-09-02). **T3 holds it** (organon#217): `world.rs::glyph_camera_rig` is a
second absolute arm on the camera selection, below the Console's `substrate_rig` and above
rails and the orbit — `(centre, yaw 0, pitch = tilt, distance, roll 0, fov)` with the
distance computed from the tiles' bounds and the frame's FOV/aspect
(`fit_distance`), never from the wheel — active only while a ring is live *and* the
preset's `glyph_cam[0]` (hold) is set, so a no-ring session and a preset that did not ask
stay on the orbit rig. `a_held_rig_lets_the_dwell_converge_where_an_orbit_cannot` walks
the same restart logic with a held rig (accumulates) and an advancing yaw (restarts every
frame). T3 also patches `Uniforms.shape` for a live ring: `x` the glyph look's own bevel,
`y` the **face crown** — a per-fragment dome normal in `fs_main` (normal-only; the depth
prepass, the silhouette and the RT hit shading are untouched), gated on `y > 0`, which every
other draw writes as 0. 🚨 Nothing here has been looked at on a GPU: what a GPU session must
see is the frame visibly sharpening over the dwell after an effect settles, and restarting
cleanly — no after-image of the held text — when the next effect begins.

#### The tile (organon#217 T9)

The plates show every cell as a **tile**: an emissive core with a soft falloff across
the face, seen *through* a thin glossy faceplate over a near-black body, and a dark cell
that still carries a sheen of the room. T9 is the shading half of that, in `cube.wgsl`;
the lowering half (a tile for every cell, dark ones too) is `glyph_ring.rs::lower_grid`'s
and is a follow-up.

**The emission profile.** `fs_main`'s per-instance term is now
`emit.rgb * emit.w * tile_profile(face_uv(local_pos), shape.z)`. `face_uv` is the
fragment's two coordinates across the face it sits on — the dominant axis of the
**un-rounded** mesh-local position, the same rule (and now the same function,
`face_axis`) the T3 crown uses, so a bevelled band still belongs to its face and the
crown and the profile cannot disagree about which face that is. The rounded-box mesh
carries no UV attribute; it needs none, because `VsOut.local_pos` is `VsIn.position`,
still on the flat unit cube, and a 1×2 tile's face is a square in it — so the profile
stretches with the tile and is keyed on the tile's own extent, never on screen space.
`tile_profile` is `mix(1, (1 − s)², k)` with `s = (2u)⁴ + (2v)⁴` clamped to 1: a p=4
squircle, flat-topped at the centre, soft-landing at the edge, `1 − k` on the edge
midlines and in the corners. ⚠️ It multiplies the **per-instance term only** — the
albedo-modulated legacy glow, the ripple and the RD term belong to other generators and
are untouched — and at `k = 0` it is **exactly** `1.0` (not close to it), so the
expression reduces bit for bit to T1's and every draw today is byte-identical (invariant
#4). The strength rides `Uniforms.shape.z`, which the shader never read before and
`build_uniforms` writes as 0; the world's `glyph_shape` lifts it from `Shared.glyph[13]`
for a live ring (`glyph_profile` on the param chain; with no ring the lane stays the
frame's own 0), and `render()` zeroes `shape.z` off the generator cube draw beside `x`
and `y`, so a live ring cannot hand the profile to another draw's per-instance emission.
Pure, so `glyph_tile.rs` mirrors it and pins zero-strength-
is-exactly-one, sign and axis-swap symmetry, monotone-along-every-ray and the curve's
values, plus a source check that `cube.wgsl` still defines both functions with the
mirrored signatures.

**The faceplate needs no code.** The clearcoat lobe (#214 T2) already composes the
Standard branch as `color * base_scale + coat_spec`, with `emissive` inside `color` and
`base_scale = 1 − fc` — so under a Clearcoat the phosphor is already seen *through* the
coat's Fresnel transmission, and `coat_spec` (the coat's prefiltered environment along
the isotropic reflection) is computed without `emissive` and added after it. A tile with
`emit == 0` therefore shades as its near-black body plus that sheen: the dark cell that
reflects the room (`doc/pbr_text_engine.md` §4.1). T3's `faceplate` preset sets the
Clearcoat material at roughness 0.22, so the faceplate is **preset data**. ⚠️ The coat is
a per-draw uniform and the backplane is an instance of the same draw, so it wears the
same coat; giving the backplane its own lobe is the same "own draw" question §15 raises
for the anisotropic backplane, and is T10's.

📌 **What does NOT see the profile:** the hardware-RT and path-trace hit shading (T8
reads `emit.rgb * emit.w` flat), so a T5 dwell converges to a flat-cored tile where the
raster frame showed a falloff. `tile_profile` and `face_uv` are pure functions of the
hit's mesh-local position, so the tracer can apply the same two lines once it has the
local hit point; named here rather than done, since `rt_*` is another worker's file.

🚨 Nothing here has been looked at on a GPU: green and ready to try. A GPU session must
load `faceplate` with a producer running and see a lit cell's core fall off toward its
edges (`glyph_profile` is 0.5 there, wired since the two lanes landed as parameters); a
dark cell (`glyph_dark_tiles` on, the full-grid lowering) show the environment's sheen
with zero emission; and the bevel highlight unchanged, since the
profile touches no normal and no vertex.

### Hardware ray tracing (#195 — the `rt_*` modules + shaders)

The plumbing every later RT effect (shadows / reflections / AO / GI) will trace
against; **ships dark** — with everything at default the render is byte-identical.
The visual requests wgpu's **`EXPERIMENTAL_RAY_QUERY`** feature only when the adapter
offers it (Metal exposes RT as ray queries; hardware-accelerated on M3+); without it
`rt::RtContext::new` returns `None`, every use site no-ops, and the editor greys the
card out via `Feedback.rt_available`. While the **RT master toggle** (`Shared.rt[0]`,
a captured Look) is on and the path is **Instanced**, each frame rebuilds a **TLAS**
from the same per-instance model matrices the instance buffer gets (BLAS per static
mesh — cube + cylinder via `render::rt_mesh`, built lazily in the first TLAS
submission; `tube` picks the cylinder exactly like the raster path). The field
animates every instance every frame, so it's a full rebuild, not a refit — the
smoothed CPU encode+submit cost is reported as **`Feedback.tlas_ms`** (shown live in
the editor card): the number that green-lights Tier 2. Capacity is fixed at
`RT_MAX_INSTANCES` (65 536); larger fields truncate. The **debug view**
(`Shared.rt[1]`, per-display like HDR/MSAA) draws a fullscreen ray query over the
final frame — geometric normals (screen-derivative of the hit point), instance-index
hash, or hit distance — so the TLAS can be verified against the raster scene: if the
silhouettes line up, the acceleration structure matches what was drawn.

**Which adapter and backend you actually got (#658 Tier 1).** `wgpu::Instance::default()`
picks silently, and until this tier nothing in the visual reported the choice — so
"HighPerformance landed on the iGPU" and "the scene is slow" were indistinguishable, and
so were "this box has no ray tracing" and "this *backend* has no ray tracing". The visual
now prints one line per launch to stderr, beside the `HDR output:` lines:

```
GPU: <adapter> [<Backend>, <DeviceType>] driver: <driver> <info> — granted: ray-query, timestamp, …
GPU: adapter also advertises: coop-matrix <yes|no>, shader-f16 <yes|no>
```

Two things make it trustworthy rather than decorative. It reports the **granted** features
— `device.features()` after `request_device`, not the `wanted` set — so it can never claim
a capability the device declined. And it names the **backend**, which is the fork that
matters: `EXPERIMENTAL_RAY_QUERY` is the bit the whole `rt_*` stack rides on, and its
availability is a per-backend question. The second line is *advertised, not enabled* —
#200 Tier 2 deliberately keeps cooperative matrix dark in the render loop — and exists
because it is what #658 Tier 5 needs to know is reachable.

⚠️ **`WGPU_BACKEND` did not work, and the same tier had to fix it to find out.** In wgpu 30
the environment is read only by the `*_from_env` / `with_env` constructors — **never** by
`InstanceDescriptor::default()`, which is what `Instance::default()` uses. So
`WGPU_BACKEND=dx12` was a silent no-op: the instance kept selecting Vulkan while the
operator believed they were testing DX12. The visual now builds its instance as
`InstanceDescriptor::new_without_display_handle().with_env()`, which is byte-equivalent to
the old call with no `WGPU_*` set and makes the variable live. Note this is the **visual's**
instance only; `wgpu_editor.rs` and `editor_probe.rs` still build their own, unchanged.

**Measured on the RTX 5090 workstation, 2026-08-07** (#658 Tier 1, the first time any of
this ran on Windows) — `Instance::default()` chose **Vulkan** unprompted, and ray tracing is
*present*, so Tier 4's "which backend gets RT" fork did not have to be taken:

```
GPU: NVIDIA GeForce RTX 5090 [Vulkan, DiscreteGpu] driver: NVIDIA 610.88
     — granted: ray-query, timestamp, timestamp-in-encoder, adapter-formats
GPU: adapter also advertises: coop-matrix yes, shader-f16 yes
```

`timestamp` + `timestamp-in-encoder` together mean the perf strip gets **real GPU ms** here,
not the CPU fallback. This is a startup *negotiation* result: it says the features were
granted and the device came up, **not** that any `rt_*` stage has been looked at on Windows.

**Tier 1 — RT shadows** (`Shared.rt[2..5]`, all captured Looks): the first consumer
of the TLAS. See render-pass step 1b above for the mechanics; the toggles are
`rt_shadows` (implies the TLAS build), `rt_shadow_soft` (light angular size),
`rt_shadow_strength`, and `rt_shadow_fill` (a second ray for the fill light). RT
supersedes the PCF map at the same seam (`shadow_factor`); the map pass still runs
when Cast Shadows is on (the #182 fluid "receive" coupling reads it).

**Tier 2 — RT reflections** (`Shared.rt2[8]`, all captured Looks — `rt_reflect.rs` +
`rt_reflect.wgsl`): trace `reflect(v, n)` per pixel (off the prepass depth, GGX-ish
cone-jittered by roughness, TAA resolves) for the CLOSEST hit against the TLAS, and
shade the hit **from its instance's own geometry** — the local-space trick: invert the
instance's affine transform (storage-bound copies of the live instance/tint buffers;
the TLAS custom index points into them); for the unit cube the local hit position is
both the RGB-cube colour (`local + 0.5`) and the face normal (dominant axis), for the
cylinder the normal is radial and the tint carries the colour. PBR-lite at the hit:
emissive glow + key/fill diffuse (with an optional traced key-shadow ray — reflections
contain shadows) + a flat ambient standing in for the IBL irradiance (documented
approximation). Output goes into the **same confidence-weighted buffer SSR writes**
(`Post::ensure_ssr_target`), so `composite.wgsl` is untouched, a miss falls back to
the env reflection with no seam — and unlike SSR there is **no screen-edge fade**:
off-screen and behind-camera geometry reflects, which is the point. Supersedes the
SSR march while on (one reflection source at a time); dials: intensity, max-roughness
cutoff (SSR's), reach (× scene diagonal), hit-shadows. Shares the shadows' health +
sway gates in the visual. The experimental wgpu RT API is contained entirely inside
the `rt_*` modules.

**Tier 3 — RT ambient occlusion** (`Shared.rt2[5..6]`, captured Looks — `rt_ao.rs` +
`rt_ao.wgsl`): the AO card's **source switch** (GTAO / Ray Traced) — see render-pass
step 0b. Both sources fill the same raw-AO target, so the blur, the composite
AO-multiply, and the Lagarde specular occlusion stay source-agnostic; `rt_ao_rays`
(1–4) is the per-pixel budget.

**Tier 4 — RT diffuse GI, one bounce** (`Shared.rt3[8]`, captured Looks — `rt_gi.rs` +
`rt_gi.wgsl`, Option B): a per-pixel **cosine-hemisphere gather** off the prepass depth
— each of `rt_gi_rays` (1–4) rays' CLOSEST hit is shaded from its instance (the same
local-space albedo/normal trick as reflections) as the neighbour's outgoing radiance
(`glow + 0.3·key`, the VXGI node estimate, with an optional traced key-shadow ray so the
bounced light is itself shadowed); the cosine-weighted average is one bounce of real
inter-cube colour bleed, **off-screen emitters included** (SSGI can't). It writes exposed
indirect radiance into the SAME buffer SSGI fills (`Post::ensure_ssgi_target`), so
`composite.wgsl` adds it unchanged and a miss (→ 0) leaves the scene's own IBL ambient —
no seam; it **supersedes the SSGI march** while on (one GI source at a time). Dials:
intensity, rays, reach (× scene diagonal), hit-shadows. Shares the other RT effects'
query-health + sway gates. The experimental wgpu RT API is contained entirely inside the
`rt_*` modules.

**Sampling / denoising** (#200 Tier 4½, part 1): all four RT passes' stochastic
directions (shadow/reflection cones, AO/GI hemispheres) are rotated by a **texture-free
spatiotemporal blue noise** (`stbn2` — Interleaved-Gradient-Noise spatial dither ×
golden-ratio temporal advance on a 64-frame cycle), replacing the old white-noise hash:
same mean, but the error is spread into a high-frequency pattern the eye, TAA, and the
bilateral filters resolve far better. The **AO blur** (`ssao.wgsl::fs_blur`, shared by
GTAO + RT AO) is already a depth-weighted **joint bilateral** (down-weights taps across a
silhouette). **Part 2 (spatial denoise)** adds an edge-aware **à-trous** filter (`rt_denoise.wgsl`,
`Post::denoise`) over the RT reflection + GI buffers, in place: a 3×3 B3-spline kernel
at two à-trous steps, edge-stopped by world-position distance (silhouettes) + relative
luminance (highlights), ping-ponged through a scratch buffer back into the source so the
composite is unchanged. Reflections are filtered **roughness-adaptively** (sharp mirrors
untouched, since the global material has no per-pixel roughness); GI at full strength.
`Shared.rt3[5..6]` (toggle + amount), captured Looks; off = raw jitter.

**Temporal accumulator** (#200 Tier 4½, part 3 — `Shared.rt4[8]` = `[enable, feedback,
beat_relax, _×5]`, captured Looks — `rt_temporal.wgsl` + `Post::temporal`): the temporal
half of SVGF. When on, the RT reflection/GI pass writes a
shared **raw** buffer (`Post::ensure_rt_raw`) instead of the SSR/SSGI view; the
accumulator reprojects the previous accumulated **history** by camera motion (current
world pos → previous clip via `inv_view_proj`/`prev_view_proj`), **3×3 neighborhood-AABB
clamps** it to reject stale history where geometry moved, **beat-relaxes** the history
weight (the visual folds the live PLL beat envelope into `beat_relax_factor`, so a strong
kick drops history toward 0 and it doesn't smear across the fast auto-orbit camera), then
**MRT**s the exponential-moving-average result into the SSR/SSGI view (composite reads it)
*and* the new history in one pass — no read/write alias, no copy. Two history textures per
effect ping-pong by parity; a per-effect validity flag seeds the first frame (feedback → 0
= passthrough). Off by default → the RT passes write the SSR/SSGI view directly
(byte-identical). Dials: feedback (history weight 0–0.98), beat relax (0–1). Composes with
part 2's spatial filter — spatial cleans within a frame, temporal integrates across
frames.

**Variance-guided SVGF** (#200 Tier 4½, part 4 — `Shared.rt4[3..5]`, captured Looks):
when the "variance" toggle is on, the temporal pass completes SVGF-lite into true SVGF via
a **3rd MRT output** — a per-effect ping-pong **moments** texture (μ₁, μ₂, accumulated
sample count `n`, σ²) lockstep with the colour history. (1) **History-length-adaptive
blend**: `hw = min(feedback, 1 − 1/n)` (capped at `max_accum`), so a fresh/disoccluded
pixel converges fast then settles toward the feedback ceiling (beat relaxation still
multiplies on top). (2) **Luminance-variance clamp**: temporal variance σ² = μ₂ − μ₁²
(blended with the 3×3 spatial variance while history is short) clamps history luma to
μ ± γ·σ (γ = clamp width) instead of the raw min/max box — a single firefly no longer
swells the box; colour is rescaled to preserve chroma + the reflection confidence alpha,
with the old box kept as an outer safety bound. Luma is capped at 64 before squaring so μ₂
fits the 16-bit-float moments texture. `variance = 0` reproduces part 3 exactly.

**Neural denoiser** (#200 Tier 5a — `Shared.ndenoise[8]` = `[enable, net_strength, seed,
omega, _×4]`, captured Looks — `rt_ndenoise.wgsl` + `Post::neural_denoise`): the neural rung
on top of the classical à-trous. A **kernel-predicting filter** (KPCN-lite): the classical
bilateral tap weight (B3-spline × position × luminance edge-stops — identical to
`rt_denoise.wgsl`) is the **base**, and a tiny **seeded MLP** (the Tier 0 network,
regenerated inline from the seed, bit-identical to `math::mlp_eval`) predicts a **bounded
per-tap multiplicative modulation** of it from local edge features `[Δpos, Δluma, tap
radius, local luma variance]`. The modulation is `exp` of a `±2`-clamped argument, so an
untrained/procedural weight set can never drive a tap negative or to infinity; at
`net = 0` the modulation is identically 1 → the pass reproduces the classical à-trous
**byte-for-byte** ("off = classical"). Same two-step ping-pong through the shared
`denoise_scratch`, same premultiplied operator + per-step blend as the classical pass; it
just swaps the pipeline inside the existing RT-denoise step (`rt_ndenoise` present →
`Post::neural_denoise`, else `Post::denoise`). Requires the RT-denoise toggle on (it reuses
that step's blend amount). CPU mirror `math::nd_tap_weight`/`nd_modulation` +
golden tests (net-0 identity, boundedness, base-linearity). Runs on any GPU (plain WGSL);
the island/MetalFX denoising path is a later accelerator of the same math.

**Neural Radiance Cache** (#200 Tier 6, **foundation / ships dark** — `math::RadianceCache`,
no Shared/IPC/render change): the endgame's substrate. Every neural tier so far ran the MLP as
*inference only* with procedurally-seeded weights; Tier 6's essence is a network **trained
online during rendering** — no external dataset, no checkpoint. A small MLP maps `(pos, dir)`
→ outgoing radiance; the path tracer traces a few extra-bounce "training paths" per frame whose
radiance estimates are the targets, runs a few gradient steps, and terminates the rest of the
frame's paths early into a **cache query** (evaluate the MLP) instead of tracing on — infinite-
bounce GI at short-path cost. The genuinely-new, offline-verifiable piece is the **trainable
network**: a dedicated `NRC_IN=5 → NRC_H=16 → NRC_OUT=3` SIREN with `nrc_forward` (cached
activations), hand-derived `nrc_backward` (½‖·‖² MSE → gradient; `d sin = cos`, the first layer
carrying the extra `ω` chain factor), SIREN-init `nrc_init`, an octahedral direction encode
(`oct_encode`), and `RadianceCache::{new, encode, query, train_step}` (one online SGD step with
gradient clamping — the standard high-variance-sample stability guard). Verified by a
**finite-difference gradient check** (the gold standard — a wrong chain-rule term would only
surface as slow/diverging training otherwise, invisible without a GPU) + a convergence test
(learns a smooth radiance function to ¼ the untrained loss). This is the CPU reference; the
**on-Mac follow-up** runs training on the Metal island (tensor ops / MPSGraph) and the WGSL
query inside `rt_pathtrace.wgsl`'s early termination — explicitly a spike (research-grade,
allowed to be re-scoped), gating on whether the cache's few-frame lag survives the beat-driven
camera.

**Learned upscaler** (#200 Tier 5c — `Shared.upscale[8]` = `[enable, sharpen, seed, _×5]`,
captured Looks — in `composite.wgsl`, no new pass): the composite's bilinear DRS upscale (the
`hdr_tex` sampler fetch when `render_scale < 1`) becomes an **HDR-safe content-adaptive
sharpen reconstruction** (CAS-style). `upsample_scene` takes the bilinear centre + a 4-tap
cross at **source-texel** spacing (the low-res grid the upscale blurred), forms an unsharp
`detail = centre − ring/4`, and adds it back scaled by `sharpen × flat-region-adaptivity ×
MLP-gain`, where the gain rides the **Tier 0 seeded MLP** (regenerated inline from `seed`,
bit-identical to `math::mlp_eval`) keyed on the local luma contrast. The gain is `exp` of a
`±1`-clamped argument (an untrained net can't invert/blow up the sharpen) and a `smoothstep`
contrast dead zone leaves flat/noisy regions untouched (no grain boost). Alpha (coverage) is
the bilinear value — sharpening it would fringe silhouettes. It repurposes the three tail
scalar slots of `CompU` (`up_mode`/`up_sharpen`/`up_seed`; struct stays 64 bytes). The visual
sets `up_mode = 1` only when enabled **and** actually upscaling (`render_scale < 0.999`);
`up_mode = 0` (or full scale) returns the exact bilinear sample **byte-identically**. CPU
mirror `math::upscale_gain`/`upscale_adapt`/`upscale_sharpen_amount` + golden tests
(flat-region zero, gain boundedness, net-0 = pure CAS). Any GPU (plain WGSL); **MetalFX
temporal upscaling** via the Metal island is the higher-quality follow-up.

### Lighting / IBL (`env.rs`, `ibl.wgsl`, `skybox.wgsl`)

Metallic-roughness PBR lit by an environment map via the **split-sum** method:
irradiance map + prefiltered specular mips + BRDF LUT, all equirect `Rgba16Float`,
built on load by render-to-texture passes. The Standard branch adds
**multiple-scattering energy compensation** (#174 T3, Fdez-Agüera — reuses the LUT
values, so rough metals no longer darken), and Chrome shades through the same
split-sum LUT instead of a bare Fresnel. **Plus two analytic Cook–Torrance lights**
(key + fill) for live specular highlights, **plus (opt-in) emissive-cubes-as-lights**
(#167 Tier 3, `Shared.manylight[4]`): the visual picks the brightest N nodes and uploads
them as point lights in **group 3 binding 1** (alongside the GI probes — cube-pipeline-only,
so no new bind group); `cube.wgsl::many_lights` loops them with the same `direct_light`
Cook–Torrance so a glowing cube casts a real glint + coloured pool on its neighbours.
count 0 → off (byte-identical). Selection has positional hysteresis + a ~10-frame fade
envelope (`gi.rs::ActiveLight`, #174 T1 — the raw per-frame top-N re-sort popped on
animated fields), light colour scales with the `glow + 0.3·key` radiance estimate, and
the shader falloff is windowed to reach exactly zero at 4×radius (with an early
`continue`, so out-of-range lights skip the BRDF). **ReSTIR many-lights** (#200 Tier 5d,
`Shared.restir[4]` = `[enable, _×3]`): when on, the light set is chosen by **weighted
reservoir sampling without replacement** (Efraimidis–Spirakis keys `u^(1/lum)` over ALL
nodes, `u` varying per frame by `gi.rs::light_frame`) instead of the hard brightest-`count`
sort — so every glowing cube gets a **luminance-proportional chance** and dim/distant/off-
screen emitters rotate into the set over time (the fade envelope + TAA integrate the
rotation); a 50k-node field lit by all its cubes, not just the top `count`. The bright cubes
key ≈1 almost always (a stable core), so it degrades toward brightest-N as `count → node
count`. `enable = 0` = brightest-N, byte-identical. Pure primitive `math::es_key`/
`restir_rand`/`Reservoir` (+ the RIS contribution weight) is unit-tested (WRS ∝ weight, RIS
unbiasedness). Per-light **RT visibility** (shadowed cube-lights via the TLAS) + per-pixel
spatiotemporal reservoir reuse are the documented follow-ups (the "RT half"). **With a glyph
ring live (organon#217 T10) the node set the renderer ranks is not the tiles.** `world.rs`'s
`glyph_light_candidates` lowers the grid's *emission* into `Surface.meta_nodes` instead: a lit
tile is a candidate on its front face carrying `emit.rgb * emit.w` (linear, SDR-white units —
the value the shader adds to `emissive`), adjacent lit tiles in one row fold into ONE
candidate per run of ≤4 at the luminance-weighted centroid with the summed radiance, and the
world pre-trims to `count` by the same linear Rec. 709 luminance `update_lights` ranks by
(ReSTIR gets the whole pool). Without it the tiles arrived coloured by tint or by position in
the bounds, so the "brightest" cells were the grid's corner. `manylight[2]` is read in
**column widths** while a ring is live — the world converts against the same `gi_min/gi_max`
diagonal this pass multiplies back — and is the scene fraction otherwise. ⚠️ The
`glow + 0.3·key` radiance estimate still scales these colours, and for a glyph light it is
one factor too many (the colour is already radiance, not albedo); until this pass skips the
scale when emission is live, the preset's `ml_intensity` absorbs it. `EnvSource` is `Procedural(DEFAULT_SKY)` (the
always-on fallback), a loaded `.hdr` (capped 4096×2048), or **`Atmosphere(AtmosphereParams)`
— a physically based single-scattering sky (#100)** baked into the env equirect (the
`fs_atmosphere`/`atmosphere()` Nishita pass in `ibl.wgsl`) then run through the same
split-sum precompute, so the geometry is lit by the real sky at the real sun angle. The
visual selects the source each frame by priority **Hdr > Atmosphere > Procedural** (an
`EnvReq` value; re-baked only on change — the atmosphere signature quantizes the sun
direction so a running day cycle re-bakes every ~degree). The atmosphere is **on by
default** (the default environment); an explicitly loaded `.hdr` overrides it. An `env_tint` uniform tints the
IBL + skybox; `terrain.wgsl` carries its own copy of `atmosphere()` for the terrain-on sky
+ aerial perspective.

### Pipeline specialisation (#618 Tier 3, `cube.wgsl` + `render.rs`)

**The scene shader is compiled per configuration, not once.** `cube.wgsl` declares a
pipeline-overridable constant and `render.rs` supplies it through
`PipelineCompilationOptions::constants`, so a feature that is off is *absent from the
compiled shader* rather than multiplied by zero inside it.

```wgsl
override material_maps: bool = true;   // cube.wgsl
```

```rust
fn cube_specialisation(material_maps: bool) -> [(&'static str, f64); 1]   // render.rs
```

**Why this exists.** `u.mtl.x` already gated the #472 material maps at *runtime*, which
kept the picture correct and cost the work anyway: five `textureSample`s, the triplanar
UV resolve and `mat_perturb_normal`'s derivative cotangent frame ran for every fragment
of every draw and were then blended out. The five fetches are the cheap part (the
neutral maps are 1×1 and cache-resident). The price is **occupancy** — a GPU allocates
registers for the worst case across the whole shader, so live state you never use caps
how many wavefronts are in flight, which is what hides memory latency.

**Two facts that make this legal, both worth keeping.**

1. **A pipeline constant is uniform control flow**, confirmed against naga. That is what
   lets the derivative call sit inside `if (material_maps) { … }`. A runtime `if` could
   not hold it — which is precisely why that block was written branchless in the first
   place, and why the override is not just a tidier version of the old gate.
2. **The WGSL default is `true`, deliberately.** The unspecialised module is exactly
   today's shader, so `tests/wgsl.rs` validates the file as written, and a pipeline that
   forgets to set the constant gets the correct-but-slower path instead of one that
   silently ignores its materials. It fails toward correctness, not toward speed.

**When variants are built.** `Renderer::material_maps` records which variant the cube
pipelines currently carry. `sync_material_specialisation` rebuilds the five affected
pipelines — `pipeline`, `pipeline_opaque`, `pipeline_skin`, `prepass_pipeline`,
`opaque_prepass_pipeline` — when `MaterialTextures::present_mask` crosses 0 ↔ non-zero,
and is called from **both** material entry points (`load_material` for a PNG folder,
`bake_material` for the #472 T2/T3 procedural path, which moves the mask too). Loading
or clearing a material is a user action, so the rebuild happens at most once per action
and never in steady state. This mirrors `set_sample_count`, which rebuilds the same set
for the same reason: a pipeline bakes the choice at creation.

The depth prepass takes the constant as well — `vs_depth` calls `mat_displace_world`.

> ⚠️ **The correctness claim is "pixel-identical while no map is loaded", and it is
> asserted here, not proven.** The `false` variant removes a block whose every output is
> multiplied by zero in that state, and `m_albedo`/`has_alb` default to the identity for
> the base-colour resolve (`mix(in.color, m_albedo.rgb, 0.0)` is `in.color` exactly), so
> it is a compile-time removal of dead work rather than a second look. Offline
> validation cannot check pixels. **`verify.sh` is where this gets tested** — and the
> gain itself is unmeasured: occupancy improves in steps as register pressure crosses
> allocation boundaries, so the win is somewhere between nothing and substantial
> depending on where this shader sits relative to the next cliff. Read `gpu_ms` in the
> performance status bar, same scene and resolution, before and after.

**The follow-on is the point.** The material block was chosen as the beachhead for its
clean boundary, not because it is the largest cost. The same lever now applies to
everything else in `fs_main` that runs unconditionally — glitter, diffraction,
retroreflection, clearcoat, sheen, anisotropy, thin-film, fluorescence, blackbody
incandescence, the calibrated LUT — which together are a far bigger share of its 650
lines.

### Shaders (`tests/wgsl.rs` naga-validates them offline)

`cube.wgsl`, `skybox.wgsl`, `ibl.wgsl`, `composite.wgsl`, `post.wgsl`, `ssao.wgsl`,
`ssr.wgsl`, `ssgi.wgsl` (#152 T2), `temporal.wgsl` (#152 T2),
`metaball.wgsl`, `voxel.wgsl` (DDA raymarch — now **physically shaded**: the flat
voxel face + splatted albedo feed the full metallic-roughness PBR + IBL + Material
card ported from `cube.wgsl`, voxel AO folded into the indirect term + soft-shadow
into the key light; a second `fs_ray_depth` entry marches depth-only into the
screen-space-FX prepass so **SSR + SSGI** gather off the voxel faces — the neural-field
pattern; hardware RT stays out, the DDA grid has no BLAS triangles)`/`voxelize.wgsl`/`voxgi.wgsl`,
`mandelbulb.wgsl`, `creature.wgsl`, `creature_overlay.wgsl`, `minimal.wgsl`, `lens.wgsl`, `kifs.wgsl`, `terrain.wgsl`, `stars.wgsl`,
`particles.wgsl` (the Particle Aura sparks, the #298 shaded **bead** impostors, and the
**capsule** impostors — Skin-Arms segments and the plexus Tier 2 nodes/edges, one billboard
per instance, sphere-traced in the fragment against `sd_capsule`, depth-written, with
`fs_capsule_depth` joining the FX prepass so the screen-space FX see the same surface.
⚠️ Impostors are analytic, not triangles: no BLAS, so hardware RT never sees them.
**PBR text T6 (#217), the coaxial glass capsule** — a Glass/Refractive capsule can show
an **emissive core through its shell** instead of the refracted environment:
`DrawU.capsule.x` is the core fraction (inner radius ÷ outer; **0 = off, and
`fs_capsule` then calls `shade_bead` exactly as before — pixel-identical**),
`DrawU.capsule.y` the Beer–Lambert density per outer radius. With the core on the view
ray is refracted at the outer hit, the inner hit and the outer exit are solved
**analytically** (`capsule_interval` — a capsule is a convex union of a finite cylinder
and two spheres, so its ray interval is min-of-entries/max-of-exits; no extra march),
the transmitted term is the instance emission (hit) or the refracted environment (miss)
attenuated in the instance colour with optical depth clamped at 6 so a near-black tint
reads dark rather than zero (`capsule_transmittance`), and `shade_capsule_glass`
Fresnel-composes it with today's environment reflection. `capsule_trace` — and so the
depth written — is unchanged either way. Air→glass entry cannot TIR (η ≤ 1); the
zero-vector guard is defensive. Knob: `ParticleSystem::set_capsule_core`, seeded by
`ORGANON_CAPSULE_CORE="<frac>[,<density>]"` until T3 wires a control; no param-chain
entry. CPU twin + tests: `particles.rs::capsule_core`),
`splat.wgsl` (Gaussian Splatting surface — `vs_splat` billboard + `fs_splat_add` additive/unlit and
`fs_splat_lit` IBL-lit 2DGS anisotropic Gaussians),
`fluid.wgsl`, `fluidvis.wgsl` (#182 dye blit + ink march + bilateral upsample),
`liquid.wgsl` (#182 T3a MLS-MPM P2G/grid/G2P + density splat),
`fluidlight.wgsl` (#182 T4 light-space dye transmittance + liquid caustics),
`sway.wgsl` (#182 T4 fluid→node sway springs),
`liquidsurf.wgsl` (#182 T3b refractive see-through water),
`rd.wgsl`, `field.wgsl`, `capture.wgsl` (#135 letterbox blit),
`overlay.wgsl` (#135 P2 overlay text/quad pass), `axes.wgsl` (#135 P5 line pass),
`fx.wgsl` (#152 post-composite creative FX),
`rt_debug.wgsl` (#195 Tier 0 ray-query debug view — `enable wgpu_ray_query;`, validated
offline like the rest since the naga validator runs `Capabilities::all()`),
`rt_reflect.wgsl` (#195 Tier 2 traced reflections into the SSR composite slot),
`rt_ao.wgsl` (#195 Tier 3 traced hemisphere AO into GTAO's raw target),
`rt_gi.wgsl` (#195 Tier 4 one-bounce diffuse GI gather into the SSGI buffer),
`rt_pathtrace.wgsl` (#200 Tier 4 progressive path tracer — camera rays + N diffuse bounces
vs the TLAS + NEE + emissive + sky, MRT into the accumulation + HDR scene buffer; #258 Tier 2
adds an opt-in **dielectric BTDF** (`Shared.ptglass` enable): Glass/Refractive shade as a
stochastic two-interface dielectric — exact-Fresnel reflect/transmit split, `refract` on entry
AND exit, TIR, Beer–Lambert body absorption over the traversed segment — and Chrome as a perfect
mirror; enable off → diffuse-only, byte-identical; organon#217 T5 — the accumulation
restart and the raster → path-trace handover for a glyph ring's dwell are decided in
`world.rs`, see "Converge on hold" under the per-instance emission section),
`rt_shadow.wgsl` (#195 Tier 1 RT shadow mask — per-pixel any-hit rays at the key/fill),
`rt_denoise.wgsl` (#200 Tier 4½ p2 edge-aware à-trous over the RT reflection/GI buffers),
`rt_temporal.wgsl` (#200 Tier 4½ p3/p4 beat-aware temporal accumulator for the RT
reflection/GI buffers — reproject + neighborhood clamp + beat relax + variance-guided
SVGF, MRT),
`rt_ndenoise.wgsl` (#200 Tier 5a neural denoiser — the à-trous bilateral base × a bounded
seeded-MLP kernel modulation; `net = 0` ≡ `rt_denoise.wgsl`),
`mlp.wgsl` (#200 Tier 0 neural shading foundation — an include-able tiny SIREN MLP whose
weights regenerate from an integer seed; no entry point of its own, later neural tiers
paste it in; `tests/wgsl.rs` validates it via a wrapper fragment shell),
`neural.wgsl` (#200 Tier 1 neural-field generator — the MLP isosurface raymarch + the FULL
Material-card shade: it reads cube's material tail from the shared group-0 buffer — a byte
prefix extension — and branches Standard / Chrome / Glass / Refractive / Anisotropic like
`cube.wgsl`, with a world-up-derived tangent standing in for the per-vertex brush and the
bound-sphere chord as the Refractive thickness proxy. A second `fs_ray_depth` entry marches
depth-only into the screen-space-FX prepass so **SSR + SSGI** gather off the neural surface
— hardware RT stays out, the isosurface has no BLAS triangles. Loaded/validated concatenated
with `mlp.wgsl`).
(`metaball.wgsl` also carries the #152 `fs_volume` emissive-volume entry.)

### The legibility harness (`legibility.rs`, PBR text T2 — organon#217)

`doc/pbr_text_engine.md` §9 states the two laws that let a glyph-grid preset go as far as
it likes — *the cell's energy stays in the cell*, and *the cell's apparent brightness tracks
the effect's value* — and says both are measurable. `native/organon-render/src/legibility.rs`
is that measurement, and it is the one module in this crate with **no wgpu in it**: pure,
deterministic CPU code, which is what lets §9's claim — real automated visual regression
rather than the usual `cargo test` ceiling — be true from the first commit rather than
after the first GPU render.

**The pieces.** A `Fixture` is the source of truth: a cell grid with a symbol and an sRGB
foreground per cell, parsed from a hand-readable text file whose rows are `|`-fenced
(`organon-render/tests/fixtures/omarchy-logo.txt` reproduces §3's census — 337 `█`, 32 `▀`,
32 `▄` — on the padded 81×10 grid; `asymmetric.txt` is a small "L" with a colour gradient,
because the logo is too symmetric to notice a flip). An `Image` is the render under test in
linear light, whatever it arrived as (`from_rgba8_srgb` decodes per pixel *before* anything
is averaged; `from_rgba_f32` / `from_rgba16f` take the HDR buffer). A `GridGeom` says where
cell `(0, 0)` is and how big a cell is, the 2:1 aspect carried from the fixture rather than
assumed; row 0 is the top of the picture, which is a wgpu readback's row 0, so nothing is
flipped. `downsample` box-filters the image to the grid, area-weighted at fractional pixel
boundaries, luma per Rec. 709. `assess` turns that into a `Report`:

| number | what it is | law |
|---|---|---|
| `correlation` | Pearson between measured and expected luma over **every cell, blanks included** | 2 |
| `correlation_lit` | the same over lit cells only — did the gradient's *shape* survive; `None` when every lit cell expects the same luma | 2 |
| `bleed_max` | for each blank cell with a lit 8-neighbour, its luma over the mean of those neighbours; the max | 1, local |
| `stray_fraction` | energy in blank cells ÷ energy in the grid | 1, global |

Expected luma is `luma709(srgb_to_linear(fg))` **times the glyph's coverage** — `▀` is half
a cell and a renderer drawing a half-height tile emits half the light, so a perfect render of
the logo (64 half blocks) could not otherwise score 1. Pass/fail is against a `Thresholds`
that is a **parameter**; the defaults (`0.90 / 0.25 / 0.10`) are what the self-test brackets
and are a starting point for T3, where they belong beside the gate's goldens in
`native/verify/`, never in the param chain.

**Verified without a GPU.** `synth` is a CPU painter — flat rectangles at the cell aspect,
on black — with four controllable degradations, each mapped to a law: **blur** of σ cells
(bleed), **scramble** (the value channel, same energy budget), **noise** (a little of both),
and **gain** (§4's phosphor at 6× paper white, which must move *nothing* — Pearson and both
law-1 numbers are ratios). `tests/legibility.rs` runs the chain against known answers, and
every invariant was mutation-tested: flip the downsample's rows and the upside-down
asymmetric render scores `corr 1.0000 · PASS` and the test fails saying so; drop the sRGB
decode and the byte-path scores move; force the aspect square and the fit disagrees with the
painter; drop Pearson's centring and the affine-fog test fails. The calibration the sweep
prints, on the logo at 6 px cells:

```text
σ (cells)  bleed_max  stray    corr     corr_lit
    0.05     0.0338   0.0053   0.9999   0.9988  pass
    0.10     0.1047   0.0264   0.9988   0.9898  pass
    0.20     0.2291   0.0618   0.9934   0.9457  pass
    0.25     0.2888   0.0781   0.9891   0.9124  FAIL   ← max_bleed 0.25 ≈ σ 0.21 cells
    0.50     0.5871   0.1576   0.9508   0.6940  FAIL
    1.00     0.9376   0.2879   0.8464   0.4343  FAIL   ← min_correlation 0.90 trips only here
```

⚠️ **Three things the numbers taught, each of which a reader of §9 would not expect.**
Pearson is invariant to an *affine* map, not just a gain — so a uniform fog over the whole
frame (every dark pixel raised by the same amount, inside half blocks too) scores **exactly
1.0** on correlation and is caught only by `stray`/`bleed`; `pass()` needs all three for that
reason. A gamma-wrong render — emission taken as `fg/255` instead of decoded — still clears
the 0.90 correlation default (0.9145 on a gradient); `correlation_lit` sees it clearly and
has no threshold yet. And an **8-bit readback clips a gain above 1**, which on a gradient
destroys the very shape `correlation_lit` measures (0.178 at 6× through bytes, 1.000 through
`f32`) — so the gate wants the HDR buffer, not the swapchain.

**What it does not do.** `GridGeom` is axis-aligned, so a tilted-camera preset (§11's
`bottled`) needs a front-on gate render or a homography this module does not have; a
one-cell `spill_fraction` exists for the spec's literal phrasing of bleed ("the fraction of a
lit cell's energy outside its footprint"), which a multi-cell image cannot answer because a
pixel does not say which cell lit it; and **no real render has been scored** — the entry
points `assess` and `assess_readback_rgba8` are wired nowhere, on purpose, until T3 decides
where the gate lives.
