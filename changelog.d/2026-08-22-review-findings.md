### Three review findings that were live on `main`, and the one that keeps recurring

All three came from the automated reviewer on PRs that had already merged, which is the first thing
worth recording: a finding on a merged PR is not closed by the merge, and nothing chases it.

🚨 **`docs.html`'s uppercase labels were still on `--faint`** — `#606b72` on `#192228`, about
**2.95:1**, failing AA for text at any size. This is the *same defect*, in the *same selectors*,
that was already found and fixed in `index.html`: `.part`, `th`, `.owns`, the rail's group headings
and `pre .c` are labels, and the theme calls labels `titanium`. The fix was made on one page and
never ported to the other, which is exactly what happens when two pages are written by two sessions
and only one of them is being looked at.

🚨 **The two figures in `index.html` had the same defect in a form no CSS audit would catch.** Both
diagrams were converted from hardcoded light-design hex to tokens when the page went dark, and the
conversion reached for `--faint` for every label — LEFT/CENTER/RIGHT, CLI/HUMAN/AGENT/REGION,
"ONE DISPATCH", the connector strokes and the caption. Eight `style="fill:var(--faint)"` /
`style="stroke:var(--faint)"` attributes, all of them text or lines a reader has to see, none of
them reachable by grepping a stylesheet for a selector. They are `--muted` now.

📌 **`--faint` is now used in exactly three decorative places across both pages** — `li::marker`,
the `$` prompt glyph, and `docs.html`'s `/` wordmark separator — and the separator carries a
comment saying it is a deliberate call rather than an oversight, because it is the one visible
thing left on that token and it will otherwise be re-flagged forever. A separator that competes
with the two words it divides is worse typography, and the token's own definition says
*decoration only*.

✏️ **`site/README.md`'s re-render command had lost its line continuation**, collapsing a
`\`-continued two-line invocation into one line with the backslash swallowed — so a copy-paste
would have run a command with a stray token in it. It matches `og.html`'s header and the changelog
fragment again.

⚠️ **The pattern across all four is one thing: a fix applied to the artifact that was on screen,
and not to its twin.** `index.html` got the contrast fix and `docs.html` did not; the stylesheet
got it and the inline SVG did not; the command was correct in two places and wrong in the third.
Every one was caught by a reviewer reading the diff rather than by anything mechanical, and the
only structural defence available — `site/README.md` already carries the one-line diff that pins
the two token blocks byte-identical — does not extend to usage.
