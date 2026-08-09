#!/usr/bin/env bash
# Stop hook (project-level) — weekly STATUS.md rollover.
#
# Protocol: STATUS.md holds the *current week's* session handoff. When a
# session runs in a later ISO week than STATUS.md's most recent "Last updated"
# date, the old file is archived to doc/status-archive/STATUS-<date>.md and a
# fresh STATUS.md is started for the new week (carrying forward the durable
# sections: the header, still-active in-flight entries, "Where we are",
# "Immediate next", "Loose ends", "Quick start"). This hook enforces it: it
# blocks the first stop of a stale-week session with the rollover
# instructions. One-time nudge — stopping again (`stop_hook_active`)
# dismisses it, so a trivial session isn't trapped.
#
# Fail-safe by design: any error path exits 0 (allow stop). Portable across
# GNU date (Linux) and BSD date (macOS).

input="$(cat 2>/dev/null)"

case "$input" in
  *'"stop_hook_active":true'* | *'"stop_hook_active": true'*) exit 0 ;;
esac

# CI guard: see architecture-doc-check.sh. STATUS.md rollover is a human-session
# ritual; the read-only PR reviewer in GitHub Actions can't perform it. Without
# this guard every review that ran in a week where STATUS.md had gone stale was
# told to archive STATUS.md — work it has no tools to do.
[ -n "${GITHUB_ACTIONS:-}" ] && exit 0

root="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
cd "$root" 2>/dev/null || exit 0

status_file="STATUS.md"
archive_dir="doc/status-archive"
[ -f "$status_file" ] || exit 0

# First "Last updated" date in the file, e.g. "**Last updated:** 2026-07-08".
last_date="$(grep -m1 -i 'last updated' "$status_file" 2>/dev/null \
  | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' | head -1)"
[ -n "$last_date" ] || exit 0

# ISO week (year-week), GNU first, BSD fallback.
week_of() {
  date -d "$1" +%G-%V 2>/dev/null || date -j -f "%Y-%m-%d" "$1" +%G-%V 2>/dev/null
}
last_week="$(week_of "$last_date")"
this_week="$(date +%G-%V 2>/dev/null)"
[ -n "$last_week" ] && [ -n "$this_week" ] || exit 0
[ "$last_week" = "$this_week" ] && exit 0

reason="New week: $status_file was last updated $last_date (ISO week $last_week; now $this_week). Per the weekly STATUS protocol, archive it before finishing: git mv $status_file $archive_dir/STATUS-$last_date.md, then write a fresh $status_file for the current week — keep the handoff header, carry forward only still-active in-flight entries plus the Where we are / Immediate next / Loose ends / Quick start sections, fold any still-binding decisions into CLAUDE.md or ARCHITECTURE.md, and set Last updated to today. If this session made no changes worth recording, stop again to dismiss this reminder."
printf '{"decision":"block","reason":"%s"}\n' "$reason"
exit 0
