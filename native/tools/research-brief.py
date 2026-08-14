#!/usr/bin/env python3
"""Build a deep-research **dispatch prompt** from a checked-in brief plus measured repo state.

`doc/research/README.md` owns the *why*. This file owns the *how*, and exists because of
one property: a research report is only worth keeping if you can tell what it was looking
at. A report that says "the docs overstate the test coverage" is a finding; the same
sentence with no commit attached is a rumour, because the tree it describes may not exist
any more.

So a dispatch prompt is two halves welded together:

  brief   doc/research/briefs/<id>.md — the QUESTION. Hand-written, checked in, reviewed
          like any other doc. Stable across runs; that stability is what makes two runs
          comparable.
  facts   measured HERE, at dispatch time, from the working tree. Never hand-written,
          never checked in.

The facts half is the reason this is a script rather than a paragraph telling people to
paste the README into a chatbot. Every number below is re-derived on every run, so a
brief cannot quietly start describing a version of the repo that no longer exists — the
failure mode `organon docs` and its drift test already close for the reference pages.

## What is deliberately NOT checked in

The **dispatch prompt itself**. It embeds a commit SHA and a dozen counts, so it changes
on essentially every commit; checking it in would either produce churn on every PR or a
drift test that fails constantly, and both train people to ignore it. Briefs are checked
in and reports are checked in. The thing in between is a build artifact, rebuilt on
demand and attached to a workflow run.

## The one number that lies if you let it

`--facts` reports counts, not judgements, and each line carries how it was obtained:

  measured  read directly from the tree (file counts, line counts, git metadata)
  derived   an exact function of something measured (catalog counts come from the
            `doc/reference/` tables, which are themselves generated from the Rust and
            guarded by `generated_reference_is_current` — so counting rows there is
            counting the source, one hop removed)

⚠️ The failure mode worth knowing: the catalog counts parse **table rows in generated
Markdown**. If `organon docs` ever changes its table shape, these counts silently drop to
zero rather than erroring. `--validate` therefore asserts they are non-zero, so a shape
change fails a CI job instead of quietly shipping a brief that tells four models this
project has no generators.

The durable-doc list is not duplicated here either — it is read out of
`.claude/hooks/doc-rules.sh`, the same table both doc hooks source. Adding a durable doc
there puts it in every future brief for free; a second copy here is exactly the rot
organon#590 is about.

Usage:
    python3 native/tools/research-brief.py --list
    python3 native/tools/research-brief.py --facts
    python3 native/tools/research-brief.py --brief doc-code-fidelity
    python3 native/tools/research-brief.py --all --out /tmp/dispatch
    python3 native/tools/research-brief.py --validate
"""

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

# native/tools/research-brief.py → repo root is two levels up from `native/`.
ROOT = Path(__file__).resolve().parents[2]
BRIEFS = ROOT / "doc" / "research" / "briefs"
REPORTS = ROOT / "doc" / "research" / "reports"
DOC_RULES = ROOT / ".claude" / "hooks" / "doc-rules.sh"

# Front-matter keys a brief must carry, and what each is for. `scoreable` is the load-
# bearing one: it records whether this question has ground truth in the tree, which
# decides whether cross-model disagreement can be adjudicated or only noted.
BRIEF_KEYS = ("id", "title", "scoreable", "cadence", "one_line")
BRIEF_SECTIONS = ("## Question", "## Scope", "## Method", "## Deliverable")

# Front-matter keys a checked-in report must carry. `commit` is what makes a claim
# re-checkable a year later; `status` is what stops an agent reading an unreviewed
# external opinion as settled fact.
REPORT_KEYS = ("brief", "model", "run_date", "commit", "status")
REPORT_STATUS = ("unreviewed", "adjudicated", "superseded")

SCOREABLE = ("yes", "partial", "no")


