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
# Fail-safe: a hiccup never blocks a session. ⚠️ Fail-safe is NOT fail-silent, and it
# used to be — see the organon#1 T1 note below.

set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 0
[ -f .claude/settings.json ] || exit 0

STATE=".claude/.context-budget.json"

# ⚠️ organon#1 T1: THIS HOOK HAD NEVER RUN ON THE WINDOWS WORKSTATION, and nothing said
# so. Each python3 call was `python3 … 2>/dev/null || exit 0`, so a missing interpreter
# produced exit 0 with zero bytes of output — indistinguishable from a clean report,
# except that a clean report is impossible here: this hook always has a number to
# print. Meanwhile CLAUDE.md asserted the budget line "always prints". It did not, and
# in the gap ARCHITECTURE.md grew 202,276 → 219,695 bytes, past the very budget below.
# python-runner.sh resolves an interpreter by RUNNING candidates and this hook now
# reports when there is none. The lesson is the house one: a status line that cannot be
# wrong is not a status line.
RUNNER="$(dirname "${BASH_SOURCE[0]}")/python-runner.sh"
if [ ! -f "$RUNNER" ]; then
  # Saying nothing here would be the very bug this file was changed to remove.
  echo "⚠️  📄 Context budget (organon#626 T2) did not run: $RUNNER is missing."
  exit 0
fi
# shellcheck source=./python-runner.sh
. "$RUNNER"

if ! py_find; then
  py_unavailable "📄 Context budget (organon#626 T2)" \
    "nobody is pricing what this session injects."
  exit 0
fi

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

# WHICH LOADERS ARE WIRED — the only question worth asking settings.json, and still
# Python's job because settings.json is JSON. stderr is deliberately not swallowed: an
# interpreter is known good by now, so anything on it is a real fault worth seeing.
loaders=$(py_run <<'PY'
import json, os, re, sys

try:
    cfg = json.load(open(".claude/settings.json"))
except Exception:
    sys.exit(1)

for group in cfg.get("hooks", {}).get("SessionStart", []):
    for hook in group.get("hooks", []):
        m = re.search(r"(load-[A-Za-z0-9._-]+\.sh)", hook.get("command", ""))
        if not m:
            continue
        if os.path.isfile(os.path.join(".claude/hooks", m.group(1))):
            print(m.group(1))
PY
)

if [ -z "$loaders" ]; then
  echo "📄 Context budget (organon#626 T2 — a report, not a gate):"
  echo "     Nothing is injected: settings.json wires no load-*.sh SessionStart hook,"
  echo "     or the one it names is missing from .claude/hooks/. If that is a surprise,"
  echo "     the \`load-*\` naming convention IS the contract — see this hook's header."
  exit 0
fi

# ⚠️ WHICH `bash` RUNS THE LOADERS IS PART OF THE MEASUREMENT, which is why this loop
# lives in the shell and not inside the Python. It used to be a
# `subprocess.run(["bash", script])` — correct while the interpreter was a native one,
# and quietly wrong the moment it is reached through WSL, because "bash" then means
# WSL's bash. A loader resolves its own root with `git rev-parse`, and Linux git cannot
# read a Windows-made worktree (its `.git` file holds `C:/…`). Measured in exactly such
# a worktree on 2026-08-20: load-architecture-doc.sh emits 220,439 bytes under Git Bash
# and 0 under WSL bash. Had the fix left this in the Python, it would have re-created
# the silent defect one layer in — a budget of 0 B, reported with total confidence.
#
# Running them here also makes the number honest by construction: this is the same
# bash, the same cwd and the same environment the harness itself uses at SessionStart.
measured=""
n_loaders=0
n_zero=0
zero_names=""
while IFS= read -r name; do
  [ -n "$name" ] || continue
  script=".claude/hooks/$name"
  [ -f "$script" ] || continue

  # Per-loader cap, deliberately well under this hook's own 20 s SessionStart budget in
  # settings.json. An inner timeout EQUAL to the outer one can never usefully fire: the
  # harness would kill this script first, mid-loop, before it prints anything or writes
  # its state file — and the fail-safe design means that failure is silent. At 5 s a
  # pathological loader is skipped, every other loader is still measured, and the report
  # still prints. Today's loaders are `cat`s (~0.1 s).
  #
  # </dev/null because a SessionStart hook is handed a JSON payload on stdin; a loader
  # must never inherit it and block on a read.
  # ⚠️ `timeout` is GNU and **stock macOS does not have it** — this project's primary
  # dev/deploy platform. Calling it unguarded exits 127 with `$tmp` still empty, which
  # this loop cannot tell apart from a loader that ran and printed nothing: the report
  # would then announce NOTHING WAS INJECTED on a session where everything was. That is
  # the same all-zero misdiagnosis this hook now exists to prevent, reached from the
  # other side. `python-runner.sh::py_probe` guards the identical call the same way;
  # `status-week-check.sh` is the repo's standing GNU/BSD convention for hooks.
  tmp=$(mktemp 2>/dev/null) || continue
  if command -v timeout >/dev/null 2>&1; then
    timeout 5 bash "$script" </dev/null >"$tmp" 2>/dev/null
  else
    # No cap available. A loader is a `cat` today; an unbounded one is still better
    # than a false zero, and the harness's own 20 s SessionStart budget is the backstop.
    bash "$script" </dev/null >"$tmp" 2>/dev/null
  fi
  st=$?
  if [ "$st" -eq 124 ]; then
    rm -f "$tmp"
    continue
  fi
  # Count regardless of exit status, matching the old subprocess behaviour: a loader
  # that exits non-zero after printing has still injected those bytes. `wc -c < file`
  # rather than `${#out}` — command substitution eats trailing newlines and `${#}`
  # counts characters, so both would undercount a UTF-8 doc full of ⚠️ and §.
  n=$(wc -c <"$tmp" 2>/dev/null | tr -d '[:space:]')
  rm -f "$tmp"
  [ -n "$n" ] || continue
  n_loaders=$((n_loaders + 1))
  if [ "$n" -eq 0 ]; then
    n_zero=$((n_zero + 1))
    zero_names="${zero_names}${zero_names:+, }${name}"
  fi
  measured="${measured}${name}	${n}
