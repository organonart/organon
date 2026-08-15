#!/usr/bin/env bash
# Build the plugin bundles with the visual binary embedded inside them, so the
# editor's "Open Visual Window" button works from any host. Re-signs ad-hoc.
#
# With --with-llm, ALSO embed the llama.cpp inference runtime
# (organic-math-mind-runtime, #367) inside each bundle at
# Contents/MacOS/organic-math-mind-runtime — same as the visual — so the plugin
# can launch it as a child (Mind tab), no separate terminal. Needs cmake. OFF by
# default so the normal fast path stays llama.cpp/C++-free.
#
# Usage: ./bundle.sh              (release)
#        ./bundle.sh --install    (also copy the .vst3 to ~/Library/Audio/Plug-Ins/VST3)
#        ./bundle.sh --with-llm   (also build + embed the mind runtime in each bundle)
set -euo pipefail
cd "$(dirname "$0")"

# --- options (order-independent) ---
INSTALL=0
WITH_LLM=0
for arg in "$@"; do
  case "$arg" in
    --install) INSTALL=1 ;;
    --with-llm) WITH_LLM=1 ;;
    *) echo "bundle.sh: unknown option '$arg' (usage: ./bundle.sh [--install] [--with-llm])" >&2; exit 1 ;;
  esac
done

# organon#49 T4c-ii — the visual is its own package now (`organon-visual`), so it needs
# `-p`; the `world` feature is on by that package's own manifest, not asked for here.
# The OUTPUT path is unchanged — one workspace, one target/ — so the copy below is untouched.
cargo build --release -p organon-visual --bin organic-math-visual
cargo xtask bundle organic-math-native --release

# Optionally build the embedded llama.cpp inference runtime (#367 Tier 2c) so it
# can be embedded inside the bundles below. Guarded on cmake (needed for llama.cpp).
if [ "$WITH_LLM" = "1" ]; then
  if ! command -v cmake >/dev/null 2>&1; then
    echo "bundle.sh --with-llm: cmake not found — it's needed to build llama.cpp." >&2
    echo "  Install it with:  brew install cmake" >&2
    exit 1
  fi
  echo "→ building embedded-llm runtime (organic-math-mind-runtime; first build is slow)…"
  cargo build --release --features embedded-llm --bin organic-math-mind-runtime
fi

VISUAL="target/release/organic-math-visual"
RUNTIME="target/release/organic-math-mind-runtime"
for ext in vst3 clap; do
  B="target/bundled/Organon.$ext"
  [ -d "$B" ] || continue
  cp "$VISUAL" "$B/Contents/MacOS/organic-math-visual"
  if [ "$WITH_LLM" = "1" ]; then
    cp "$RUNTIME" "$B/Contents/MacOS/organic-math-mind-runtime"
    echo "embedded mind runtime: $B"
  fi
  codesign --force --deep -s - "$B" >/dev/null 2>&1 || true
  echo "embedded visual + signed: $B"
done

if [ "$INSTALL" = "1" ]; then
  DEST="$HOME/Library/Audio/Plug-Ins/VST3"
  mkdir -p "$DEST"
  rm -rf "$DEST/Organon.vst3" "$DEST/Organic Math.vst3"   # old name too — same VST3 class ID
  cp -R "target/bundled/Organon.vst3" "$DEST/"
  codesign --force --deep -s - "$DEST/Organon.vst3" >/dev/null 2>&1 || true
  echo "installed: $DEST/Organon.vst3"
fi
