#!/usr/bin/env bash
# verify.sh — drive Organon from the outside, capture frames, and judge them.
#
# The scripted half of the deploy/verify loop. `deploy.sh` installs the plugin for a
# human to look at; this launches the visual on its own, drives it through the
# `organon` CLI, snaps frames, and diffs them against committed goldens — so "does it
# still render what it rendered last week" becomes a pass/fail with evidence instead
# of a person squinting at a window.
#
# It exists because the test suite says almost nothing about the frame. Three egui
# 0.33 defects shipped against a 920/0 suite (#545); the depth-prepass group(5) crash
# (#519) was a runtime pipeline error no offline naga pass can see. Everything in that
# class is one snapshot away from being caught.
#
#   ./verify.sh                     # the standing suite, diffed against goldens
#   ./verify.sh --pr                # ALSO run verify/pr/ — this PR's own checks
#   ./verify.sh 02-chrome 03-glass  # just these scenes (name or file stem)
#   ./verify.sh --update-golden     # re-baseline: adopt this run's frames as truth
#   ./verify.sh --strict            # a missing golden is a failure (for CI)
#   ./verify.sh --legibility        # ALSO run the legibility gate (PBR text T13, below)
#   ./verify.sh --legibility-only   # just the gate — the one command for a text-look PR
#       [--effect NAME] [--seed N] [--converge S]   the producer's effect and seed, and
#                                                    how long the held frame accumulates
#
# Three kinds of check, deliberately kept apart:
#   verify/scenes/  the STANDING suite — permanent, golden-backed. "Did I break
#                   something that already worked?"
#   verify/pr/      THIS PR's acceptance checks, written by whoever built the feature.
#                   "Does the new thing actually do what the PR claims?" These lean on
#                   `#!expect same-as / differs-from`, which compares two frames from
#                   the SAME run — so they need no committed golden and work the very
#                   first time, on a feature that did not exist yesterday.
#   verify/legibility/  THE LEGIBILITY GATE (PBR text T13, organon#217) — not a golden
#                   and not a diff: doc/pbr_text_engine.md §9's two laws as a number,
#                   over a frame the real renderer produced. `--legibility` starts the
#                   text producer (`organon-glyphs`) on the Omarchy logo with a fixed
#                   effect and seed, drives the `faceplate` look through the CLI
#                   (`faceplate.scene` — as much of the rung as the vocabulary reaches;
#                   its header lists the nine fields it cannot), waits for the effect to
#                   settle and the held frame to accumulate, snaps it TWICE, and runs
#                   `legibility-gate` over the pair against `thresholds.toml`, with the
#                   fixture's colours taken from the settled ring. Exit 1 below a
#                   threshold. verify/README.md says what a pass looks like.
#
# Artifacts land in target/verify/ — report.md, summary.json, frames/, diffs/.
# Exit: 0 all checks passed · 1 a check failed · 2 the harness could not run.
#
# Runs anywhere the visual runs. On the Mac that is today, with no extra hardware.
# On Linux it needs a real GPU and a display; with none it will reach for xvfb-run.
# It talks to a PRIVATE IPC namespace, so it is safe to run while Organon is open in
# Ableton — the two do not see each other.
set -euo pipefail
cd "$(dirname "$0")"

SCENE_DIR=verify/scenes
PR_DIR=verify/pr
GOLDEN_DIR=verify/golden
OUT=target/verify
WITH_PR=0
UPDATE_GOLDEN=0
STRICT=0
BUILD=1
KEEP_VISUAL=0
STARTUP_TIMEOUT=40
# The legibility gate (PBR text T13). `expand` settles in a couple of seconds and never
# lets a lit cell go dark on the way (measured in T11); the seed is the issue number.
# CONVERGE is how long the held frame accumulates before the first snap — the settled
# frame is path-traced (T5's handover), and the second snap SPREAD_WAIT later is what
# makes "the same render twice" a number rather than a claim.
LEGIBILITY=0
LEGIBILITY_ONLY=0
LEG_EFFECT=expand
LEG_SEED=217
LEG_CONVERGE=6
LEG_SPREAD_WAIT=2
LEG_SETTLE_TIMEOUT=90
# macOS still ships bash 3.2, where `"${ARR[@]}"` on an EMPTY array under `set -u` is an
# unbound-variable error. Hence the parallel counter and the `${ARR[@]+…}` guards below —
# this script has to survive /bin/bash on the Mac, not just a modern Homebrew bash.
FILTER=()
FILTER_N=0

