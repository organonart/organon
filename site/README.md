# site/ — organon.art

**The page for `organon.art`, and the front door to this repository.** One file,
hand-authored, no build step and no external requests. Same letterhead lineage as
[`organonmind.org`](https://organonmind.org), whose source is in
[`organonart/organon-mind`](https://github.com/organonart/organon-mind) — the two
sites are meant to read as one outfit on two kinds of paper.

```
index.html    the page
favicon.svg   the mark
vercel.json   cleanUrls, so a future /docs serves docs.html
```

## What it is for

**Not advertising.** It exists to say briefly and accurately what Organon is, and
then to hand the reader to the repository, which is where the real material is.
Every section ends in a link out. There is nothing to buy, nothing to sign up
for, and — until there is a release — nothing to download.

⚠️ **`doc/organon_prd.md` §1.1 is canonical for the identity claim and this page
quotes it rather than re-authoring it.** The hero is the one-or-two-sentence
version verbatim. That rule is why the wording is what it is: the claim was once
spelled six different ways across the tree, which is how it came to be stale in
six places at once, and a site is the surface most likely to become the seventh.
If §1.1 changes, this page changes in the same breath.

🚨 **The *State of play* section is §12 of the same document**, and it is on the
page rather than behind a link on purpose. The sections above it describe what
Organon *is*; §12 is what it currently *does*, and the difference between an
enforced principle and an intended one is the whole of whether the rest is worth
reading. A public page that quotes §1.1 without §12 is the exact failure §12's
own header warns about.

## House rules, inherited from organonmind.org

- **No external requests.** No fonts, no scripts, no trackers, no analytics, no
  cookie banner. System type only. The page is one file and it works offline.
- **Single theme.** It does not follow the reader's operating system. Every
  colour is painted explicitly, the way a publication has a canonical appearance.
- **Hand-authored HTML is the document.** There is no markdown source and no
  generator. A renderer would only stand between the writing and the design, and
  the plates need design control a renderer cannot give.

## The plates

The dark panes down the right-hand side are **pictures of Organon, not live
things**. The paper half of the page is shared with the publications; the plates
are the product's own visual identity — warm graphite, taupe hairlines,
bone-white type, per the PRD's §5.4.

⚠️ **Every command and every message in a plate is a real word from the real
vocabulary** — the twelve region words, the four content words (`agent`,
`panel`, `3d`, `off`), the working-directory report, and the last-agent refusal
quoted from `region.rs`. A mock that invents its own spelling teaches a reader
something the program will refuse, and this page is read by people about to type
into a terminal. When the vocabulary changes, the plates are wrong and must be
corrected like any other statement of fact.

## Deploy

Static, no build step. Deployed from this directory as the project root.
`vercel.json` sets `cleanUrls`, which does nothing today and is the one line that
makes a second page cost nothing.

## Not here yet

- **Releases.** There are none. The page says so in the meta block and again in
  §Getting it, and both must be updated in the same change as the first tag —
  a site claiming "build from source" beside a downloads page is worse than
  either alone.
- **Documentation.** A `/docs` in the shape of [pi.dev/docs](https://pi.dev/docs)
  is wanted and is not built. Until it is, *Read next* links into the repository
  docs, which are maintained in the same change as the code they describe and are
  therefore the honest place to send someone.
