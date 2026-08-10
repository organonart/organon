# Presets

A preset in Organon is not one thing. The parameter set is partitioned into buckets, and
you can save and recall them independently — which is what lets you keep a look and swap
the motion under it, or keep the motion and swap the whole world.

## Scenes and components

Seven buckets, matching the editor's tabs:

| Bucket | What it holds |
|---|---|
| **Generator** | the generator choice and all its parameters — plus the surface |
| **Motion** | animation, camera, pulse, speed pulse, breath |
| **Look** | materials, surface FX, lighting, reflections, GI, particles, bloom, post |
| **Environment** | terrain, sun and day cycle, sky, clouds, ocean, starfield, the loaded HDR |
| **Audio** | metering, the calibrated instrument, pulse routing |
| **Synth** | the built-in Duo-Field sound engine |
| **Settings** | HDR/MSAA/tone map, render scale, output framing, sync and tempo |

The **first four together make a Scene**. Recalling a Scene recalls exactly those four and
never touches Audio, Synth or Settings — so your metering setup and your output resolution
survive every look change you make during a set. That is the point of the split.

Each bucket also has its own list, so you can mix: recall a Scene, then drop a different
Look on top of it, then a different Environment. Each saved file holds only its own
bucket's fields.

A parameter lives in the bucket whose **tab its card is drawn under** — what you see is
what gets saved. There is one exception, and it is deliberate: the **Surface** controls sit
on the Look tab but belong to the Generator bucket, because a surface without its generator
is meaningless.

## Beat-quantized recall

Two dropdowns in the Sync/Tempo card set when a recall actually lands — one for Scenes, one
for components:

**Instant · 1/4 · 1/2 · Bar · 2 Bars · 4 Bars · 8 Bars**

With anything but Instant, a recall is *queued* and fires when the host's beat crosses the
next boundary. The editor shows a queued recall as pulsing, and the pad controller's
Stop/Solo/Mute cancels it.

Audio, Synth and Settings recalls always fire immediately — they are not musical. So does
any recall while the transport is stopped, since there is no boundary to wait for.

## Recall is atomic

Recall sets every captured parameter one at a time on the GUI thread, while the plugin
publishes a snapshot to the visual every audio block. Left alone, that would let the visual
render a half-applied state — new geometry, old colour, for a frame or two.

It does not, because the publish is held off across the whole apply. **You see the new look
in one step**, which matters most exactly when it is most visible: a hard cut on a
downbeat.

Recall goes through your host's parameter setter, so it is **automation-recordable and
undoable** like any other parameter change.

## Row actions

Each preset row carries **R / D / U** — Rename, Delete, and **Update** (overwrite the
saved preset with the current state). Update and Delete ask first, defaulted to Yes.

## Recorded defaults

Separate from named presets, and easy to miss: every numeric slider's reset button can
remember *your* default rather than the factory one.

- **⌘-click** (hold ⌘, the glyph flips to ●) records the current value as that control's
  default.
- **Click** resets to the recorded default if there is one, else to the factory value.
- **⟲ Reset All** is always a hard factory reset — it ignores your recordings.

Recorded defaults live in their own file and are a *reset target only*: a fresh plugin
instance still opens at the factory values. Enum dropdowns have no record option, only
reset.

## Where it all lives

One directory, per platform:

| | |
|---|---|
| macOS | `~/Library/Application Support/OrganicMath/` |
| Windows | `%APPDATA%\OrganicMath\` |
| Linux | `~/.local/share/OrganicMath/` |

Inside it: `presets.json` (Scenes) alongside one file per component bucket, your recorded
defaults, the Key Map, the pad and knob layouts, and the network gallery.

Five finished factory rides are seeded into the store once, on first load. Delete or rename
them and they stay gone — the seeding is guarded by a marker, so it will not resurrect them
behind your back.

**Old preset files keep loading.** Fields added since a preset was saved fall back to their
defaults rather than refusing the file.

## Presets vs. recipes

They are different tools and it is worth not confusing them:

- A **preset** is *your* saved state, in your user directory.
- A **[recipe](../reference/recipes.md)** is a named starting-point compiled into the
  binary. A fresh install with an empty preset store can still be driven to a finished look,
  because the recipes ship inside it. Apply one from the CLI, then make it yours, then save
  it as a preset.
