---
name: organon-cli
description: Operate Organon — the parametric hyperscope — in real time through its local `organon` command-line tool. Use whenever you are asked to drive, tune, demonstrate, or capture a look in Organon: read the live state, choose a generator / surface / material, set parameters, apply a recipe, and take a snapshot or record a clip to see the result. Assumes the `organon` CLI is installed and Organon's visual window is running on the same machine. Also use when asked to change Organon Console itself from inside a console tab — where its code lives, what verifies it, and the traps.
---

# Operating Organon through the CLI

You drive Organon **from the outside**. Organon runs as its own window (a
separate process); you speak to it through the `organon` command, read its live
state, change its controls, and **look at what you made** with a snapshot. You do
not live inside it — you operate it and watch it respond.

One exception: if you are running in a tab of an **Organon Console**, you *do* live inside
it, and you can change it — see **Changing the console from inside it**, below.

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
organon console patch --up N --rows M --kind <kind>   # claim a gap you already printed
organon console portal <state>       # float a live window onto the world over the transcript
organon console camera [--reset] [--yaw R] [--pitch R] [--distance D]   # where you STAND
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

`patch` is the one to reach for, and it inverts who makes the hole. **You** print the
gap — ordinary blank lines on ordinary stdout, as part of your own output — and then
say where it is: `--up` counts back from the line you are on now, `--rows` is how tall.
The console writes nothing into the terminal; it records the rectangle and paints it.
Print the gap and claim it in one breath, because "the line you are on" is resolved when
the console next drains, and anything you print in between moves it.

`--kind` says *what* the console should draw there — a name it resolves, never a command
and never a path. Ask `--help` for the kinds this build knows; it defaults to the one the
verb shipped with, so a claim without it is unchanged. A rectangle scrolls, ages and
evicts with the rows it is pinned to, and a terminal **width** change invalidates it.

`portal` is the one that is **not** anchored to your output, and that is the whole
difference: it holds a place on the *screen*, so the transcript scrolls past underneath
it and nothing you print moves it. Nothing to print first, no counting back from the line
you are on, and none of `patch`'s timing caveat — there is no line to resolve. Ask
`--help` for the states it takes. Two consequences worth knowing before you open one:
it **occludes** the rows it floats over until it is closed, and it shows the **world**,
so the `set` / `generator` / `recipe` verbs above drive what is inside it — from this same
console, with nothing else to wire up. While it is open the backdrop does not paint and a
scene patch has no picture; closing it gives both back.

`camera` is where **you stand**, and it is the one to reach for when the picture is right and
you cannot see it properly — too far away, or facing the wrong side. Do not confuse it with the
world's own camera: `set cam_path …` and its siblings above are the *auto-orbit*, part of the
composition; this walks around whatever that composition is doing. The two compose, so a shot
framed here still spins.

Three things about how to use it, all of which follow from the lane having **no return path**:

- **Every axis is absolute**, in the units the drag and the wheel already write — you cannot
  nudge, because you cannot read where the camera is to nudge from. Ask `--help` for the units
  and the bands, and expect a value outside them to be **refused rather than clamped**.
- **`--reset` is how you establish a known state**, and it is applied before the axes in the
  same command — so "back to the default view, then pull in" is one line and needs no read-back
  anywhere. Reach for it first when you have lost the camera.
- 🚨 **The hand outranks you.** If James is dragging or wheeling the portal — or did so within
  the last couple of seconds — your command is **dropped**, not queued, and you will not be told:
  the console says so on its own stderr and nothing comes back to you. So do not conclude the
  camera is broken from one command that appeared to do nothing. Wait a moment and say it again.

And one thing that is not about the lane: a camera command with **no portal open and no
`background world`** moves the viewpoint somewhere nothing is drawing it. It succeeds and
changes no pixel. Open the portal first.

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

## Changing the console from inside it

Everything above is **operating** Organon. This is the other mode: you are running in a
tab of an Organon Console and the ask is to change *the console you are running in*. That
is not a new tool. It is this repository — `native/organon-console`, plus `console_main.rs`
and `cli.rs` in the root crate — and the ordinary workflow in `CONTRIBUTING.md` applies
unchanged: branch off `main`, PR it, close a review cycle. What follows is only the part
that is **not** discoverable by reading the code.

**Read the doc before you read the tree.** `CONSOLE_ARCHITECTURE.md` is the console's living
state, and it is hook-enforced rather than hopeful: `.claude/hooks/doc-rules.sh` makes it
accountable for `native/organon-console/src/*.rs`, so the code cannot move without the doc
being called for. §1 is the terminal host, §1.1 the conversation view, §2 the seams
already claimed by coming work, §3 the honesty ledger — what is known to be *unverified*,
which is the section that tells you whether a thing you are about to trust has ever been
seen on screen. It is the authority; if it and this file disagree, it wins.

These are the doors, not a map — the doc has the map:

| The ask is about | Start at |
|---|---|
| the grid, the PTY, colour, keys | `term.rs` / `term_view.rs` — §1 |
| tabs, the strip along the top, the **+** menu | `tabs.rs` — §1 |
| which agents a tab can run | `harness.rs` — and read **data or code** first |
| the agent's wire format | `agent_event.rs`, the decoder — §1.1 |
| what a transcript *is* | `conversation.rs` — §1.1 |
| what gets rendered from what arrived | `agent_map.rs`, the only file that knows both — §1.1 |
| the scrollback, the composer, the status strip | `conversation_view.rs` — §1.1, "The two bands under the scrollback" |
| the lit backdrop and the `organon console` verbs | `console_main.rs` + `cli.rs` — §1 |

**Data or code — ask that before you write anything.** The console has exactly one seam
that extends it with no rebuild: `harness::load` seeds from the built-in registry, reads
`harnesses.json` from the OrganonShell store root, and merges user entries **over the
built-ins by id** — a matching id replaces one wholesale, a new id appends. So a new tab
type is a data change. Two traps, both silent: ⚠️ an *unparseable* file is swallowed
exactly like an absent one, which is how a byte-order mark makes a valid-looking config do
nothing at all; and ⚠️ `cwd` is a path in the namespace the process actually **starts** in,
so a WSL harness takes a Linux path. Nothing else is read from disk today — command specs
and material names are compiled tables — so anything beyond a harness row is a code change.

🚨 **Do not build a hot-load path, a plugin system, or any new runtime machinery.** How a
change takes effect is an open question James has deliberately not answered
(`doc/console_spike_execution_plan.md` §5.9.26), along with whether self-extension reaches
code at all or stays at the data seams. Until he answers it, a data change lands at the
next launch and a code change is a rebuild — and reaching for hot-loading Rust is the
first mistake §5.9.26 exists to prevent.

**The verification bar, exactly:**

```bash
cd native
cargo test -p organon-console --lib                          # the compositor lib — the tight loop
cargo check --features console-edition --bin organon-console # the bin the lib is compiled into
```

Run both, in the foreground, and read them — a build you launched in the background and
never looked at is not verification. 🚨 **Never `cargo test --workspace` and never a bare
`cargo test` from `native/`**: it is ~45 minutes of codegen and it is the single most
expensive mistake available here. ⚠️ **Never `cargo fmt`** — a bare run reformats the whole
tree and buries the real diff; format your own edits by hand. A *release* build needs every
`organon-console` process closed first, your own tab included, or the link fails on the
exe lock.

**Four traps that have each cost real time, and none of which the code admits to:**

- egui's `Modifiers::matches_logically` is **shift-permissive**, so a pattern that does not
  ask for shift still matches a press *with* it. That is why the composer declares
  **Shift+Enter** as its `return_key` rather than the obvious arrangement, and reads bare
  Enter out of `ui.input` without consuming it.
- A vertical `ScrollArea` dropped into a `Layout::bottom_up` column **collapses the entire
  column** — measured at 684 pt of a 684 pt pane for one row of text. Both bottom bands
  reserve their height with `allocate_ui_with_layout` and lay out top-down inside it.
- A reserved height has to be computed from the text style actually **drawn** in it. Two
  styles in one reservation means taking the max, or the number is right only until someone
  changes a font call — and it goes wrong as a *second row*, in the one band that must stay
  one line.
- Geometric and box-drawing glyphs are **tofu in egui's proportional face**: a missing glyph
  draws as an empty box rather than failing. Either change the glyph or draw it
  `.monospace()`, depending on what it sits in.

**The honesty rule is the product, not a nicety.** Everything the console displays is either
*measured* — something the session actually reported — or *derived* from a measurement by a
stated rule, and **an omitted field is better than an invented one**. The worked example is
the declined-readouts list in `SessionFacts`'s doc comment (`agent_map.rs`): a context
percentage was refused outright while the only available numerator was a sum across a turn's
round trips, which read nearly 2× the true figure and would have looked exactly as confident,
and was built only once an honest per-request numerator was found on the wire. **Refusing a
readout is a normal outcome, and so is reversing a refusal on new measurement.** If you add
one, say what measured it; if you decline one, write down why, and put anything you have not
actually seen on screen in §3's ledger.

**And the doc moves in the same change as the code.** `CONSOLE_ARCHITECTURE.md` for anything
under `organon-console/`, `CHANGELOG.md` for anything meaningful, and this file if you add or
change a command. The Stop hook is the safety net, not the instruction — it fires after the
fact, and a sub-agent may never see it.

## When something is off

- `organon status` / `organon get` / `organon watch` returning an error → **nothing is
  writing the snapshot.** Those three READ `Shared`, and only an editor writes it — the
  standalone, the plugin in a host, or Organon Console. A visual on its own will never
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
  Organon Console each use their own, which is what lets them run beside a plain Organon.
  Export the same value the app was launched with, or you will read one product's state
  while trying to steer another's.
- A control you set does not seem to take → confirm it is the right generator's
  parameter (many are generator-specific), then `snap` to check; a value can be
  legal but invisible (e.g. iridescence on a matte material).
- Unsure a value is sane → `organon describe <id>` for its range and meaning
  before guessing.
