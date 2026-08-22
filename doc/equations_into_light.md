# Equations into light — the Organon visualizer

**A physically based light-transport engine that lives inside your DAW, pointed at
fifty years of beautiful mathematics.**

> 📌 **This essay is about one thing Organon hosts, not about what Organon is.** Organon is one
> native application whose identity is data — you divide the window into regions, declare what
> each holds, and save the arrangement under a name — and the generative-math visualizer
> described here is the arrangement it grew out of, built in and conceptually a module like any
> other. `doc/organon_prd.md` §1.1 is the canonical description of the whole.

The visualizer is built in Rust on wgpu and ships inside a
VST3/CLAP plugin whose renderer runs as a separate fullscreen process. The plugin
is deliberately thin — a control surface exposing ~1,370 host-automatable
parameters — and the visual is a full HDR rendering engine that reads a
shared-memory snapshot at frame rate and turns equations into light. Every knob
can be automated, MIDI-learned, beat-pumped, or driven by the audio itself. It
began in 2000 as an OpenGL hello-world that got out of hand — a color cube, then
a cube of cubes, then sliders — and that original algorithm is still generator
zero.

## One rule, applied relentlessly

The founding machine is small: a transform composed rotation-first (**R·T** — the
order matters, because translating inside the already-rotated frame makes every
step a small screw, and a sequence of screws sweeps arcs, coils, helices), plus a
fourth loop that compounds transforms without ever resetting. That accumulating
strand is turtle graphics elevated to differential geometry — the discrete
integration of a moving frame along a path, which is how a tendril coils and a
nautilus gets laid down. The angle driving it is a sine whose phase grows with
node index: a phase gradient, which is a traveling wave, which is how nearly
everything in the sea without fins or legs swims. The default state is a perfect
cube of cubes; every deformation is an amplifier on top of a clean identity. I
didn't set out to model a jellyfish. The jellyfish was already in the math.

## Twenty-seven ways to grow a universe

The field is fed by a pluggable **generator stage**: anything that emits strands
of oriented frames gets the entire engine downstream for free. One contract,
twenty-seven generators, none of the mathematics invented here:

- **Frenet–Serret frames** integrated along curvature and torsion. A **DNA double
  helix** that respects the supercoiling identity L = T + W. **Strange
  attractors** (Lorenz, Aizawa, Thomas, Halvorsen) integrated with RK4.
  **Maxwell fields** computed from real charges and dipoles at retarded time — the
  magnetic swirl reversing *with* the electric wave, because in the far field it
  does — and a **synchrotron generator** that solves the Liénard–Wiechert field of
  a relativistic charge by Newton iteration on the retarded-time equation: the
  beamed radiation lobe is the physics, not an artist's impression of it. An
  **acoustic field** whose standing-wave cavity modes trace 3-D Chladni figures,
  90° out of phase where the textbook says they should be.
- A **field engine** that takes an arbitrary closed-form equation over
  (x, y, z, t), compiles it to a little stack-machine bytecode, and renders
  whatever comes back — scalar, vector, or complex ψ tinted by phase. Grad, div,
  curl and laplacian are in the vocabulary as numeric operators, so `E = -grad(phi)`
  and `B = curl(A)` are one-liners. Under it, a **time-marched PDE bench**: heat,
  wave, a norm-preserving split-step Schrödinger, and Gray–Scott
  reaction–diffusion, CFL-clamped so the music can drive it and it cannot blow up.
  And a **density-map attractor** — an iterated complex map whose two parameters
  ride a closed, beat-locked orbit through parameter space while an inset plot
  shows the trajectory live: *you are here in chaos-space*.
- **Aperiodic tilings** — Penrose P3 by inflation *or* de Bruijn cut-and-project,
  Ammann–Beenker, and a true 3-D icosahedral quasicrystal built as a Z⁶ rod
  lattice — plus **minimal surfaces**: raymarched gyroids, algebraic sextics like
  the Barth surface, and constant-mean-curvature unduloids with RK4-integrated
  Delaunay meridians. **Kaleidoscopic fractals** across nine spaces, including
  the Apollonian gasket by circle inversion and the modular group's tessellation
  of the hyperbolic plane; a **Mandelbulb** on its own distance-estimated
  raymarch path; and an analytic **lens**, built as exact CSG of spheres, that
  under the path tracer's dielectric mode actually focuses.
