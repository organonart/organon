# Physically based text — the terminal screensaver as a lit object

> **Status: DESIGN. Nothing here is built.** This document is the argument and the contract,
> written before any code so the two repositories involved can be worked against the same seam
> rather than against each other. Every claim below is marked **measured** (I ran it against the
> tree) or **reasoned** (it follows, and nobody has checked); §12 is the ledger, and it is the
> first thing to read if you are about to depend on a sentence here.
>
> James, 2026-09-02:
> *"Omarchy has this great screensaver that uses the terminal text effects package… it's very
> beautiful because it is very retro looking. What I want to do is keep the retro look, but figure
> out how to re-implement each of the glyphs using physically based rendering in Organon… take the
> glyph and extrude them very slightly, just these very slightly beveled tiles. And then if we can
> figure that out once we can apply it to every glyph, and we could even use it for rendering in
> Organon on screen, as a way to have a generally upgraded text. What if all our text is physically
> based rendered?"*
>
> **Reading order.** `ARCHITECTURE.md` §17 (the param chain) and `doc/arch/render.md` (the pass
> structure) are assumed. `CONSOLE_ARCHITECTURE.md` §1.14's producer seam is the model for §6's
> channel. Nothing here contradicts invariant #2 — no `Shared` field is added — and §6 explains
> why not.
>
> ⚠️ **Corrected 2026-09-02, same day, before any code.** The first version assumed `ttfx` might
> be a rename of the Python package and recommended tapping it through a PTY. James settled it:
> **`ttfx` is a Rust port of terminaltexteffects, written by DHH by having an agent port the
> Python** (`omacom/ttfx`, forked to `organonart/ttfx`). That inverts §2, retires §7's biggest
> risk, and — once its output was actually run — turned out to falsify half of §8. Every section
> that changed says so in place; §12 carries the new measurements.

---

## 1. Two products, one mechanism

This looks like a screensaver project and it is not. Organon Console's terminal grid
(`organon-console/src/term_view.rs`) is drawn today through egui's text painter, and that file's
own module doc names the gap: *"the dedicated glyph-atlas instanced pipeline (the perf ceiling) is
a later tier of #10."* The screensaver and the Console's terminal want the same thing — **a grid of
cells rendered as lit geometry instead of as blitted coverage bitmaps** — and building it once pays
for both.

📌 **So the screensaver is the forcing function, not the deliverable.** It is the right forcing
function because it is visually unforgiving, it has no interaction latency budget, and it has a
public that already loves the thing being replaced. If the result is not obviously better than a
terminal, the project has failed in a way a Console tab would have let us hide.

---

## 2. The seam is the cell grid, not the effect code

**Measured.** `terminaltexteffects` is ~7.6k lines of engine and ~13k lines of effects across 36
files. The effects are where the beauty is, they are Python, and reimplementing them in Rust would
be months of work with no leverage.

It is also unnecessary. Every effect in TTE resolves to the same output:
`Terminal._update_terminal_state()` walks the visible characters in layer order and writes each
one's `CharacterVisual` — **a symbol, a foreground colour, a background colour, and SGR flags** —
into a cell of a rectangular buffer. That buffer is the interchange format, and it sits below every
effect rather than beside them.

**And the thing Omarchy actually runs is not Python.** `ttfx` (James, 2026-09-02: DHH's Rust
port of TTE, agent-written from the Python) is a **Cargo crate with a library target**, not a
binary-only tool. **Measured** against `organonart/ttfx @ 7203e35` (v0.3.2):

