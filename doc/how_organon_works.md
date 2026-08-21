# Organon Engine — Technical Overview

> **What this is.** A human-readable technical description of the Organon engine: what
> the system is, how it is put together, what it can do, and where the seams are. It is
> distilled from the repo's working architecture references (`ARCHITECTURE.md` for the
> native engine, `doc/arch/render.md` for the render pipeline, `MIND_ARCHITECTURE.md`
> for the Mind lane, `CONSOLE_ARCHITECTURE.md` for the Organon Console) with the
> implementation minutiae — byte offsets, slot indices, layout versions, per-PR history —
> deliberately left out.
>
> **Audience:** an engineer, a technically literate reader, or a writer who needs an
> accurate account of the whole system in one pass.
>
> **Status:** counts current as of ISO week 2026-33 and re-measured at that date; anything
> not yet merged to `main`, or not yet verified on real hardware, is explicitly marked *in
> flight* or *pending verification*. ✏️ **§1 and §16 were reframed on 2026-08-21** to match
> `doc/organon_prd.md` — that edit changed what this document says Organon *is*, and
> deliberately re-measured nothing, so every count below still carries the week-33 date.

---

## 1. What Organon is

Organon is **one native application, written in Rust on wgpu, whose identity is assembled at
runtime rather than compiled in.** You divide its window into regions, declare what each region
holds, and save the arrangement under a name; that named arrangement is what somebody means when
they say which program they are running. It has four defining properties:

1. **A layout is the unit of identity.** Up to six addressable regions over a 3×2 grid, each
   declaring its content — an agent conversation, a column of instrument panels, a live 3D
   viewport, a piece of media. An arrangement can be named and written to disk, and a load
   applies whole or refuses with one sentence.
2. **An agent is not optional.** Every valid arrangement contains a working agent harness, and
   this is enforced rather than encouraged: any command whose *result* would leave no agent
   region is refused, and a saved layout naming none does not load. The agent reaches the same
   command vocabulary a human types and a script calls, and is bounded by a permission card
   rather than by good behaviour (§13).
3. **Its renderer draws everything, and runs as a separate OS process.** The chrome, the text
   and the world are one renderer's output rather than a 3D view inside a widget toolkit. A
   second binary owns a fullscreen window, the animation clocks, the camera and the whole
   pipeline; the two communicate through a shared-memory snapshot.
4. **It is a physically based light-transport engine pointed at whatever fills a region.** Today
   the thing being lit is the output of a *generator* — a parametric mathematical system
   evaluated fresh every frame — because that is the only producer built. The boundary is
   deliberately smaller than that: a producer yields a texture the application can sample, at a
   size it asks for.

🚨 **Organon is not the visualizer.** The generative-math visualizer is *one thing Organon
hosts* — built in, and conceptually a module like any other. The founding algorithm is a
cube-field visualizer that began life in 2000 as an OpenGL exercise; it is still generator zero,
and everything else grew around it. The test for any description of this system: **would it still
be true if the visualizer were deleted?** ⚠️ The tree does not currently pass that test — `3d` has
exactly one producer — which is a reason not to claim a plurality of hosted things in the present
tense, not a reason to mistake the instance for the identity.

**It is all one product**, and the two other arrangements it builds from the same workspace (§2.4)
are **Mind** — watching a language model think (§11) — and the **Console** — an agent-operating
workstation (§12). ⚠️ **They are still three binaries today**, chosen by a compile-time edition
rather than by a saved layout; collapsing that into one binary that opens into a named arrangement
is issue #111 and has not started. `doc/organon_prd.md` is the product definition and its §12 is
the honest state of play.

🚨 **The one thing that cannot be a layout is the plugin.** A VST3/CLAP inside a DAW has a
host-owned window, a host-controlled lifetime, an audio thread with hard real-time constraints,
and a saved-session identity that outlives any decision here — so it stays a separate artifact.
Every one of its ~1,370 parameters is host-automatable, MIDI-learnable and preset-captured, and
the host supplies tempo and transport for free.

This repository is the native engine and its documentation; a browser port of the founding
algorithm exists in the project's history but is parked and does not live here.

Everything below describes the native engine unless stated otherwise.

---

## 2. Shape of the system

### 2.1 One workspace, eight binaries

