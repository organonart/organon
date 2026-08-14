# The portal — reconnaissance for a screen-anchored, live, orbitable Organon window in the console

**Status:** read-only reconnaissance, 2026-08-13. **No code was written and no source file was
changed.** Everything below is a claim about the tree as it stands at `main` @ `02fff1a`, with
the site that supports it named. Where something is a judgement call it is marked as one and a
single recommendation is given.

---

### 🚨 Amendment, 2026-08-13 (landing): the portal has since been built, and one claim below is wrong

This document was written *before* the portal existed and is merged *after* it. Both halves of
that sentence matter. Read on the original terms — a survey of what a portal would cost — but
read knowing that PR #32 (`console/portal`) landed the portal itself and PR #33
(`console/portal-camera`) landed its camera, and that **`CONSOLE_ARCHITECTURE.md` §1.2 and §1.3 are
now the authority on what the console actually does.** This is the investigation those two tiers
were built from; it is not a description of the tree.

⚠️ **One claim did not survive contact, and it is the first row of the headline table:
"immersive is landed" is FALSE, and false in a way that would make anyone scoping immersive
budget it at zero.** The correction is §1.1's amendment and is restated here because the table
is what gets read: the backdrop's *rendering* is indeed already there, but the portal's
*painting* is a different path. `paint_portal` paints the portal **over** the front-end — that is
what floating means — while immersive needs the portal's texture **under** the glyphs with the
scrim over it, and the scrim lives inside `term_view::draw`'s `Some(bands)` arm, fed from the
epoch ledger. Immersive is therefore a **new integration** (a single-band `BandedBackdrop`
carrying the portal's texture, deliberately not opening a look epoch), not a variant added to
`portal::step`. Everything else below stands.

📌 **What became of the recommendations.** §2 (a `portal` field, not a `SurfaceKey` variant), §3
(the state machine is also the render budget), §5 (a rect in the `CentralPanel`, not an
`egui::Area`; an explicit rect test for the wheel), §6's risk 1 (the portal shows the `World`, not
the substrate), §8 (`CommandSpec` first, clap second) and §9 (one `World`, no publish/restore
dance) were all adopted as recommended. **Still unbuilt, and still the open work:** immersive,
full screen, the animated grow, the click/double-click transitions, `scene_viewport`'s `Sense`
parameter (§6 — it is still hardcoded `Sense::drag()`), state-conditional Escape (§7), and §10's
recommendation that `ORGANON_SHELL_BACKDROP` stop being a way to *start* in the backdrop.

⚠️ **Two shapes shipped narrower than §7 proposed, deliberately.** `PortalState` is
`{Closed, Open}` and `PortalEvent` is `{Open, Close, Toggle}` — not the four states and five
events sketched there — on the rule that *an event nothing can raise is an untested arm
pretending to be a design*. The four-state machine is still the destination; §7 is its argument,
not its shipped signature.

⚠️ **Every line number below is as of `02fff1a` and most have moved** — `console_main.rs` alone
gained ~1100 lines in the tiers this document produced. `world.rs:6525`, the substrate-rig trap
that is the single most valuable finding here, is now `world.rs:6530`. Follow the **function
names**, which are stable; treat the numbers as provenance for what was read, not as an index.

---

**The ask, in James's words:**

> "I would love to get a full Organon window in there… rendering… and then we could use the
> Organon CLI so you could actually open the window and control Organon from the shell… and the
> window could float in some way so that everything flows around it… so when it scrolls, the
> window doesn't scroll away… so you have this little portal into a 3D world in your console."

> "when it's Organon on, you could also click it or double-click it, and it's just like selecting
> a photo in a very nicely designed gallery online. The frame will smoothly grow to fill the whole
> shell, turning the shell into an immersive mode. And in immersive mode, the frame becomes the
> background, and then we just put some sort of a nice semi-translucent background on the overlay
> shell so we can read it. Then you could double-click that again, and the whole thing would
> animate again and become truly full screen."

Three states — **portal → immersive → full screen** — with animated transitions, opened from the
`organon` CLI, screen-anchored so the transcript scrolls past it.

In issue #3's vocabulary this is **a screen-anchored patch that is live and interactive**. That
sentence is the whole design; everything below is what it costs.

---

## 0. The headline

**Two of the three states are nearly free. The third is real but small. The two things that will
actually hurt are the camera and the animation's texture churn, and neither is where you would
look first.**

| Verdict | |
|---|---|
| ❌ **~~Immersive is landed~~ — WRONG, see the amendment above** | *As written:* the backdrop path already renders a pane-sized `World` every frame, paints it at UV 0..1 under the glyphs, and lays a legibility scrim over it with an inviolable floor. That *is* "the frame becomes the background + a semi-translucent overlay". It is reachable at **runtime** by a call that already ships. — **The rendering is landed; the painting is not.** The portal paints *over* the front-end and immersive needs it *under* the glyphs, through a seam the portal does not use. Corrected in §1.1. |
| ⚠️ **Full screen is NOT free from `render_source`** | That seam separates *what the engine draws* from *what the backdrop paints*. It says nothing about whether the **overlay** paints, and no path today suppresses the scrim, the glyph grid or the tab strip. Small, but genuinely new. **Still true**, and `render_source` is now one half of `engine_plan` — see §1.2. |
| ⚠️ **Portal is new, but less new than it looks** | The conversation front-end's `/surface` is already a rendered, engine-drawn, egui-laid-out rect with its own texture, its own render list and its own cap. It is *scroll*-anchored. Re-anchoring it to the screen and making it live is the work. **Borne out** — that is exactly what PR #32 did, reusing `SurfaceTexture` and `make_surface_texture` verbatim. |
| 🚨 **A substrate portal cannot be orbited, silently** | An installed substrate rig overrides the camera tuple wholesale, so `World::apply_camera_input` writes values nothing reads. The drag will look unwired. |
| 🚨 **The animation is a texture-realloc storm by construction** | A size change frees and reallocates the surface texture and logs unconditionally. An animated grow does that every frame, by design. |
| ✅ **"Control Organon from the shell" is already true** | The param lane (`organon set` / `generator` / `recipe`) drains inside `World::frame_body`, and every console tab is launched with `ORGANON_IPC_NS` injected. The moment a portal renders the World, it is CLI-drivable with **zero** new work. |

---

## 1. Testing the central claim, state by state

### 1.1 Immersive ≈ the existing backdrop — ~~**confirmed**~~ **REFUTED once the portal existed**