while [ $# -gt 0 ]; do
  case "$1" in
    --update-golden) UPDATE_GOLDEN=1 ;;
    --strict)        STRICT=1 ;;
    --no-build)      BUILD=0 ;;
    --keep-visual)   KEEP_VISUAL=1 ;;
    --out)           shift; OUT="${1:?--out wants a directory}" ;;
    --scenes)        shift; SCENE_DIR="${1:?--scenes wants a directory}" ;;
    --pr)            WITH_PR=1 ;;
    --pr-dir)        shift; PR_DIR="${1:?--pr-dir wants a directory}"; WITH_PR=1 ;;
    --golden)        shift; GOLDEN_DIR="${1:?--golden wants a directory}" ;;
    --timeout)       shift; STARTUP_TIMEOUT="${1:?--timeout wants seconds}" ;;
    --legibility)    LEGIBILITY=1 ;;
    --legibility-only) LEGIBILITY=1; LEGIBILITY_ONLY=1 ;;
    --effect)        shift; LEG_EFFECT="${1:?--effect wants a ttfx effect name}" ;;
    --seed)          shift; LEG_SEED="${1:?--seed wants an integer}" ;;
    --converge)      shift; LEG_CONVERGE="${1:?--converge wants seconds}" ;;
    # Print the whole header block — from line 2 to the first non-comment line —
    # rather than a hardcoded range, which silently truncates whenever the header grows.
    -h|--help)       awk 'NR>1 && /^#/ {sub(/^# ?/, ""); print; next} NR>1 {exit}' "$0"; exit 0 ;;
    -*)              echo "verify.sh: unknown option '$1'" >&2; exit 2 ;;
    *)               FILTER+=("${1%.scene}"); FILTER_N=$((FILTER_N + 1)) ;;
  esac
  shift
done

# A private namespace keeps the harness off the live instrument: every IPC path,
# command channel, and eyes channel is namespaced (ipc::ns_file), so a visual started
# here is invisible to an Organon running in Ableton and vice versa.
export ORGANON_IPC_NS="${ORGANON_VERIFY_NS:-organon-verify}"
# Never grab the projector. A disabled-but-enumerated second display throws the window
# somewhere nobody can see, which is indistinguishable from "it never opened".
export ORGANON_VISUAL_DISPLAY=off

VISUAL=target/release/organic-math-visual
ORGANON=target/release/organon
IMGDIFF=target/release/examples/imgdiff
# The legibility gate's two extra binaries (PBR text T13): the text producer, and the
# judge. Built and required only with --legibility, so the standing suite's build is
# exactly what it was.
GLYPHS=target/release/organon-glyphs
GATE=target/release/legibility-gate

# Git Bash on Windows: `uname -s` is MINGW64_NT-…; there is a display and no xvfb, and a
# binary is `foo.exe`. MSYS's `test -x foo` does find `foo.exe`, but say so explicitly
# rather than lean on it.
WINDOWS=0
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) WINDOWS=1 ;; esac

if [ "$BUILD" = "1" ]; then
  echo "building (visual + CLI + imgdiff)…"
  # organon#49 T4c-ii — the visual is `-p organon-visual`; `organon` and the example stay
  # in the root package, so this is two invocations now rather than one.
  cargo build --release -p organon-visual --bin organic-math-visual
  cargo build --release --bin organon --example imgdiff
  if [ "$LEGIBILITY" = "1" ]; then
    echo "building (organon-glyphs + legibility-gate)…"
    cargo build --release -p organon-glyphs --bin organon-glyphs
    cargo build --release -p organon-render --bin legibility-gate
  fi
fi
NEEDED=("$VISUAL" "$ORGANON" "$IMGDIFF")
if [ "$LEGIBILITY" = "1" ]; then NEEDED+=("$GLYPHS" "$GATE"); fi
for bin in "${NEEDED[@]}"; do
  [ -x "$bin" ] || [ -x "$bin.exe" ] || { echo "verify.sh: missing $bin (drop --no-build?)" >&2; exit 2; }
done

rm -rf "$OUT"
mkdir -p "$OUT/frames" "$OUT/diffs" "$GOLDEN_DIR"

# --- scene selection ---------------------------------------------------------------
SCENES=()
SCENE_SEARCH=("$SCENE_DIR")
if [ "$WITH_PR" = "1" ] && [ -d "$PR_DIR" ]; then
  SCENE_SEARCH+=("$PR_DIR")
