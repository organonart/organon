### PBR text T9 — every cell gets a tile: the lowering half, behind a switch that defaults to today

`doc/pbr_text_engine.md` §15's first row: the spec-sheet plate shows every cell as a
tile — a dark cell is a dark, glass-capped tile that reflects the room, a quarter as proud
as a lit glyph — and the first render drew a dark cell as nothing, i.e. bare backplane. The
shading half (the clearcoat sheen a zero-emission tile shows, the emission profile) is
#233 in `cube.wgsl`; this is the other half, in the pure lowering.
`glyph_ring::lower_grid_with` takes a `LowerOptions`, and with `dark_tiles` on a cell that
draws no symbol — empty, space, a control — lowers as a **full-cell tile at the `░`
depth** (`DARK_TILE`: `0..1 × 0..1`, `depth 0.25` on the shared `look.depth` scale, so a
dark tile sits a quarter as proud and a lit full block still reads four times raised),
faceplate tint, and `emit = (0, 0, 0, gain)`. Symbol cells lower exactly as before; the
backplane, the gap and the contact-shadow wells are untouched; the bounds are untouched.
**Default off, and off is byte-identical** (invariant #4): `lower_grid` *is*
`lower_grid_with(…, LowerOptions::default(), …)`, and the pin lowers an asymmetric
fixture — holes in different places per row, a slide with a remainder, a letter, a T11
trail, a space character on a path — both ways at three blends and compares every
instance, tint, emit and the bounds. The world still calls `lower_grid`; nothing changes on
screen until the wire lands.

📌 **A lowering option, not a `GlyphLook` field, and the reason is the landing order.**
The brief named `GlyphLook.dark_tiles`, and the world builds `GlyphLook` by a full struct
literal (`world.rs::glyph_look_from`), so a new field there does not compile until the
one-line wire lands — and `world.rs` is T10's file (#234), which this change must not
touch. An options struct with a `Default` lets this land first and the wire follow in
either order, with `main` green at every step; and it is a struct rather than a `bool` so
T12's lowering-only switch (sub-cell rendering) is a field, not another signature. The
proposed lane is **`Shared.glyph[14]`** (`> 0.5`), `[13]` being the profile strength #233
named; the wire is the world passing `LowerOptions { dark_tiles: s.glyph[14] > 0.5 }` to
`lower_grid_with` where it calls `lower_grid` today.

**Three rules the spec left to the lowering, each pinned and mutation-tested.** *A dark
cell never slides.* A space **character** on a path is a real ttfx thing — it carries a
`character_id`, `SGR_ACTIVE_PATH` and a sub-cell remainder — and its tile is the
faceplate's cell, not the character's, so it sits at the cell centre whatever the cell
says; giving it `exact()` and the slide fails with *"not slid, not offset: [1.9, -0.6,
0.0225]"*. *A dark cell emits exactly zero whatever colour the producer left in it* — the
lowering multiplies the cell's colour by the tile's `emission`, and `DARK_TILE`'s is `0.0`,
so a stale `fg` in an empty cell cannot light it and neither the bloom prefilter nor the
brightest-N light selection can ever pick a dark tile; making it emit `default_fg` fails
with *"a dark cell emits nothing: [0.75, 0.75, 0.75, 3]"*. *A T11 trail is a lit cell*,
dim but not dark: the rule fires only on symbol-less cells, and the trail test asserts its
decayed colour and full-block depth. The bounds pin needed a fixture the lit one could not
provide: on a grid with lit full blocks, dark tiles are inside the backplane's footprint
and below every glyph in `z`, so folding them into the bounds is invisible — the mutation
**survived** the first version. On an all-empty grid it lifts `max.z` from the
backplane's face to a quarter depth and the camera frames a different box for the same
grid; that grid is now in the test and the mutation fails there.

**Measured, the fullscreen case §15.2 called unmeasured** — the CPU half of it. A
synthetic 200×80 grid (16 000 cells, one in seven lit and sliding), `--release`, best of
fifty runs per condition in **interleaved** rounds: **92 µs** with dark tiles off (2 286
instances), **125 µs** with them on and sliding (16 001), **92 µs** on and settled. ⚠️ The
first draft ran the three conditions in a fixed order and reported "on" *faster* than
"off" (125 against 160 µs) — clock ramp and cache warmth landed on whichever ran first,
which is why the rounds interleave now. The test prints and never gates, and it names its
own build: under `CARGO_PROFILE_TEST_OPT_LEVEL=0` it says so in the line, because an
unoptimised lowering is a different program. So the lowering is not where a fullscreen
frame's cost will be; the 16 000-instance draw is, and that is unmeasured until a GPU look.

⚠️ **The two plates disagree, and the switch is what reaches both.** The spec sheet tiles
every cell; the before/after plate's "after" panel shows lit tiles over a bare brushed
backplane, no dark tiles at all. Off is the before/after plate, on is the spec sheet.
Green and ready to try; no dark tile has been drawn. With the lane wired and on, a GPU
session should see dark cells as dim glass tiles carrying the environment's sheen between
the lit glyphs, a quarter as proud, the lit glyphs unchanged at full depth — and should
watch the frame time on a fullscreen producer, since that is the number nobody has.
