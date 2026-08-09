# Playing it

Organon is built to be played, not just configured. There are five ways to move a control
while music is running, and they are deliberately different from each other.

## 1. Host automation

Every editor control is a host parameter. Automate it, MIDI-learn it, draw an envelope on
it — your DAW does this natively and Organon does nothing special to help or hinder.

This is the right tool for anything you want **recorded in the arrangement**.

## 2. MIDI clips (the CC map)

A clip of CC data drives the visual **directly**, bypassing the host parameter layer.
Thirty-two parameters are mapped to **CC 16–47** — starting at 16 to stay clear of mod
wheel, volume, pan, expression and sustain.

The mapped slots, in CC order:

| CC | Control | CC | Control |
|---|---|---|---|
| 16–19 | node counts X, Y, Z, Q | 34–36 | ambient, key, fill light |
| 20–22 | rotation amplitude X, Y, Z | 37–38 | sun elevation, azimuth |
| 23–25 | rotation speed X, Y, Z | 39–40 | glow, opacity |
| 26–28 | translation amplitude X, Y, Z | 41–42 | tempo, pulse depth |
| 29–31 | translation offset X, Y, Z | 43–45 | rotation / translation / scale waveform |
| 32–33 | global speed, scale amplitude | 46–47 | animate, pulse |

Several of those are **Original-generator geometry** (node counts, rotation and
translation amplitudes and offsets, the waveforms). On any other generator they exist but
do nothing visible — the lighting, glow, tempo and pulse slots are the ones that apply
everywhere.

Two behaviours worth internalising:

- **Last-touched wins.** If you move a slider for a parameter a clip is currently holding,
  the clip's hold on *that one parameter* is released. This is intended — you can always
  take a control back by hand without stopping the clip.
- **Release MIDI clip** (a button in the editor) drops every CC hold at once.

## 3. The Key Map (notes → presets)

Map MIDI notes to Scene presets. **Hold a note and that preset is the look; release it and
the previous look returns.** It is momentary, it is instant, and it is the highest-priority
input in the system.

Notes are named the Ableton way — **C3 is MIDI 60** — and the editor's keyboard pages
through C0 to C5. Last press wins if you hold several.

The Key Map is the one performance surface that works **with the editor window closed**,
because it drives the shared snapshot from the audio thread rather than through the host's
parameter setter. If you want reliable note-triggered visuals in a live set, this is the
one to use.

> The Key Map maps to **Scene** presets only — the four-tab composite described in
> [presets](presets.md). Per-component presets are not note-mappable.

## 4. The pad controller

An 8×8 pad grid — a Novation Launchpad Mini MK3 by default — played as an instrument.
Each 4×4 quadrant owns one Scene component:

```
  ┌─────────────┬─────────────┐
  │  GENERATOR  │   MOTION    │
  ├─────────────┼─────────────┤
  │    LOOK     │ ENVIRONMENT │
  └─────────────┴─────────────┘
```

Each pad recalls that component's preset slot, **quantized to the next beat boundary** you
have chosen. The arrow buttons page banks of 16 and step the quantize division; Stop/Solo/
Mute cancels a queued recall. The editor draws a mirror of the grid so you can see what is
loaded, what is active, and what is pending.

It is gated behind a **Performance Controller** switch, off by default. That gate is
load-bearing:

> **When it is on, the controller owns incoming notes and CCs.** The Key Map, the built-in
> synth and the clip CC map are all bypassed for that input, so a pad press cannot
> double-fire. When it is off, the surface is completely inert and everything behaves as
> though it did not exist.

Two more things to know before a show:

- **The plugin's editor window must be open** for the pads to drive recalls. Preset recall
  goes through the host's parameter setter, which is a GUI-thread path. The Key Map does
  not have this constraint; the pads do.
- The layout is re-learnable — pads are routed by note number, and there is a learn flow in
  the card. It persists next to your presets.

## 5. The knob bank

The knobs sibling of the pads: 24 encoders (a Launch Control XL's 3×8, by default) driving
**parameters** rather than presets, on the same Performance Controller gate.

Unlike the clip CC map, knob moves are **real host parameter sets** — sliders follow, presets
capture them, and your DAW will record the automation. Two modes:

- **Explore** — the bank follows whatever you are looking at. On the Generator tab it maps
  the selected generator's own parameters; on Motion, Look and Environment it maps a curated
  bank of 24; on Synth it maps the sound engine. Switch generator and the knobs re-point.
- **Performer** — hand-assigned named pages of 24 bindings you build yourself, Ableton-macro
  style.

**Pickup (soft takeover)** is on by default: a knob does nothing until it reaches the
parameter's current value, so a preset recall never causes a jump the next time you touch
it. Engagement resets when you change tab, generator or page.

Learn the layout by twisting all 24 in row-major order; it adopts the device's channel and
persists to disk.

## Who wins

Several of these can want the same control at once. The order is fixed and it is worth
memorising:

```
  sliders / automation  <  MIDI clip CC  <  held-key preset
        (lowest)                                (highest)
```

The `organon` CLI's override lane sits alongside the CC layer and follows the same
last-touched-wins courtesy: **a human moving a physical slider always takes the control
back.** See [the CLI guide](cli.md#letting-go).
