#!/usr/bin/env python3
"""`changelog.d/` fragments → `CHANGELOG.md`. One file per change, concatenated at release.

## Why this exists

`CHANGELOG.md` had **one shared insertion point** — a `## Unreleased` block that every PR
added a section to the top of — so any two open branches conflicted there by construction,
whether or not they touched a single common concern. `.gitattributes` answered that with
`CHANGELOG.md merge=union`, and for `git` it worked: merging `main` into a conflicting
branch locally reported "Automatic merge went well", no markers.

🚨 **GitHub does not honour it.** GitHub computes PR mergeability with its own three-way
merge that ignores `.gitattributes` merge drivers, so the same PR displays `CONFLICTING`
while `git` resolves it silently. The tax was per-branch and manual: merge `main` locally
and push, purely to make the PR page agree with what git already thought. On 2026-08-14
that happened on **four separate branches in one day**, and the resolution was "keep both
sides" every single time.

A fragment directory removes the shared insertion point rather than teaching git to cope
with it: two branches write two different files, and two different files do not conflict.
`.gitattributes`' `CHANGELOG.md` block carries the full argument and the measurement.

⚠️ **The rejected alternative is worth stating, because it is the obvious one.** An Action
that auto-resolves the conflict and pushes to PR branches would race the agent worktrees
this repo routinely has live at once — it would push to a branch someone is mid-commit on.
The directory needs no automation to be correct.

## The scheme

    changelog.d/YYYY-MM-DD-<slug>.md

The body of a fragment is **exactly the Markdown that would have gone under
`## Unreleased`** — one or more `### ` sections in the house style, with its markers and
its paragraphs. No frontmatter, no metadata, no one-line-bullet discipline. That is
deliberate: a scheme that pushed people toward terse bullets would trade a merge tax for a
documentation loss, and this file's density is the point of it.

**`<slug>` is derived from the branch name**, which git guarantees is unique among the
branches that exist at any one moment — so two live branches cannot pick the same
filename. Sequential numbers would be exactly wrong here: two branches both reach for
`0042` and the conflict is back in a new place.

⚠️ **The residual case, stated rather than hidden:** two branch names that *slugify* to the
same string (`feat/foo` and `feat-foo`) on the same day would collide. `new` cannot see the
other worktree's file, so the collision would surface as an add/add conflict at merge —
loud, not silent, and rarer by far than the by-construction conflict this replaces. Pass
`--slug` to name a fragment yourself when you know you are in that case, or when a
human-meaningful name would read better than a branch name.

## Ordering

Fragments arrive unordered on disk, so the release step sorts them: **filename descending**.
The filename begins with an ISO-8601 date, so that is newest-day-first, matching the
newest-first convention `CHANGELOG.md` already uses. Within one day the tiebreak is
reverse-alphabetical by slug — arbitrary, but deterministic and predictable, which
"filesystem order" is not.

## `## Unreleased` stays, permanently

The heading is not deleted once fragments take over, and its body is normally empty. It
stays because it is the tool's **second input**: anything that still lands there — a
long-lived branch written before this changed, a contributor working from an older
checkout, a merge that resolved a hunk into it — is absorbed by the next release rather
than being silently left behind or silently dropped. It is placed *below* the fragments in
the emitted section, because prose written under the old scheme predates every fragment and
the file is newest-first.

That also answers the transition: **the boundary is not a date, it is a mechanism.** Both
sources are read, permanently, so an open PR carrying a `## Unreleased` entry cannot be
orphaned by this change. `merge=union` stays in `.gitattributes` as the belt for exactly
those in-flight entries.

## Usage

    python3 native/tools/changelog.py new "The console command lane leaves the plugin crate"
    python3 native/tools/changelog.py check          # validate every fragment; exit 1 on findings
    python3 native/tools/changelog.py render         # print what a release would insert
    python3 native/tools/changelog.py release 0.1.0  # rewrite CHANGELOG.md, clear changelog.d/

`release` runs `check` first and **refuses to write if anything fails**. That is the guard
against the one failure mode worse than the conflict it replaces: a concatenation step that
silently drops a fragment. A file in `changelog.d/` is either a valid fragment, or
`README.md`, or an error — never ignored.

Tests: `native/tools/test_changelog.py` (stdlib `unittest`, no dependencies, no cargo).
"""

from __future__ import annotations

import argparse
import datetime as _dt
import re
import subprocess
import sys
from pathlib import Path

