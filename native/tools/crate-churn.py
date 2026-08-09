#!/usr/bin/env python3
"""Cross-crate churn: what fraction of commits touch more than one crate?

**This is #626 §2.4's deferred measurement**, and the whole topology decision waits on
it. The issue declines to rule on a repository split until the number exists:

  ≲10%   the crates are genuinely independent; a repo split is cheap
  10–30% viable, but every split-repo change needs a two-PR dance
  ≳30%   the engine is still churning too hard to expose a stable API

A script rather than a paragraph, because #618's method lesson is that a number nobody
can re-derive is a number that silently rots. Re-run it; don't trust this file's output
from whenever it was last pasted into ARCHITECTURE.md.

## The thing that makes this non-trivial

**History predates the crates.** `organon-core` is two days old; `organon-render` is
hours old. A commit from last week that edited `native/src/math.rs` was, in every sense
that matters to this question, `organon-core` work — but a naive path prefix map
attributes it to the root crate and reports near-zero cross-crate churn, which is a
wrong answer that looks plausible.

So every historical path is **resolved forward through renames to where it lives today**
before being classified. `git log -M --name-status` reports the crate extractions as
`R100` rename pairs (101 of them across the last 400 commits); this builds the old→new
map, follows it transitively, and classifies the *destination*. A file that moved twice
resolves through both hops.

⚠️ If a future extraction is done as delete+add rather than `git mv`, git will not emit
`R` lines, this resolver will silently miss it, and the number will drift low. That is
the one failure mode worth knowing about — it is why the script prints how many renames
it resolved, so an implausible drop is visible rather than silent.

Usage:
    python3 native/tools/crate-churn.py [--commits 400] [--verbose]
"""

import argparse
import collections
import os
import subprocess
import sys

# Path prefix → crate. Order matters: the most specific prefix wins, so the four
# `native/<crate>/` entries must precede the bare `native/` catch-all.
CRATE_PREFIXES = [
    ("native/organon-core/", "organon-core"),
    ("native/organon-mind/", "organon-mind"),
    ("native/organon-render/", "organon-render"),
    ("native/xtask/", "xtask"),
    ("native/vendor/", "vendor"),
    # Their own workspace roots, excluded from the default build and the mirror (#418
    # parked) — counted separately so they cannot inflate the cross-crate number.
    ("native/organon-wasm/", "(wasm, parked)"),
    ("native/organon-manifest/", "(wasm, parked)"),
    ("native/", "organic-math-native"),
]

# Crates the topology question is actually about. `xtask` is the VST3 bundler and
# `vendor` is a vendored dependency; neither is a candidate for its own repo, and the
# parked wasm crates are not in the build at all. Counting them would answer a
# different question than §2.4 asks.
TOPOLOGY_CRATES = {
    "organon-core",
    "organon-mind",
    "organon-render",
    "organic-math-native",
}


def repo_root():
    return git("rev-parse", "--show-toplevel").strip()


def git(*args):
    return subprocess.run(
        ["git", *args], capture_output=True, text=True, check=True
    ).stdout


def build_rename_map(n):
    """old path → new path. Applied transitively by `resolve`.

    Built over ALL commits (not first-parent) and over a wider window than the
    measurement itself, because a rename recorded on a feature branch still has to
    resolve for a first-parent commit that predates the merge.
    """
    out = git("log", "-M", "--name-status", "-n", str(n * 4), "--format=%x00%H")
    renames = {}
    for line in out.split("\n"):
        if not line.startswith("R"):
            continue
        parts = line.split("\t")
        if len(parts) == 3:
            _, old, new = parts
            renames[old] = new
    return renames


def resolve(path, renames, _seen=None):
    """Follow a path forward through renames to where it lives today."""
    seen = _seen if _seen is not None else set()
    while path in renames and path not in seen:
        seen.add(path)
        path = renames[path]
    return path


