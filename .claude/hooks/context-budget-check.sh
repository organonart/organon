#!/usr/bin/env bash
# SessionStart: report what the SessionStart hooks inject, and how it is moving.
# organon#626 Tier 2. Companion to #618 T0c's structure-drift-check.sh — same
# posture (a number in front of you, never a gate), different subject.
#
# WHY THIS EXISTS
#
# #590 cut ARCHITECTURE.md from 296,157 bytes to 153,407 — a 48% reduction in what
# every session pays before reading a line of code. All five of its tiers were
# ONE-TIME CUTS, and nothing was left watching. Four days later the file measured
# 174,214 bytes: +20,807 (+13.6%), monotonic across every commit in between.
#
# None of that growth was wrong. #617, #621 and #618 T0 all added real architecture,
# and this repo's same-change doc discipline is exactly WHY the file grows — a doc
# that never grew would be a doc nobody was updating. The gap is that nothing prices
# it. Three checks already ask three questions about the docs:
#
#   doc-staleness-check.sh    has this doc fallen behind the code?
#   doc-coherence.sh          does this doc still agree with itself?
#   structure-drift-check.sh  is any one function or struct getting too big?
#
# and none of them asks: is the thing we inject into EVERY session getting bigger?
# That is the regression #590 exists to prevent, and it was already recurring.
#
# ⚠️ SMALLER IS NOT THE GOAL. The injected core earns its place — §17's extension
# guide and §19's file map are the highest-value pages in the repo and the cheapest
# to read. The goal is that growth be a DECISION rather than an accretion, which
# needs only that someone can see it happening.
#
# HOW IT MEASURES, and why it is not a second table
#
# It reads .claude/settings.json for the SessionStart hooks whose script is named
# `load-*`, runs each, and counts the bytes. So it measures the REAL injected cost
# — content plus banner — and it self-updates: wire a new injection hook and it is
# counted, unwire one (as #626 T2 did for web) and it stops being counted. Nothing
# here names a document, which is deliberate; a hardcoded list of "the injected
# docs" is precisely the duplicate-that-rots #590 is about.
#
# ⚠️ `load-*` IS THE CONTRACT, and the reason is safety rather than naming — worth
# stating, because "why not just measure every SessionStart hook?" is the obvious
# question and the answer is not obvious. **Because measuring means EXECUTING, and
# only the loaders are safe to run twice.** `structure-drift-check.sh` writes
# `.claude/.structure-drift.json`; running it from in here would let it consume its
# own state, and it would then report "nothing moved" to the real session every
# time. A `load-*` script is a pure `cat` — idempotent, no state, safe to run for
# measurement. So the convention is not decoration — it is a three-part contract:
#
#     A SessionStart hook that INJECTS must
#       1. be named `load-*`          — or the budget never finds it;
#       2. be pure (no state writes)  — or measuring it corrupts it;
#       3. resolve its own root       — `$CLAUDE_PROJECT_DIR` is NOT set for the
#          subprocess spawned here, so a loader that depends on it would report a
#          different size than it injects in production. Both current loaders use
#          `git rev-parse --show-toplevel`; match them.
#
# Follow all three and the budget counts you automatically. Break any of them and
# you are silently uncounted or miscounted — the two gaps #637's review correctly
# identified, written here so nobody hits either by accident.
#
# (No recursion risk either: only `load-*` scripts are executed, and this one is
# not one. That falls out of the same rule.)
#
# Fail-safe: every error path exits 0 silently, so a hiccup never blocks a session.

set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 0
[ -f .claude/settings.json ] || exit 0

STATE=".claude/.context-budget.json"

# The ceiling at which the report starts saying something instead of just counting.
# 200,000 bytes ≈ 50k tokens. Calibrated between two measured points rather than
# picked: the post-#590 low-water mark is 153,407 and the pre-#590 peak that issue
# was filed about is 296,157. A budget below the low-water mark would fire on day
# one; one above the peak would never have fired at all. 200k sits ~30% above where
# #590 left things — enough headroom for normal same-change growth, low enough to
# speak up long before the old peak returns.
CONTEXT_BUDGET_BYTES="${CONTEXT_BUDGET_BYTES:-200000}"

