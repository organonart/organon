#!/usr/bin/env bash
# Stop hook (project-level) — remind the session to keep ARCHITECTURE.md current.
#
# Fires when an architecturally significant source file changed this session but
# ARCHITECTURE.md did not. It is a *one-time nudge*, not a hard gate: it blocks
# the first stop with a reminder, and if the model stops again
# (`stop_hook_active`), it allows the stop. So a genuinely non-architectural
# change is dismissed by simply stopping a second time.
#
# Fail-safe by design: any error path exits 0 (allow stop) so a hiccup in the
# hook never traps a session.

input="$(cat 2>/dev/null)"

# Loop guard: if we already nudged and the model is stopping again, let it stop.
case "$input" in
  *'"stop_hook_active":true'* | *'"stop_hook_active": true'*) exit 0 ;;
esac

# CI guard: this nudge is for interactive dev sessions that can still edit the
# doc. In GitHub Actions the only Claude that runs is the read-only PR reviewer
# (.github/workflows/claude-review.yml) — it has no Edit/Write tool, so blocking
# its stop can't produce a doc update. It just costs turns and denied tool calls
# until the run dies on --max-turns. The reviewer checks this rubric item itself
# (see .github/organon-review-guide.md, "Architecture doc discipline").
[ -n "${GITHUB_ACTIONS:-}" ] && exit 0

root="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
cd "$root" 2>/dev/null || exit 0

# The doc→source mapping lives in ONE place, shared with doc-staleness-check.sh.
# See doc-rules.sh for the format and for why the triggers are what they are.
# shellcheck source=doc-rules.sh
. "$root/.claude/hooks/doc-rules.sh" 2>/dev/null || exit 0
rules="$DOC_RULES"
[ -n "$rules" ] || exit 0

# Likewise "changed this session" — shared with web-architecture-doc-check.sh.
# shellcheck source=session-changes.sh
. "$root/.claude/hooks/session-changes.sh" 2>/dev/null || exit 0

# Files THIS SESSION changed: commits whose committer date falls inside this
# session and which aren't already on origin/main, plus staged + unstaged +
# untracked. The session boundary comes from the first timestamp in the
# transcript named by the stdin payload's `transcript_path`; if that can't be
# read, the committed leg is dropped and only the working tree counts. It is
# deliberately NOT the branch-vs-main diff that used to live here — on a
# long-lived branch that reported every earlier session's work as this one's.
# session-changes.sh owns the reasoning and the measurements.
changed="$(session_changed_files "$input")"

reason=""
set -f   # no pathname expansion: trigger globs must reach the matcher intact
while IFS= read -r rule; do
  [ -n "$rule" ] || continue
  doc="${rule%%|*}"
  triggers="${rule#*|}"

  hit=""
  for f in $triggers; do
    case "$f" in
      *'*'*)
        # glob → ERE: escape dots, then `*` matches within one path segment.
        rx="^$(printf '%s' "$f" | sed -e 's/\./\\./g' -e 's/\*/[^\/]*/g')\$"
        m="$(printf '%s\n' "$changed" | grep -E "$rx")" ;;
      *)
        m="$(printf '%s\n' "$changed" | grep -xF "$f")" ;;
    esac
    if [ -n "$m" ]; then
      hit="$hit $(printf '%s' "$m" | tr '\n' ' ')"
    fi
  done

  # Trigger changed but this doc did not → add it to the reminder.
  [ -n "$hit" ] || continue
  printf '%s\n' "$changed" | grep -qxF "$doc" && continue

  # Cap the listed files: a sweep across many shaders shouldn't emit a wall of text.
  set -- $hit
  [ "$#" -gt 4 ] && hit="$1 $2 $3 $4 (+$(($# - 4)) more)"

  # No case for web/ARCHITECTURE.md on purpose — web-architecture-doc-check.sh owns its
  # same-change reminder. A doc with no case here contributes nothing, which is how a
  # rule can be staleness-only. Don't "fix" this by adding one; it would double-remind.
  case "$doc" in
    ARCHITECTURE.md)
      reason="$reason Architecturally significant files changed this session ($hit ) but ARCHITECTURE.md was not updated. If these changes added or modified a generator (GeneratorMode), a Shared/IPC block, a RenderPath, a param block, a material, or a world layer, update ARCHITECTURE.md (its tables, counts, and file map) in this same change." ;;
    doc/arch/render.md)
      reason="$reason Render-pipeline files changed this session ($hit ) but doc/arch/render.md was not updated. That is the #590 Tier 3 split-out of the old ARCHITECTURE.md §9 and it is NOT auto-injected, so nothing else will remind you: if this change added or altered a pass, a RenderPath, the RenderFrame seam, an rt_* stage, or the IBL/shader inventory, update it in this same change. Structural changes may also need ARCHITECTURE.md §9's altitude summary." ;;
    MIND_ARCHITECTURE.md)
      reason="$reason Organon Mind files changed this session ($hit ) but MIND_ARCHITECTURE.md was not updated. It is the living state doc for what Mind does RIGHT NOW, and it carries the honesty ledger — so if this change added or altered a lens, a readout, the activation path, or the edition shell, update it in this same change, and give any new displayed quantity its provenance marker (measured / derived / proxy / projection)." ;;
    SHELL_ARCHITECTURE.md)
      reason="$reason Organon Shell files changed this session ($hit ) but SHELL_ARCHITECTURE.md was not updated. It is Shell's living state doc (the PRD and build plan are in doc/, private; the code-grounded what-exists-now lives here and is the only Shell doc that goes public) — if this change added or altered a panel, a surface, the command seam, or the edition wiring, update it in this same change." ;;
  esac
done <<EOF
$rules
EOF

if [ -n "$reason" ]; then
  reason="${reason# } If the change does not affect the doc, stop again to dismiss this reminder."
  printf '{"decision":"block","reason":"%s"}\n' "$reason"
fi

exit 0
