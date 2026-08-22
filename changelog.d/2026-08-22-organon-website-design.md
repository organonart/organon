### organon.art moves into this repository, so it can be held to §1.1

`site/` is one hand-authored HTML file, one SVG mark and a four-line `vercel.json`. No build
step, no external requests — no fonts, no scripts, no trackers, no analytics, no cookie banner —
and no npm, which keeps `CLAUDE.md`'s claim about this tree true with one word added to it. It is
the front door to the repository rather than an advertisement: every section ends in a link out,
there is nothing to buy, and until there is a release there is nothing to download. The page says
that in the meta block rather than showing an empty downloads section.

📌 **The reason it is here and not built elsewhere is `doc/organon_prd.md` §1.1.** The identity
claim was once spelled six different ways across the tree, which is how it came to be stale in six
places at once; the 2026-08-21 conversion pointed five of them at the PRD and named the sites as
the surface that had not caught up, because they were outside this repository and nothing here
could reach them. A public page is the surface most likely to become the seventh wording. The hero
is now the one-or-two-sentence version **verbatim**, `README.md`'s ⚠️ is corrected to name one
outstanding surface instead of two, and `organonmind.org` — still built in `organonart/organon-mind`
— is the one that remains outside.

🚨 **§12 is on the page, not behind a link, and that is the decision the rest of the design
follows from.** The sections above it describe what Organon *is*; *State of play* is what it
currently *does*, in three rows: enforced, designed-and-not-built, direction. §12's own header says
to read it before quoting §1.1 anywhere public, so a page that quoted §1.1 without it would be the
exact failure that header exists to prevent — and "no dynamic loading anywhere in the tree" and
"Organon is not yet the dispatcher" are on the public page in those words. Nothing on the page is
unshipped work written in the present tense.

**The plates are the product's own identity, and the paper is the publications'.** The page's white
ground, system sans, tracked-monospace wordmark, meta block and colophon are `organonmind.org`'s,
so the two sites read as one outfit; the dark panes down the right-hand side are the PRD §5.4 shell
— warm graphite, taupe hairlines, bone-white type — because that is what the application looks
like. They are sticky beside the prose, so a section's illustration stays with its argument, with
no JavaScript: the effect is `position:sticky` on the right-hand grid column, and below 62rem the
plate simply falls under the prose.

⚠️ **Every command and message in a plate is a real word from the real vocabulary**, and this is a
maintenance obligation rather than a nicety. The region words, the four content words
(`agent`/`panel`/`3d`/`off`), the four working-directory rules and their report text, and the
last-agent refusal are quoted from `region.rs` and `harness.rs` — including the refusal in full:

```
> /viewport full off
`full` holds the last agent — emptying it would leave the console with
nothing to talk to, and the verb that undoes it is typed at an agent
```

A mock that invents its own spelling teaches a reader something the program will refuse, and this
page is read by people about to type into a terminal. When the vocabulary changes the plates are
wrong, the same way a stale count in a reference is wrong.

✏️ **The provenance plate carries field names and no numbers, deliberately.** An earlier draft
showed `layers 36 · heads 32 · kv-heads 8` under a *measured* marker, which would have been a
fabricated measurement on the page whose subject is that measurements are not fabricated. It now
lists the fields and says the counts are read from the header and never inferred from the file's
name or its size.

**`site/index.html` gains a staleness-only row in `.claude/hooks/doc-rules.sh`, triggered on the
PRD alone.** ⚠️ Not on `native/organon-console/src/*.rs`, even though the plates quote it: that
glob churns most weeks and would fire the rule constantly, which is how a reminder gets dismissed —
the calibration note in the same file is explicit that a reminder people learn to ignore is worse
than no reminder. The vocabulary obligation is carried in `site/README.md` and `CLAUDE.md`'s doc
table instead, where someone changing a region word will meet it. There is no case for the row in
`architecture-doc-check.sh`, so it adds no same-change nudge — the same arrangement as
`web/ARCHITECTURE.md`, for the same reason.

⚠️ **Nobody has looked at the rendered page yet.** The Browser pane was not displayed for this
session, so it was verified by measurement rather than by eye: no horizontal overflow at 1280 or
375 px, sticky resolving on the wide breakpoint and static on the narrow one, every `<text>` inside
its `viewBox`, and no line in a plate wrapping where the two-column alignment depends on it not
wrapping. That is a real bar and it is not the same as having seen it. Typography, rhythm and
whether the plates carry their sections are James's call, and the layout diagram in §Arrange has
never been on a screen.

**Not built, and named rather than implied: a `/docs`.** `vercel.json`'s `cleanUrls` does nothing
today and is the one line that makes a second page cost nothing. Until there is one, *Read next*
links into the repository's own docs, which are maintained in the same change as the code they
describe and are therefore the honest place to send someone.
