# site/ — organon.art

**The pages for `organon.art`, and the front door to this repository.** Two files,
hand-authored, no build step and no external requests. Sibling to
[`organonmind.org`](https://organonmind.org), whose source is in
[`organonart/organon-mind`](https://github.com/organonart/organon-mind) — the two
sites share a structure and deliberately not a surface; see **The look**.

```
index.html    the landing page — what Organon is
docs.html     /docs — how to build it, run it and operate it
favicon.svg   the mark
vercel.json   cleanUrls, which is what serves docs.html at /docs
```

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
