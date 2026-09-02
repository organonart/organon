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

Two ways to tap it:

| | How | Cost | What it buys |
|---|---|---|---|
| **PTY tap** *(preferred)* | Run `ttfx` under a pseudo-terminal and read the cell grid out of `alacritty_terminal`'s `Term` | Almost nothing — `organon-console/src/term.rs` already does exactly this, PTY and all | Zero changes to TTE. Every future effect arrives for free. |
| **Binary writer** | Patch TTE with an alternate `Terminal` writer emitting packed cells instead of ANSI | ~200 lines, and it forks a dependency | Higher fidelity: layer, visibility, and the *unquantized* position (see §7) |

🚨 **Start with the PTY tap.** It costs a day and it is honest about where the value is: not in the
effects, which someone else wrote well, but in what we do with their output.

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

**Decouple the rates.** TTE ticks at its own speed; the renderer runs at 120 and interpolates.
Otherwise the whole thing is capped at Python's frame rate.

---

## 7. Motion is quantized, and only 3-D reveals it

**Measured.** `geometry.Coord` is `column: int, row: int`. `Path.step()` computes a float distance
factor, calls `find_coord_on_line` / `find_coord_on_bezier_curve`, and the sub-cell remainder is
discarded.

In a terminal this is invisible — the cell *is* the atom. Rendered as tiles under a camera it reads
as stepping. Two fixes, and they are not equivalent:

- **Patch TTE** to expose the pre-rounded coordinate (this is the §2 "binary writer" route earning
  its keep).
- **Interpolate cell-to-cell on the Organon side** — cheap, but a character that *teleports* (and
  several effects do) will interpolate wrongly, sliding where it should cut.

⚠️ This is the one place the project **changes** the original rather than adding to it, and it is
the one the effects were authored against. Treat it as the highest-risk unknown.

---

## 8. A screensaver has time — converge on hold

**Measured.** `world.rs:8458` restarts path-trace accumulation on camera move, buffer resize, or a
change to the settings that decide what the buffer holds. It does **not** restart on geometry
change, and the surrounding comment says a moving field would smear the average.

**Reasoned.** Every TTE effect animates, resolves to its `final_gradient`, and holds; Omarchy's
loop then restarts `ttfx`. So each cycle is `motion → settle → hold → restart`, and a screensaver
has the one thing an interactive app never has: **time, with nobody waiting.**

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

**Reasoned, unverified** — and each is a place this document could be wrong:

- **`ttfx` is assumed to be TTE's lineage** because the CLI surface matches (`-i`, `--anchor-text`,
  `--reuse-canvas`, `--random-effect`, `--xterm-colors`, effect subcommands). Whether it is a
  rename or a native rewrite was **not** established, and it decides whether the §2 "binary writer"
  route is even available. ⚠️ **Settle this first.**
- **Every TTE effect settles and holds.** Inferred from the shared `final_gradient_*` config
  surface, not from running them. §8's whole behaviour rests on it.
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

- **T1 — cell ring + block glyphs.** PTY tap → glyph ring → instanced beveled boxes with
  per-instance emission and a PBR backplane. Renders the entire Omarchy logo and most effect
  symbols. This alone is the demo.
- **T2 — the legibility harness.** §9. Before the exotic presets, not after.
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
