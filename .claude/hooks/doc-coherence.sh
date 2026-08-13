#!/usr/bin/env bash
# Does a durable doc still agree with ITSELF? (organon#618 Tier 0b)
#
# 📌 web/ARCHITECTURE.md IS STILL IN THIS HOOK'S ARGUMENT LIST, on purpose, even
# though #418 is parked (#626 Tier 2). The list lives in `.claude/settings.json`'s
# Stop array — JSON, so it cannot carry the explanation, which is why it is here.
# This hook only reads the files it is handed and only speaks when one of them
# contradicts itself; a parked doc that nobody edits simply never trips it. Dropping
# it from the list would save nothing measurable and would mean a stale-but-parked
# doc could rot silently into an incoherent state while unwatched — the exact
# failure mode #618 T0b exists to catch. Parked is not unwatched.
#
# 📌 SHELL_ARCHITECTURE.md and CONSOLE_ARCHITECTURE.md are BOTH in the list, and only
# one of them exists at a time — the Shell→Console rename lands on its own branch, and
# a hook that has to be edited in the same commit as a rename is a hook that will be
# wrong for whichever branch you are not on. `[ -f "$f" ] || continue` below makes a
# listed-but-absent doc a silent skip, so the list is correct on both sides of it.
#
# The existing doc hooks answer two questions:
#   architecture-doc-check.sh   (Stop)         "you changed X without Y"
#   doc-staleness-check.sh      (SessionStart) "Y has fallen behind X over time"
#
# Both are about FRESHNESS. Neither notices when a doc that was dutifully updated has
# become internally contradictory — which is what actually happened one week after
# #590 closed, and what the 2026-08 review found by reading:
#
#   * §19's file map carried TWO rows each for `ui_layer.rs` and `baseview_input.rs`,
#     one written before #593 T3 and one after. Both live. For `baseview_input.rs`
#     they disagreed on a plain matter of fact (whether `lib.rs` declares it).
#   * A stray `---` split the same table in half, and the table was terminated by a
#     closing ``` fence that was never opened.
#
# Nobody was careless. The doc simply grew past the length at which a human re-reads
# it end to end to check consistency, and the automation was built to enforce that it
# was touched. So: check the two properties that are mechanically checkable and that
# both current defects fall under.
#
#   1. DUPLICATE TABLE KEYS — the same first-column cell twice in one table. In a
#      file-map or rule table the first column is the key, and two rows for one key
#      means one of them is stale. This is the check that would have caught #593 T3.
#   2. UNBALANCED CODE FENCES — an odd number of ``` lines. A stray fence silently
#      swallows or escapes the rest of the document when rendered.
#
# Usage:  doc-coherence.sh FILE...        (exit 1 and print findings if any)
# Wired into architecture-doc-check.sh (Stop). Safe to run by hand or in CI.

set -uo pipefail

status=0

for f in "$@"; do
  [ -f "$f" ] || continue

  # --- 1. duplicate first-column table keys -------------------------------------
  # Scoped PER TABLE, and to keys that are a bare code span (`foo.rs`).
  #
  # File scope was the original choice and it cried wolf on every Stop: ARCHITECTURE.md
  # §18 legitimately names the same eleven `World` clusters in an inventory table and
  # again in a binding-measurement table, which is correct authoring, not a stale row.
  # A hook that fires unconditionally teaches everyone to dismiss hooks, so it was
  # catching nothing while costing attention on every turn.
  #
  # ⚠️ A TABLE ENDS AT ITS NEXT SEPARATOR ROW, not at the first non-table line — and
  # that distinction is the whole reason file scope was picked in the first place. The
  # #593 T3 defect hid behind a second defect: a stray `---` split §19's file map in
  # half and carried the two duplicate rows into different halves. A stray rule opens
  # no new header, so both halves stay one scope here and the pair is still caught;
  # only a real `|---|` separator — which two genuinely separate tables always have —
  # starts a new one.
  #
  # Restricting to code-span keys keeps it quiet either way: those are the file-map /
  # rule-table identifiers, where one row per key is the whole contract. Prose or
  # numeric first columns (the generator table's `| 0 |`, the edition table's
  # `| Full |`) legitimately repeat and are ignored.
  #
  # Fenced blocks are excluded so an ASCII diagram containing pipes cannot produce a
  # false positive.
  dupes=$(awk '
    /^```/ { infence = !infence; next }
    infence { next }
    /^\|/ {
      if ($0 ~ /^\|[ \t:|-]+$/ && $0 ~ /-/) { table++; next }   # separator → new scope
      line = $0
      sub(/^\|[ \t]*/, "", line)
      sub(/[ \t]*\|.*$/, "", line)
      gsub(/^[ \t]+|[ \t]+$/, "", line)
      if (line == "") next
      if (line !~ /^`[^`]+`$/) next      # code-span keys only
      k = table SUBSEP line
      seen[k]++
      at[k] = (seen[k] == 1) ? NR : at[k] "," NR
      if (seen[k] == 2) order[++n] = k
    }
    END {
      for (i = 1; i <= n; i++) { split(order[i], p, SUBSEP); print p[2] "\t" at[order[i]] }
    }
  ' "$f")

  if [ -n "$dupes" ]; then
    status=1
    echo "‼️  $f — duplicate table rows (same first column twice in one table):"
    while IFS=$'\t' read -r key lines; do
      [ -z "$key" ] && continue
      echo "      $key   (lines $lines)"
    done <<< "$dupes"
    echo "    One of each pair is stale. Delete it — do not merge them."
  fi

  # --- 2. code-fence balance ----------------------------------------------------
  fences=$(grep -c '^```' "$f" || true)
  if [ $((fences % 2)) -ne 0 ]; then
    status=1
    last=$(grep -n '^```' "$f" | tail -1 | cut -d: -f1)
    echo "‼️  $f — $fences code fences (odd). The last one is at line $last."
    echo "    An unmatched fence swallows or escapes everything after it when rendered."
  fi
done

exit $status
