# Getting started

There is no installer yet — you build Organon from source, then install the bundle where
your DAW will find it. It is a normal Rust build and it wants no npm, no Python and no
build step outside cargo.

## 1. Build it

Install Rust via [rustup](https://rustup.rs). On Linux, install the audio and X11 dev
headers **first** — without them the build dies inside a *build script*, which reads like
a code error but is not:

```bash
sudo apt-get update && sudo apt-get install -y \
  libasound2-dev libjack-jackd2-dev \
  libx11-dev libx11-xcb-dev libxcb1-dev libxcursor-dev libxrandr-dev libxi-dev \
  libxext-dev libgl1-mesa-dev libxkbcommon-dev libwayland-dev
```

Nothing above applies on macOS or Windows.

```bash
cd native
./bundle.sh          # macOS / Linux → target/bundled/Organon.{vst3,clap}
.\bundle.ps1         # Windows
```

The fullscreen visual is a **second binary**, and `bundle.sh` embeds it inside the bundle,
so the `.vst3` you install is self-contained.

## 2. Install it where your DAW looks

**macOS.** The short way puts it in the standard user VST3 folder:

```bash
./bundle.sh --install        # → ~/Library/Audio/Plug-Ins/VST3
```

Or use `deploy.sh`, which builds, bundles, installs, **and** installs the `organon`
command-line tool with its shell completions and the network gallery:

```bash
./deploy.sh --dest ~/Library/Audio/Plug-Ins/VST3
```

> Pass `--dest`. Its default is `~/Documents/vst3`, which is the author's Ableton custom
> folder, not a macOS convention — leave it off and your DAW may never scan the result.
> `deploy.sh` is **macOS only**; it refuses to run elsewhere, because it ad-hoc signs the
> bundle and nothing outside macOS has an equivalent.

**Windows.**

```powershell
.\bundle.ps1
.\deploy.ps1 -Dest 'C:\Program Files\Common Files\VST3'
```

**Linux.** There is no deploy script; copy the bundle yourself:

```bash
./bundle.sh && mkdir -p ~/.vst3 && cp -R target/bundled/Organon.vst3 ~/.vst3/
```

Whichever route, it is idempotent — re-run it after every rebuild.

**macOS Gatekeeper blocks self-built plugins**, and the "Allow Anyway" button in System
Settings does **not** work for plugins — it only covers apps. You need:

```bash
sudo spctl --global-disable          # re-enable later with --global-enable
```

Then **rescan** in your DAW. If it cached a failed scan, quit it, clear its plugin-scan
cache, and reopen — a DAW will happily remember a plugin as broken forever.

## 3. Put it on a track

Organon is an **audio effect with MIDI input**, not an instrument. That has one practical
consequence, and it is the most common thing to get wrong:

> **It must sit on a track that receives MIDI.** Audio passes through untouched. If you
> put it on a plain audio track, the sliders will still work but no clip will ever reach
> it, and the Key Map and pad controller will look broken.

In Ableton, that means an audio-effect slot on a MIDI track (after an instrument, or on a
MIDI track with no instrument at all if you only want the visuals).

## 4. Open the visual

The plugin window is the **editor** — tabs, cards, sliders, the preset rail. It is not the
picture. Click **Open Visual Window** and a second, fullscreen-capable window appears:
that is the render.

Two windows, two processes, on purpose — see [the three choices](concepts.md#two-windows-two-processes)
for why, and what it buys you. For now the thing to know is that the visual is where you
point a projector, and closing it does not disturb your session.

## Trying it without a DAW

You do not need a host to see anything:

```bash
cd native
cargo run --release --bin organon-standalone
```

Same editor, no host. Click **Open Visual Window** and you are in the same place. The
transport-driven features (host tempo, beat lock, clip CCs) obviously need a DAW, but
everything else — generators, materials, the camera, presets, the manual tempo clock —
works standalone.

## When something looks wrong

- **Editor is up, nothing on screen.** The visual is a separate window. Did you click
  Open Visual Window? Is it behind your DAW?
- **The visual opened but is black.** Give it a few seconds. The render path early-returns
  while the window is occluded or unfocused, so a window buried behind your DAW may
  genuinely not be drawing.
- **"Open Visual Window" does nothing under a CLAP host on Windows.** This is a known,
  documented gap: nih-plug emits the CLAP as a bare DLL, so there is no bundle directory to
  embed the visual into. Point `ORGANIC_MATH_VISUAL` at the full path of
  `organic-math-visual.exe`, or use the VST3, which is unaffected. macOS CLAP is fine.
- **You upgraded and the visual now shows nonsense.** The plugin and the visual share a
  memory-mapped snapshot whose layout is versioned. After an upgrade that bumps it, close
  and reopen the visual window so both sides are the same build. Rescan in your DAW too.
- **A control does nothing.** Many parameters belong to one generator only. Check you are
  on the generator whose card you are adjusting — the editor hides cards that do not
  apply, so a visible card is usually a good sign, but the CLI's
  `organon describe <id>` will tell you plainly.
