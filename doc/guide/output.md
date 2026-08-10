# Output and capture

The visual is a separate, fullscreen-capable window, so pointing it at a projector is a
window-management problem, not an Organon problem: drag it to the display and go
fullscreen. What Organon adds is control over **what resolution it renders at**, which is
what you need for a clean capture or a pixel-exact projector feed.

All of this lives on the **Settings** tab.

## Output resolution

By default the visual renders straight into its window at whatever size the window is.
That is fine for working and wrong for capturing, because the frame changes size whenever
the window does.

The **Output** card fixes the frame instead:

- **aspect** — a preset ratio, or Custom.
- **long edge** — the long side in pixels. **0 means "match the display"**: full native
  resolution with no downscale, so 16:9 on a 4K projector renders a true 4K frame.
- **width / height** — the exact pixel size, when aspect is Custom.

The card shows the resolution actually in use underneath. When a fixed output is set, the
frame is rendered at that size and letterboxed into the window, so an OBS capture is
pixel-exact regardless of how you have sized the window.

> For maximum sharpness, keep **Render Scale** at 1.0 on the Renderer card. Render scale
> multiplies the internal render resolution before it reaches the output frame — it is a
> performance dial, and anything below 1.0 costs you detail you cannot get back.

Output resolution is a **per-display setting and is not saved in Scene presets**, along
with HDR, MSAA and tone mapping. This is deliberate: your presets travel between machines
and projectors, and a look that dragged a resolution along with it would be a nuisance
every time.

## Letterbox and guides

The **Letterbox & Guides** card controls what surrounds the frame:

- **bar R / G / B** — the letterbox colour. Black is the default; anything else is for
  checking your framing, not for shipping.
- **frame guide** — an on-screen boundary marker, also togglable with **G** in the visual
  window.
- **lock window to output** — constrain the window to the output aspect so there is no
  letterbox at all.

## Overlay

The **Overlay** card draws text and pre-rendered formula plates over the render — handle,
title, opacity, scale. Toggle it with **T** in the visual window. Overlay decoration is not
captured by presets, on the same reasoning as resolution.

## Stills and video

From the terminal, against a running visual:

```bash
organon snap                      # one frame → PNG, prints the path
organon snap -o /tmp/look.png     # choose where it lands

organon record start --bars 8     # start; auto-stops after 8 bars
organon record start --bars 0     # start; runs until you stop it
organon record stop               # stop, finalize, print the path
```

`snap` reads back the **production frame** — the fixed-resolution output above, if you have
set one — so a snapshot is the same pixels a capture would get, not a screenshot of the
window.

Recording is beat-synced when you give it `--bars`, so an 8-bar clip is 8 bars of your
host's tempo rather than an approximate wall-clock stretch. If the plugin is running in a
host, the audio it processes is muxed into the recording.

**A first `snap` right after launch can time out.** The visual takes a few seconds to come
up, and the render path deliberately early-returns while the window is occluded or
unfocused — a covered window is not drawing, so there is no frame to read back. Retry it,
and make sure the visual is visible.

## Recording from your DAW instead

None of the above replaces screen capture, and for a finished piece you may not want it to.
The fixed output frame exists precisely so OBS, Syphon/NDI-style tools, or a hardware
capture card get a stable, known-size image. Set the output resolution to your capture
target and the letterbox tells you exactly where the frame edges are.
