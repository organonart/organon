# How to record a change

**One file per change, here. Not in `CHANGELOG.md`.**

```bash
python3 native/tools/changelog.py new "What changed, as a heading"
```

That prints the path it made — `changelog.d/YYYY-MM-DD-<your-branch>.md`, seeded with a
`### ` heading. Write the entry into it and commit it with the rest of your change.

## What goes in one

**Exactly what would have gone under `## Unreleased`** — one or more `### ` sections in
this project's house style: full paragraphs that explain the *why* and the trap, with
🚨 / ⚠️ / 📌 where they earn their place, code fences where an example is clearer than a
sentence. No frontmatter. No metadata. No one-line bullets.

That last point is the reason the scheme is shaped this way. A fragment format with fields
to fill in would push everyone toward terse summaries, and this changelog's density is the
part of it worth keeping — trading a merge tax for a documentation loss would be a bad
deal. A fragment is just the prose, in a file of its own.

Validate before you push:

```bash
python3 native/tools/changelog.py check
```

It rejects an empty fragment, a fragment that does not open with `### `, a `## ` heading
(the release step owns those), an odd number of code fences, and any file in here whose
name is not `YYYY-MM-DD-<lowercase-slug>.md`. A mis-named file is an **error**, never a
skip — a concatenation step that silently drops an entry would be worse than the merge
conflict this replaces.

## Why a directory

`CHANGELOG.md` has one shared insertion point, so any two open branches conflicted at the
top of `## Unreleased` by construction — whether or not they touched a single common
concern. `.gitattributes` answered that with `merge=union`, which works for `git` and
**not for GitHub**: GitHub computes PR mergeability with its own three-way merge that
ignores `.gitattributes` merge drivers, so a PR reads `CONFLICTING` while `git` resolves
it silently. On 2026-08-14 that cost four hand-merges on four branches in one day, and the
resolution was "keep both sides" every time.

Two branches writing two different files do not conflict. That is the whole mechanism —
no Action, no merge driver, nothing to configure in a fresh clone.

`.gitattributes`' `CHANGELOG.md` block carries the measurement and the argument;
`native/tools/changelog.py`'s module docstring carries the design, including the
alternative that was rejected (an auto-resolving Action would race the agent worktrees
this repo routinely has live at once).

## The filename

`YYYY-MM-DD-<slug>.md`, where the slug comes from your branch name. Git guarantees branch
names are unique among the branches that exist at any moment, so two live branches cannot
choose the same filename. Sequential numbers would be exactly the wrong answer: two
branches both reach for `0042` and the conflict is back somewhere new.

⚠️ **The residual case:** two branch names that *slugify* to the same string (`feat/foo`
and `feat-foo`) on the same day would collide, and `new` cannot see the other worktree to
prevent it. It surfaces as an add/add conflict at merge — loud, not silent, and far rarer
than the by-construction conflict it replaces. Pass `--slug` when you know you are in that
case, or when a human-meaningful name reads better than a branch name.

Several changes on one branch on one day get `-2`, `-3` automatically. That loop only ever
sees your own worktree, which is right: it is resolving a collision with yourself, where
there is no race.

## Where the boundary falls

**`CHANGELOG.md` is the record and is not being rewritten.** Everything already in it,
including whatever currently sits under `## Unreleased`, stays exactly where it is. This
directory starts empty and applies to changes from here on.

**`## Unreleased` stays in `CHANGELOG.md` permanently**, and its body is normally empty.
It is the release step's *second input*: anything that still lands there — a long-lived
branch written before this changed, a contributor on an older checkout, a merge that
resolved a hunk into it — is folded into the next release rather than being left behind.
So the transition needs no cut-off date, and an open PR carrying an `## Unreleased` entry
cannot be orphaned by this change. `merge=union` stays in `.gitattributes` as the belt for
exactly those in-flight entries.

## At release time

```bash
python3 native/tools/changelog.py render          # preview, writes nothing
python3 native/tools/changelog.py release 0.1.0   # rewrite CHANGELOG.md, clear this directory
```

`release` runs `check` first and refuses to write anything if it finds a problem.

**Order:** fragments are emitted **filename descending**. The name starts with an ISO-8601
date, so that is newest-day-first, matching the newest-first convention `CHANGELOG.md`
already uses; within one day the tiebreak is reverse-alphabetical by slug — arbitrary, but
deterministic and predictable, which "filesystem order" is not. Any prose still under
`## Unreleased` is appended *below* the fragments, because it was written before them.

## Tests

```bash
python3 native/tools/test_changelog.py
```

Stdlib `unittest`, no dependencies and no cargo — this is prose plumbing and has no
business adding a workspace member or a compile to anyone's verification bar.
