---
name: organon-cli
description: Operate Organon — the parametric hyperscope — in real time through its local `organon` command-line tool. Use whenever you are asked to drive, tune, demonstrate, or capture a look in Organon: read the live state, choose a generator / surface / material, set parameters, apply a recipe, and take a snapshot or record a clip to see the result. Assumes the `organon` CLI is installed and Organon's visual window is running on the same machine.
---

# Operating Organon through the CLI

You drive Organon **from the outside**. Organon runs as its own window (a
separate process); you speak to it through the `organon` command, read its live
state, change its controls, and **look at what you made** with a snapshot. You do
not live inside it — you operate it and watch it respond.

The one habit that matters: **see → act → see.** Look at the current state, make a
change, then look again at the result. Never assume a change did what you intended
— take a snapshot and check.

> Everything the CLI knows about is discoverable *from* the CLI. This file teaches
> the loop and the grammar; the **live catalog is the authority**. When you are
> unsure what a control does or what values are legal, ask the tool, not your
> memory: `organon describe <thing>` and `organon catalog --manual`.

## The loop

1. **Discover** — what exists and what it does:
   - `organon catalog --manual` — the whole vocabulary with a one-line description
     of every parameter, generator, surface, and material.
   - `organon describe <query>` — one thing in depth. A parameter gives its kind,
     range, current value, and what it does; a generator / surface / material /
     recipe gives its prose.
   - `organon recipes` — the built-in starting-points (see **Recipes** below).
2. **Look** — read the current state:
   - `organon status` — generator, surface, material, tempo, transport.
   - `organon get <id>` / `organon get --all` — current parameter values.
   - `organon snap` — render one frame to a PNG and print its path. **This is your
     eyes.** Open the image and judge it.
3. **Act** — change the look (see **Acting** below).
4. **Verify** — `organon snap` again. Compare. Adjust. Repeat.

## Building a look from nothing

You do **not** need any saved presets. A freshly-started Organon can be driven to
anything it can render, because the vocabulary and the recipes are built in. Two
ways in:

- **Start from a recipe**, then tweak. `organon recipe helix` selects a
  generator/surface/material and sets the key parameters in one command — a launch
  pad. Then `snap`, adjust individual params, `snap` again.
- **Compose from scratch.** Choose a generator for the *form* you want
  (`organon describe <name>` to check what each produces), a surface for how its
  nodes are drawn, a material for the shading, then set the handful of parameters
  that matter. Read the descriptions first; set deliberately; look.

## Acting

Selectors take a name, an unambiguous substring, or an ordinal:

```
organon generator "swept tubes"     # or an ordinal, or a substring like "dna"
organon surface swept
organon material glass
```

Parameters are set in raw units (the ranges are in `describe` / `catalog`):

```
organon set metallic 0.9 glow 0.3 exposure 0.0     # any number of id/value pairs
organon do '{"moves":[{"op":"set_param","id":"glow","value":1.0}]}'   # a batch plan
organon recipe nebula                                # a whole described look at once
```

`organon console …` is a **separate namespace, and it drives the console itself** —
the lit surface behind the terminal text and the shape of the transcript, not the
world in front of it:

```
organon console background <name>    # the surface behind the glyphs
organon console rig <name>           # how that surface is lit
organon console block <rows>         # reserve blank rows in the transcript
```

Reach for it when the ask is about *the workspace* ("make the background darker",
"warmer light", "leave me room for a panel"); reach for the world verbs above when the
ask is about *what Organon is rendering*. It only means anything while an Organon
Console is running, and it changes nothing a `snap` would show. `--help` lists the
accepted names and the row bound — ask it rather than guessing, exactly as with
parameters.

`block` opens its rows in the **active tab**, just below the cursor, and the next
prompt lands underneath them. They are ordinary scrollback rows, so they scroll away
with the rest of the transcript. Nothing is painted into them yet.

Your changes ride an **override lane**. Two rules follow from that:

- **The human always wins.** If a person moves a physical slider for a parameter
  you are holding, your hold on *that* parameter is released (last-touched-wins).
  That is intended — do not fight it.
