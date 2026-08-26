### Organon on the desktop — the Omarchy plan

`doc/organon_desktop_plan.md` asks whether the console's move works one layer out: Organon
took charge of the console by running the child, decoding the structured stream it emits
and rendering the conversation natively; can it take charge of the *desktop* the same way,
against Omarchy 4 "Quattro"?

The answer is yes, and most of it is cheaper than it looks — but not where the analogy puts
it. **Omarchy's desktop has no text stream to intercept.** `omarchy-shell` is one
long-running Quickshell process (95 QML files, 33 506 lines) rendering Wayland surfaces
directly. One layer up there is a better pipe than the console ever had:
`bin/omarchy-shell` drives the entire desktop by running
`qs ipc -n -p "$OMARCHY_PATH/shell" call -- "$@"` and parsing the text that comes back — 38
of the 439 commands in `bin/` funnel through that one file, speaking 18 documented methods.
📌 Answer those 18 and every Omarchy command keeps working while Organon owns every pixel.
**It is also the last thing to build rather than the first.**

🚨 **Quickshell is not a UI toolkit, and that measurement rules out the obvious plan.**
58 747 lines of C++ across 425 files; the drawing is **1 720 of them, under 3 %** — and
those are wrappers over Qt Quick (`WrapperItem`, `ClippingRectangle`, `IconImage`), not a
renderer. The mass is `src/wayland` (12 200 — layer-shell, session lock, screencopy,
foreign-toplevel), `src/services` (12 129 — pipewire, mpris, notifications, pam, polkit,
greetd) and `src/core` (13 853). So "map every Quickshell UI feature into Organon" has
almost nothing to map: the UI features belong to Qt Quick, and mapping them means writing a
QML engine with Omarchy's 33 506 lines of shell QML as the input. What is valuable is the
other 97 %, and in particular the two protocols Organon lacks.

⚠️ **Organon cannot draw desktop chrome today, and the tier order is built around it.**
The window stack is `winit` + `wgpu` + `egui`, and winit makes ordinary `xdg-shell`
toplevels; a bar, an OSD and an overlay are `zwlr_layer_shell_v1`, a lock screen is
`ext-session-lock-v1`, and winit implements neither. The way across is a road already
walked — `editor_probe.rs` puts a wgpu surface on a host's parent view and
`baseview_input.rs` feeds `egui::RawInput` with no winit at all — so it is a spike rather
than a rewrite. **That spike is T5, last, because nothing in T1–T4 depends on it.**

📌 **What moved to the front instead.** Organon Console is already a winit/wgpu app CI
builds on Linux, and Hyprland tiles `xdg-shell` toplevels — so several consoles, each a
different app-by-layout, is **T1 and nearly free**, gated on three small gaps:
`invocation()` accepts only `--help`/`--version` so a tile cannot be started in a saved
layout; no Wayland `app_id` is set, so `windowrule` cannot target one tile; and 🚨 every
tile needs its own `$ORGANON_IPC_NS`, because two consoles in one namespace are two seqlock
writers on one mmap and the failure is silent corruption. **T2 is the radial menu**, which
needs no shell at all: `default/omarchy/omarchy-menu.jsonc` is 368 lines of documented
schema whose leaves carry a shell command, and `bin/omarchy-menu` only toggles a plugin —
so a replacement reads that file, renders it, and runs the chosen action. ⚠️ It has to be
**resident**: Omarchy's menu is instant because Quickshell is already running, and a
process that launches a wgpu device on Super will be visibly worse than the list it
replaces.

⚠️ **The fork stays near-zero-diff, and the licence agrees with the mechanism.** Quickshell
is LGPL-3.0 and Qt6 is LGPLv3, while `organon-module` carries the tree's hardest bar — one
dependency, `cargo tree` as the acceptance test in *both* repositories, because
`organonart/ascent` links it. Linking is permitted with relinking rights, but a mechanism
that puts a process boundary where the licence boundary already is costs nothing to decide;
so Organon's desktop code lives in Omarchy's own extension points, and the two `bin/`
commands the later tiers need are shadowed through `PATH` on the machine rather than edited
in the fork.
