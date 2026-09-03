# Demo text for the PBR text producer

`organon.txt` is the word ORGANON as `organon-glyphs --input` reads it: 9 lines × 82 columns
of `█` full blocks and spaces (one all-space row above and below the seven glyph rows),
rendered from a 5×7 pixel font at two cells per pixel so the 2:1 cell aspect makes square
pixels. Its legibility fixture is `organon-render/tests/fixtures/organon.txt`, and
`verify.sh --legibility --text native/assets/text/organon.txt` gates the render on it.

⚠️ ttfx trims trailing spaces from input lines, so a canvas is only as wide as the widest
*trimmed* line — every row here is padded to 82 anyway, and the gate pins `--cols 82`.
`.gitattributes` pins `native/assets/text/*.txt` to LF (the #225 lesson: a CRLF checkout
makes a byte-exact edit silently miss).