fi
# --legibility-only runs no scenes at all; the gate is the whole run.
if [ "$LEGIBILITY_ONLY" = "1" ]; then SCENE_SEARCH=(); fi
for dir in ${SCENE_SEARCH[@]+"${SCENE_SEARCH[@]}"}; do
  for f in "$dir"/*.scene; do
    [ -e "$f" ] || continue
    name=$(basename "$f" .scene)
    if [ "$FILTER_N" -gt 0 ]; then
      match=0
      for want in ${FILTER[@]+"${FILTER[@]}"}; do
        if [ "$want" = "$name" ]; then match=1; fi
      done
      [ "$match" = "1" ] || continue
    fi
    SCENES+=("$f")
  done
done
if [ ${#SCENES[@]} -eq 0 ] && [ "$LEGIBILITY_ONLY" = "0" ]; then
  echo "verify.sh: no scenes matched in ${SCENE_SEARCH[*]}" >&2
  exit 2
fi

# --- launch the visual -------------------------------------------------------------
VISUAL_PID=""
# The legibility gate's producer, while one is running (PBR text T13). Always killed:
# --keep-visual keeps the window, not a producer holding a 600 s dwell on the ring.
GLYPHS_PID=""
cleanup() {
  if [ -n "$GLYPHS_PID" ] && kill -0 "$GLYPHS_PID" 2>/dev/null; then
    kill "$GLYPHS_PID" 2>/dev/null || true
    wait "$GLYPHS_PID" 2>/dev/null || true
  fi
  if [ -n "$VISUAL_PID" ] && [ "$KEEP_VISUAL" = "0" ] && kill -0 "$VISUAL_PID" 2>/dev/null; then
    kill "$VISUAL_PID" 2>/dev/null || true
    wait "$VISUAL_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

LAUNCH=("$VISUAL")
if [ "$WINDOWS" = "0" ] && [ "$(uname)" != "Darwin" ] && [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
  if command -v xvfb-run >/dev/null 2>&1; then
    echo "no display — wrapping in xvfb-run (the GPU still does the rendering)"
    LAUNCH=(xvfb-run -a -s "-screen 0 1280x1024x24" "$VISUAL")
  else
    echo "verify.sh: no DISPLAY and no xvfb-run — the visual needs a surface to render into." >&2
    exit 2
  fi
fi

echo "starting the visual (ns=$ORGANON_IPC_NS)…"
if [ "$(uname)" = "Darwin" ]; then
  # Launch through a THROWAWAY .app bundle, not by exec'ing the binary.
  #
  # winit fires `resumed` off `applicationDidFinishLaunching`, which macOS only delivers
  # to a process LaunchServices actually launched *and activated*. `main()` builds the
  # event loop with `with_activate_ignoring_other_apps(false)` on purpose — so the plugin
  # can open the visual without yanking focus off Ableton — and a bare `./organic-math-visual &`
  # from a script therefore often never gets `resumed` at all: no window, no device, and the
  # run loop spinning at ~100% CPU. That is #588, and it is what made this harness's first
  # Mac run look like a timeout. Measured here, not theorised.
  #
  # `open` also brings it frontmost, which the harness needs for a second reason:
  # `render()` early-returns on `CurrentSurfaceTexture::Occluded`, so a fully covered
  # window produces no frames and answers no eyes requests.
  #
  # When #588 is fixed this whole branch collapses back to the plain background launch.
  APP="$OUT/OrganonVerify.app"
  rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS"
  cp "$VISUAL" "$APP/Contents/MacOS/OrganonVerify"
  cat >"$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>OrganonVerify</string>
<key>CFBundleExecutable</key><string>OrganonVerify</string>
<key>CFBundleIdentifier</key><string>art.organon.verify</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
  codesign --force -s - "$APP" >/dev/null 2>&1 || true
  # `open --env` is how the namespace reaches it: LaunchServices does NOT inherit the
  # shell environment, so exporting ORGANON_IPC_NS alone would leave the visual on the
  # DEFAULT namespace while the CLI talked to the private one — a silent mismatch that
  # looks identical to "the visual never answered".
  open --env "ORGANON_IPC_NS=$ORGANON_IPC_NS" --env "ORGANON_VISUAL_DISPLAY=off" \
       --stderr "$OUT/visual.log" --stdout "$OUT/visual.log" "$APP"
  # `open` returns immediately and gives us no pid, so find the child it started.
  VISUAL_PID=""
  for _ in $(seq 1 40); do
    VISUAL_PID="$(pgrep -f "$APP/Contents/MacOS/OrganonVerify" | head -1)"
    [ -n "$VISUAL_PID" ] && break
    sleep 0.25
  done
  if [ -z "$VISUAL_PID" ]; then
    echo "verify.sh: could not start the visual via $APP (open succeeded but no process)." >&2
    exit 1
  fi
else
  "${LAUNCH[@]}" >"$OUT/visual.log" 2>&1 &
  VISUAL_PID=$!
fi

# Bring the visual back to the front before every snap.
#
# Not belt-and-braces: `render()` early-returns on `CurrentSurfaceTexture::Occluded`, so
# the moment anything covers the window the visual stops producing frames and every
# subsequent snap times out — which reads as "the visual is wedged or died" even though
# it is healthy and idling. That is exactly how this harness's first working run failed:
# scenes 00 and 01 passed (taken right after launch, still frontmost), then 02 onward all
# failed together. On a developer's desktop something WILL steal focus during a run.
#
# A no-op off macOS, and a no-op on macOS once the visual can render while occluded.
raise_visual() {
  [ "$(uname)" = "Darwin" ] || return 0
  [ -d "${APP:-}" ] || return 0
  open "$APP" 2>/dev/null || true
}

# The readiness probe is a THROWAWAY SNAP, not `status`.
#
# `status` cannot work here and no timeout will fix it: it calls `require_live()`
# (ctl.rs), which gates on `ipc::Reader::is_live()` — a read of the *Shared snapshot*.
# Organon's IPC contract is plugin = Writer, visual = Reader, so with only the visual
# running nothing ever writes Shared and `status` exits 3 forever. That is structural.
#
# `snap` needs no writer: it rides the eyes channel, which the visual itself answers.
# Verified on the Mac against a visual with no writer in its namespace — `snap`, `set`,
# `generator`, `surface` and `release` all work and visibly change the render; only the
# three read commands (`status`/`get`/`watch`) need a live writer, and the harness uses
# none of them. Probing with the very capability the harness depends on also means a
# green probe proves the thing we actually need, rather than a proxy for it.
ready=0
probe_frame="$OUT/.probe.png"
deadline=$(( STARTUP_TIMEOUT * 4 ))
for _ in $(seq 1 "$deadline"); do
  if "$ORGANON" snap -o "$probe_frame" >/dev/null 2>&1; then ready=1; break; fi
  if ! kill -0 "$VISUAL_PID" 2>/dev/null; then
    echo "verify.sh: the visual exited during startup. Last lines of $OUT/visual.log:" >&2
    tail -30 "$OUT/visual.log" >&2 || true
    exit 1
  fi
  sleep 0.25
done
rm -f "$probe_frame"
if [ "$ready" != "1" ]; then
  {
    echo "verify.sh: the visual is running (pid $VISUAL_PID) but never answered a snap"
    echo "  within ${STARTUP_TIMEOUT}s in namespace '$ORGANON_IPC_NS'."
    echo
    echo "  A snap is answered from inside the render loop, so this means it is not"
    echo "  producing frames. The two causes seen on real hardware:"
    echo "    · the window never opened — winit's 'resumed' only fires once the app is"
    echo "      activated (#588); the 'open' above should handle it, but a headless or"
    echo "      remote session can defeat it. A visual at ~100% CPU with no window is this."
    echo "    · the window is fully occluded — render() early-returns on Occluded, so a"
    echo "      covered window renders nothing. It must be frontmost, not just mapped."
    echo
    echo "  It is NOT a missing writer: snap does not need one (only status/get/watch do)."
  } >&2
  tail -30 "$OUT/visual.log" >&2 || true
  exit 1
fi
echo "visual up (answered a snap)."

# --- helpers -----------------------------------------------------------------------
# `#!key value` directives out of a scene file, with a default.
directive() {
  local v
  v=$(sed -n "s/^#![[:space:]]*$2[[:space:]]\{1,\}\(.*\)$/\1/p" "$1" | head -1)
  if [ -n "$v" ]; then printf '%s' "$v"; else printf '%s' "$3"; fi
}
# All values of a repeatable directive, one per line (`#!expect` may appear many times).
directive_all() {
  sed -n "s/^#![[:space:]]*$2[[:space:]]\{1,\}\(.*\)$/\1/p" "$1"
}
# One number out of imgdiff's JSON line.
jnum() {
  printf '%s' "$1" | sed -n "s/.*\"$2\"[[:space:]]*:[[:space:]]*\([0-9.eE+-]*\).*/\1/p" | head -1
}
fle() { awk -v a="$1" -v b="$2" 'BEGIN{exit !(a<=b)}'; }
fge() { awk -v a="$1" -v b="$2" 'BEGIN{exit !(a>=b)}'; }
has() { case ",$1," in *",$2,"*) return 0 ;; *) return 1 ;; esac; }