- Stateful lifelike behaviors: **boids** flocking under Reynolds rules, their
  trails becoming strands; **L-systems** growing ferns and seaweed; **phyllotaxis**
  at the golden angle; a spherical-harmonic bell that is secretly an **XPBD soft
  body** — distance constraints, volume preservation — genuinely contracting and
  recoiling on the beat instead of replaying a waveform; and a **creature
  engine** that assembles a sea creature from smooth-unioned SDF primitives along
  a spine and swims it with a traveling peristaltic warp, body plans authorable
  as JSON and hot-reloaded mid-set.
- The **neural lane**: axon bundles as step-index optical fibres with Ranvier
  nodes and a traveling action potential; graphs of neurons wired by routed
  tracts — synthetic small-worlds, real ingested connectomes, trained MLPs whose
  live forward pass lights the units, transformer attention laid out causally
  along a residual-stream backbone; a stylized bilateral brain whose stimulation
  crosses hemispheres only when a corpus callosum is present to carry it; and a
  SIREN implicit organism morphed by a beat-driven latent walk. Load a `.gguf`
  and the model's own wiring, read from the file, becomes the specimen.

And alongside whichever generator is playing, a **scenery layer** you move
through: a beat-parametrized corridor, or a flowing fBm landscape — fjords,
canyons — whose channel meanders under a straight-flying camera and grows a
rippling water sheet that reflects the valley walls.

Plant growth, electromagnetism, chaos, quasicrystals, flocking, a running
language model: nothing apparently common at the surface, one grammar underneath.

Point fields become geometry through interchangeable **surface modes**: instanced
cubes, flow-aligned rods, swept tubes carrying the spectrum down their length, a
metaball isosurface, a lofted membrane skin, DDA-raymarched voxels, an emissive
volume that treats the field as glowing fog, anisotropic **Gaussian splats**, a
**plexus** that rewires any node cloud into a proximity web and fires it to the
music, or closed **neural tissue** — somas, myelinated axons, a synapse with a
visible cleft.

## Light that earns it

Shading is metallic-roughness Cook–Torrance under split-sum image-based lighting
— irradiance map, prefiltered specular mips, BRDF LUT, multiple-scattering
compensation so rough metal doesn't go dark — with analytic key and fill lights
for the crisp moving highlights IBL can't produce. The default environment isn't
a photo: it's a **Nishita single-scattering atmosphere** computed in-shader and
baked through the IBL pipeline, so the geometry is lit by a physically derived
sky at the actual sun angle, re-baked as the day cycle turns.

Then it stacks. A key-light shadow map with a corner-fit frustum and slope-scaled
bias. Horizon-based **GTAO** with bilateral filtering. **Screen-space
reflections** with bisection-refined hits that composite by confidence instead of
double-counting energy. **Screen-space GI** for one diffuse bounce, a **band-1
spherical-harmonic probe grid** for directional color bleed, and **voxel cone
tracing** — the field is scattered into a radiance volume by per-node atomic
splatting each frame, then cone-marched for both diffuse bounce *and* world-space
reflections that see off-screen emitters. And because the field's whole identity
is self-illumination, the brightest cubes are promoted to actual **point
lights** — a hysteresis-stabilized top-N, or reservoir-sampled **ReSTIR** so dim
and off-screen emitters get their luminance-proportional turn — so a glowing
cube throws a real specular glint onto its neighbor. Glass is Fresnel-correct
against its live IOR with spectral dispersion, thin-film interference, and
premultiplied compositing — transparency attenuates what passes through a thing,
never what shines from it.

## Rays, when the silicon offers them

