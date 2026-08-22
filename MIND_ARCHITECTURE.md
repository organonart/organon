# Organon Mind — Architecture (living state)

> **What this is.** The **living state doc** for **Organon Mind**: what *exists in the
> code right now*, so a fresh session is oriented in one read. It is **not** a target
> spec and not a roadmap — if something here isn't built, it doesn't belong here.
> **Update it in the same change as every Mind PR.**
>
> **How it relates to the other docs:**
> - `doc/organon_prd.md` — the **product definition**, and it is Organon's rather than Mind's:
>   ✏️ **`doc/organon_mind_prd.md` was absorbed into it on 2026-08-21** and is now a stub, because
>   there is one product. **§6.2 there is the Mind layout** — the reverse-engineering frame, the
>   lens catalog and the honesty stance — and FR-1 … FR-32 keep their numbers. Above the issues.
>   ⚠️ This file's own opening claim to be a *separate product* is on **#111**'s move list and is
>   deliberately not rewritten here; the words go first, the code after.
> - `doc/organon_mind_buildplan.md` — the **tactical order** (phases, what's next, the
>   execution protocol in §8).
> - Root `ARCHITECTURE.md` — the durable **shared-engine** architecture (`math.rs`, the
>   `Shared` snapshot, the render pipeline, the two-process model). **Authoritative on
>   everything Organon Mind reuses** — this file points at it, never restates it.
>   §4.1 there owns the **Editions** mechanism.
> - `doc/watching_a_mind_think.md` — the public statement of the honesty stance.
> - `CHANGELOG.md` — per-PR history. `STATUS.md` — the whole repo's weekly handoff.

**Last updated:** 2026-08-04 · Phase **A′**, with **B** begun.