FAILED=0
ROWS="$OUT/.rows"
JSON="$OUT/.json"
: >"$ROWS"
: >"$JSON"
# Per-scene note lines, in report order.
NOTES="$OUT/.notes"
: >"$NOTES"
# `#!expect` assertions, evaluated in a second pass once every frame is captured:
# `scene|kind|target|tolerance`. Deferred so a scene may reference one declared
# later in the run, and so ordering never becomes load-bearing.
EXPECTS="$OUT/.expects"
: >"$EXPECTS"

# Detail strings quote scene commands verbatim, and a scene command legitimately
# contains double quotes (`surface "swept tubes"`) — which would otherwise emit
# invalid JSON exactly when a scene fails, i.e. when the summary matters most.
json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
# A bare `|` in a detail silently splits the report's table row into extra columns.
md_escape() { printf '%s' "$1" | sed 's/|/\\|/g'; }

record() { # scene verdict metric detail
  printf '| `%s` | %s | %s | %s |\n' \
    "$(md_escape "$1")" "$(md_escape "$2")" "$(md_escape "$3")" "$(md_escape "$4")" >>"$ROWS"
  printf '{"scene":"%s","verdict":"%s","metric":"%s","detail":"%s"}\n' \
    "$(json_escape "$1")" "$(json_escape "$2")" "$(json_escape "$3")" "$(json_escape "$4")" >>"$JSON"
  [ "$2" = "FAIL" ] && FAILED=1
  return 0
}