def git(*args, default=""):
    """Run a git command, returning `default` if git or the repo is unavailable.

    Dispatch has to work from a tarball export as well as a checkout — the public repo is
    produced by subtraction and someone will eventually run this without `.git`.
    """
    try:
        out = subprocess.run(
            ["git", "-C", str(ROOT), *args],
            capture_output=True, text=True, check=True,
        )
        return out.stdout.strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return default


def rust_stats(path):
    """(files, lines) of `.rs` under `path`. Counts source only — no shaders, no toml."""
    files = sorted(path.rglob("*.rs"))
    lines = 0
    for f in files:
        try:
            lines += sum(1 for _ in f.open(encoding="utf-8", errors="replace"))
        except OSError:
            pass
    return len(files), lines


def crate_license(manifest):
    m = re.search(r'^license\s*=\s*"([^"]+)"', manifest.read_text(encoding="utf-8"),
                  re.MULTILINE)
    return m.group(1) if m else "?"


def workspace_members():
    """The workspace root plus every `members = [...]` entry, in manifest order."""
    root_manifest = ROOT / "native" / "Cargo.toml"
    text = root_manifest.read_text(encoding="utf-8")
    m = re.search(r"^members\s*=\s*\[(.*?)\]", text, re.MULTILINE | re.DOTALL)
    names = re.findall(r'"([^"]+)"', m.group(1)) if m else []
    out = [("organic-math-native", ROOT / "native" / "src", root_manifest)]
    for n in names:
        d = ROOT / "native" / n
        if (d / "Cargo.toml").exists():
            out.append((n, d / "src", d / "Cargo.toml"))
    return out


def binaries():
    """Every `[[bin]]` across the workspace, with the feature that gates it (if any).

    The gate is the interesting half: three of these do not exist in a default build, and
    a model told "eight binaries" without that qualifier will draw the wrong conclusion
    about what CI covers.
    """
    found = []
    for manifest in [ROOT / "native" / "Cargo.toml"] + [
        ROOT / "native" / d / "Cargo.toml" for d in os.listdir(ROOT / "native")
        if (ROOT / "native" / d / "Cargo.toml").exists()
    ]:
        text = manifest.read_text(encoding="utf-8")
        for block in re.split(r"^\[\[bin\]\]", text, flags=re.MULTILINE)[1:]:
            block = re.split(r"^\[", block, flags=re.MULTILINE)[0]
            name = re.search(r'^name\s*=\s*"([^"]+)"', block, re.MULTILINE)
            feat = re.search(r"^required-features\s*=\s*\[(.*?)\]", block,
                             re.MULTILINE | re.DOTALL)
            if name:
                gate = ", ".join(re.findall(r'"([^"]+)"', feat.group(1))) if feat else ""
                found.append((name.group(1), gate))
    return sorted(set(found))


def catalog_counts():
    """Row counts from the generated `doc/reference/` tables.

    Derived, not measured — but derived from a file a test keeps in step with the Rust,
    which makes it a cheaper way to count generators than parsing an enum whose shape is
    not guarded by anything.
    """
    def rows(page, pattern):
        p = ROOT / "doc" / "reference" / page
        if not p.exists():
            return 0
        return sum(1 for line in p.read_text(encoding="utf-8").splitlines()
                   if re.match(pattern, line))

    return [
        ("generators", rows("generators.md", r"^\|\s*\d+\s*\|")),
        ("surfaces", rows("surfaces.md", r"^\|\s*\d+\s*\|")),
        ("materials", rows("materials.md", r"^\|\s*\d+\s*\|")),
        ("CLI parameters", rows("parameters.md", r"^\|\s*`")),
        ("recipes", rows("recipes.md", r"^##\s+.*\(`[^`]+`\)")),
    ]


