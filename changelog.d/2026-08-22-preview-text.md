### The preview text catches up with the headline, and the two descriptions stop being one

The card image was replaced hours before the words under it were, so a paste showed the new
tagline in 68px italic sitting on top of the *old* mechanism-first sentence — "Divide the window
into regions, declare what each holds…" — which is precisely the framing the page had already
moved away from. The same string was the GitHub repository description until James replaced it.

📌 **`name="description"` and `og:description` now carry different text, deliberately, because they
do different jobs.**

- **`name="description"`** is what a *search result* shows, where there is no image and the tagline
  has to carry itself. It leads with the tagline and is **byte-identical to the GitHub repository
  description** — one wording, several front doors, the same discipline §1.1 already applies to the
  identity claim.
- **`og:description`** is what sits *under the card*, and the card image already says the tagline
  in 68px italic. Repeating it there would print the same sentence twice inside one preview, so
  this one carries the second beat: what Organon replaces, and the thing a terminal cannot do.

⚠️ **That distinction is written above the tags rather than left to be inferred**, because the
obvious "tidy-up" is to make the two match, and doing so would quietly make every link preview say
its own headline twice.

✏️ **`CLAUDE.md`'s top bullet still said both sites were "built elsewhere; not in this repo".**
Half of that stopped being true when `site/` landed, and it survived the change because the same
commit updated the repository map and the doc-ownership table — the two places anyone would think
to look — while the stale claim sat in the naming section above them. The site row in the doc table
was added and the sentence contradicting it was not read. Same shape as the three findings before
it: **the copy that was on screen got fixed and its twin did not.**