# --- run the scenes ----------------------------------------------------------------
for scene in ${SCENES[@]+"${SCENES[@]}"}; do
  name=$(basename "$scene" .scene)
  desc=$(directive "$scene" desc "")
  checks=$(directive "$scene" checks "nonblack,golden")
  tol=$(directive "$scene" tolerance "0.005")
  settle_ms=$(directive "$scene" settle "800")
  settle=$(awk -v m="$settle_ms" 'BEGIN{printf "%.3f", m/1000}')

  echo "── $name — ${desc:-(no description)}"

  # Every scene starts from a known state: drop all agent holds, then set its own
  # generator / surface / material explicitly.
  #
  # ⚠️ `release` with no id clears the mode selectors TOO — `agent.rs::release_all`
  # does `holds.clear()` and sets `generator`/`surface`/`material` to `None`
  # (world.rs:9459 is the CLI's entry point). An earlier comment here claimed the
  # opposite, that selectors survive and "a scene that omits them inherits its
  # predecessor". They do not. The rule ("every scene sets its own") is unchanged;
  # only the reason for it was wrong, which is worse than useless in a file scene
  # authors read to learn the contract.
  #
  # What release does NOT reset is the visual's own frame state — in particular the
  # rotation-phase accumulators `World::angle` / `wind_phase`, which no param
  # addresses. See verify/README.md; it is why an `#!expect` pair must be adjacent.
  "$ORGANON" release >/dev/null 2>&1 || true

  applied=1
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    # xargs, not eval: it does shell-style quote parsing (so `generator "swept tubes"`
    # arrives as one argument) without handing the file the shell.
    if ! printf '%s' "$line" | xargs "$ORGANON" >/dev/null; then
      record "$name" FAIL "—" "scene command failed: \`$line\`"
      applied=0
      break
    fi
  done < <(grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$scene" || true)
  [ "$applied" = "1" ] || continue

  sleep "$settle"

  frame="$OUT/frames/$name.png"
  raise_visual
  if ! "$ORGANON" snap -o "$frame" >/dev/null 2>&1; then
    record "$name" FAIL "—" "snap failed — the visual is wedged or died (see visual.log)"
    continue
  fi

  # -- nonblack: did it draw anything at all? --------------------------------------
  if has "$checks" nonblack; then
    s=$("$IMGDIFF" "$frame")
    mean=$(jnum "$s" mean); sd=$(jnum "$s" stddev)
    if fle "$mean" 0.002 || fle "$sd" 0.004; then
      record "$name" FAIL "mean=$mean sd=$sd" "frame is black or flat — it rendered nothing"
      continue
    fi
    record "$name" ok "mean=$mean sd=$sd" "nonblack"
  fi

  # -- animates: two snaps, zero input, must differ --------------------------------
  # A frozen redraw loop passes every other check. Only this one fails it.
  if has "$checks" animates; then
    sleep "$settle"
    frame_b="$OUT/frames/$name-b.png"
    raise_visual
    if ! "$ORGANON" snap -o "$frame_b" >/dev/null 2>&1; then
      record "$name" FAIL "—" "second snap failed"
      continue
    fi
    d=$("$IMGDIFF" "$frame" "$frame_b" --diff-out "$OUT/diffs/$name-motion.png")
    df=$(jnum "$d" diff_frac)
    if fge "$df" 0.002; then
      record "$name" ok "diff_frac=$df" "animates over ${settle_ms}ms"
    else
      record "$name" FAIL "diff_frac=$df" "frame did not change in ${settle_ms}ms — animation is frozen"
    fi
  fi

  # -- golden: has the look moved? --------------------------------------------------
  if has "$checks" golden; then
    golden="$GOLDEN_DIR/$name.png"
    if [ ! -f "$golden" ]; then
      if [ "$UPDATE_GOLDEN" = "1" ]; then
        cp "$frame" "$golden"
        record "$name" ok "—" "golden created"
      elif [ "$STRICT" = "1" ]; then
        record "$name" FAIL "—" "no golden committed (run --update-golden)"
      else
        record "$name" skip "—" "no golden yet — run \`./verify.sh --update-golden\`"
      fi
    else
      d=$("$IMGDIFF" "$golden" "$frame" --diff-out "$OUT/diffs/$name.png")
      df=$(jnum "$d" diff_frac); ma=$(jnum "$d" mean_abs); dims=$(printf '%s' "$d" | grep -c '"dims_match":true' || true)
      if [ "$UPDATE_GOLDEN" = "1" ]; then
        cp "$frame" "$golden"
        record "$name" ok "diff_frac=$df" "golden updated (was $df off)"
      elif [ "$dims" != "1" ]; then
        record "$name" FAIL "—" "frame size changed vs the golden"
      elif fle "$df" "$tol"; then
        record "$name" ok "diff_frac=$df" "matches golden (tol $tol)"
      else
        record "$name" FAIL "diff_frac=$df mean_abs=$ma" "**moved vs golden** (tol $tol) — see diffs/$name.png"
      fi
    fi
  fi

  # -- expectations: queued now, evaluated once every frame exists ------------------
  while IFS= read -r exp; do
    [ -n "$exp" ] || continue
    printf '%s|%s|%s\n' "$name" "$exp" "$tol" >>"$EXPECTS"
  done < <(directive_all "$scene" expect || true)

  printf '%s\n' "$name|$desc" >>"$NOTES"
done

# --- the legibility gate (PBR text T13, organon#217) --------------------------------
# doc/pbr_text_engine.md §9's two laws — the cell's energy stays in the cell, and the
# cell's brightness tracks what TTE said it was — over a frame THIS renderer produced,
# judged by `legibility-gate` (organon-render/src/legibility_gate.rs) against
# verify/legibility/thresholds.toml. Not a golden: a golden proves the look did not
# move, this proves the text is still readable, and a wrong look once baselined would
# pass a golden forever while failing here.
#
# The shape of the step: start the producer on the Omarchy logo (the fixture's own text,
# derived by the gate so there is no second copy of it) with a fixed effect and seed and
# a dwell long enough to gate inside; drive the look from faceplate.scene; wait for the
# producer to say the effect settled; wait LEG_CONVERGE more seconds, because the
# settled frame is handed to the path tracer (T5) and accumulates; snap; snap again
# LEG_SPREAD_WAIT later; gate the pair. The second snap is the determinism claim as a
# number — two noise realisations of one held picture must agree on every judged
# number to within max_spread — rather than as a sentence in a PR.
#
# ⚠️ The fixture's COLOURS come from the ring (`--ring`), because what TTE said a cell
# was is the effect's own final gradient, which the hand-written one-colour fixture
# cannot know; the fixture's SHAPE is cross-checked against the ring cell for cell, so a
# producer that drew the wrong text fails as "could not measure", never as a low score.
# ⚠️ The frame is what `organon snap` writes: 8-bit sRGB AFTER the visual's Reinhard
# tonemap — the display frame, not the HDR buffer. The gate's first line says so.
legibility_step() {
  local name="legibility-faceplate"
  local fixture="organon-render/tests/fixtures/omarchy-logo.txt"
  local thresholds="verify/legibility/thresholds.toml"
  local look="verify/legibility/faceplate.scene"
  local text="$OUT/legibility-input.txt"
  local cols rows
  cols=$(sed -n 's/^cols[[:space:]]\{1,\}\([0-9]\{1,\}\).*/\1/p' "$fixture" | head -1)
  rows=$(sed -n 's/^rows[[:space:]]\{1,\}\([0-9]\{1,\}\).*/\1/p' "$fixture" | head -1)
  echo "── $name — the legibility gate over a real faceplate render (effect $LEG_EFFECT, seed $LEG_SEED, ${cols}x${rows})"

  if ! "$GATE" --emit-text "$fixture" >"$text"; then
    record "$name" FAIL "—" "could not derive the producer input from $fixture"
    return
  fi
  # `--once --dwell 600`: one effect, then hold the settled grid for ten minutes (we kill
  # it long before). `--cols/--rows` pinned to the fixture's, so a ragged input cannot
  # make the canvas a different size from the grid the gate expects.
  "$GLYPHS" --input "$text" --effect "$LEG_EFFECT" --seed "$LEG_SEED" \
    --cols "$cols" --rows "$rows" --once --dwell 600 >"$OUT/glyphs.log" 2>&1 &
  GLYPHS_PID=$!

  "$ORGANON" release >/dev/null 2>&1 || true
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    if ! printf '%s' "$line" | xargs "$ORGANON" >/dev/null; then
      record "$name" FAIL "—" "look command failed: \`$line\` (is every id on the vocabulary?)"
      return
    fi
  done < <(grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$look" || true)

  # The producer logs `… settled after N ticks` when the effect returns None and the
  # held grid goes on the ring with FRAME_SETTLED. The gate refuses an unsettled ring
  # anyway; this wait is so the refusal never fires on a merely slow effect.
  local settled=0
  for _ in $(seq 1 $((LEG_SETTLE_TIMEOUT * 4))); do
    if grep -q "settled after" "$OUT/glyphs.log" 2>/dev/null; then settled=1; break; fi
    if ! kill -0 "$GLYPHS_PID" 2>/dev/null; then break; fi
    sleep 0.25
  done
  if [ "$settled" != "1" ]; then
    record "$name" FAIL "—" "the producer never settled within ${LEG_SETTLE_TIMEOUT}s — see glyphs.log"
    tail -5 "$OUT/glyphs.log" >&2 || true
    return
  fi
  echo "   settled — accumulating ${LEG_CONVERGE}s before the first snap"
  sleep "$LEG_CONVERGE"

  local frame="$OUT/frames/$name.png"
  local frame_b="$OUT/frames/$name-b.png"
  raise_visual
  if ! "$ORGANON" snap -o "$frame" >/dev/null 2>&1; then
    record "$name" FAIL "—" "snap failed — the visual is wedged or died (see visual.log)"
    return
  fi
  sleep "$LEG_SPREAD_WAIT"
  raise_visual
  if ! "$ORGANON" snap -o "$frame_b" >/dev/null 2>&1; then
    record "$name" FAIL "—" "second snap failed"
    return
  fi

  local gate_out="$OUT/$name.txt"
  local rc=0
  "$GATE" "$frame" "$fixture" --second "$frame_b" --thresholds "$thresholds" --geom auto \
    --ring "$ORGANON_IPC_NS" --dump "$OUT/diffs/$name-cells.png" >"$gate_out" 2>&1 || rc=$?
  cat "$gate_out"
  local summary verdict
  summary=$(grep '^legibility [0-9]*x[0-9]*:' "$gate_out" | head -1)
  verdict=$(tail -1 "$gate_out")
  case "$rc" in
    0) record "$name" ok "$summary" "$verdict — full report in $name.txt" ;;
    1) record "$name" FAIL "$summary" "**$verdict** — see $name.txt and diffs/$name-cells.png" ;;
    *) record "$name" FAIL "—" "the gate could not measure (exit $rc): $verdict" ;;
  esac
  printf '%s\n' "$name|the legibility gate (PBR text T13): \`faceplate\` over the Omarchy logo, effect \`$LEG_EFFECT\` seed $LEG_SEED, two snaps ${LEG_SPREAD_WAIT}s apart after ${LEG_CONVERGE}s of accumulation. Numbers in \`$name.txt\`; the per-cell picture in \`diffs/$name-cells.png\`." >>"$NOTES"

  kill "$GLYPHS_PID" 2>/dev/null || true
  wait "$GLYPHS_PID" 2>/dev/null || true
  GLYPHS_PID=""
}
if [ "$LEGIBILITY" = "1" ]; then
  legibility_step
