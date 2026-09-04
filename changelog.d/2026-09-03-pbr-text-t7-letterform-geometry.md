### PBR text T7 phase one — a real letterform is an extruded, bevelled mesh, headless

`organon-core/src/letterform.rs`: `ab_glyph` outlines → flattened contours → a non-zero
scanline cap → a bevel band → an extruded side wall → an LRU mesh atlas, as plain `Vec`s
of vertices and indices. **No `wgpu`, no buffers, no draw**, and not one line of the
renderer or the tile path changed — §14 of `doc/pbr_text_engine.md` says treating T7 as a
prerequisite for anything before it is how this project would fail to ship, so this tier
builds the geometry a later one may adopt rather than the adoption. Behind the default-off
`letterform` feature, which nothing forwards: `cargo tree -p organon-core` still prints its
six dependencies and the crate's acceptance test is unchanged. ⚠️ The other half of that is
that **the default build does not compile the module at all**, so a green
`cargo test -p organon-core` says nothing about it — 791 either way; the feature leg is
791 → 825. Everything is in **em units**, converted once at the font boundary, so `depth`,
`bevel` and `tolerance` transfer between faces. §16 of the engine doc is the design as
built.

**Measured** on the repository's own CFF face, `site/fonts/CommitMono-400-Regular.otf`:
ORGANON at the 0.005 em default is **2 286 triangles** (1 394 at 0.02 em, 4 456 at
0.001 em), and the atlas over all 95 printable ASCII glyphs is **1.82 ms cold / 11.4 µs
warm** in `--release` — 19 µs a glyph to build, 120 ns to find, 28 438 triangles held for
the whole set. That last number is the budget answer for a 200×80 grid: the atlas is 28 k
triangles *total* and a cell is an instance of one, so tolerance prices the atlas and never
the draw. Vertices are deliberately **not** welded (`vertices == 3 × triangles`), because
welding needs a hash keyed on position *and* normal and getting it wrong smooths a corner
that should be sharp.

🚨 **The cap is a sampler, not a triangulator, and that is what makes real fonts survive
it.** The plane is cut into bands at every vertex's `y` *and every self-intersection's* `y`,
then the non-zero winding rule is evaluated once per band at its midpoint; inside a band no
vertex and no crossing exists, so each filled span is an **exact** trapezoid. An ear-clipper
asks whether the input is a valid polygon and either refuses or folds; this asks what the
rasteriser would fill, which has an answer for every input. Non-zero rather than even-odd
is not taste: a composite glyph — nearly every accented letter — is two overlapping
components, and even-odd punches the overlap out of the middle of the letter. ⚠️ The
**side wall** still follows the source contours, so a self-intersecting contour puts a wall
segment inside the solid: invisible for an opaque material, not for glass, and resolving it
needs a real boolean on the filled region.

🚨 **The material is not on the side the contour's winding says it is, and both wrong
answers look right on a square.** The obvious rule — interior is left of travel for a
counter-clockwise ring, right for a clockwise one — is correct for an *isolated* ring and
wrong for a nested one, **and the two cases are the same clockwise ring**: an isolated
clockwise square has its material on the right, a clockwise counter inside a
counter-clockwise `O` has material on its left. Neither "always inset left" nor "inset
along `sign(area) × left`" is right; the first grows half the cases and the second shrinks
a counter that has to grow. What settles it is the winding number of the whole ring **set**
either side of the contour, so that is what the module measures — a probe a hair to each
side of the middle of the ring's longest edge, halving on a tie so a stem thinner than the
epsilon still gets an answer. It matters beyond one bevel: **TrueType and CFF wind their
outer contours opposite ways**, so a module that guesses works on half the fonts installed.

⚠️ **A contour is not dropped for having zero *signed* area**, which the first draft did.
A figure-eight with equal lobes has signed area exactly zero and *filled* area twice one
lobe, because under non-zero both lobes have winding ±1 — so the rule deletes a contour a
rasteriser draws. Degeneracy is tested as "every point on one line", which is what actually
cannot bound anything. Separately, `ab_glyph`'s `close()` emits `Line(last, first)` even
when `last == first`, so a zero-length edge arrives on nearly every glyph; left in, the
edge direction is `(0,0)` and the miter normal is a NaN that propagates through the ring.

📌 **Two tests passed against deliberately broken code, and the mutation harness is the
only reason either was found.** The bevel-normal test asserted *at least* one tilted normal
per bevel triangle; replacing one of the two per-edge miter normals with the cap's own
`(0,0,1)` left the other tilted and the lower bound could not see half a band go flat — an
equality (`3 × bevel_triangles`, and no other vertex) kills it. And **area is not shape**:
removing the self-intersection scanlines makes the bowtie fixture tessellate as *one*
triangle of area exactly 4 where the true fill is two lobes of area 2, the same number and a
completely different picture, with the area assertion reporting success. Every fill test now
pins covered and uncovered *points* as well as totals. Eleven mutations, eleven kills.

**Where the cell law lands (§9 law 1), split honestly.** In **z** the mesh spans exactly
`±depth/2` — bounded by a parameter. In **x/y** it can only ever *shrink* the silhouette,
because the bevel insets — but the contour's own bounds routinely leave the em square, and
that is the font's business, not this module's: CommitMono's `g` occupies y `[−0.210,
0.550]` em and fits no cell centred on the em. `Bounds3::fits_cell` asks the question in
the only form that has an answer, and a consumer that needs the law must measure and then
scale or accept the overhang. The mesh atlas is keyed on
`(font id, glyph id, depth, bevel, tolerance)` with the font id an FNV-1a of the font's
**bytes** — a key derived from a name serves one face's glyph for another's, and the
symptom is a single wrong letter in an otherwise perfect string. A shape parameter missing
from that key is not a slow cache but a wrong mesh served silently, which a test proves by
building the broken key and watching two bevels collide.

**Green and ready to try, never verified working**: there is no GPU look here and phase one
does not need one, since nothing draws it yet.
