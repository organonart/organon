#!/usr/bin/env bash
# SessionStart hook (project-level) — report durable docs that have DRIFTED.
#
# organon#590 Tier 5. The other doc hooks answer "you changed X without updating Y"
# — a same-change gate, scoped to one session. This answers a different question:
# **has Y fallen behind X over time, across everyone's sessions?** Nothing asked that
# before, so nothing would have noticed a doc quietly falling behind its subsystem.
#
# The signal is the SUBJECT's churn, not the calendar. A doc untouched for six months
# is perfectly current if the code it documents did not move — web/ARCHITECTURE.md is
# exactly that case today (last touched 2026-07-18, zero web commits since). Ageing a
# doc by wall-clock would cry wolf on it every session and train everyone to ignore
# the whole mechanism.
#
# Measured against this repo's whole history, the check would never have fired: the
# same-change discipline has been holding. It is a REGRESSION DETECTOR for a practice
# that currently works, not a cleanup tool with a backlog to burn down.
#
# **`doc-rules.sh` owns the threshold and the calibration measurements behind it** —
# deliberately not restated here. A number kept in two places drifts, and this file
# already shipped one debunked figure that way (organon#597 review).
#
# ⚠️ WHAT THIS DOES NOT CATCH, stated plainly so nobody trusts it further than it goes:
# it would NOT have caught the CLAUDE.md rot that started #590. That file was edited
# four times in the fortnight its body sat ~550 PRs out of date — the mirror section,
# the Linux headers, the commit trailer — and every small edit reset the clock. No
# commit-counting signal distinguishes "this file was touched" from "the stale part of
# this file was touched". That failure was DUPLICATION, and it was fixed structurally
# (#591/#594: delete what another doc owns), not by a check. This hook is a backstop
# for a different, also-real failure mode.
#
# Output goes into the session context. It is SILENT when nothing has drifted, so the
# common case costs nothing — which matters, because #590 Tier 3 exists to shrink what
# every session loads.
#
# Fail-safe by design: any error path exits 0 with no output.

root="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
cd "$root" 2>/dev/null || exit 0

# shellcheck source=doc-rules.sh
. "$root/.claude/hooks/doc-rules.sh" 2>/dev/null || exit 0
[ -n "${DOC_RULES:-}" ] || exit 0
threshold="$DOC_STALE_THRESHOLD"   # doc-rules.sh defaults it; env can override

report=""
set -f   # trigger globs are git pathspecs; don't expand them against the tree
while IFS= read -r rule; do
  [ -n "$rule" ] || continue
  doc="${rule%%|*}"
  triggers="${rule#*|}"

  [ -f "$doc" ] || continue

  # When was the doc last committed? (No history yet → nothing to compare against.)
  last="$(git log -1 --format=%H -- "$doc" 2>/dev/null)" || continue
  [ -n "$last" ] || continue

  # How many commits have touched its subject since? git handles the globs as
  # pathspecs, so `native/src/rt*.rs` matches every RT module including future ones.
  # shellcheck disable=SC2086
  n="$(git log --oneline "$last"..HEAD -- $triggers 2>/dev/null | wc -l | tr -d ' ')"
  [ -n "$n" ] || continue
  [ "$n" -gt "$threshold" ] || continue

  when="$(git log -1 --format=%ad --date=short -- "$doc" 2>/dev/null)"
  if [ "$n" -eq 1 ]; then s="1 commit has"; else s="$n commits have"; fi
  report="$report
  - $doc — last updated $when, but $s touched its subject since."
done <<EOF
$DOC_RULES
EOF

[ -n "$report" ] || exit 0

printf '===== DOC DRIFT (auto-checked at session start) =====\n'
printf 'These durable docs have fallen behind the code they describe. Each carries an\n'
printf 'update-in-the-same-change contract, so an outstanding count means real drift,\n'
printf 'not a stylistic backlog:\n'
printf '%s\n\n' "$report"
printf 'Not a gate, and not necessarily this session'"'"'s job. But if your work touches\n'
printf 'one of these areas, bring the doc with you — that is how the count goes down.\n'
printf 'Threshold is %s subject-commits (.claude/hooks/doc-rules.sh).\n' "$threshold"
printf '===== END DOC DRIFT =====\n'
exit 0