> 🚨 **Amendment, 2026-08-13 (landing).** Everything measured in this section is still true of the
> **backdrop**. The inference drawn from it — that immersive is therefore nearly free — is not,
> and the reason is a path this section never had cause to look at because it did not yet exist.
>
> The portal, as built, is painted by `paint_portal` **over** the front-end: it is drawn into the
> `CentralPanel` after `term_view::draw`, which is what makes it float and occlude. Immersive
> needs the same texture **under** the glyph layer with the scrim over it — and that is not a
> place a painter can be moved to, because the under-the-glyphs path is not a painter at all. It
> is `term_view::draw`'s `backdrop: Option<BandedBackdrop>` parameter, fed from the epoch ledger,
> with the scrim applied once inside the `Some(bands)` arm after every band. So immersive means
> **handing the portal's texture into that seam as a single band** — a new integration between two
> subsystems that today do not meet, not a fourth arm on `portal::step`.
>
> The two qualifications below are the ones that carry into it, and they get sharper rather than
> weaker: the single band must deliberately **not** open a look epoch, or the first screenful is
> striped; and because the conversation front-end is handed no `bands` at all, immersive is a
> **terminal-tab-only route** as things stand.
>
> The measurement was right; the conclusion drawn from it was one path short. Recorded rather than
> deleted because the backdrop half is what immersive will be built out of.

`Console::render_backdrop` (`native/src/console_main.rs:1979`) allocates a texture sized to the
**pane's fraction of the swapchain** (`scene_input::pane_pixels_in`, `:2019-2022`), renders the
World or the substrate into it every frame (`:2073`), and registers it with egui. `term_view::draw`
then paints it — banded by look-epoch — and lays the scrim over it
(`native/organon-console/src/term_view.rs:665-682`). `scrim_alpha`
(`native/organon-console/src/term_view.rs:86`) defaults to 185 and can never go below
`SCRIM_FLOOR` = 96; `no_scrim_setting_can_cross_the_floor` proves that over the entire byte range.

So: a full-window render, behind the text, with a tuned semi-translucent legibility layer over it
whose floor is structural. That is James's immersive description almost word for word. Three
qualifications, all minor:

- **It is the pane, not the window.** The 30 pt tab strip is declared before the `CentralPanel`,
  so the backdrop stops beneath it. Arguably correct — the strip is "the one permitted chrome" —
  but say so rather than discover it.
- **It bands.** `band_quads` cuts the pane into one quad per look-epoch, so a console that has
  changed looks shows history in stripes. With one epoch it collapses to exactly one full-rect
  `0..1` quad, which is what immersive wants; a portal-driven immersive should therefore not open
  a new epoch, or the first screenful will be striped.
- **The conversation front-end has no backdrop at all.** `bands` and `patch_image` are handed only
  to the terminal branch (`native/src/console_main.rs:2479-2489`); the conversation branch draws
  nothing behind itself. Immersive in a conversation tab is genuinely new work — the CentralPanel
  would need a painted image beneath `conversation_view::draw` and a scrim over it.

### 1.2 Full screen ≈ immersive minus the overlay — **refuted as stated**

`Console::render_source` (`native/src/console_main.rs:1971`) is:

```rust
if self.backdrop_source == BackdropSource::Off && self.patches_want_image() {
    BackdropSource::Substrate
} else {
    self.backdrop_source
}
```

📌 **Superseded, 2026-08-13 (landing): that body is now one line.** `render_source` delegates to
`engine_plan(portal_open, backdrop, patches_want_image) -> (BackdropSource, bool)` and returns its
first element — the third input §6 predicted this seam would need, added exactly where §6 said it
belonged. The quoted logic survives verbatim as `engine_plan`'s non-portal arm, so the argument
below is unchanged; only the address moved.

That is *engine draws* vs *backdrop paints*. It has no third axis for *overlay paints*, and there
is no such axis anywhere: the tab strip is declared unconditionally
(`native/src/console_main.rs:2452`), the `CentralPanel` always dispatches to one of the two
front-ends (`:2471-2505`), and the scrim is applied unconditionally inside the `Some(bands)` arm.

Full screen therefore needs a new suppression branch. **It is small, and the precedent for its
argument already exists in the tree**: the patch painter reasons that a block's reserved rows
"carry none [no text] — so dimming them buys no legibility and costs the whole effect", and paints
*after* the scrim for exactly that reason. Full screen is the same argument at window scale — you
do not lower the scrim below its floor, you **do not draw the layer the floor exists to protect**.
`SCRIM_FLOOR` is untouched and the contract holds.

📌 **Judgement call: what "truly full screen" means.** Two readings — (a) the overlay is
suppressed inside the existing window, (b) the OS window also goes borderless-fullscreen.
**Recommend (b)**, because "the whole thing would animate again" only reads as a second step if
something visibly more happens, and (a) alone barely differs from immersive on a maximised window.
The precedent is `World::wants_fullscreen` (`native/src/world.rs:10280`) — a world-side request the
host applies — although the console would drive `winit`'s `set_fullscreen` directly rather than
route through the World.

### 1.3 Portal — **new, and here is exactly how new**

The nearest existing thing is `/surface` in the conversation front-end. It is already:

- an egui-laid-out rect (`conversation_view::surface_element`, full column width by
  `SURFACE_HEIGHT` = 260 pt);
- backed by its own render target, allocated and registered by `Console::make_surface_texture`;
- rendered by the **one** World into that target, with the rig re-framed for that rect's aspect;
- bounded by a cap (`MAX_SURFACE_TEXTURES` = 4) with least-recently-**requested** eviction and one
  log line per release;
- sized by handing **points** across the crate seam and never a scale.

Everything in that list is reusable verbatim. What a portal changes is exactly four things:

1. it is **screen-anchored**, not an element in a transcript (§2);
2. it is **live**, not a still life (§3);
3. it **animates** between three rects (§4);
4. it **takes camera input** (§6).

---

## 2. Keying a screen-anchored surface

### The sites that assume a surface belongs to a transcript element

