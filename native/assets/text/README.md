# Demo text for the PBR text producer

`organon.txt` is the word ORGANON as `organon-glyphs --input` reads it: 7 lines × 82 columns
of `█` full blocks and spaces, rendered from a 5×7 pixel font at two cells per pixel so the
2:1 cell aspect makes square pixels. Its legibility fixture is
`organon-render/tests/fixtures/organon.txt`, and `verify.sh --legibility --text
native/assets/text/organon.txt` gates the render on it.

⚠️ **There is no blank padding row, above or below, and there cannot usefully be one**
(organon#217 W20). It shipped with one of each in #246 and neither did what it looked like.
Three ttfx behaviours decide where text lands in a canvas: trailing spaces are stripped from
every input line, so an all-space row arrives as an *empty* line; trailing empty lines are
then dropped outright; and the default `sw` text anchor resolves to a row delta of
`bottom - 1`, i.e. zero, which leaves the block on the canvas floor. So **every row of slack
between the text and `--rows` surfaces at the TOP of the published grid and none at the
bottom.** Measured: the nine-line file published an **82×8** grid — the trailing row never
reached the screen at all, and the word carried one dead row above it — while against the
gate's `--rows 9` it published two dead rows above, which is why
`verify.sh --legibility --text …/organon.txt` aborted with *"the ring's text is not the
fixture's: 220 cell(s) differ"* and had never once produced a number.

Padding rows cannot give the word symmetric breathing room for **any** `--rows`. The rig's
`glyph_margin` is the knob that can, and it is the one to reach for.
`no_fixture_carries_a_blank_bottom_row` in `organon-render/tests/legibility.rs` is the
standing guard, over every fixture in the directory rather than a list.

⚠️ ttfx trims trailing spaces from input lines, so a canvas is only as wide as the widest
*trimmed* line — every row here is padded to 82 anyway, and the gate pins `--cols 82`.
`.gitattributes` pins `native/assets/text/*.txt` to LF (the #225 lesson: a CRLF checkout
makes a byte-exact edit silently miss).
