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

## Factory presets

Two families ship inside the binary and are seeded into your store once, the first time
Organon loads it. Rename or delete them freely — the seed never re-adds a name you have
touched.

- **Rails — …** — five finished rides for the scenery corridor.
- **The PBR-text ladder** — six presets, `faceplate` · `nixie` · `foundry` · `anodized` ·
  `bottled` · `cathode`, each a terminal text effect rendered as a lit, physically shaded
  object (`doc/pbr_text_engine.md` §10). All six need a text producer publishing the
  glyph ring to draw anything; with one live, each holds the camera on the grid, dims
  the room, and lets the path tracer sharpen the held text through each dwell. They
  share that room and differ in the material the text is made of:
  - **`faceplate`** — the classic: phosphor tiles behind a thin clearcoat, slightly
    bevelled and crowned, every cell a tile (a dark cell is a low glass tile that shows
    the room; a lit cell's glow falls off toward its edges), the lit glyphs pooling
    their colour onto the backplane.
  - **`nixie`** — each cell a glass envelope: a deeper, domed tile whose glow is
    gathered to a filament-thin core, with a faint colour split in the glass and a warm
    halation. The glow sits at the envelope's surface — a filament *inside* it is not
    something the tile can do yet.
  - **`foundry`** — hot metal type: dark, rough, with a dull-cherry blackbody ember
    under everything and the effect's own colour held low at the slug's centre. The
    ember is one temperature across the whole plate; the effect's value cannot drive it.
  - **`anodized`** — an iridescent film over a dark metal, the colour rolling across
    each tile with the viewing angle; the phosphor turned down so the film is what you
    see.
  - **`bottled`** and **`cathode`** — the two that leave tiles behind: the lit cells
    become a **Plexus** web (the Surface controls, Generator bucket), wired to their four
    neighbours and drawn as impostors — glass beads on glass rods with a glowing core
    seen through the shell, looked at steeply along the rods (`bottled`), or emissive
    nodes on thin wires (`cathode`). ⚠️ Both are **monochrome** today — the web keeps the
    tiles' faceplate grey and drops their colour — and a one-column gap is wired like a
    vertical neighbour; they are the closest honest reading of the spec's rungs, not the
    finished picture.

  The look the tile presets dress — cell width, extrusion, gap, emission gain, bevel,
  face crown, emission profile, dark tiles, faceplate, backplane — is the **PBR Text**
  card on the Look tab, and the held camera (hold / tilt / zoom) is the **Text Camera**
  card on the Motion tab. Both are inert until a producer is running, so they cost
  nothing in any other preset. The factory copies are seeded once and then amended in
  place when a release changes them — unless you have saved over one, in which case
  yours is kept; and a preset of yours that happens to share one of the five new names
  is never touched, whenever you saved it. Every control on both cards, plus the
  capsule core, is also on the CLI vocabulary (`organon set glyph_bevel 0.12
  glyph_cam_hold 1`) — the ids and ranges are in
  [the parameter reference](../reference/parameters.md), so a script can dress the look
  without opening the editor.

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
