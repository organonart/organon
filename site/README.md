# site/ — organon.art

**The page for `organon.art`, and the front door to this repository.** One file,
hand-authored, no build step and no external requests. Sibling to
[`organonmind.org`](https://organonmind.org), whose source is in
[`organonart/organon-mind`](https://github.com/organonart/organon-mind) — the two
sites share a structure and deliberately not a surface; see **The look**.

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
quotes it rather than re-authoring it.** It appears **verbatim** under *The
definition, in the words the repository uses* in §The role. The claim was once
spelled six different ways across the tree, which is how it came to be stale in
six places at once, and a site is the surface most likely to become the seventh.
If §1.1 changes, this page changes in the same breath.

📌 **The headline is deliberately NOT §1.1**, and the reason is worth keeping
because the first cut got it wrong. §1.1 is a *definition* — accurate, canonical,
and the wrong thing for a front page to open with; leading with it made the page
read as a specification rather than an idea. The headline is what Organon is
*for*, the standfirst says what it replaces, and the definition arrives one
screen down where a reader who wants it will look. Quoting §1.1 is an obligation
about **wording**, not about **position**.

🚨 **The *State of play* section is §12 of the same document**, and it is on the
page rather than behind a link on purpose. The sections above it describe what
Organon *is*; §12 is what it currently *does*, and the difference between an
enforced principle and an intended one is the whole of whether the rest is worth
reading. A public page that quotes §1.1 without §12 is the exact failure §12's
own header warns about.

## House rules, inherited from organonmind.org

- **No external requests.** No scripts, no trackers, no analytics, no cookie
  banner. ⚠️ **This is a promise about *other hosts*, not about file count** —
  self-hosted `.woff2` files under `site/fonts/` would keep it intact, because
  they are served from the same origin. A Google Fonts link would break it. Type
  is system-only *today* because the OFL faces have not been added to the tree
  yet, not because anything needs buying; see **The look**.
- **Single theme.** It does not follow the reader's operating system. Every
  colour is painted explicitly, the way a publication has a canonical appearance.
- **Hand-authored HTML is the document.** There is no markdown source and no
  generator. A renderer would only stand between the writing and the design, and
  the plates need design control a renderer cannot give.

## The look

**Dark, warm, and derived rather than picked.** pi.dev's ground is `#161d27` —
HSV(215°, 44%, 15%). Organon's mark is `#d9c7a0`, hue 41°. Rotating **the hue
alone**, keeping Pi's saturation and value, gives **`#272216`**, and that is the
origin of this palette.

📌 It landed on a colour the product had already specified rather than near one:
PRD §5.4 asks for a shell that is *"near-black with a hint of brown, never
blue-black"*. The derivation and the identity doc agree, which is why the ground
is warm rather than merely not-blue.

⚠️ **Every token is the same hue, 41°, varying only in saturation and value —
including the link colour**, which is Pi's pale blue accent rotated by the same
amount and comes out as the favicon's taupe. Do not introduce a second hue.

⚠️ **This page is dark and `organonmind.org` is stark white, and that divergence
is a decision rather than drift.** What carries the family resemblance is the
*structure* — the tracked-monospace wordmark, the labelled meta block, the
measure, the colophon, the refusal to advertise. The surface is allowed to
differ: one of these sites is a publication, the other is a front door to an
application whose own shell is dark. Do not "restore" a light ground for
consistency; the consistency lives one level up.

⚠️ **The plates are darker than the page, not lighter.** On a dark ground a
terminal is an inset well you look into, not a card sitting on top — the
inversion is deliberate, and it is what stops the plates dissolving into the
page now that both are dark.

🚨 **Both font stacks lead with faces this repository does not ship**, and there
are no `@font-face` blocks at all, so nothing 404s and every reader falls through
to the stack behind them. `Commit Mono` is **SIL Open Font License 1.1** — read
out of the file's own name table, so nothing needs buying — and the serif aims at
**Source Serif 4**, also OFL. ⚠️ Shipping either means also shipping the OFL text
alongside it and adding a line to the repository's `NOTICE`; the licence requires
it to travel with the files.

## The plates

The dark panes down the right-hand side are **pictures of Organon, not live
things** — warm graphite, taupe hairlines, bone-white type, per the PRD's §5.4.

⚠️ **Every command and every message in a plate is a real word from the real
vocabulary** — the twelve region words, the four content words (`agent`,
`panel`, `3d`, `off`), the working-directory report, and the last-agent refusal
quoted from `region.rs`. A mock that invents its own spelling teaches a reader
something the program will refuse, and this page is read by people about to type
into a terminal. When the vocabulary changes, the plates are wrong and must be
corrected like any other statement of fact.

## Deploy

Static, no build step. Vercel, team **Organon**, project **`organon`** — the same
project that has always held `organon.art`, repointed from `organonart/organon-private`
to this repository on 2026-08-22. Repointing rather than creating a project is why
there was no DNS change: `organon.art` and `www.organon.art` never left the project
they were attached to.

| Setting | Value |
|---|---|
| Production branch | `main` |
| Root Directory | `site` |
| Framework Preset | Other |
| Build / Output / Install | all empty |

⚠️ **Root Directory is `site`, so `vercel.json` must stay in this directory** — Vercel
reads it from the root directory, not from the repository root. Moving it up one level
silently stops `cleanUrls` from applying, which nothing today would reveal, because
there is only one page and it is `index.html`. The first `/docs` link would be the thing
that broke.

🚨 **These live in the Vercel dashboard, not in this repository, so this table is a
mirror and the dashboard is authoritative.** It is written down because the settings are
invisible from here and a wrong one fails in a way that looks like a broken page — a
build command left over from the previous repository, for instance, fails on the first
push against a directory with no `package.json`.

**The Ignored Build Step is the one worth having and the one to check first:**

```
git diff --quiet HEAD^ HEAD ./
```

Exit 0 skips the build, exit 1 proceeds, and `./` is the root directory — so a deployment
happens only when something in `site/` actually changed. This repository takes many
commits a day and almost none of them are the site. ⚠️ The consequence to remember is the
inverse: **a change made anywhere else cannot deploy this page.** If a future `/docs` is
generated from something outside `site/`, that generator's output has to land in here or
the site will not rebuild — and the symptom is a deployment that never fires rather than
one that fails.

`cleanUrls` does nothing today and is the one line that makes a second page cost nothing.

## Not here yet

- **Releases.** There are none. The page says so in the meta block and again in
  §Getting it, and both must be updated in the same change as the first tag —
  a site claiming "build from source" beside a downloads page is worse than
  either alone.
- **Documentation.** A `/docs` in the shape of [pi.dev/docs](https://pi.dev/docs)
  is wanted and is not built. Until it is, *Read next* links into the repository
  docs, which are maintained in the same change as the code they describe and are
  therefore the honest place to send someone.
