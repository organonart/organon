# site/ — organon.art

**The pages for `organon.art`, and the front door to this repository.** Two files,
hand-authored, no build step and no external requests. Sibling to
[`organonmind.org`](https://organonmind.org), whose source is in
[`organonart/organon-mind`](https://github.com/organonart/organon-mind) — the two
sites share a structure and deliberately not a surface; see **The look**.

```
index.html    the landing page — what Organon is
docs.html     /docs — how to build it, run it and operate it
og.html       SOURCE of the link-preview card — not a page anyone visits
og.png        the card itself, 2400x1260, referenced by og:image
endcard.html  SOURCE of the video end card — not a page anyone visits
endcard.png   the frame itself, 3840x2160, for the end of a video
favicon.svg   the mark
vercel.json   cleanUrls, which is what serves docs.html at /docs
```

## The link-preview card

`og.png` is what appears when the URL is pasted into Slack, X, Discord or
iMessage. Without it a paste renders as bare text, which is what prompted it.

It is **rendered from `og.html`, never drawn by hand**, so a change to the
headline or the palette is a diff rather than an image someone has to reproduce:

```
chrome --headless=new --hide-scrollbars --force-device-scale-factor=2 \
       --window-size=1200,630 --screenshot=site/og.png site/og.html
```

There is no image toolchain in this repository and none is wanted — headless
Chrome is already on any machine that can look at the site, and it renders with
real fonts, which an SVG converter would not.

⚠️ **`og.html` duplicates the palette rather than sharing it.** An `og:image` is a
flat file: it cannot read a custom property at paste time. **If the palette moves,
re-render — the card will not follow on its own and nothing will tell you.**

📌 **It is not the only rendered file that copies the palette** — `endcard.html`
does too, for the same reason, and `docs.html` copies the token block itself (see
**`/docs`**). The claim worth remembering is narrower than a count: *anything the
site renders to a flat file has its own copy of these colours, and re-renders on
its own schedule.* When the theme moves, re-render every one of them.

⚠️ **Three things about the tags that fail silently if you get them wrong.** The
URL must be **absolute** (a relative path is ignored by most scrapers); the image
must be a **raster** (no scraper renders SVG, which is what the favicon is); and
`og:image:width`/`height` must state the **pixels of the file** — 2400×1260 — not
the 1200×630 the card is laid out at. `twitter:card` is `summary_large_image`,
which is what makes it render wide rather than as a thumbnail.

📌 A paste of `organon.art` is served after a redirect to `www.organon.art`. Every
major scraper follows it, so the tags name the apex — which is also what stays
correct if the primary domain is ever flipped.

## The video end card

`endcard.png` is the still a video cuts to before it fades out — the claim, the
address and an invitation, held over music for a few seconds. **Nothing links to
it and nobody is meant to visit it** — the same standing as `og.html`. It lives
here because it is made of this site's motifs, and the place to keep it correct
is next to them.

⚠️ **"Nobody is meant to visit it" is not the same as "it is not served",** and
the first draft of this section said the second. Every file in this directory is
deployed: `vercel.json` carries no rewrites and no excludes, `cleanUrls` puts the
page at `/endcard`, and the frame at `/endcard.png` — which is the same mechanism
that serves `og.png` to every scraper. So it is a **public URL that is simply
unadvertised**, and anything written on it is published whether or not a link
points at it.

It is a **sibling of `og.html`, not an extension of it** — same ground, same dot
grid, same italic serif claim, same single teal cursor. Three things differ, and
each follows from the medium rather than from taste:

- **16:9**, because a video frame is and an `og:image` is not.
- **The address is set large and in bone**, where the card sets it small and in
  titanium. On a link card `organon.art` is a footnote; on a screen somebody is
  watching, it is the thing they are meant to type.
- **It carries the repository and an invitation** — `github.com/organonart/organon`
  and the colophon's own *come to the bench* — which the card has no room for.

Rendered the same way, at 2× for headroom:

```
chrome --headless=new --hide-scrollbars --force-device-scale-factor=2 \
       --window-size=1920,1080 --screenshot=site/endcard.png site/endcard.html
```

⚠️ **Nothing in the foot is set below 19px, and that floor is not taste.** A
video frame is re-encoded before anybody sees it; at 720p every length is
multiplied by 0.67, and tracked uppercase at 16px did not survive that. The link
card can afford 15px because it is served as pixels and never re-encoded.

🚨 **This is the site's SECOND copy of the palette**, on exactly the same terms as
`og.html`'s: a rendered frame is a flat file and cannot read a custom property at
playback time. If the palette moves, re-render both — neither follows on its own
and nothing will tell you.

⚠️ **Type is system-only here too**, so the committed `endcard.png` was rendered
one step down both stacks — Bitstream Charter for the serif, not Source Serif 4.
A machine with the named faces installed produces a *better* frame and a
*different* one, so render every cut of a given video on the same machine.

## What it is for

**Not advertising.** `index.html` exists to say briefly and accurately what
Organon is; `docs.html` exists to get someone from a clone to a running window
and then out of their way. Both hand the reader to the repository, which is where
the real material is. Every section ends in a link out. There is nothing to buy,
nothing to sign up for, and — until there is a release — nothing to download.

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

**The palette is the application's, copied rather than resembled.** Every value
comes from `native/src/theme_config.rs`'s shipped blue-slate `Palette::default()`,
and the CSS custom properties carry that file's own surface names:

| Token | From | |
|---|---|---|
| `--paper` | `shell` | `#192228` — the application body behind everything |
| `--panel` | `panel` / `card` | `#212a30` |
| `--panel-hi` | `raised` | `#273139` |
| `--raise` | `well` | `#141c22` — recessed: code, inputs |
| `--ink` | `bone` | `#d1d6d9` — **never pure white** |
| `--muted` / `--faint` | `titanium` / `muted` | `#8e999f` / `#606b72` |
| `--rule` / `--rule-strong` | `hairline` / `edge_strong` | `#3a464f` / `#52616b` |
| `--pane*` | `well_deep`, `well`, `card_header` | the plates |

⚠️ **This is a copy, so if the theme moves it is wrong.** The trade is deliberate:
it drifts *detectably*, because every value is named after the token it came from
and a diff against `Palette::default()` is mechanical. A palette that merely
rhymed with the product would drift silently.

🚨 **Do not re-warm it.** PRD §5.4 still specifies a warm graphite shell *"never
blue-black"*; the shipped theme moved to blue-slate and `theme.rs` says so in its
own comments. The code is what ships, so this follows the code — **§5.4 is the
thing that needs correcting.**

📌 **Chroma is rationed, by the theme's own rule**: *"strong chroma belongs to
data, selection, and status, and every saturated pixel spent on chrome devalues
the data."* Small mono labels are grey, not teal. ⚠️ **Teal and amber are not
declared as page-level tokens at all** — they reach the page only through
`--pane-accent` and `--pane-warn`, which keeps the rule structural rather than
remembered. Teal appears in exactly one place: the provenance plate, which is what
teal is for in the application.

⚠️ **This page is dark and `organonmind.org` is stark white, and that divergence
is a decision rather than drift.** The family resemblance is carried by the
*structure* — the tracked-monospace wordmark, the labelled meta block, the
measure, the colophon, the refusal to advertise. One site is a publication; the
other is the front door to an application whose own shell is dark. Do not
"restore" a light ground for consistency; the consistency lives one level up.

🚨 **Both font stacks lead with faces this repository does not ship**, and there
are no `@font-face` blocks at all, so nothing 404s and every reader falls through
to the stack behind them. `Commit Mono` is **SIL Open Font License 1.1** — read
out of the file's own name table, so nothing needs buying — and the serif aims at
**Source Serif 4**, also OFL. ⚠️ Shipping either means also shipping the OFL text
alongside it and adding a line to the repository's `NOTICE`; the licence requires
it to travel with the files.

## The plates

The dark panes — down the right-hand side of `index.html`, in the flow of a
section on `docs.html` — are **pictures of Organon, not live things**: warm
graphite, taupe hairlines, bone-white type, per the PRD's §5.4.

⚠️ **Every command and every message in a plate is a real word from the real
vocabulary** — the twelve region words, the four content words (`agent`,
`panel`, `3d`, `off`), the working-directory report, and the last-agent refusal
quoted from `region.rs`. A mock that invents its own spelling teaches a reader
something the program will refuse, and this page is read by people about to type
into a terminal. When the vocabulary changes, the plates are wrong and must be
corrected like any other statement of fact.

## `/docs`

`docs.html` is in the shape of [pi.dev/docs](https://pi.dev/docs): a persistent
left rail, dense reference prose in the centre, and a section you can link
someone straight to. Every section carries an `id`, a `#` permalink beside its
heading, and a `:target` mark so arriving from a deep link lands somewhere that
says so — and **every section appears in the rail exactly once**, which is the
invariant to re-check after adding one:

```bash
python3 - <<'EOF'
import re, pathlib
s = pathlib.Path('site/docs.html').read_text(encoding='utf-8')
start = s.index('<nav class="rail"')            # NOT s.index('</nav>') for the end:
end   = s.index('</nav>', start)                # the masthead nav closes first
ids   = re.findall(r'<section id="([a-z-]+)"', s)
rail  = re.findall(r'href="#([a-z-]+)"', s[start:end])
print('sections not in the rail:', [i for i in ids if i not in rail])
print('rail entries with no section:', [r for r in rail if r not in ids])
EOF
```

Both lists print empty. ⚠️ On Windows `python3` is the Microsoft Store stub —
prefix `wsl.exe -e`, the same wrinkle `CLAUDE.md` records for the hooks.

🚨 **It is a guided index, not a second copy of the documentation.** Every
section closes with an `owns` line naming the file that owns its subject, and
that file is the one to believe. The page states a fact only where a reader
needs it to walk from a clone to a running window; everything exhaustive is a
link. Three concrete rules follow, and they are the whole discipline:

- ⚠️ **`doc/reference/` is never reproduced here.** It is generated by
  `organon docs` from prose in `agent.rs` and `recipe.rs`, and a test
  (`generated_reference_is_current`) fails the build when a checked-in page
  drifts from the code. A copy of those tables on this page would be a copy
  **nothing pins** — right on the day it was written, wrong the first time a
  description changed, with no test anywhere to notice. `#reference` links out
  and says why in as many words.
- ⚠️ **State of play is on `index.html` and stays there.** `docs.html`'s `#state`
  points at `/#state` rather than restating the table, for the same reason: the
  difference between an enforced principle and an intended one is exactly the
  thing that must not go stale in a second copy. 🚨 **An early draft quoted two
  of its lines and was wrong within a day** — it said hosted modules had nowhere
  to draw, which was true of §12 and false of the tree by the time it was
  written. A *partial* copy is the worst of the three options: it goes stale,
  reads as authoritative, and cannot be corrected by the change that invalidated
  it, because nothing links the two.
- ⚠️ **Every command word is real, on the plates rule above** — and the docs page
  spends far more of them than the landing page does. It carries the twelve
  region words with their short forms, the four content words, the `organon`
  verbs and the `organon console` verbs with their value lists. Those come from
  `region.rs` (`REGION_WORDS`, `REGION_ALIASES`, `CONTENT_WORDS`) and from
  `native/src/bin/ctl.rs`, which is where `--help` comes from. When a vocabulary
  grows a word, this page is wrong.

⚠️ **The `:root` token block is duplicated from `index.html`, deliberately and
with a cost.** The site has no build step, so a shared stylesheet would mean a
second request — the tokens are copied instead, and the two must be edited
together. `index.html` owns them. Only `--measure` differs, and it differs on
purpose: 34rem is a landing page's measure, and at 34rem this page's tables wrap
into columns two words wide. The check is one line and should print nothing:

```bash
diff <(grep -oE -- '--[a-z-]+:[^;]+;' site/index.html | grep -v '^--measure') \
     <(grep -oE -- '--[a-z-]+:[^;]+;' site/docs.html  | grep -v '^--measure')
```

⚠️ **It compares DECLARATIONS, not the `:root` block, and that is not fussiness.**
The obvious spelling — `sed -n '/^:root{/,/^}/p'` — was what this file carried
first, and the warm-paper change broke it the same day by adding multi-line
comments inside `:root`: the opening `/*` filters out, the continuation lines do
not, and the check then reports a difference that is entirely prose. A check that
cries wolf on a comment gets ignored, and the next real drift goes with it.

📌 **The heading SCALE differs from `index.html` and the type SYSTEM does not.**
That page carries seven sections and sets `h2` as display type at 1.75rem; this
one carries twenty-nine and would read as a stack of titles. Same serif, same
italic, same weight, same colours, same mono chrome — smaller. The system is the
part that has to match; a landing page and a reference page wanting different
sizes of the same face is not drift.

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
silently stops `cleanUrls` from applying. ✏️ **When this was written there was one page
and nothing would have revealed it; `/docs` is now that reveal.** `index.html` still
answers at `/` either way, so the failure is not a broken site — it is `/docs` alone
returning 404 while the landing page looks perfect, which is a worse-shaped bug than the
one this paragraph was written to warn about.

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
inverse: **a change made anywhere else cannot deploy these pages.** `docs.html` is
hand-authored in this directory, so it satisfies that by construction — but it *describes*
things that live elsewhere, and editing `region.rs` or `ctl.rs` deploys nothing. A page
gone stale against the code will keep serving happily, because from Vercel's side nothing
changed. That is what the §/docs rule below is for.

`cleanUrls` is what makes `docs.html` answer at `/docs`. It cost nothing to set
before there was a second page, and it is now load-bearing: without it `/docs`
404s and only `/docs.html` works, which is not what anything links to.

## Not here yet

- **Releases.** There are none. The page says so in the meta block and again in
  §Getting it, and both must be updated in the same change as the first tag —
  a site claiming "build from source" beside a downloads page is worse than
  either alone.
- **A search field, and a highlighted "you are here" in the rail.** Both are
  ordinary on a documentation site and both need JavaScript, which this one does
  not have. `:target` gives the anchored section its own mark instead, which is
  the part a reader arriving from a deep link actually needs. If `/docs` ever
  grows past one page, that trade is worth re-opening — with James, because it
  is the no-external-requests promise being spent, not a styling choice.