| Site | Assumption |
|---|---|
| `SurfaceKey` | `type SurfaceKey = (usize, ElementId)` — pane index plus element id (`native/src/console_main.rs:776`) |
| the map | `surfaces: HashMap<SurfaceKey, SurfaceTexture>` (`:1035`) |
| the request list | `wanted: Vec<SurfaceKey> = requests.iter().map(|r| (pane, r.element))` (`:2154`) |
| key construction | `let key = (pane, request.element)` in the allocate/render loop (`:2173`) |
| the eviction log | prints `element {key.1 .0} (pane {key.0})` (`:2274-2279`) — a portal here would print a fabricated element id in a fabricated pane |
| `free_all_surfaces` | frees everything when a tab close renumbers the panes (`:2290`, called at `:1402`) |
| the test helper | `fn key(pane: usize, element: u64) -> SurfaceKey` and its three tests (`:3510-3557`) |
| the image map | `pub type SurfaceImages = HashMap<ElementId, egui::TextureId>` (`native/organon-console/src/conversation_view.rs:146`), filled at `native/src/console_main.rs:2199` |

`surfaces_to_evict` itself is key-agnostic — it only needs `Copy + PartialEq` — so the pure
eviction policy and its tests survive any keying change untouched.

### Recommendation: **do not widen `SurfaceKey`. Give the portal its own slot.**

The obvious move is `enum SurfaceKey { Element { pane, element }, Portal }`. I recommend against
it, for a reason that is about meaning rather than effort:

**Eviction is a policy for many things competing for few slots. A portal is one thing that is
either open or closed.** A portal is requested every frame it exists, so its `touched` stamp is
always `now` and `surfaces_to_evict` can never choose it — a `Portal` variant would exist solely
to be structurally excluded from the one function the type exists to serve. Worse, the enum
imports two obligations that then have to be *remembered* rather than being impossible: excluding
the portal from `free_all_surfaces` (a pane renumbering means nothing to it, and blanking it on
every tab close is a visible flicker in a thing that is meant to be steady), and teaching the
eviction log a second sentence so it does not print a fake element id.

So: a `portal: Option<SurfaceTexture>` field on `Console`, beside `backdrop`. `SurfaceTexture`,
`make_surface_texture` and the free-and-log body are all reused; only the log's identifying clause
differs. `SurfaceKey`, its three tests, `SurfaceImages` and the whole `conversation_view` seam stay
exactly as they are — which also means the portal works identically in a **terminal** tab, where
there are no elements and no `ElementId` at all. That is the deciding argument: the portal must
work in a terminal tab, and `SurfaceKey`'s element half has no meaning there.

The portal's texture id is then returned alongside `SurfaceImages` rather than inside it —
`fn render_surfaces(&mut self, ..) -> (SurfaceImages, Option<egui::TextureId>)`, or a small struct
if that reads better at the call site.

### What the cap costs, honestly

Under this recommendation the portal occupies **none** of `MAX_SURFACE_TEXTURES`; it costs a
**fifth** texture. Quote the number rather than the reassurance:

- **portal-sized** (say 40 % of a 2475×1553 pane, 990×621) — 2.5 MB;
- **pane-sized** (2475×1553) — 15.4 MB, identical to the backdrop's own.

Against the surface budget's ≈23 MB and the epoch cache, that is real but not alarming — *provided*
the portal is never pane-sized at the same time as the backdrop. §7 shows the state machine makes
that impossible for free. `surface_budget_bytes` should gain a portal term so the eviction log's
quoted budget stays true, on the same "a test quotes the figure so the prose cannot drift" rule
that function already carries.

> 📌 **Solved differently, 2026-08-13 (landing).** `surface_budget_bytes` was left alone and
> `free_portal` reports the two separately instead — *"N of 4 conversation surfaces live, portal
> B bytes"*. Same requirement, better answer: the cap really is four conversation surfaces, and
> folding a fifth non-evictable texture into a constant named for the cap would have made the
> number describe neither quantity.

⚠️ If you take the enum route anyway: a portal requested every frame **permanently costs the
conversation one of its four slots**, so a transcript with four visible surfaces plus a portal
starts thrashing — the truncation branch at `native/src/console_main.rs:2132-2144` fires, prints its
"scroll one out of view to free a slot" line, and one surface renders "rendering…" forever. That is
a real regression in an unrelated feature, caused by a keying decision. Bump the cap to 5 and
re-pin `the_surface_budget_is_four_textures_worth` if you go that way.

---

## 3. Live versus still

Surfaces today are **still lifes**, and that is the load-bearing property:

```rust
let (id, size_px, stale) = { … (held.id, held.size, held.holds.as_ref() != Some(&desired)) };
images.insert(request.element, id);
if !stale || budget == 0 { continue; }
budget -= 1;
```

(`native/src/console_main.rs:2194-2203`.) `SurfaceLook` is compared for equality; a surface whose
look has not changed is not re-rendered, "and that is what makes an idle conversation cost zero
engine frames" (`:738-750`). `SURFACE_RENDERS_PER_FRAME` = 1 bounds it to one extra `World` frame
per console frame.

**A portal deletes that comparison for itself.** It is stale every frame by definition — the
world moves, the beat clock advances, the camera orbits. Concretely:

- the `holds`/`desired` test does not apply; the portal renders unconditionally while open;
- it must not consume the surfaces' budget, or a portal open beside a dragged slider would
  starve one of them at random. Give the portal its own render, outside the `budget` loop.

### The per-frame cost with the backdrop also up

The console **repaints continuously** — `redraw()` ends with `window.request_redraw()`
(`native/src/console_main.rs:2593`) — so this is a per-frame, always-on cost, not an occasional one.
Each `World::render_to_texture` is a full `frame_body` (`native/src/world.rs:2050` → `:2071` →
`:2081`): every generator, every pass, the whole post chain, and the CLI drain.

| Frame contains | Engine frames per console frame |
|---|---|
| today, backdrop off, no surface changing | 0 |
| today, backdrop on | 1 |
| today, backdrop on + a dragged surface | 2 |
| **portal open, backdrop off** | **1** |
| **portal open + backdrop on** | **2 — and this is the case to forbid** |

🚨 **The last row is the case `SURFACE_RENDERS_PER_FRAME`'s doc explicitly rules out.** Its own
words: what double-steps is what counts *frames* — `frame_index`, the TAA jitter phase and the
temporal history beside it, shared between the two targets — and "on a moving World it would be
[visible], and intermittently — which is why the surface look is the substrate". A live World
portal *plus* a live World backdrop is exactly that forbidden case, promoted from a documented
non-issue to the default.

✅ **The state machine dissolves it at no cost.** In **Portal** the backdrop is off and only the
portal renders. In **Immersive** and **Full screen** the portal *is* the backdrop — same pane-sized
texture, same render — so only the backdrop renders. **At most one World render per frame, in
every state, by construction.** Say this out loud in `CONSOLE_ARCHITECTURE.md` when it lands; it is
the kind of property that is free while someone remembers why and expensive the first time
somebody adds a second portal.

