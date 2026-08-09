#!/usr/bin/env bash
# SessionStart hook (project-level) — load ARCHITECTURE.md into every new session.
#
# Why: the harness auto-injects CLAUDE.md at session start, but NOT
# ARCHITECTURE.md — the durable, code-grounded technical reference (the
# two-process IPC model, the Shared snapshot, the generator contract, the param
# single-source-of-truth, the render pipeline, the "how to add X" guide). A prose
# instruction to "read ARCHITECTURE.md" is not a mechanism; this hook is. On
# SessionStart, a hook's stdout is added to the session context, so cat-ing the
# doc here guarantees it is in context from the first turn.
#
# Fires on: startup / clear / compact (fresh or reset contexts) — see the matcher
# in .claude/settings.json. Not on resume (that context is already restored).
#
# Fail-safe by design: any error path exits 0 with no output, so a hiccup in the
# hook never blocks or delays a session — it just falls back to the old behaviour
# (the doc not pre-loaded).

root="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
doc="$root/ARCHITECTURE.md"
[ -f "$doc" ] || exit 0

# Read the whole doc FIRST, before emitting anything. If the read fails (or the
# file is empty), exit 0 with no output at all — a truly silent fallback. Emitting
# the header before cat would leave a dangling "authoritative architecture" frame
# with no document behind it if the read failed, which is worse than silence.
content="$(cat "$doc" 2>/dev/null)" || exit 0
[ -n "$content" ] || exit 0

printf '===== BEGIN ARCHITECTURE.md (auto-loaded by the SessionStart hook) =====\n'
printf 'The durable architecture reference below is injected into every new\n'
printf 'session so it is in context from turn one. Treat it as authoritative on\n'
printf 'architecture. Keep it current in the same change whenever the\n'
printf 'architecture shifts.\n'
printf '\n'
printf 'It is the CORE, not the whole reference. Three child docs are deliberately\n'
printf 'NOT injected and are read on demand:\n'
printf '  * doc/arch/render.md    — the render pipeline in depth (the old §9)\n'
printf '  * MIND_ARCHITECTURE.md  — Organon Mind'"'"'s living state\n'
printf '  * SHELL_ARCHITECTURE.md — Organon Shell'"'"'s living state\n'
printf 'Open the relevant one before renderer, Mind or Shell work; §19'"'"'s file map\n'
printf 'says which module lives where.\n\n'
printf '%s\n' "$content"
printf '\n===== END ARCHITECTURE.md =====\n'
exit 0