FRAGMENT_DIRNAME = "changelog.d"
CHANGELOG_NAME = "CHANGELOG.md"
UNRELEASED_HEADING = "## Unreleased"
UNRELEASED_NOTE = (
    "<!-- Normally empty. New entries are one file each in `changelog.d/`; "
    "see `changelog.d/README.md`. Anything that does land here is absorbed by the "
    "next release. -->"
)

# `YYYY-MM-DD-<slug>.md`. The slug is lowercase alphanumerics and hyphens, starting with
# an alphanumeric — the shape `slugify` produces, so a hand-written name that this rejects
# is a name `new` would not have made.
FRAGMENT_RE = re.compile(r"^(\d{4}-\d{2}-\d{2})-([a-z0-9][a-z0-9-]*)\.md$")

# A whole line that is one HTML comment. Used to tell "the Unreleased body is empty" from
# "the Unreleased body has prose in it" — the pointer note above is not prose.
HTML_COMMENT_LINE_RE = re.compile(r"^\s*<!--.*-->\s*$")


# ── locating things ──────────────────────────────────────────────────────────────────


def repo_root_from_here() -> Path:
    """`native/tools/changelog.py` → the repo root, two directories up.

    Derived from this file's own location rather than from the cwd, so the tool works
    from anywhere; `--repo` overrides it, which is what the tests use.
    """
    return Path(__file__).resolve().parents[2]


def fragment_dir(root: Path) -> Path:
    return root / FRAGMENT_DIRNAME


def changelog_path(root: Path) -> Path:
    return root / CHANGELOG_NAME


def read_text(path: Path) -> str:
    """UTF-8, always.

    ⚠️ Not the platform default. Python's default encoding on Windows is the ANSI
    codepage, which cannot represent 🚨/⚠️/📌 — the markers this file's house style is
    built on. Reading a fragment with the default would raise, or worse, mangle.
    """
    return path.read_text(encoding="utf-8")


def write_text(path: Path, text: str) -> None:
    """UTF-8 and LF, always.

    LF because `.gitattributes` stores this tree LF and normalises on commit; writing the
    platform ending would produce a working tree that differs from the index for no
    reason. The `newline=""` argument disables Python's own translation.
    """
    with path.open("w", encoding="utf-8", newline="") as fh:
        fh.write(text)


# ── naming ───────────────────────────────────────────────────────────────────────────


def slugify(text: str) -> str:
    """Lowercase; every run of non-alphanumerics becomes one hyphen; trimmed.

    ⚠️ Not injective — `feat/foo` and `feat-foo` both land on `feat-foo`. That is the
    residual collision documented at the top of this file, and it is accepted rather than
    engineered around: the branch names this repo actually uses carry a hex suffix, the
    collision surfaces as a loud add/add conflict rather than a silent overwrite, and
    `--slug` is the escape hatch.
    """
    out = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")
    return out[:60].rstrip("-")