---

## 4. 🚨 The animation, and the specific way it will hurt

### The mechanism, confirmed

```rust
let size = scene_input::pane_pixels_in(swapchain, request.size_points, window_points);
if self.surfaces.get(&key).is_none_or(|t| t.size != size) {
    self.free_surface(key, "the surface changed size");
    let Some(made) = self.make_surface_texture(&device, size, now) else { continue };
    self.surfaces.insert(key, made);
}
```

(`native/src/console_main.rs:2174-2179`.) And `free_surface` logs **unconditionally**
(`:2271-2286`) — deliberately, on the rule that "a cap that silently drops a picture is
indistinguishable from a renderer that failed to draw one".

So a size change is: free the texture, free the egui registration, create a new texture, register
it, and `holds` resets to `None` so the next frame re-renders. Plus one `[surface]` line.

> ⚠️ **Confirmed live, 2026-08-13 (landing), and it is the one finding here that is now a real
> defect rather than a prediction.** `render_portal` carries the identical body — a size change
> calls `free_portal("the portal changed size")` and reallocates — so the portal frees,
> reallocates, re-registers and logs one `[surface]` line on **every frame of a window-resize
> drag**. The animation half is still hypothetical because the animation is unbuilt; the
> resize half is shipped, and `CONSOLE_ARCHITECTURE.md`'s "what nobody has looked at" list names
> it. The recommendation below — allocate at the destination, scale the quad, settle once — is
> unchanged and closes both.

**Confirmed: an animated grow does this every frame, by design.** A 250 ms transition at 60 Hz is
~15 reallocations and ~15 log lines per portal, per transition, in each direction — and every one
of those frames also throws away a just-registered egui texture id, which is the part that costs
more than the allocation. The prior recon's window-resize-drag finding is the same defect from the
other side and is equally real: `pane_pixels_in` changes on every frame of a drag, so every open
surface reallocates and logs on every frame of it.

### Recommendation: allocate for the destination, scale the quad, reallocate once on settle

Not a debounce, not a size tolerance — **a rule that removes the churn instead of damping it**:

1. **When a transition starts, allocate the texture at the size the transition will END at**, and
   keep it for the whole transition. One free, one create, one registration, per transition.
2. **The quad interpolates; the texture does not.** Paint the same texture into a rect that eases
   from the source rect to the destination rect.
3. **Reallocate once, on settle** — and only if the settled size differs from what was allocated
   (it will not, if step 1 was right).

Three things fall out of it:

- **Filtering goes the right way.** During a grow the texture is larger than the quad, so the
  image is *minified* — which the `FilterMode::Linear` registration already handles well.
  Magnifying a small texture up to full window, which is what render-once-at-the-source-size
  would do, is the version that looks soft, and it looks softest at the end of the animation where
  the eye has settled.
- **The log stops lying.** `"the surface changed size"` fires twice per transition instead of
  thirty times, and it stays unconditional — the repo's rule about silent drops is preserved
  rather than traded away.
- **The window-resize drag is fixed by the same rule**, with a settle timer instead of an
  animation curve: hold the current texture through the drag, reallocate on the first frame the
  size is unchanged. That closes the "one thing that will look broken on day one" finding without
  a special case.

### What interpolates — and does an animation need a UV fit policy?

**Only the rect. Not the texture, not the camera, not the UVs.**

The current policy is UV 0..1 with a deliberate comment: "the console renders the target at
exactly this rect's pixel size, so there is no fit policy to get wrong and no letterboxing"
(`native/organon-console/src/conversation_view.rs:1500-1501`). The rule above preserves that
property *at both endpoints* and relaxes it only mid-flight, where the quad is a uniformly scaled
copy of the destination image.

That is also the right answer aesthetically, and it is James's own metaphor that settles it: **a
photo growing in a gallery does not reframe.** If the camera were re-framed per frame from the
interpolating aspect, the animation would read as a dolly — the content would change as it grew,
which is not what selecting a photo does.

So the aspect band question does not arise during the animation at all. It arises **twice**, at
the two endpoints, and there it is already handled:

- for a **substrate** portal, `SubstrateRig::frame_plane(SUBSTRATE_EXTENT, SUBSTRATE_FOV_DEG,
  aspect)` is re-framed per target exactly as `render_backdrop` and `render_surfaces` already do
  it. `ASPECT_MIN` / `ASPECT_MAX` (`native/src/substrate_camera.rs`) are documented as a *tested
  band, not a clamp*, and a portal's aspect at any sane size sits inside 0.1–10 with room to
  spare;
- for a **World** portal there is no framing function — aspect enters `build_uniforms` directly
  and the FOV is vertical, so a wider target simply shows more world horizontally, which is what
  growing a window should do.

📌 One geometric note worth having, in case a future tier wants a *crop* rather than a scale:
with a fixed **vertical** FOV, horizontally cropping a wide render down to a narrower aspect is
*exactly* the picture the engine would have drawn at that narrower aspect. A vertical crop is not
— that would need a narrower FOV. So a horizontal-only UV crop is a legitimate, exact tool; a
vertical one is a lie. Not needed by the recommendation above, but the day someone reaches for
`uv` on this quad, that is the rule.

---

## 5. Floating over a scroll area

### Recommendation: **not `egui::Area`.** A rect in the `CentralPanel`, registered after the content.

The tree has one `Area` (`native/organon-console/src/tabs.rs:184`, the harness menu) and its comment
carries the hazard: *"it still looked right only because egui clamps an `Area` back inside the
screen rect — so the position was a fallback, not a placement"*. An `Area` is positioned in screen
coordinates and silently clamped; a portal positioned from a constant rather than derived from the
pane rect would be right by accident and wrong after any chrome change.

The layout trap you asked about — `ScrollArea` inside `Layout::bottom_up` collapsing the column,
measured at **684 pt of a 684 pt pane** — is a *placer* defect: the scroll area places itself at
`available_rect_before_wrap().min` while the bottom-up cursor is at the bottom, and allocates
everything between. An `Area` does not go through the placer at all, so it is immune. **But so is
the recommended alternative**, for the same reason `allocate_ui_with_layout` is the workaround
there: a rect derived from the pane and painted into is not laid out either.

