### Changelog entries move to `changelog.d/` fragments

`CHANGELOG.md` had **one shared insertion point** — a `## Unreleased` block that every PR
added a section to the top of — so any two open branches conflicted there by construction,
whether or not they touched a single common concern. New entries are now one Markdown file
each in `changelog.d/`, named `YYYY-MM-DD-<branch-slug>.md`, concatenated into
`CHANGELOG.md` at release time by `native/tools/changelog.py`. Two branches write two
different files, and two different files do not conflict.

🚨 **The fact that decided it: GitHub ignores `.gitattributes` merge drivers.** The
previous answer was `CHANGELOG.md merge=union`, and it works — for `git`. Measured
2026-08-14: merging `main` into a conflicting PR branch locally reported *"Automatic merge
went well"*, no markers. GitHub computes PR mergeability with its own three-way merge that
does not consult `.gitattributes`, so the same PR still displayed `CONFLICTING`, and
someone still had to merge `main` locally and push purely to make the page agree with what
git already thought. That happened on **four separate branches in one day** — the same
count as the day before union landed. Union bought nothing at the boundary where the cost
is actually paid.

⚠️ **The rejected alternative, recorded so it is not re-proposed:** an Action that
auto-resolves the conflict and pushes to PR branches. Wrong here, because this repo
routinely has many agent worktrees live at once — it would push to a branch someone is
mid-commit on. The directory needs no automation to be correct: there is no hunk left for
anything to resolve.

**A fragment is exactly what would have gone under `## Unreleased`** — one or more `### `
sections in the house style, full paragraphs with 🚨/⚠️/📌 and code fences where they earn
their place. No frontmatter, no metadata fields. That is deliberate: a form-shaped fragment
would push everyone toward one-line bullets, and trading a merge tax for a documentation
loss would be a bad deal.

**The name comes from the branch**, which git guarantees is unique among the branches that
exist at any one moment. ⚠️ Sequential numbers would be exactly the wrong answer — two
branches both reach for `0042` and the conflict is back somewhere new. ⚠️ The residual
case is stated rather than hidden: two branch names that *slugify* to the same string
(`feat/foo` and `feat-foo`) on the same day would collide, surfacing as a loud add/add
conflict; `--slug` is the escape hatch, and `slugify`'s non-injectivity is pinned by a test
so a later change cannot quietly claim to have fixed it.

**Ordering is filename descending** — the name starts with an ISO-8601 date, so newest-day
first, matching the file's existing newest-first convention; within one day the tiebreak is
reverse-alphabetical by slug, arbitrary but deterministic, which "filesystem order" is not.

📌 **`## Unreleased` stays in `CHANGELOG.md` permanently, and that is what handles the
transition.** It is the release step's *second input*: anything that still lands there — a
long-lived branch written before this change, a contributor on an older checkout, a merge
that resolved a hunk into it — is folded into the next release below the fragments, because
it was written before them. So the boundary is a mechanism rather than a date, and the
`## Unreleased` entries on PRs open right now cannot be orphaned. `merge=union` stays in
`.gitattributes` as the belt for exactly those in-flight entries, and its block now records
that union proved insufficient and why.

**`CHANGELOG.md` itself is untouched as a record** — no history rewritten, no sections
restructured. `changelog.d/` starts empty and applies from here on.

**The tool is Python in `native/tools/`, beside `crate-churn.py`, not an `xtask`
subcommand.** It is prose plumbing: it has no business adding a compile to anyone's
verification bar, and `xtask`'s `main` is a two-line passthrough to `nih_plug_xtask::main()`
that would have to be forked to host it. `python3 native/tools/test_changelog.py` runs 40
stdlib `unittest` cases with no dependencies and no cargo.

🚨 **The bar those tests aim at is "can a fragment go missing", not "does the happy path
work".** The step being replaced is a merge conflict — loud, annoying, impossible to miss.
A concatenation that silently drops an entry would be strictly worse. So a file in
`changelog.d/` is either a valid fragment, or `README.md`, or an **error**; `release` runs
`check` first and refuses to write or delete anything if it finds one; and the malformed
cases assert that `CHANGELOG.md` is byte-identical afterwards and the valid fragments are
still on disk, rather than merely that the tool complained.
