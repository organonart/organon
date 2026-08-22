#!/usr/bin/env bash
# THE doc→source mapping. Sourced by both doc hooks; not executable on its own.
#
#   architecture-doc-check.sh   (Stop)         "you changed X without Y"
#   doc-staleness-check.sh      (SessionStart) "Y has fallen behind X over time"
#
# Two hooks, two different questions, ONE table — deliberately. Keeping a second
# copy of this mapping in the second hook is the exact failure organon#590 is about:
# an unenforced duplicate of a maintained thing rots, and nothing notices. If you add
# a durable doc, add it here once and both hooks pick it up.
#
# Format: one rule per line, "<doc>|<space-separated trigger paths>".
#
# A trigger may be a literal path OR a glob (`*` matches within one path segment).
# Globs matter: the RT subsystem is seven `rt*.rs` files and the render doc covers 54
# shaders, so enumerating today's files would silently stop covering tomorrow's —
# which is the drift these hooks exist to prevent. Both consumers disable pathname
# expansion (`set -f`) so these reach the matcher as patterns rather than being
# expanded against the working tree.
#
# Choosing triggers: list the files a doc is ACCOUNTABLE for, not every file it
# mentions. MIND_ARCHITECTURE.md's triggers are Mind-only because the shared spine
# (params/ipc) already nudges ARCHITECTURE.md; listing it under both would fire on
# nearly every native change and train everyone to ignore the reminder.

DOC_RULES="
ARCHITECTURE.md|native/src/params.rs native/organon-core/src/ipc.rs native/src/param_table.rs
doc/arch/topology.md|native/Cargo.toml native/organon-core/Cargo.toml native/organon-mind/Cargo.toml native/organon-render/Cargo.toml native/organon-scene/Cargo.toml native/organon-agent/Cargo.toml native/organon-world/Cargo.toml native/organon-visual/Cargo.toml native/organon-console/Cargo.toml native/organon-module/Cargo.toml native/tools/crate-churn.py
doc/arch/render.md|native/organon-render/src/*.rs native/organon-render/src/*.wgsl native/organon-world/src/world.rs native/organon-world/src/rt.rs native/organon-world/src/*.wgsl native/organon-visual/src/*.rs native/src/*.wgsl
MIND_ARCHITECTURE.md|native/organon-core/src/edition.rs native/organon-mind/src/*.rs native/organon-core/src/gguf*.rs native/src/mind_main.rs native/src/bin/mind_runtime.rs native/src/bin/mind_writer.rs
CONSOLE_ARCHITECTURE.md|native/organon-console/src/*.rs native/organon-console/Cargo.toml native/organon-module/src/*.rs native/organon-module/Cargo.toml
.claude/skills/organon-cli/SKILL.md|native/src/bin/ctl.rs native/src/cli.rs
site/index.html|doc/organon_prd.md
web/ARCHITECTURE.md|web/src/contracts/sharedState.ts web/src/contracts/generatorOutput.ts web/src/contracts/renderer.ts web/src/contracts/stateSource.ts web/src/render/pbrRenderer.ts web/src/render/webgpuRenderer.ts web/src/state/store.ts native/organon-wasm/src/lib.rs native/organon-manifest/src/lib.rs
"

# 📌 The site/index.html row is STALENESS-ONLY, and deliberately so. organon.art quotes
# doc/organon_prd.md §1.1 (the identity claim) and §12 (the state of play) rather than
# carrying its own wording — that is the whole reason the page lives in this repository,
# since the claim was once spelled six ways and went stale in all six at once. A public
# page is the surface most likely to become the seventh.
#
# ⚠️ Its ONE trigger is the PRD, not the console sources, even though the page's plates
# also quote the real region and content words. `native/organon-console/src/*.rs` churns
# constantly and would fire this rule most weeks, which is how a reminder gets ignored —
# the calibration note below is explicit that a reminder people learn to dismiss is worse
# than no reminder. The vocabulary obligation is carried in site/README.md and CLAUDE.md's
# doc table instead, where a person changing a region word will meet it.
#
# There is no case for it in architecture-doc-check.sh, so it contributes no same-change
# nudge — same arrangement as web/ARCHITECTURE.md, and for the same reason: the PRD moves
# rarely, and drift across sessions is the failure worth catching.

# 📌 The web/ARCHITECTURE.md row STAYS despite #418 being parked (#626 Tier 2), and
# so do its `organon-wasm` / `organon-manifest` triggers. This is deliberate, not an
# oversight: a rule whose triggers never fire costs exactly nothing — the matcher
# walks a handful of extra strings per hook run and finds no hit, forever. What it
# buys is reversibility. Resuming #418 should be re-adding one entry to
# settings.json (see load-web-architecture-doc.sh), not archaeology to reconstruct
# which files a doc was accountable for. Deleting the row would trade a zero cost
# for a real one.
#
# NOTE on web/ARCHITECTURE.md: its *same-change* reminder is owned by the separate
# web-architecture-doc-check.sh (Stop), which predates this table. It is listed here so
# the *staleness* check covers it too — architecture-doc-check.sh has no reason-text case
# for it and therefore skips it, which is deliberate: one same-change reminder per doc,
# not two.
#
# Its four contract files are listed EXPLICITLY rather than as `contracts/*.ts`, to stay
# byte-for-byte in step with that hook's trigger set. The glob would sweep in
# `contracts.test.ts`, `types.ts`, `paramManifest.ts`, `audioFeatures.ts` and `index.ts`,
# which that hook excludes on purpose ("deliberately narrow — routine UI/param tweaks
# don't trigger"). Two checks claiming the same trigger set must actually have it; the
# glob version shipped in the first cut of #597 and did not.

# How many commits may touch a doc's triggers before the SessionStart staleness check
# calls it drifted. NOT a same-change gate — architecture-doc-check.sh covers that; this
# is the backstop for drift accumulating across sessions, unnoticed.
#
# 8 is calibrated against measured history, and the measurement is worth recording
# because it is mildly surprising. True peak drift ever reached, per doc:
#
#     ARCHITECTURE.md        1
#     MIND_ARCHITECTURE.md   4     ← and this one had NO hook at all until #595
#     web/ARCHITECTURE.md    0
#
# So the same-change discipline has actually been holding: docs get updated within a
# few commits, hook or no hook. 8 is 2× the observed peak — high enough that normal
# rhythm never trips it, low enough to catch a genuine lapse.
#
# ⚠️ Consequence, stated so nobody mistakes silence for proof: **this check would not
# have fired at any point in the repo's history.** That is a calibration result, not a
# defect — it is a regression detector for a discipline that is currently working, and
# a reminder people learn to ignore is worse than no reminder. To see what it says,
# override the threshold:  DOC_STALE_THRESHOLD=0 bash .claude/hooks/doc-staleness-check.sh
DOC_STALE_THRESHOLD="${DOC_STALE_THRESHOLD:-8}"