A cargo workspace of five crates — a root crate plus **`organon-core`** (the host-free
spine: math, IPC, params, GGUF, editions — no plugin framework, no GPU, no UI, enforced
by dependency test), **`organon-render`** (the renderer and its ~50 shaders — no plugin
framework, no UI toolkit, no windowing), **`organon-mind`** (Mind's own code), and
**`organon-console`** (the console's compositor and terminal) — compiles into:

| Binary | What it is |
|---|---|
| **plugin** (cdylib) | the VST3/CLAP plugin — parameters, editor, `process()` |
| **standalone** | the same editor without a host |
| **visual** | the renderer: a winit + wgpu window, the whole GPU pipeline |
| **`organon`** (CLI) | a command surface over the live engine, for the terminal and for external agents |
| **mind-writer** | a synthetic activation-frame generator (exercises the live LLM path with zero inference) |
| **mind-runtime** | an embedded llama.cpp runtime that loads a `.gguf`, runs real inference, and streams activations (opt-in build feature) |
| **organon-mind** | **Organon Mind** — the LLM-analysis edition (opt-in build feature) |
| **organon-console** | the **Organon Console** — Organon as an agent-operating workstation (opt-in build feature) |

Module compilation is split by binary: the plugin dylib never compiles the renderer or
the inference runtime. The pure mathematics (`math.rs`) lives in `organon-core`, is
shared by everything, and is unit-tested independently of any GPU.

### 2.2 Two processes, one snapshot

```
  ┌──────────────────────────────┐        ┌──────────────────────────────┐
  │ PLUGIN (in the host)         │  mmap  │ VISUAL (renderer process)    │
  │  · host parameters           │ ─────► │  · reads the snapshot/frame  │
  │  · process() on audio thread │ Shared │  · OWNS clocks/camera/state  │
  │  · egui editor on GUI thread │        │  · evaluates generators      │
  │  · WRITES the snapshot       │ ◄───── │  · renders and presents      │
  └──────────────────────────────┘Feedback└──────────────────────────────┘
```

**Why it is built this way.** A plugin cannot set its own parameters from the audio
thread — parameter writes are GUI-thread only. So any input that must drive the look at
audio rate (MIDI CC from a clip, a held note recalling a preset, an agent's override)
bypasses the host parameter layer entirely and writes the shared snapshot directly. The
plugin stays a thin control surface, and the host keeps native ownership of automation,
mapping and MIDI learn.

The snapshot (`Shared`) is a flat, C-layout, plain-old-data struct — currently about
8.5 KB — memory-mapped into a file. The plugin writes it once per audio block; the
visual reads it once per frame. Writer and reader form a **seqlock**, so a reader never
observes a torn blend of two snapshots — a real failure mode that was reproduced in a
test before it was fixed, not a theoretical one. A reverse channel (`Feedback`) carries
resolution, frame rate, GPU timing, and hardware-capability flags back to the editor.

Two further channels stay *off* the snapshot on purpose, because their rates are wrong
for a control-rate block: an **activation ring** (per-token LLM activations, written by
the inference runtime) and an **audio sample ring** (the plugin's post-synth stereo
output, streamed to the visual's recorder).

### 2.3 The layout invariant

The snapshot is **append-only**. New features add blocks at the end; existing offsets
never move. That is what lets a running plugin and a running visual survive a rebuild of
one side. When an incompatible change is genuinely needed, a layout version is bumped and
both binaries are rebuilt together. Golden tests pin the struct size, key offsets, and a
hash of the entire default snapshot — so a refactor that claims to be byte-identical has
to prove it.

### 2.4 Editions

**Organon** (the visualizer), **Organon Mind** (the analysis lane as its own standalone
instrument), and the **Organon Console** (the `console-edition` build) are the same
engine wearing a different front-of-house, selected at build time by a cargo feature.
This is one product in three builds — an edition, not a fork: the algorithm, every
shader, the snapshot layout, the preset store, and — critically — the *visual binary*
are byte-identical across them.

An edition drives six behaviors: the displayed product name; the IPC namespace (so the
editions' memory-mapped files never collide, and two of them can run simultaneously); which
editor tabs are visible; whether the visual window is an instrument window or a
projector feed; whether the on-scene UI layer starts visible; and whether the world
module compiles into the library at all. Each is a pure function of the edition value,
so every edition's behaviour is unit-tested from a single default build. An environment
override on the namespace is how one compiled visual binary serves every edition. Mind
and the Console are standalone-only permanently — no second plugin identity, ever.

---

## 3. The control layer

### 3.1 Parameters

About **1,370** host-mappable parameters, with roughly **100 distinct enum types** carrying
the discrete choices (generator, surface mode, material, tone map, camera path, palette,
per-generator families and views, and so on). Every parameter is automatable,
MIDI-learnable, and captured by presets.

The packing from parameters into the shared snapshot used to be hand-written twice — once
for live parameters, once for presets — an indexed scheme where a wrong slot silently
corrupted whatever the visual read. It is now generated from **one ordered slot table**:
a single declaration per snapshot block emits both packers. The two can no longer drift,
and a renamed field is a compile error rather than a silent zero. That same table is also
what mechanically generates the **action catalog** the AI agent and CLI speak (§13), so a
new parameter enters the agent's vocabulary automatically — and the user-facing
parameter reference (`doc/reference/`) is generated from the same described catalog, with
a test that fails the build if the generated pages drift from the code.

### 3.2 Presets

A preset is the complete serializable parameter state. Recall is applied *through the
host's parameter setter*, so it is automation-recordable and undoable. Storage is JSON in
the application support directory.

Three details matter:

- **Atomic recall.** Setting well over a thousand parameters one at a time on the GUI thread, while the
  audio thread snapshots every block, means the visual could render half-applied states —
  new geometry with old colour. A sequence-lock around the apply makes the visual see the
  new look in one step.
- **A tab partition.** The editor's tabs (Generator / Motion / Environment / Look / Audio
  / Synth / Settings, plus Mind) also partition the preset state, so you can recall a
  whole Scene and then swap a different Look or Environment on top. The
  field→tab partition is a single source of truth with a drift-guard test asserting the
  partition is exactly the set of captured fields — no orphans, no double-counting.
- **Beat-quantized recall.** A recall can be scheduled to the next bar or beat boundary
  instead of firing immediately, which is what makes preset changes musical rather than
  jarring.

A separate sparse **recorded-defaults** overlay lets any slider's "reset" target a value
you chose rather than the factory default.

### 3.3 Ways in

Four input paths, with a defined priority cascade:

1. **Sliders / host automation** — the ordinary path, through parameters.
2. **MIDI clips** — a per-parameter CC map. Incoming CC fills an override that is stamped
   into the snapshot on the audio thread. **Last-touched-wins**: moving a slider releases
   that parameter's override.
3. **Key Map** — MIDI notes mapped to presets. A held note wholesale-replaces the
   snapshot with a pre-resolved preset image, published to the audio thread lock-free.
4. **Performance controllers** — a Launchpad-style 8×8 pad grid where each 4×4 quadrant
   drives one Scene component and each pad recalls that component's preset slot
   (beat-quantized), plus a 24-encoder knob bank that drives *parameters* through real
   host parameter sets (so sliders follow and hosts record automation), with soft-takeover
   pickup and both context-aware and hand-assigned page modes. Raw MIDI crosses from the
   audio thread to the GUI thread through a wait-free mailbox.

The audio thread's `process()` is allocation-free throughout: pre-allocated state,
lock-free reads, atomics.

---

## 4. The founding algorithm

The organic motion in generator zero comes from two things a naïve port gets wrong.

**Rotate, then translate.** The transform is composed rotation-first, mirroring the
original OpenGL call order. Translation therefore happens *inside the already-rotated
frame*, so every step is a small screw motion, and a loop whose rotation grows with its
index sweeps its nodes around an arc — spirals, coils, helices, instead of a lattice.

**The accumulating strand.** A fourth loop compounds transforms without ever resetting —
turtle graphics, but read as differential geometry: the discrete integration of a moving
frame along a path. That is how a tendril coils and how a shell is laid down.

The angle driving it is a periodic function whose phase grows with node index — a phase
gradient, which is a travelling wave, which is how most of the sea swims without fins.

Two structural choices make it controllable: translation has a **unit base grid**, so the
default state (every amplifier at zero) is a clean cube of cubes and each deformation is
an amplifier over a clean identity; and the per-axis rotation modifier is a *speed* the
clock integrates rather than a static offset. An origin mode selects whether the grid's
corner sits at the world origin (the historical look, each rotating arm pivoting off it)
or whether the field is re-centred so the middle node is the un-rotated pivot.

---

## 5. The generator system

**The pluggable stage.** A generator emits geometry; everything downstream —
surface, material, lighting, post-processing, camera, beat — is generator-agnostic. This
is the property that makes the engine's breadth affordable: adding a natural law costs
almost nothing, because it inherits the entire pipeline.

### 5.1 The contract

A generator emits **strands**: ordered polylines of oriented frames, where a frame is a
position, an optional tangent and normal, a scale, and a colour tint. It declares a
**topology** — Grid, Streamlines, or Tree — which says how its strands relate to each
other.

Downstream, one lowering step turns strands into renderer primitives (a per-instance
model matrix and tint), and one lofting step skins Grid generators into a continuous
membrane mesh. Streamlines and Tree topologies degrade gracefully to swept tubes.

Most generators are a **pure function** of `(parameters, phase)`, evaluated fresh each
frame and unit-tested offline. A few carry persistent simulation state — flocking, the
soft-body bell, the neural cascade — and the pattern for those is settled: the visual owns
the simulation on its app struct, rebuilds it on a structural change, and the dispatch arm
calls `step(dt)` before laying geometry. No trait change, no snapshot growth.

A handful of generators are **not** node fields at all: they are per-pixel raymarched and
run on sibling render paths that bypass strand lowering while still sharing the whole
PBR/IBL/HDR/camera/beat stack.

### 5.2 The 27 generators

Grouped by what they are, rather than by id:

**The seed**
- **Original cube field** — the algorithm of §4.

**Curves, growth, and packing**
- **Frenet–Serret frames** integrated along curvature and torsion.
- **DNA double helix**, respecting the supercoiling identity *Lk = Tw + Wr*.
- **L-systems** — fern, bush, tree, seaweed (Tree topology).
- **Phyllotaxis** at the golden angle, on disks, cones, spheres and shells.
- **Spherical harmonics** — the pulsing bell, which in its *Physical* mode is secretly a
  position-based-dynamics soft body: distance constraints and volume preservation, genuinely
  contracting and recoiling on the beat instead of replaying a waveform.

**Dynamical systems**
- **Strange attractors** — Lorenz, Aizawa, Thomas, Halvorsen, integrated with RK4
  (continuous ODEs, Streamlines).
- **Density-map attractor** — a *discrete* iterated complex map rendered as a point cloud;
  its parameters walk a closed, beat-locked loop in parameter space, so the whole field
  morphs and returns home on the bar. An overlay draws the live trajectory in
  parameter space: *you are here in chaos-space*.
- **Curl-noise flow** — divergence-free ink and smoke.
- **Boids** — Reynolds flocking, the first stateful generator; trails become strands and
  the beat can pulse the goal attractor.

**Fields**
- **Maxwell field** — real charges and dipoles evaluated at retarded time. The dipole
  oscillation can free-run or phase-lock to the beat clock as an LFO, and when locked the
  magnetic swirl reverses *with* the electric wave (the far-field radiation relationship),
  with a phase dial that walks continuously from far-field to near-field induction.
- **Circular polarization** — the E/B helix fan.
- **Synchrotron radiation** — the Liénard–Wiechert field of a relativistic charge orbiting
  a circle, solved at the retarded time of the *moving* source by Newton iteration. Both
  the velocity term and the relativistically beamed radiation term; field arrows, traced
  field lines, or an extruded field volume; the orbit itself can tilt and precess.
- **Acoustic field** — a radiating sound source as a two-channel field: signed harmonic
  monopoles superposed into monopole/dipole/quadrupole, retarded in time. Scalar
  **pressure** drives the geometry (a breathing multipole shell), vector particle
  **velocity** drives the particle aura, and the near term is 90° out of phase with the
  pressure — antinodes at the nodes. A cavity model swaps the radiating multipole for a
  rectangular standing-wave eigenmode whose pressure nodal planes are 3-D Chladni figures.
- **Vector field** — the classic vector-field plot in three dimensions: a curated function
  bank rendered as arrow lattices, RK4 field lines seeded several ways and traced
  bidirectionally through each seed, or a stream *surface* lofted into a flowing sheet.
  Plus a **function builder** in which each component of F is assembled from terms of
  `gain · func(a·x + b·y + c·z + phase)`, optionally passed through a gradient, curl, or
  Helmholtz operator — every knob a host parameter, so the function itself is automatable.
- **Field Engine** — the generalization: an arbitrary closed-form field equation over
  (x, y, z, t), parsed into a small stack-machine bytecode and evaluated per sample.
  It returns a scalar, a vector, or a complex value, and the return kind picks the
  renderer (field lines and aura for vectors, a glyph lattice for scalars, a
  phase-tinted |ψ|² lattice for complex). The vocabulary includes numeric differential
  operators — grad, div, curl, laplacian, advect — evaluated by central differences of a
  wrapped sub-program and freely nestable, so `E = -grad(phi)` and `B = curl(A)` are
  one-liners. Authored from a gallery of phenomena or hot-reloaded from a text sidecar.
  On top of that sits a **time-marched PDE** mode: heat, wave, Schrödinger (norm-preserving
  split-step), and Gray–Scott reaction–diffusion, integrated on a periodic grid off the
  beat clock, explicit and CFL-clamped so it cannot blow up.

**Structure and surface**
- **Aperiodic tilings** — Penrose P3 by inflation *or* de Bruijn cut-and-project,
  Ammann–Beenker, pinwheel, Truchet, hyperbolic {p,q}; rendered as edges, filled tiles,
  extruded prisms, or a true 3-D icosahedral quasicrystal built as a Z⁶ rod lattice. With
  phason flips, Ammann bars, and beat-driven inflation breathing.
- **Minimal surfaces** — dual-path by family. Implicit families raymarch: triply periodic
  minimal surfaces (gyroid, Schwarz P and D), merged soap bubbles, Voronoi/Plateau foam
  with intrinsic thin-film, and an algebraic bank (Clebsch, Barth, Kummer, heart,
  tanglecube). Parametric families emit a (u,v) grid that the membrane loft skins:
  Weierstrass surfaces (Enneper, catenoid, helicoid) and constant-mean-curvature
  unduloids/nodoids with RK4-integrated Delaunay meridians.
- **Kaleidoscopic fractals (KIFS)** — nine spaces including the Apollonian gasket by
  circle inversion and the modular group's tessellation of the hyperbolic plane.
- **Mandelbulb** — distance-estimated raymarch.
- **Lens** — an analytic double-convex or plano-convex lens as exact CSG of spheres and
  half-spaces, sphere-traced; under the path tracer's dielectric mode it actually focuses.

**Bodies and networks**
- **Axon waveguide** — a bundle of myelinated axons treated as step-index optical fibres,
  with periodic Ranvier-node constrictions and a travelling emissive action potential.
  Guided-mode intensity patterns light the bundle cross-section; bend degradation makes
  edge-riding fibres leak while the core mode survives; the bundle can be curved into a
  white-matter arc with arc length preserved (so the pulse flows around the bend), given
  per-fibre tortuosity, and cross-faded toward the diffusion-MRI tractography colouring.
- **Neural field** — a tiny SIREN network mapping (x, y, z, t) to density and colour,
  with weights regenerated from seeds. Dual-path: raymarched as an implicit isosurface, or
  sampled on a grid to displace nodes into ordinary strand geometry. A beat-driven latent
  walk morphs one organism into another.
- **Neural network** — a graph of neuron nodes wired by routed fibre tracts. Synthetic
  topologies (random-geometric, layered feed-forward, ring lattice, small-world, folded
  cortical sheet); a signal-propagation simulation with thresholds, refractory periods and
  per-edge conduction delays; and **ingested real structure**: connectomes and trained
  MLPs (real signed weights, a live forward pass lighting the nodes by actual activations)
  and **transformer attention** (real attention tensors, or a stylized causal synthesis,
  laid out as tokens with a residual-stream backbone and strictly causal attention edges).
  Plus a stylized bilateral **brain model** — two mirrored cortical hemispheres with
  gyri/sulci, cerebellum and brainstem, wired short-range local plus sparse long-range
  association tracts and a corpus callosum, with a standard parcellation of stimulation
  landmarks and a focal coil-like drive whose effect crosses to the contralateral
  hemisphere only when a callosum is present.
- **Creature engine** — a synthetic sea creature assembled as a per-primitive smooth union
  of SDF primitives placed along a spine, sphere-traced. Body plans are built on the CPU
  and mirrored exactly in the shader; a travelling peristaltic domain warp is the swim; a
  metachronal wave of light runs along the body on the beat; body plans are authorable as
  JSON and hot-reloaded; an optional depth-occluded anatomy overlay draws the spine,
  cross-section rings and limb vectors over the living body.

**Utility**
- **Demo (scene bench)** — a hand-authored reference scene (Cornell box, sphere grids,
  a glass menagerie, a light stage with placeable emitters that both bloom and drive real
  analytic point lights) for exercising and validating the ray-tracing stack.
- **None** — the primary generator off, so the scenery layer carries the scene.

### 5.3 The scenery layer

A **second, concurrent generator category**: generated scenery you move *through*, running
alongside the primary generator with its own material, surface and palette.

- **Zone** — a beat-parametrized corridor: archetypes that morph per cell and per phrase,
  with quantized transitions.
- **Terra** — a beat-parametrized flowing landscape (fjords, river banks, canyons) from a
  continuous fBm heightfield, contiguous by construction rather than tiled. The channel
  meanders on a lateral treadmill — geometry is generated channel-relative, so the camera
  flies straight while the valley sweeps beneath — and a navigable channel is *exactly*
  guaranteed open. It grows a water sheet at the per-cell water level, rippling on the
  beat, which reflects the valley walls via screen-space reflections.

Two camera models compose: with a generator visible, the orbit rig stays in charge and the
corridor renders view-locked (glued to the eye, always flying ahead); with the generator
off, a rails camera engages, rail space becomes world space, and the scenery joins the
scene bounds and casts shadows.

---

## 6. From points to geometry: surface modes

Surface modes are orthogonal to generators. The same node field can be rendered as:

- **Instanced cubes** — the classic look.
- **Flow-aligned rods** — each node bridged to its successor by an oriented, stretched rod.
- **Swept tubes** — the same bridging with a cylinder mesh, optionally welded into one
  contiguous mesh, and optionally sweeping the spectrum along each strand's length.
- **Metaball isosurface** — the node set voxelized and raymarched as a blobby surface.
- **Membrane** — a lofted continuous skin over Grid topologies, with seam-closing for
  genuine 360° wraps and an "arms" mode that skins each strand as its own capped finger
  (as welded mesh or as analytic capsule impostors).
- **Voxel** — DDA-raymarched grid-snapped cubes, physically shaded like everything else.
- **Volume** — the field as glowing fog.
- **Gaussian splats** — the node set as anisotropic 3-D Gaussians: additive, IBL-lit 2DGS,
  or relightable with full materials.
- **Plexus** — a generator-agnostic proximity graph: whatever node cloud was emitted gets
  rewired to its nearest neighbours by struts, with morphable node and strut cross-sections,
  optional impostor rendering with *independent* node and edge materials, and a beat-driven
  activation shell that makes the web fire to the music. It can also be layered as an outer
  shell around another surface.
- **Neural tissue** — closed anatomical primitives: soma icospheres, capped capsules and
  boutons, with grown neuron morphology (dendritic arbors with monotone radius taper, a
  hillock axon, terminal bouton arbors), myelinated edges with saltatory conduction, and a
  living synapse (a visible cleft, a deterministic neurotransmitter shimmer on spike
  arrival, cytoplasmic glow by live activation) seated in optional glial and capillary
  tissue context.

---

## 7. Materials

Eight material types, all reflecting the environment and, where enabled, the traced scene:

**Standard** (metallic-roughness PBR) · **Chrome** (prefiltered-environment mirror with
Fresnel) · **Glass** (Fresnel-blended reflection and refraction at a live IOR) ·
**Refractive** (Glass plus Beer–Lambert absorption over the *measured* chord through the
body, so thickness reads honestly) · **Anisotropic** (elliptical GGX lobe streaked along
the instance's long axis — brushed metal, satin, hair) · **Clearcoat** (a thin smooth
dielectric lobe over the base) · **Velvet** (a grazing sheen lobe) · **Subsurface**
(translucent back-glow driven by measured body thickness, so thin edges glow and thick
centres go deep).

Several of these are also available as **overlays** on Standard and Chrome — lacquer a
brushed metal, dust any surface, open a diffuse body into frosted refraction — so the
material space is larger than the type list.

Layered on top:

- **Physical thin-film interference** — a wavelength-resolved Airy reflectance driven by
  an actual thickness field, including a gravity-drainage gradient (thin at the top, thick
  at the bottom) and value-noise marbling. Real soap film, not a cosine hack.
- **Spectral glass** — dispersion as three-tap RGB refraction at offset IORs, thin-film
  tint, and a focusing caustic.
- **Microstructure** — sparse per-facet glitter flakes, diffraction-grating rainbows, and
  retroreflection.
- **Spectral emission** — fluorescence (absorbing the environment's short-wavelength
  irradiance and re-emitting at a chosen hue) and blackbody incandescence by temperature.
- **Screen-space refraction** — for the Refractive material, replacing the environment-only
  transmission with the displaced *resolved scene*, so a glass body shows its neighbours.
- **Procedural materials** — a real PBR texture set (albedo, normal, roughness, metallic,
  AO, height) feeding the one unified material pipeline rather than a parallel path, so a
  loaded brick can be made chrome or subsurface and honours both. Sources: loaded PNG sets,
  or a compute-baked **procedural noise graph** — a curated ~16-entry noise library
  (value, Perlin, simplex, fBm, turbulence, ridged, Worley, Gabor, curl, domain-warp,
  checker, stripes, hex, brick, veins) with layer stacks, blend modes, and *derived* normal
  and AO computed from the same height field, which is what makes stacked-noise materials
  read as real. Materials are authorable as JSON graphs (human- and agent-writable),
  animate over time, and can displace geometry along the height field.

Because most Organon geometry has no UVs, texture projection defaults to triplanar in
world space.

A palette layer (Native plus a dozen cosine gradients) retints fields, including inside
the raymarched shaders.

---

## 8. Light transport

### 8.1 The base

Metallic-roughness Cook–Torrance under **split-sum image-based lighting** — irradiance
map, prefiltered specular mips, BRDF LUT, all precomputed by render-to-texture passes on
load — with multiple-scattering energy compensation so rough metals don't darken, plus
**two analytic directional lights** (key and fill) for the crisp moving highlights IBL
cannot produce.

The default environment is not a photograph. It is a **physically based
single-scattering atmosphere** computed in-shader and baked through the IBL pipeline, so
geometry is lit by a derived sky at the real sun angle, re-baked as the day cycle turns.
A loaded HDR image overrides it; a procedural sky is the always-on fallback.

Because the field's whole identity is self-illumination, the **brightest nodes are
promoted to actual point lights** — so a glowing cube throws a real specular glint and a
coloured pool onto its neighbours. Selection is hysteresis-stabilized with a fade envelope
(a raw per-frame top-N re-sort pops on animated fields), and an optional **ReSTIR** mode
replaces the hard top-N with weighted reservoir sampling, giving every emitter a
luminance-proportional chance so dim and off-screen emitters rotate into the set over
time.

### 8.2 The stack

Each of these is optional, and each is inert-by-default in the sense that turning it off
reproduces the previous image exactly:

- **Shadows** — a key-light shadow map with a corner-fit, texel-snapped frustum and
  slope-scaled bias, PCF-sampled.
- **Ambient occlusion** — GTAO (horizon-based visibility integration) with a depth-weighted
  joint-bilateral blur; the blurred AO also drives specular occlusion on the environment
  lobes.
- **Reflections** — screen-space reflections with step-scaled hit bands, bisection
  refinement, and stochastic GGX-cone roughness jitter; output is premultiplied by a
  confidence weight also stored in alpha, so the composite *blends* by confidence instead
  of double-counting energy.
- **Diffuse GI** — screen-space GI for one bounce; a band-1 spherical-harmonic probe grid
  for directional colour bleed; and **voxel cone tracing**, where the field is scattered
  into a radiance volume by per-node atomic splatting each frame and cone-marched for both
  diffuse bounce *and* world-space reflections that see off-screen emitters.

### 8.3 Hardware ray tracing

Where the GPU exposes ray queries, an acceleration structure is rebuilt each frame from
the same instance transforms the raster path uses (the field animates every instance every
frame, so it is a full rebuild, and its cost is reported live in the editor). On top of it:

- **RT shadows** — per-pixel any-hit rays toward the key and fill lights into a
  screen-space visibility mask, consumed at the same seam the shadow map used. Ground-truth
  occlusion with no bias or frustum tuning, and the fill light gains shadows the map never
  had. Softness comes from the light's angular size via per-pixel cone jitter.
- **RT reflections** — closest-hit traces shaded *from the hit instance's own geometry* by
  inverting its transform, with an optional traced shadow ray so reflections contain
  shadows. Unlike SSR there is no screen-edge fade: off-screen and behind-camera geometry
  reflects, which is the point.
- **RT ambient occlusion** — short cosine-weighted hemisphere rays filling the same raw-AO
  target GTAO writes, so the blur and composite never know which source produced it.
- **RT diffuse GI** — a per-pixel cosine-hemisphere gather, one bounce of real inter-node
  colour bleed including off-screen emitters, written into the same buffer SSGI fills.
- **Photon-mapped caustics** — a light-tracing pass that fires photons from the key light
  and splats their landings into a per-pixel accumulation buffer.
- **A progressive path tracer** — a whole-image trace against the acceleration structure
  with next-event estimation, accumulating while the camera is still. It can replace the
  raster image, blend over it, or contribute indirect-only. An opt-in dielectric BTDF makes
  glass a real two-interface dielectric (exact Fresnel split, refraction on entry *and*
  exit, total internal reflection, Beer–Lambert absorption through the body), and a
  hero-wavelength spectral mode refracts at a per-wavelength Cauchy IOR reconstructed to
  RGB through the CIE colour-matching functions — so a prism throws a real spectrum and the
  analytic lens actually focuses.

### 8.4 Sampling and denoising

All stochastic directions are rotated by texture-free **spatiotemporal blue noise**
(interleaved-gradient spatial dither times a golden-ratio temporal advance), so the error
lands in a high-frequency pattern the eye, TAA, and the bilateral filters resolve far
better than white noise.

Above that sits a denoising ladder: an edge-aware **à-trous** spatial filter stopped by
world-position distance and relative luminance; a **temporal accumulator** that reprojects
history by camera motion, neighbourhood-clamps it, and *beat-relaxes* the history weight
so a strong kick drops history rather than smearing it across a fast auto-orbit camera;
and full **variance-guided SVGF** — history-length-adaptive blending plus a
luminance-variance clamp, so a single firefly no longer swells the clamp box.

Two neural rungs sit on top of the classical stack, both built so that "off" is
byte-identical to the classical result:

- A **kernel-predicting denoiser**: the classical bilateral tap weight is the base, and a
  tiny seeded MLP predicts a *bounded* multiplicative modulation of it from local edge
  features. The modulation is an exponential of a clamped argument, so an untrained network
  can never drive a tap negative or to infinity.
- A **learned upscaler**: the dynamic-resolution upscale becomes an HDR-safe
  content-adaptive sharpen whose gain rides the same seeded MLP, with a contrast dead zone
  so flat and noisy regions are untouched.

And below them, the substrate for the endgame: a **neural radiance cache** — a small
network *trained online during rendering*, with hand-derived backpropagation verified by a
finite-difference gradient check and a convergence test, so paths can terminate early into
a cache query instead of tracing on.

---

## 9. Media, world, and the frame

### 9.1 Participating media

- **Particle aura** — additive HDR billboards, or opaque **sphere-impostor droplets** that
  reconstruct a normal and depth from the billboard UV, shade with the full PBR/IBL stack,
  occlude each other, take on materials and SDF shapes, and write into the depth prepass so
  the screen-space effects treat them as first-class geometry.
- **A Navier–Stokes fluid solver** with RGB dye transport, solid boundaries from node
  occupancy (so wakes shed off the structure), heat-driven buoyancy, beat-gated splashes
  and dye injection, and substepping. The dye is raymarched into the scene with
  Beer–Lambert extinction, Henyey–Greenstein scattering with a short self-shadow light
  march, ambient in-scatter from the IBL irradiance map, and render-time curl-noise
  micro-detail so a coarse grid reads finer than it is.
- **An MLS-MPM liquid** — a free-surface particle liquid in an invisible tank, with the
  generator's own nodes as moving no-slip colliders, whose density feeds the metaball
  isosurface so the full material stack applies (Glass renders it as water). Container
  shapes include a boundless soft-absorbing shell so the liquid trails off into space
  rather than plating a box. A dedicated refractive-water mode snapshots the resolved
  scene, marches the field, splits at the live IOR, Snell-refracts, and fetches the scene
  at the bent ray's landing.
- **One world, one light.** The medium is coupled into the light transport in both
  directions: fluid dye and liquid occupancy fold into the GI voxel volume; a light-space
  map carries Beer–Lambert dye transmittance from the key light so the medium shadows the
  geometry, while the ink march reads the scene shadow map so the geometry shades the
  smoke; caustics are computed by firing one key ray per texel, finding the liquid
  isosurface, refracting at the field gradient and splatting where it lands; the fluid
  itself receives GI; and a two-way coupling samples the fluid velocity at each node and
  integrates a damped displacement spring, so the structure sways in its own wake.

### 9.2 The world layer

Global display layers drawn behind any generator, sharing the camera and HDR pipeline:

- **Terrain** — a raymarched infinite fBm landscape with multiple noise flavours and
  palettes, a day→night sun cycle, atmospherics and god-rays, and reflective water.
- **Volumetric clouds** — a raymarched coverage/erosion layer with forward scattering and a
  sun light march for silver linings, casting soft shadows on the land.
- **An FFT ocean** — a Tessendorf wave field from a Phillips spectrum, inverse-FFT'd on the
  CPU into height, normal and foam tiles (foam from the displacement Jacobian). With the
  landscape off, the ocean fills an infinite open-ocean world.
- **A starfield** that is not noise: the Yale Bright Star Catalog, 9,110 real stars,
  rotated into world space by latitude and sidereal time, fading in as the simulated sun
  sets, with a companion HDR sun disc.

### 9.3 The frame

The scene lives in linear 16-bit-float radiance from the first fragment to the final
operator.

**Bloom** is a soft-knee bright-pass into a downsample/upsample chain, with a Karis average
on the first downsample so sub-pixel fireflies don't flicker-bloom, and energy normalized
by mip count so window size no longer changes bloom brightness.

**Composite** applies exposure in EV stops, adds GI, multiplies AO, blends reflections by
their confidence, adds bloom, and tone-maps — a chosen operator (ACES, ACES-fitted,
AgX properly linearized, Reinhard, or neutral) plus output dither to kill 8-bit banding.

**True HDR.** On macOS the swapchain goes to extended dynamic range: the SDR look is
preserved below a knee and highlights re-expand into the display's *measured* headroom, so
HDR is "the SDR look plus brighter highlights" rather than a flat washed-out ramp. The
surface is tagged Rec.2020 when wide gamut is on, with a vividness dial that stretches
toward the wide primaries. Built for a triple-laser projector and confirmed on one. On
Windows the same composite reaches scRGB HDR natively through wgpu — the display's
maximum luminance and SDR white level are read from the OS, and only the headroom number
crosses the platform seam; wide-gamut Rec.2020 output remains macOS-only.

**Temporal.** TAA is a real jittered supersampler — a Halton sub-pixel jitter on the scene
matrices, reprojection with the unjittered matrices, neighbourhood clamping in YCoCg, and
depth-dilated velocity so foreground motion wins at silhouettes — sharing its pass with
camera-velocity motion blur.

**Creative post** — pixelate, depth of field whose focus rides a perceptual ramp, chromatic
aberration, NPR styles (toon, outline, halftone, dither), colour grading, halation whose
red channel scatters farthest because on film it actually does, lens flares anchored to the
key light's true screen position, vignette, grain, and feedback trails that decay through
float history buffers so they fade like phosphor instead of freezing.

**A scene kaleidoscope** folds the fully lit HDR buffer back into itself through N-fold
symmetry before bloom, so the shards are the real, moving, PBR-lit scene rather than a
post-hoc pattern.

**Capture.** With a fixed output aspect, the whole pipeline renders into a fixed-resolution
production texture whose size (not the window's) drives the projection — so an external
capture is pixel-exact — and a final pass letterboxes it into the swapchain. An **in-app
recorder** reads that same texture back and pipes it to ffmpeg: H.264 Rec.709 for SDR, and
for HDR the float radiance PQ-encoded on the CPU to Rec.2020 10-bit HEVC. So the file is
the render itself, not a re-tone-mapped screen grab. Takes can auto-stop on a bar boundary,
audio is muxed in from the plugin's own output ring, and a fixed-timestep "perfect capture"
mode drives the animation at exactly 1/FPS per frame and captures every frame 1:1 — a
deterministic offline render that matches the viewport frame for frame.

**An overlay** draws the mathematical account of what is on screen — title, description,
the TeX formula, and a live readout panel whose values are the formula *plugged in*,
updating every frame — laid out inside the production rect so it tracks the letterbox.
Its per-generator metadata is pure and unit-tested. A companion decoration draws shaded
axis tubes with conical arrowheads and gridded back walls only, so the reference box reads
as a room rather than a busy lattice.

---

## 10. It plays in time

One machine couples the renderer to music. All of it is CPU-side; none of it touches a
shader.

- **A phase-locked loop** slaves the visual's beat clock to the host transport: a
  continuous beat accumulator free-runs each frame at the active BPM and is gently pulled
  toward the host's position, so tempo changes feel like a drummer adjusting rather than a
  metronome snapping. The tempo source can be the host, a BPM *detected from the audio*
  (which holds its last estimate through a breakdown), or a manual dial. No animation math
  runs on the audio thread.
- **A camera that rides the beat.** Each beat crossing kicks angular velocity into an
  orbiting camera that carries momentum and rings down. Above that sits a **shot
  sequencer** running off the bar clock: camera moves cycle on bar boundaries with glide or
  cut transitions, a decoupled dolly breathes the radius on its own period, and the move
  set includes roll (a dutch tilt via an up-vector rotation), FOV and dolly-zoom, and
  lateral truck. Shot order can be series, random, shuffled or weighted, with hold
  probabilities and phrase-locked facing — or overridden entirely by an authored
  **storyboard** of shots with their own bar counts, plus a bar-quantized "next shot"
  trigger.
- **Pulse routing.** Two slots send a decaying beat envelope into any target parameter with
  bipolar depth, made musical per-target by a span table. Plus a logarithmic **speed pulse**
  and a universal **breath** scene scale, each with its own attack and decay.
- **Audio reactivity.** The plugin analyses its input into band envelopes that can drive
  the same machinery as the synthetic beat.
- **Calibrated metering.** Running beside the expressive analyzer is a metrologically
  honest measurement layer: BS.1770-4 K-weighting, momentary/short/integrated LUFS with
  proper gating, loudness range, 4× oversampled true peak, stereo correlation, and an
  IEC-fractional-octave (or linear-FFT) spectrum analyser with A/C/Z weighting and
  fast/slow/peak-hold/Leq averaging — all pre-allocated and real-time-safe, and unit-tested
  against the standard's defined values. It surfaces three ways: a numeric HUD in the
  visual, a full meter/RTA/spectrogram/oscilloscope page in the editor, and — as the
  **Field Chamber** — analyser panels composited onto the back walls of the reference box,
  so the room the object sits in *is* the instrumentation.
- **The field speaks.** A synthesis engine sonifies the same field kernels the renderer
  draws: field-probe listener microphones, an oscillator lattice, modal struck cavities
  tuned to the eigenmodes, a granular aura of probes advected through the field velocity,
  and a scanned-geometry wavetable that plays the shell's pressure cross-section. The
  audio-driven path runs the other way too: broadband level scales the Maxwell dipole's
  source amplitude, and the five band envelopes drive *distinct multipole moments* — band
  *b* realized as the textbook order-*b* linear multipole — so the field's spatial shape
  encodes the spectrum through honest interference, with per-band attribution for
  colour-by-band tinting.

The stance throughout: audio modulates the *parameters of a source*; the rendered field
mathematics stays real. The 20 Hz–20 kHz carrier is never itself rendered, and that is
stated rather than glossed.

---

## 11. Watching a mind think

One lane is **real-time visualization of local language models**, and it exists in two
coupled forms: a set of capabilities inside full Organon, and a standalone edition,
**Organon Mind**, that puts them front-of-house. Mind has its own crate over the shared
engine, and it carries no plugin framework at all.

### 11.1 What is actually built

- **A GGUF reader.** A header parser that reads a model file's metadata and tensor
  directory — quantization families, per-tensor byte sizes, bits per weight, KV-cache cost
  — without ever touching a weight byte. From that alone it builds the **architecture
  specimen**: the model's true wiring, transformer blocks laid out as planes along the
  residual-stream backbone, attention heads as a per-layer ring, per-tensor declared sizes
  as edge weights — fed into exactly the same neural-graph rendering path the connectome
  and attention lenses use, so the whole surface/material/lighting stack applies to it.
- **An embedding galaxy.** A GGUF *payload* reader with dequantization for the common
  quant families, and a bit-deterministic streamed PCA that projects the embedding matrix
  into a navigable point cloud — labelled in the UI as a projection, because it is one.
- **An activation ring.** A separate memory-mapped channel from the control snapshot,
  carrying per-token frames — per-layer norms, MLP activity, per-head summaries — from a
  writer to the visual and the editor. It is deliberately off the control snapshot because
  a token-rate stream should not churn a control-rate layout. The frame layout already
  carries appended blocks for the residual trajectory and per-layer logit lens, sparse
  mixture-of-experts routing, and SAE features — each zero-means-absent, so writers can
  light them up independently.
- **A real inference runtime.** An embedded llama.cpp build (an opt-in feature, so the
  default build never links C++) that loads the `.gguf`, runs live inference on a typed
  prompt, streams per-token frames into the ring and text back through a sidecar. A
  model-free synthetic writer exercises the entire live path with zero inference, which is
  how the plumbing is tested without a model.
- **An in-plugin console.** The runtime is spawned as a managed child with piped stdio, a
  bounded log ring, and a command REPL — no separate terminal.
- **A single-process workstation editor.** Organon Mind's editor draws its interface
  directly on the renderer's own wgpu device — the scene is not a picture inside the UI;
  the scene *is* the window, with the interface floating over it. Two shapes, switchable
  live: **workstation** (the scene as a docked pane under the tab bar, beside a model
  dock and a live-telemetry dock) and **immersive** (the scene as the whole window, the
  interface floating). The separate visual window remains as the projector path.
- **Live telemetry.** Editor-side widgets read the ring directly: peak-hold and auto-gain
  activation displays, per-token effort scrolling, tokens per second.
- **A log.** Every prompt, reply, plan, action and rejection is appended to a JSONL corpus.

### 11.2 The honesty ledger

The Mind lane commits to three things: structure is read from the file, the live signal
comes from the real forward pass, and every projection is labelled *as* a projection.
Concretely, what is displayed carries a provenance marker:

| Displayed | Provenance |
|---|---|
| Layer / head / expert counts, dimensions, vocabulary, quantization mix | **measured** (from the file) |
| The specimen's wiring | **measured** |
| Parameter counts, weight bytes, bits/weight, KV cost | **derived** (exact functions of the tensor directory) |
| The embedding galaxy | **projection — labelled** (a streamed PCA of the real embedding matrix) |
| The per-layer glow during generation | **proxy — labelled, pending verification**: entropy and confidence, not real activations |

That last row is the lane's number-one honesty gap, and it is mid-closure: the real
activation tap — reading per-layer tensors out of the inference graph through a safe
evaluation-callback API — is implemented in the runtime, which reports on its first token
whether the tap measured real activations or fell back to the proxy. The ledger keeps the
row at *proxy* until a run on real hardware confirms the measured path, because the
ledger records what is confirmed, not what is implemented.

### 11.3 In flight

Packaging: no `.app` bundle exists yet — Organon Mind runs from the build directory, and
the standalone bundle (own name, icon, embedded visual, own namespace) is planned work.
And the activation-tap confirmation above is a run-on-hardware away from flipping the
ledger's last row.

---

## 12. The console

The **Organon Console** is Organon used as an agent-operating workstation — the third
edition and the newest lane, not a separate product. When you are using the console you
are using Organon. (It grew under the working name *Organon Console*; the code now spells the
product name throughout — the `organon-console` crate and binary, the `console-edition`
feature, `CONSOLE_ARCHITECTURE.md` — and the only survivals of the old name are the
`organon-shell` IPC namespace and the `ORGANON_SHELL_*` variables, both of which other
processes read.) Its current form is a **GPU terminal with the engine behind
the glyphs** — tabs of agent harnesses (Claude Code, Pi, and friends, plus a plain
shell) drawn as a real terminal emulator inside a wgpu window, with the Organon world
rendered underneath.

What exists right now:

- **A real terminal core.** A PTY per tab with a full VT state machine, advanced by a
  lock-free pull loop; the grid draws through the same UI stack as the editor, with the
  full colour system, scrollback, bracketed paste, and a key table pinned by test. It
  answers terminal device queries — the handshake Windows' ConPTY blocks on — so the same
  code runs on macOS and Windows, including WSL-hosted harnesses.
- **A harness registry.** Each tab runs a registered agent harness resolved from PATH,
  with user-defined harnesses merged from a JSON file; command keys stay chrome-side, so
  a harness can never see or shadow the host's tab controls.
- **The living backdrop.** The engine's world renders window-sized under the glyphs,
  behind a legibility scrim whose floor is structural — no setting can trade the glyphs
  away. Summoned, never imposed.
- **A self-steering loop.** The console publishes its own control snapshot in its own
  IPC namespace, so the `organon` CLI works from *inside* its terminal: an agent running
  in a console tab can read the live state and switch the generator of the very backdrop
  it is sitting on.

The console is standalone-only permanently, an edition like Mind, with its own
namespace — a console session, a Mind session and a plugin session can run
simultaneously without trampling each other. The deeper workstation — command surfaces,
viewport interaction, richer agent panes — is planned work, *in flight*, with its
groundwork (session/event log, typed command service, event cards) already in the crate.

---

## 13. The agent lane

Organon can be played by a language model as well as by hands, through the same
last-touched-wins discipline as every other input.

- **An in-process performer.** A worker thread in the visual runs an agentic loop against
  an OpenAI-compatible localhost endpoint (Ollama, LM Studio, llama.cpp, MLX — the endpoint
  and model name are configuration, not compiled in). Its action set covers setting
  parameters, selecting generators/surfaces/materials, applying and saving presets, and
  reading back state. Actions dispatch onto an **override lane** the visual applies before
  geometry and look are built — the same seam pulse routing uses.
- **Parameters remain the single source of truth.** The lane renders the change
  immediately, but it would otherwise bypass the plugin's parameters and leave the sliders
  lying. So each applied action is also mirrored back to the editor, which applies it to the
  *real* parameters through the host's setter — so sliders and dropdowns follow the agent,
  presets capture what the agent did, and the lane then hands off to the parameter.
- **The vocabulary is generated, not maintained.** The action catalog comes mechanically
  from the same slot table that generates the packers, so a new parameter appears in the
  agent's vocabulary automatically. A curated one-line gloss per actuatable parameter is
  enforced by a test — a new actuatable parameter does not pass CI until it is described.
  The same described catalog generates the user-facing reference documentation
  (`organon docs` → `doc/reference/`), pinned by its own drift test.
- **A CLI for external agents.** The `organon` binary opens the same lane to the terminal
  and to local agents with no daemon and no protocol server: reads decode the live snapshot
  directly (with a liveness probe that distinguishes "not running" from "defaults"); writes
  append command lines the visual drains by self-detecting file growth, with cursor
  discipline such that a failed read commits nothing and commands issued while the visual
  was down are never replayed. `catalog`, `describe` and `recipes` make the whole vocabulary
  and a set of described built-in starting points queryable, so an agent can build a look
  from an empty preset store.
- **Eyes.** `snap` and `record` close the see→act→see loop: they ride a request/reply
  channel, because unlike fire-and-forget commands they need the visual to do GPU work and
  hand a file path back. The visual forces the production-texture path for that frame, reads
  it back, writes a PNG, and replies with the path.

---

## 14. Engineering discipline

The engine's breadth is only affordable because a handful of invariants are enforced
mechanically rather than remembered.

- **Append-only IPC.** Offsets never move; new blocks go at the end. Golden tests pin the
  struct size, key offsets, and a hash of the whole default snapshot, so a "byte-identical"
  refactor has to prove itself.
- **One packing table.** Both packers are generated from one ordered slot list; a renamed
  field is a compile error, not a silent zero.
- **Off-by-default means byte-identical.** Nearly every feature in this document ships with
  a default that reproduces the previous image exactly, and that claim is usually pinned by
  a test. It is what makes an engine this large safe to keep extending.
- **The pure core is testable without a GPU.** Every generator's geometry, the compose step,
  the GI probes, the fluid projection oracle, the MPM mirror, the neural-network gradient
  check, the parser and layout reducers — all unit-tested on CPU. Current bar: about
  **1,300 test functions** across the workspace.
- **Shaders are validated offline.** All **54** WGSL shaders are parsed and validated with
  naga before any GPU sees them, which catches binding, type and uniformity errors in CI.
  It cannot catch pipeline/layout mismatches or how something looks — those need a GPU.
- **The frame itself is gated mechanically where a GPU exists.** A verification harness
  launches the visual on a private IPC namespace, drives it through the `organon` CLI,
  snaps frames, and checks each scene three ways: it drew something, it is animating, and
  it matches a committed golden within a per-pixel difference budget.
- **Large refactors are proved, not reviewed.** The big mechanical moves — hoisting the
  editor body, partitioning the world's state — ship with checker scripts that diff the
  result against a mechanical rewrite of the base commit and fail on any deviation, so a
  type-compatible mis-mapping cannot hide in a thousand-line diff.
- **CI builds every edition.** The default build compiles neither the Mind nor the Console edition, so a
  green default suite says nothing about them; CI builds and tests each edition, plus
  Windows legs, on every pull request, and every PR closes at least one automated review
  cycle.
- **What the cloud cannot verify is stated as such.** A finished remote pull request is
  "green and ready to deploy", never "verified working". GPU look, feel and performance, the
  DAW integration, MIDI behaviour, true-HDR output on an EDR display, and projector-resolution
  performance are all verified on the machine, by hand.

**Rough scale:** ~110 Rust modules and 54 shaders across a five-crate workspace; ~1,370
host parameters (~1,170 of them captured by presets); 27 generators; 10 surface modes;
8 material types; an ~8.5 KB control snapshot; 8 binaries from one workspace.

---

## 15. The extension seams

The architecture is shaped so that adding a natural law is cheap. The four patterns:

1. **A node-field generator** — write the pure strand function and its tests in `math.rs`,
   add the enum variant and parameter block, add the dispatch arm and the editor card.
   Everything downstream (surface, material, light, post, camera, beat) is inherited.
2. **A raymarched generator** — the same parameter and preset wiring, but instead of
   strands, a new render path with its own distance-estimator shader and pass.
3. **A parameter** — declare the field, the preset mirror, and *one* slot in the packing
   table; both packers are generated. The layout golden fails until it is re-pinned, on
   purpose — and the described catalog (and with it the agent vocabulary and the generated
   reference) fails CI until the parameter carries its one-line gloss.
4. **A render subsystem** — a module plus a validated shader, threaded through the render
   frame's relevant sub-struct rather than a new positional argument, plus a pass.

Stateful generators need no trait change: hold the simulation on the visual's app struct,
step it with a fixed-dt accumulator and a stored seed for determinism, then emit strands
like any other arm.

---

## 16. In one paragraph

Organon is a parametric generative visual engine that runs as a DAW plugin with its
renderer in a separate process, so a physically based light-transport pipeline —
metallic-roughness PBR under split-sum IBL off a computed atmosphere, analytic key and
fill lights, emitters promoted to real lights by reservoir sampling, GTAO, screen-space
and hardware-ray-traced reflections/AO/GI, a spectral path tracer, an SVGF and neural
denoising ladder, and a fully linear HDR chain that reaches true EDR output in wide gamut
— can be pointed at 27 interchangeable mathematical generators behind one geometry
contract, from the original cube field and Frenet–Serret frames to Maxwell and acoustic
fields, aperiodic tilings, minimal surfaces, arbitrary field equations with a PDE solver,
and the live internals of a language model. Every parameter is host-automatable, the beat
clock is phase-locked to the transport, and the camera, the modulation routing, the media
simulations and the audio analysis all run off that one clock. And all of that is one
*arrangement* of one application — the window divides into named regions, each declaring what
it holds, always including a live agent that reaches the same verbs a human types; the
visualizer is the region content Organon grew out of rather than the thing Organon is. It
ships three ways from one workspace today — the plugin, the Mind arrangement for watching a
language model think, and the console for working with agents, its terminal glowing from
underneath — and of those three the plugin alone can never become a layout, because a host owns
its window and its lifetime.
