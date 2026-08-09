#!/usr/bin/env bash
# Build + bundle (visual embedded, ad-hoc signed) and install the .vst3 to the
# user's custom Ableton VST3 folder. Run this on the Mac after every native
# change — it's the "deploy-native-build" standing rule.
#
# NOTE: macOS only. A remote/Linux Claude Code session CANNOT run this (no Mac,
# no codesign, no ~/Documents/vst3) — it builds + tests there and you deploy here.
#
# Usage: ./deploy.sh [--dest DIR] [--with-llm]
#   --dest DIR  VST3 install folder. Defaults to ~/Documents/vst3, which is a
#               PERSONAL choice (the author's Ableton custom folder), not a macOS
#               convention. The standard user location is
#               ~/Library/Audio/Plug-Ins/VST3, which is where bundle.sh --install
#               puts it — the two scripts disagreed silently until this flag, and
#               the first public-repo trial landed the plugin where its DAW would
#               never scan. Pass --dest to choose; the Windows arm has had -Dest
#               since #658 T3 and this is the same idea.
#   --with-llm  ALSO build the embedded llama.cpp inference runtime
#               (organic-math-mind-runtime, #367 Tier 2c). It is now embedded
#               INSIDE the installed .vst3/.clap (Contents/MacOS/) — bundle.sh
#               --with-llm does that — so the plugin can launch it as a child
#               (Mind tab), no separate terminal. A standalone copy is ALSO
#               installed next to the bundle for direct/terminal use. Needs cmake.
#               OFF by default so a normal deploy stays llama.cpp/C++-free + fast.
set -euo pipefail
cd "$(dirname "$0")"

# --- options ---
WITH_LLM=0
DEST="$HOME/Documents/vst3"
while [ $# -gt 0 ]; do
  case "$1" in
    --with-llm) WITH_LLM=1 ;;
    --dest) shift; [ $# -gt 0 ] || { echo "deploy.sh: --dest needs a directory" >&2; exit 1; }; DEST="$1" ;;
    --dest=*) DEST="${1#--dest=}" ;;
    *) echo "deploy.sh: unknown option '$1' (usage: ./deploy.sh [--dest DIR] [--with-llm])" >&2; exit 1 ;;
  esac
  shift
done

if [ "$(uname)" != "Darwin" ]; then
  echo "deploy.sh: macOS only (this host is $(uname)). Build/test only here; deploy on the Mac." >&2
  exit 1
fi

# Build + embed the visual + ad-hoc sign the bundle (target/bundled). With
# --with-llm, bundle.sh ALSO embeds the mind runtime inside each bundle (and
# guards on cmake), so the plugin can launch it itself.
if [ "$WITH_LLM" = "1" ]; then
  ./bundle.sh --with-llm
else
  ./bundle.sh
fi

SRC="target/bundled/Organon.vst3"
mkdir -p "$DEST"
rm -rf "$DEST/Organon.vst3" "$DEST/Organic Math.vst3"   # remove the old name too — same VST3 class ID
cp -R "$SRC" "$DEST/"
codesign --force --deep -s - "$DEST/Organon.vst3"
echo "installed: $DEST/Organon.vst3"

# Install the `organon` CLI (#452 Tiers 1–2) — the local command surface for
# external agents (Bianca) and terminal use: status/catalog/get/watch read the
# live IPC snapshot; set/do/release/generator/surface/material queue ops the
# visual drains into the Performer's override lane. Built explicitly —
# bundle.sh only builds the plugin + visual. Symlinked into the first writable
# bin dir on the list below so plain `organon` works in a shell.
cargo build --release --bin organon
cp target/release/organon "$DEST/organon"
codesign --force -s - "$DEST/organon"
echo "installed CLI: $DEST/organon"
# ⚠️ **/usr/local/bin does not exist on a stock Apple Silicon Mac.** This checked only
# there, so on every M-series machine without Intel-era Homebrew the symlink silently
# no-opped and a "successful" deploy left NO `organon` on PATH at all — while CLAUDE.md
# went on claiming deploy "installs the organon CLI". The first public-repo trial only had
# a working CLI because of a hand-made symlink from weeks earlier. Try the real bin dirs in
# order and say plainly when none took.
CLI_LINKED=""
for bindir in /opt/homebrew/bin /usr/local/bin "$HOME/.local/bin"; do
  if [ -d "$bindir" ] && [ -w "$bindir" ]; then
    ln -sf "$DEST/organon" "$bindir/organon"
    echo "linked: $bindir/organon"
    CLI_LINKED="$bindir"
    break
  fi
done
if [ -z "$CLI_LINKED" ]; then
  echo "note: no writable bin dir found (/opt/homebrew/bin, /usr/local/bin, ~/.local/bin)."
  echo "      run the CLI as $DEST/organon, or add that folder to PATH:"
  echo "        echo 'export PATH=\"$DEST:\$PATH\"' >> ~/.zshrc"
fi
# Tab completion (zsh — the macOS default): install into site-functions when
# writable, else print the on-the-fly line. bash/fish: `organon completions --help`.
# Same Apple Silicon problem, same fix: prefer Homebrew's prefix, which is where a
# stock M-series `fpath` actually points.
if [ -d /opt/homebrew/share ]; then
  ZFUNC=/opt/homebrew/share/zsh/site-functions