def current_branch(root: Path) -> str | None:
    """The checked-out branch, or None if git cannot say (detached HEAD, no git, no repo).

    Returning None rather than guessing is deliberate: the branch is what makes the
    filename unique, so a fabricated stand-in would quietly reintroduce collisions.
    `new` asks for `--slug` instead.
    """
    try:
        proc = subprocess.run(
            ["git", "rev-parse", "--abbrev-ref", "HEAD"],
            cwd=str(root),
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        return None
    if proc.returncode != 0:
        return None
    name = proc.stdout.strip()
    if not name or name == "HEAD":  # detached
        return None
    return name


# ── reading fragments ────────────────────────────────────────────────────────────────


def fragment_paths(root: Path) -> list[Path]:
    """Every valid fragment, **newest first** — filename descending.

    `README.md` is excluded by name. Anything else that does not match `FRAGMENT_RE` is
    NOT skipped here; it is left for `check` to report, so a mis-named fragment fails the
    release rather than vanishing from it.
    """
    d = fragment_dir(root)
    if not d.is_dir():
        return []
    found = [p for p in d.iterdir() if p.is_file() and FRAGMENT_RE.match(p.name)]
    return sorted(found, key=lambda p: p.name, reverse=True)


def check(root: Path) -> list[str]:
    """Validate `changelog.d/`. Returns findings; empty means clean."""
    findings: list[str] = []
    d = fragment_dir(root)
    if not d.is_dir():
        return findings

    for path in sorted(d.iterdir(), key=lambda p: p.name):
        if path.is_dir():
            findings.append(f"{path.name}: directories are not fragments — remove it")
            continue
        if path.name == "README.md":
            continue

        m = FRAGMENT_RE.match(path.name)
        if not m:
            findings.append(
                f"{path.name}: not a fragment name. Expected `YYYY-MM-DD-<slug>.md` "
                f"(lowercase slug). Rename it, or `changelog.py new` will make one."
            )
            continue

        stamp = m.group(1)
        try:
            _dt.date.fromisoformat(stamp)
        except ValueError:
            findings.append(f"{path.name}: `{stamp}` is not a real date")

        body = read_text(path)
        stripped = body.strip()
        if not stripped:
            findings.append(f"{path.name}: empty")
            continue

        first = stripped.splitlines()[0]
        if not first.startswith("### "):
            findings.append(
                f"{path.name}: must open with a `### ` heading — a fragment is exactly "
                f"what would have gone under `## Unreleased`. Found: {first[:60]!r}"
            )

        # Level-2 headings belong to the release step, which owns the version heading.
        # One inside a fragment would split the emitted section in half.
        for n, line in enumerate(stripped.splitlines(), start=1):
            if re.match(r"^##(?!#)\s", line):
                findings.append(
                    f"{path.name}:{n}: `## ` heading — a fragment may only use `### ` "
                    f"and deeper. The release step owns the version heading."
                )
                break

        # ⚠️ Duplicates `.claude/hooks/doc-coherence.sh`'s fence rule ON PURPOSE, and
        # inherits the duty to be updated with it. That hook takes an explicit list of
        # DURABLE docs from `.claude/settings.json`; a fragment is transient and never
        # appears on that list, so it would reach `CHANGELOG.md` unchecked — and an
        # unmatched fence swallows everything after it when rendered, which in a
        # concatenated file means it swallows other people's entries too.
        fences = sum(1 for line in body.splitlines() if line.startswith("```"))
        if fences % 2:
            findings.append(
                f"{path.name}: {fences} code fences (odd). An unmatched fence swallows "
                f"the rest of CHANGELOG.md once this is concatenated."
            )

    return findings


def render(root: Path) -> str:
    """The fragments, newest first, joined by one blank line. No trailing newline."""
    blocks = [read_text(p).strip() for p in fragment_paths(root)]
    return "\n\n".join(b for b in blocks if b)


# ── the release rewrite ──────────────────────────────────────────────────────────────


def _split_unreleased(text: str) -> tuple[list[str], list[str], list[str]]:
    """Split CHANGELOG.md into (before, unreleased body, after-and-including next `## `).

    Raises ValueError if `## Unreleased` is missing — which is a real error, not something
    to paper over: without it the tool has no idea where the top of the file's history is,
    and guessing would put a release section somewhere arbitrary.
    """
    lines = text.split("\n")
    try:
        start = next(i for i, ln in enumerate(lines) if ln.strip() == UNRELEASED_HEADING)
    except StopIteration:
        raise ValueError(
            f"{CHANGELOG_NAME} has no `{UNRELEASED_HEADING}` heading. It is the anchor "
            f"this tool splices at, and it is meant to stay in the file permanently."
        )
    end = len(lines)
    for i in range(start + 1, len(lines)):
        if re.match(r"^##(?!#)\s", lines[i]):
            end = i
            break
    return lines[:start], lines[start + 1 : end], lines[end:]


def _absorbed_prose(body_lines: list[str]) -> str:
    """Whatever real prose still sits under `## Unreleased`, with the pointer note dropped.

    An HTML-comment line is not prose — that is how "the body is the note we put there"
    is told from "someone's entry landed here".
    """
    kept = [ln for ln in body_lines if not HTML_COMMENT_LINE_RE.match(ln)]
    return "\n".join(kept).strip()


def release(root: Path, version: str, date: str | None = None) -> str:
    """Fold `changelog.d/` (and any stray `## Unreleased` prose) into a version section.

    Returns the new `CHANGELOG.md` text and writes it; deletes the fragments it consumed.
    Refuses — raising ValueError, having written and deleted nothing — if `check` finds
    anything, so a mis-named or malformed fragment cannot be dropped on the floor.
    """
    findings = check(root)
    if findings:
        raise ValueError(
            "changelog.d/ has problems; refusing to release:\n  " + "\n  ".join(findings)
        )

    stamp = date or _dt.date.today().isoformat()
    _dt.date.fromisoformat(stamp)  # reject a hand-passed non-date loudly

    fragments = fragment_paths(root)
    rendered = render(root)

    path = changelog_path(root)
    before, body, after = _split_unreleased(read_text(path))
    absorbed = _absorbed_prose(body)

    if not rendered and not absorbed:
        raise ValueError(
            "nothing to release: changelog.d/ is empty and `## Unreleased` has no prose."
        )

    sections = [s for s in (rendered, absorbed) if s]
    region = [
        UNRELEASED_HEADING,
        "",
        UNRELEASED_NOTE,
        "",
        f"## {version} — {stamp}",
        "",
        "\n\n".join(sections),
        "",
    ]

    new_text = "\n".join(before + region + after)
    write_text(path, new_text)

    for frag in fragments:
        frag.unlink()

    return new_text


# ── creating a fragment ──────────────────────────────────────────────────────────────


def new_fragment(
    root: Path,
    title: str,
    slug: str | None = None,
    date: str | None = None,
) -> Path:
    """Create `changelog.d/YYYY-MM-DD-<slug>.md` seeded with a `### <title>` heading.

    A name already taken gains `-2`, `-3`, … — several changes on one branch on one day.
    That loop only ever sees **your own worktree**, which is exactly right: it is
    resolving a collision with yourself, where there is no race. Collisions with another
    branch are the documented residual case, not this loop's job.
    """
    stamp = date or _dt.date.today().isoformat()
    _dt.date.fromisoformat(stamp)

    if slug:
        base = slugify(slug)
    else:
        branch = current_branch(root)
        if not branch:
            raise ValueError(
                "cannot read the current branch (detached HEAD, or not a git checkout). "
                "Pass --slug to name the fragment yourself."
            )
        base = slugify(branch)

    if not base:
        raise ValueError("the slug is empty once normalised — pass a --slug with letters in it")

    d = fragment_dir(root)
    d.mkdir(parents=True, exist_ok=True)

    candidate = d / f"{stamp}-{base}.md"
    n = 2
    while candidate.exists():
        candidate = d / f"{stamp}-{base}-{n}.md"
        n += 1

    heading = title.strip() or "TODO — what changed, in a sentence"
    write_text(candidate, f"### {heading}\n\n")
    return candidate


# ── cli ──────────────────────────────────────────────────────────────────────────────


def main(argv: list[str] | None = None) -> int:
    # 🚨 Windows consoles default to the ANSI codepage and `print` raises
    # UnicodeEncodeError on 🚨/⚠️/📌 — the markers `render` is guaranteed to be echoing.
    # A tool that dies printing the thing it just built correctly reads as a broken tool.
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")  # type: ignore[union-attr]
        except (AttributeError, ValueError):
            pass

    ap = argparse.ArgumentParser(
        prog="changelog.py",
        description="changelog.d/ fragments → CHANGELOG.md",
    )
    ap.add_argument("--repo", type=Path, default=None, help="repo root (default: derived from this file)")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_new = sub.add_parser("new", help="create a fragment for this branch")
    p_new.add_argument("title", nargs="?", default="", help="the `### ` heading")
    p_new.add_argument("--slug", default=None, help="override the branch-derived slug")
    p_new.add_argument("--date", default=None, help="override today's date (YYYY-MM-DD)")

    sub.add_parser("check", help="validate every fragment; exit 1 on findings")
    sub.add_parser("render", help="print what a release would insert")

    p_rel = sub.add_parser("release", help="fold fragments into CHANGELOG.md")
    p_rel.add_argument("version", help="e.g. 0.1.0")
    p_rel.add_argument("--date", default=None, help="override today's date (YYYY-MM-DD)")

    args = ap.parse_args(argv)
    root = args.repo.resolve() if args.repo else repo_root_from_here()

    if args.cmd == "check":
        findings = check(root)
        if findings:
            print(f"‼️  {FRAGMENT_DIRNAME}/ — {len(findings)} problem(s):")
            for f in findings:
                print(f"      {f}")
            return 1
        n = len(fragment_paths(root))
        print(f"✅ {FRAGMENT_DIRNAME}/ — {n} fragment(s), all well-formed.")
        return 0

    if args.cmd == "render":
        out = render(root)
        if not out:
            print(f"(no fragments in {FRAGMENT_DIRNAME}/)")
            return 0
        print(out)
        return 0

    if args.cmd == "new":
        try:
            path = new_fragment(root, args.title, slug=args.slug, date=args.date)
        except ValueError as e:
            print(f"‼️  {e}")
            return 1
        print(path.relative_to(root).as_posix())
        return 0

    if args.cmd == "release":
        try:
            release(root, args.version, date=args.date)
        except ValueError as e:
            print(f"‼️  {e}")
            return 1
        print(f"✅ {CHANGELOG_NAME} now carries `## {args.version}`; {FRAGMENT_DIRNAME}/ is clear.")
        return 0

    return 2  # unreachable while `required=True` holds


if __name__ == "__main__":
    raise SystemExit(main())
