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

# 🚨 THE WHOLE FENCED BLOCK IS COMPARED, not the block from some anchor downwards —
# and that distinction is a fix, not a preference. The first cut of this hook started
# grabbing at the first leg (`cargo test  -p organon-console --lib`) and ran to the
# closing fence, which left everything ABOVE the first leg outside the comparison. It
# shipped in that state with a `cd native` duplicated in BRIEF.md's copy, and the check
# reported the two copies identical, because the divergence sat in its blind spot. The
# hook was mutation-tested before it landed — but only by mutating lines after the
# anchor, which is a test that confirms the thing it was built from.
#
# 📌 So the anchor now selects WHICH block, and the whole of that block is compared. The
# file is scanned fence to fence; a block is buffered as it is read; the block that
# CONTAINS the anchor line is the one printed. Anchoring on a line inside the block
# rather than on a preceding heading is still right — either file's prose may be
# rewritten freely, and only the commands must agree — but "which block" and "what is
# compared" are now two different questions, which is what the first version conflated.
#
# ⚠️ Note that CONTRIBUTING.md has a SECOND `cd native` block (the `--workspace` bar,
# ~line 74), so `cd native` cannot itself be the anchor. The anchor has to be a line
# unique to the seven-leg block.
extract() {
  awk '
    /^```/ {
      if (infence) {
        if (found) { printf "%s", buf; exit }
        infence = 0; buf = ""; found = 0
      } else {
        infence = 1; buf = ""; found = 0
      }
      next
    }
    infence {
      buf = buf $0 "\n"
      if ($0 == "cargo test  -p organon-console --lib") found = 1
    }
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
