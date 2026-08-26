# Organon on the desktop — taking charge of Omarchy

**The question.** Organon Console took charge of the *console*: it runs the child,
decodes the structured stream the child emits, and renders the conversation
natively rather than letting a terminal flatten it (`CONSOLE_ARCHITECTURE.md`
§1.1). The ask is whether the same move works one layer out — against
[Omarchy](https://github.com/basecamp/omarchy) 4 "Quattro", DHH's Arch/Hyprland
desktop, which arrives at a keyboard-driven tiling layout close enough to
Organon's own that the two ought to be one thing.

**The short answer: yes, and the seam is cleaner than the console's was — but not
where the analogy puts it, and one prerequisite is unproven.** This document is
the reasoning and the tier sequence. The machine is
[`zenbook-01`](https://github.com/james-andrew-walsh/workshop-machines/tree/main/hosts/zenbook-01)
in `workshop-machines`; the fork is `organonart/omarchy`, whose `FORK.md` owns
the near-zero-diff policy.

> ⚠️ **Provenance.** Everything in §1–§3 was read out of the Omarchy tree at
> `0ae1694` (2026-08-25) and is **measured** in this document's sense — a file
> and a line, not a recollection. Everything in §4 onward is **reasoned**: no
> part of it has run on hardware, because the machine did not exist when this was
> written. The distinction is load-bearing and the tiers are ordered by it.

---

## 1. The correction: a desktop is not a pipe — except at exactly one place

The console interception worked because Claude Code *already* emits its structure:
NDJSON on stdout, which `agent_event.rs` decodes into typed events and
`conversation.rs` folds into a transcript. The grid was a lossy encoding of
something structured, and the win was refusing the encoding.

**Omarchy's desktop has no such stream.** `omarchy-shell` is a single
long-running [Quickshell](https://quickshell.org/) process — 95 QML files, 33 506
lines — that renders Wayland surfaces directly. There is no text to intercept
between it and the screen, and any plan that assumes there is will fail on
contact.

But one layer up there *is* a pipe, and it is a better one than the console had:

🚨 **Omarchy drives its own desktop by shelling out.** `bin/omarchy-shell` is the
single file every desktop-driving command funnels through, and its whole
mechanism is:

```bash
qs ipc -n -p "$OMARCHY_PATH/shell" call -- "$@"
```

It writes a command into a pipe and reads a text answer back off stdout — it even
parses that answer as strings (`"Target not found."`, `"Function not found."`,
`"Not ready to accept queries yet"`). **38 of the 439 commands in `bin/` reach the
desktop through it**, and the vocabulary they speak is 18 methods, documented in
`docs/omarchy-shell.md`: `summon`, `hide`, `toggle`, `call`, `applyTheme`,
`listPlugins`, `listShellConfig`, `setPluginEnabled`, `putBarWidget`, and the rest.

📌 **That is the interception point, and it is one file.** Answer those 18 methods
and every Omarchy command keeps working while Organon owns every pixel — the
console's move exactly, with an unchanged producer and a replaced renderer. The
difference is only that the producer is a fleet of bash scripts rather than one
agent.

---

## 2. The three seams, in order of how much they cost

| Seam | Where | What it gives | Cost |
|---|---|---|---|
| **Theme** | `themes/<name>/colors.toml` — pure data | The whole desktop in Organon's palette | An evening. No code. |
| **Plugin** | a git repo with `manifest.json`, `omarchy plugin add` | A bar, panels, menus, overlays, services — ours, by contract | QML, not Organon rendering |
| **IPC + launch** | `bin/omarchy-shell`, `bin/omarchy-launch-shell` | The desktop itself | Everything below |

**The theme seam is real skinning and should not be dismissed as cosmetic.** A
theme is `colors.toml` (accent, background, foreground, the eight ANSI colours and
their bright variants) plus shell theme tokens for spacing, typography and bar
size. It reaches the bar, the panels, the menu, the lock screen, the terminal and
the editor at once, because Omarchy templates all of them from the same values.

**The plugin seam is the sanctioned path and is deliberately generous.** Plugin
kinds are `bar-widget`, `panel`, `overlay`, `menu`, `service` and — the one that
matters — **`bar`**, a full bar that *replaces* the built-in one. Only one `bar`
plugin is active at a time and "missing or invalid selections fall back to the
built-in `omarchy.bar`, so users always have a safe path home"
(`shell/README.md`). A plugin that fails degrades to Omarchy rather than to a
black screen, which is the property that makes this tier safe to run on a machine
you also use.

⚠️ **A plugin is QML running inside Quickshell. It is not Organon rendering, and
saying otherwise later would be the kind of claim this project's logs keep having
to correct.** What it buys is the *layout, vocabulary and behaviour* — the desktop
starts thinking like Organon before it starts looking like it.

**The launch seam is one line.** `default/hypr/autostart.lua:6` is
`hl.exec_cmd("omarchy-launch-shell")`, and `bin/omarchy-launch-shell` is a
supervisor — relaunch backoff, compositor liveness checks, journal capture —
wrapped around exactly one command: `quickshell -n -p "$OMARCHY_PATH/shell"`.
Change that command and Organon is the desktop shell. The supervisor is worth
keeping; it is better than anything we would write first.

---

## 3. The gap: Organon cannot put a surface on a Wayland compositor yet

Organon's window stack is `winit 0.30` + `wgpu 30` + `egui` (`native/Cargo.toml`).
winit makes ordinary `xdg-shell` toplevel windows.

🚨 **A bar, an OSD, a menu and a fullscreen overlay are not toplevel windows.**
They are `zwlr_layer_shell_v1` surfaces — anchored to a screen edge, given an
exclusive zone the compositor tiles around, outside the window stack. A lock
screen is `ext-session-lock-v1`, which is stricter still. **winit does not
implement layer-shell**, so today Organon cannot draw a single piece of desktop
chrome, however much of the rest of the plan is sound.

⚠️ **Verify that sentence at T3 rather than trusting it here.** It is the one
claim in §1–§3 that comes from knowledge of upstream winit rather than from a
file in this tree, and the whole tier order rests on it.

**The way across is a road Organon has already walked.** `src/editor_probe.rs`
puts a wgpu surface on a *host's* parent view for the VST3 editor, and
`baseview_input.rs` feeds `egui::RawInput` with no winit involved. A layer-shell
surface is the same shape with a different provider: `smithay-client-toolkit`
creates the surface, `raw-window-handle` hands it to wgpu, and the existing
winit-free egui input path takes the events. That is a spike, not a rewrite — but
it is a spike that has not been run.

---

## 4. The tiers

Each is shippable, each leaves the machine usable, and each defaults to inert —
Organon's invariant 4, which is what lets a large feature land over weeks.

**T0 — the machine exists and is true.** Stock Omarchy 4, installed by hand.
Fill `hosts/zenbook-01/README.md`'s facts table *from the box*. Confirm which
NVIDIA branch fired (`pacman -Q | grep nvidia`) and whether the Zenbook is
Optimus (`lspci -k`). Confirm `type -a omarchy-shell` shows a shadowable `PATH`.
**Nothing Organon. The deliverable is a machine and a record that agrees with it.**

**T1 — the Organon theme.** `colors.toml` plus shell tokens, in a repo of its
own. Zero fork diff, zero code, reversible with `omarchy theme set`. This is the
literal answer to "skin his UI" and it should ship first because it is the only
tier whose value does not depend on any of the others working.

**T2 — the bar is ours.** A `kind: "bar"` plugin, `omarchy plugin add`. The
layout and the vocabulary become Organon's; the rendering does not. Highest ratio
of visible change to risk in the whole plan, and the documented fallback to
`omarchy.bar` is the safety net.

**T3 — Organon draws one surface.** The spike §3 demands: one
`zwlr_layer_shell_v1` surface, one wgpu device, one egui frame, on the real box.
Measured the way the module frame boundary was measured
(`doc/measurements/module-frame-boundary-2026-08-21.md`) — a number per frame,
and the adapter question from `hosts/zenbook-01` settled here rather than assumed.
🚨 **This tier is a measurement, not a product, and it is the one that can come
back "no".** Everything after it is contingent on it; nothing before it is.

**T4 — Organon answers `omarchy-shell`.** Implement the 18-method vocabulary
behind a socket, and shadow `bin/omarchy-shell` to route to it. **The routing is a
proxy, not a replacement**: Organon answers for the surfaces it has taken and
forwards everything else to `qs ipc` unchanged. That is what makes the tier
land one surface at a time instead of all at once, and it is the exact shape of
the console's own interception — decode what you understand, pass through what you
do not, count what you dropped.

**T5 — Organon is the shell.** Shadow `bin/omarchy-launch-shell`. Quickshell
stops. ⚠️ **Price this honestly before starting: it means having reimplemented
enough of 33 506 lines of QML that the machine is still usable without them** —
bar, notifications, OSD, menu, lock, polkit agent, idle. The tier is reachable
only because T4 lets the count go down one surface at a time; attempted directly
it is a rewrite. The prize is that the World renders *behind* the desktop, which
is the thing none of this is for until it happens.

**T6 — the compositor. Deliberately not now.** Hyprland stays. Replacing it means
a Smithay compositor or a Hyprland fork, and it is a lifetime commitment of the
same class as a second VST3 class ID. The shell layer is the right boundary until
something concrete is impossible above it.

---

## 5. The fork policy, and where Organon's code lives

**Near-zero diff.** `organonart/omarchy` carries `FORK.md` and nothing else.
Themes and plugins are their own repos. The two files T4 and T5 need are
shadowed on the machine through `PATH`, not edited here — a change reversible
with `rm`, tracked in `workshop-machines` where machine-level changes already
live.

📌 The reason is upstream's cadence: 4.0.0 shipped 2026-08-14 and 4.0.1 eleven
days later, dominated by security fixes to plugins, themes, notifications,
Docker privileges and privileged DNS. A diff here is a merge conflict in someone
else's security patch, resolved under time pressure. `FORK.md` carries the
`--ff-only` check that detects drift.

---

## 6. What is not known

1. **Layer-shell in Organon is unproven** (§3). T3 exists to settle it and
   nothing downstream should be scheduled as though the answer is yes.
2. **Which GPU wgpu should pick on an Optimus laptop** is a real question, not a
   formality — `PowerPreference::HighPerformance` reaches for the dGPU, which may
   be wrong for chrome that must never stutter and should never wake the NVIDIA
   card to draw a clock.
3. **`PATH` shadowing order** in the session Hyprland's autostart inherits is
   asserted, not measured. If it is wrong the answer is a different shadow
   directory, never a fork edit.
4. **The 18-method vocabulary is documented, not exercised.** `docs/omarchy-shell.md`
   is the source; whether the 38 callers use only what it documents is a `grep`
   nobody has run.
5. **Nothing in §4 has run on hardware.** This document is a plan. The first
   thing that will be wrong in it is something T0 finds.
