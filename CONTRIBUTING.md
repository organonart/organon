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
cargo build --release --features shell-edition --bin organon-console
```

CI runs exactly this matrix (default / mind / shell / Windows) on every PR.

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
