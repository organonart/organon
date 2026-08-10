# The three choices

Almost everything you see in Organon is the product of three independent choices. They
compose freely, which is the whole design: you are not picking from a list of finished
looks, you are picking coordinates.

```
  GENERATOR  ×  SURFACE  ×  MATERIAL
  what shape    how it        how it is
  the maths     becomes       shaded
  builds        geometry
```

**Generator** — the engine that builds the form. A rotating field of colour cubes, a DNA
double helix, a strange attractor, the real electromagnetic field of an oscillating
dipole, a raymarched sea creature. Every one of them is listed with a paragraph of its own
in the [generator reference](../reference/generators.md).

**Surface** — how the generator's output becomes drawable geometry. The same helix can be
solid cubes, oriented rods that follow the flow, continuous swept tubes, a fused molten
skin, a lofted membrane, or a cloud of glowing motes. See the
[surface reference](../reference/surfaces.md).

**Material** — how that geometry is shaded: PBR metal, chrome mirror, glass, refractive
glass with absorption, brushed anisotropic, clearcoat, velvet, subsurface. See the
[material reference](../reference/materials.md).

Two things follow from the choices being independent:

- **Any surface works with any node-field generator.** DNA in swept tubes and glass is the
  obvious one; DNA as a cloud of Gaussian splats is also available and looks nothing like it.
- **Some generators emit no nodes at all.** Mandelbulb, the kaleidoscopic fractal, Lens and
  Creature are raymarched per pixel, so surface modes have nothing to act on. Materials
  still apply. Two more — Minimal Surfaces and Neural Field — are **dual-path**: their
  implicit families raymarch, while their parametric ones emit a node grid that surfaces
  can skin.
- **The editor tells you which kind you are on**, by hiding the Surface card when it does
  not apply. That is the fastest check available.

A fourth choice, **palette**, retints the whole thing and is orthogonal again.

## Two windows, two processes

The plugin window is the **editor**. The picture is a **separate process** with its own
window. This is not an accident of implementation; it buys three things you will actually
use:

- **The visual can own a display.** Put it fullscreen on a projector while your DAW stays
  on your laptop screen. Nothing about your session layout constrains the picture.
- **The render cannot stall the audio thread.** The heavy GPU work is in another process
  entirely.
- **The visual survives.** Closing the plugin editor does not disturb the picture.

The two talk through a shared-memory snapshot: the plugin writes, the visual reads, once
per audio block. The visual owns the clock, the camera and any simulation state; the
plugin owns the parameters. That division is why a few things behave the way they do —
notably that some of the `organon` CLI's commands need the editor running and some only
need the visual.

## The beat clock

One clock couples the picture to the music, and most motion in Organon is hung off it.

A continuous beat counter free-runs at the active tempo and gently pulls its phase toward
your host's transport position, rather than snapping. The **clock source** (Sync/Tempo
card, Settings tab) picks where the tempo comes from:

| Source | What it uses |
|---|---|
| **Host (Transport)** | your DAW's tempo and play position |
| **Audio (Detect BPM)** | tempo detected from the audio reaching the plugin |
| **Manual (Dial)** | the tempo slider, ignoring everything else |

Because the clock free-runs rather than following the transport frame-by-frame, the
picture keeps moving musically when you stop the transport — it does not freeze mid-motion.

What rides the clock: the **pulse** envelope (a decaying kick on each beat, routable to
two parameters of your choosing with bipolar depth), **Speed Pulse** (a logarithmic kick to
global speed), **Breath** (a pulse-driven scene-wide scale), the auto-orbit camera's
momentum, beat-quantized preset recall, and a good deal of per-generator motion — the
travelling action potential down a nerve fibre, the parameter orbit of the density-map
attractor, the metachronal wave along a creature.

**Audio reactivity** is a separate switch. With it on, the plugin analyses the audio
passing through into band envelopes, and the pulse can be driven by the audio's bass rather
than by the synthetic beat.

## What is a parameter

Every control in the editor is a real host parameter: automatable, MIDI-learnable, and
captured by presets. There are about 1,370 of them, which is more than any list wants to
be — so there are three ways in, and you should expect to use all three:

1. **The editor**, where they are grouped into cards by subject, and cards that do not
   apply to your current generator are hidden.
2. **The `organon` CLI**, which exposes a curated, stable-id subset built for scripting —
   see [the parameter reference](../reference/parameters.md) and [the CLI guide](cli.md).
3. **`organon describe <id>`**, which explains any one of them in prose, with its range.

The distinction that matters: the CLI subset is small and stable, the editor surface is
complete. If a control has a CLI id, it is safe to script against.
