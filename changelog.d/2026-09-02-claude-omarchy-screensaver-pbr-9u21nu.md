### PBR text — the terminal screensaver as a lit object

`doc/pbr_text_engine.md` proposes rendering a terminal cell grid as real lit geometry instead of as
blitted coverage bitmaps, using Omarchy's `terminaltexteffects` screensaver as the forcing function.
It is design only — nothing is built — and §12 is an explicit measured/reasoned ledger rather than a
summary, because several of the load-bearing claims below were established by running things against
the tree and several were not.

📌 **The project is not really a screensaver.** `organon-console/src/term_view.rs`'s own module doc
names the gap it closes — *"the dedicated glyph-atlas instanced pipeline (the perf ceiling) is a
later tier of #10"* — so the Console's terminal and the screensaver want the same pipeline and
building it once pays for both. The screensaver is the right forcing function because it is visually
unforgiving, has no interaction-latency budget, and has a public that already likes what it would
replace.

🚨 **The seam is the cell grid, not the effect code, and that changes the size of the work by an
order of magnitude.** `terminaltexteffects` is ~7.6k lines of engine under ~13k lines of effects
across 36 files; the effects are where the beauty is and reimplementing them in Rust has no leverage.
Every effect resolves through `Terminal._update_terminal_state()` to one rectangular buffer of
`CharacterVisual` — symbol, foreground, background, SGR flags — which sits *below* all of them. And
Organon can already read such a buffer: `organon-console/src/term.rs` runs a real PTY through
`alacritty_terminal`, so tapping `ttfx` costs approximately a day and inherits every future effect
for free.

📌 **The glyph problem is ~5% of what it looks like, measured rather than assumed.** A character
census of `logo.txt` is `Counter({'█': 337, ' ': 312, '▄': 32, '▀': 32})` — the Omarchy logo is
*three* glyphs — and an inventory of the symbols the 36 effects substitute in during animation is
overwhelmingly the same block/shade family. Every one is an axis-aligned sub-cell rectangle, which is
to say they are already the beveled box the proposal wants, and `cube.wgsl:528`'s `round_local()`
rounded-box morph draws them today. ⚠️ Real letterform extrusion is kept as T7 and explicitly is
**not** a prerequisite for anything before it; treating it as one is how this would fail to ship.

🚨 **Colour must drive emission, not albedo, and this is physics rather than preference.** A TTE
colour is display-referred — the output of a lighting model, not a material — so as albedo it fails
three compounding ways: albedo is bounded and multiplicative so it can only ever render *darker* than
the source; it is chromatically filtered by the light so authored absolutes shift; and `N·L`
modulates the effect's gradient by geometry, which is the "it stopped being crisp" failure people
feel without naming. A terminal is an emissive display and a phosphor is an emitter, so the correct
model is an emissive element behind a dielectric faceplate. ⚠️ The tree cannot express this yet:
`cube.wgsl:1544` computes `emissive = albedo * (glow + u.env_tint.w) + …`, which a near-black
dielectric albedo multiplies to nothing. The change is a per-instance `emit: vec4` at location 8
alongside the existing `mat4` (3–6) and `tint` (7) — a vertex-layout addition local to the cube
pipeline, **no `Shared` field and no `LAYOUT_VERSION` bump**, so invariant #2 is untouched.

📌 **Getting that right is what makes the rest work for free.** `post.wgsl`'s `prefilter()` is a
soft-knee bright-pass, so emission above 1.0 triggers bloom on exactly the lit glyphs and nothing
else — where albedo-driven colour never crosses the threshold and you end up cranking bloom globally
and fogging the image. `cube.wgsl:201`'s *"emissive cubes as real lights (#167 Tier 3)"* then throws
the green pool onto the backplane. And a clearcoat layer gives dark cells an environment specular
*independent of emission*, so an unlit cell shows a faint sheen of the room exactly like a
switched-off patch of real faceplate — where a terminal can only render `#000000`.

🚨 **Impostors are invisible to hardware RT, which decides the geometry question.** `rt_shadow`,
`rt_reflect`, `rt_gi`, `rt_ao` and `rt_caustic` all bind a `wgpu::Tlas` and only triangles enter a
BLAS, so if glyphs are to cast ray-traced shadows into their own cell wells — probably the
highest-value shading term in the design — the hero path must be mesh, not the analytic
sphere/capsule impostors the tree already has.

📌 **A screensaver has the one thing an interactive app never has: time.** `world.rs:8458` restarts
path-trace accumulation on camera move, resize, or a content-setting change — **not** on geometry
change. Every TTE effect animates, resolves to its `final_gradient` and holds, so the proposal is to
raster during motion and hand the held frame to the path tracer: the screensaver visibly *resolves
into a photograph*, with real dispersion and caustics, then dissolves and does it again. The change
needed is one line's worth — add the cell-grid generation counter to the `pt_content` tuple so
accumulation restarts when glyphs move rather than never. ⚠️ TAA must stay off: `temporal.rs`
reconstructs velocity from camera reprojection only and its own doc says per-object deformation
ghosts, and glyphs teleport cell-to-cell.

⚠️ **Two things are named early because they are the ones most likely to sink it.** TTE's
`geometry.Coord` is integer and `Path.step` discards the sub-cell remainder — invisible in a terminal
where the cell *is* the atom, and read as stepping once there is a camera. That is the one place this
project *changes* the original rather than adding to it, and it is the risk to retire first. And
`ttfx` is only *assumed* to be `terminaltexteffects`' lineage, on CLI-surface evidence (`-i`,
`--anchor-text`, `--reuse-canvas`, `--random-effect`, `--xterm-colors`); Omarchy's migration
`1786355450.sh` replaced `python-terminaltexteffects` with it, but whether it is a rename or a native
rewrite was not established — and that decides whether patching the writer is even an option.

📌 **The law that lets a preset go far, and the reason it is unusually testable.** Legibility of a
glyph grid comes from the grid, not from each tile's silhouette, so a cell may become a glass tube or
a lump of cooling metal provided its energy stays inside the cell and its integrated luminance tracks
what the effect said that cell was. Both are measurable: downsample the render to the cell grid and
correlate against the source cell luma. Given a fixed grid the render is deterministic, which makes
this one of the rare Organon features that can carry real automated visual regression rather than the
usual no-GPU `cargo test --workspace` ceiling — so the harness is T2, before the exotic presets
rather than after.