Where the GPU exposes ray queries, an acceleration structure is rebuilt **every
frame** — the field animates every instance, so there is nothing static to cache
— and the raster stack gains ground-truth siblings: per-pixel traced shadows for
both lights, softened by the light's angular size; reflections with no screen
edge, shaded from the hit geometry itself, so what's behind the camera shows up
in the chrome; traced AO and one-bounce GI written into the same buffers the
screen-space passes fill, so the composite never knows which produced them; and
photon-mapped caustics fired from the key light. Above it all, a **progressive
path tracer** with next-event estimation accumulates whenever the camera rests —
replace the raster image, blend over it, or take indirect light only. Its
dielectric mode makes glass a real two-interface medium — Fresnel split,
refraction in *and* out, total internal reflection, Beer–Lambert absorption
through the measured body — and a hero-wavelength spectral mode refracts at a
per-wavelength Cauchy IOR reconstructed through the CIE color-matching functions,
so a prism throws a real spectrum and the lens focuses it.

The noise is managed like a signal chain: texture-free spatiotemporal blue noise
under every stochastic pass; an edge-aware à-trous filter; a temporal accumulator
that **beat-relaxes** its history weight, so a hard kick drops the history
instead of smearing it across a fast orbit; variance-guided SVGF above that; then
two bounded neural rungs — a kernel-predicting denoiser and a learned upscaler,
each built so "off" is byte-identical to the classical result — and, beneath it
all, a **neural radiance cache** trained online *while rendering*, its
hand-derived backpropagation pinned by a finite-difference gradient check.

## Smoke, water, and the wake

The field can sit inside a medium, and the medium is coupled into the light. A
**Navier–Stokes solver** carries RGB dye through the scene — walls formed from
the nodes themselves so wakes shed off the structure, heat-driven buoyancy,
splashes gated to the beat — and the ink is raymarched with Beer–Lambert
extinction, Henyey–Greenstein scattering, a short self-shadow light march, and
curl-noise micro-detail so a coarse grid reads finer than it is. An **MLS-MPM
liquid** sloshes in an invisible tank with the generator's nodes as moving
colliders; its density feeds the metaball isosurface, so Glass renders it as
water, and a refractive mode Snell-bends the resolved scene through it.
Everything is one world under one light: the dye's transmittance shadows the
geometry, the geometry's shadow map shades the smoke, key-light caustics refract
at the liquid surface and land where they should, the fluid receives GI — and
the fluid pushes back, sampling its own velocity at each node so the structure
**sways in its own wake**.

## The last 128 bits

The scene lives in linear Rgba16Float from first fragment to final operator.
Bloom is the 13-tap downsample/tent-upsample chain, with a Karis average on the
first tap so sub-pixel fireflies don't flicker-bloom; tone mapping is your pick
of ACES (stock or fitted), AgX (properly linearized), or Reinhard. On macOS the
swapchain goes **extended dynamic range**: the SDR look is preserved below the
knee and highlights re-expand into measured display headroom, tagged Rec.2020
with a vividness dial that stretches toward the wide primaries — built for a
triple-laser projector and confirmed on one. On Windows the same composite
reaches scRGB HDR natively through wgpu. On top: TAA and motion blur, a depth
of field whose focus rides a perceptual ramp, halation whose red channel scatters
farthest because on film it actually does, lens flares anchored to the key
light's true screen position, NPR styles, feedback trails that decay through
float history buffers so they fade like phosphor instead of freezing — and a
**scene kaleidoscope** that folds the fully lit HDR frame through N-fold symmetry
before bloom, so the shards are the real moving scene, not a pattern pasted on
top.

Behind the field, an optional world: a raymarched terrain, **volumetric clouds**
with Henyey–Greenstein scattering and a sun light-march for silver linings, a
**Tessendorf FFT ocean** (Phillips spectrum, hand-rolled radix-2 FFT, foam from
the displacement Jacobian), and a starfield that is not noise — it's the **Yale
Bright Star Catalog**, 9,110 real stars, rotated into place by latitude and
sidereal time, fading in as the simulated sun sets.

