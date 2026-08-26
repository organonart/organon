# Organon on the desktop — taking charge of Omarchy

**The question.** Organon Console took charge of the *console*: it runs the child, decodes
the structured stream the child emits, and renders the conversation natively rather than
letting a terminal flatten it (`CONSOLE_ARCHITECTURE.md` §1.1). The ask is whether the same
move works one layer out — against [Omarchy](https://github.com/basecamp/omarchy) 4
"Quattro", DHH's Arch/Hyprland desktop, which arrives at a keyboard-driven tiling layout
close enough to Organon's own that the two ought to be one thing.

**The short answer: yes, and most of it is cheaper than it looks — but not where the
analogy puts it.** The single most convincing thing is also nearly free, the most
*striking* thing needs no new plumbing at all, and the part that first looked like the
foundation turns out to be the last tier rather than the first.

The machine is
[`zenbook-01`](https://github.com/james-andrew-walsh/workshop-machines/tree/main/hosts/zenbook-01);
the fork is `organonart/omarchy`, whose `FORK.md` owns the near-zero-diff policy;
`organonart/quickshell` is a fork of the shell toolkit Omarchy is built on.

> ⚠️ **Provenance.** §1–§5 were read out of the two trees — Omarchy at `0ae1694`
> (2026-08-25), Quickshell at `9f59d0a` — and are **measured** in this document's sense: a
> file, a line, a count taken from the tree. §6 onward is **reasoned**. **Nothing here has
> run on hardware.** The tiers are ordered by that distinction and §8 lists what is not
> known.

---

## 1. The correction: a desktop is not a pipe — except at exactly one place

The console interception worked because Claude Code *already* emits its structure: NDJSON
on stdout, decoded by `agent_event.rs` and folded into a transcript by `conversation.rs`.
The grid was a lossy encoding of something structured, and the win was refusing the
encoding.

**Omarchy's desktop has no such stream.** `omarchy-shell` is a single long-running
[Quickshell](https://quickshell.org/) process — 95 QML files, 33 506 lines — rendering
Wayland surfaces directly. There is no text between it and the screen, and a plan that
assumes otherwise fails on contact.

One layer up there *is* a pipe, and a better one than the console had:

🚨 **Omarchy drives its own desktop by shelling out.** `bin/omarchy-shell` is the single
file every desktop-driving command funnels through, and its whole mechanism is:

```bash
qs ipc -n -p "$OMARCHY_PATH/shell" call -- "$@"
```

It writes a command into a pipe and reads a text answer back — it even parses that answer
as strings (`"Target not found."`, `"Function not found."`,
`"Not ready to accept queries yet"`). **38 of the 439 commands in `bin/` reach the desktop
through it**, speaking **18 methods** documented in `docs/omarchy-shell.md`: `summon`,
`hide`, `toggle`, `call`, `applyTheme`, `listPlugins`, `setPluginEnabled`, `putBarWidget`
and the rest.

📌 That is the interception point, and it is one file. Answer those 18 and every Omarchy
command keeps working while Organon owns every pixel — unchanged producer, replaced
renderer. **It is also the last thing to build, not the first**, for reasons §6 makes
concrete.

The launch seam is one line: `default/hypr/autostart.lua:6` is
`hl.exec_cmd("omarchy-launch-shell")`, and that script is a supervisor — relaunch backoff,
compositor liveness checks, journal capture — wrapped around exactly one command,
`quickshell -n -p "$OMARCHY_PATH/shell"`.

---

## 2. Quickshell is not a UI toolkit

The obvious next thought is to map Quickshell's UI surface onto Organon's renderer. The
tree refuses that reading.

**58 747 lines of C++ across 425 files. The drawing is 1 720 of them — under 3 %.** And
those are not a renderer: `src/widgets/` is `WrapperItem`, `ClippingRectangle`,
`IconImage`, `MarginWrapper` — convenience wrappers around Qt Quick. `src/ui/` is 97 lines,
a reload popup and a tooltip.

| Directory | Lines | What it is |
|---|---|---|
| `src/core` | 13 853 | QML engine bindings, reload, config |
| `src/wayland` | 12 200 | **layer-shell, session lock**, screencopy, foreign-toplevel, Hyprland IPC, idle, focus grab |
| `src/services` | 12 129 | pipewire, mpris, notifications, pam, polkit, greetd, upower, status-notifier |
| `src/io` · `network` · `window` · `x11` · `dbus` · `launch` · `bluetooth` | ~18 200 | processes, sockets, NetworkManager, tray |
| `src/ui` + `src/widgets` | **1 720** | wrappers over Qt Quick |

🚨 **So "map every Quickshell UI feature into Organon" has almost nothing to map** — the UI
features belong to Qt Quick, not to Quickshell. Mapping them means writing a QML engine,
with Omarchy's 33 506 lines of shell QML as the input. That is building a Qt clone rather
than building Organon, and it is the one direction this document rules out.

**What is valuable is the other 97 %.** In particular `src/wayland/wlr_layershell/` (with
`wlr-layer-shell-unstable-v1.xml` vendored) and `src/wayland/session_lock/` — which are
exactly the two protocols §5 names as Organon's blocker.

⚠️ **And the licence decides how that value can be reached.** Quickshell is **LGPL-3.0**;
Qt6 is LGPLv3. Organon's split is deliberate — engine crates `MIT OR Apache-2.0`, the root
crate GPL *only* because `nih_export_vst3!` sits on GPLv3 `vst3-sys` — and `organon-module`
carries the hardest bar in the tree: one dependency, `cargo tree` as the acceptance test in
**both** repositories, because `organonart/ascent` links it and one transitive arrow breaks
a second repository's posture. LGPL permits linking with relinking rights, so this is not
fatal; it is exactly what `LICENSING.md` says to read before touching. **A mechanism that
puts a process boundary where the licence boundary already is costs nothing to decide.**

---

## 3. Three surfaces, three mechanisms

The mechanisms sort by *surface*, not by preference. Choosing one per surface is most of
the design.

| Surface | Mechanism | Why |
|---|---|---|
| A whole tile — the console, an editor, an agent | **Own toplevel window** | Hyprland tiles `xdg-shell` toplevels. Works today. |
| A summoned modal — the menu | **Own toplevel + Hyprland window rules** | Transient and fullscreen; needs no shell at all |
| A texture inside Quickshell's chrome — bar widget, panel background | **Frame ring → `QQuickItem`** | Quickshell owns surface and input; Organon supplies pixels |
| Chrome Organon draws itself — bar, OSD, lock | **layer-shell** | A surface type winit cannot make |

**The plugin seam is generous and worth knowing even where it is not used.** Plugin kinds
are `bar-widget`, `panel`, `overlay`, `menu`, `service` and — the one that matters — **`bar`**,
a full bar that *replaces* the built-in one. Only one is active at a time and
"missing or invalid selections fall back to the built-in `omarchy.bar`, so users always
have a safe path home" (`shell/README.md`). A plugin that fails degrades to Omarchy rather
than to a black screen, which is what makes this safe to run on a machine you also use.

⚠️ **A plugin is QML inside Quickshell. It is not Organon rendering**, and saying otherwise
later is the kind of claim this project keeps having to correct.

---

## 4. The menu is already data

The primary UI — Super, then `Trigger…` with Emoji, Reminder, Capture, Transcode, Share,
Toggle, Hardware, Speed Test — is defined in `default/omarchy/omarchy-menu.jsonc`: **368
lines**, documented schema. Dotted IDs make the tree; each leaf carries `icon`, `label`, and
an `action` that is a shell command, plus `when` / `checked` / `disabled` guards and
`provider` for dynamic submenus. `bin/omarchy-menu` is a thin wrapper that does nothing but
`omarchy-shell shell toggle omarchy.menu '{"menu":"root"}'`.

📌 **So a replacement menu is: read that file → render it → run the selected `action`.**
Nothing to reimplement, and a radial pie serves that data as well as a list does.

**Which means the menu never touches Quickshell.** It is summoned by a *command*; repoint
the keybind and it is Organon's, with no Qt, no layer-shell and no LGPL contact. Hyprland
window rules can make an ordinary `xdg-shell` toplevel float, pin, drop its border and go
transparent — which winit makes today.

Two things decide whether it survives daily use, and both are design constraints rather
than unknowns:

🚨 **Latency, and this is the one most likely to kill it.** Omarchy's menu appears the
instant Super is pressed because Quickshell is already running; their README says so
outright — *"summoning a panel is an IPC call into a process that is already running, not a
fresh `quickshell -p ...` cold start."* A binary that launches, creates a wgpu device and
loads an environment map is not instant. It has to be **resident** — a daemon that shows
and hides — or it is visibly worse than the list it replaces, however beautiful.

⚠️ **Text.** Shaded 3D UI usually dies on label legibility. The answer is a compositing
pass — geometry shaded below, text drawn sharp above — not text as geometry.

### 4.1 Why the hosted-module input contract does not fit here

`organon-module/src/input.rs` carries exactly four events — `Down`, `Up`,
`Pointer { dx, dy }`, `ReleaseAll` — and **refuses absolute pointer position by design**:
*"The console owns the pointer. A producer that could place the cursor could place it over
a confirmation button in some other window. Motion is a delta and nothing else."* Text and
IME are refused too, explicitly, as keylogger-shaped.

A radial menu driven by *motion from the centre* fits that perfectly — it is how Blender's
and Maya's pie menus work. **A menu with a search field does not**, and Omarchy's has one.
Text would need a new named grant, argued on its own, which that file's own rule demands.

📌 Those refusals exist because a hosted module is *somebody else's code*. Here the trust
runs the other way. Reusing that contract for a first-party surface would mean inheriting a
permission set designed for an adversary — a third reason the menu is its own window rather
than a hosted module.

---

## 5. What Organon can and cannot draw today

Organon's window stack is `winit 0.30` + `wgpu 30` + `egui`. winit makes ordinary
`xdg-shell` toplevel windows. That covers §3's first two rows completely and the last row
not at all.

🚨 **A bar, an OSD, a menu-as-chrome and an overlay are `zwlr_layer_shell_v1` surfaces** —
anchored to a screen edge, given an exclusive zone the compositor tiles around, outside the
window stack. A lock screen is `ext-session-lock-v1`, stricter still. **winit implements
neither.**

⚠️ **Verify that sentence rather than trusting it here.** It is the one claim in §1–§5 that
comes from knowledge of upstream winit rather than from a file in these trees.

The way across is a road Organon has walked: `src/editor_probe.rs` puts a wgpu surface on a
*host's* parent view for the VST3 editor, and `baseview_input.rs` feeds `egui::RawInput`
with no winit at all. A layer-shell surface is the same shape with a different provider —
`smithay-client-toolkit` creates it, `raw-window-handle` hands it to wgpu, the existing
winit-free egui path takes the events. A spike, not a rewrite. **It has not been run.**

### 5.1 What already exists on the Quickshell side

If the spike goes the other way — Organon producing, Quickshell consuming — the receiving
half is already written. `src/wayland/buffer/{shm,dmabuf,qsg}` turns a foreign buffer into a
`QSGTexture`, and `screencopy/view.cpp` is a `QQuickItem` that paints one. An `OrganonView`
is that class of thing with a different producer.

And the producing half exists too: **`organon-module` *is* this contract** —
`CONSOLE_ARCHITECTURE.md` §1.20's "a producer yields a texture the console can sample, at a
size the console asks for", three-slot ring, per-slot seqlock, **0.44 ms round trip at
1280×720**, preallocation pinned at 1, wire format pinned byte-for-byte by tests. Today the
console is the consumer; a `QQuickItem` becomes a second one, needing a C++ reader for an
already-specified format.

---

## 6. The five tiers

Tier 1 is independently shippable and each later tier is inert by default — `CONTRIBUTING.md`'s
tier pattern, and invariant 4. **The ordering is by conviction-per-cost, not by
architectural depth**, which is why the layer-shell work that first looked foundational is
last.

**T0 — the machine (a precondition, not a tier).** Stock Omarchy 4, installed by hand. Fill
`hosts/zenbook-01/README.md`'s facts table *from the box*. Confirm which NVIDIA branch fired
(`pacman -Q | grep '^nvidia'`) and whether the Zenbook is Optimus (`lspci -k`). Nothing
Organon. ⚠️ Omarchy installs `ufw` default-deny and ships sshd **off**, so a fresh box is
unreachable by design: `omarchy-setup-security-sshd --gh-keys <user>` and
`omarchy-install-service-tailscale` are what make it drivable, and both want doing while
someone is sitting in front of it.

**T1 — Organon Console, tiled.** The console is already a winit/wgpu app that CI builds on
Linux, and Hyprland tiles `xdg-shell` toplevels, so `exec organon-console` *is* a tile
today. Several consoles, each a different app-by-layout, is already how Organon runs on
organon-one — `organon-console.cmd` opens `TABS=pi-wsl,shell-wsl`, `oc.cmd` opens
`TABS=claude-chat,shell-wsl`. Hyprland turns that into a grid. **Three small gaps:**

- **No launch-time layout.** `/layout save` and `/layout load` exist and persist, but
  `invocation()` in `console_main.rs` accepts only `-h/--help` and `-V/--version`; a tile
  cannot be *started* in a layout. Every other console knob is an env var and Hyprland
  `exec` lines set env trivially, so `ORGANON_SHELL_LAYOUT=<name>` is the house-consistent
  fix, reusing the loader that already refuses rather than half-applies (§1.15).
- **No Wayland `app_id`.** The window is created with `.with_title(PRODUCT_NAME)` and
  nothing else, so every tile presents identically and `windowrule` cannot target one.
- 🚨 **Every tile needs its own IPC namespace.** `ipc.rs`'s `ns_file` resolves
  `$ORGANON_IPC_NS` once per process; `oc.cmd` forks it on purpose because two consoles in
  one namespace are two seqlock writers on one mmap. N tiles, N namespaces — and the
  failure is silent corruption rather than an error.

⚠️ **One reading of "Organon in the terminal windows" is a closed door.** Organon *as* a
tile, replacing a terminal — that is all of the above. Organon rendering *inside* someone
else's running terminal is already measured and refused: a harness owns the whole grid and
repaints by absolute positioning, so anything injected is both displaced and overwritten.

**T2 — the radial menu.** §4's mechanism: a resident Organon process, the Super keybind
repointed, `omarchy-menu.jsonc` read at summon, a PBR pie rendered, the chosen `action`
executed. Inert by construction — the stock menu is one keybind away for as long as the
binding is not repointed, and reverting is repointing it back. **This is the first thing on
that desktop that could not have been built any other way**, and it is the argument for
doing any of this: Omarchy's menu is a styled list; Organon has IBL, HDR, ray-traced
surfaces and materials. Making the menu an *object* has no equivalent in the Quickshell
world.

**T3 — the Organon theme.** `themes/<organon>/colors.toml` plus the shell theme tokens
(spacing, typography, bar size), in a repo of its own. Data only, zero code, reversible with
`omarchy theme set`. It reaches the bar, panels, menu, lock screen, terminal and editor at
once because Omarchy templates them all from the same values. 📌 **Order-independent** —
it has no dependency on T1 or T2 and can be pulled forward the moment anyone wants the
desktop to *look* right; it sits at T3 because it makes the chrome Organon has not taken
agree with the parts it has.

**T4 — Organon inside Quickshell.** §5.1's `OrganonView`: a `QQuickItem` consuming the
`organon-module` frame ring, so Organon renders *inside* Quickshell's chrome — a bar widget,
a panel background, a live texture behind glyphs. Needs a C++ reader for the ring's wire
format and, for anything interactive, a decision about the input channel (§4.1). ✏️ The
payoff worth naming: this makes Organon a citizen of **stock Omarchy**, as an ordinary
plugin, **with zero fork diff** — Organon on DHH's desktop without owning any of it.

**T5 — Organon draws its own surfaces.** The layer-shell spike of §5, measured the way the
module frame boundary was (`doc/measurements/module-frame-boundary-2026-08-21.md`) — a
number per frame, and the Optimus adapter question settled rather than assumed. 🚨 **This
tier is a measurement, not a product, and it is the one that can come back "no."** Nothing
in T1–T4 depends on it, which is exactly why it is last: it was the first thing that looked
like a foundation and it is the only thing here whose answer is genuinely unknown.

### Beyond T5, and deliberately not scheduled

**The IPC proxy.** Implement §1's 18 methods behind a socket and shadow `bin/omarchy-shell`
to route to it — as a **proxy, not a replacement**: Organon answers for the surfaces it has
taken and forwards the rest to `qs ipc` unchanged, which is what lets it land one surface at
a time. Decode what you understand, pass through what you do not, count what you dropped —
the console's own shape.

**The shell swap.** Shadow `bin/omarchy-launch-shell`. ⚠️ Price it honestly: it means having
replaced enough of 33 506 lines of QML that the machine is still usable without them — bar,
notifications, OSD, menu, lock, polkit agent, idle. Reachable only because the proxy lets
the count fall one surface at a time.

**The compositor.** Hyprland stays. Replacing it is a Smithay compositor or a Hyprland fork
— a lifetime commitment of the same class as a second VST3 class ID. The shell layer is the
right boundary until something concrete proves impossible above it.

---

## 7. The fork policy

**Near-zero diff.** `organonart/omarchy` carries `FORK.md` and nothing else. Themes and
plugins are their own repositories. The two `bin/` commands the later work needs are
**shadowed through `PATH` on the machine** — a change reversible with `rm`, tracked in
`workshop-machines` — rather than edited in the fork.

📌 The reason is upstream's cadence: 4.0.0 shipped 2026-08-14 and 4.0.1 eleven days later,
dominated by security fixes to plugins, themes, notifications, Docker privileges and
privileged DNS. A diff here is a merge conflict inside somebody else's security patch,
resolved under time pressure. `FORK.md` carries the `--ff-only` check that detects drift.

---

## 8. What is not known

1. **Layer-shell in Organon is unproven** (§5). T5 exists to settle it; nothing downstream
   should be scheduled as though the answer is yes.
2. **`PATH` shadowing order** in the session Hyprland's autostart inherits is asserted, not
   measured. If it is wrong the answer is a different shadow directory, never a fork edit.
3. **Which GPU wgpu should pick on an Optimus laptop** — `PowerPreference::HighPerformance`
   reaches for the dGPU, possibly the wrong answer for chrome that must never stutter and
   should never wake the NVIDIA card to draw a clock.
4. **N processes versus one process, N windows** (T1). N processes exist today and isolate
   failures; one process shares a device and a `World`, and would allow the backdrop to be
   *continuous across tiles* — a single world through N frusta, which
   `workshop-machines/docs/organon-os-roles.md` already carries as the `surfaces` axis.
   Start with N processes; the frusta version is what to build toward.
5. **The 18-method vocabulary is documented, not exercised.** Whether the 38 callers use
   only what `docs/omarchy-shell.md` documents is a `grep` nobody has run.
6. **Nothing in §6 has run on hardware.** The first thing that will be wrong in this
   document is something T0 finds.
