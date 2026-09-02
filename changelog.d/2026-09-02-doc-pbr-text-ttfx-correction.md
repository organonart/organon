### PBR text — `ttfx` is a Rust library, and the design is corrected before any code

`doc/pbr_text_engine.md` recommended tapping Omarchy's screensaver through a PTY because the
alternative appeared to be forking a Python package. James settled the open question the design
had flagged as "settle this first": **`ttfx` is a Rust port of terminaltexteffects, written by
DHH by having an agent port the Python.** Read against `organonart/ttfx @ 7203e35`, it is **MIT**,
it ships a **library target** (`src/lib.rs`; `main.rs` is a thin driver), its `CharacterVisual`
mirrors Python's field for field, and its `Effect::next_frame` hands the caller one frame per
call under a `Clock::Virtual` that never sleeps. So §2 inverts: **link it as a git dependency**
(it is not on crates.io) from a small permissive producer crate, and keep the PTY tap as the
process-boundary fallback and as the route to the Console's own terminal.

📌 **The win is identity, not colour.** The first draft of this correction assumed the terminal
round-trip was quantizing colours; it is not — ttfx stores `rgb: [u8; 3]` at the source, and only
`--xterm-colors` (which Omarchy's screensaver does not pass) reduces further. What the library
knows and the terminal forgets is *which character* is in a cell, its layer, `previous_coord`,
and whether `motion.active_path` is set — the last being exactly the slide-versus-cut signal §7's
sub-cell interpolation needed and could not get from a terminal. §7's "highest-risk unknown"
becomes a Tier 1 gate plus an optional additive upstream patch that leaves ttfx's byte-for-byte
parity suite untouched.

🚨 **Running the effects falsified half of §8.** All 37 were run headless (`--parity-dump`,
`--seed 1`, the Omarchy logo): **37 / 37 settle to the input text, and almost none hold it** —
eleven exit within three frames of the text landing, the colour keeps moving through the final
gradient until the last frame, and `bin/omarchy-screensaver`'s `while true` restarts the process
immediately. The terminal screensaver has no dwell at all. The hold that "converge on hold"
rests on is therefore ours to add, which on the library route is trivial (the producer owns the
loop) and on the PTY route would have meant patching Omarchy's loop.

⚠️ Two things the effects were authored against that a tile grid must preserve, both measured in
`geometry.rs`: the integer step is what the effects' *timing* is authored against, so smoothing
may change where a character is between ticks and never when it arrives; and **TTE's cell is
2:1** — row deltas are doubled in every length and x offsets doubled on every circle — so square
tiles turn every ring into an ellipse. The cell aspect goes in the ring header. Also measured:
ttfx `cargo check`s and builds on Windows despite its README saying Linux and macOS, because the
Unix-only part is signal plumbing a library caller never invokes.