def classify(path):
    for prefix, crate in CRATE_PREFIXES:
        if path.startswith(prefix):
            return crate
    return None  # docs, site, scripts, .github, .claude — not a crate


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--commits", type=int, default=400)
    ap.add_argument("--verbose", action="store_true")
    # §2.4's bands are about the cost of a "two-PR dance", which is paid per UNIT OF
    # WORK — a merge to main — not per intermediate commit on a branch. So first-parent
    # is the default. `--all-commits` answers the narrower question of whether the work
    # decomposes crate-locally even when the PR doesn't.
    ap.add_argument("--all-commits", action="store_true")
    args = ap.parse_args()

    # ⚠️ A SHALLOW CLONE SILENTLY TRUNCATES THIS. Cloud sessions get a shallow clone by
    # default; `-n 400` on a 344-commit clone quietly measures 344 and reports a number
    # that looks fine while answering a smaller question. Check and say so.
    # ⚠️ RUN THIS FROM A TREE THAT HAS THE CRATES. Checked out before the extractions,
    # the rename map is empty, every path classifies as the root crate, and the script
    # reports a serene 0.0% cross-crate churn — the exact "wrong answer that looks
    # plausible" this file's header warns about. Verified by reproducing it: at HEAD~30
    # the number was 0.0% (0/257). Assert the crates exist on disk instead.
    missing = [
        d for d in ("native/organon-core", "native/organon-mind", "native/organon-render")
        if not os.path.isdir(os.path.join(repo_root(), d))
    ]
    if missing:
        print(
            "ERROR: the working tree predates the crate split — missing "
            + ", ".join(missing),
            file=sys.stderr,
        )
        print(
            "  Rename resolution needs TODAY's tree as its reference. Run from a commit\n"
            "  that has the crates; otherwise every path resolves to the root crate and\n"
            "  the result is a plausible-looking 0%.",
            file=sys.stderr,
        )
        return 2

    # ⚠️ Count in the SAME MODE the walk will use. `--all-commits` reaches strictly more
    # commits than first-parent does, so a first-parent count here rejects valid
    # `--all-commits --commits N` runs and tells the user to unshallow a clone that is
    # already deep enough. A guard that blocks a correct invocation is the same class of
    # defect as one that permits a wrong one — this script exists to avoid both.
    fp = [] if args.all_commits else ["--first-parent"]
    available = int(git("rev-list", "--count", *fp, "HEAD").strip())
    if available < args.commits:
        shallow = git("rev-parse", "--is-shallow-repository").strip() == "true"
        mode = "reachable" if args.all_commits else "on first-parent"
        print(
            f"ERROR: asked for {args.commits} commits, only {available} {mode}.",
            file=sys.stderr,
        )
        if shallow:
            print("  This is a SHALLOW clone — run `git fetch --unshallow` first.", file=sys.stderr)
        else:
            print(f"  Re-run with --commits {available}.", file=sys.stderr)
        return 2

    # One pass per commit: which crates did it touch, after resolution?
    renames = build_rename_map(args.commits)
    out = git(
        "log", *(["--first-parent"] if not args.all_commits else []),
        "-M", "--name-only", "-n", str(args.commits), "--format=%x00%H %s",
    )
    commits = []
    for block in out.split("\x00"):
        if not block.strip():
            continue
        head, *paths = block.strip("\n").split("\n")
        sha, _, subject = head.partition(" ")
        crates = set()
        resolved_any = 0
        for p in paths:
            if not p.strip():
                continue
            r = resolve(p, renames)
            if r != p:
                resolved_any += 1
            c = classify(r)
            if c in TOPOLOGY_CRATES:
                crates.add(c)
        commits.append((sha, subject, crates, resolved_any))

    touching = [c for c in commits if c[2]]
    multi = [c for c in touching if len(c[2]) > 1]

    unit = "commits (all parents)" if args.all_commits else "merges to main (first-parent)"
    print(f"Window: last {args.commits} {unit}  —  {len(commits)} walked")
    print(f"Renames resolved forward: {len(renames)} distinct path moves")
    print()
    print(f"Commits touching ≥1 topology crate: {len(touching)}")
    print(f"Commits touching ≥2 topology crates: {len(multi)}")
    if touching:
        pct = 100.0 * len(multi) / len(touching)
        print(f"\n  CROSS-CRATE CHURN: {pct:.1f}%  ({len(multi)}/{len(touching)})\n")
        band = (
            "≲10% — crates are genuinely independent; a repo split is cheap"
            if pct <= 10
            else "10–30% — viable, but every split-repo change needs a two-PR dance"
            if pct <= 30
            else "≳30% — the engine is still churning too hard to expose a stable API"
        )
        print(f"  §2.4 band: {band}")

    print("\nPer-crate commit counts (a commit can appear in several):")
    per = collections.Counter()
    for _, _, crates, _ in touching:
        per.update(crates)
    for c, n in per.most_common():
        print(f"  {n:4d}  {c}")

    print("\nPer-crate-pair breakdown (commits touching both):")
    pairs = collections.Counter()
    for _, _, crates, _ in multi:
        for a in sorted(crates):
            for b in sorted(crates):
                if a < b:
                    pairs[(a, b)] += 1
    if not pairs:
        print("  (none)")
    for (a, b), n in pairs.most_common():
        print(f"  {n:4d}  {a}  ×  {b}")

    if args.verbose:
        print("\nCross-crate commits:")
        for sha, subject, crates, _ in multi:
            print(f"  {sha[:8]}  {'+'.join(sorted(crates))}  {subject[:60]}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