def durable_docs():
    """The durable docs, read out of `.claude/hooks/doc-rules.sh` rather than re-listed.

    Returns (path, lines, last-commit-date). Rows whose file is absent are skipped: the
    table deliberately carries rules for a `web/` tree this repo does not have.
    """
    if not DOC_RULES.exists():
        return []
    text = DOC_RULES.read_text(encoding="utf-8")
    m = re.search(r'^DOC_RULES="(.*?)"', text, re.MULTILINE | re.DOTALL)
    if not m:
        return []
    out = []
    for line in m.group(1).splitlines():
        line = line.strip()
        if not line or "|" not in line:
            continue
        doc = line.split("|", 1)[0]
        p = ROOT / doc
        if not p.exists():
            continue
        lines = sum(1 for _ in p.open(encoding="utf-8", errors="replace"))
        when = git("log", "-1", "--format=%as", "--", doc, default="?")
        out.append((doc, lines, when))
    return out


def count_matches(pattern, *globs):
    n = 0
    for g in globs:
        for f in ROOT.glob(g):
            try:
                n += len(re.findall(pattern, f.read_text(encoding="utf-8",
                                                         errors="replace")))
            except OSError:
                pass
    return n


def fact_pack():
    """The measured half of a dispatch prompt.

    Every line is labelled with how it was obtained. That labelling is not decoration:
    the reports these prompts produce get read by agents, and this project's whole
    posture on provenance (MIND_ARCHITECTURE.md §3) is that an unlabelled number is worse
    than a missing one.
    """
    L = []
    add = L.append

    sha = git("rev-parse", "HEAD", default="(no git)")
    add("## Repository state (measured at dispatch)\n")
    add("Everything in this section was read from the tree at dispatch time. Anchor every")
    add("claim you make to this commit — a later reader needs to know which tree you saw.\n")
    add(f"- **commit** — `{sha}` *(measured)*")
    add(f"- **commit date** — {git('log', '-1', '--format=%as', default='?')} *(measured)*")
    add(f"- **branch/ref** — `{git('rev-parse', '--abbrev-ref', 'HEAD', default='?')}` *(measured)*")
    add(f"- **commits in history** — {git('rev-list', '--count', 'HEAD', default='?')} *(measured)*")

    add("\n### Crates *(measured: `.rs` files and lines under each crate's `src/`)*\n")
    add("| Crate | Licence | `.rs` files | Lines |")
    add("|---|---|---:|---:|")
    for name, src, manifest in workspace_members():
        if not src.exists():
            continue
        files, lines = rust_stats(src)
        add(f"| `{name}` | {crate_license(manifest)} | {files} | {lines:,} |")

    add("\n### Binaries *(measured: `[[bin]]` across the workspace)*\n")
    add("| Binary | Gated behind |")
    add("|---|---|")
    for name, gate in binaries():
        add(f"| `{name}` | {gate or '— (default build)'} |")
    add("")
    add("⚠️ A binary with a gate is **not built or tested by a default `cargo build` /")
    add("`cargo test`**. Any claim about coverage has to account for that.")

    add("\n### Catalog *(derived: row counts from the generated `doc/reference/` tables,")
    add("which a test keeps in step with the Rust)*\n")
    for label, n in catalog_counts():
        add(f"- **{label}** — {n}")

    add("\n### Scale of the other trees *(measured)*\n")
    add(f"- **WGSL shaders** — {len(list(ROOT.glob('native/**/*.wgsl')))}")
    n_tests = count_matches(r"#\[test\]", "native/**/*.rs")
    add(f"- **`#[test]` attributes** — {n_tests} *(measured, but an undercount: "
        "`#[cfg(test)]` helpers and table-driven cases mean this is not the number of "
        "assertions)*")
    add(f"- **Markdown under `doc/`** — {len(list(ROOT.glob('doc/**/*.md')))} files")

    add("\n### Durable docs *(measured; list read from `.claude/hooks/doc-rules.sh`)*\n")
    add("| Doc | Lines | Last touched |")
    add("|---|---:|---|")
    for doc, lines, when in durable_docs():
        add(f"| `{doc}` | {lines:,} | {when} |")

    log = git("log", "-30", "--format=%as  %s", default="")
    if log:
        add("\n### The last 30 commits *(measured — this is the current front of the work)*\n")
        add("```")
        add(log)
        add("```")

    return "\n".join(L)


