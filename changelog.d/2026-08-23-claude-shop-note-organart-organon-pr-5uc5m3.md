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
whole corpus the issue proposes to train on. And the scope argument re-measures: across
this repository's 542 commits, `doc/reference/` was touched **2** times and `doc/guide/`
**3**, against **27** for `ARCHITECTURE.md`, **100** for `CONSOLE_ARCHITECTURE.md` and
**168** for `changelog.d/`. That is the same near-two-orders-of-magnitude spread the
issue's six-month figures found, on a different fortnight of history.

🚨 **The first version of those numbers was wrong, and the way it was wrong is worth
keeping.** It claimed 186 commits over three days. `git log --oneline | wc -l` really did
print 186 in the session that wrote it — in a **shallow clone**, which is what an agent
session gets by default. A truncated history answers every `git log` question confidently
and quietly, with no marker on the output saying the log stops early. `git rev-parse
--is-shallow-repository` is the one-word check, and `git fetch --unshallow` is the fix;
the corrected figures above come from the full log. ⚠️ Note which half of the claim
survived: the *shape* (reference and guide barely touched, architecture and changelog
churning) was right in both counts, so nothing about the argument looked wrong from the
inside. That is the failure mode of a measurement whose instrument is misconfigured rather
than whose reasoning is bad, and it landed in a note whose own rule is to say whether a
number was measured or estimated. Caught by the automated review on #193, which re-ran the
commands against the same commit.

🚨 **The note says the corpus builder belongs beside `organon docs` in the binary, not in
`native/tools/`**, and the reason is the one this repo already applies to `doc/reference/`:
anything that parses the generated Markdown is downstream of it and can drift, while both
artifacts generated from the same Rust definitions cannot. Nothing of the five steps is
built yet, and the note says so on every page it comes up.

`doc/shopnotes/README.md` is the authoring guide: the header contract, the voice rules (no
em dashes, and never the word the rule names), figures, and the page-by-page check that
catches the orphaned line the Markdown cannot show you.
