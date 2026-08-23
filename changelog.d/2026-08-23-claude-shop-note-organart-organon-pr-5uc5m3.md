### A shop note on issue #192, and the builder that makes one

`doc/shopnotes/` opens with one note, *The Manual as Training Data*, written up from issue
#192: the plan to turn `doc/reference/` into a training corpus, the five steps, and which
pieces of them exist in the tree today. Five body pages, one figure, built to PDF and
committed beside its source.

📌 **The builder crosses with it.** `doc/build_shopnote_pdf.py` is the house letter style
(classic serif, brass-ruled heads, title page, Workshop footer) as one reusable script, so
a new note needs no build code of its own — the second note is a Markdown file and nothing
else. A PDF committed with no way to rebuild it is a binary nobody can review; this is the
half that makes the first half reviewable.

⚠️ **Two of the note's claims are worth repeating here, because they are measurements
rather than plan.** The catalog is 27 generators, 10 surfaces, 8 materials, 48 parameters
and 7 recipes, and `doc/reference/` plus `doc/guide/` come to about 10,600 words — the
whole corpus the issue proposes to train on. And the scope argument re-measures: on this
repository's own log (186 commits, three days) the reference pages were touched once and
the guide twice, against 42 for one architecture document and 123 for the changelog
fragments. That is the two-orders-of-magnitude spread the issue's six-month figures found,
reproduced on public history.

🚨 **The note says the corpus builder belongs beside `organon docs` in the binary, not in
`native/tools/`**, and the reason is the one this repo already applies to `doc/reference/`:
anything that parses the generated Markdown is downstream of it and can drift, while both
artifacts generated from the same Rust definitions cannot. Nothing of the five steps is
built yet, and the note says so on every page it comes up.

`doc/shopnotes/README.md` is the authoring guide: the header contract, the voice rules (no
em dashes, and never the word the rule names), figures, and the page-by-page check that
catches the orphaned line the Markdown cannot show you.
