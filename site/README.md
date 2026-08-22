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

- **No external requests.** No scripts, no trackers, no analytics, no cookie
  banner. ⚠️ **This is a promise about *other hosts*, not about file count** —
  self-hosted `.woff2` files under `site/fonts/` would keep it intact, because
  they are served from the same origin. A Google Fonts link would break it. Type
  is system-only *today* only because the licensed faces have not been bought
  yet; see **The look**.
- **Single theme.** It does not follow the reader's operating system. Every
  colour is painted explicitly, the way a publication has a canonical appearance.
- **Hand-authored HTML is the document.** There is no markdown source and no
  generator. A renderer would only stand between the writing and the design, and
  the plates need design control a renderer cannot give.

## The look

**Same outfit as organonmind.org, not the same paper.** The publications are
stark white with system sans; this page is warm bone (`#faf9f7`, never
`#ffffff`), warm near-black ink, a serif for headings *and* body, and cool
blue-grey for every hairline, panel border and small mono label. A faint dotted
field sits behind it all — one CSS gradient, no asset. What carries the family
resemblance is the *structure*: the tracked-monospace wordmark, the labelled
meta block, the measure, the colophon.

⚠️ **The two temperatures are deliberate and must not be "fixed".** The paper and
its chrome are cool blue-grey; the plates are warm graphite. Tinting the plates
to match the page would break the PRD's §5.4, which says the shell is
*"near-black with a hint of brown, never blue-black"* — the plates depict the
application, so they carry the application's colour, not the website's.

🚨 **Both font stacks lead with faces this repository does not ship**, and this
is a licensing state rather than an oversight. `Plantin MT Pro` needs a Monotype
**web** licence — a desktop licence does not permit `@font-face` — and
`Commit Mono` needs its business licence. Until the `.woff2` files are in
`site/fonts/` there are no `@font-face` blocks at all, so nothing 404s and every
reader falls through the stack behind them. Adding the files plus two
`@font-face` blocks is the whole change; nothing else moves.

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
