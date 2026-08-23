# Shop Notes

A **shop note** is a short typeset letter about one piece of work, written from the
workbench for a reader who was not there. Not documentation, not marketing, not a paper.
Each one is a Markdown file here plus the PDF built from it, and the builder that turns
one into the other is `doc/build_shopnote_pdf.py` — **the whole of the machinery**, shared
by every note, so a new note needs no build code of its own.

```bash
python3 doc/build_shopnote_pdf.py doc/shopnotes/issue-192-manual-as-training-data.md
# writes issue-192-manual-as-training-data.pdf beside the source
```

It needs `reportlab` and nothing else. Fonts resolve to macOS Times New Roman where that
exists and to Liberation Serif (its metric twin) otherwise, so line breaks measured on one
machine hold on the other.

## The two flavours

The `**Issue**` line in the header picks the stamp on the title page, and with it the job
the note is doing:

- **an open issue → "Notes on work in progress"**: what we intend to build and why, with
  plain markers for what exists today against what is planned, sometimes ending in a
  question for the reader.
- **a closed or shipped issue → "Notes on finished work"**: what was built, how it works,
  what was verified and how, and what was deliberately left out.

Many issues here are programs that stay open while their tiers land one at a time. For
those the note's job is visibility: say which parts are on `main`, which are in flight,
which are still ideas, and what each is *for*. Never let a note imply more is built than
is.

## The header, which is a contract

```markdown
# The Title of the Note
**Author**: James Andrew Walsh
**Issue**: #192 · open
**Subject**: an italic subtitle for the title page
**In one line**: the whole note in one sentence
**Date**: August 2026
```

`open` gives *Notes on work in progress*; `shipped` / `closed` / `landed` / `merged` /
`done` give *Notes on finished work*; anything else gives *Notes from the workbench*.
Body sections are `##` (brass-ruled in the PDF). `[[PAGEBREAK]]` forces a break, and is
worth using only to keep a section with its figure. There is no table support: turn an
issue's tables and checklists into prose or bullets.

## The voice, which is not negotiable

- 🚨 **No em dashes. Ever.** Recast as commas, colons, parentheses, semicolons, or two
  sentences. En dashes inside compounds (reaction–diffusion) are fine. `grep -c '—'` on a
  finished note must be `0`; the builder warns as well.
- 🚨 **Never the word "honest", in any form.** We are, so saying it adds nothing, and a
  document that announces the quality reads as claiming it rather than showing it. Write
  the fact instead: *"this part is not built"*, *"that number is estimated, not
  measured"*, *"nobody has run it yet"*. The rule covers figure captions and labels, which
  is where it slips back in, because captions get written last.
- **Plain workbench register.** First person, complete sentences, no corporate or academic
  tone. Someone who knows nothing about the project should be able to follow every
  paragraph.
- **No lofty metaphors and no slogans.** Do not name machinery with poetic images, and do
  not end a section on an aphorism. End on a concrete fact. If a sentence would look at
  home on a poster, rewrite it.
- **Explain or drop every term of art**, in the prose and doubly inside figures, where
  labels use everyday words only.
- **Show what is built and what is not.** This is the house feature. Separate what exists
  from what is planned, in as many words. When a displayed quantity stands in for
  something else, say so. When a number appears, say whether it was measured, calculated
  or estimated. Do all of that without reaching for the word banned above: the separation
  *is* the quality, and naming it weakens it.
- **The issue is raw material, not copy.** Issues are written for engineers. Re-derive the
  story rather than pasting it, and let internal identifiers appear only if the note
  explains them, which usually means not at all.

## Figures

Optional, and worth adding only when a diagram explains something the prose cannot. A
sibling `<note-stem>_figure.py` defines `figure()` returning a `reportlab` `Drawing` about
468 points wide, plus a `CAPTION` string; extra figures are `figure_<name>()` with
`CAPTION_<NAME>`. The Markdown places them with `[[FIGURE]]` and `[[FIGURE:<name>]]`. Copy
the palette from the top of `build_shopnote_pdf.py` so every note looks like the same
publication.

## Before you call one finished

⚠️ **Look at the built PDF, page by page.** The builder places flowables one at a time, so
it will happily leave a single orphaned line at the top of a page or a figure crowding its
caption. Neither shows up in the Markdown. Check the page count, read every page, and fix
what you find by tightening the prose rather than by nudging the layout.