- **Phase A** (the dedicated build) — shipped: #483 T1, the edition shell.
- **Phase A′** (the workstation interface) — the bulk has landed: the workstation layout
  (#520 T1 via PR #521, T2 via #525), the docks (#532 T1), the mindview spine and
  world/window split (#541, via #548/#549), the house style (#542/#551), and the embedded
  viewport (#554 T1 via #557, T4 via #569, on the #572 world hoist). Several are on `main`
  **without a Mac pass** — `STATUS.md` carries that checklist, not this file.
  **#593 is closed: all five tiers built (#601/#602/#610/#614 + Tier 4), and the wgpu editor
  is now Mind's editor by default** — so Mind's viewport is the scene itself rather than a
  mirror of it, with `ORGANON_EDITOR_WGPU=0` as the bring-up fallback. #617 Tier 1 makes it
  two modes (workstation / immersive). See §2.4.
- **Phase B** (scientific honesty) — #522 T1, the real activation tap, landed via
  **PR #528** and is now **confirmed MEASURED by running it** (2026-08-21, RTX 5090 /
  CUDA, a 48-layer Gemma). The per-layer glow is real tapped tensors, not the
  entropy proxy — the #1 honesty gap is closed for `layer_norm` and `mlp_act`.
  `head_summ` stays a labeled proxy (that is #522 Tier 2's flash-attention trade),
  and **Metal is still unrun**. See §3 and §3.1 — §3.1 also corrects the acceptance
  test this file used to give, which the real profile does not satisfy.

> The previous header said "#520 Tier 1 — in review" for a week after PR #521 merged it,
> while the body below moved on without it. Update this line whenever a phase moves —
> a header that disagrees with its own body is worse than no header (organon#590).

---

## 1. What Organon Mind is, structurally

**Its own crate — `native/organon-mind` — plus a build-time edition of the engine.**
Not a fork, and as of #626 Tier 4 not merely a `cfg` flag either.

`organon-mind` holds the activation ring, the Mind UI surface and the model shell
(~3,000 lines), and carries **no nih-plug**: `cargo tree -p organon-mind` is the check.
The `mind_ui`/`mind_viz` egui imports moved off `nih_plug_egui::egui` — a re-export path
rather than an API dependency — which is what let the crate exist without a plugin host.

What is still shared is the *engine*, and that is the point:
`cargo build --release --features mind-edition --bin organon-mind` produces a standalone
app whose front-of-house is the Mind lane, while the algorithm (`organon-core::math`),
every shader, the `Shared` snapshot (`organon-core::ipc`), the visual binary and the
preset store are the same code Organon runs.

> 📌 **This paragraph was rewritten by the change that made the old one false.** It read
> *"a build-time edition of the same `native/` crate … not a fork"* — accurate until Mind
> stopped living inside `native/`. #626 Tier 3's prompt deferred the rewrite to Tier 4
> precisely so it would land with the code, not after it.

Three commitments define the product (PRD §4): structure is read from the file, the
live signal comes from the real forward pass, and every projection is labeled *as* a
projection. Anything that renders a quantity carries — or is on the hook to carry — a
**provenance marker** (measured / derived / proxy / projection).

Deliberately **standalone-only**: no VST3/CLAP export, so **no new plugin class ID**
and none of the host audio-thread constraints. The `.app` packaging is #483 Tier 4.

---

## 2. What exists right now

### 2.1 The edition shell (#483 Tier 1) — *landed*

| Piece | File | What it does |
|---|---|---|
| `Edition` / `EDITION` | `native/organon-core/src/edition.rs` | compile-time `Full` \| `Mind` \| `Console` (the third is **Organon Console**, Console #3 T1 — mutually exclusive features, `compile_error!` if both; Mind's behaviour is untouched, pinned by the same tests). Drives the front-of-house three (product name, IPC namespace, visible `UiTab`s) plus the three the visual grew in #554/#572 — the module doc is the authority on the count |
| IPC namespace fork | `native/organon-core/src/ipc.rs` | all 27 `$TMPDIR` mmap/sidecar paths funnel through `ns_file(suffix)` → `$TMPDIR/<namespace>-<suffix>`; resolved once per process from `$ORGANON_IPC_NS` (sanitized) else the edition's own |
| Tab filter + chrome | `native/organon-mind/src/mind_ui.rs`, `organon-core/src/tabs.rs` (`UiTab::ALL`/`label`), `lib.rs` | `tab_bar` draws only `EDITION.visible_tabs()`; `clamp_tab` re-homes an active-but-hidden tab so the window can't come up blank |
| The Mind tab layout | `lib.rs` (one `fixed_columns` grid) | col 0 Neural Network · col 1 Model/Specimen · col 2 Chat/Agent + Design Space, dashboard below. **Same in both editions** (#520 T1) |
| Auto-point at the specimen | `mind_ui::should_point_at_specimen`, `mind_ui::auto_point_step`, `lib.rs` | on the `model_gen` edge, sets generator = Neural Network + topology = Connectome (loaded), so a loaded `.gguf` actually draws. Once per load; the Mind card says it happened. ⚠️ **#620 — the latch moves only once the params read back as intended.** It used to be set *before* the writes, so the auto-point got exactly one attempt and could never retry: a Mac session on 2026-08-03 saw both writes go missing and the model stay un-pointed, unreproducibly. `set_parameter` **queues** for the audio thread rather than applying inline, so a readback is false for ~1 frame after every issue — `auto_point_step` distinguishes that from failure, re-issuing every `AUTO_POINT_RETRY_EVERY` (30) frames and giving up with a `nih_warn!` at `AUTO_POINT_MAX_FRAMES` (180, ~3 s). **Bounded on purpose**: the reported symptom was the viewport "flashing back and forth forever", which an unbounded retry produces rather than fixes. Root cause never established — a full `unprocessed_param_changes` queue fits, and is recorded in `lib.rs` as a lead, not a diagnosis. ⚠️ **Retrying nearly broke the promise one-shot writes kept for free** — that this never fights a later manual change. During the ~3 s pending window a user sees nothing happen and reaches for the generator; a re-issue would overwrite them. `auto_point_externally_changed` + `PresetUi::mind_auto_view_baseline` detect that and `Abandon`. It is **per-param** on purpose: the two params land *separately*, so "differs from baseline" would flag the ordinary mid-flight state and abandon nearly every real auto-point |
| "No model loaded" cue (#620) | `lib.rs` (the workstation viewport pane) | one translucent line over the scene while `model_gen == 0`: *No model loaded — this is the default scene.* Mind ships no Generator tab (#483 T1), so before a `.gguf` load the viewport shows the **Original** cube field with nothing saying why — which has read as a regression twice. ⚠️ **The #593 close-out made this easier to misread, not harder**: an empty-looking Mind used to be ambiguous between "no model" and "no viewport", and now that the viewport is always present a cube field is the only thing a first-run user sees — indistinguishable from the wgpu editor having failed. Workstation mode only; immersive has no reserved rect to hang it on, and the Model dock is visible there anyway |
| The binary | `native/src/mind_main.rs` | `organon-mind`, `required-features = ["mind-edition"]`. **Still a nih-plug standalone** — #572 route C is what changes that, and until Tier 3 lands it has not |
| The world, as a library (#572) | `native/src/world.rs` | route C's prerequisite: the renderer *and everything that drives it* — `World`, was `bin/visual.rs`'s `App` — hoisted out of the binary so `lib.rs`'s editor can reach it. A binary's `#[path]` modules are unreachable from the library it depends on, which is the whole reason the interface could not previously share the scene's device. Gated on `mind-edition` (measured: ungated it grows the plugin cdylib by 490 KB); compiled twice in a Mind build so there is one source of truth. ⚠️ **#626 T4 halved that**: the renderer is now the `organon-render` crate, so `world::render` — 40 files and 50 shaders, the bulk of what was being compiled twice — compiles **once** and links into both copies. `world.rs` itself is still doubled, and stays that way until #618's `World` decomposition lets it become a crate too. `bin/visual.rs` keeps winit and forwards three calls, because `World` being a library type makes `impl ApplicationHandler` for it an orphan-rule error |
| The workstation docks (#532 T1) | `native/organon-mind/src/mind_shell.rs`, `lib.rs` | a left **Model** dock and a bottom **Live Telemetry** dock, drawn on *every* Mind tab. Sized by `egui_docks`, which enforces "furniture yields before the middle does" |
| Resizable editor window | `native/src/window_macos.rs`, `lib.rs` | #520 T2 — the window is no longer stuck at 1280×860. Two routes, because the two products own their window differently (below) |
| The house style (#542 T1–T2) | `native/src/theme.rs`, `lib.rs` | design tokens (the blue-slate palette from `doc/organon_mind_visual_reference.md` §2), the Inter type ramp, the pure `row_grid` control-row partition, and `theme::paint` — gradient meshes, grain/mottling tiles, bevels, the ambient key (all `epaint`). Both editions draw from it |
| Embedded viewport (#554 T1) | `native/src/frame_ring.rs`, `lib.rs`, `bin/visual.rs` | mirrors the visual's scene into a pane **directly under the tab bar** — always present, **no toggle**: the viewport is native to the window, the instrument simply never had one before. Space is **reserved** with `allocate_exact_size` and the pane runs in a child `Ui` pinned to that rect — 16:9 of available width, capped at 40% of `content_rect` height, clamped 160–420 pt. Both halves are load-bearing: `allocate_ui` reserves only what a child *uses* and `viewport_pane` is paint-only (it reads `ui.max_rect()` and draws through `ui.painter()`, never allocating), so nothing was reserved and the cards drew over it; and since the pane trusts `max_rect`, an inherited rect made the letterbox edition-dependent — tall and overpainting in full Organon, a ~12 pt strip inside Mind's docks. Visual renders one extra offscreen frame per `MIRROR_EVERY` (~15 Hz), reads it back, publishes to the frame ring; editor uploads only *new* frames. `Shared.mindview[3]` survives as the cross-process request (#541 T1's reservation ⇒ **no `LAYOUT_VERSION` movement**), written by **`frame_ring::mirror_requested(editor_open, viewport_drawn)`** — the conjunction of `EguiState::is_open()` (set in `Editor::spawn`, cleared in `EguiEditorHandle::drop`) with a `viewport_on` latch stored at the pane's own draw site. Read it as *"an editor is open **and** this build draws a mirror pane"*, and note the second clause is what makes #593 Tier 4's gate switch the request off for free. ⚠️ **This is #609's fix, and it replaced the opposite behaviour:** `viewport_on` defaulted to `1` and nothing ever stored `0`, so the request went live from the plugin's first audio block whether or not an editor had ever opened — a projector-only Ableton session paid a second complete 640×360 scene render plus a blocking `poll(Wait)` readback at ~15 Hz, forever. Pinned by a four-case unit test, because the claim "the mirror is off unless someone is looking" is exactly the kind nothing checked for a month. ⚠️ **The pane is not edition-gated** — the same `viewport_pane` call draws in the plugin, the standalone *and* Organon Mind (§2.5). The separate visual window is unaffected and stays the projector path. **Still inside the central `ScrollArea`** — true pinning needs the heading/buttons/tab bar lifted out of it, which is its own change |
| Viewport modes (#617 T1) | `native/src/wgpu_editor.rs`, `lib.rs`, `preset.rs`, `ui_layer.rs` | **two shapes for the integrated viewport, switchable live** by `⛶ Immersive` in the button row. **Workstation** (default): the world is drawn into a pane-sized offscreen texture (`World::render_to_texture`), registered with the vendored `egui-wgpu` (`UiLayer::register_scene_texture`) and painted by egui in a rect `editor_ui` reserves under the tab bar — 16:9, capped at 40% of `content_rect`, clamped 160–420 pt, the same geometry the #554 pane used. The scene is a **widget**: it clips, it scrolls with the panel, and nothing bleeds behind the interface's text. **Immersive**: exactly #593 Tier 4 — `render_into` straight to the swapchain, `theme::workspace_frame(true)`, interface floating over it. ⚠️ **The pane needs two texture formats and getting it wrong is silent.** egui's shader calls its sample `tex_gamma` and then linearizes it, so it assumes every texture is sRGB-encoded: the world renders through an `Rgba8UnormSrgb` view (hardware encodes once) and egui samples through a plain `Rgba8Unorm` view (no decode). A single `Rgba8Unorm` texture — which is what `register_native_texture` documents — stores linear bytes, egui linearizes them a second time, and the pane comes out far too dark with nothing erroring: measured against the same scene in immersive mode, sky reading `0.431 0.436 0.336` came back `0.238 0.219 0.120`, and `gamma_from_linear(0.2384) = 0.5256` is exactly the corrected value. The A/B against immersive is the only thing that catches it. State lives on `PresetUi` (`immersive`, `scene_pane_rect`, `scene_texture`) — **not** `Shared`, so no IPC field and no `LAYOUT_VERSION` movement, and **not** the preset system, so recalling a Scene never changes the mode. The mode does not persist across launches (#617 open question 2). ⚠️ **Neither mode has camera control** — see #621 |
| Configurable UI (#551 T1–T2) | `native/src/theme_config.rs` | the theme as runtime state: colour pickers for every token plus grain / gradient / bevel / lighting controls, live, behind the `UI` toggle. Own `ui_theme.json` store — **never** the parameter-preset system. **T2** adds a named `ThemeLibrary` in its own `ui_themes.json` (save/rename/delete/update/duplicate + JSON import/export) and a read-only built-in gallery — Blue Slate, the superseded Warm Instrument (§18), and a deliberately spec-breaking High Contrast for daylight |

#### The workstation docks (#532 Tier 1) — *landed*

Tier 1 builds the workstation **inside the editor window** rather than in a window of its
own. That is not a compromise, it follows from two facts: `mind_main.rs` calls
`nih_export_standalone`, so **nih-plug owns the event loop and the window**, and for
Organon Mind the editor rectangle already *is* the whole window. Two of the five
workstation regions therefore existed as chrome before this change:

| Region | Already was |
|---|---|
| right dock | the presets rail (`SidePanel::right("presets_panel")`, 150 pt) |
| status bar | the perf strip (`PERF_BAR_H`, shown when `state.perf_open`) |
| left dock | **new** — `SidePanel::left("mind_model_dock")` |
| bottom dock | **new** — `TopBottomPanel::bottom("mind_readouts")`, resizable |
| middle | the existing `CentralPanel` + tab content, unchanged |

`mind_shell::egui_docks(w, h, status_h)` sizes the two new docks by running the pure
`layout_workstation` partition and returning only the regions egui does not already own.
A dock that would come out narrower than `DOCK_MIN_USEFUL` (96 pt) is dropped to zero
rather than drawn as an unreadable sliver. All of it is unit-tested with no egui context
(15 tests): the regions tile exactly, docks never claim space the fixed rail owns, and
dock size is monotonic as the window shrinks.

**Why the docks draw on every tab, not just the Mind tab.** Before this, switching to
Look or Motion hid which model was loaded and whether it was streaming. Keeping those two
facts on screen while you work on something else is the whole difference between a tab
set and a workstation. The left dock also answers a question the dashboard cannot: a flat
dashboard means *idle*, not *broken*, and the two are indistinguishable unless something
says so, which is what the dock's `streaming` / `idle` indicator is for.

**Full Organon is untouched.** It keeps the Mind lane as one tab among eight with the
dashboard inline; `mind_observe` was split out of the dashboard body so both editions
pump the ring reader from their own place. The Load button lives in the dock for Mind and
in the Model/Specimen card for Full, so neither product shows it twice.

**What Tier 1 did *not* do — and how it was since resolved.** #532 T1 got you one window for
everything egui already draws, but not the portrait: that is wgpu while the editor is
egui-on-glow, and embedding it needed "the world being drawn" separated from "the window it is
drawn in" inside `bin/visual.rs` (`World::render`, ~7 100 lines).

That split landed as **#541 S2 T3**, and **#554 T1** then embedded the portrait — but *not* the
way #541 T4 assumed. No published `egui-wgpu` pairs with the renderer's wgpu 30 (`egui-wgpu 0.33`
→ wgpu `^27`; 0.34/0.35 → `^29`), and a wgpu-30 handle cannot cross into wgpu-27, so T1 made the
boundary **CPU memory** — the visual reads a frame back and publishes it over `frame_ring`, and
egui draws pixels without caring what produced them.

> ⚠️ **Corrected by #554 T4.** The version constraint is real; the inference drawn from it — that
> sharing a device would require forking `egui-baseview` and *downgrading* the renderer's wgpu —
> was never measured and is false. Porting `egui-wgpu` **up** to wgpu 30 is eight mechanical fixes
> (`vendor/egui-wgpu`, compiled and tested in-tree). That matters because a CPU mirror has two
> ceilings no tuning removes: it is a system-memory round trip taken on the render thread, and it
> **cannot carry HDR** — `egui::ColorImage` is 8-bit sRGB, so the whole EDR path quantises away at
> the copy.
>
> So Organon Mind draws its interface **directly onto the renderer's device**, in the visual's own
> window, after the composite (`native/src/ui_layer.rs`). The scene is not a picture inside the
> UI; the scene *is* the window and the UI floats over it, the way any other 3-D application
> works. `frame_ring` is not obsolete — it remains the **plugin's** path, where the editor does
> not own its window and a GPU device has no business inside Ableton's process. ⚠️ **That is the
> *justification*, not the state of the code:** `ui_layer.rs` draws a HUD panel on the visual's
> window, not the workstation, and the mirror pane in the editor is **un-gated**, so Mind still
> draws a photograph too. #593 Tiers 2–4 are what make "plugin-only" true — see §2.5.

The separate window remains, deliberately: it is the projector path.

Organon Mind shows **`Mind · Look · Motion · Environment · Settings`** in that order
(#520 Tier 1) — it wants most of Organon's functionality *rearranged*, not less of it.
Only the Generator tab genuinely goes (Mind's generator is always a neural network, so
its card leads the Mind tab instead); Synth and Audio stay out. Full Organon shows all
eight tabs in its own order, unchanged. The presets rail is drawn in **both**.

Full Organon's namespace is `organic-math` — **every path byte-identical to before the
fork**, pinned by a unit test. Mind's is `organon-mind`. Every edition-shaped UI decision
is a pure function of `Edition` in `mind_ui.rs`, so both products' behaviour is
unit-tested from a default (feature-off) build.

> **The lesson the tab filter taught, worth keeping.** Hiding a tab hides the *controls*
> on it, and some of those controls were load-bearing for the Mind lane itself — the
> generator/topology selectors, and then the preset rail Mind turned out to want back.
> Whenever Mind drops a surface, ask what stopped working as a result; the answer is a
> fix that belongs in the same change, not a later one.

**The resizable window (#520 Tier 2).** Tier 1 put four cards across three columns; at a
fixed 1280×860 that is unusable on a laptop, which is the concrete reason this tier
exists. The **default size is unchanged** — `EguiState::from_size(1280, 860)` in
`params.rs` — only the ability to grow is new. `fixed_columns` already divides
`available_width`, so the columns widen on their own; nothing in the layout code changed.

Two routes, because the two products own their window differently, and **both are wired
in both editions**:

| | Who owns the frame | Route |
|---|---|---|
| VST3 / CLAP | the **host** | `nih_plug_egui::ResizableWindow` wraps the editor's `CentralPanel` and draws a drag corner. It calls `EguiState::set_requested_size`, which the wrapper turns into a host resize request. `EguiState` is a `PersistentField`, so the chosen size is saved with plugin state |
| Standalone | **us** | baseview opens the `NSWindow` with `Titled \| Closable \| Miniaturizable` and **no `Resizable`**, and exposes no API to change it, so `window_macos::ensure_resizable` ORs `NSWindowStyleMaskResizable` in through objc and sets `contentMinSize`. That also lights up the green **zoom** button |

The drag corner works in the standalone too — baseview's `resize()` sets the `NSWindow`'s
content size, not just the view's, when the window is one it owns. The objc half adds the
*native* frame affordances (edge drag, zoom) that no amount of in-canvas drawing can.

`window_macos` is gated on a flag the two standalone `main()`s set before handing off to
nih-plug (`mark_standalone`). That gate is not decoration: in a plugin, `NSApp`'s windows
belong to the **host**, and reaching into them would be both wrong and hostile. The plugin
build calls the same function and it returns immediately. Off macOS the whole `imp` module
is a no-op, matching `hdr_macos.rs`'s shape.

> **Making the window resize is not the same as making the UI resize** — the lesson this
> tier cost three attempts to learn. A nih-plug standalone nests **three** views:
>
> ```text
>   NSWindow.contentView        the wrapper's baseview view — AppKit resizes this
>     └─ baseview NSView        the editor's view — nothing resizes it
>          └─ NSOpenGLView      what egui actually paints into
> ```
>
> AppKit keeps only the outermost in step; baseview gives the other two a fixed
> `initWithFrame_` and no autoresizing mask, because it never expected a resizable window.
> `sync_editor_view` walks all three each frame, converging rather than latching. Resizing
> the editor view but not the GL view is the failure mode that looks most like success:
> `screen_rect` grows, layout is correct, and the UI still sits in its original rectangle
> because the surface underneath it never grew.
>
> The final signal to baseview must be **deferred to the run loop**. Its handler ends in
> `trigger_event`, which takes the same `window_handler.borrow_mut()` that `trigger_frame`
> holds for the whole of `on_frame` — and the editor closure runs inside `on_frame`. Sent
> inline it is an unconditional `RefCell` double borrow, and because baseview's view methods
> are `extern "C"` the panic cannot unwind: it aborts the process on the first resize.

**The telemetry dock is sized to its contents.** The bottom dock asks for
`mind_shell::DASHBOARD_H`, derived from the widget stack in `lib.rs::mind_dashboard_ui`,
not from the PRD mock's round number. egui clips a panel to its own rect, so a dock shorter
than its dashboard silently *crops* the cards — and because the dock height is absolute,
maximizing the window gave every new pixel to the middle and cropped them identically at
any size. The dashboard is now **always the compact layout** (the expanded one never fit,
which is what its toggle was really working around), and
`the_bottom_dock_fits_the_dashboard_on_a_normal_window` fails the build if the two drift
apart again. The dashboard still sits in a `ScrollArea`, so being wrong here degrades to
scrolling rather than to clipping.

**The one operational gotcha.** The visual and the inference runtime are compiled
*once* (feature-off, so their own `EDITION` is `Full`). Children the editor spawns
inherit the right namespace automatically (`spawn_visual`, `mind_console::start` both
pass `ORGANON_IPC_NS`). A **hand-run** runtime does not — give it the namespace:

```sh
ORGANON_IPC_NS=organon-mind ./organic-math-mind-runtime
```

Mechanism details (the sanitizer, the `OnceLock`, what is *not* edition-dependent):
root `ARCHITECTURE.md` §4.1.

### 2.2 The Mind lane it wraps (#367, pre-existing)

Everything below already worked inside full Organon's Mind tab; the edition is a new
front-of-house over it, not new capability.

| Capability | File | State |
|---|---|---|
| **GGUF header parser** | `native/organon-core/src/gguf.rs` | metadata + tensor directory only — never reads a weight byte. Quant families, per-tensor byte sizes, bits/weight, KV-cache cost |
| **Architecture specimen** (#367 T1) | `math.rs::gguf_architecture_graph` | the model's true wiring — layers/heads/MLP — drawn from the file into a `NeuralGraph`, through the #226 neural-glow path |
| **Activation ring** (#367 T2) | `native/organon-mind/src/mind_ring.rs` | `MindRing`/`MindFrame` mmap, a channel **separate from `Shared`**: per-token `layer_norm`, `mlp_act`, `head_summ[layer][head]`. `MR_MAX_LAYERS`/`MR_MAX_HEADS` = 64 |
| **Synthetic writer** | `bin/mind_writer.rs` | fake per-token frames, zero inference — exercises the whole live path model-free |
| **Embedded runtime** (#367 T2b/T2c) | `bin/mind_runtime.rs` | llama.cpp, `required-features = ["embedded-llm"]`; loads the `.gguf`, runs one completion per `prompt_gen`, streams frames into the ring + text into the reply sidecar. **GPU backend is per-target** (#658 T5): Metal on macOS, **CUDA on Windows**, CPU elsewhere — `llama-cpp-sys-4`'s build.rs turns *every* GGML backend off unless its feature is on, and the feature names differ per vendor, so `Cargo.toml` splits `llama-cpp-4`'s feature list across `[target.'cfg(target_os = …)'.dependencies]` blocks (cargo unions them) and `n_gpu_layers` keys the offload decision on the **same** `target_os` values. ⚠️ The two must be edited together — a third backend (Vulkan, HIP) is a change in both places, and offloading to a backend that was never compiled in silently lands the whole model on the CPU. The runtime now prints which it took (`GPU offload on (all layers)` / `off — CPU-only build`) so a Windows run cannot quietly be a CPU run |
| **In-plugin console** | `native/organon-mind/src/mind_console.rs` | spawns the runtime as a managed child (piped stdio), bounded log ring, command REPL — no separate terminal |
| **Live telemetry** (#482 T1) | `native/organon-mind/src/mind_viz.rs` | editor-side `MindViz` + `paint_*` egui widgets reading the ring directly (peak-hold, auto-gain, per-token effort scroll, tokens/sec) |
| **Mind log** | `native/organon-mind/src/mind_log.rs` | append-only JSONL corpus at `<store>/mind-log/organon-mind.jsonl`, where `<store>` is `dirs::data_dir()/OrganicMath` — the *same* root as presets/keymap/theme/networks. macOS `~/Library/Application Support/OrganicMath`, Windows `%APPDATA%\OrganicMath`, Linux `~/.local/share/OrganicMath` (#658 T1; before it, the path was hand-spelled from `$HOME` and so escaped to `%TEMP%` on Windows) |

### 2.3 The live channels

Two, and they are different on purpose:

- **`Shared`** (`ipc.rs`) — the control-rate snapshot, plugin → visual. The Mind block
  is `Shared.mind[8]` = `[mind_on, model_gen, topo_mode, prompt_gen, temp, ctx, rate,
  fullattn]`, a **runtime-stamped counter block** (not params, not preset-captured).
  `topo_mode` selects what the visual draws for the loaded model.
- **`MindFrame`** (`mind_ring.rs`) — the per-token activation ring, writer → visual.
  Kept off `Shared` so the token-rate stream adds no `LAYOUT_VERSION` churn.

**Both are append-only and offset-sensitive** (root `ARCHITECTURE.md`, invariant #3).

### The three-way `MindFrame` append — *done, and why it was done first*

The Phase-B spine step is **complete**: all three blocks are assigned, and they were
assigned **together, in one sitting, before any of them was implemented**.

| Block | Owner | Fields | Absent when |
|---|---|---|---|
| **A** | #507 T2/T3 — residual trajectory + logit lens | `resid_layers`, `lens_k`, `resid_proj[64·3]`, `lens_id[64·4]`, `lens_prob[64·4]` | `resid_layers == 0` / `lens_k == 0` |
| **B** | #505 T2 — live sparse expert routing | `expert_count`, `expert_used`, `expert_id[64·8]`, `expert_w[64·8]` | `expert_count == 0` (dense) |
| **C** | #409 T2 — SAE feature meaning | `feat_count`, `feat_layer`, `feat_recon_err`, `feat_id[32]`, `feat_act[32]` | `feat_count == 0` (no SAE) |

Frame: **17 264 → 24 464 bytes**. Not `Shared`, so **no `LAYOUT_VERSION` bump** — the
ring is a transient `$TMPDIR` mmap recreated each run.

**Why one sitting, and not "whoever gets there first".** The writer (`mind_runtime` /
`mind_writer`) and the readers (the visual, the editor dashboard) are **separate
binaries** indexing the same mmap by byte offset. If two issues each append on their own
branch, the layouts disagree and **nothing fails**: it compiles, it runs, and it shows
wrong numbers. Assigning all three up front fixes the offsets before the first
implementer can move them, and each block is **zero = absent**, so they can be
implemented in any order without touching each other.

Two guards make a mistake loud instead of silent:

- **`MindFrame` offsets are pinned by test** (`frame_field_offsets_are_pinned`). A field
  *inserted* rather than *appended* fails the build. If it ever fails, the fix is to move
  the field to the tail — never to update the expected numbers.
- **`MindRing.frame_bytes`** (the old spare `_pad`) carries the writer's
  `size_of::<MindFrame>()`, and the reader refuses a ring that disagrees. A stale writer
  beside a fresh reader used to decode every field past the divergence as
  plausible-looking garbage; now it reports **no signal**, which is honest.

**Sparse by design (Block B).** MoE models declare 8 (Mixtral) to 256 (DeepSeek-V3)
experts but fire 2–8 per token. A dense `[layer][expert]` grid would cost 64 KB a frame
to carry ~8 non-zeros per layer, so the frame stores the **fired** experts as (id,
weight) pairs — which is also exactly what #505 T2 renders.

**Labels are not in the frame (Block C).** It carries feature *ids*; the editor resolves
id → name through the feature-label corpus, which is a versioned artifact in its own
right (PRD §1.2/§13), not something to inline per token.

### 2.4 The integrated viewport (#593) — *closed; the scene is the viewport, by default*

> 🧹 This section carried **two `### 2.4` headers** for a day — #610 and #614 each appended
> their own while the other was in flight, and neither noticed the duplicate. Merged back into
> one here. Worth a line because it is the failure mode a living-state doc has: two true
> paragraphs written past each other read as one contradictory section.

**For most of this thread the answer to "what is in Mind's viewport" was: the #554 T1 CPU frame
mirror** — the visual process rendering a *second*, offscreen 640×360 frame, reading it back
over shared memory, and the editor uploading it as a texture at ~15 Hz. A picture of the scene,
taken by another process. A great deal of *enabling* work landed without changing that (#569's
`egui-wgpu` on wgpu 30, the #574/#575/#582 world hoist), which is why the sentence survived so
many PRs.

**Tier 4 ended it.** `wgpu_editor.rs` runs `World::render_into` and `editor_ui` on **one device,
in one window**, and the whole mirror subsystem is now `#[cfg(not(feature = "mind-edition"))]` —
so there is no photograph left to show even if something asked for one.

**The default is flipped, and that is what closes #593.** For all five tiers this also needed
`ORGANON_EDITOR_WGPU=1` — house invariant #6, new capability defaults to inert — and that gate's
own documented exit condition was the Mac pass. It happened **2026-08-03**: one window,
`2560×1720` Retina, sustained **60.0 fps**, `$TMPDIR/organon-mind-frame.bin` never created with a
visual running alongside, both #617 modes exercised. So plain `organon-mind` now opens the editor
that has a viewport, and `ORGANON_EDITOR_WGPU=0` is the way back.

⚠️ **Read the fallback as a bring-up hatch, not a mode.** `=0` opens the `nih_plug_egui` editor,
which since Tier 4 has **no viewport at all** — you keep the docks, telemetry and model loading,
and you lose the specimen. That is also the shape of the gap this flip closed: between Tier 4
landing and the flip, plain `organon-mind` shipped with *no viewport whatsoever* — strictly less
than before Tier 4 — because the mirror had left its path and its replacement was one unset
variable away. Worth recording, because nothing was broken and every doc said "all five tiers
landed": a feature can be complete, reviewed, Mac-verified **and unreachable**, and the only
artifact that said so was one sentence in this file.

**Route C is what got there**: replace `nih_plug_egui`'s baseview+glow editor with our own
`nih_plug::editor::Editor` that builds a **wgpu surface on the parent view nih-plug hands us**,
keeping nih-plug's wrapper as the owner of the params so `ParamSetter` stays real.

| Tier | What it bought | PR |
|---|---|---|
| **0** | the premise, compiled *and run*: parent handle → rwh 0.5 → parented baseview window → rwh 0.6 → `Surface<'static>` → cycling clear | #601 |
| **1** | `editor_ui` — the 4 316-line editor body as a function a second host can call | #602 |
| **2** | the custom `Editor`: `World::render_into` + egui over it, on one device | #610 |
| **3** | the `EguiPlatform` seam — `FrameTarget::ui_scale_factor` replaces `ui_window`; `winit::Window` leaves `world.rs` | #614 |
| **4** | the mirror gated off Mind's path, **and the central region opened so the scene shows** | #615 |
| — | **close-out**: the `=1` gate inverted, so all of the above is what `organon-mind` actually opens | this change |

**Tier 4's first half is the one anyone will actually feel, and it is two lines of code.** Tier 2
drew the world into its surface correctly on the very first run **and nobody could see it**:
`editor_ui`'s `CentralPanel` is opaque, and `viewport_pane` painted the 640×360 photograph over
exactly the region the scene occupied. "Nothing rendered" and "rendered, then covered" look
identical in a screenshot. So Tier 4 gives the central region back —

> ⚠️ **Superseded in part by #617 Tier 1 — read this section as describing *immersive mode*.**
> Tier 4 gave the whole central region to the scene and made the panel transparent, which is the
> **game** pattern: the world is the screen and the interface floats on it. Correct for an
> instrument you are watching; wrong for a tool with docks, rails and a `ScrollArea`. On the Mac
> (2026-08-03) it put the scene behind the heading, the hint line and the tab bar, and — because
> the scene is painted to the *window* while the workstation scrolls — **scrolling slid the
> interface across a stationary scene**.
>
> #617 Tier 1 keeps all of the below as one of **two modes**, and makes the other one the default:
> **workstation**, where the world is rendered into a pane-sized texture and egui paints it in a
> rect reserved under the tab bar, so the scene clips, scrolls and lays out like any other widget.
> `scene_behind` still gates the transparent frame; `PresetUi::immersive` now chooses between the
> two shapes. Nothing here was reverted — the noun was wrong, not the code.

- `EditorCtx::scene_behind` — *has this host already drawn the world into the surface egui is
  about to paint on?* `true` only under `wgpu_editor`; `false` for the plugin,
  `organon-standalone`, and Mind's `nih_plug_egui` editor.
- `theme::workspace_frame(scene_behind)` — `None` (i.e. `CentralPanel::default()`, today's
  opaque faceplate) or a frame with the same geometry and **no fill**.

— and the world then shows through wherever a widget has not painted its own surface. The docks,
rails, cards and wells all paint themselves, so the workstation reads as furniture *over* the
scene rather than as a lid on it.

**Tier 0 is in the tree: `native/src/editor_probe.rs`.** It is the *compiled* form of the
one claim the rest of the thread rests on — parent handle → rwh 0.5 → parented baseview
window → rwh 0.6 → `wgpu::Surface<'static>` → configure → clear to a **cycling** colour
and present. It draws no `World`, no egui, and handles no input; those are Tiers 1–3. It
is gated on `mind-edition` **and** `ORGANON_EDITOR_PROBE=1`, checked at the top of
`lib.rs`'s `editor()`, so Mind behaves exactly as before unless asked.

**Route C is validated for BRING-UP ONLY.** Mac pass 2026-08-01 (M5 Max, macOS 25.4),
`ORGANON_EDITOR_PROBE=1 ./organon-mind --backend dummy` (`--backend dummy` is required —
#579). Split it the way the ledger below splits everything else:

| | State | Evidence |
|---|---|---|
| macOS compile of the AppKit path | **measured** | first try, zero errors, zero warnings from the file. Nothing had compiled it before: `cargo check --target aarch64-apple-darwin` cannot finish in the cloud container, because `coreaudio-sys`'s bindgen needs the macOS SDK headers |
| a frame presents in a parented `NSView` | **measured** | window up at 640×392, surface `Bgra8UnormSrgb` on Apple M5 Max |
| the colour cycles (not a frozen first frame) | **measured** | 10 distinct centre pixels across 10 captures; counter climbing to frame 5640; zero panic/abort/validation lines |
| repeated bring-up and clean teardown | **measured** | three bring-ups, three exits through `ProbeEditorHandle::drop`, exit 0 each |
| **a surface outliving its view across a re-create** | **asserted** | the drop ordering is written, never executed. See below |

**The open question, re-aimed — Tier 2 is where it gets answered.** The original check was
"close and reopen the editor twice", which assumed a *host* driving that cycle. Organon
Mind is **standalone-only and permanently so** (no Mind VST3/CLAP will ever be built — see
§1 and the class-ID invariant), so nothing external ever closes and reopens this editor and
the literal test describes a situation the shipping product cannot reach. The underlying
risk does survive, on paths the standalone genuinely has: a **resize or scale-factor /
display change that rebuilds the surface**, or a Tier 2 editor that rebuilds its window.
**Whoever builds Tier 2 owns proving it** — a real editor exercises that cycle whether or
not anyone tests for it deliberately.

#### Tier 2 — the custom wgpu `Editor` (`src/wgpu_editor.rs`) — *written, compiles, inert*

`WgpuEditor` is a second `nih_plug::editor::Editor` that does what the probe proved possible:
it opens a parented baseview window, builds a `wgpu::Surface` on it **through
`editor_probe`'s handle chain reused verbatim**, negotiates the device the renderer actually
needs, hands it to `World::attach_gpu`, and then each frame

1. draws the scene with `World::render_into` (`FrameTarget { presented: true, ui_window: None }`),
2. draws the interface over it with the vendored `egui-wgpu`, by calling **`lib.rs`'s
   `editor_ui`** — the same function the `nih_plug_egui` editor calls, so the two cannot drift,
3. feeds input through **`baseview_input`** (#599), which this change finally declares as a
   module (its `tests/baseview_input.rs` shim is deleted and `keyboard-types` moves up to
   `[dependencies]`, exactly as that shim's own note specified),
4. and presents — leaving nih-plug's wrapper owning the params, so `ParamSetter` is real.

Two seams were deliberately **not** touched at Tier 2 and both have since been: `ui_window` went
in Tier 3 (`FrameTarget::ui_scale_factor`), `frame_ring`/`Mirror` in Tier 4. No
`Shared`/IPC/`LAYOUT_VERSION` change — held true through all five tiers.

**Gated on `mind-edition`** so the shipping plugin cdylib cannot move — and, for the duration of
the build-out, on `ORGANON_EDITOR_WGPU=1` as well, like the probe. **That second gate is now
inverted**: this is Mind's editor by default, and `=0` opens the `nih_plug_egui` editor, which
after Tier 4 has no viewport pane at all because that pane was the mirror. Only an exact `"0"`
disables, so a typo leaves you in the default rather than silently in the viewport-less editor.

```sh
./organon-mind --backend dummy          # the wgpu editor
ORGANON_EDITOR_WGPU=0 ./organon-mind --backend dummy   # the fallback, no viewport
```

**The §2.4 open question, answered structurally rather than left open.** The risk was a
`Surface` outliving its `NSView` across an in-process re-create. Tier 2's answer is that **it
never gets the chance**: the surface is created once per window and a size or scale change only
ever *reconfigures* it. That policy is a pure function, `surface_action`, with `SurfaceAction`
carrying no re-create variant at all, and three tests pin it — so adding a rebuild path is a
test failure rather than a silent regression.

**It has now been run on real hardware — on Linux/X11, not the Mac.** The Ubuntu dev box has an
RTX 5060 Ti and an X server, so `organon-mind --backend dummy` with the gate set exercises the
**X11 arm** of the same `spawn` → surface → `render_into` → egui → present path. That is the
first time any tier of #593 has been *seen* running, and it is not a substitute for the Mac: the
AppKit arm, VST3 hosting and EDR are all still untouched.

| | State | Evidence |
|---|---|---|
| both editions compile with the new editor | **measured** | `cargo check --all-targets` clean, first try, and **zero warnings from `wgpu_editor.rs` or `baseview_input.rs`** — the #582 detector |
| a resize reconfigures and never re-creates | **measured** (as policy) | `surface_action` + its tests; `SurfaceAction` has no re-create variant |
| a `wgpu::Surface` comes up on the parent view **(X11 arm)** | **measured** | `surface up: 1280×860 Bgra8UnormSrgb on NVIDIA GeForce RTX 5060 Ti` |
| the device is sufficient for the **world's** pipelines | **measured** | `vs_sky` / `vs_terrain` / `vs_star` / `vs_sun` all created *after* bring-up, i.e. lazily inside `render_into`. This is the risk the probe's `Limits::default()` would have hit — window up, pipelines dead |
| `editor_ui` draws, on the renderer's own device | **measured** | screenshot: tab bar, toolbar, Model card, Neural Network / Chat / Agent cards, Live Telemetry — the real interface, not a placeholder |
| the render loop is live, not a frozen redraw | **measured** | `frame 1560 — 66.7 fps over the last 120 frames (1280×860)`, zero wgpu validation lines. **The captures are byte-identical because the idle UI is static**, which is exactly why `FRAME_LOG_EVERY` exists — a screenshot cannot tell a live loop from a wedged one |
| **the scene is visible** | ✅ **Tier 4 — measured, X11 arm** | Tier 2's honest gap, and it is closed. `editor_ui`'s `CentralPanel` was opaque and covered the whole window, and `viewport_pane` drew *"no frames — open the visual window (640×360 mirror)"* right where the scene was; the world rendered underneath and nobody could see it. See the Tier 4 evidence table below |
| input landing (mouse/keyboard through `baseview_input`) | **asserted** | wired, never exercised — nothing has clicked this window |
| `surface.configure` on a live parented `NSView` under a **scale-factor or display change** | **asserted** | never executed anywhere. The residue of the open question, and the one thing the Mac deploy must do on purpose — drag the window between displays of different scale |
| the AppKit arm, VST3 hosting, EDR | **asserted** | Linux run says nothing about any of them |

⚠️ **The gap row was the interesting result, and it was a scope finding rather than a bug.** #593's
"Done means" says *"The workstation UI draws over it"* — but `editor_ui` as it stood was not a UI
that drew *over* anything: its central panel was opaque, and the pane in the middle of it was the
**mirror**, which in a one-process Mind has nothing to show. So Tier 2 genuinely put the scene in
the window, and the interface genuinely drew on the same device — and the two were stacked in the
wrong order for anyone to tell. **Tier 4 resolved it**, exactly where Tier 2 said it belonged
rather than smuggling it in early.

#### Tier 4's own evidence — X11 arm, 2026-08-02

`main` and this branch, **built from the same tree, run back to back, identical environment and
identical warm-up** (wait for the first `FRAME_LOG_EVERY` report, then capture).

| | State | Evidence |
|---|---|---|
| **the scene fills the central region, with the workstation over it** | **measured (X11)** | The pair is the whole finding. `main`: an opaque central region carrying *"no frames — open the visual window (640×360 mirror)"*, with the Neural Network / Model-Specimen / Chat-Agent cards pushed off the bottom of the window. Branch: the world — sky, horizon, environment — filling that same region, with all three cards drawn over it and readable |
| the render loop is still live at full rate | **measured** | branch `frame 480 — 66.7 fps`, `main` `frame 480 — 64.8 fps`. **A screenshot cannot tell a live loop from a wedged one** when the UI is static — that is what `FRAME_LOG_EVERY` is for |
| **no frame-rate cost** | **measured** | A/B, 60 s per run, alternating, two rounds: 2400/2520 then 3840/3840 frames (main/branch). Indistinguishable |
| the mirror is gone from the binary, not merely unreached | **measured** | `strings organon-mind`: `no frames` · `organon_viewport` · `frame.bin` · `waiting for the first frame` each appear **1×** in `main`'s build and **0×** in this one |
| `$TMPDIR/<ns>-frame.bin` never appears | **measured, but it does not discriminate here** | absent on both builds — because the ring is created by the **visual process**, and no visual was running. ⚠️ This is completion-test item 3 and **the Linux run cannot settle it**; it needs the Mac, with a visual up |
| **The `nih_plug_egui` path still comes up** (now reached by `ORGANON_EDITOR_WGPU=0`) | **measured** | window up, opaque central region as before, **no mirror pane** — and the workstation cards sit directly under the tab bar instead of being pushed off the bottom by a 420 pt placeholder. Measured while this was still the *default*; it is now the fallback, and the flip changed which branch is taken, not what either branch does |
| both editions compile and test | **measured** | `cargo test --release`: **1103 / 0** default, **1247 / 0** mind-edition. See §4 |
| input, the AppKit arm, VST3 hosting, EDR | **asserted** | unchanged from Tier 2's row above — a Linux run says nothing about any of them |

⚠️ **One thing this run got wrong first, recorded because it nearly became a reported finding.**
An early control run of `main` logged **zero** frame reports in 105 s at ~10% CPU, which read as a
50× regression on `main` that this branch had somehow fixed. It was not: a controlled A/B, run
back to back under identical conditions, shows the two builds at the same frame rate. The first
measurement was the artifact. *Run the control before you report any non-zero difference* —
`organon-dev`'s rule, earned on #602's title-bar anti-aliasing, and it applies to frame rates too.

📌 **Observed and deliberately not fixed here: the top strip's legibility.** The title, the hint
line and the tab bar sit directly on the scene now, with no surface of their own — every other
piece of chrome in this interface (docks, rails, cards, wells) paints one, because until now the
panel behind them was opaque. Against a bright sky the `weak()` hint line is marginal. The fix is
small and known — reserve a deferred shape before the strip and `painter().set` a
`shell_face` + grain behind it once its rect is measured, the same idiom `card_chrome` already
uses — but it is a **look** decision, this project has a live theme editor for exactly those, and
Tier 4's brief was to open the region rather than to redecorate it.

---

**Tier 3 is in the tree: the input path is no longer winit's.** `native/src/egui_platform.rs`
is the seam — `WindowGeometry` (physical size + scale factor) plus the `EguiPlatform` trait —
and `world::winit_platform` is the winit arm. `ui_layer` is generic over it, so
`FrameTarget::ui_window` is gone and **`world.rs` names no `winit::window::Window` at all**.

The design turns on one asymmetry, and it is worth stating because it is what made this its own
tier rather than a rename: **`baseview::Window` is a handle you act on, not one you ask.** It
reports neither size nor scale — geometry only ever arrives inside `WindowEvent::Resized(
WindowInfo)` — and it is only lent to you inside a callback. So geometry crosses the seam as
*data*, and platform output (cursor, clipboard) crosses *back* as data: a backend that holds its
window acts and returns `()`, one that does not returns its plan and its host applies it where
the window is in scope. That is what `baseview_input::PlatformActions` (#599) already was, which
is why the baseview arm satisfies the trait **unedited**.

⚠️ **Do not mistake this for "input works in the editor".** Nothing baseview-shaped is wired to
anything: Tier 3 built the seam and the winit side of it, and the arm that matters for Mind
arrives with **Tier 2's window**, since the window is what produces the events. What is
verified is the winit host — `organic-math-visual` *is* one, so the visual's whole existing
keymap and pointer routing run through this abstraction for real. What is **not** verified is
that a baseview event ever reaches egui; no code path exists for it yet.

Also unchanged and worth saying plainly: `World::on_window_event` is still `winit::event::
WindowEvent`-typed. That is the visual's keymap (**H**, **U**, **Esc**, **R**, …) and a baseview
host never calls it. Giving Tier 2 a route into those shortcuts is separate work with its own
question — who owns **Esc** inside a plugin editor — and is not a gap Tier 3 left by accident.

> ✅ **That separate work is #621, and it took the narrow half.** See §2.6 — the world gained a
> *second*, backend-neutral entry point for the camera (`apply_camera_input`), and
> `on_window_event` stayed exactly as described above, keymap and winit typing intact.

### 2.5 What Tier 4 actually retired — the `frame_ring` verdict, and its execution

> ✅ **EXECUTED.** This section was written on 2026-08-02 as a *verdict* about a tier that had
> not been built. It has now been built, and the verdict held item for item: **nothing was
> deleted, everything was gated.** The analysis below is kept as written because it is the
> reasoning, and the execution record is at the end.

#593 Tier 4 states its completion test as a deletion: *"Then retire the mirror. `frame_ring`,
`Mirror`, `pump_mirror`, `MIRROR_EVERY`, the `Shared.mindview` request — all of it exists only
to photograph one process from another. When the viewport is native, deleting it is the proof
the job is done."*

**That test cannot be run as written, and the issue's own "Done means" list is where it breaks.**
Two of its boxes contradict each other:

- ☐ **No second process.** No `frame_ring`, no mirror, no readback.
- ☐ Full Organon (the VST3/CLAP plugin) is **unchanged** — same editor, same behaviour.

Full Organon's editor **draws from `frame_ring`**, and #593 keeps that editor
(`nih_plug_egui`'s baseview+glow one) exactly as it is — the custom wgpu `Editor` is
`mind-edition`-gated, and a Mind VST3 will never be built. So deleting the module deletes the
shipping plugin's viewport: the second box fails the instant the first passes.

**Established from the code (2026-08-02), because the prose was wrong in three places:**

| Question | Answer | Where |
|---|---|---|
| Is the mirror viewport pane edition-gated? | **No.** `viewport_pane` is called unconditionally in `editor_ui`, immediately after `mind_ui::tab_bar`, inside the one `CentralPanel` both editions show. No `cfg`, no `EDITION.is_mind()`, no tab gate | `lib.rs:2096` |
| So which products draw it? | **All three hosts of that editor** — the VST3/CLAP plugin, `organon-standalone`, and `organon-mind`. Same code, same pane. The per-edition *rect* differs (Mind's docks constrain the middle), which is why the letterbox needed pinning — that difference is itself evidence it draws in both | `lib.rs:2059–2095` |
| Is `Shared.mindview[3]` reachable in full Organon? | **It is not merely reachable — it is a constant `1`.** `viewport_on` is `AtomicU32::new(1)` in the plugin's `Default`; the editor's toolbar row re-stores `1` every frame in both editions; **nothing anywhere stores `0`**. There is deliberately no toggle | `lib.rs:363`, `lib.rs:1978`, `lib.rs:1354` |
| Does the mirror idle when no editor is open? | **No.** `process()` stamps `mindview[3] = 1` from the first audio block whether or not an editor was ever opened, so the visual pays the extra 640×360 scene render **and its synchronous `poll(Wait)` readback** at ~15 Hz in every session | `lib.rs:1354`, `world.rs:1619`, `world.rs:1450` |

**Item-by-item verdict for Tier 4** — assuming Tiers 2/3 have put a native viewport in Mind's
window, and assuming full Organon keeps the glow editor (which #593 requires):

| Item | Verdict |
|---|---|
| `native/src/frame_ring.rs` | **STAYS.** It is the plugin's only viewport path. Gate the module `#[cfg(not(feature = "mind-edition"))]` |
| `Mirror` (world.rs) | **STAYS.** It lives in `bin/visual.rs`'s `#[path]` copy of `world.rs`, which is full Organon's projector. Gate the same way |
| `pump_mirror` / `pump_mirror_after_frame` | **STAYS**, gated. In a one-process Mind it is unreachable by call graph anyway (`bin/visual.rs:203` is the only caller) |
| `MIRROR_EVERY` / `MIRROR_W` / `MIRROR_H` / `MIRROR_FORMAT` | **STAY** with the code they pace and size |
| `Shared.mindview[3]` (the request) | **STAYS, and stays even if it were dead.** It is *live* in full Organon; and removing a `Shared` field is a `LAYOUT_VERSION` bump plus a golden re-pin against every saved Ableton set, for four bytes. **The rule for a dead `Shared` field is reserved, never removed.** In the Mind edition it simply stays `0` |
| The editor-side `FrameRingReader` (`viewport_pane`, `PresetUi.frame_reader/_tex/_buf/_retry`) | **STAYS for full Organon; gated out of Mind.** This is the one item Tier 4 genuinely changes — un-gated, it is why Mind showed a photograph |
| *(not on the original list)* the **central region's opacity** | Turned out to be the other half of the same item, and the half that delivers the thread. Removing the pane is not enough: `CentralPanel` fills its whole rect, so a Mind build with no pane still paints the scene out. See §2.4 |

**So the honest headline is smaller than #593's**: not *"no second process, no `frame_ring`, no
mirror"* but **"none of it on Mind's path."** Full Organon keeps every line of it.

**A completion test that can actually run**, replacing the deletion:

1. **Compile-time, and it is deletion-shaped where deletion is real.** Put
   `#[cfg(not(feature = "mind-edition"))]` on `pub mod frame_ring;`, on the `Mirror` block in
   `world.rs`, and on `viewport_pane` + its call site + the `PresetUi.frame_*` fields + the
   `viewport_on` stamp. Then `cargo build --release --features mind-edition --bin organon-mind`
   **compiling green is the proof** — the compiler, not a paragraph, asserts that nothing on
   Mind's path names the mirror. `cargo build --release` staying green is the other half.
2. **Runtime, on the Mac, one `ls`.** Run `organon-mind`; `ipc::ns_file("frame.bin")` —
   `$TMPDIR/organon-mind-frame.bin` — is **never created**. A ring file that never appears is
   the observable form of "no photograph".
3. **The regression half.** Full Organon in Ableton still shows its mirror viewport pane, live,
   under the tab bar. That box is what makes items 1–2 a *gate* rather than a removal.

✅ **The defect this analysis surfaced is FIXED — #609, filed against #554 rather than folded in
here.** `viewport_on` used to default to `1` with nothing ever storing `0`, so the mirror ran in
every full-Organon session in Ableton — projector-only, editor never opened — costing a second
full scene render plus a blocking readback 15× a second, forever. It now defaults to `0`, is
stored at the pane's own draw site, and `process()` publishes
`frame_ring::mirror_requested(EguiState::is_open(), viewport_on)`. The stale comment that made it
invisible (*"Off by default — with this 0 the visual publishes no frames"*, sitting one line above
`AtomicU32::new(1)`) went with it.

**That fix quietly does half of item 1 above.** `viewport_on` is now stored inside the same block
that draws the pane, so gating `viewport_pane` on `not(mind-edition)` takes the request with it —
Tier 4 does not need a separate edit to stop Mind asking. The verdict table is otherwise
unchanged: still nothing to delete.

#### The execution — where each gate actually went

Every item above got `#[cfg(not(feature = "mind-edition"))]`. Nothing was deleted; `Shared` was
not touched; `LAYOUT_VERSION` did not move.

| File | Gated |
|---|---|
| `lib.rs` | `pub mod frame_ring;` · `OrganicMath::viewport_on` (field **and** its `Default` line) · the `snapshot.mindview[3]` stamp in `process()` · `EditorCtx::viewport_on` · `editor_ui`'s re-materialized `viewport_on` · the viewport block in the editor body (rect, `viewport_on.store`, `viewport_pane`, separator) · `fn viewport_pane` |
| `preset.rs` | `PresetUi::frame_reader` / `frame_tex` / `frame_buf` / `frame_retry` (it derives `Default`, so nothing else was needed) |
| `world.rs` | the `frame_ring` import · `struct Mirror` · `MIRROR_EVERY` / `MIRROR_FORMAT` · `World::mirror` / `mirror_want` / `mirror_tick` (fields **and** their constructor lines) · `pump_mirror` / `pump_mirror_after_frame` · `drop_mapped` · the `mirror_want = s.mindview_mirror()` latch |
| `bin/visual.rs` | the one `pump_mirror_after_frame()` call |

Three things that are easy to get wrong here, recorded because each cost a compile or a warning:

1. **`world.rs` is compiled twice**, and only one of those copies is the library's. `pub mod world`
   is itself `mind-edition`-only, so in the *library* — the `World` that Mind's editor drives — the
   mirror is now unconditionally absent. `bin/visual.rs`'s `#[path]` copy is compiled in **both**
   editions, so the gate lives there too.

   ⚠️ **But the visual-side gate is not what stops Mind mirroring, and it is worth being exact.**
   `organic-math-visual` is only ever *built* feature-off — one binary, both products, the
   namespace chosen at runtime by `$ORGANON_IPC_NS` — so its `EDITION` is permanently `Full` and
   the mirror code is always present in the visual Mind spawns (the same fact #616 recorded from
   the other direction). What actually stops it is **upstream**: a mind-edition *editor* never
   stamps `Shared.mindview[3]`, so `mirror_want` latches `false`, `pump_mirror` returns before
   allocating anything, and `$TMPDIR/<ns>-frame.bin` is never created. That is the mechanism
   completion-test item 3 rests on. The `cfg` here is the belt to that brace.

   This is also not a new kind of per-edition divergence in that binary: `bin/visual.rs:268`
   already reads `EDITION.is_mind()` for the instrument-window rule.
2. **`pump_mirror_after_frame` is gated, not stubbed.** An inert empty function would still let
   Mind's path *name* the mirror, and "nothing on Mind's path names it" is precisely what the
   mind-edition build compiling is supposed to assert. So `bin/visual.rs`'s call site carries the
   matching `cfg` — two lines rather than one, buying a real assertion.
3. **Two things went dead that were not on the list**, and the warning count is what said so.
   `drop_mapped` had `pump_mirror` as its only caller and is gated with it.
   `World::render_to_texture` also had exactly one caller (`pump_mirror`) — but it is the #541 S2
   T3 offscreen seam, a real capability rather than mirror plumbing, so it keeps its place with
   `#[cfg_attr(feature = "mind-edition", allow(dead_code))]`: scoped to the one build where it is
   dead, so the default build still reports it if its last real caller ever goes.

**What the compiler now asserts, measured:** `cargo check --all-targets --features mind-edition`
comes back at the same warning counts as `main` **minus one** — and the one that disappeared is
inside `frame_ring.rs`, which no longer compiles on that path. The default edition's counts are
identical to `main`'s, target for target.

---

### 2.6 The viewport's camera (#621) — *built, and driven locally*

Tiers 2–4 and #617 Tier 1 produced a native-resolution viewport in two shapes, **neither of which
could be navigated**: `wgpu_editor`'s `on_event` forwards every event to egui and never touches
`World`. Orbit, zoom and the whole keymap were unreachable in the editor while the separate visual
window kept all of them.

**The design call: a second entry point, not a wider one.** `World::apply_camera_input(
CameraInput)` takes `Orbit { dx, dy }` (physical pixels) and `Zoom { dy }`, and
`on_window_event`'s `CursorMoved` / `MouseWheel` arms **delegate** to it rather than implementing
the gesture — so the visual and the editor cannot orbit at different rates. The keymap does not
cross: **F**, **R**/**B**/**C**/**V**, **O** are projector concerns that mean nothing in a docked
pane, and **Esc** (quit) has no settled owner inside a plugin editor. That door is still open.

⚠️ **`mind_shell::PointerRouter` cannot be the event source here, and this is the part not to
re-derive.** `editor_ui` draws a `CentralPanel`; egui's `PassState::allocate_central_panel` sets
`unused_rect = Rect::NOTHING`, which makes `Context::wants_pointer_input()` unconditionally true
everywhere in the window — and that is the router's `egui_wants_pointer`, which wins ties. Every
event would route to `PointerTarget::Ui`. The visual's own window escapes it only because it draws
a floating `egui::Window` and no central panel. So the authority here is **egui's widget
hit-test**: `scene_input::scene_viewport` registers the scene as a drag-sensing widget and reads
its `Response`, which brings capture, arbitration against sliders, and a `drag_delta` in screen
space that a scrolled pane cannot make stale.

| | region | registered | second rule |
|---|---|---|---|
| **Workstation** | the pane's `vp_rect` | after the scroll area and every card | none needed — egui only hands the pane a drag the surrounding layout did not want |
| **Immersive** | the central panel's whole rect | before the interface | `press_belongs_to_the_scene` — no **interactive** widget under the pointer (unfiltered is never zero: egui registers a `WidgetRect` for every `Ui`) |

| | State | Evidence |
|---|---|---|
| the camera responds to a drag and a wheel | ✅ **measured** | **driven by hand in a local Mac session, 2026-08-04 — confirmed working.** The first input to reach `World` from this editor on any platform, and the row that retires §2.4's long-standing *"input landing — wired, never exercised"* |
| each #617 mode confirmed **separately**, a card drag not orbiting, tracking after the workstation has scrolled | **asserted** | the local run confirms orbit and zoom; it is not an itemised pass, and each of these is a different failure that looks like nothing. Still the finer pass to do deliberately |
| orbit reaches the world in the units the visual uses | **measured** (offline) | `orbit_pixels` + `a_drag_orbits_in_physical_pixels`; without it every Retina display orbits the editor at half rate, working the whole time |
| a workstation drag is unaffected by the pane having moved | **measured** (offline) | `workstation_orbit_is_unchanged_after_the_pane_has_moved` — two harnesses, different pane rects, identical hand movement, identical orbit |
| capture: a drag begun on the scene survives crossing a control | **measured** (offline) | `a_drag_begun_on_the_scene_keeps_the_camera_over_a_control` |
| the interface keeps its own gestures | **measured** (offline) | a click still reaches its button in both modes; a card drag does not orbit |
| the wheel zooms without scrolling the workstation, and still scrolls it off the pane | **measured** (offline) | two tests, both halves |
| `PointerRouter` is unusable under a `CentralPanel` | **measured** (offline) | `a_central_panel_makes_egui_want_every_pointer` — the design note as an executable claim |
| **how any of it feels** | **partly measured** | the local run says it works; nobody has yet compared the orbit *rate* against the visual's on the same display (`orbit_pixels` exists to make them match), or exercised a macOS trackpad's pixel-delta scroll |

No `Shared`/IPC/`LAYOUT_VERSION` change — **six** tiers now. `SceneInput` lives on `PresetUi`
beside `immersive`: transient editor state, so recalling a Scene moves neither the camera nor the
mode.

### 2.7 The LoRA adapter reader (#147 Tier 2) — *landed, and nothing draws it yet*

`organon-core/src/lora.rs`. Point it at a PEFT adapter directory — `adapter_config.json`
plus `adapter_model.safetensors` — and it answers, per adapted module, **how far the
weights moved** (`‖ΔW‖_F`) and **how concentrated the movement was** (the effective rank
of the update). Both are exact functions of the file, which is what makes them *measured*
rather than proxies: this is the first thing in Mind whose provenance is unarguable
because there is nothing to instrument.

`ΔW = s·B·A` with `s = alpha/r` (or `alpha/sqrt(r)` under rsLoRA), and **`ΔW` is never
materialized** — that is a correctness rule here, not a performance note. `ΔW` is
`out × in`; every number is computed through the r×r middle instead. `‖BA‖²_F =
trace((BᵀB)(AAᵀ))`, and the singular values come from `R_B R_Aᵀ` after a Householder QR
of each factor with `Q` never formed. ⚠️ **The per-neuron version has no such shortcut**
— a per-output-row norm needs the full product, because the answer has `out` numbers in
it. #147 names that cliff and this tier stops short of it deliberately.

| | State | Evidence |
|---|---|---|
| the safetensors header, `F32`/`F16`/`BF16` payloads, both spellings of a factor name | **measured** (offline) | 40 unit tests; the byte layout is built by the tests themselves |
| `‖ΔW‖_F` and the spectrum | **measured** (offline) | checked twice over — against hand-stated diagonal fixtures, and against an explicitly materialized `B·A` on dense random factors |
| rsLoRA, `alpha_pattern`, a rank the config disagrees with | **measured** (offline) | each is a silently-wrong-number path, so each has its own test |
| DoRA | **refused**, by name | its update is not `(alpha/r)·B·A`; reading it as LoRA would produce plausible numbers rather than an error |
| **anything against a real adapter** | 🚨 **never run** | no adapter has been parsed on any machine. Every fixture here is synthetic. #147's own closing line — *"nothing here has been run"* — is still true of the file format; what this tier makes false is only *"no arithmetic exists"* |

📌 **No `Shared` change, no `LAYOUT_VERSION` movement, no renderer, no network.** T3
(below) is what turns these numbers into a lens; T1 is what discovers adapters over the
Studio's API. This module knows about neither.

### 2.8 The Delta lens (#147 Tier 3) — *landed; nothing has ever selected it on a machine*

`math.rs`'s `delta_sites` / `delta_into_scalars` / `delta_lens_graph`, behind
**`Shared.mind[2] == 2`**. The specimen, shaped and lit by how far each site actually
moved during a fine-tune — the BinDiff parallel `doc/organon_prd.md` §6.2 has been
asking for.

Structurally it is `stream_frame_into_scalars`' twin, and deliberately so: build the
architecture topology, then overwrite `node_scalar` — from a **static adapter summary**
where the live path takes a **streamed frame**. Both writers walk the same
`for_each_arch_node`, which is the single source of truth for node order. A private
re-implementation of that order would misattribute every value on screen while still
producing the right node *count*, and nothing else would notice.

| The mapping | Node | Why |
|---|---|---|
| `gate_proj` / `up_proj` / `down_proj` (+ `c_fc`, `fc1`/`fc2`, `w1`…`w3`, `dense_h_to_4h`, …), or any `…mlp.…` / `…ffn.…` / `…feed_forward.…` / T5's `…DenseReluDense.…` parent | the layer's **`Mlp`** | |
| `q_proj` / `k_proj` / `v_proj` / `o_proj` (+ `query_key_value`, `c_attn`, `wq`…`wo`, …) | **every** `Head` of the layer, identically | ⚠️ see the limit below |
| everything the layer adapts, recognised or not | the layer's **`Backbone`** | so an unlisted name can never make a trained layer look untouched |

🚨 **A generic leaf must never outvote its parent, and that rule was learned the
expensive way.** The exact-tail tables run *before* the container fallback, so a leaf
name reused by the other kind of site does not merely mis-label — it **overrides the
parent that would have got it right**, landing a real measurement on the wrong node as
a confident picture. Strictly worse than not recognising the name at all. Two entries
were admitted under the weaker rule "this name is used for attention" and are gone:
**`dense`** (HuggingFace BERT names the attention output *and* both FFN projections
with a bare `dense` leaf, and `intermediate.dense` / `output.dense` are not under an
`attention.` parent) and **`wo`** (T5's FFN output is `…DenseReluDense.wo`, while its
*attention* output is `…SelfAttention.o`). ⚠️ Neither removal loses anything, which is
the tell that both were redundant to begin with: Falcon's `self_attention.dense` and
Meta-llama's `attention.wo` are still caught by the container fallback. An entry that
is redundant on its true positives and wrong on its false ones is all cost. The
admission rule now sits above `ATTN_LEAVES` in the source, and both tables were audited
against it — the conclusion for every remaining entry is written there so nobody
re-audits the table from scratch.

📌 **The two gaps that audit left open are now closed, against a tree that was read
rather than remembered** — an installed **transformers 5.5.0** (`transformers/models`,
453 packages). Both had been parked because closing them needed HuggingFace names
nobody was willing to guess at, and a guess in the table whose whole purpose is
not-guessing is the same mistake in a new costume.

- **`fc1` / `fc2` joined the MLP leaf table.** OPT-style decoders declare them
  **directly on the layer**, so the path is `…decoder.layers.N.fc1` with no `mlp`/`ffn`
  segment for the container fallback to catch — and `classify_site` is handed the tail
  *after* the layer index, which for OPT is the bare leaf `fc1`. They were
  `Unclassified`: a layer whose MLP node stayed dark while a real measurement for it
  existed. **155** classes in that tree define `self.fc1` and every one is
  feed-forward; exactly one has *Attention* in its name
  (`Mask2FormerMaskedAttentionDecoderLayer`) and it is not a counterexample — there
  `fc1`/`fc2` are the `dim_feedforward` pair while attention is `self_attn` /
  `cross_attn`. `fc2` scans identically.
- **`densereludense` joined a new `MLP_CONTAINERS` table.** T5 and its family name the
  feed-forward block `self.DenseReluDense`; its attention siblings are `SelfAttention`
  / `EncDecAttention`, which the attention container already catches and which is
  checked first, so the two can never fight over a name. This is what *finishes* the
  `wo` removal above: that removal stopped T5's FFN down-projection being drawn on the
  attention ring, but left it `Unclassified` — the container is what puts it on the
  node it belongs to.

🚨 **A table entry that can never match is exactly as bad as a wrong one, and it is
invisible.** The obvious companion to `DenseReluDense` is `DenseGatedActDense` — and it
is dead. `T5DenseGatedActDense` is a *class*; the attribute it is bound to is
`self.DenseReluDense` in **both** the gated and the ungated variant, in all seven
families using the layout (`t5`, `mt5`, `umt5`, `longt5`, `udop`, `pop2piano`,
`pix2struct`). `self.DenseGatedActDense` occurs **zero** times in the tree, so no module
path can carry that segment. Measured, not argued: adding it and skipping the one guard
below leaves **654/654** tests passing — it compiles, reads as thorough, and covers
nothing. `an_unmatchable_table_entry_is_dead_weight` is the standing guard that makes
the next such entry fail instead.

⚠️ **Uniform across heads is a limit, not a shortcut, and the picture says so.**
`q_proj` is *one* tensor covering every head; resolving per-head needs per-output-row
norms of `ΔW`, which is the full `out × in` product T2 stopped short of on purpose. So
the head ring carries a **per-layer attention** quantity drawn on per-head nodes — and
it therefore renders as a *perfect circle*, where the live lens's ring is ragged
because its heads really do differ. The absence of resolution is visible rather than
implied.

#### 🚨 Two lenses, one visual channel — how a viewer tells them apart

The #226 node glow renders `node_scalar` whether it came from an activation ring (a
*labeled proxy* for "this site is busy right now", §3's #1 recorded gap) or from an
adapter file (**measured** — "this site moved this far during training"). The mode
selector is off-screen from the viewport, so it cannot be the answer. Three things
separate them, and each is decisive alone:

1. **The silhouette.** The live lens rides the skeleton unchanged — a straight-sided
   cylinder, every head ring the same radius at every depth, forever. The Delta lens
   **displaces each off-axis site radially by its own movement** (`DELTA_R_REST` = 0.30
   at nothing, 1.0 at full), so an adapter's footprint is a *profile*: bulging where it
   moved, pinched toward the axis where it did not. A cylinder is never a delta view;
   a waisted specimen is never a live one. The trunk never bends — backbone nodes sit
   on the axis, so the scaling is a no-op on them by construction.
2. **It holds still**, and there are two ways it could have failed to. `world.rs`'s
   `topo == 5` seam is gated to view **0**, so an arriving activation frame can never
   overwrite a Delta view — that gate pre-dates this tier (it is what keeps the galaxy
   static) and makes the separation structural rather than a convention someone has to
   remember. ⚠️ **The #226 cascade sim was the other way**, and it is not gated by
   anything view-shaped: with a firing mode set it computes an `activity` the glow uses
   *instead of* `node_scalar`, replacing the measurement with a free-running procedural
   pulse. `sim_on` now excludes the Delta lens on exactly the reasoning that already
   excludes a live stream. 📌 **The embedding galaxy has the identical hole and it is
   left open** — its node scalars are full N-D embedding norms, equally real and equally
   paintable-over — because that is #507's call to make, not this tier's.
3. **The ring is round** — the uniform-across-heads limit above.

#### 🚨 The normalisation, and what it refuses to be

The displayed quantity is **root-mean-square displacement per weight**,
`‖ΔW‖_F / sqrt(out·in)`, mapped onto `0..1` through a **fixed** five-decade log window
(`DELTA_RMS_LO` 1e-6 … `DELTA_RMS_HI` 1e-1) — the same window for every adapter ever
loaded. Two refusals are doing the work:

- **Not raw `‖ΔW‖_F`.** Frobenius norms grow with entry count, so a `14336×4096` MLP
  projection outweighs a `4096×4096` attention projection by ~1.87× **before any
  training happens**. Lighting the specimen with raw norms would paint every model's
  MLP brighter than its attention and invite *"fine-tuning moves the MLP most"* — an
  artifact of matrix shape wearing the clothes of a measurement. Dividing by
  `sqrt(out·in)` also composes exactly: over a group of modules the RMS is
  `sqrt(Σ‖ΔW‖²_F / Σ(out·in))`, i.e. the RMS over the concatenation of their entries,
  which is how a site pools its modules and how the backbone pools a whole layer.
- **Not a per-adapter maximum.** That puts `1.0` at the top of *every* adapter, so a
  barely-trained LoRA and a heavily-trained one render identically — destroying
  precisely the comparison this lens exists for. Pinned by test: two adapters differing
  only in how far the weights moved must not produce the same picture.

⚠️ The window's ends are the **one** display choice here that is not an exact function
of the file. Values outside it clamp rather than wrap, and `DeltaSites::rms_range()`
reports the real extremes so a readout can print what was actually measured.

| | State | Evidence |
|---|---|---|
| node order matches the live writer's, per site kind | **measured** (offline) | mutation-tested: swapping the head and MLP arms fails with *"head node 1 got the attention value: 0.2"* |
| two different adapters do not render identically | **measured** (offline) | mutation-tested: a per-adapter max-normalise fails with *"a louder adapter must not look identical"* |
| shape does not masquerade as movement | **measured** (offline) | mutation-tested: dropping the `sqrt(out·in)` fails with *"same per-weight movement ⇒ same site value (4.096 vs 7.6629…)"* |
| the silhouette distinguishes it from the live lens | **measured** (offline) | mutation-tested: removing the radial displacement fails with *"an untouched site pinches to the rest radius"* |
| an unrecognised module name is reported, never guessed | **measured** (offline) | mutation-tested: falling back to `Mlp` fails with *"nor onto the MLP node"* |
| a generic leaf never outvotes its parent | **measured** (offline) | mutation-tested both ways: re-admitting `dense` fails with *"BERT's FFN up-proj must not be drawn on the attention ring — left: Attn, right: Unclassified"*, re-admitting `wo` with the T5 equivalent, and deleting the container fallback fails the Falcon and Meta-llama guards, so those two are not passing by accident |
| OPT's inline `fc1`/`fc2` and T5's `DenseReluDense` reach the MLP node | **measured** (offline) | the leaf and the container are mutation-tested one at a time: dropping `fc1`/`fc2` fails with *"left: Unclassified, right: Mlp"* on `model.decoder.layers.3.fc1`, dropping `densereludense` fails two tests, one of them *"T5's FFN down-proj belongs on the MLP node, not the attention ring — left: Unclassified, right: Mlp"*. The names behind both were read out of an installed transformers 5.5.0, not recalled |
| no container pattern is unmatchable | **measured** (offline) | `an_unmatchable_table_entry_is_dead_weight` requires every entry in `MLP_CONTAINERS` to be reachable by a name in circulation. Adding the dead `densegatedactdense` fails only that test — with it skipped, all **654** others pass, which is the measurement of how invisible a dead entry is |
| **anything on a screen** | 🚨 **never run** | no adapter has been read on any machine, nothing has ever written the adapter sidecar, and no GPU has drawn this. Every claim above is arithmetic and geometry, checked offline |

📌 **No `Shared` change and no `LAYOUT_VERSION` movement.** The view rides the `mind[2]`
slot that already exists (`0` specimen, `1` galaxy, `2` Delta), and the adapter
*directory* rides a new sidecar, `ipc::adapter_sidecar_path()` →
`$TMPDIR/<ns>-adapter.txt`, because a path is not a control-rate value. ⚠️ **Nothing
writes that sidecar yet** — the picker is a later tier — so selecting the view today
clears the graph and prints *"no adapter selected"*. That is the honest failure, on the
same rule the galaxy follows: substituting the specimen would show the user a different
thing than the one they asked for.

⚠️ **The write clamp and the read decoder must move together.** `lib.rs` clamps
`mind_topo` to the highest view that exists and `math::mind_view_mode` decodes it; a
view added to one and not the other is either a selector that silently does nothing or
a value nothing decodes. Both now say `2`.

## 3. The honesty ledger

What the product currently claims, and how true it is. Keep this honest — it is the
brand.

| Displayed | Provenance | Notes |
|---|---|---|
| Layer / head / expert counts, dims, vocab, quant mix | **measured** (from the file) | `gguf.rs`, header only |
| The specimen's wiring | **measured** | `gguf_architecture_graph` |
| Parameter counts, weight bytes, bits/weight, KV cost | **derived** | exact functions of the tensor directory |
| `‖ΔW‖_F` per adapted module, and the update's singular values | **measured** — an exact function of the adapter file | `lora.rs`. ⚠️ Two caveats it owes wherever it is rendered. **The base may be quantized**: an adapter trained on a 4-bit base is a delta against weights that are not the released ones, and the file states only `base_model_name_or_path` — #147 T1's `is_quantized` is what answers it from data. And **nothing has been read from a real adapter yet**; the arithmetic is tested against synthetic fixtures only |
| Effective rank, stable rank, "which layers this fine-tune changed most" | **derived** | exact functions of the singular values. The effective rank is Roy & Vetterli (2007) — `exp` of the entropy of the normalised spectrum — stated in `lora.rs` rather than left implicit, because at least three quantities go by that name |
| **The Delta lens's glow and silhouette** (#147 T3) | **measured** — the RMS weight displacement at each site, an exact function of the adapter file | §2.8. 🚨 **It drives the SAME visual channel as the per-layer generation glow below, which is a proxy**, so the two are made distinguishable *in the picture*: the Delta lens deforms the specimen's silhouette (a live specimen is a straight-sided cylinder; a delta specimen has a waist), it cannot move (the ring's overwrite is gated to view 0), and its head ring is perfectly round because it has no per-head resolution. The mode selector is off-screen and is deliberately not the answer. ⚠️ Two things the *quantity* still owes wherever it is written in words: the mapping to brightness is a **fixed** five-decade log window — a display choice, the only non-exact step — and per-head is a **limit**, so a bright ring means "this layer's attention moved", never "these heads moved" |
| "This layer learned \<concept\>" | 🚨 **contested claim** | norm is not importance and effective rank is not meaning. Not currently rendered anywhere; recorded now so the first lens that wants to say it finds the row already written |
| The per-layer glow during generation | **measured** — `layer_norm` + `mlp_act`; `head_summ` is still a labeled proxy | ✅ **Confirmed by running it, 2026-08-21** — organon-one (RTX 5090, CUDA 13.3), `gemma-4-12B-it-QAT-Q4_0.gguf`, 48L×16H, all layers GPU-offloaded. The runtime printed `mind-runtime: activation tap MEASURED — real per-layer tensors (#522 T1) (48 layers requested)`, and frames carry `flags=0x6`/`0x7` (`FLAG_RESID_MEASURED` + `FLAG_MLP_MEASURED`), so the #482 dashboard's provenance glyphs for these two read `=`. **It was the Windows/CUDA path that got there first, not Metal** — the tap is the safe `llama-cpp-4` `cb_eval` API, so this is evidence about the API, not about one backend; Metal remains unrun. ⚠️ **The prediction this row used to make was wrong, and the correction matters more than the flag** — see §3.1 |

### 3.1 What the measured depth profile actually looks like

⚠️ **This row predicted that a measured profile would "rise monotonically with depth
instead of showing the proxy's travelling sine." It does not, and a reader checking the
glow against that sentence would have concluded the tap had failed.** What the residual
norms actually do on a real model is climb to a **mid-late peak and then fall away**, with
a sharp collapse at the final layer. Measured over four consecutive tokens of a real
generation (`layer_norm`, 48 layers, normalized by `normalize_tap` so the token's own max
is 1.5):

```text
token 55  L0=0.695  ... rises to L22=1.500 ... L29=0.719 ... L46=0.852  L47=0.072
token 57  L0=0.585  ... rises to L25=1.500 ... L29=0.770 ... L46=0.754  L47=0.110
```

`mlp_act` is more lopsided still: layers 0–1 carry the maximum and the rest of the stack
sits **two orders of magnitude** below it (0.006–0.09), which is the well-known
early-layer outlier behaviour, not a bug in the tap.

📌 **So "monotonic" is the wrong acceptance test. Three properties actually distinguish
measured from proxy, and each is decisive on its own:**

1. **The proxy cannot reach 1.5.** Its two factors are each ≤ 1.0
   (`(0.30 + 0.70*entropy) * (0.45 + 0.55*wave)`), so its ceiling is **1.0**. The measured
   path divides by the token's own max, so its max is **exactly 1.5000**. Observed:
   1.5000 in 4/4 frames, both channels.
2. **The proxy has a floor.** `layer_norm` cannot go below `0.30*0.45 = 0.135` and
   `mlp_act` cannot go below `0.25*0.40 = 0.100`. Observed: **160 of 192** `mlp_act`
   samples below 0.100, reaching 0.0057.
3. **The proxy travels; a real profile does not.** The proxy's phase is
   `(tok*0.35 + lp*6.0)`, so its argmax moves every token by construction. Observed
   argmax stayed at L25 (once L22) and argmin at L47 across all four tokens, with a
   Pearson **r of 0.94–0.97** between consecutive tokens' profiles.

The visible quantity was always *relative* depth profile — `normalize_tap` says so — and
that is what these numbers are. A future reader wanting to re-confirm the tap should check
those three properties, not the shape.

---

## 4. Verification bar (unchanged from the house rule)

- **Cloud (a session like this one):** `cd native && cargo build --release`, plus
  `cargo build --release --features mind-edition --bin organon-mind`, plus
  `cargo test --release` (includes `tests/wgsl.rs` — naga parses + validates every
  shader offline). Pure logic — parsers, projections, geometry scalars, layout and
  selection reducers, export — is unit-tested against synthetic frames. **This is the
  ceiling.**
- **Mac (a local session):** the standalone actually booting, the shared visual attaching to the
  Mind namespace, live inference and the `cb_eval` tap, the look of every lens, the
  external mirror, GPU perf.

A finished cloud PR is **"green and ready to deploy"**, never "verified working."

---

## 5. Where the next work goes

Phase order lives in `doc/organon_mind_buildplan.md` §4; this is only the map from a
capability to the seam it plugs into.

| Building… | Plugs into |
|---|---|
| A new **lens** | a `math.rs` graph builder feeding `neural_loaded` (the #226 glow path), selected by `Shared.mind[2]` (`topo_mode`) — add the value to **both** `math::mind_view_mode` (which decodes it) and `lib.rs`'s `mind_topo` clamp (which decides what can be written), then give `world.rs::build_mind_graph` a branch. 🚨 **And say how a viewer tells your lens from the others**: they all drive one glow channel, so a new quantity on it inherits the duty to be distinguishable in the picture, not only in the selector (§2.8 is the worked example) |
| **Real activations** | the `cb_eval` tap in `bin/mind_runtime.rs` → appended `MindFrame` fields (single-threaded spine step) |
| A new **analytics readout** | `mind_viz.rs` (editor-side, reads the ring directly — no `Shared` change) |
| **Mind-only UI** (sub-tabs, panels) | `mind_ui.rs` — Tier 2 factors the Mind card body out of `lib.rs` and adds the sub-tab router |
| **A new dock or pane** | `mind_shell.rs` for the pure geometry (`egui_docks` / `layout_workstation`), the panel call itself in `lib.rs` beside the #532 T1 docks |
| **The portrait inside a pane** (#532 T4) | separating the world from the window — `world.rs::World::render` since the #572 hoist; `mind_shell::PointerRouter` is wired to `ui_layer` (#554 T4) and ready for the moment the viewport becomes a child rect |
| **The one-process viewport** (#593) | `editor_probe.rs` — the custom `Editor` that owns a wgpu surface on the host's parent view (§2.4). Tier 1 extracts `lib.rs`'s editor body so both hosts call it (*done*, #602); Tier 3 replaced `FrameTarget::ui_window` with the `egui_platform::EguiPlatform` seam + its winit arm (*done*; the baseview arm is Tier 2's, since the window is what produces the events); Tier 2 grows the probe's `on_frame` into `World::render_into` + egui; Tier 4 **gates** `frame_ring`/`Mirror` out of the Mind edition — it cannot *delete* them, because full Organon's editor still draws from them (§2.5) |
| **Packaging** | #483 Tier 4: an `.app` around `organon-mind`, embedding the visual, its own name/icon/namespace |
| ~~**The Delta lens** (#147 T3)~~ | **landed** — §2.8. ⚠️ Note what changed on the way: the per-site scalar is *not* `per_layer_fro()`, because a raw Frobenius norm grows with matrix size and would light the MLP brightest on every model before any training happened. It is RMS-per-weight (`‖ΔW‖_F / sqrt(out·in)`), which pools exactly and is shape-free |
| **The checkpoint scrub** (#147 T3's extension) | the same builder, one `DeltaSites` per checkpoint on a slider — `CheckpointInfo{path, loss}` from `/api/models/checkpoints` is already the index. Needs T1 (the API client) and the adapter picker that writes `ipc::adapter_sidecar_path()`, which nothing does yet |
| **Anything that talks to Unsloth Studio** (#147 T1/T4/T5) | a hand-rolled HTTP client over `TcpStream`, the shape `organon-agent`'s `HttpChatClient` and `mcp_http.rs` already have. 🚨 **Not `Shared`** — step-rate telemetry from someone else's process must not buy a permanent offset-sensitive layout commitment |
