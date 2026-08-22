### A link-preview card, rendered from a page rather than drawn

Pasting `organon.art` anywhere produced no preview, because the page carried `og:title`,
`og:description` and `og:url` but **no `og:image`** — and without an image most platforms render a
bare text row or nothing at all. `site/og.png` is now that image: 2400×1260, the mark and wordmark,
the claim in italic serif, and the sentence closed by a teal cursor block, since the claim is that
it *starts as a blinking cursor*.

📌 **It is rendered from `site/og.html`, never drawn**, so the headline and the palette are a diff
rather than an image somebody has to reproduce by hand. There is no image toolchain in this
repository and none is wanted — the whole build is:

```
chrome --headless=new --hide-scrollbars --force-device-scale-factor=2 \
       --window-size=1200,630 --screenshot=site/og.png site/og.html
```

Headless Chrome is already on any machine that can look at the site, and it renders with real
fonts, which is the reason to prefer it over an SVG converter.

⚠️ **`og.html` duplicates the palette rather than sharing it, and it is the only real duplication
on the site.** An `og:image` is a flat file — it cannot read a custom property at paste time. The
comment at the top of that file says so in as many words: **if the palette moves, re-render, because
the card will not follow on its own and nothing will tell you.** That is a genuine second copy
accepted with its cost named, rather than one that crept in.

⚠️ **Three ways these tags fail silently, all of them recorded beside the tags themselves.** The
URL must be **absolute** — a relative path is ignored by most scrapers. The image must be a
**raster**; no scraper renders SVG, which is what the favicon is. And `og:image:width`/`height`
must state the **pixels of the file**, 2400×1260, not the 1200×630 the card is laid out at — the
2× device scale factor is exactly the sort of thing that gets copied wrong from the render command.
`twitter:card` moves from `summary` to `summary_large_image`, which is what makes it render wide
instead of as a thumbnail.

✏️ **One defect the first render caught, which no check would have.** The cursor block wrapped onto
a line of its own beneath the sentence — it is punctuation on the last word, not a mark underneath
it. The closing word and the block are one `nowrap` span now. It was found by looking at the PNG,
which is the only way that class of thing is ever found.

📌 Both pages get the tags, so a pasted `/docs` link previews too.
