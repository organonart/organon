### Organon on the desktop — the Omarchy plan

`doc/organon_desktop_plan.md` asks whether the console's move works one layer out:
Organon took charge of the console by running the child, decoding the structured
stream it emits and rendering the conversation natively; can it take charge of the
*desktop* the same way, against Omarchy 4 "Quattro"?

The answer is yes, and the seam is cleaner than the console's — but not where the
analogy puts it. **Omarchy's desktop has no text stream to intercept.**
`omarchy-shell` is one long-running Quickshell process (95 QML files, 33 506
lines) rendering Wayland surfaces directly; there is nothing between it and the
screen to decode. One layer up, though, there is a better pipe than the console
ever had: `bin/omarchy-shell` drives the whole desktop by running
`qs ipc -n -p "$OMARCHY_PATH/shell" call -- "$@"` and parsing the text that comes
back — 38 of the 439 commands in `bin/` funnel through that one file, speaking a
vocabulary of 18 documented methods. 📌 **Answer those 18 and every Omarchy
command keeps working while Organon owns every pixel**, which is the console's
shape exactly: unchanged producer, replaced renderer.

🚨 **One prerequisite is unproven and the tier order is built around it.**
Organon's window stack is `winit` + `wgpu` + `egui`, and winit makes ordinary
`xdg-shell` toplevels. A bar, an OSD, a menu and an overlay are
`zwlr_layer_shell_v1` surfaces; a lock screen is `ext-session-lock-v1`. winit
implements neither, so today Organon cannot draw a single piece of desktop
chrome. The way across is a road already walked — `editor_probe.rs` puts a wgpu
surface on a host's parent view and `baseview_input.rs` feeds `egui::RawInput`
with no winit at all, so `smithay-client-toolkit` creating the surface is the same
shape with a different provider. That is a spike, not a rewrite, and the plan's
T3 is that spike and nothing else: a measurement that can come back "no", with
every later tier contingent on it and no earlier tier touched by it.

⚠️ **The fork stays near-zero-diff, and the reason is upstream's cadence rather
than tidiness.** Quattro shipped 2026-08-14 and 4.0.1 eleven days later, dominated
by security fixes to plugins, themes, notifications, Docker privileges and
privileged DNS. Every line carried in `organonart/omarchy` is a line rebased
forever, and the ones that hurt are conflicts inside somebody else's security
patch. So Organon's desktop code lives in Omarchy's own extension points — a theme
is `colors.toml`, a bar is a `kind: "bar"` plugin in a repo of its own with a
documented fallback to `omarchy.bar` — and the two `bin/` commands the later tiers
need are **shadowed through `PATH` on the machine**, reversible with `rm`, rather
than edited in the fork.
