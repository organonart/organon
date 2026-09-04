#!/usr/bin/env bash
# selftest.sh — pin `verify.sh`'s verdict arithmetic without a GPU (PBR text W20, #217).
#
# `verify.sh` is the one thing in this tree that cannot test itself by running: it needs a
# GPU, a visual and ten minutes. So the part of it that is pure decision — **what a run's
# exit code is** — is a function of three counters and nothing else, and this file drives
# every case of it in milliseconds by sourcing the script with `VERIFY_SH_DEFINE_ONLY=1`,
# which defines the counters and `exit_code_for` and stops before the option loop.
#
# 🚨 Why this exists at all. Until W20 `record` set one `FAILED` flag for every non-`ok`
# verdict and the script ended `exit "$FAILED"`, so a gate that **aborted without scoring
# anything** returned the same 1 as a gate that scored and missed a threshold — while the
# script's own header promised 2 for the first. The ORGANON leg had been in exactly that
# state since #246: it had never once produced a number, and it had never once said so in
# the only place a machine reads. The second hole was quieter: `FAILED` starts at 0 and
# nothing required a check to exist, so a run that recorded nothing printed **All checks
# passed** and exited 0.
#
#   bash verify/selftest.sh        # or: ./verify.sh --self-test
#
# ⚠️ Every assertion here was mutation-tested — the arithmetic broken on purpose, the
# failure read — and the messages are quoted in the PR that landed this file. If you change
# `exit_code_for`, this is the file that has an opinion about it.
set -uo pipefail
NATIVE=$(cd "$(dirname "$0")/.." && pwd)

# ⚠️ `verify.sh` opens with `cd "$(dirname "$0")"`, and under `.` that `$0` is still THIS
# file — so sourcing it lands us in verify/ rather than native/. Come back deliberately
# instead of relying on where we end up.
VERIFY_SH_DEFINE_ONLY=1 . "$NATIVE/verify.sh"
cd "$NATIVE"

FAILURES=0
check() { # expected checks failed unmeasured why
  local got
  got=$(exit_code_for "$2" "$3" "$4")
  if [ "$got" != "$1" ]; then
    echo "FAIL: checks=$2 failed=$3 unmeasured=$4 -> $got, wanted $1 ($5)" >&2
    FAILURES=$((FAILURES + 1))
  fi
}

# 0 — a run where every check was measured and every one passed. The only success.
check 0 3 0 0 "three checks, all measured, all passed"

# 1 — MEASURED and failed. This is the number that means "go and read the numbers".
check 1 3 1 0 "a measured check missed a threshold"

# 2 — nothing was measured. This is the number that means "there are no numbers".
check 2 3 0 1 "a check reached no judgement"

# 2 wins over 1. A run with a hole in it cannot honestly be summarised as "a check failed":
# whoever reads the 1 goes looking for numbers that were never taken. This is the exact
# case the ORGANON gate was in — the harness reported 1 for a run that scored nothing.
check 2 3 1 1 "unmeasured outranks failed"

# 2 for an empty report. `FAILED=0` alone made "nothing ran" indistinguishable from "nothing
# went wrong", which is the same defect at full strength: the most complete failure a
# harness can have, reported as its cleanest result.
check 2 0 0 0 "no check was recorded at all"
check 2 0 1 1 "no check recorded, counters set — still nothing was measured"

# `record` is what moves the counters, and it must move them per verdict. Driven here with
# its two output files pointed at a scratch directory, since the real ones live under
# target/verify/ and are created long after this point in the script.
tmp=$(mktemp -d 2>/dev/null || echo "${TMPDIR:-/tmp}/verify-selftest.$$")
mkdir -p "$tmp"
ROWS="$tmp/.rows"
JSON="$tmp/.json"
: >"$ROWS"
: >"$JSON"
CHECKS=0
FAILED=0
UNMEASURED=0
record "a" ok "m" "d"
record "b" skip "m" "d"
record "c" FAIL "m" "d"
record "d" UNMEASURED "m" "d"
if [ "$CHECKS" != "4" ]; then
  echo "FAIL: record counted $CHECKS checks, wanted 4 (every verdict is a check)" >&2
  FAILURES=$((FAILURES + 1))
fi
if [ "$FAILED" != "1" ] || [ "$UNMEASURED" != "1" ]; then
  echo "FAIL: record set failed=$FAILED unmeasured=$UNMEASURED, wanted 1 and 1" >&2
  FAILURES=$((FAILURES + 1))
fi
# `skip` and `ok` must move neither flag — a skipped golden is not a hole in the run.
CHECKS=0
FAILED=0
UNMEASURED=0
record "e" ok "m" "d"
record "f" skip "m" "d"
if [ "$FAILED" != "0" ] || [ "$UNMEASURED" != "0" ]; then
  echo "FAIL: ok/skip set failed=$FAILED unmeasured=$UNMEASURED, wanted 0 and 0" >&2
  FAILURES=$((FAILURES + 1))
fi
if [ "$(wc -l <"$ROWS" | tr -d ' ')" != "6" ]; then
  echo "FAIL: the report kept $(wc -l <"$ROWS") rows for six recorded checks" >&2
  FAILURES=$((FAILURES + 1))
fi
rm -rf "$tmp"

# ⚠️ The script's HEADER is the thing people read before they read the code, and a header
# that disagrees with the arithmetic is the defect this file exists to close, one layer out.
for phrase in \
  "1 something was MEASURED and failed" \
  "2 something was NOT" \
  "recorded nothing at all is also 2"; do
  if ! grep -qF "$phrase" verify.sh; then
    echo "FAIL: verify.sh's header no longer says \"$phrase\" — the promise and the code have parted" >&2
    FAILURES=$((FAILURES + 1))
  fi
done

if [ "$FAILURES" != "0" ]; then
  echo "verify/selftest.sh: $FAILURES failure(s)" >&2
  exit 1
fi
echo "verify/selftest.sh: ok — the exit code says which of the three things happened"
