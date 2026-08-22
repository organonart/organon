### organon.art gains a `/docs`, and it is an index with opinions rather than a second copy of the documentation

`site/docs.html` — a persistent left rail, dense reference prose in the centre, twenty-nine
deep-linkable sections. Same hand-authored single file, no build step, nothing fetched from
another host as the landing page beside it; `vercel.json` already set `cleanUrls`, so it serves
at `/docs` with no configuration change and the line that cost nothing to add has now earned
itself.

**What it is for is the whole design.** It gets someone from a clone to a running window and
then out of their way: the Linux dev headers and why their absence reads as a code error,
`--workspace` and what leaving it off silently costs, the three binaries and the two
default-off features, the DAW install and its four platform traps, the region vocabulary,
saved layouts, the agent's working directory, the `organon` verbs and which of them need the
editor rather than the visual. Everything past that is a link.

🚨 **Every section closes with an `owns` line naming the file that owns its subject, and the
page says in its own standfirst that the file is the one to believe.** That is not modesty,
it is the only arrangement that survives: the repository's docs are updated in the same
change as the code and hooks say so when they are not, while a page on a website has no such
enforcement and never will. A docs page that positions itself as authoritative is a docs page
that will be quietly wrong, at the surface most likely to be read by someone who cannot check.

⚠️ **`doc/reference/` is deliberately not reproduced, and this is the trap the page was most
likely to fall into.** Those tables are the obvious thing to put on a documentation site —
every generator, every surface, every material, every parameter with its range. They are also
*generated* by `organon docs` from prose in `agent.rs` and `recipe.rs`, and pinned by
`generated_reference_is_current`, which fails the build the moment a checked-in page drifts
from the code. **A copy of them here would be a copy nothing pins** — correct on the day it
was written, wrong the first time a description changed, and with no test anywhere that could
notice. `#reference` links out and says exactly that in as many words, so the next person to
consider pasting a table in reads the argument first.

🚨 **§12 is not restated, and the first draft proved why by getting it wrong within a day.**
That draft quoted two of its lines — that there is no dynamic loading anywhere in the tree, so
hosted modules had nowhere to draw. Then *the picture arrived*: a producer is launched and its
frames are painted into a region. §12 itself has not caught up, which is the argument in
miniature and worse than the plain version of it: **a partial copy of a state-of-play table
goes stale, reads as authoritative, and cannot be corrected by the change that invalidated
it**, because nothing links the two. `#state` now points at `/#state` and at the PRD and says
nothing of its own, and `#modules` describes what the launcher actually does — the binary
derived rather than named by the manifest, the handoff as an environment variable so a
module's own argument parser never learns it exists, and the process handle asked before the
channel because a producer that dies quietly leaves counters that merely stop.

⚠️ **Every command word on the page is a real word**, on the plates rule `site/README.md`
already carried — and this page spends far more of them than the landing page does. The
twelve region words with their short forms come from `REGION_WORDS` and `REGION_ALIASES`, the
four content words from `CONTENT_WORDS`, and the verb tables from `native/src/bin/ctl.rs`,
which is where `--help` comes from. A mock that invents its own spelling teaches a reader
something the program will refuse, and this page is read by people about to type into a
terminal.

📌 **No search field and no highlighted "you are here" in the rail, and both absences are
choices rather than omissions.** Either needs JavaScript, and the site promises none — a
promise made in its own README and repeated in its colophon, so spending it is James's call
and not a styling decision. `:target` gives the anchored section its own mark instead, which
is the part a reader arriving from a deep link actually needs, and `section:target` is pure
CSS. `site/README.md` records the trade under *Not here yet* so re-opening it later starts
from what was decided rather than from scratch.

**The page took the warm-paper-and-serif look while it was being written, not after.** Warm
bone ground, the dotted field, serif headings and body, cool blue-grey chrome, bordered panels
— the rail is one of those panels rather than a bare list on the paper. The plates stay warm,
per the rule that put them there: they depict the application, so they carry the application's
colour and not the website's. ⚠️ **The heading SCALE is denser than `index.html`'s and the type
SYSTEM is identical.** That page carries seven sections and sets `h2` as display type; this one
carries twenty-nine and would read as a stack of titles at that size. Same face, same italic,
same weight, same colours — smaller. The system is the part that has to match.

⚠️ **The `:root` token block is duplicated from `index.html` and the two must be edited
together.** No build step means no shared stylesheet without a second request, so this is a
copy with a cost rather than an oversight; `index.html` owns the values and only `--measure`
differs, because at 34rem this page's tables wrap into columns two words wide. `site/README.md`
carries the one-line check, and **the shape of that check is itself a small lesson**: the
obvious spelling walks the `:root` block with `sed` and diffs it, which is what this file
carried first — and it broke the same day, because the warm-paper change added multi-line
comments inside `:root`. The opening `/*` filters out and the continuation lines do not, so it
reports a difference that is entirely prose. It now compares **declarations**, not the block.
A check that cries wolf on a comment gets ignored, and the next real drift goes with it.

Also on the landing page: `Docs` joins the masthead nav and heads the *Read next* list, since
a page nothing links to is a page nobody reads. And `site/README.md`'s deploy section is
corrected where `/docs` made it stale — the warning about `vercel.json` leaving that directory
was written when nothing would have revealed it, and `/docs` is now that reveal. The failure
shape is worth keeping straight: `index.html` still answers at `/` either way, so a misplaced
`vercel.json` is not a broken site, it is `/docs` alone returning 404 while the landing page
looks perfect.
