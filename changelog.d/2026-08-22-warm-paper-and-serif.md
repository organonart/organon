### organon.art gets warm paper, a serif, and blue-grey chrome

The first cut matched `organonmind.org` exactly — stark white, system sans — on the reasoning
that the two sites should read as one outfit. Seeing it rendered, James's call was that the
resemblance had been bought too cheaply: *"I don't want the such white and black clinical look…
something a little bit more fun."* So the **structure** carries the family resemblance and the
**surface** stops trying to: the tracked-monospace wordmark, the labelled meta block, the measure
and the colophon are still the publications', while the ground is warm bone (`#faf9f7`, never
`#ffffff`), the ink is a warm near-black, headings *and* body are set in a serif, and every
hairline, panel border and small mono label is a cool blue-grey. A faint dotted field sits behind
all of it — one CSS radial gradient, no asset, no request.

📌 **The reference is pi.dev, and what was taken from it is the type pairing and the chrome, not
the palette.** Pi sets everything in two families — a serif for display *and* body, a mono for
every label and terminal — with content in bordered panels that carry a small uppercase header.
That pairing is most of why it reads as friendly rather than clinical, and it survives being moved
onto light paper. What was deliberately not taken is Pi's dark ground: `organonmind.org` is stark
white, and inverting one of two sibling sites would have cost more resemblance than the surface
was worth.

🚨 **Both font stacks now LEAD with faces this repository does not ship, and there are no
`@font-face` blocks at all.** `Plantin MT Pro` and `Commit Mono` are Pi's, James is licensing
them, and until the `.woff2` files exist in `site/fonts/` a rule pointing at them would 404 on
every load. Naming them first in the stack costs nothing — a reader without them falls straight
through to `Iowan Old Style` / `Charter` / `Palatino` / Georgia — and makes the arrival of the
files a ten-line addition rather than a re-typesetting. ⚠️ The trap worth recording, because it is
the expensive one: **a desktop licence does not permit `@font-face`.** Monotype sells web
embedding separately, and self-hosting a desktop purchase is a breach, not a shortcut.

⚠️ **Two temperatures, on purpose, and the plates must not be "fixed" to match the page.** The
paper and its chrome are cool blue-grey; the dark plates stay warm graphite, because PRD §5.4 says
the application's shell is *"near-black with a hint of brown, never blue-black"*. The plates depict
Organon, so they carry Organon's colour rather than the website's. Tinting them to match would
make the page more coherent and the depiction wrong.

✏️ **The hero is centred now, and that came from looking at it rather than measuring it.** The
first cut left the headline in a 30rem column inside a 66rem container, so it broke to six lines
with half the page empty beside it — a defect that every automated check passed, because nothing
was overflowing, nothing was unbalanced in the DOM, and no line wrapped where alignment depended on
it. It took a screenshot. The mark now sits above the claim, the claim is italic at a wider
measure, and one two-line clone-and-build block sits under it; the three-variant build plate stays
in *Getting it*, so the hero is a summary rather than a second copy.

⚠️ **`site/README.md`'s "no external requests" rule is restated rather than weakened.** It is a
promise about *other hosts*: a `.woff2` served from `site/fonts/` keeps it intact, a Google Fonts
link breaks it. The old wording said "system type only", which conflated the promise with the
current state of a purchase — and would have read as forbidding exactly the thing that is about to
happen.
