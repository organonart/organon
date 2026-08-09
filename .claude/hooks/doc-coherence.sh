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
  # Scoped to the whole FILE, and to keys that are a bare code span (`foo.rs`).
  #
  # Both choices are deliberate, and the first one is the lesson of the very defect
  # this check was written for. Scoping per-table looks more precise and is worse:
  # the stray `---` that split §19's file map in half ALSO split the two duplicate
  # rows into different tables, so a per-table check saw nothing. The defect hid
  # itself behind the other defect. File scope cannot be fooled that way.
  #
  # Restricting to code-span keys is what keeps file scope quiet: those are the
  # file-map / rule-table identifiers, where one row per key is the whole contract.
  # Prose or numeric first columns (the generator table's `| 0 |`, the edition
  # table's `| Full |`) legitimately repeat across tables and are ignored.
  #
  # Separator rows are skipped, and fenced blocks are excluded so an ASCII diagram
  # containing pipes cannot produce a false positive.
  dupes=$(awk '
    /^```/ { infence = !infence; next }
    infence { next }
    /^\|/ {
      line = $0
      sub(/^\|[ \t]*/, "", line)
      sub(/[ \t]*\|.*$/, "", line)
      gsub(/^[ \t]+|[ \t]+$/, "", line)
      if (line ~ /^-+$/ || line == "") next
      if (line !~ /^`[^`]+`$/) next      # code-span keys only
      seen[line]++
      if (seen[line] == 2) print line
    }
  ' "$f")

  if [ -n "$dupes" ]; then
    status=1
    echo "‼️  $f — duplicate table rows (same first column twice in one table):"
    while IFS= read -r key; do
      [ -z "$key" ] && continue
      lines=$(grep -n -- "^| *${key//\//\\/} *|" "$f" | cut -d: -f1 | tr '\n' ',' | sed 's/,$//')
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