def parse_front_matter(path):
    """Parse a leading `---` fenced `key: value` block. Returns ({}, body) if absent."""
    text = path.read_text(encoding="utf-8")
    m = re.match(r"^---\n(.*?)\n---\n(.*)$", text, re.DOTALL)
    if not m:
        return {}, text
    meta = {}
    for line in m.group(1).splitlines():
        if ":" in line:
            k, v = line.split(":", 1)
            meta[k.strip()] = v.strip()
    return meta, m.group(2)


def load_briefs():
    out = []
    for p in sorted(BRIEFS.glob("*.md")):
        meta, body = parse_front_matter(p)
        out.append((p, meta, body))
    return out


def dispatch(path, meta, body, facts):
    """Weld a brief to the fact pack into the text you actually paste into a model."""
    sha = git("rev-parse", "--short", "HEAD", default="unknown")
    return f"""<!-- Organon deep-research dispatch — brief `{meta.get('id', path.stem)}` @ {sha}
     GENERATED by native/tools/research-brief.py. Not checked in: it is rebuilt from
     doc/research/briefs/{path.name} plus repo state every time it is dispatched. -->

# Deep research brief — {meta.get('title', path.stem)}

You are being asked to research an open-source project and produce a report that will be
**checked into that project's repository** and read by both its maintainers and by AI
agents doing later work on it. Take the second audience seriously: an agent that believes
a wrong claim in your report will act on it.

Three rules govern the report, and they are not negotiable:

1. **Separate what you verified from what you inferred.** This project labels every
   displayed quantity `measured` / `derived` / `proxy` / `projection` and will hold your
   report to the same standard. A confident sentence you did not check is the one defect
   that makes the whole report unusable.
2. **Anchor to the commit below.** The tree moves. A claim without a commit cannot be
   re-checked later, and re-checking later is the entire point of keeping these.
3. **Say what you could not see.** You are working from a repository, not a running
   application. This is a GPU visualizer and an audio plugin: you cannot see it render,
   you cannot hear it, and you cannot load it in a DAW. Name the questions that needed
   those and were therefore left open, rather than answering them anyway.

{body.strip()}

---

{facts}

---

## Output contract

Emit Markdown, in this order. The last section is the one that gets machine-read, so keep
its shape exactly.

1. `## Summary` — at most 200 words. What you found, and how much of it you are sure of.
2. `## Findings` — the body of the report. Free-form; use whatever structure the question
   deserves. Cite files as `path:line` wherever a claim rests on one.
3. `## What I could not determine` — the questions this repository cannot answer, and
   what would answer them.
4. `## Claims` — a numbered list, **one line each**, in exactly this form:

   ```
   C1. [verified|inferred|speculative] (high|medium|low) — <the claim, one sentence> — <evidence: path:line, doc §, or "none">
   ```

   A `verified` claim is one you traced to a specific place in the tree and could quote.
   `inferred` follows from something you verified but is a step beyond it. `speculative`
   is judgement, prediction, or anything about the world outside the repo. Fifteen to
   thirty claims is the useful range. **The claims are what get compared across models
   and adjudicated against the tree**, so a claim that is too vague to be wrong is worse
   than no claim.
"""


