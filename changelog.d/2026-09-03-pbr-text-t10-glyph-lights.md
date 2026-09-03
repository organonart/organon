### PBR text T10 — glyphs as lights, and the backplane rig

The tenth tier of `doc/pbr_text_engine.md` (organon#217, §4.1 / §15): the lit glyphs are
now the point lights, so the green pools onto the backplane around each stroke, and the
`faceplate` rung carries a light rig with a side. Two changes in `organon-world`'s
`world.rs`, both pure functions with the decision pinned by test, and an amendment to the
preset. What did **not** land — the brushed backplane and the warm rim — was measured as
out of `world.rs`'s reach, and the doc now says what it needs instead.

**The point lights come from the emission, not the tint.** The emissive-cubes-as-lights
path (#167 T3) is the renderer's: `gi.rs::update_lights` ranks `Surface.meta_nodes` by
luminance and uploads the brightest N. The world owns what goes into that set, and with a
glyph ring live the answer was wrong in a way nothing reported: the tiles had replaced the
generator's instances, so the node builder handed the renderer every tile — the backplane
included — coloured by its **tint** (the near-black faceplate) or, with no palette, by its
**position in the bounds**, so the "brightest" cells were whichever sat nearest the grid's
top-right corner. `glyph_light_candidates` lowers the grid's emission instead: a lit tile
is a candidate on its **front face** (a light in the face's own plane never lights that
face — `n·l = 0` — and shines past its edges onto the backplane) carrying `emit.rgb *
emit.w`, the exact linear value `cube.wgsl` adds to `emissive`, so the pool a glyph throws
is the colour the glyph shows. Ranking is by **linear** luminance with the renderer's own
Rec. 709 weights, and `lights_are_ranked_by_linear_luminance_not_srgb` pins the pair the
encode flips — a saturated green (linear 0.2146) against a mid grey (0.2), where the
sRGB-encoded rank picks the grey. Off ReSTIR the set is pre-trimmed to the preset's count
so the renderer's own select is the identity; under ReSTIR it gets the whole pool. With no
ring the node builder takes the branch it always took (invariant #4); a grid whose every
cell is dark sheds nothing, and the backplane, which never emits, is never a candidate.

**A stroke is one light, not four.** Sixteen thousand cells cannot each be a light and
`MAX_LIGHTS` is 64, so an unclustered logo lights only its 64 brightest cells and a stroke
lit at one end reads as a stroke with a bulb in it. Adjacent lit tiles in one row fold
into one light at their luminance-weighted centroid carrying the **sum** of their radiance
— exact in the far field, and capped at **four** tiles a run so the pool under a long
stroke does not peak at its middle and fade at its ends (a run of four's ends are 1.5
cells from its centre, inside a two-cell pool). The row key is `floor(y / cell_h + phase)`
with the phase from the row parity, because cell centres sit at half-integer pitches on an
even-row grid and integers on an odd one, and a `▄` is offset 3/8 of a row from its
centre — rounding would put a full block and the half block beside it in different rows,
and `the_row_key_keeps_sub_cell_tiles_in_their_row` fails on exactly that. Cost: a sort
of the lit cells per frame; on the 81×10 logo it takes 64 lights from covering 64 cells to
covering up to 256. ⚠️ The renderer still multiplies every uploaded colour by its
`radiance_scale` (`glow + 0.3·key`) — right for a tint, which is albedo, and one factor
too many for a glyph light, whose colour is already radiance. That is `render.rs`, W8's;
until it lands the preset's `ml_intensity` absorbs it.

**The pool radius is in column widths while a ring is live.** `manylight[2]` is a fraction
of the scene diagonal, which for text is the wrong unit (§5.1): the same lane is 2.6 cells
on the 81×10 logo and 6.8 on a 200×50 fullscreen grid, so the pool would grow with the
amount of text on screen. `glyph_light_radius_frac` converts a lane in cells to the
fraction the renderer wants against the **same** bounds it scales the fraction back by
(`the_radius_is_in_cells_while_live_and_the_lane_otherwise` pins the round trip); with no
ring the lane passes through untouched. Same shape as T3's `glyph_shape`: a Generator /
Look lane re-read under the glyph look while a ring is live.

**`faceplate` gets the lights and a rig.** Many-lights on, 64 slots, radius 2 columns,
brightest-N rather than ReSTIR (a rotating light set on a held frame is a twinkle the T5
dwell would then converge *into*), and a key from low on the right — elevation 15°,
azimuth 70°, `dir_from_angles` putting 90° at `+x` — with a faint fill, so the bevelled
tile edges catch one side and the backplane is raked. ⚠️ **The seed is one-shot behind
`seeded_text_v1`, so amending the rung in place reaches a fresh store only**: a machine
that already seeded keeps the T3 values, by the same rule that keeps a user's own
`faceplate` from being replaced. Picking it up there means deleting that `faceplate` and
the marker, or a `v2` marker that replaces the factory-named entry the way
`seed_rails_presets` does — left to the coordinator, and said so in the function's doc.

**Measured, not done: the brush and the warmth are not reachable from here.** The
anisotropy lobe is a per-draw uniform gated on the material type or an overlay flag, with
no per-instance amount, so switching it on brushes every tile. The renderer's one second
instanced draw — the Demo sub-batch path with its per-batch patched uniform — was read as
the candidate route and rejected: it is excluded from `cube_draw`, so the tiles would lose
T3's bevel and crown, and it zeroes every material overlay including the faceplate's
clearcoat. The brush needs a second instanced draw in `render.rs` for the backplane
instance alone, with a patched group-0 uniform that sets `amb.y` to `Anisotropic` and
keeps `shape` and the overlays. The warm rim is simpler and further away: the key light is
white (`key_rad = key_light.w`) and no lane colours it, so it needs a key colour on the
param chain and in `cube.wgsl`. The tiles and the backplane **are** in the TLAS already —
T1's `rt_instances.clear()` is what makes `rt_geo` the instance buffer, and the backplane
is real geometry for exactly this — so shadows and AO can fall in the wells the moment the
RT passes read the emit buffer (T8).

🚨 No GPU touched any of this: green and ready to try. A GPU session must load `faceplate`
with `organon-glyphs` running and see a green pool on the backplane under each lit stroke,
brightest strokes first; the pool holding its size when the grid changes size; and the
key catching the tile edges from the right, the left edges dark under the fill.