The positive reason to prefer a plain rect is interaction, and it is already documented in-tree.
`scene_input`'s module doc records the arrangement that works: *"in workstation mode the pane
registers **after** the scroll area, and egui breaks a tie by taking the topmost"*. That is a
tested property of the exact mechanism the portal needs. An `Area` is a separate layer, and
`press_belongs_to_the_scene` counts interactive widgets under the pointer to arbitrate immersive
mode — introducing a second layer makes that count a different question than the one it was
measured against.

Painting order also comes free: within one layer, painter order is draw order, so painting the
portal after `term_view::draw` (or after `conversation_view::draw`) puts it over the glyphs with
no z-order machinery.

### 🚨 The hazard that actually matters, and it is not the layout

**The terminal front-end does not use egui's hit test for the wheel or the keyboard.**

```rust
// The terminal owns the keyboard, full stop (T1: no other widget exists).
let events = ui.input(|i| i.events.clone());
…
let scroll = ui.input(|i| i.raw_scroll_delta.y);
```

(`native/organon-console/src/term_view.rs:597-648`.) Raw input, read directly. **egui layer order is
irrelevant to code that reads raw input** — an `Area`, a later-registered rect, a modal, none of
them would stop the wheel scrolling the transcript out from under a portal or stop a keystroke
reaching the PTY.

The existing answer is an explicit rect test, and it is exactly the precedent to copy:
`block_panel::pointer_inside(&panel_placements(..), pointer)`
(`native/organon-console/src/block_panel.rs:241`), fed into `term_view::draw` and consulted *before*
the wheel is applied — deliberately against the pre-wheel view state, "because the pointer is over
what is on the screen right now". Note that today it is fed `panel_placements`, not every patch:
**a scene patch deliberately does not claim the wheel**, on the reasoning that "a scene patch is
something to look at, and the wheel over it must keep scrolling the page, exactly as the wheel over
a paragraph does."

**A portal reverses that decision** — the wheel over it must zoom the camera. That is a real
behavioural change to a documented rule, not an oversight to fix, and it should be argued in the
change rather than slipped in: a scene patch is a picture, a portal is an instrument.

### On "everything flows around it"

📌 I read this as: **content scrolls underneath and past the portal; the portal holds its screen
position.** Not newspaper-style text wrap. Nothing in the code suggests otherwise, and two things
argue for it strongly — the terminal's grid is a fixed cell lattice with no notion of an exclusion
region, and `block_anchor`/`block_quads` already implement the *other* anchor (rows reserved in the
buffer, the picture scrolling with them) for the case where you do want the text to move aside.
Screen-anchored is the complement, and its whole point is that the text does **not** move.

⚠️ The visible consequence, worth deciding on purpose: a screen-anchored portal **occludes**
transcript rows. Rows behind it are drawn and then covered. That is fine for a portal you can
dismiss, and it is what "float" means; it is not fine as a permanent state, which is another
reason the CLI verb should be able to close it as easily as open it.

---

## 6. Input

### Camera: the path exists, is tested, is winit-free, and is reusable as-is

`native/src/scene_input.rs` is precisely this problem, already solved for the standalone editor.
Its module doc explains why `PointerRouter` cannot work in a host that draws a `CentralPanel`
(`unused_rect` is `Rect::NOTHING`, so `wants_pointer_input()` is unconditionally true everywhere)
and why egui's own widget hit test is the authority instead. The console draws a `CentralPanel`,
so it is in exactly the host shape that argument was written for.

The working chain, three calls:

| Step | Where |
|---|---|
| register the rect, accumulate orbit + zoom | `scene_input::scene_viewport(ui, rect, mode, &mut st)` |
| convert points → physical px | `scene_input::orbit_pixels(delta, ctx.pixels_per_point())`, called inside the above |
| drain into the world, once per frame, after the UI and before the next render | `for input in st.gesture.take().inputs() { world.apply_camera_input(input) }` — the precedent is `native/src/wgpu_editor.rs:751-753` |

Four properties come free and are the reason not to write a second one: drag **capture** (the
gesture survives the pointer wandering off the rect), **arbitration** (egui prefers a small
control over a big drag background), **no fight with the ScrollArea** (registering after it wins
the tie), and **coordinates that cannot go stale** (`drag_delta()` is screen-space motion, so a
scrolled or offset pane needs no mapping). The wheel is *consumed* from inside — `scene_viewport`
zeroes `smooth_scroll_delta` and `raw_scroll_delta` — which is what stops a zoom also scrolling
the page, and which conveniently also blinds `term_view`'s raw read for that frame.

### 🚨 The trap: a substrate rig makes the camera inert, silently

```rust
let substrate = self.substrate_rig;
let (cam_center, yaw, pitch, distance, cam_roll, fov_deg) = if let Some(rig) = substrate {
    rig
} else if self.rails_ride { …
```

(`native/src/world.rs:6525-6528`.) The substrate arm is tried **first** and returns the whole
six-tuple. `World::apply_camera_input` writes `self.yaw`, `self.pitch` and `self.distance`
(`native/src/world.rs:10230-10252`) — every one of which is discarded while a rig is installed,
and `render_backdrop` / `render_surfaces` both install one before every substrate render.

**So an orbit drag on a substrate portal does nothing, with no error, no log line and a green
build.** It will read as "the input isn't wired up" and the search will start in the wrong file.

Two ways out, and they are a design choice, not a bug fix:

- ✅ **Recommended: the portal shows the `World`, not the substrate.** This is what James asked
  for — "a portal into a 3D world", "control Organon from the shell". The World arm calls
  `set_substrate_rig(None)`, so orbit, zoom and the auto-orbit all work exactly as they do in the
  visual, and the entire `organon set` / `generator` / `recipe` lane is live in it for free (§8).
  The substrate remains what it is: the console's *material*, for the backdrop and for `/surface`.
- Alternative: teach `SubstrateRig` an orbit offset so a substrate portal can be nudged. Real work,
  and it fights `frame_plane`'s coverage guarantee — the whole point of that rig is that the plane
  exactly fills the target, and orbiting it breaks that. Not recommended.

📌 A consequence worth stating: **the portal and the backdrop then want different sources.** In
Portal state the backdrop is Off and the portal wants `World`; `render_source` currently promotes
Off → `Substrate` only, and only when `patches_want_image()`. It needs a third input — "a portal is
open and wants source X". That is a one-function extension of the exact seam the 2026-08-11
amendment created, which is the right place for it.

