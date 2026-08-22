# Contributing to Organon

Functional changes are very welcome. This document is the short version of how work
actually gets made here — the invariants that break expensive things, the shape of a
change, and what "done" honestly means.

## Before anything else: five invariants

These are not style preferences. Each one has cost someone real work.

1. **Never touch the VST3 class ID or the CLAP ID.** `VST3_CLASS_ID` and `CLAP_ID` in
   `native/src/lib.rs` identify the plugin to every DAW that has ever loaded it. Changing
   either orphans the device in every saved session — the most destructive edit available
   in this codebase, and it looks like tidying up. The CLAP ID contains an old studio name;
   that is *why* it is commented, and it stays. Equally: never add a second plugin
   identity. Organon Mind and Organon Console are standalone-only, permanently.

2. **`Shared` — the IPC snapshot — is append-only.** The plugin process writes it and the
   visual process reads it at fixed byte offsets. Never reorder or insert fields. Append
   at the tail, bump `LAYOUT_VERSION`, re-pin the goldens, and give new preset fields a
   serde default so old presets still load.

3. **A param is a chain, not a line.** A new parameter touches `params.rs` →
   `param_table.rs` → `to_shared()` → `ipc.rs` → the visual's `build_uniforms`/shader, and
   usually `clip.rs` (the CC map) and `preset.rs` (capture/apply). Follow the whole chain
   or the parameter will exist and do nothing. `ARCHITECTURE.md` §17 is the checklist.

4. **New capability defaults to inert.** Off, or set to a value that reproduces today's
   output. This is what lets large features land in pieces over weeks without changing
   anyone's saved look.

5. **Branch off the default branch, not off another PR.** A PR stacked on a branch that
   later gets deleted can report "merged" and never actually reach the main line. It has
   happened here twice.

## The shape of a change

**Small fix** — open a PR. That is the whole process.

**`main` is a protected branch, so a PR is not a convention — it is the only route in.**
A ruleset blocks direct pushes, force-pushes and deletion, and requires every review
thread to be resolved before merge. `.github/rulesets/README.md` is the source it was
built from and explains each rule, including the four that are deliberately switched
off and what would make each one worth turning on.

**Anything larger** — open an issue first and let it be discussed. Big work here is
structured in **tiers**: 3–5 increments of ascending sophistication where **Tier 1 is
independently shippable** and each later tier is inert by default (invariant 4). That is
what allows a feature to land across weeks without a long-lived branch. If you propose a
feature this way, you will find the maintainers meet you much faster.

**Where new capability goes** — figure out which extension point you are adding to, then
read that section of `ARCHITECTURE.md` for the mechanics:

| You're adding | The slot | Lives in |
|---|---|---|
| a new motion algorithm | a `GeneratorMode` variant | `math.rs` — pure, unit-test it |
| a new way nodes become geometry | a `SurfaceMode` variant | `math.rs` + a mesh in the renderer |
| a new way surfaces shade | a `MaterialType` branch | `cube.wgsl` |
| a new pixel-stage effect | a post/GI pass | `post.rs`/`gi.rs` + a `.wgsl` + composite wiring |
| new controls | a param block | `params.rs` → the whole chain (invariant 3) |
| a new Organon Mind lens | a `NeuralGraph` builder | `math.rs`; `MIND_ARCHITECTURE.md` §5 maps the seams |

For Mind specifically there is a sixth invariant, and it is the product: **every displayed
quantity carries its provenance** — measured / derived / proxy / projection. A new readout
gets its marker and a row in `MIND_ARCHITECTURE.md` §3's honesty ledger. A readout that
looks authoritative and is actually a proxy is the one bug this project will not ship.

## Verification — what "done" means

Run before you open a PR:

```bash
cd native
cargo build --release
cargo test --release --workspace
```

If your change touches shared ground — `lib.rs`, `params.rs`, `preset.rs`, `ipc.rs`,
`world.rs`, the `mind_*` modules, anything in `organon-core` — **build the other editions
too**, because their features are default-off and a green suite says nothing about them:

```bash
cargo build --release --features mind-edition  --bin organon-mind
cargo build --release --features console-edition --bin organon-console
```