fi

# --- second pass: `#!expect same-as / differs-from` ---------------------------------
# A/B assertions between two frames captured in THIS run. They need no committed
# golden, which is the whole point: a check written for a feature that did not exist
# yesterday works the first time it runs. This is how "default-inert" (dispersion 0 =
# today's glass, palette Native = current) stops being prose and becomes a test.
while IFS='|' read -r sname kind_target tol; do
  [ -n "$sname" ] || continue
  kind=${kind_target%% *}
  target=${kind_target#* }
  target=${target%% *}                       # tolerate trailing comments
  a="$OUT/frames/$sname.png"
  b="$OUT/frames/$target.png"
  if [ ! -f "$a" ] || [ ! -f "$b" ]; then
    record "$sname" FAIL "—" "expect $kind \`$target\`: a frame is missing (was that scene run?)"
    continue
  fi
  d=$("$IMGDIFF" "$a" "$b" --diff-out "$OUT/diffs/$sname-vs-$target.png")
  df=$(jnum "$d" diff_frac)
  case "$kind" in
    same-as)
      if fle "$df" "$tol"; then
        record "$sname" ok "diff_frac=$df" "inert: identical to \`$target\` (tol $tol)"
      else
        record "$sname" FAIL "diff_frac=$df" "**not inert** — differs from \`$target\` (tol $tol); see diffs/$sname-vs-$target.png"
      fi ;;
    differs-from)
      if fle "$df" "$tol"; then
        record "$sname" FAIL "diff_frac=$df" "**no effect** — identical to \`$target\` (tol $tol); the feature did nothing"
      else
        record "$sname" ok "diff_frac=$df" "has effect: differs from \`$target\` (tol $tol)"
      fi ;;
    *)
      record "$sname" FAIL "—" "unknown expect kind \`$kind\` (want same-as | differs-from)" ;;
  esac
