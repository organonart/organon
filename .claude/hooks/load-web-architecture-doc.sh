#!/usr/bin/env bash
# ⚠️ NOT WIRED. This hook is UNREGISTERED on purpose — do not "fix" it. (#626 Tier 2)
#
# #418 (the WebGPU port) is parked, so as of #626 Tier 2 this hook's entry was
# removed from the SessionStart array in .claude/settings.json. It injected
# **16,500 bytes** into every single session — measured, and the largest fixed
# cost the park was still paying — for a program nobody is working on.
#
# THE FILE STAYS, UNMODIFIED, AND THAT IS THE POINT. Resuming #418 is re-adding
# one entry to `.claude/settings.json`'s SessionStart array:
#
#   {
#     "type": "command",
#     "command": "bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/load-web-architecture-doc.sh\"",
#     "timeout": 15,
#     "statusMessage": "Loading web/ARCHITECTURE.md into the session…"
#   }
#
# Deleting this file instead would have made the park a one-way door for no
# saving — an unregistered hook costs nothing at rest. Everything below is the
# original, working, still-correct implementation.
#
# ── original header ────────────────────────────────────────────────────────────
# SessionStart hook (project-level) — load web/ARCHITECTURE.md into every new session.
#
# The web app has its own durable architecture reference (raw-WebGPU render core +
# React UI, the contracts/seams, the WASM generator bridge, the manifest codegen,
# the render loop). It is the counterpart to the root ARCHITECTURE.md (native).
# The harness auto-injects CLAUDE.md but not this, so this hook cat-s it into the
# session context from turn one.
#
# Fires on: startup / clear / compact (see the matcher in .claude/settings.json).
#
# Fail-safe: any error path exits 0 with no output, so a hiccup never blocks a
# session — it just falls back to the doc not being pre-loaded.

root="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
doc="$root/web/ARCHITECTURE.md"
[ -f "$doc" ] || exit 0

content="$(cat "$doc" 2>/dev/null)" || exit 0
[ -n "$content" ] || exit 0

printf '===== BEGIN web/ARCHITECTURE.md (auto-loaded by the SessionStart hook) =====\n'
printf 'The durable architecture reference for the WEB app (web/) is injected below\n'
printf 'so it is in context from turn one. It is authoritative on the web app; the\n'
printf 'root ARCHITECTURE.md is authoritative on native. Keep this current in the\n'
printf 'same change whenever the web architecture shifts.\n\n'
printf '%s\n' "$content"
printf '\n===== END web/ARCHITECTURE.md =====\n'
exit 0
