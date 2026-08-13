#!/usr/bin/env bash
# What "changed this session" means. Sourced by the Stop hooks; not executable.
#
#   architecture-doc-check.sh       "you changed X without updating Y"
#   web-architecture-doc-check.sh   the same question, for web's contracts
#
# Both computed this inline and identically, and both were wrong the same way:
# the committed leg was `git diff $(git merge-base origin/main HEAD)...HEAD` —
# everything the BRANCH has done since main, not what THIS SESSION did. One
# table, one definition; see doc-rules.sh's header for why a second copy of a
# maintained thing is the failure mode, not the safety net.
#
# ── the bug this replaces ─────────────────────────────────────────────────────
# On a long-lived branch the branch-wide diff is a permanent false positive.
# Measured on `console/t5-paint` (75 commits ahead of main): a session with a
# completely clean working tree that made zero edits was told to update
# MIND_ARCHITECTURE.md for native/organon-core/src/edition.rs — a file last
# touched in bd12b8f on 2026-08-11 by an earlier session, which had already
# answered for it. Every session on that branch inherited every earlier
# session's outstanding obligations, forever, and each new commit added one.
#
# The cost is not a stray line of text. This hook is fail-safe and loop-guarded,
# so a false positive is dismissed by simply stopping twice — which is precisely
# what makes it corrosive. CLAUDE.md treats the same-change doc discipline as
# real, and a check that trains everyone to reflexively dismiss it has stopped
# protecting anything at all.
#
# ── deriving the session boundary ─────────────────────────────────────────────
# A Stop hook receives the session JSON on stdin. Its base fields are
# `session_id`, `transcript_path` and `cwd` (plus `hook_event_name` and
# `stop_hook_active`, and optionally `prompt_id` / `permission_mode`). There is
# no "commit at session start" among them and nothing else in the tree records
# one, so the boundary has to be derived rather than read.
#
# `transcript_path` is the lever: it points at the session's own .jsonl, whose
# opening entries are written when the session starts. Their `timestamp` is the
# session start in ISO-8601 UTC — exactly the format `git log --since` accepts,
# and from the same machine clock that stamps commits, so there is no skew to
# reconcile.
#
# ⚠️ Take the MINIMUM of the first several lines, not line 1. The opening
# entries are not strictly ordered: measured on a real transcript, line 1 was
# 03:43:01.270Z while line 3 was 03:43:00.565Z. Sub-second slack is irrelevant
# to commit filtering, but the min biases EARLY, which is the safe direction —
# a commit made during the session is never missed, and the worst an early bias
# can do is notice a commit made in the seconds before the session opened.
#
# Two properties worth stating plainly rather than discovering later:
#
#   * `--since` filters on COMMITTER date, so a rebase or amend — which rewrites
#     it to now — makes rewritten history count as this session's. That is
#     defensible (the session did handle those commits) and rare.
#   * A resumed conversation keeps its transcript, so its start is the ORIGINAL
#     start and it sees the whole conversation's commits. That is the honest
#     answer, not a leak: it is the same session. Either way the scope is the
#     conversation, never the branch.
#
# ── when the boundary can't be derived ────────────────────────────────────────
# No transcript_path, an unreadable file, or no timestamp in it → fall back to
# the working tree alone: staged + unstaged + untracked. That run LOSES the
# committed leg, so a session which commits its work before stopping gets no
# nudge. Deliberate, and the only defensible direction: the alternative fallback
# is the branch-wide diff, which is the bug. A missed nudge costs one doc update
# noticed later; a false nudge costs the credibility of every real one.

# Session start as ISO-8601, or empty if it cannot be established.
# Usage: start="$(session_start_iso "$stdin_json")"
session_start_iso() {
  # Tolerate `"k":"v"` and `"k": "v"` — the loop guards in both callers already
  # do. `agent_transcript_path` (SubagentStop) can't false-match: the pattern
  # requires a quote immediately before `transcript_path`.
  _ss_tp="$(printf '%s' "$1" \
    | sed -n 's/.*"transcript_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
    | head -1)"
  [ -n "$_ss_tp" ] || return 0

  # On Windows the value is a JSON-escaped path (C:\\Users\\…). Collapse any run
  # of backslashes to one forward slash so the msys tools can open it; a POSIX
  # path contains none and passes through untouched.
  _ss_tp="$(printf '%s' "$_ss_tp" | sed 's|\\\\*|/|g')"
  [ -r "$_ss_tp" ] || return 0

  head -n 40 "$_ss_tp" 2>/dev/null \
    | grep -oE '"timestamp":"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:.]+Z"' \
    | sed -e 's/^"timestamp":"//' -e 's/"$//' \
    | sort \
    | head -1
}

# Files this session changed: committed-during-this-session (and not already
# upstream) + staged + unstaged + untracked. One path per line, sorted, unique.
# Never fails — the working-tree legs still print if the committed leg can't be
# scoped, so callers can treat empty output as "nothing changed".
# Usage: changed="$(session_changed_files "$stdin_json")"
session_changed_files() {
  _sc_start="$(session_start_iso "$1")"

  # Narrow to this branch's own commits when we can tell where main is; without
  # a remote, `--since` alone already scopes it to the session.
  _sc_base="$(git merge-base origin/main HEAD 2>/dev/null)"
  _sc_range="HEAD"
  [ -n "$_sc_base" ] && _sc_range="$_sc_base..HEAD"

  {
    # Merge commits list no files here (no -m), so a merge contributes nothing —
    # right, since it authors no change.
    [ -n "$_sc_start" ] && \
      git log --since="$_sc_start" --name-only --pretty=format: "$_sc_range"
    git diff --name-only
    git diff --name-only --cached
    git ls-files --others --exclude-standard
  } 2>/dev/null | sed '/^$/d' | sort -u
}