else
  ZFUNC=/usr/local/share/zsh/site-functions
fi
if mkdir -p "$ZFUNC" 2>/dev/null && [ -w "$ZFUNC" ]; then
  "$DEST/organon" completions zsh > "$ZFUNC/_organon"
  echo "installed zsh completions: $ZFUNC/_organon (restart your shell to pick up)"
else
  echo "note: for tab completion, add to ~/.zshrc:  source <(organon completions zsh)"
fi

# Install the loadable network gallery (#226) into the app-support store, next to
# presets.json + clips/. The plugin's "Load Network (JSON)…" dialog opens here
# (preset::networks_dir), so the connectome / MLP / attention demos are one click
# away. Idempotent — re-copies every deploy so the installed set matches the repo.
NET_DEST="$HOME/Library/Application Support/OrganicMath/networks"
mkdir -p "$NET_DEST"
cp assets/networks/*.json "$NET_DEST/"
echo "installed gallery: $NET_DEST ($(ls "$NET_DEST"/*.json | wc -l | tr -d ' ') files)"

# Install the procedural material graph gallery (#472 Tier 4) into the app-support
# store, next to networks/. The Material card's "Load Material Graph…" dialog opens
# here (preset::material_graphs_dir), so the nacre / weathered-stone / brick graphs
# are one click away. Idempotent — re-copies every deploy so it matches the repo.
MATGRAPH_DEST="$HOME/Library/Application Support/OrganicMath/materials"
mkdir -p "$MATGRAPH_DEST"
cp assets/materials/graphs/*.json "$MATGRAPH_DEST/"
echo "installed material graphs: $MATGRAPH_DEST ($(ls "$MATGRAPH_DEST"/*.json | wc -l | tr -d ' ') files)"

# Install the Creature Engine body-plan gallery (#476 Tier 2b) into the app-support
# store, next to networks/. The plugin's "Load Creature (JSON)…" dialog opens here
# (preset::creatures_dir), so the authored body plans are one click away.
# Idempotent — re-copies every deploy so the installed set matches the repo.
CREATURE_DEST="$HOME/Library/Application Support/OrganicMath/creatures"
mkdir -p "$CREATURE_DEST"
cp assets/creatures/*.json "$CREATURE_DEST/"
echo "installed creatures: $CREATURE_DEST ($(ls "$CREATURE_DEST"/*.json | wc -l | tr -d ' ') files)"

# Install the Field Playback clip gallery (#407 Tier A) into the app-support store,
# next to networks/. The plugin's "Load Field Clip…" dialog opens here
# (preset::fields_dir), so the baked The Well demo clips are one click away.
# Idempotent — re-copies every deploy so the installed set matches the repo.
FIELD_DEST="$HOME/Library/Application Support/OrganicMath/fields"
mkdir -p "$FIELD_DEST"
cp assets/fields/*.bin "$FIELD_DEST/" 2>/dev/null || true
echo "installed field clips: $FIELD_DEST ($(ls "$FIELD_DEST"/*.bin 2>/dev/null | wc -l | tr -d ' ') files)"

# Install the Neural CA model gallery (#407 Tier B) into the app-support store, next
# to presets.json + networks/. The Field Engine card's "Load NCA Model (JSON)…" dialog
# opens here (preset::nca_dir), so the trained NCA weights are one click away. The
# built-in default (nca-default.json) ships so it works out of the box. Idempotent.
NCA_DEST="$HOME/Library/Application Support/OrganicMath/nca"
mkdir -p "$NCA_DEST"
cp assets/nca/*.json "$NCA_DEST/"
echo "installed NCA gallery: $NCA_DEST ($(ls "$NCA_DEST"/*.json | wc -l | tr -d ' ') files)"

# Also install a STANDALONE copy of the embedded llama.cpp inference runtime
# (#367 Tier 2c) next to the bundle. bundle.sh --with-llm (above) already embedded
# it INSIDE the .vst3/.clap and built it (guarding on cmake), so this just installs
# the freshly-built binary — harmless, and handy for direct/terminal use or tests.
# It uses the SAME default IPC namespace as the installed Organon.vst3, so they pair up.
if [ "$WITH_LLM" = "1" ]; then
  RUNTIME_DEST="$DEST/organic-math-mind-runtime"
  cp "target/release/organic-math-mind-runtime" "$RUNTIME_DEST"
  codesign --force -s - "$RUNTIME_DEST"
  echo "installed runtime: $RUNTIME_DEST"
  echo "  → The runtime is now ALSO embedded in the installed Organon.vst3/.clap, so the"
  echo "    plugin can launch it itself — no separate terminal needed. In Organon's Mind"
  echo "    tab: Load a .gguf → Live (streaming) → prompt → Generate (the track must be"
  echo "    processing audio — play / monitor In)."
  echo "  → To run the standalone copy by hand instead, leave it running in a terminal:"
  echo "      \"$RUNTIME_DEST\""
  echo "    For the #317 Performer, point \$TMPDIR/organic-math-agent.txt line 1 at"
  echo "      http://127.0.0.1:1234/v1/chat/completions"
fi

echo "→ Rescan in Ableton (Settings → Plug-Ins) — the visual is a separate process,"
echo "  so close + reopen the visual window too if it was open."
