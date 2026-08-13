#!/usr/bin/env bash
# 📌 STILL WIRED, DELIBERATELY, even though #418 is parked. (#626 Tier 2)
#
# Tier 2 unregistered the SessionStart *injection* of web/ARCHITECTURE.md and left
# this Stop check alone. That asymmetry is the whole design, so it is written down
# here rather than left to look like a half-finished job:
#
#   the SessionStart injection was the EXPENSIVE half — 16,500 bytes in every
#   session, paid unconditionally, whether or not a web file was ever touched.
#
#   this Stop check is the CHEAP half — it only produces output when one of the
#   trigger files below actually changes, which under the park is approximately
#   never. Its resting cost is one `git diff --name-only` per stop.
#
# Keeping it is also what keeps the park honest: if someone does touch a web
# contract or the WASM ABI, the reminder still fires and web/ARCHITECTURE.md still
# gets updated in the same change. Parked is not abandoned.
#
# ── original header ────────────────────────────────────────────────────────────
# Stop hook (project-level) — remind the session to keep web/ARCHITECTURE.md current.
#
# Fires when a load-bearing WEB source file changed this session but
# web/ARCHITECTURE.md did not. A one-time nudge, not a hard gate: it blocks the
# first stop with a reminder; if the model stops again (stop_hook_active), it
# allows the stop. So a genuinely non-architectural change is dismissed by
# stopping a second time.
#
# Fail-safe: any error path exits 0 (allow stop) so a hiccup never traps a session.

input="$(cat 2>/dev/null)"

# Loop guard: if we already nudged and the model is stopping again, let it stop.
case "$input" in
  *'"stop_hook_active":true'* | *'"stop_hook_active": true'*) exit 0 ;;
esac

# CI guard: see architecture-doc-check.sh. The read-only PR reviewer that runs in
# GitHub Actions has no Edit/Write tool, so blocking its stop burns turns instead
# of updating the doc.
[ -n "${GITHUB_ACTIONS:-}" ] && exit 0

root="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
cd "$root" 2>/dev/null || exit 0

# Web source files whose change implies web/ARCHITECTURE.md should be revisited:
# the contracts/seams, the render core, the WASM bridge ABI, the store, the
# manifest codegen. (Deliberately narrow — routine UI/param tweaks don't trigger.)
triggers="web/src/contracts/sharedState.ts web/src/contracts/generatorOutput.ts web/src/contracts/renderer.ts web/src/contracts/stateSource.ts web/src/render/pbrRenderer.ts web/src/render/webgpuRenderer.ts web/src/state/store.ts native/organon-wasm/src/lib.rs native/organon-manifest/src/lib.rs"
doc="web/ARCHITECTURE.md"

# Files THIS SESSION changed — same definition as architecture-doc-check.sh, and
# now literally the same code: commits made inside this session that aren't yet
# on origin/main, plus staged + unstaged + untracked. This file carried a
# byte-identical copy of the branch-vs-main version and therefore the identical
# false positive; it was merely latent here, because the park means these
# triggers approximately never fire. session-changes.sh owns the reasoning.
# shellcheck source=session-changes.sh
. "$root/.claude/hooks/session-changes.sh" 2>/dev/null || exit 0
changed="$(session_changed_files "$input")"

hit=""
for f in $triggers; do
  if printf '%s\n' "$changed" | grep -qxF "$f"; then
    hit="$hit $f"
  fi
done

# Trigger changed but the doc did not → emit a one-time reminder.
if [ -n "$hit" ] && ! printf '%s\n' "$changed" | grep -qxF "$doc"; then
  reason="Load-bearing web files changed this session ($hit ) but web/ARCHITECTURE.md was not updated. If these changes added or modified a contract/seam, a renderer, the WASM generator ABI, the store, or the manifest codegen, update web/ARCHITECTURE.md (its data flow, subsystem sections, and file map) in this same change before finishing. If the change is not architectural, stop again to dismiss this reminder."
  printf '{"decision":"block","reason":"%s"}\n' "$reason"
  exit 0
fi

exit 0