> ✅ **Adopted, 2026-08-13 (landing).** This is `engine_plan`, and it went further than the
> paragraph asked: it returns *both* decisions — the backdrop's source and whether the portal
> renders — from one pure function, so "at most one `World` render per frame" (§3) is a property
> of a single body rather than an agreement between two, and
> `the_engine_is_asked_for_at_most_one_frame` proves it over the whole input space.
>
> ⚠️ **The risk-1 recommendation was taken and it is why the camera works.** The portal shows the
> `World` and `render_portal` calls `set_substrate_rig(None)` immediately before rendering. The
> trap itself is untouched and still live for anything else that installs a rig — it has only
> moved, to `world.rs:6530`.

### Sense: `drag()` vs `click_and_drag()`

`scene_viewport` hardcodes `Sense::drag()`, and the choice is argued: *"A drag-only widget is what
egui's hit-test treats as 'a big background thing', which is what lets it hand a click on top of us
to the button that wants it… Sensing clicks here would make the region the topmost click target and
swallow the interface."*

The portal needs clicks — that is the whole gallery gesture. And in **Portal** state sensing clicks
is correct (it is a bounded object and it *is* the click target); in **Immersive** it would swallow
the overlay, which is the failure that comment describes.

**Recommend: widen `scene_viewport` with a `sense` parameter**, the editor's two call sites passing
`Sense::drag()` verbatim so their behaviour is provably unchanged, and the portal passing
`click_and_drag()` in Portal state and `drag()` in Immersive. Do not add a second overlapping
`ui.interact` with a different id — two widgets on one rect fight in the hit test and the loser is
decided by registration order, which is exactly the kind of thing that works until someone reorders
a call.

### Click vs double-click in egui 0.33

Measured from the source, not assumed:

```rust
pub fn double_clicked_by(&self, button: PointerButton) -> bool {
    self.flags.contains(Flags::CLICKED) && self.ctx.input(|i| i.pointer.button_double_clicked(button))
}
```

(`response.rs:210-213`.) So **`clicked()` fires on the first click immediately** — egui does not
delay a click waiting to see whether a second arrives — and on the second click of a pair, both
`clicked()` and `double_clicked()` are true in the same frame.

**A single click therefore cannot be made to wait on a double.** This is not a problem here,
because James's gesture design already avoids it: click **or** double-click in Portal both mean
"grow to immersive", and the *second* double-click happens in **Immersive**, a different source
state. Transitions keyed on `(state, event)` never see the ambiguous pair. Worth pinning as a test
so a later "let's make single-click do something else in Portal" is caught at the design step
rather than by feel.

---

## 7. The state machine, and escape

### Where it should live

**A new pure module in `organon-console`** — that crate is nih_plug-, wgpu- and World-free, and is
already where every pure, headless-tested piece of console arithmetic lives (`scroll_anchor`,
`block_anchor`, `block_panel::placements`, `strip_content`, `scrim_alpha`). It cannot see
`BackdropSource`, `World` or `Shared`, and it does not need to: the machine is a state and an
event.

```rust
pub enum PortalState { Closed, Portal, Immersive, FullScreen }
pub enum PortalEvent { Open, Close, Click, DoubleClick, Escape }
pub fn step(state: PortalState, event: PortalEvent) -> PortalState;
```

Plus the animation as a second pure function — a source rect, a destination rect, a normalised
`t`, an easing curve, and `fn rect_at(from, to, t) -> egui::Rect`. Both are exhaustively testable
with no GPU, no window and no agent, which is the bar `CLAUDE.md` sets for cloud sessions.

`console_main.rs` maps the resulting state onto the two render decisions (which source the engine
draws, whether the backdrop paints) and onto the winit fullscreen request. That mapping is a match,
and it is where the "at most one World render per frame" invariant of §3 becomes visible.

### Escape — it works, with two conditions

**egui does not consume Escape.** `Memory` clears `focused_widget` on it
(`memory/mod.rs:560-562`) and `interaction.rs:137` aborts an in-flight drag on it, but neither
removes the event, so `ctx.input(|i| i.key_pressed(Key::Escape))` fires. The composer's own
predicate ignores it too — `composer_key` returns `Ignore` for everything that is not Enter, and
`any_other_key_falls_through` pins Escape explicitly.

The two conditions:

1. 🚨 **In a terminal tab, Escape belongs to the child.** `term_view::draw` forwards every key
   event to the PTY — "the terminal owns the keyboard, full stop". `vim` needs Escape. So the
   portal's handler must **consume** it, and only in Immersive/FullScreen:
   `ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))`. `count_and_consume_key`
   (`input_state/mod.rs:759-779`) `retain`s the event out of `i.events`, which is the exact vector
   `term_view` clones — so consuming genuinely removes it from the PTY stream. It must run inside
   `egui_ctx.run` **before** the `CentralPanel`, which is where `tabs::command_key_action` already
   reads keys (`native/src/console_main.rs:2440-2449`) and is the natural home.
2. ⚠️ `consume_key` matches modifiers **logically**, so it also eats Shift+Escape and Alt+Escape.
   Harmless in practice; if it ever matters, read `i.events` and test `matches_exact(NONE)` —
   the same inversion the composer already had to make for Shift+Enter.

📌 The general point, and the addressable-surfaces doc already names it as an open problem: *"A
pointer/wheel/keyboard arbiter exists between an addressable region and the PTY (nothing exists
today; the console has no focus concept at all)."* The portal does not need a general arbiter — it
needs one conditional consume and one pointer-rect test — but it is the first thing in the console
to need **state-dependent** key ownership, and that is worth naming when it lands.

---

## 8. The CLI as the opener

### The path today, end to end

| Hop | Where |
|---|---|
| clap subcommand | `ConsoleAction` in `native/src/bin/ctl.rs:282`, dispatched by `run_console` at `:529` |
| wire form | `cli::console_op_to_line` / `cli::parse_console_op` — `<verb> <word>`, one line each |
| transport | `cli::console_cmd_path()` → `ipc::ns_file("console.txt")`, i.e. `$TMPDIR/<namespace>-console.txt` |
| drain | `Console::drain_console`, once per frame, before the snapshot is published, on the same file-length cursor pair the World uses for `cli.txt` |
| validate + log | `Console::dispatch_console` builds a `CommandService` per batch, registers `console_specs()`, dispatches as `Issuer::Worker("organon-cli")` |
| apply | `Console::apply_console` |

And crucially: `term.rs:195` sets `cmd.env("ORGANON_IPC_NS", organon_core::ipc::namespace())` on
every PTY child. **A command typed in a console tab reaches *this* console**, which is what makes
"open the window from the shell" a real gesture rather than a demo.

