# Where the build time actually goes

**Measured on ORGANON-ONE, 2026-08-20/21, against `f29ba06`.** AMD Threadripper PRO
9955WX (16C/32T), 31 GB RAM, `rustc 1.97.1` MSVC, `native/target` emptied before each
cold run. Every number below is wall-clock around one `cargo` invocation; where a run
shared the machine with another agent's build that is stated, because it invalidates the
timing.

This document exists because the ranking everyone (including me) assumed turned out to be
wrong, and the reason is visible in one `--timings` report.

## The headline

```
cargo build --release --features console-edition --bin organon-console   # cold
```

**462 s** (cargo reports 7m41s), matching an earlier independent measurement of 7m19s.
The `--timings` report covers **352 units** whose durations sum to **993 s of CPU** — so
the dependency graph parallelises about 11× across the 16 cores. Wall clock resolves to
three phases, and they are almost perfectly serial with respect to each other:

| phase | wall | share |
|---|---|---|
| all ~350 dependency units, in parallel | 0 → 53 s | **11.5%** |
| `organic-math-native` **lib** | 53 → 271 s (218.4 s) | **47.3%** |
| `organic-math-native` **bin `organon-console`**, incl. link | 271 → 462 s (190.5 s) | **41.2%** |

🚨 **88.5% of a cold build is two serial compilations of a single crate.** The entire
dependency graph — wgpu, egui, naga, ash, nih_plug, 350 units of it — is **53 seconds**.
Every individual dependency is small: the largest non-workspace unit is `naga` at 19.8 s,
and nothing else exceeds 18 s.

⚠️ **This inverts the intuition that a fresh worktree is expensive because it "rebuilds
the whole dependency graph cold".** It does, and that costs 11.5%. What actually costs is
that `organic-math-native` is one enormous crate that must be compiled twice, end to end,
with nothing else able to proceed in between.

## What that implies for caching

A compilation cache (`sccache`, or any shared-artifact scheme) can only remove work whose
inputs it has seen before. A dispatched agent **edits the root crate** — that is what it
was dispatched to do — so both dominant units miss by construction. The reachable ceiling
for a cache on this build is therefore the **53 s dependency tail, not the 409 s that
dominates it.**

📌 That is not an argument against caching; 53 s per worktree across 16 live worktrees is
real. It is an argument against expecting it to be the structural fix. **The structural
fix is the shape of the root crate**, and this measurement is the evidence for that claim
rather than a hunch about it.

⚠️ Two properties of `sccache` specifically remain **unmeasured** and would need settling
before anyone claims a figure. Both are load-bearing and neither is visible from a
single-worktree test:

1. **Cross-worktree keying.** The dispatch case is a *different absolute path*, not merely
   an empty `target/`. Re-running in the same worktree does not exercise it. `sccache`
   folds the working directory into its hash when debug info is on — otherwise cached
   objects would carry another worktree's paths — so a hit rate measured in one directory
   says nothing about the case that matters.
2. **Incremental compilation.** `sccache` refuses to cache incremental units. Cargo enables
   incremental for `dev`/`test` by default and `native/Cargo.toml` sets no `incremental`
   key; `target/debug/incremental` on this machine holds 25 directories, so it is
   genuinely on. The expensive *test* legs may therefore gain nothing at all from a cache
   until incremental is disabled — which is its own trade, because it is what makes a
   human's repeated local rebuild fast.

## The linker

MSVC's `link.exe` versus LLVM's `lld-link` (22.1.8, already installed for `libclang`), on
the bin unit alone. Isolated with `cargo rustc`, which appends flags to the **final unit
only**, so exactly one unit recompiles:

```
cargo rustc --release --features console-edition --bin organon-console -- \
  -C "linker=C:/Program Files/LLVM/bin/lld-link.exe"
```

| run | linker | wall | machine |
|---|---|---|---|
| A1 | `link.exe` | 223 s | exclusive |
| A2 | `link.exe` | 184 s | contended |
| B1 | `lld-link` | 195 s | contended |

🚨 **This is not a result, and must not be quoted as one.** The two `link.exe` runs differ
by **39 s**, which is larger than the 28 s gap between A1 and B1 — the noise floor exceeds
the effect. A clean answer needs interleaved repeats on a machine nobody else is using.
What can be said is only that lld-link is *not obviously slower*, and that it acts on the
bin unit, which is on the critical path of every build in every worktree.

⚠️ **`/STACK:33554432` survives the swap — verified, not assumed.** `native/.cargo/config.toml`
sets that 32 MB stack in a `cfg(all(windows, target_env = "msvc"))` table, and it is what
stops the console overflowing during `OrganonPanels::new`. Cargo will not accept `linker`
inside a `cfg()` table, so the linker has to arrive by another route, and whether the two
still compose is exactly the kind of thing that fails silently. Both checks pass:

```
llvm-readobj --file-headers target/release/organon-console.exe | grep StackReserve
  SizeOfStackReserve: 33554432
```

and the lld-linked binary **launches** — window `Pi — Organon Console`, alive at 15 s,
393 MB working set, closed cleanly. The header check alone would not have proved it; the
failure mode is a startup crash, so the binary has to be run.

⚠️ **Neither `lld-link` nor `sccache` can be configured in the repo.** `C:\Program Files\LLVM\bin`
is on neither the user nor the machine PATH here, so `-C linker=lld-link` cannot resolve
and needs an absolute path — and an absolute Windows path committed to
`native/.cargo/config.toml` would break every other Windows checkout and the Windows CI
leg. The same objection retires a repo-level `[build] rustc-wrapper`: macOS contributors
and CI have no `sccache`, and would get a missing-binary failure. Both belong in
machine-scoped `~/.cargo/config.toml`, documented here rather than imposed there.

## Method notes worth keeping

⚠️ **Do not change the linker through `RUSTFLAGS` or `.cargo/config.toml` to measure it.**
Either changes the fingerprint of *every* target unit, so each arm costs a full 462 s
rebuild instead of a 190 s one. `RUSTFLAGS` additionally *replaces* `target.*.rustflags`
rather than appending to it, which would silently drop the `/STACK` flag and hand you a
binary that dies at startup for a reason unrelated to what you were measuring.
`cargo rustc` confines the flag to the final unit; A1 taking 223 s rather than 462 s is
the evidence that the confinement works.

⚠️ **A timing taken while another agent is building is not a timing.** This machine
routinely has several dispatched agents compiling at once; B1 acquired a competitor 88 s
into its run. Record the contention alongside every number, or the table quietly becomes
fiction.

⚠️ **Verify the artifact, never the exit code.** A `cargo install sccache` here reported
success while installing nothing: the command was piped to `tail`, and the pipeline
returned `tail`'s status. The binary's absence was the only honest signal.

## Not measured

`sccache` end to end (populate, then a genuinely different worktree, then a hit rate from
`--show-stats`); `[profile.test]` debug-info settings, for which `native/Cargo.toml`
currently has no `[profile.test]` at all, so tests inherit `[profile.dev]`'s
`opt-level = 1` and full debug info. Both were displaced by machine contention rather than
by any finding, and neither should be landed on reasoning alone.
