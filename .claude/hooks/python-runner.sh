#!/usr/bin/env bash
# Shared: find a Python that actually runs, and say so when there isn't one.
# organon#1 Tier 1. Sourced by context-budget-check.sh and structure-drift-check.sh.
#
# WHY THIS EXISTS
#
# Both of those hooks are wired in .claude/settings.json's SessionStart array, and on
# this repo's Windows workstation NEITHER HAD EVER RUN. Every python3 call in them was
# spelled `python3 … 2>/dev/null || exit 0` — the fail-safe posture every hook here
# uses, and correct for a hiccup. It is wrong for a missing interpreter, because the
# result is a hook that exits 0 having printed nothing, which is byte-for-byte what
# "nothing to report" looks like. CLAUDE.md meanwhile asserted the budget line "always
# prints". Measured 2026-08-20: `bash .claude/hooks/context-budget-check.sh` → exit 0,
# zero bytes out. It had been that way long enough for ARCHITECTURE.md to grow from
# 202,276 bytes to 219,695 — 110% of the very budget the silent hook exists to police.
#
# ⚠️ THE STORE STUB IS THE TRAP, and it is why `command -v` is not the test.
# Windows ships %LOCALAPPDATA%\Microsoft\WindowsApps\python3.exe: a stub that exists on
# PATH, is executable, and — when Python is not installed from the Store — prints
# "Python was not found; run without arguments to install from the Microsoft Store…"
# to STDERR and exits 49. So `command -v python3` says yes and tells you nothing at
# all. Locating an interpreter proves nothing; only RUNNING one does. `py_probe` runs
# each candidate and demands a sentinel back on stdout.
#
# ⚠️ THE PROGRAM GOES ON STDIN, NEVER IN ARGV. Every backend is invoked as `… -`, so
# the Python source travels as bytes on a pipe. That is not style: `wsl.exe` re-quotes
# its arguments, and this workstation has a long history of `$`-laden one-liners
# arriving inside WSL with their quoting eaten. A heredoc cannot be re-quoted.
#
# ⚠️ ENV VARS DO NOT CROSS INTO WSL BY THEMSELVES. `NOW=… wsl.exe -e python3` leaves
# NOW unset on the far side — verified. `WSLENV` is the only forwarding mechanism, and
# it takes NAMES, not values, so a multi-line JSON blob crosses intact with no quoting
# involved (also verified). Hence `py_run`'s argument list: the names to forward. On a
# native backend the exported variables are already there and the list is ignored,
# which keeps one call shape for both.
#
# WHAT THIS DELIBERATELY DOES NOT DO
#
# It does not reimplement anything in awk or bash. The hooks' Python parses JSON and
# computes session-over-session deltas; a shell rewrite would be a second
# implementation of that, and second implementations rot. This fixes only WHICH
# interpreter gets the program — and, when there is none, makes the hook say so.

# Not `set -e` anything here: this file is sourced INTO hooks that manage their own
# error posture, and flipping shell options under a caller is how a fail-safe stops
# being safe.

PY_KIND=""      # "" until resolved, then "native" or "wsl"
PY_LABEL=""     # what was found, for the record — printed nowhere unless asked
PY_BIN=()       # the command words

# Ceiling on a single probe. A candidate that hangs must not eat the hook's whole
# SessionStart budget (20 s in settings.json) — if the harness kills the hook mid-probe
# it dies before printing anything, and a silent death is the exact defect being fixed
# here. Measured on organon-one: the WSL backend answers in ~100 ms, the Store stub
# fails in ~164 ms, so 10 s is four decimal orders of headroom over both.
PY_PROBE_TIMEOUT="${PY_PROBE_TIMEOUT:-10}"

# Run one candidate and require proof of life. Anything short of exit 0 plus the
# sentinel on stdout is a no: that covers the Store stub (exit 49, message on stderr),
# a `python` that is really Python 2 (`print("…")` still works, so the sentinel alone
# would pass it — but every program here is 3-only and would fail loudly rather than
# silently, which is the acceptable half of the trade), and a wsl.exe with no distro.
py_probe() {
  local out
  if command -v timeout >/dev/null 2>&1; then
    out=$(printf 'print("PY_RUNNER_OK")\n' | timeout "$PY_PROBE_TIMEOUT" "$@" - 2>/dev/null) || return 1
  else
    out=$(printf 'print("PY_RUNNER_OK")\n' | "$@" - 2>/dev/null) || return 1
  fi
  # Substring, not equality: a Windows interpreter under Git Bash returns CRLF, and
  # command substitution strips the \n but leaves the \r.
  case "$out" in *PY_RUNNER_OK*) return 0 ;; *) return 1 ;; esac
}

# Resolve once per process. Native first — it is faster and has no path translation to
# get wrong. WSL last, because it is a real interpreter reached across a boundary, and
# the boundary has consequences the callers have to know about (see the ⚠️ in
# context-budget-check.sh about which `bash` a WSL Python would spawn).
py_find() {
  [ -n "$PY_KIND" ] && return 0
  local c
  for c in python3 python; do
    command -v "$c" >/dev/null 2>&1 || continue
    if py_probe "$c"; then
      PY_KIND=native; PY_BIN=("$c"); PY_LABEL="$c"; return 0
    fi
  done
  # The Windows launcher. Absent on organon-one, present on plenty of Windows boxes
  # that also carry the Store stub — so it is worth a probe precisely because the two
  # coexist and the stub is what wins the `python3` name.
  if command -v py >/dev/null 2>&1 && py_probe py -3; then
    PY_KIND=native; PY_BIN=(py -3); PY_LABEL="py -3"; return 0
  fi
  if command -v wsl.exe >/dev/null 2>&1 && py_probe wsl.exe -e python3; then
    PY_KIND=wsl; PY_BIN=(wsl.exe -e python3); PY_LABEL="wsl.exe -e python3"; return 0
  fi
  return 1
}

# py_run [VARNAME …] — run the program on stdin. Named variables must already be
# exported by the caller; they are forwarded across the WSL boundary via WSLENV and
# ignored on a native backend.
py_run() {
  py_find || return 127
  if [ "$PY_KIND" = "wsl" ] && [ "$#" -gt 0 ]; then
    local names
    names=$(IFS=:; printf '%s' "$*")
    WSLENV="${names}${WSLENV:+:$WSLENV}" "${PY_BIN[@]}" -
  else
    "${PY_BIN[@]}" -
  fi
}

# The whole point: a check that cannot run says so, instead of exiting 0 into a silence
# indistinguishable from "all clear". $1 names the check; $2 says what you are not
# being told because of it.
py_unavailable() {
  echo "⚠️  $1 did not run: no working Python on this machine, so $2"
  echo "     Probed by RUNNING, not locating — python3, python, py -3, wsl.exe -e python3."
  echo "     On Windows \`python3\` is usually the Microsoft Store stub: it is on PATH and"
  echo "     it exits non-zero, so \`command -v\` finding it means nothing."
  echo "     Fix: install Python 3, or make \`wsl.exe -e python3\` work. This is a report,"
  echo "     not a gate — nothing is blocked either way."
}