def cmd_validate():
    """The CI gate. Checks the parts of this system that can rot without anyone noticing.

    Not a review — nobody can machine-check whether a research question is a good one.
    This checks the mechanical contracts: briefs parse, reports declare their provenance,
    and the fact-pack parsers still find something.
    """
    errs, warns = [], []

    briefs = load_briefs()
    if not briefs:
        errs.append("no briefs found under doc/research/briefs/")
    ids = set()
    for path, meta, body in briefs:
        rel = path.relative_to(ROOT)
        for k in BRIEF_KEYS:
            if not meta.get(k):
                errs.append(f"{rel}: front matter is missing `{k}`")
        if meta.get("id") and meta["id"] != path.stem:
            errs.append(f"{rel}: id `{meta['id']}` does not match the filename")
        if meta.get("scoreable") not in SCOREABLE:
            errs.append(f"{rel}: scoreable must be one of {'/'.join(SCOREABLE)}")
        if meta.get("scoreable") in ("yes", "partial") and not meta.get("ground_truth"):
            errs.append(f"{rel}: scoreable={meta.get('scoreable')} needs a `ground_truth` "
                        "line saying where an adjudicator checks")
        for s in BRIEF_SECTIONS:
            if not re.search(rf"^{re.escape(s)}\s*$", body, re.MULTILINE):
                errs.append(f"{rel}: missing section `{s}`")
        ids.add(path.stem)

    for path in sorted(REPORTS.glob("*.md")):
        if path.name == "README.md":
            continue
        rel = path.relative_to(ROOT)
        meta, body = parse_front_matter(path)
        for k in REPORT_KEYS:
            if not meta.get(k):
                errs.append(f"{rel}: front matter is missing `{k}`")
        if meta.get("brief") and meta["brief"] not in ids:
            errs.append(f"{rel}: names brief `{meta['brief']}`, which does not exist")
        if meta.get("status") and meta["status"] not in REPORT_STATUS:
            errs.append(f"{rel}: status must be one of {'/'.join(REPORT_STATUS)}")
        if meta.get("run_date") and not re.match(r"^\d{4}-\d{2}-\d{2}$", meta["run_date"]):
            errs.append(f"{rel}: run_date must be YYYY-MM-DD")
        if not re.search(r"^## Claims\s*$", body, re.MULTILINE):
            errs.append(f"{rel}: no `## Claims` section — the report cannot be compared "
                        "or adjudicated without one")

    # The parsers that fail SILENTLY rather than loudly. A generated-table shape change
    # would zero these out and nothing else would notice.
    for label, n in catalog_counts():
        if n == 0:
            errs.append(f"catalog count for {label} is 0 — the doc/reference/ table shape "
                        "probably changed; fix catalog_counts() in this script")
    if not durable_docs():
        warns.append("no durable docs resolved from .claude/hooks/doc-rules.sh")

    for w in warns:
        print(f"warning: {w}")
    for e in errs:
        print(f"error: {e}")
    n_reports = len([p for p in REPORTS.glob("*.md") if p.name != "README.md"])
    print(f"checked {len(briefs)} brief(s), {n_reports} report(s): "
          f"{len(errs)} error(s), {len(warns)} warning(s)")
    return 1 if errs else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--list", action="store_true", help="list the briefs")
    g.add_argument("--facts", action="store_true", help="print the fact pack alone")
    g.add_argument("--brief", metavar="ID", help="build one dispatch prompt")
    g.add_argument("--all", action="store_true", help="build every dispatch prompt")
    g.add_argument("--validate", action="store_true", help="CI gate: check the contracts")
    ap.add_argument("--out", metavar="DIR", help="write to DIR instead of stdout")
    args = ap.parse_args()

    if args.validate:
        return cmd_validate()

    if args.list:
        for path, meta, _ in load_briefs():
            print(f"{path.stem:<24} scoreable={meta.get('scoreable', '?'):<8} "
                  f"{meta.get('one_line', '')}")
        return 0

    if args.facts:
        print(fact_pack())
        return 0

    briefs = load_briefs()
    if args.brief:
        briefs = [b for b in briefs if b[0].stem == args.brief]
        if not briefs:
            print(f"error: no brief `{args.brief}`. Try --list.", file=sys.stderr)
            return 1

    facts = fact_pack()
    outdir = Path(args.out) if args.out else None
    if outdir:
        outdir.mkdir(parents=True, exist_ok=True)
    for path, meta, body in briefs:
        text = dispatch(path, meta, body, facts)
        if outdir:
            dest = outdir / f"dispatch-{path.stem}.md"
            dest.write_text(text, encoding="utf-8")
            print(f"wrote {dest} ({len(text):,} bytes)")
        else:
            print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