"
done <<EOF
$loaders
EOF

# ⚠️ A WIRED LOADER THAT PRINTS NOTHING IS A BROKEN LOADER, NOT A ZERO BUDGET, and
# saying "0 B ≈ 0.0k tokens (0% of budget)" would be the same silent-failure shape this
# whole change is about — one layer further out. Found while proving the failure branch
# on 2026-08-20: with `git` off PATH, load-architecture-doc.sh takes its `|| exit 0`
# path and emits nothing, and the old arithmetic reported 0 B as settled fact. Every
# loader here exits 0 on its failure paths BY DESIGN (silence beats a dangling header),
# which is right for the loader and is precisely why the budget cannot read silence as
# a measurement. So: all-zero is a diagnosis, not a number.
if [ "$n_loaders" -gt 0 ] && [ "$n_zero" -eq "$n_loaders" ]; then
  echo "⚠️  📄 Context budget (organon#626 T2): NOTHING WAS INJECTED THIS SESSION."
  echo "     $n_loaders wired loader(s) ran and printed 0 bytes: $zero_names"
  echo "     That is not a budget of zero — it means the loader bailed. Each exits 0"
  echo "     silently when it cannot resolve its root, so check \`git rev-parse\` works"
  echo "     here: a git worktree driven by the wrong git is the usual cause."
  echo "     Reproduce with: bash .claude/hooks/load-architecture-doc.sh | wc -c"
  exit 0
fi

if [ -z "$measured" ]; then
  echo "📄 Context budget (organon#626 T2 — a report, not a gate):"
  echo "     $(printf '%s\n' "$loaders" | grep -c . | tr -d '[:space:]') loader(s) are wired, but none produced a measurement."
  echo "     Reproduce with: bash .claude/hooks/context-budget-check.sh"
  exit 0
fi

prev=""
[ -f "$STATE" ] && prev=$(cat "$STATE" 2>/dev/null)

# One Python pass does the arithmetic AND writes the state file. It used to print a
# JSON blob that two further `python3 -c` calls immediately re-parsed in order to split
# report from state — three interpreter round trips to move data three lines. Harmless
# when python3 was a local process; wasteful now that a round trip may cross into WSL.
export MEASURED="$measured" PREV="$prev" BUDGET="$CONTEXT_BUDGET_BYTES" \
       BPT="$BYTES_PER_TOKEN" TODAY="$(date +%Y-%m-%d)" STATE_PATH="$STATE"
report=$(py_run MEASURED PREV BUDGET BPT TODAY STATE_PATH <<'PY'
import json, os

# name<TAB>bytes, one loader per line, measured by the shell above.
now = {}
for line in os.environ["MEASURED"].splitlines():
    if not line.strip():
        continue
    name, _, count = line.rpartition("\t")
    try:
        now[name] = int(count)
    except ValueError:
        continue

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

if total > budget:
    lines.append(f"  ⚠️  Over the {budget:,} B budget. Growth is normal — same-change doc")
    lines.append("     discipline is why. What is not normal is nobody deciding it.")
    lines.append("     Move a subsystem section to doc/arch/ (the #590 T3 pattern) or")
    lines.append("     raise the budget in this hook on purpose, with a reason.")

state = {"total": total, "per_hook": now, "first": first,
         "first_date": first_date, "peak": peak}
try:
    os.makedirs(".claude", exist_ok=True)
    with open(os.environ["STATE_PATH"], "w") as fh:
        fh.write(json.dumps(state))
except OSError:
    # A read-only tree loses the session-over-session delta and nothing else. The
    # number in front of you is still correct, so report it rather than bailing.
    pass

print("\n".join(lines))
PY
)

if [ -z "$report" ]; then
  echo "⚠️  📄 Context budget (organon#626 T2) measured the loaders but could not report."
  echo "     $PY_LABEL ran and returned nothing from the arithmetic pass."
  echo "     Reproduce with: bash .claude/hooks/context-budget-check.sh"
  exit 0
fi

echo "📄 Context budget (organon#626 T2 — a report, not a gate):"
echo "$report"
# The partial case of the all-zero diagnosis above: the total is real, but one loader
# contributed nothing to it and the per-hook breakdown would show it as a bare 0 B.
if [ "$n_zero" -gt 0 ]; then
  echo "  ⚠️  $n_zero wired loader(s) printed 0 bytes and are missing from that total:"
  echo "     $zero_names — a loader that bails exits 0 silently. Not a zero cost."
fi