# Bytes per token. 4 is the usual rough figure for English prose; the report marks
# every token figure with ≈ because this is an estimate, not a tokenizer.
BYTES_PER_TOKEN=4

measured=$(python3 - <<'PY' 2>/dev/null
import json, os, re, subprocess, sys

try:
    cfg = json.load(open(".claude/settings.json"))
except Exception:
    sys.exit(1)

out = {}
for group in cfg.get("hooks", {}).get("SessionStart", []):
    for hook in group.get("hooks", []):
        cmd = hook.get("command", "")
        m = re.search(r"(load-[A-Za-z0-9._-]+\.sh)", cmd)
        if not m:
            continue
        script = os.path.join(".claude/hooks", m.group(1))
        if not os.path.isfile(script):
            continue
        # Per-loader cap, deliberately well under this hook's own 20 s SessionStart
        # budget in settings.json. An inner timeout EQUAL to the outer one can never
        # usefully fire: the harness would kill this script first, mid-loop, before it
        # prints anything or writes its state file — and the fail-safe design means
        # that failure is silent. At 5 s a pathological loader is skipped, every other
        # loader is still measured, and the report still prints. Today's loaders are
        # `cat`s (~0.1 s), so this only bites if a second injection hook returns
        # (#418's `load-web-architecture-doc.sh` is the one candidate).
        try:
            r = subprocess.run(["bash", script], capture_output=True, timeout=5)
        except Exception:
            continue
        out[m.group(1)] = len(r.stdout)

if not out:
    sys.exit(1)
print(json.dumps(out))
PY
) || exit 0
[ -n "$measured" ] || exit 0

prev=""
[ -f "$STATE" ] && prev=$(cat "$STATE" 2>/dev/null)

result=$(NOW="$measured" PREV="$prev" BUDGET="$CONTEXT_BUDGET_BYTES" \
         BPT="$BYTES_PER_TOKEN" TODAY="$(date +%Y-%m-%d)" python3 <<'PY'
import json, os

now = json.loads(os.environ["NOW"])
budget = int(os.environ["BUDGET"])
bpt = int(os.environ["BPT"])
today = os.environ["TODAY"]
total = sum(now.values())

try:
    prev = json.loads(os.environ["PREV"])
except Exception:
    prev = None

first = prev.get("first", total) if prev else total
first_date = prev.get("first_date", today) if prev else today
peak = max(prev.get("peak", total), total) if prev else total
last = prev.get("total") if prev else None

lines = []
tok = total / bpt
lines.append(f"     injected each session   {total:,} B  ≈ {tok/1000:.1f}k tokens"
             f"  ({100*total/budget:.0f}% of budget)")

# Per-hook, only when more than one is wired — with a single injection the breakdown
# just repeats the total.
if len(now) > 1:
    for name, n in sorted(now.items(), key=lambda kv: -kv[1]):
        lines.append(f"       {name}  {n:,} B")

if last is not None and total != last:
    d = total - last
    lines.append(f"     since last session      {d:+,} B")

if total != first:
    d = total - first
    pct = 100 * d / first if first else 0
    lines.append(f"     since {first_date}        {d:+,} B ({pct:+.1f}%)"
                 + (f"   peak {peak:,}" if peak > total else ""))

over = total > budget
if over:
    lines.append(f"  ⚠️  Over the {budget:,} B budget. Growth is normal — same-change doc")
    lines.append("     discipline is why. What is not normal is nobody deciding it.")
    lines.append("     Move a subsystem section to doc/arch/ (the #590 T3 pattern) or")
    lines.append("     raise the budget in this hook on purpose, with a reason.")

print(json.dumps({
    "report": "\n".join(lines),
    "state": {"total": total, "per_hook": now, "first": first,
              "first_date": first_date, "peak": peak},
}))
PY
) || exit 0
[ -n "$result" ] || exit 0

report=$(printf '%s' "$result" | python3 -c 'import json,sys;print(json.load(sys.stdin)["report"])' 2>/dev/null) || exit 0
state=$(printf '%s' "$result" | python3 -c 'import json,sys;print(json.dumps(json.load(sys.stdin)["state"]))' 2>/dev/null) || exit 0

mkdir -p .claude
printf '%s' "$state" > "$STATE"

echo "📄 Context budget (organon#626 T2 — a report, not a gate):"
echo "$report"
