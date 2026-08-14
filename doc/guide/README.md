# Using Organon

Organon is a parametric generative visualizer. You pick a **generator** (what shape the
maths builds), a **surface** (how that shape becomes geometry) and a **material** (how it
is shaded), and then you play it — from your DAW's transport, from MIDI clips, from a pad
controller, or from a terminal.

It runs as a **VST3/CLAP plugin** and as a **standalone app**. Either way the picture
lives in its own fullscreen window, in its own process, so it can own a projector while
your DAW owns the parameters.

> This guide is the narrative half of the documentation: what the pieces are and how to
> play them. The [reference](../reference/README.md) is the exhaustive half — every
> generator, surface, material, parameter and recipe — and it is generated from the source,
> so it cannot fall behind the code.

## Start here

| | |
|---|---|
| **[Getting started](getting-started.md)** | Install it in your DAW, get a picture on screen, and understand why there are two windows. |
| **[The three choices](concepts.md)** | Generator × surface × material, the beat clock, and what "the visual owns the pixels" actually means for you. |
| **[Playing it](performance.md)** | MIDI clips, the Key Map, the pad controller and the knob bank — and the priority order when several of them want the same control. |
| **[Presets](presets.md)** | Scenes and components, beat-quantized recall, and recording your own defaults. |
| **[Output and capture](output.md)** | Projector setup, fixed-resolution framing for OBS, stills and video. |
| **[The `organon` command](cli.md)** | Driving a running instance from a terminal — the fastest way to explore, and the way to script it. |

## The other two instruments

The same engine ships two siblings. Both are **standalone-only** — neither has a plugin
build, deliberately, and neither is packaged as an app yet, so you run them from a build:

- **Organon Mind** — load a `.gguf` and watch a language model think. Its shape is read
  from the model file; see [`../watching_a_mind_think.md`](../watching_a_mind_think.md) for
  the plain-language account of what is measured and what is a stand-in, and
  [`../../MIND_ARCHITECTURE.md`](../../MIND_ARCHITECTURE.md) for what exists right now.
- **Organon Console** — an agent-operating workstation. See
  [`../../CONSOLE_ARCHITECTURE.md`](../../CONSOLE_ARCHITECTURE.md).

## If you are here to build on it

This guide is for operating Organon. For extending it, read
[`CONTRIBUTING.md`](../../CONTRIBUTING.md) first, then
[`ARCHITECTURE.md`](../../ARCHITECTURE.md).
