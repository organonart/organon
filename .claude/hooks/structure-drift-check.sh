#!/usr/bin/env bash
# SessionStart: report the biggest functions and structs, and how they moved.
# organon#618 Tier 0c.
#
# WHY THIS EXISTS
#
# The 2026-08 review found `World::frame_body` at 7 451 lines and `struct World` at
# 250 fields. Neither arrived in one commit; both accreted over ~100 PRs across two
# months, and nobody — human or agent — ever raised it, because nothing ever asked.
#
# That is not carelessness, it is an asymmetry in what this repo automates. Doc drift
# is machine-checked three ways (same-change reminders, staleness, and now #618 T0b's
# coherence check). Code structure is checked zero ways. Every individual PR was
# locally right: adding a match arm is a 40-line diff, extracting a stage is a
# 4 000-line one that risks the byte-identity invariant. The gradient points one way
# on every single change, and the total was nobody's decision.
#
# An agent is especially bad at noticing this unprompted. It does not accumulate
# irritation the way a human maintainer does — every session starts fresh, reads the
# function, understands it, adds its piece and leaves. The cost lands on a future
# session that has no memory of the last one's friction.
#
# So: report, every session. Not a blocker, not a threshold to satisfy — a number in
# front of you, with its delta, so a trend is visible while it is still cheap.
#
# ⚠️ THIS IS NOT A "SHORTER IS BETTER" CHECK. #618 Tier 4 is explicit that the
# received function-length rule is human working-memory ergonomics and does not
# survive a codebase read by an agent in one pass. `frame_body` is expected to STAY
# long: a frame is a script that runs in order, and inlining keeps that honest. The
# number that actually binds is the FIELD COUNT — how much mutable state one region
# can reach — which is why struct size is reported beside function size and why the
# advice below points at partitioning rather than chopping.

set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 0
[ -d native/src ] || exit 0

STATE=".claude/.structure-drift.json"

read -r -d '' MEASURE <<'PY' || true
import json, pathlib, re, sys

root = pathlib.Path("native/src")

def fn_spans(path, text):
    """(name, lines) for each fn, measured signature -> matching closing brace at the
    same indent. Measuring to the NEXT item instead is the exact error the 2026-08
    review made: it overcounted three functions, one of them by 99 lines."""
    lines = text.splitlines()
    out = []
    pat = re.compile(r"^(\s*)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)")
    for i, line in enumerate(lines):
        m = pat.match(line)
        if not m:
            continue
        indent, name = m.group(1), m.group(2)
        close = indent + "}"
        for j in range(i + 1, len(lines)):
            if lines[j] == close:
                out.append((f"{path.name}::{name}", j - i + 1))
                break
    return out

def struct_fields(path, text):
    lines = text.splitlines()
    out = []
    pat = re.compile(r"^pub struct ([A-Za-z0-9_]+)\s*\{")
    field = re.compile(r"^    (?:pub(?:\([^)]*\))?\s+)?[A-Za-z_][A-Za-z0-9_]*\s*:")
    for i, line in enumerate(lines):
        m = pat.match(line)
        if not m:
            continue
        n = 0
        for j in range(i + 1, len(lines)):
            if lines[j] == "}":
                break
            if field.match(lines[j]):
                n += 1
        out.append((f"{path.name}::{m.group(1)}", n))
    return out

fns, structs = [], []
for p in sorted(root.rglob("*.rs")):
    try:
        t = p.read_text(errors="ignore")
    except OSError:
        continue
    fns += fn_spans(p, t)
    structs += struct_fields(p, t)

fns.sort(key=lambda x: -x[1])
structs.sort(key=lambda x: -x[1])
print(json.dumps({"fns": fns[:5], "structs": structs[:5]}))
PY

now=$(python3 -c "$MEASURE" 2>/dev/null) || exit 0
[ -n "$now" ] || exit 0

prev=""
[ -f "$STATE" ] && prev=$(cat "$STATE" 2>/dev/null)

report=$(NOW="$now" PREV="$prev" python3 <<'PY'
import json, os

now = json.loads(os.environ["NOW"])
try:
    prev = json.loads(os.environ["PREV"])
except Exception:
    prev = None

def delta(kind, name, value):
    if not prev:
        return ""
    old = dict(prev.get(kind, []))
    if name not in old:
        return "  (new)"
    d = value - old[name]
    return f"  ({d:+d})" if d else ""

lines = ["  largest functions:"]
for name, n in now["fns"][:3]:
    lines.append(f"    {name}  {n} lines{delta('fns', name, n)}")
lines.append("  widest structs:")
for name, n in now["structs"][:3]:
    lines.append(f"    {name}  {n} fields{delta('structs', name, n)}")

moved = []
if prev:
    for kind, unit in (("fns", "lines"), ("structs", "fields")):
        old = dict(prev.get(kind, []))
        for name, value in now[kind]:
            if name in old and value != old[name]:
                moved.append(f"  {name}  {old[name]} → {value} {unit}")

if moved:
    lines.append("  moved since last session:")
    lines += moved
print("\n".join(lines))
PY
) || exit 0

mkdir -p .claude
printf '%s' "$now" > "$STATE"

echo "🏗️  Structure watch (organon#618 T0c — a report, not a gate):"
echo "$report"
echo "  Length is NOT the metric — #618 T4 keeps frame_body long on purpose."
echo "  Field count is: it bounds what any one region can reach. Partition, don't chop."
echo "  ⚠️ A flat serde mirror (PresetValues) is a different animal from a struct a long"
echo "     function reaches through (World). Wide is only a problem for the second kind."