done <"$EXPECTS"

# --- report ------------------------------------------------------------------------
REPORT="$OUT/report.md"
{
  echo "# Organon frame verification"
  echo
  if [ "$FAILED" = "0" ]; then
    echo "**All checks passed.**"
  else
    echo "**FAILED** — at least one check did not pass. Details below."
  fi
  echo
  echo "\`$(uname -s)\` · $(date -u '+%Y-%m-%d %H:%M UTC') · commit \`$(git rev-parse --short HEAD 2>/dev/null || echo '?')\`"
  echo
  echo "| Scene | Verdict | Metric | Detail |"
  echo "|---|---|---|---|"
  cat "$ROWS"
  echo
  echo "## Frames"
  echo
  while IFS='|' read -r n d; do
    echo "### \`$n\`"
    [ -n "$d" ] && echo "$d"
    echo
    echo "![$n](frames/$n.png)"
    if [ -f "$OUT/diffs/$n.png" ]; then
      echo
      echo "diff vs golden — bright pixels are where it moved:"
      echo
      echo "![$n diff](diffs/$n.png)"
    fi
    # `#!expect` comparisons write `<scene>-vs-<target>.png`. Embed them too — for an
    # inertness failure this picture *is* the finding, and naming it in the detail text
    # is no use to someone reading the report on a PR.
    for vs in "$OUT/diffs/$n"-vs-*.png; do
      [ -e "$vs" ] || continue
      vsname=$(basename "$vs" .png)
      echo
      echo "expect comparison — \`${vsname#"$n"-vs-}\`:"
      echo
      echo "![$vsname](diffs/$vsname.png)"
    done
    echo
  done <"$NOTES"
  echo "---"
  echo
  echo "Metrics: \`diff_frac\` = fraction of pixels differing by >2% on any channel"
  echo "(sensitive to layout shifts); \`mean_abs\` = mean channel difference"
  echo "(sensitive to overall brightness/colour). Re-baseline with \`./verify.sh --update-golden\`."
} >"$REPORT"

{
  echo "{"
  echo "  \"passed\": $([ "$FAILED" = "0" ] && echo true || echo false),"
  echo "  \"host\": \"$(uname -s)\","
  echo "  \"commit\": \"$(git rev-parse --short HEAD 2>/dev/null || echo '?')\","
  echo "  \"checks\": ["
  sed 's/^/    /' "$JSON" | sed '$!s/$/,/'
  echo "  ]"
  echo "}"
} >"$OUT/summary.json"

rm -f "$ROWS" "$JSON" "$NOTES" "$EXPECTS"

echo
# Echo the report down to (not including) the Frames section. `sed …q` rather than
# `head -n -1`, which is a GNU extension BSD/macOS head does not have.
sed -n '/^## Frames/q;p' "$REPORT"
echo "full report: $REPORT"
if [ "$KEEP_VISUAL" = "1" ]; then
  echo "visual left running (pid $VISUAL_PID, ns=$ORGANON_IPC_NS) — kill it yourself"
fi

exit "$FAILED"