### Is "open a window" expressible? Yes, and the precedent is `patch`, not `background`

`ConsoleOp` (`native/src/cli.rs:268`) already has two members that change **the transcript** rather
than the dressing — `Block` and `Patch` — and `apply_console` routes them *before* `console_step`
precisely because "that function's domain is `(source, look)` and a block has neither". A portal
has neither either. So:

- a `ConsoleOp::Portal { … }` variant, routed in `apply_console` alongside `Block`/`Patch`;
- a `console.portal` `CommandSpec` in `console_specs()`, with its argument slots named on the same
  rule the existing ones follow (a distinct slot name per kind — `rows` is an `Int`, `name` is a
  `Choice`, and reusing one for the other is how a palette starts describing the wrong thing);
- a clap `ConsoleAction::Portal` whose `PossibleValuesParser` binds to **the same constant table**
  the spec reads, the way `PATCH_KIND_WORDS` binds `cli::PatchKind` to both sides today.

📌 **§5.9.25's rule is satisfied by construction if you do that, and violated by any shortcut.**
"One vocabulary, many renderings — the CLI, the agent's catalog, `doc/reference/`, and MCP as a
fourth if it earns its place. A hand-written MCP server beside a hand-maintained CLI is exactly the
failure this tree already paid for." A portal verb that exists in clap but not in `console_specs()`
would be invisible to the palette and to `--discover`, which is the same failure one layer down.

Two inherited properties worth knowing:

- ✅ **The out-of-band drain problem does not apply.** `open_block` and `claim_patch` both carry a
  warning that the sidecar is drained once per frame and is out of band with the PTY byte stream,
  so "the line the cursor is on now" is resolved at drain time and is only correct while the child
  is idle. A **screen-anchored** portal resolves against the screen, not against a line. The
  hardest-won caveat in Tier 5 simply evaporates. That is a genuine argument for building the
  screen anchor next rather than more scroll-anchored things.