| | |
|---|---|
| Licence | **MIT** — `LICENSE` carries both copyrights (37signals / omacom-io; ChrisBuilds for TTE), `NOTICE` says every effect and the engine are TTE's design |
| Shape | `src/lib.rs` exports `cli`, `effects`, `engine`, `utils`; `main.rs` is a thin driver over them. Three deps: `clap`, `clap_complete`, `terminal_size` |
| Cell model | `engine::animation::CharacterVisual` — `symbol: String`, eight SGR bools, `colors: Option<ColorPair>`, fg/bg codes — mirrors Python's field for field. `Terminal::update_terminal_state` is the same painter's walk (`(layer, character_id)` max per cell) |
| Coordinate | `utils::geometry::Coord { column: i64, row: i64 }` — integer, same as Python; §7 |
| Frame loop | `Effect::next_frame(&mut EngineCtx) -> Option<String>` — one ANSI string per tick, **you own the tick**; `Clock::Virtual` advances a fixed `dt` per frame with no sleeping |
| Determinism | `--seed` / `Rng::seeded` — deterministic within ttfx (not bit-identical to Python's Mersenne Twister, by design) |
| Distribution | **not on crates.io** (`cargo search ttfx` finds only an unrelated `ttfx-rs`); a **git dependency** with a pinned `rev`, which is how this workspace already takes `nih_plug` and `baseview` |
| Platform | README says Linux and macOS; `plan.md` says Linux only. `cargo check` and `cargo build --release` **pass on Windows** (measured here), and `--parity-dump` ran every effect headless on Windows. The Unix-only part is the signal plumbing in `lib.rs`, which a library caller never invokes |

So the two routes are now:

| | How | Cost | What it buys |
|---|---|---|---|
| **Link it** *(preferred)* | `ttfx = { git = …, rev = … }`; build an `EffectCommand`, tick `next_frame`, read the grid out of `ctx.terminal` | A small producer crate. No PTY, no ANSI encode, no reparse of output we generated a microsecond earlier | Everything the buffer knows and the terminal forgets: **which character** is in a cell (`character_id`), its `layer`, whether a path is active (`motion.active_path`, the teleport-vs-slide signal §7 needs), `previous_coord`, and a tick we control |
| **PTY tap** | Run `ttfx` under a pseudo-terminal and read cells out of `alacritty_terminal` | Almost nothing — `organon-console/src/term.rs` already does this | Zero coupling to ttfx's internals. Keep it as the **process-boundary fallback** if the licence question ever changes, and as the route to the Console's own terminal, which is a real PTY anyway |

⚠️ **The grid is readable without patching ttfx, but not through the obvious field.**
`Terminal.terminal_state: Vec<String>` is public and is the *formatted* rows — colour already
folded into ANSI bytes. `render_cells` and `visible_characters` are private. What is public is
`arena: Vec<EffectCharacter>` with `is_visible`, `layer`, `character_id`,
`motion.current_coord` and `animation.current_character_visual` — enough to rebuild the painter's
walk in ~15 lines on our side. A `pub fn` exposing `render_cells` upstream is a one-line PR and
worth sending, but the producer must not wait on it.

📌 **What the library route does *not* buy is colour precision.** The first draft of this
correction assumed the PTY round-trip was quantizing colours; it is not. `Color` stores
`rgb: [u8; 3]` and gradients resolve to 8-bit at the source, so a truecolor PTY carries them
losslessly. The only quantization is `--xterm-colors`, which Omarchy's *provisioning splash*
passes (the framebuffer console crushes 24-bit codes — the "muddy lavender" comment in
`bin/omarchy-provision-owner`) and its **screensaver does not** (`bin/omarchy-screensaver`). §4's
`srgb_to_linear` decode applies identically on either route.

🚨 **Link it, from a crate that is not in the render process.** §6 says where.

---

## 3. The glyph problem is ~5% of what it looks like

The instinct is "solve one glyph, apply to all glyphs." **Measured**: for this content you never
need a letterform at all.

```
$ python3 -c "import collections; print(collections.Counter(open('logo.txt').read()))"
Counter({'█': 337, ' ': 312, '▄': 32, '▀': 32, '\n': 10})
```

Omarchy's logo is **three glyphs**: full block, upper half block, lower half block. And an
inventory of the symbols the 36 effects substitute in during animation is overwhelmingly the same
family — `█ ▓ ▒ ░ ▁▂▃▄▅▆▇ ▌▍▎▏ ▖▙▜▝` plus a dozen ASCII punctuation marks.

Every one of those is an **axis-aligned sub-cell rectangle**. Which is to say: they are already the
beveled box, differing only in offset and extent. No font, no outline extraction, no tessellation,
no SDF.

📌 **The shade blocks are an upgrade rather than a workaround.** `░▒▓█` is a coverage ramp, so map
it to **extrusion depth** — 25 / 50 / 75 / 100%. The dithered fade that reads as stipple in a
terminal becomes a physical height field that catches light differently at each level. This is free
and it is better than what it replaces.

⚠️ **Real letterform extrusion is a separate project** — outline extraction (`ab_glyph` already
gives us these; `organon-world/src/overlay.rs` rasterizes a CPU atlas with it today), contour
tessellation, hole handling, mitre generation, caching. Keep it, aim it at the Console's terminal,
and do **not** make it a prerequisite for the screensaver.

---

## 4. Colour is emission, not albedo

This is the decision most likely to be got wrong, and getting it wrong is the most likely reason a
first attempt looks *worse* than a terminal.

A TTE colour is **display-referred**: it already encodes "this is how bright this looks on screen."
It is the output of a lighting model, not a material property. Feeding it in as albedo fails three
ways at once, and the three compound:

1. **Albedo is bounded and multiplicative.** Final radiance ≈ albedo × irradiance, and irradiance
   is below 1.0 across most of the hemisphere under any sane environment. So albedo can only make
   things *darker* than the source colour. TTE's display-maximum green becomes a dim sage. Every
   colour loses its top end at once, which reads as washed out, and the reflex fix — crank exposure
   — blows out everything else in frame.
2. **Albedo is chromatically filtered by the light.** A saturated green reflectance under a warm
   HDRI goes olive. TTE's colours were authored as absolutes.
3. **Shading destroys the gradient's shape.** The whole point of an effect is that cell *(r,c)* is
   a specific colour at frame *t*; `N·L` modulates that by geometry, a signal with nothing to do
   with the effect. This is the failure people feel without naming: the animation stops being
   crisp.

🚨 **Emission is not a workaround, it is the correct physics.** A terminal is an emissive display
and a phosphor is an emitter. The honest model of a glyph is *an emissive element behind a
dielectric front surface* — which is what a CRT is.

```
albedo    = near-black dielectric, 0.02–0.04   // the faceplate, not the light
metallic  = 0
roughness = 0.15–0.30                          // the sheen of the envelope
emissive  = srgb_to_linear(tte_rgb) * gain     // the phosphor
```

Three specifics against this tree:

⚠️ **`srgb_to_linear` first, always.** TTE emits sRGB-encoded bytes. Skipping the decode makes
mid-tones roughly 2× too bright and bends every gradient. It is the classic version of this bug.

⚠️ **The existing emission path is albedo-modulated and therefore unusable as-is.**
`cube.wgsl:1544` computes `emissive = albedo * (glow + u.env_tint.w) + ripple_emission(…)`. With a
near-black albedo that multiplies the phosphor to nothing. Per-instance attributes today are a
`mat4` at locations 3–6 plus `tint: vec4` at location 7, and `tint` multiplies albedo. **A
per-instance `emit: vec4` at location 8, bypassing albedo, is the change.** It is a vertex-layout
addition local to the cube pipeline — no `Shared` field, no `LAYOUT_VERSION` bump, invariant #2
untouched.

📌 **Denominate gain in SDR-white units, because the EDR path is real.** `organon-visual`'s
`hdr_macos.rs` / `hdr_windows.rs` give extended-linear output with genuine display headroom and
`composite.wgsl` rolls highlights toward `hdr_max` rather than clamping. A phosphor at 3–6× paper
white is the moment it stops looking like a picture of a screen. **It is the one thing the terminal
being replaced fundamentally cannot do.**

### 4.1 What emission unlocks that albedo never would

**Bloom starts working by itself.** `post.wgsl`'s `prefilter()` is a soft-knee bright-pass at a
threshold. With emission above 1.0 only the lit glyphs cross it — exactly the right pixels,
automatically. Albedo-driven colour never crosses, so you compensate with global bloom and fog the
image. Same failure as (1), different door.

**Glyphs become light sources.** `cube.wgsl:201` — *"emissive cubes as real lights (#167 Tier 3)"* —
the visual uploads the brightest N nodes as up to 64 point lights and the shader adds real
Cook-Torrance direct lighting from them. That is the green pool spilling onto the backplane, and it
is the strongest CRT cue after halation. Sixteen thousand cells obviously cannot each be a light;
the existing brightest-N selection is already the right shape, and the DDGI probes (`gi.rs`) handle
emissive geometry volumetrically for the diffuse part.

**Dark cells still show the room.** With a Clearcoat or Glass front layer the environment specular
is *independent of emission*, so an unlit cell carries a faint sheen of the HDRI — exactly like the
switched-off region of a real faceplate. A terminal renders an unlit cell as literally `#000000`.
This single cue does an enormous amount of work.

---

## 5. Geometry: impostor, mesh, and the RT constraint

⚠️ **"Impostor" in this tree means *analytic*** — sphere and capsule, closed-form or sphere-traced
in a fragment shader (`particles.wgsl:825`, the plexus Tier 2 path). A beveled glyph tile has no
closed form. Two real options:

- **Rounded-box mesh instances.** `cube.wgsl:528`'s `round_local()` already does this — a
  rounded-box morph with `bevel` 0 = sharp cube → 1 = sphere, applied in the vertex stage before
  the instance transform. Near-zero new code for §3's block-glyph case.
- **Extruded 2-D SDF raymarched on a quad** — `max(sdf2d(xy), |z| − h)`, trivially composable, and
  the tree has SDF raymarching conventions already (`kifs.wgsl`, `mandelbulb.wgsl`). The correct
  generalization to arbitrary glyphs.

🚨 **The decisive constraint is that impostors are invisible to hardware RT.** `rt_shadow`,
`rt_reflect`, `rt_gi`, `rt_ao` and `rt_caustic` all bind a `wgpu::Tlas`, and only triangles enter a
BLAS. If glyphs are to cast ray-traced shadows on the backplane or appear in reflections — and the
contact shadow in each cell well is probably the highest-value shading term in the whole design —
they must be real geometry. **The hero path is mesh.**

### 5.1 What the bevel will and will not do

**Reasoned, not measured.** A 3 px bevel produces a ~3 px specular band: a rim, not a roll. The
other 95% of the tile is a flat patch with a constant normal, so it reads as flat coloured text with
a bright edge. What sells "light moving across a surface" is **curvature across the face** — a
slight keycap crown, or normal-mapped microstructure. And at a near-orthographic camera the
extruded side walls are never seen, so extrusion depth is nearly irrelevant to the look; a few
degrees of tilt and you get letterpress metal type instead.

⚠️ **Express depth in cell units, never pixels.** "6 px" survives neither a resolution change, a
font-size change, nor a camera move.

---

## 6. The channel: a new ring, not `Shared`

`Shared` is an append-only fixed struct carrying control-rate parameters. A 16k-cell grid at 120 Hz
is neither, and invariant #2 makes its byte offsets load-bearing across every saved DAW session.

📌 **The precedent is established three times over.** `ipc.rs` already carries `mind_ring_path()`
(*"a SEPARATE channel from `Shared`… kept off `Shared` on purpose so Tier 2's model-free slice adds
no `Shared` size/LAYOUT_VERSION change"*), `audio_ring_path()` (*"a continuous high-rate stream, not
a control-rate snapshot"*) and the #554 frame mirror. A **glyph ring** — `ns_file("glyphs.bin")` —
is the same shape and the same argument.

🚨 **It must go through `ns_file`.** A hard-coded `$TMPDIR` path silently breaks the one
cross-product invariant: that a Mind session and an Organon session can run simultaneously.

**Decouple the rates.** The effect ticks at its own cadence; the renderer runs at 120 and
interpolates. With ttfx linked (§2) the cadence is ours — `Clock::Virtual` steps a fixed `dt` per
`next_frame` and never sleeps — so "capped at the effect's frame rate" stops being a constraint
and becomes a dial.

### 6.1 The ring survives the library route — and the producer is its own crate

Linking ttfx does not mean ticking it on the render thread. The ring stays, for the three
reasons it existed: the producer and the renderer run at different rates; a screensaver process
must be killable without the renderer noticing anything but silence; and the Console's terminal
(a real PTY, §1) will feed the *same* ring from a different producer, which is the whole point of
building this once.

📌 **So the producer is a new, small, `MIT OR Apache-2.0` workspace member** — working name
`organon-glyphs` — depending on `organon-core` (for `ns_file`) and on `ttfx` by git `rev`, and on
nothing else. `doc/arch/topology.md`'s rule is that any leaf may grow an edge to `organon-core`,
and this one grows no other. It is the `organic-math-mind-writer` shape — a binary that fills a
ring the visual reads — and the Ascent producer shape one repo over. The ttfx dependency then
touches **no existing crate**: not the GPL root, not `organon-render`, not the Console.

⚠️ **Per-cell, the ring should carry more than a terminal would.** Symbol, fg, bg and SGR flags
are the terminal's atom; the library also knows `character_id`, `layer`, and whether
`motion.active_path` is set. Reserve a per-cell sub-cell offset pair (§7) even before anything
fills it — a ring layout is the kind of thing that is cheap to widen on day one and expensive on
day thirty. The producer is the one place the effect's *identity* model meets the renderer's
*cell* model, and the ring is where that meeting is recorded.

---

## 7. Motion is quantized, and only 3-D reveals it

**Measured, in both trees.** Python: `geometry.Coord` is `column: int, row: int`; `Path.step()`
computes a float distance factor, calls `find_coord_on_line` / `find_coord_on_bezier_curve`, and
the sub-cell remainder is discarded. ttfx: `Coord { column: i64, row: i64 }`; `EngineCtx::path_step`
(`ctx.rs`) computes the same float `t` and the rounding happens inside
`geometry::find_coord_on_line` — `round_half_even` on both axes, banker's rounding to match Python.
The pre-rounded point exists for exactly one expression and is gone.

In a terminal this is invisible — the cell *is* the atom. Rendered as tiles under a camera it reads
as stepping. Three fixes now, ranked cheapest first, and they compose:

- **Interpolate on the Organon side, gated by the library's own signal.** The terminal cannot tell
  a slide from a cut; the library can. `Motion.active_path` is `Some` while a character is on a
  path and `None` when an effect calls `set_coordinate` to teleport it, and `previous_coord` is
  public. Interpolate `previous → current` only while a path is active, and the "slides where it
  should cut" failure of the first draft goes away without touching ttfx. **Tier 1 does this.**
- **Carry the pre-rounded point.** ✅ **Done — W6, organonart/ttfx#1.** `Motion.current_pos:
  (f64, f64)` (the field this section first called `current_point`) is written beside
  `current_coord` at every write: `path_step` now returns the float pair and `motion_move` rounds
  it through `Motion::set_position`, so `round_half_even(current_pos) == current_coord` holds by
  construction; `set_coordinate` sets it to the integer coordinate's exact value, and the two
  direct writes in `matrix.rs` plus the `SetCoordinate` event action go through it. `Motion::
  sub_cell()` is the remainder. The rounded output is unchanged — same banker's rounding at the
  same moment — which is what makes this an upstream PR rather than a fork. **Checked, not
  reasoned:** ttfx's Python-traced engine golden (`engine_traces_match_python`, which logs
  `current_coord` every tick of every motion scenario) passes, and failed with 317 mismatches
  when the new rounding site was deliberately given swapped axes; and every case in
  `tools/parity/cases.txt` at both suite seeds was dumped with `--parity-dump` from a binary
  built at the previous commit and from this one and `cmp`'d byte for byte (numbers in §12).
  ⚠️ The parity suite *proper* is Linux/glibc-pinned and did not run on the Windows box; the
  differential run is the stronger claim for this change anyway, since it measures "unchanged"
  directly rather than through Python. **On the Organon side** the producer fills the ring's
  reserved `sub_x`/`sub_y` with `current_pos − current_coord` (cells, `+y` up on both sides — the
  row *index* is flipped, the remainder is not; `f64→f32` is the only loss), and `lower_grid`
  slides between the two **exact** positions. ⚠️ That last part was not optional: the T1
  consumer already added `sub_x`/`sub_y` after lerping cell *centres*, so the moment a producer
  filled them a character at 0.3 cells/tick would have jumped *back* toward the cell boundary at
  the start of every tick. Zero-sub producers lower byte-identically.
- **Fork.** Not needed on any evidence so far, and the upstream PR above is the proof.

⚠️ **Two things the effects were authored against that a tile grid must not break.** First, the
integer step is also what the effects' *timing* is authored against — `max_steps =
round(total_distance / speed)` — so sub-cell smoothing changes where a character is between
ticks, never when it arrives. Second, **TTE's cell is 2:1**: `find_length_of_line` doubles the row
delta and `find_coords_on_circle` doubles the x offset, so every circle, ring and spiral in the
effect set is authored for a cell twice as tall as it is wide. Render the grid at square tiles
and every ring becomes an ellipse. Keep the cell aspect, and put it in the ring header where the
renderer cannot guess it.

📌 This was "the highest-risk unknown". It became a Tier 1 gate plus an upstream patch, and
**the patch is sent and the field is filled (W6)** — the risk is retired. What remains is
taste, not risk: whether a slide between exact positions reads better than the cell-quantised
one on a real render, which is a GPU question T3's look controls are the place to answer.

---

## 8. A screensaver has time — converge on hold

**Measured.** `world.rs:8458` restarts path-trace accumulation on camera move, buffer resize, or a
change to the settings that decide what the buffer holds. It does **not** restart on geometry
change, and the surrounding comment says a moving field would smear the average.

**Measured 2026-09-02, and half of the original sentence was wrong.** The first draft reasoned
that every effect "animates, resolves to its `final_gradient`, and holds". All 37 ttfx effects were
run headless (`--parity-dump --seed 1`, Omarchy's `logo.txt`, 100×30) and their last frame compared
to the input:

| | |
|---|---|
| Effects whose final frame **is** the input text | **37 / 37** |
| Frames per effect | 54 (`overflow`) – 1430 (`swarm`); at Omarchy's `--frame-rate 120`, 0.45 – 11.9 s |
| Trailing frames with the text already settled | 1 – 655; **eleven effects settle 3 frames or fewer before exiting** (≤ 25 ms) |
| Trailing frames **byte-identical** (colour settled too) | 1 – 66; median 5 |

So **every effect settles, and almost none of them hold.** The colour keeps moving through the
final gradient until the last frame, the process exits within a few frames of the text landing,
and `bin/omarchy-screensaver`'s `while true` restarts `ttfx` immediately. The terminal screensaver
has no dwell at all — which is fine for a terminal, where the last frame *is* the picture.

📌 **The hold is ours to add, and on the library route it is trivial.** The producer (§6.1) owns
the loop: run `next_frame` until it returns `None`, then keep the final grid on the ring for a
dwell of N seconds, then pick the next effect. A PTY tap could not have done this without
patching Omarchy's loop, which is one more reason §2 inverted. So each cycle is
`motion → settle → dwell → next`, and a screensaver has the one thing an interactive app never
has: **time, with nobody waiting** — because we give it some.

🚨 **So: raster during motion, path-trace during the hold.** The screensaver *resolves into a
photograph* — ten seconds of animation that comes to rest and then visibly sharpens, over two or
three seconds, into a fully path-traced still with real dispersion and caustics. Then it dissolves
and does it again.

The change needed is small and specific: **add the cell-grid generation counter to the `pt_content`
tuple** so accumulation restarts when the glyphs move rather than never.

⚠️ **Do not use TAA.** `temporal.rs` reconstructs velocity from *camera* reprojection only and its
own doc says per-object deformation ghosts. Glyphs teleport cell-to-cell; the neighbourhood clamp
will not save them. MSAA or supersampling instead.

---

## 9. The law that lets presets go far — and the harness that proves it

Legibility of a glyph grid does **not** come from each tile's silhouette. It comes from the grid.
So a cell can be replaced by almost any object — a tube, a bottle, a plexus node, a lump of cooling
metal — provided two things hold:

> **1. The cell's energy stays in the cell.** Refraction reach, dispersion spread, halation radius
> and persistence must all be bounded relative to cell size. Inter-cell bleed is the only thing
> that actually destroys text.
>
> **2. The cell's apparent brightness tracks the effect's value.** Whatever happens inside a cell,
> its integrated luminance must correlate with what TTE said that cell was.

📌 **Both are measurable, which is the point.** Downsample the render to the cell grid, correlate
against the source cell luma, and "is this preset still readable" stops being a matter of taste. It
runs on any GPU, it is deterministic given a fixed cell grid, and it makes this one of the rare
Organon features that can carry real automated visual regression rather than the usual
`cargo test --workspace` ceiling. **Build the harness before the exotic presets, not after.**

---

## 10. The preset ladder

Ordered by how far they depart, all satisfying §9. Each names the existing machinery it rides.

| Preset | The look | Rides |
|---|---|---|
| **`faceplate`** | The classic, done properly: phosphor emission, thin clearcoat, per-channel persistence, halation, dark cells reflecting the room | §4 + `fx.wgsl` halation |
| **`nixie`** | Each cell a glass envelope with a warm neon filament, mesh in front, orange low-pressure glow | Glass material + §4 |
| **`foundry`** | Blackbody incandescence with cell **value driving temperature** rather than colour — glyphs as cooling hot metal type | `cube.wgsl` `emit.w`, `blackbody()` |
| **`anodized`** | Thin-film iridescence over dark metal; colour from film thickness, not albedo | `cube.wgsl` `thinfilm` (thickness nm, marbling, film IOR, drainage) |
| **`bottled`** | Dispersive glass rods with emissive cores, camera tilted so you see down them, TIR carrying light along their length | §11 |
| **`cathode`** | Cells become plexus nodes; edges wire together the cells *within* each glyph, so the letterform emerges from the circuitry | Plexus Tier 2, near as-is |

📌 **`foundry` and `anodized` are the sneaky-good ones**, because in both the effect's own value
channel drives a *physical* parameter rather than a colour. The gradient TTE authored becomes a
physical gradient. That is the most Organon-ish reading of the whole idea.

---

## 11. Light in a bottle — and the one gap

Two distinct glass ideas, and conflating them is a mistake:

- **The faceplate** — a *thin* dielectric over an emitter. Its job is a specular reflection
  independent of emission (§4.1). Cheap: a clearcoat lobe, no transmission.
- **Light in a bottle** — a *thick* dielectric volume with a separate emissive object suspended
  inside it. The magic is neither the glass nor the wire but seeing the wire *through* a refracting,
  dispersing, absorbing medium. (Look reference: the Instagram account **@uonvisuals**, working
  offline in Cinema 4D. Reference frames deliberately not committed — third-party work.)

Decomposed, it is five ingredients, and **measured**, the tree already has four:

| Ingredient | Status |
|---|---|
| Stochastic dielectric BTDF — Fresnel split, refract on entry + exit, TIR, Beer–Lambert | `rt_pathtrace.rs` `pt_dielectric` / `pt_absorb` |
| Spectral dispersion with a real Abbe number (per-λ Cauchy) | `params.rs` — `spectral_enable` + `spectral_abbe` |
| Cheap RGB-split dispersion for the raster path | `cube.wgsl:1200` `glass_dispersion()` |
| Photon-mapped caustics | `rt_caustic.rs` |
| Analytic capsule/tube impostors — sphere-traced SDF, per-instance endpoints + radius + colour + emissive, writes depth, joins the FX prepass | `particles.wgsl:825` |
| **Seeing the emissive core through the shell** | ❌ **missing** |

🚨 **The gap is exactly one thing.** `particles.wgsl:661`'s Glass/Refractive branch refracts the
**environment** — it samples the prefiltered IBL through the refracted direction. So you get a glass
tube showing you the sky, not a glass tube showing you the glowing wire inside it.

Three routes, ranked:

1. **Coaxial SDF in the impostor.** Trace to the outer capsule, refract, keep marching against an
   *inner* capsule of smaller radius carrying the emission, apply Beer–Lambert over the path length
   between them. Exact for the coaxial case, runs at raster speed, roughly 40 lines added to
   `capsule_trace`. **Start here.**
2. **Path tracer.** Already correct — nested dielectrics, dispersion, caustics, no new shading code.
   The cost is convergence, which §8 turns from a problem into the feature.
3. General nested dielectrics in the raster path. Not worth it.

---

## 12. What is measured, what is reasoned

🚨 **This section is the one to read before depending on a sentence above.**

**Measured** — run against the tree on 2026-09-02: the `logo.txt` character census; the TTE effect
symbol inventory (by regex over `effects/*.py`, so approximate at the margins); `Coord` being
integer and `Path.step` discarding the remainder; `Terminal._update_terminal_state`'s cell model;
`term.rs`'s PTY + `alacritty_terminal`; `term_view.rs`'s own note about the deferred glyph-atlas
pipeline; every file:line citation in §4, §5, §8 and §11; the `mind_ring` / `audio_ring` /
frame-mirror precedent in `ipc.rs`; the TLAS bindings in the five `rt_*` modules; the licence fields
of every workspace member and Omarchy's MIT `LICENSE`; Omarchy's migration `1786355450.sh` replacing
`python-terminaltexteffects` with `ttfx`.

**Measured 2026-09-02, later the same day, against `organonart/ttfx @ 7203e35` (v0.3.2) and
`terminaltexteffects @ 7a91dd9` (v0.15.0):** ttfx's `LICENSE` (MIT, both copyrights) and `NOTICE`;
`Cargo.toml` (`license = "MIT"`, three deps, `src/lib.rs` present); `CharacterVisual`'s fields
against Python's; `Coord`'s type; `path_step`'s rounding site; `Motion.active_path` /
`previous_coord` / `set_coordinate` being public; `Terminal`'s public and private fields;
`Clock::Virtual`; `EffectCommand` being a public clap `Subcommand` with `build_effect`; `cargo
search ttfx` (absent); `cargo check` and `cargo build --release` on Windows; the 37-effect
settle/hold table in §8 (dumps in the session scratchpad, reproducible from the command given
there); `bin/omarchy-screensaver`'s invocation (`--frame-rate 120`, no `--xterm-colors`, `while
true`); the 2:1 cell aspect in `geometry.rs` (`double_row_diff`, the doubled x on circles).

**Measured 2026-09-02, W6, against `organonart/ttfx @ 8d79d82` (this branch's head; its base is
`7203e35`):** `path_step` returns the pre-rounded pair and `Motion::set_position` is the only
float→`Coord` site; `cargo test` in ttfx on Windows — 20 unit + 5 golden/trace + 5 new tests,
all green, and `engine_traces_match_python` failing with **317** mismatched lines under a
deliberately swapped rounding axis; the **differential parity run** — every case in
`tools/parity/cases.txt` at seeds 42 and 1337, `--parity-dump --max-frames 400`, pre-change
binary against post-change binary, both built `--release` on the same Windows toolchain:
**354 of 354 identical** (177 cases × 2 seeds, compared by SHA-256 and exit code; one case,
`laseretch-group-quirk`, is a 48-byte dump on both sides and proves nothing about motion —
every other dump is tens to hundreds of kilobytes of frames). The parity suite proper (Python reference, Linux/glibc) was **not** run here.
On the Organon side: `sub_x`/`sub_y` round-trip through the ring as the exact `f32` pair, a
placed character encodes `(0, 0)`, the producer's remainder equals `current_pos − current_coord`
computed inline through a real engine tick (a swapped helper axis fails with the pair
reversed), and a swapped consumer axis fails `lower_grid`'s tile-placement test — all four
mutation-tested. ⚠️ Not measured: the look. No frame has been rendered with a non-zero
remainder; "smoother" is reasoned from the arithmetic.

**Attributed, not measured:** that `ttfx` is DHH's Rust port of TTE, written by having an agent
port the Python — **James, 2026-09-02.** ttfx's own `LICENSE` names 37signals / omacom-io and
its `NOTICE` names the original as ChrisBuilds' design; neither names an individual author.

**Reasoned, unverified** — and each is a place this document could be wrong:

- ~~**`ttfx` is assumed to be TTE's lineage.**~~ Settled above; a port, and a library.
- ~~**Every TTE effect settles and holds.**~~ Measured in §8: settles 37/37, holds almost never.
- **The library route has not been driven.** Every claim in §2's table is read from the source;
  no crate has yet built an effect through `EffectCommand`, ticked it under `Clock::Virtual` and
  walked `arena`. Tier 1's first commit is that program.
- **Cell counts and instance-path performance.** ~14–16k cells at 18 pt fullscreen is an estimate;
  no draw was timed.
- **The visual on Linux/Wayland.** `organon-visual` is winit + wgpu and already opens
  borderless-fullscreen on a named display for the projector case, but it has never been run as a
  Hyprland screensaver. Unknown.
- **§5.1's bevel perception claims.** Optics reasoning, no render.

---

## 13. Deployment — the least fun part, named early

⚠️ **Licensing.** Omarchy is MIT (`LICENSE`, DHH). `organon-visual` is GPL-3.0-or-later — inherited
by depending *upward* on the root crate, per `native/Cargo.toml`'s licence note. Exec'ing a separate
GPL binary is fine; vendoring it into an MIT tree is not.

📌 **ttfx is MIT, so linking it is clean in every direction** — into the permissive producer crate
(§6.1) and, if it ever came to that, into the GPL root. Had it been GPL, the PTY tap would have
come back as a *process boundary doing licence work*, which is why §2 keeps that route named. Two
obligations come with the dependency and both are cheap: keep ttfx's `LICENSE` and `NOTICE` text
with any distributed binary, and credit both lineages where the screensaver credits anything —
**terminaltexteffects** is ChrisBuilds' design (Chris, `741258@pm.me` in `pyproject.toml`, MIT),
**ttfx** is DHH's Rust port of it (per James; the file says 37signals / omacom-io). Pin the git
`rev`; a floating branch dependency in a workspace that already pins `baseview` by rev would be
the odd one out.

⚠️ **The screensaver path is also the lock-screen path.** `bin/omarchy-system-lock` stops `ttfx`
before closing its terminal. A GPU app crashing on a lock screen is a materially worse failure than
a terminal doing so.

⚠️ **Cold start and VRAM, per monitor.** `omarchy-launch-screensaver` spawns one instance per
monitor on idle and it must die on any keypress.

📌 **Therefore: an optional Omarchy extension, not a replacement of the default**, with `ttfx`
remaining the fallback. Upstream acceptance into an opinionated, minimal distribution is a separate
conversation and should not be assumed.

📌 **Presets should be self-contained.** Organon's presets live at
`dirs::data_dir()/OrganicMath/presets.json`. That resolves on Linux, but requiring Organon to be
installed makes authoring a barrier. Export a single self-contained preset file the screensaver
reads standalone, so Organon is the *authoring* tool rather than a runtime dependency.

---

## 14. Tiers

Each is independently shippable and each defaults to inert per invariant #4.

- **T1 — cell ring + block glyphs.** A producer crate linking `ttfx` (§2, §6.1) → glyph ring →
  instanced beveled boxes with per-instance emission and a PBR backplane. Slide-vs-cut gated on
  `active_path` (§7); the dwell after settle (§8). Renders the entire Omarchy logo and most effect
  symbols. This alone is the demo.
- **T2 — the legibility harness.** §9. Before the exotic presets, not after. Its fixture grid can
  come from the same producer under `Clock::Virtual` and a fixed seed, which is what makes it
  deterministic end to end rather than only from the grid onward.
- **T3 — look controls + preset.** Extrusion, bevel, face crown, emission gain, backplane material,
  camera tilt, light rig, CRT post. Saved as a preset.
- **T4 — screensaver mode.** Borderless-fullscreen per monitor, reads a preset, exits on input.
  Optional package.
- **T5 — converge on hold.** The `pt_content` generation counter, and the raster→path-trace
  handover.
- **T6 — coaxial glass capsule.** §11 route 1. Unlocks `bottled` and `cathode`.
- **T7 — real letterforms.** `ab_glyph` outlines → tessellate → extrude + bevel → cached mesh
  atlas. Generalizes to "all our text is PBR". Its real customer is the Console's terminal.

⚠️ **T7 is not a prerequisite for anything before it**, and treating it as one is how this project
would fail to ship.

---

## 15. The gap to the plates — measured on the first GPU look

Three plates were committed beside this document (`doc/images/`): the before/after, the spec
sheet, and the resolve arc. They are the claim. On 2026-09-02 the first tiles came up on
organon-one (RTX 5090, `main @ 0b0f3e6`, then `1c1c3ba` with T5) and were held against them.
**Measured, not inferred, and the honest summary is: the spine is there and almost none of the
skin.** What follows is the gap as a plan, so that it can be worked in parallel by file ownership
rather than argued about.

**What the plates show that the first render did not** — each row names the machinery that
already exists, because most of this is wiring:

| Plate | On screen, `1c1c3ba` | Closes it | Owns |
|---|---|---|---|
| Every cell is a tile; a dark cell shows the room through a glass faceplate | Only lit cells get tiles; dark cells are bare slab | Full-grid tiles with the existing clearcoat lobe (`cube.wgsl` `coat`) — both halves landed: the shader's profile and coat (#233), and the lowering as `lower_grid_with(…, LowerOptions { dark_tiles })`, off by default, `Shared.glyph[14]` proposed as the lane (§15.1) | `cube.wgsl`, `glyph_ring.rs::lower_grid` |
| The emissive core has a soft falloff, seen *through* the faceplate | Flat, uniform emission across the face | A per-tile emission profile in the cube shader, keyed on the instance's own UV | `cube.wgsl` |
| Glyphs as lights: the green pool on the backplane, a contact shadow in each well | Nothing spills; no well shadow | Emission-driven selection for the brightest-N point lights (`world.rs:9146`), RT shadow + AO for the wells | `world.rs` (selection), `rt_shadow`/`rt_ao` |
| A brushed dark-metal backplane with a warm rim | A flat dark slab | The existing anisotropy lobe (`cube.wgsl` `aniso`, brush along local +Z) on the backplane instance, and a light rig | `world.rs` (rig), T3's backplane params |
| The path-traced still: caustics, converged, **lit** | 🚨 **The dwell goes dark.** T5 hands the held frame to the tracer, and the tracer shades from `tint` — it has never seen the emit buffer | Every `rt_*` pass and `rt_pathtrace` read the per-instance emission | `rt_pathtrace.{rs,wgsl}`, `rt_*.rs`, their binding sites in `render.rs` |
| Camera held, framed, slightly tilted; dark environment | Orbiting, far, the atmosphere's fog behind the grid | T3 (in flight): framing from the grid's bounds in cell units, a held camera while a ring is live, `faceplate` with a dark environment and TAA off | T3 |
| Phosphor persistence | **Not in this document until now** | Producer-side per-cell decay in linear light, published as the cell's colour, with a `persist` flag so the renderer can tell a trail from a lit cell — **landed as T11** (`organon-glyphs --persist-ms`, `SGR_PERSIST`; §15.1) | `organon-glyphs` |
| The scatter phase: motion streaks with dispersion | **Not in this document until now** | A velocity-keyed streak in post, RGB-split; the one row that is new rendering work rather than wiring | `fx.wgsl`, `post.rs` |
| Six preset rungs (§10) | Only `faceplate` is scoped (T3) | Preset data once T3's knobs exist; `bottled`/`cathode` ride T6's capsule core, already landed | `preset.rs` (data), after T3 |

📌 **Confidence, stated plainly.** The still "after" plate is reachable with what exists — every
row above except two names a lobe, a pass or a selection that is already in the tree. The two
that are not: the scatter streaks (new post work), and whether the anisotropic lobe can be applied
to the backplane *instance alone* while the tiles stay isotropic (the lobe is a per-draw uniform
today; the backplane may need its own draw). The resolve-arc plate is therefore medium confidence
until those two are tried.

### 15.1 Tiers, continued — and the order they can run in

Each independently shippable, each inert by default (invariant #4), each owning files no other
running worker touches. **T3 is the gate**: it owns `world.rs`, `render.rs`'s uniform builders,
`cube.wgsl`'s uniforms and the whole param chain, so nothing below that names those files starts
until it lands.

- **T8 — the tracer sees emission.** Every `rt_*` pass and `rt_pathtrace` bind and read the
  emit buffer beside the tint buffer, so a ray-traced reflection of the grid is lit glyphs and the
  T5 dwell converges to a photograph rather than to black. Owns `rt_*.{rs,wgsl}` and their binding
  sites in `render.rs`. **After T3.** Nothing else in this list is worth looking at until this is
  in — a dark dwell is worse than no dwell. **Landed** (organon#217 T8): the three passes that
  shade a hit — `rt_pathtrace`, `rt_reflect`, `rt_gi` — bind `emit_buf` and add `cube.wgsl`'s
  own `emit.rgb * emit.w` at the hit; an emissive hit terminates a camera path. `rt_shadow` and
  `rt_ao` are visibility-only and bind nothing; `rt_caustic` shades the photon's BSDF, where the
  landing surface's emission plays no part — *emitters as photon sources* is its own tier, and so
  is NEE toward emitters (the tracer has no light list; it reaches key + fill only). Green and
  ready to try; the GPU look this needs is the one §15's row names: the dwell converging lit.
- **T9 — the tile itself.** Full-grid tiles (dark cells too), the faceplate as a clearcoat lobe
  over a near-black dielectric, and an emission *profile* across the face — a soft falloff so the
  core reads as behind glass rather than painted on. Owns `cube.wgsl` (shading, not uniforms) and
  `glyph_ring.rs::lower_grid`. Can run **beside T8** — different files. **Landed, shader half:**
  `tile_profile` on the face UV (`doc/arch/render.md`, "The tile"), strength on
  `Uniforms.shape.z` from `Shared.glyph[13]` (the lane is named; the world's `glyph_shape` lifts
  it — W10). The faceplate turned out to be preset data: the clearcoat lobe already transmits
  `emissive` through `(1 − fc)` and adds its environment sheen independently. **Landed, lowering
  half:** `glyph_ring::lower_grid_with` with `LowerOptions { dark_tiles }` — a symbol-less cell
  (empty, space, a control) is a full-cell tile at the `░` depth (`DARK_TILE`, a quarter as proud
  on the shared `look.depth` scale), faceplate tint, emit exactly `(0, 0, 0, gain)`; a T11 trail
  is still a *lit* cell; a dark tile sits on the grid at its cell centre and never slides (a
  space *character* on a path is a real ttfx thing, and its tile is the faceplate's, not the
  character's); backplane, wells and bounds unchanged. **Default off and byte-identical to
  `lower_grid`**, which the world still calls — pinned by lowering an asymmetric fixture both
  ways. Wire proposed: **`Shared.glyph[14]`** (`[13]` is the profile strength above), the world
  passing the flag where it calls `lower_grid` today. **Measured** (release, 200×80 = 16 000
  cells, one in seven lit and sliding, best of fifty interleaved): 92 µs without dark tiles
  (2 286 instances), 125 µs with (16 001, sliding), 92 µs with (settled) — the CPU lowering is
  not where the fullscreen cost will be; the 16 000-instance draw is, and that waits on a GPU
  look.
- **T10 — glyphs as lights and the backplane rig.** Emission-driven brightest-N selection so the
  green pools onto the backplane; the anisotropic brushed backplane; the warm rim; RT shadow and
  AO in the wells. Owns `world.rs`. **After T3; beside T8/T9.**
- **T11 — persistence. Shipped.** `organon-glyphs --persist-ms <τ>` (default **0 = off**, and
  off is byte-identical to a producer without it — invariant #4, pinned over a whole effect).
  `organon-glyphs/src/persist.rs` keeps one phosphor per cell **in linear light** and rewrites
  the walk before it is published; the ring's colour contract does not change (sRGB8 in the
  cell, decoded by the world) and the header carries nothing new — the colour arrives already
  decayed, so the world needs no τ. **The rule:** excitation is instant, decay is slow, and a
  phosphor cannot be un-lit by a new colour — a lit cell publishes `max(source, residual)` per
  channel (a steadily lit cell is exactly its source; bright→dim fades into the dim; a hue
  change keeps the old hue's residual under the new; a literal sum would run away on a
  re-excited constant source, and "source replaces" would cut every bright→dim transition,
  which is most of what a resolve *is*). A cell whose source went dark publishes the **last lit
  cell** — its symbol, because the tile shape is what fades, plus attributes, identity and
  sub-cell offset — with `fg` decayed and re-encoded and the cell flag **`SGR_PERSIST`** (bit
  11) set, `ACTIVE_PATH` cleared; below a floor of linear `1e-3` (≈ 3/255) it is spent and the
  cell reverts to its source (~6.9τ from full white). A lit cell with no colour of its own leaves
  no trail: it draws in the renderer's `default_fg`, a look constant the producer must not bake
  into the ring. Time is the producer's *published* time, nominal (`1/tick_hz` per motion tick,
  the heartbeat interval per dwell beat, zero for the settle publish), so a seed reproduces a
  run and `--tick-hz` below `--fps` slows the effect, not the phosphor; the phosphors outlive an
  effect, so one effect's settled text fades under the next. **The settle rule: the effect has
  settled when the *source* has.** `FRAME_SETTLED` is set whatever the phosphors are doing, so
  a trail can never hold the settle off — but the trails keep decaying through the dwell, so
  the payload keeps changing, `generation` keeps moving, and T5's accumulation restarts every
  heartbeat until the last trail crosses the floor. That is the right order: the tracer
  converges once the picture has stopped changing, and it learns that from the counter it
  already watches. `lower_grid` never takes a trail as a slide origin (a trail keeps the
  `character_id` of the character that left it, and that character is live elsewhere in the
  same grid). No `layout_version` move: a bit in an existing word, and a reader that predates
  it draws a dimmer tile, which is the right picture. ⚠️ **Measured at this rev over a 24×2
  fixture: `decrypt` never lets a lit cell go dark** (752 frames, zero trails), and neither do
  `wipe`, `expand`, `slice` or `middleout` — what persistence does in `decrypt` is the
  bright→dim fade of each resolving character, not tails. Tails behind moving characters are
  `rain`, `pour`, `print`, `beams`, `swarm`, `bubbles`, `crumble`. Not yet looked at on a GPU.
- **T12 — sub-cell rendering.** The ring already carries `sub_x`/`sub_y` (in flight); the
  renderer slides a tile whose character is on a path and cuts one that teleported
  (`ACTIVE_PATH`). Owns the grid lowering only. **After T3 and T9.**
- **T13 — the gate on a real render.** T2's harness over a `faceplate` frame read from the HDR
  buffer, in `verify.sh`, with the thresholds beside the goldens. **After T3 and T8.**
- **T14 — the preset ladder.** `nixie`, `foundry`, `anodized`, `bottled`, `cathode` as preset
  data over T3's knobs and T6's core. **After T3; needs a GPU look per rung.**
- **T15 — the scatter.** Velocity-keyed motion streaks with an RGB split for the raster phase.
  New post work; **last**, and allowed to fail without blocking anything above.

T4 (Omarchy) and T7 (letterforms) stand as written in §14; T4 waits for T3's self-contained
preset, T7 for `world.rs` to be free.

### 15.2 What the first look settled (§12, continued)

**Measured 2026-09-02 on organon-one:** with no producer the visual is byte-for-byte its ordinary
self and the grid appears only once the ring exists (invariant #4 held); the logo renders as
emissive beveled tiles over a dark slab, half-blocks as half-height tiles, bloom on the lit glyphs
only, `decrypt` animating live through the ring and resolving to the correct text; the tiles
arrive small and far because the grid inherits the cube field's default camera distance; the
auto-orbit never stops, so T5's accumulation restarts every frame (the T5 worker, #227, found the same in the code);
the T5 dwell renders **dark** because the tracer shades from `tint` (the T1 worker named the gap in #224 before
merge; this is it on screen); and the out-of-the-box environment is the physical atmosphere, which
reads as fog over terrain behind the grid. **Retired from §12:** `ttfx` is a Rust rewrite (§2.1);
every effect settles (measured over all 37). **Not yet drawn:** the ~14–16k-cell fullscreen
case — the logo is 81×10, and nothing larger has been rendered. Its CPU lowering *is*
measured (§15.1, T9: ~125 µs for 16 000 cells with dark tiles on, release); the
16 000-instance draw is not.
