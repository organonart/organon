### CI: Organon Mind stops being a leg, and the path filter stops guarding a repo we do not have

Organon Mind is no longer a separate product or a separate build — there is one Organon,
plus the VST it exports — so `.github/workflows/ci.yml` no longer runs a
`build (mind-edition)` job or a `Check (mind-edition, Windows target)` step. Five legs
remain: `build (default)` and `build (console-edition)` on Linux, the Windows
cross-check, a real Windows build+test, and the macOS Console build+test.

📌 **The saving is real but it is not time.** Measured on run 32443462639 (PR #105,
2026-08-21, warm cache, every leg green): the Mind leg cost 5m58s of Linux
runner and its cross-check step another 15s, while the whole run's wall clock was
22m31s and set by `build (windows)` alone — the legs run in parallel and Windows is 3x
the next slowest. The leg went because it policed a front-of-house that no longer
exists, not because anyone was waiting on it. That per-leg table now lives in the
workflow header in one place, replacing four separate figures that had drifted to
roughly double the current numbers (they were cold-cache measurements from the legs'
first-ever runs, never refreshed).

⚠️ **What is deliberately NOT claimed: the `mind-edition` cargo feature still exists.**
So does the `organon-mind` package, which is a workspace member and therefore still
built and tested by `cargo test --release --workspace` in the default leg — that
coverage did not move. What is gone is the leg that turned the *feature* on. Until the
feature is deleted from `native/Cargo.toml`, a change that breaks
`--features mind-edition` lands green, and `CLAUDE.md` and `CONTRIBUTING.md` now say so
where they previously promised that CI covered every edition. If you touch `edition.rs`,
`lib.rs`'s cfg arms or `world.rs`, run the check yourself:

```bash
cd native && cargo check --workspace --all-targets --features mind-edition
```

**`paths-ignore` loses eight entries, and the reason is the opposite of tidiness.** It
carried patterns for `site/`, `site-mind/`, `web/`, `src/` (the legacy
React-Three-Fiber app), `brand/`, `songs/`, `original_code/` and the npm/vite config
files. Not one of those paths exists in this repository; they crossed over with the file
when it did. A pattern for a directory that is not here does nothing today and, the day
someone adds that directory back, silently exempts it from CI — which is exactly the
default-deny failure the filter's own header paragraph rejects ("a new top-level
directory gets tested until someone says otherwise"). What is left is `doc/**`,
`.claude/**`, `**.md` and `.gitignore`, with the `!doc/reference/**` carve-out still
last, where negation order requires it.