- ⚠️ **There is no return path.** `organon console <verb>` is fire-and-forget; an unknown verb
  vanishes silently by design (`parse_console_op` returns `None`, "which is that format's whole
  versioning story"). So `organon console portal open` cannot tell you it opened one. §5.9.25 point
  1 flags the return path as something the pivot gains; the portal does not need it, but a user
  will expect it.

### 🚨 "Control Organon from the shell" is already true — for free

This is the single most valuable thing in this document. The World's own parameter-override lane
drains **inside `World::frame_body`** (`native/src/world.rs:9738-9754`, via `agent::cli_drain_step`
on `cli.txt`), and `render_to_texture` runs `frame_body`. So the moment a portal renders the World:

- `organon set …`, `organon generator …`, `organon recipe …` typed in a console tab reach that
  World, through the namespace the console injected, with **no new code at all**;
- the execution plan's Tier 1 note becomes the argument for this design rather than a caveat:
  *"The World stays selectable as a backdrop source beside the substrate — replacing it kills the
  live `organon set/generator/recipe` response."*

James asked to "open the window and control Organon from the shell". Opening is a small new verb.
**Controlling is already built and shipped, and the only thing standing between it and being
visible is a rectangle to look at it through.**

---

## 9. `Shared` / IPC, and whether the portal wants its own `World`

### The publish-and-restore dance

```rust
if let Some(writer) = self.shared_writer.as_mut() { writer.write(*surface_shared(&desired)); }
… render …
if budget < SURFACE_RENDERS_PER_FRAME {
    if let Some(writer) = self.shared_writer.as_mut() { writer.write(*published); }
}
```

(`native/src/console_main.rs:2205-2230`.) The World has exactly one way to learn what to draw — the
snapshot — so a surface's look is published, the frame is taken, and the console's own snapshot is
put back. The restore is not tidiness: without it `organon status` / `organon get` would report
whichever surface rendered last, "a lane nobody typed into, describing a picture that is not the
window."

**A World portal needs none of this.** It shows the console's own snapshot, which is already
published at `:2337-2339` before any rendering happens. No override, no restore, no risk of the
CLI reporting the wrong lane. The portal is *simpler* than a surface here — one more case where the
World choice pays.

(A *substrate* portal would need the dance, exactly as a surface does. One more reason not to.)

### A second `World`: don't

`render_surfaces`' own doc prices it: *"A second one would recompile ~50 shaders and ~62 pipelines
and duplicate every sim buffer, to draw the same plane."* The console constructs its one `World` in
`Console::new` and hands it the device at `attach_gpu`; it renders only into textures, never the
swapchain.

Beyond the init cost, a second World would be a second `Shared` reader and a second CLI drain — so
`organon set` would reach one of them and not the other, and which one would depend on frame order.
That is a worse problem than anything a second World solves.

**The one-World arrangement plus the §3 invariant (at most one render per frame) is the whole
answer.** Per-frame safety for a live portal is not in question; what needed answering was the
double-render, and the state machine answers it.

---

## 10. Does this satisfy James's own ruling?

His 2026-08-11 amendment, recorded in both the execution plan and the demo script:

> *"Setting the background of a terminal is nothing. People have been doing that for years. It
> doesn't communicate what we are really doing — in fact it breaks the whole concept that we're
> creating an illusion that we are in a terminal."*

with two consequences: the console **opens indistinguishable from an ordinary terminal**, and the
reveal is **the patch, not the surface** — the material arrives only when something asks for it.

### The reading in the ask is correct, and the code supports it

Under this design the backdrop is never the opening state. It is a **destination reached by acting
on an object in the page** — the portal is summoned by a verb, and immersive is reached by clicking
the portal. That is the amendment's own structure ("summoned, never imposed"), applied one level up:
the *patch* is now the thing that can grow into the surface, so even the surface has a summoning
gesture in front of it. If anything this is a stronger reading of the ruling than the tree currently
implements, because today `background world` paints the window with nothing to have clicked.

### 🔍 Is the backdrop hard-coded as a startup-only mode? **No — checked, and this is good news.**

`backdrop_source` is *seeded* from the environment once
(`parse_backdrop_source(std::env::var("ORGANON_SHELL_BACKDROP")…)`, `native/src/console_main.rs:1055`),
but it is a plain field that `apply_console` writes at runtime (`:1546-1548`). `console_step` is
total over `world` / `off` / `substrate` — `BACKDROP_SOURCE_WORDS` — and its transitions are
directly tested (`native/src/console_main.rs:2898-2953`), including `substrate → world → off →
substrate`. `render_backdrop`'s `Off` arm even clears the substrate rig specifically because the
source *can* become `off` at runtime and a stale rig would frame a plane nobody is drawing.

So **entering immersive is a call that already ships**, and it is exercised. Nothing has to be
un-hard-coded.

### One challenge to the reading, and a recommendation

⚠️ **Immersive is, visually, precisely the thing the amendment demoted** — a terminal with a
picture behind the text and a scrim over it. James's own words license it, so it is his call and
the design is faithful to what he asked for. But the property that keeps it honest is entirely
about *how you got there*, and there is exactly one path that would break it:

📌 **Recommend: `ORGANON_SHELL_BACKDROP=1` (and `=substrate`) should stop being a way to *start* in
the backdrop.** It is the one route that puts the console back in the state the amendment forbids —
opening as a picture with text on it — and it is the route that already did so once, via
`organon-console.cmd` forcing the value (removed 2026-08-11). Keep the flag as a developer escape
hatch if it earns it, but the shipped console should reach a lit window only through a gesture. If
that is agreed, it is a one-line change and a `--help` edit, and it should land *with* the portal
rather than after it, because the portal is what makes the flag unnecessary.

---

## 11. Recommended shape

**A screen-anchored, live, orbitable `World` portal, painted in the `CentralPanel` after the
front-end, driven by a pure state machine in `organon-console`, opened by a `console.portal` verb.**

1. **`portal.rs` in `organon-console`** — `PortalState`, `PortalEvent`, `step`, and the rect
   interpolation. Pure, headless, exhaustively tested. No egui state, no World, no `Shared`.
2. **`Console::portal: Option<SurfaceTexture>`** beside `backdrop`, plus the portal's own state and
   in-flight animation. Reuses `make_surface_texture` and the free-and-log body; `SurfaceKey`,
   `SurfaceImages` and the `conversation_view` seam are untouched.
3. **`render_source` gains a third input** — "a portal is open and wants source X" — extending the
   seam the 2026-08-11 amendment created rather than inventing a second one.
4. **The state machine is also the render budget**: Portal ⇒ backdrop off, portal renders;
   Immersive/FullScreen ⇒ backdrop renders, portal paints the backdrop's texture and its own
   texture is released. At most one World render per frame, always.
5. **Allocate at the destination size, scale the quad, reallocate once on settle.** Extend the same
   settle rule to window-resize drags and the log-spam finding closes with it.
6. **Input via `scene_input`**, with `scene_viewport` widened by a `Sense` parameter (editors pass
   what they pass today). In a terminal tab, feed the portal's rect into `term_view::draw` on
   `block_panel::pointer_inside`'s pattern so the wheel and the pointer are claimed explicitly —
   layer order will not do it.
7. **Escape consumed conditionally**, in the pre-panel key block, only in Immersive/FullScreen.
8. **`console.portal` as a `CommandSpec` first**, clap second, both reading one table.
9. **Document it in `CONSOLE_ARCHITECTURE.md` in the same change** — the doc hook watches
   `organon-console/src/*.rs` and `console_main.rs` is covered by the root architecture rule.

### Honest cost

| Piece | Size | Confidence |
|---|---|---|
| the state machine + animation curve, pure | small | high — it is arithmetic with tests |
| the portal texture, its render, its paint | small | high — it is `render_surfaces` minus eviction |
| `render_source` third input | trivial | high |
| the destination-size / settle allocation rule | small-to-medium | medium — it touches the surface path too, and that path has tests to keep green |
| camera input wiring | small | high *if* the portal shows the World; **medium-to-large** if it shows the substrate (see risk 1) |
| terminal-tab pointer + Escape arbitration | medium | medium — no focus concept exists; this is the first state-dependent key ownership in the console |
| the full-screen overlay suppression | medium | medium — new branch, plus `winit` fullscreen, plus deciding what happens to the tab strip |
| immersive in a **conversation** tab | medium | low-to-medium — that front-end has no backdrop at all today |
| the CLI verb | small | high — `Patch` is the template |

### The top three risks, ranked

1. 🚨 **The substrate rig silently swallows camera input.** `world.rs:6526` returns the rig's whole
   six-tuple before `yaw`/`pitch`/`distance` are ever read. A portal showing the substrate will
   have a drag that does nothing, with a green build and no log line, and the investigation will
   start in `scene_input.rs` — which will be correct. **Mitigation: the portal shows the `World`.**
   It is what was asked for, it makes the camera work, it makes the CLI param lane work, and it
   removes the publish/restore dance. Decide this first; it is upstream of most other choices.
2. 🚨 **Per-frame texture churn through every transition.** Free + reallocate + re-register + one
   unconditional log line, on every frame of every animation and every frame of every window-resize
   drag. **Mitigation: allocate for the destination, scale the quad, reallocate once on settle.**
   Do not silence the log — the rule it enforces is the reason surface bugs in this tree are
   findable.
3. ⚠️ **Input arbitration in a terminal tab.** The terminal owns the keyboard "full stop" and reads
   the wheel from raw input; the console has no focus concept at all, and the addressable-surfaces
   doc already lists the arbiter as an unbuilt prerequisite. Getting this wrong does not look like
   a portal bug — it looks like `vim` stopped taking Escape, or like the scrollback stopped
   scrolling. **Mitigation: explicit rect and state tests on `pointer_inside`'s pattern, and a
   test for each of the four states × (wheel, Escape).**

### What is nearly free, stated bluntly

- **Immersive.** Already renders, already scrims, already switches at runtime.
- **CLI control of the portal's contents.** Already drains, already namespaced into every tab.
- **The render target, its sizing, its registration and its budget accounting.** `/surface` built
  all of it, including the points-not-pixels seam that is the thing most likely to be got wrong.
- **The camera gesture itself** — capture, arbitration, unit conversion, the ScrollArea tie — all
  written, tested and winit-free in `scene_input`.

### What is genuinely new, stated equally bluntly

- **A screen anchor.** Every anchor in the console today is a scroll anchor or a whole pane.
- **A live surface.** Everything rendered into a target today is a still life, and the
  still-life property is what bounds the engine cost.
- **Animation.** Nothing in the console animates a rect. There is no easing, no in-flight state,
  and the allocation path assumes size changes are rare.
- **State-dependent input ownership.** The first thing in the console that must take a key away
  from the PTY *sometimes*.
- **Overlay suppression.** No path hides the glyph layer or the chrome.