- **Let go when done.** `organon release <id>` drops one hold; `organon release`
  (no id) drops all of them, handing control back to the sliders.

## The eyes — snapshots and clips

```
organon snap                     # → prints the PNG path; the default lands in the cwd
organon snap -o /tmp/look.png    # choose the path
organon record start --bars 8    # start a clip (auto-stops after 8 bars; 0 = manual)
organon record stop              # stop and finalize; prints the file path
```

`snap` requires the visual window to be open. Use it constantly — after every
meaningful change — so you are judging the *actual* render, not your intention.

## Recipes — described starting-points

`organon recipes` lists them; `organon describe <name>` shows exactly what a recipe
does before you apply it; `organon recipe <name>` applies it; add `--dry-run` to see
the commands without changing anything. Recipes are launch pads, not the ceiling —
apply one, then make it yours. When a look is worth keeping, a person saves it as a
preset in the editor; your job is the live composition, not the preset store.

## Values worth knowing (the rest live in `catalog --manual`)

- **`exposure`** is in EV **stops**: 0 is neutral, +3 is ~8× brighter and usually
  blows the scene to white. Move it in small steps.
- **`bloom_intensity`, `glow`, `ambient`** read best kept modest (~0.2–0.4) unless
  a deliberately hot, hazy glow is wanted.
- **`metallic` / `roughness`** are 0..1: low roughness = mirror, high = matte.
- **True translucency** needs a **Glass**, **Refractive**, or **Subsurface**
  material — lowering `opacity` alone just fades a surface, it does not refract.
- **To make any generator visibly turn or spin**, use the auto-orbit camera:
  `cam_path` (1 = horizontal circle, 2 = vertical, 4 = spiral; 0 = off) with a slow
  `cam_speed` (0.1–0.3). The geometry rotation params (`rot_amp_*`, `rot_mod_*`)
  only move the Original cube-field generator; other generators build their own
  motion.
- **`mat_hue`** (0..1 around the wheel: ~0 red, ~0.33 green, ~0.6 blue) is the
  quickest way to recolour the whole look.

## Showing, not telling

When you demonstrate, start from what is actually on screen and follow it. Set a
control, snapshot, and describe what changed — let the instrument speak. The
grammar was in the mathematics before you touched a slider; your part is to find
the settings that let it show, and to look honestly at what comes back.

## When something is off

- `organon status` / `organon get` / `organon watch` returning an error → **nothing is
  writing the snapshot.** Those three READ `Shared`, and only an editor writes it — the
  standalone, the plugin in a host, or Organon Shell. A visual on its own will never
  satisfy them, no matter how long you wait, so this failure is structural and permanent
  rather than a timing problem. Say so plainly.
- `snap` timing out → **different failure, do not confuse the two.** `snap` needs no
  writer; it asks the *visual* for a frame. A timeout usually means the visual is still
  coming up, or its window is covered or unfocused (the render path early-returns when
  occluded, and winit needs the window activated). **Retry it** — the repo's own frame
  harness budgets 160 attempts over 40 s before giving up, while the bare CLI gives you one
  10-second try. `set`, `generator`, `surface` and `release` also work against the visual
  alone.
- Setting a **negative** value → `organon set exposure -3.0` works. If you are on an older
  build that rejects it (`error: unexpected argument '-3' found`), `organon set -- exposure
  -3.0` is the escape hatch; the nine negatively-ranged params are `exposure`, `elevation`,
  `azimuth` and the six `rot_mod_*` / `trans_mod_*` axes.
- Talking to the **wrong** Organon → the CLI resolves `$ORGANON_IPC_NS` from its own
  environment and addresses whatever it names (default `organic-math`). Organon Mind and
  Organon Shell each use their own, which is what lets them run beside a plain Organon.
  Export the same value the app was launched with, or you will read one product's state
  while trying to steer another's.
- A control you set does not seem to take → confirm it is the right generator's
  parameter (many are generator-specific), then `snap` to check; a value can be
  legal but invisible (e.g. iridescence on a matte material).
- Unsure a value is sane → `organon describe <id>` for its range and meaning
  before guessing.
