#!/usr/bin/env bash
# Do the two published copies of the seven-leg verification bar still agree?
#
# The bar exists in two places ON PURPOSE, and this hook is the price of that:
#
#   CONTRIBUTING.md                              the human-facing process doc
#   .claude/skills/coordinate-sessions/BRIEF.md  the copy a WORKER session is handed
#
# Neither can be dropped. A contributor reading CONTRIBUTING.md must find the bar
# where the rest of the process is; a worker session cannot load the coordinator's
# skill and needs a file it can `git show` from a checkout it already has. Making
# one file `include` the other is not available in Markdown that both a human and a
# `git show` render.
#
# 📌 So this is `organon-module`'s ROW_ALIGNMENT move (CONSOLE_ARCHITECTURE.md §1.20):
# the constant is duplicated, and the AGREEMENT is tested. Duplication that a check
# pins is a copy; duplication that nothing pins is a fork waiting to happen — and this
# particular fork has already happened once. The six-command version of the bar
# circulated in briefs and handoffs for months while CONTRIBUTING.md's copy was right,
# and the divergence was invisible because nobody diffs prose.
#
# ⚠️ The check is on the COMMAND BLOCK only, not the prose around it. The two copies
# are addressed to different readers and their surrounding paragraphs SHOULD differ;
# what must never differ is which commands you are told to run.
#
# Usage:  bar-agreement-check.sh          (exit 1 and print a diff if they disagree)
# Wired into Stop. Safe to run by hand or in CI.

set -uo pipefail

root="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || echo .)}"
a="$root/CONTRIBUTING.md"
b="$root/.claude/skills/coordinate-sessions/BRIEF.md"

# A listed file that cannot exist is false reassurance, not tolerance — say so rather
# than exiting 0 the way an existence guard would. (doc-coherence.sh's note on the
# SHELL_ARCHITECTURE.md entry is the same argument.)
for f in "$a" "$b"; do
  if [ ! -f "$f" ]; then
    echo "‼️  bar-agreement-check: ${f#$root/} is missing — the bar has only one copy left."
    exit 1
  fi
done

# The block runs from the first leg to the closing fence. Anchoring on the first
# COMMAND rather than on a heading keeps this working when either file's prose is
# rewritten, which is the whole point of checking the commands and not the paragraphs.
extract() {
  awk '
    /^cargo test  -p organon-console --lib$/ { grab = 1 }
    grab && /^```/                           { exit }
    grab                                     { print }
  ' "$1"
}

bar_a=$(extract "$a")
bar_b=$(extract "$b")

if [ -z "$bar_a" ] || [ -z "$bar_b" ]; then
  echo "‼️  bar-agreement-check: could not find the bar in one of the two copies."
  [ -z "$bar_a" ] && echo "      not found in CONTRIBUTING.md"
  [ -z "$bar_b" ] && echo "      not found in .claude/skills/coordinate-sessions/BRIEF.md"
  echo "    Both must carry a fenced block starting at 'cargo test  -p organon-console --lib'."
  exit 1
fi

if [ "$bar_a" != "$bar_b" ]; then
  echo "‼️  The verification bar has forked."
  diff <(printf '%s\n' "$bar_a") <(printf '%s\n' "$bar_b") \
    | sed 's/^/      /'
  echo "    < CONTRIBUTING.md    > .claude/skills/coordinate-sessions/BRIEF.md"
  echo "    Decide which is right and make the other match. A worker is handed the second."
  exit 1
fi

exit 0