## It plays in time

A **phase-locked loop** slaves the visual's beat clock to the host transport —
free-running between corrections, gently pulled into phase, so tempo changes feel
like a drummer adjusting rather than a metronome snapping. Each beat kicks
angular momentum into an orbiting camera that carries momentum and rings down,
and above that a **shot sequencer** runs off the bar clock: moves cycle on bar
boundaries with glides or cuts, a decoupled dolly breathes the radius, rolls and
dolly-zooms are in the move set, and the whole thing can be overridden by an
authored storyboard with a bar count per shot. Two routing slots pump a decaying
beat envelope into any parameter; a logarithmic speed pulse and a whole-scene
breath ride the same envelope; an FFT analyzer turns the input audio into band
envelopes that drive the same machinery. Driven harder, the analysis becomes
physics: the broadband level scales a radiating dipole's amplitude, and five
band envelopes drive five **distinct multipole moments** — band *b* realized as
the textbook order-*b* multipole — so the field's spatial shape encodes the
spectrum through honest interference.

It listens like an instrument, too: a metrologically honest meter layer —
BS.1770 K-weighted LUFS with proper gating, 4× oversampled true peak, a
fractional-octave RTA — that can composite its scope and spectrum onto the back
walls of the reference box, so the room the object sits in *is* the
instrumentation. And it speaks: a synthesis engine sonifies the same field
kernels the renderer draws, from stereo listener probes placed *in* the field to
modal struck cavities tuned to the bell's eigenmodes.

The renderer doesn't display music. To play is to shape the field in real time.

## The file is the render

With a fixed output aspect, the whole pipeline renders into a fixed-resolution
production texture — the window is just a letterboxed preview of it — so capture
is pixel-exact by construction. An in-app recorder reads that texture back and
pipes it to ffmpeg: H.264 Rec.709 for SDR, and for HDR the float radiance
PQ-encoded to Rec.2020 10-bit HEVC, so the file is the render itself, not a
re-tone-mapped screen grab. Takes auto-stop on a bar boundary, the plugin's own
audio is muxed in, and a fixed-timestep **perfect capture** mode steps the
animation exactly 1/FPS per frame and writes every frame 1:1 — a deterministic
offline render that matches the viewport frame for frame. A typographic overlay
can draw the live TeX formula of whatever equation is on screen — actual values,
plugged in, updating every frame.

## Two processes, one nervous system

Plugin and visual talk through a memory-mapped, append-only C-layout snapshot —
the plugin writes it every audio block, the visual reads it every frame, a
seqlock keeps a reader from ever seeing half of one state blended with half of
another, and a golden test pins the byte layout so the two processes stay
compatible across rebuilds. Every parameter's packing is generated from a single
source-of-truth table where a mistake is a compile error, not a silently
corrupted render. The pure math — every generator, the GI probes, the fluid
projection, the soft-body step — is unit-tested, north of thirteen hundred tests
across the workspace; all 54 WGSL shaders are validated offline through naga
before a GPU ever sees them. Presets capture the whole state and apply it
atomically, quantized to the next bar if you ask; MIDI notes recall them
wholesale; a Launchpad's 8×8 grid recalls them by quadrant while 24 encoders
drive the parameters themselves with soft-takeover pickup, and last-touched
wins, always. A CLI opens the same nervous system to the terminal — read the
live state, set anything, snap a frame, roll a take — which means an external
agent gets hands and eyes with no daemon in between; an in-process AI performer
plays the same lane, its every action mirrored back through the host's parameter
setter so the sliders never lie. And all of it is one *arrangement* of one
application, shipping three ways from one workspace today: this one, inside the plugin;
**Mind** — load a language model and watch it think, every readout labeled measured, derived,
or projection — and the **Console**, a GPU terminal for working with agents, the engine glowing
under the glyphs. Three faces of one program, and the window is learning to hold any of them at
once.

Organon is what happens when you take the demoscene's ambition, a PBR
engine's discipline, and a physics textbook's index, and make them all dance to
a kick drum.