CI runs most of this matrix (default / console, on Linux, Windows and macOS) on every
PR — but **not** `mind-edition`. Organon Mind stopped being a separate product in
August 2026 and its leg went with it, while the cargo feature stayed; see the header of
`.github/workflows/ci.yml`. So the mind-edition line above is the one in this block that
nothing checks for you, and it is on you to run it while it still exists.

🚨 **The targeted bar, for when `--workspace` is not practical — and it is EIGHT commands.**
On a workstation `cargo test --release --workspace` is often not affordable (a cold per-worktree
`target/`, and the box's RAM is shared with WSL), so sessions run a targeted substitute instead.
Written down here because it circulates in briefs and handoffs, and the version that circulated
until 2026-08-22 had **six** commands and a hole in it:

```bash
cd native
cargo test  -p organon-console --lib
cargo test  -p organon-core
cargo check --features console-edition --bin organon-console
cargo check --tests -p organic-math-native --features console-edition
cargo test  -p organic-math-native --bin organon-console --features console-edition
cargo test  -p organic-math-native --bin organon --features console-edition
cargo test  -p organic-math-native --lib  --features console-edition   # ← the one that goes missing
cargo test  -p organon-module --all-features                           # ← the second hole
```

⚠️ **And the eighth is the same hole one crate over — in the crate BOTH repositories depend on.** Legs 1–2 cover `organon-console` and `organon-core`, legs 3–7 the root crate. **No leg of the bar ran `organon-module`** — the contract crate a module's own repository pins. 🚨 **Its tests were not unrun; CI ran them all along**, in release, on three platforms: features unify across a workspace build, and the root crate depends on `organon-module` with `features = ["wgpu"]`, so `cargo test --workspace` compiles it with `wgpu` on and runs every one. **The bar had a hole; the gate did not.** ⚠️ **No count appears in this paragraph, deliberately** — it named one, and within a day the number was stale and sat a sentence away from a different one. This file's own rule applies to this file: measure the crate's own. A change landing there could report *"the bar is green"* in good faith with none of its own tests run — which is leg 7's failure exactly, one crate over, found after that class had already been found and closed once.

📌 `--all-features` rather than `--features wgpu`: it is the wider net, and it is safe under `CARGO_PROFILE_TEST_OPT_LEVEL=0` because the two timing-shaped staleness tests in that crate are `#[ignore]`d and never run.

⚠️ **Without the seventh line, the root crate's several hundred lib tests never run.** The fourth command
only `check`s that target and the fifth and sixth test *binaries*, so every unit test under
`native/src/` — `panel_table.rs`, `panel_surface.rs`, `preset.rs` and the rest — is compiled and
never executed. A change whose tests live there can report *"the bar is green"* in good faith while
none of its own tests has run. Measured 2026-08-22, and found only because a contributor's new
tests were entirely in that target and their count never moved.

✏️ **That sentence used to name a number — "the root crate's 324 lib tests" — and the number was
wrong within a day.** It was 324 when this paragraph was written, 332 that evening and 336 the next
morning, because several sessions merge in parallel here. The count is not the point; *that the
fourth command does not run them* is the point, and it is true at every count. A literal here would
have to be re-measured by whoever noticed, and the person most likely to notice is a contributor
deciding whether they have found a regression.

✏️ **It is EIGHT commands now, and this block was the stale copy.** An eighth leg —
`cargo test -p organon-module --all-features` — was added on 2026-08-22 after `organon-module`'s 85
tests turned out to be run by nothing: leg 7's failure exactly, one crate over, in the contract crate
a module's own repository pins. ⚠️ **That correction reached
`.claude/skills/coordinate-sessions/BRIEF.md` and not this file**, so the two disagreed for a day —
which is the same two-copies defect the bar itself keeps catching. `BRIEF.md` is the copy briefs are
generated from; keep them in step or delete one.

```bash
cargo test  -p organon-module --all-features    # ← the eighth leg
```

🚨 **And the class is not closed even at eight: the bar names four packages and the workspace has
ten.** Measured 2026-08-22 — it runs **2 171** tests and misses **366**, because `-p` selects a
package and cargo never runs a *dependency's* own `#[test]`s:

| Covered | | Missed entirely | |
|---|---|---|---|
| `organon-console` (leg 1) | 921 | `organon-world` | 165 |
| `organon-core` (leg 2) | 685 | `organon-mind` | 64 |
| `organic-math-native` (legs 3–7) | 480 | `organon-scene` | 49 |
| `organon-module` (leg 8) | 85 | `organon-agent` | 42 |
| | | `organon-render` | 36 |
| | | `organon-visual` | 10 |

⚠️ **`organon-mind` bites soonest** — it is a path dependency of the root crate, so legs 3–7
*compile* it and run none of its tests. A Mind PR whose tests live in `mind_viz.rs` or
`mind_train.rs` clears every leg without executing one of them. Found exactly that way: a worker on
#147 T4 ran `-p organon-mind` on its own initiative and reported 63 → 70 in a crate no leg touches.

📌 **This does not make the bar wrong; it makes its scope explicit.** It was always a *targeted*
substitute, and the four packages it names hold 86% of the tests. The failure mode is believing
"the bar is green" answers a question about a crate it never ran — the seventh-leg lesson one level
up, and the reason CI's `--workspace` stays the real gate. **Add `-p <crate>` for wherever your
change actually lives**, and say which leg ran your tests rather than that the bar is green.

📌 **`CARGO_PROFILE_TEST_OPT_LEVEL=0` turns roughly 43 minutes into roughly 70 seconds** for
this set. It changes codegen only, so it is a fair substitute for a debug-profile run and not for a
`--release` one.

🚨 **And it is *actively dangerous* for anything timing-shaped.** Codegen-only means no
test's **verdict** changes — unless the test's subject is **time**, in which case an unoptimised
binary is a different experiment. Measured 2026-08-22: the module staleness rig's simulator cannot
draw 1280×720 in 4 ms unoptimised, so every cadence in a sweep collapsed to one real period, the
lever was connected to nothing, and the rig concluded *"staleness is the TRANSPORT"* — a
recommendation to buy `unsafe` per-backend GPU interop, on a false premise, from a green run made
exactly as this paragraph advises.

⚠️ **So a timing rig must measure the quantity it varies rather than the knob it set** — read
the achieved period off the data, never off the flag — and must **fail naming the real cause** when
the sweep did not sweep. Run anything timing-shaped with `--release`, and say which you used where
the numbers are recorded.

🚨 **Never `--workspace` on `cargo test` without `--release`-scale time to spend, and never
a bare `cargo test`** — `native/`'s root *package* is `organic-math-native`, so a bare invocation
runs that package alone and skips `organon-core` **silently**. Extracting that crate once cost the
suite 44 tests while it stayed green.

📌 **Say which command ran your tests, and what the number was.** *"The bar is green"* and
*"my tests ran"* are different claims, and the gap between them is exactly what the seventh command
closes.

📌 **This block has a second home, and the two are pinned equal.** A session coordinating
other sessions cannot hand a worker a skill — a worker in its own worktree has the files and
not the skill — so the same eight commands are published at
`.claude/skills/coordinate-sessions/BRIEF.md`, where a worker can `git show` them out of a
checkout it already has. `.claude/hooks/bar-agreement-check.sh` diffs the two command blocks on
every Stop and refuses if they have forked. ⚠️ **Edit the bar here and the hook will tell you
about the other copy** — which is the point, because the six-command version drifted for months
precisely because nothing diffed prose.

**Be precise about what you verified.** `cargo test` includes offline shader validation,
so it catches binding, type and uniformity errors without a GPU — but it cannot see
pipeline/layout mismatches, runtime GPU behaviour, UI layout, or *the actual look*. A
green suite means "compiles and the logic tests pass", never "works". If you have a GPU,
`native/verify.sh` renders frames and diffs them against committed goldens; saying which
of these you ran is more useful than a confident summary.

## Documentation is part of the change, not a follow-up

Update `ARCHITECTURE.md` in the **same** change as any architectural shift — a new
generator, an IPC/`Shared` change, a render path, a param block, a material. `doc/arch/render.md`
owns the render pipeline; `MIND_ARCHITECTURE.md` owns Mind's living state and its honesty
ledger. Docs here are hook-enforced and current, which is only true because that rule is
kept.

**The user documentation is split in two, and the halves have different rules.**
`doc/guide/` is hand-written prose about how to *operate* Organon — update it when a
user-visible behaviour changes. `doc/reference/` is **generated** and must never be edited
by hand:

```bash
cargo run --bin organon -- docs          # regenerate doc/reference/
cargo run --bin organon -- docs --check  # report drift without writing
```

Its content comes from the descriptions compiled into the binary — `agent.rs`'s
`generator_desc` / `surface_desc` / `material_desc` / `param_desc` and `recipe.rs`. Adding
a generator, a surface, a material or an actuatable param therefore means **writing its
description in the Rust**, then regenerating. The `match`es are exhaustive and
`every_actuatable_id_has_a_gloss` is a test, so you cannot land an undescribed one; and
`generated_reference_is_current` fails the build if the checked-in Markdown no longer
matches what the code emits. Regenerate in the same commit.

## How to record a change

**Not by editing `CHANGELOG.md`.** New entries are one Markdown file each in
`changelog.d/`, concatenated into `CHANGELOG.md` at release time:

```bash
python3 native/tools/changelog.py new "What changed, as a heading"
python3 native/tools/changelog.py check
```

`new` prints the path it made — `changelog.d/YYYY-MM-DD-<your-branch>.md` — seeded with a
`### ` heading. Write the entry into it and commit it alongside the rest of your change.

A fragment is **exactly what would have gone under `## Unreleased`**: full paragraphs in
this project's house style, explaining the why and the trap, with 🚨/⚠️/📌 and code fences
where they earn their place. There is no frontmatter and no metadata to fill in,
deliberately — a form-shaped fragment would push everyone toward one-line bullets, and
that density is the part of the changelog worth keeping.

**Why a directory rather than a file.** `CHANGELOG.md` had one shared insertion point, so
any two open branches conflicted at the top of `## Unreleased` by construction, whether or
not they touched a single common concern. `merge=union` in `.gitattributes` fixed that for
`git` and not for GitHub — GitHub computes PR mergeability with its own three-way merge
that ignores merge drivers, so a PR read `CONFLICTING` while `git` resolved it silently,
and someone still had to merge `main` locally and push just to make the page agree. Two
branches writing two different files do not conflict at all. `changelog.d/README.md` has
the full story, including the ordering rule and the one residual collision case.

**`CHANGELOG.md` itself is the record and is not being rewritten** — everything already in
it stays where it is, `## Unreleased` included. That heading stays in the file
permanently as the release step's second input, so an entry written under the old scheme
on a long-lived branch is absorbed at the next release rather than orphaned.

## Two things specific to this repository

**It is generated, so do not send tidying patches.** This repo is produced from a private
monorepo by subtraction, and every file is byte-identical to its upstream counterpart.
That is what lets your patch apply cleanly on the other side. A PR that reformats,
renames or "cleans up" a file purely to improve this copy breaks that property and will be
declined — while the same effort spent on a functional change is genuinely wanted.

**Some references point somewhere you can't follow.** Issue numbers below ~#700, and some
`doc/` paths, live in the private repository. They are provenance for decisions, not
broken links, and the docs ship unmodified on purpose. If a doc's *content* is wrong, that
is a real bug — please report it.

## If what you found is a vulnerability

Don't open a PR or a public issue for it — the diff and the description are the exploit.
Report it privately instead:
[a security advisory](https://github.com/organonart/organon/security/advisories/new), or
`hello@organon.art` with `[security]` in the subject.
[`SECURITY.md`](SECURITY.md) says what the real surface is and, as usefully, which parts of
it are by design — worth two minutes before you spend an afternoon.

## Licence of your contribution

You licence your contribution under the terms of the crate you touched: **MIT OR
Apache-2.0** for the engine crates, **GPL-3.0-or-later** for the root plugin crate. No CLA.
See [`LICENSING.md`](LICENSING.md) — and if a change moves code *from* an engine crate
*into* the root crate, say so in the PR, because that direction relicenses it.
