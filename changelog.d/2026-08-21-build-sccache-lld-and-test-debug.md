### Measured where the build time actually goes, and it is not the dependency graph

A cold `cargo build --release --features console-edition --bin organon-console` on
ORGANON-ONE takes **462 s**, and `--timings` resolves it into three almost perfectly
serial phases: **53 s** for all ~350 dependency units in parallel, **218 s** for the
`organic-math-native` lib, then **190 s** for its `organon-console` bin including the
link. The 352 units sum to 993 s of CPU against 462 s of wall clock, so the dependency
graph parallelises about 11× across the 16 cores and costs almost nothing in wall time.

🚨 **88.5% of a cold build is two serial compilations of a single crate.** wgpu, egui,
naga, ash and nih_plug together are the other 11.5%; the largest single dependency unit is
`naga` at 19.8 s and nothing else exceeds 18 s. This was measured because a fresh worktree
is expensive and the assumed cause was "it rebuilds the whole dependency graph cold" — it
does, and that is 53 seconds of it.

⚠️ **The consequence is that a compilation cache cannot be the structural fix here.** A
cache only removes work whose inputs it has seen, and a dispatched agent edits the root
crate by definition, so both dominant units miss. The reachable ceiling for `sccache` on
this build is the 53 s dependency tail — worth having across 16 live worktrees, and not
the thing that makes a dispatch cost seven minutes. The structural fix is the shape of the
root crate, and `doc/build_timing_measurement.md` is now the evidence for saying so rather
than the hunch.

📌 **Nothing was landed as a build-configuration change, deliberately.** Three were
proposed — `sccache`, `lld-link`, and a `[profile.test]` debug-info setting — and none
earned it. `sccache` and `[profile.test]` were displaced by machine contention before they
could be measured at all. The linker A/B *was* run and is **inconclusive in a way worth
recording**: `link.exe` measured 223 s and 184 s on two runs of the identical unit, a 39 s
spread that is wider than the 28 s gap to `lld-link`'s 195 s. The noise floor exceeds the
effect, so there is no result, and reporting one would have been an invention.

⚠️ **Two things about the linker swap were verified rather than assumed, and both are
recorded because they are the parts that fail silently.** `native/.cargo/config.toml` sets
`/STACK:33554432` in a `cfg(all(windows, target_env = "msvc"))` table, which is what stops
Organon Console overflowing its stack inside `OrganonPanels::new`; cargo refuses `linker`
inside a `cfg()` table, so the two have to compose by a different route. After linking
with `lld-link`, `llvm-readobj --file-headers` still reports `SizeOfStackReserve:
33554432`, **and** the binary launches — window `Pi — Organon Console`, alive at 15 s,
393 MB, closed cleanly. The header check alone proves nothing here, because the failure
mode is a crash during startup.

⚠️ **Neither tool can be configured from the repository, which is the reason this change
is documentation and not a `config.toml` edit.** `C:\Program Files\LLVM\bin` is on neither
the user nor the machine PATH, so `-C linker=lld-link` cannot resolve and would need an
absolute Windows path — which, committed, breaks every other Windows checkout and the
Windows CI leg. A repo-level `[build] rustc-wrapper = "sccache"` fails the same way for
macOS contributors and for CI, neither of which has `sccache` installed: the build would
die on a missing binary. Both belong in a machine-scoped `~/.cargo/config.toml`.

⚠️ **Method traps, all paid for here.** Changing the linker through `RUSTFLAGS` or
`.cargo/config.toml` re-fingerprints every target unit, turning a 190 s measurement into a
462 s one — and `RUSTFLAGS` *replaces* `target.*.rustflags` instead of appending, so it
silently drops `/STACK` and produces a binary that dies at startup for reasons unrelated
to the experiment. `cargo rustc` confines a flag to the final unit instead. A timing taken
while another dispatched agent is compiling is not a timing, and this machine routinely
has several at once. And `cargo install sccache` reported success while installing
nothing, because the command was piped to `tail` and the pipeline returned `tail`'s exit
status — the missing binary was the only honest signal.
