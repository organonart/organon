### Organon modules — the three levels of a contribution

`doc/organon_modules_plan.md` proposes how Organon is extended by someone who is **not** editing
Organon, and the repository topology that follows. It is a level-1 concern and stays in this repo
even though most of what it describes ends up outside it: the module *host* is Organon's, the
modules are not.

📌 **Today there is exactly one shape of contribution — a PR to this repo — and the proposal is
that there are three.** Level 1 is core (a generator, a camera arm). Level 2 is a **module**: new
capability on Organon's public surface that Organon does not want to own — a game engine, a 6DOF
FPS layer. Level 3 is an **application** made with a module. Without the distinction, every game
and every module anyone writes grows this tree, and a visualizer whose repository contains four
games is a monorepo with an identity problem.

🚨 **The placement test falls out of a measurement this repo already took, rather than out of
taste: does the change need to touch the param chain?** `doc/arch/topology.md` records that 96 %
of the dominant cross-crate churn is `params.rs` → `param_table.rs` → `to_shared()` → `ipc.rs` →
shader — invariant #3, a chain that crosses three crates by construction. Anything that must touch
it is level 1 and cannot leave, because splitting it makes every param addition a two-repo dance.
Anything that need not is a level-2 candidate. That is why a game engine can leave and a generator
cannot.

⚠️ **This is not a reversal of the topology doc's "do not split repositories".** That finding
measured the engine's *internal* seams; a downstream consumer was not what was measured, and it
touches none of the param chain. The two plans converge instead: that doc's preferred end state is
already "one repo, crates published to crates.io," and publication is exactly what turns a
workspace member into something an outside project can depend on. This proposal supplies the
consumer that makes publishing worth doing.

🚨 **The licence split already makes an external module legal, and that is not luck.** Checked
against the manifests: `organon-core`, `organon-render`, `organon-scene` and `organon-world` —
everything a game module needs, the camera finalization included — are MIT OR Apache-2.0, while
GPL-3.0-or-later is confined to the root crate that exports the VST3. `native/Cargo.toml`'s own
licence note says the siblings must stay permissive because "they are the part of the engine an
outside project can actually reuse"; this is that sentence's first real use, and the seam fitting a
case it was not designed for is the strongest available evidence it sits in the right place.
⚠️ One trap: `organon-visual` depends *upward* on the GPL root crate, so a module reusing the
visual's host loop inherits GPL — it must build its own winit/wgpu host over `organon-world`.

⚠️ **"Plugin" is already taken and must not be reused.** In this project a plugin is Organon
*being* a VST3/CLAP inside a DAW. An extension is a **module**, always.

📌 **There is no dynamic loading anywhere in the tree** — no `libloading`, no `dlopen`, no wasm
runtime, verified against every manifest — so "pull a module in at runtime" is new capability and
the plan's largest genuine unknown. Two kinds are proposed and the tempting third is refused:
**linked modules** (a cargo dependency; full engine access, requires a rebuild) and **hosted
modules** (a separate process, composited; adopted by launching it). `dlopen`-ing a Rust cdylib is
rejected outright — Rust has no stable ABI, so it needs a hand-maintained C boundary and it fails
at runtime, in a driver, on someone else's machine. 🚨 The hosted design is unusually cheap here
because **Organon already pays its cost**: the visualizer renders in a separate process and the
Console's portal already composites that surface and grows it to full screen. The frame copy is not
a new tax. And the harness registry is the precedent for the manifest — a harness is already pure
data with detection and forward-compat discipline worked out.

### Licensing — three stale rows, and one omission that mattered

Chasing the plan's own citation turned up that `LICENSING.md`'s crate table had fallen **three**
crates behind: `organon-scene`, `organon-agent` and `organon-world` are all `MIT OR Apache-2.0` and
none was listed. It also omitted `organon-visual` entirely, which matters more than an omission
usually would — that crate is **GPL-3.0-or-later by inheritance through an upward dependency**
rather than by its own licence choice, and a table that lists the permissive crates while saying
nothing about it invites exactly the mistake of reusing the visual's host loop from a module
believed to be permissive. The row now says so.

📌 **One rung further down the same ladder.** `native/Cargo.toml`'s licence comment — the text the
plan cites as authority — carried two stale claims of its own: it said "the four sibling crates"
(seven) and it still listed the visual binary among this crate's own `[[bin]]` blocks, which
stopped being true when `organon-visual` was extracted. Both fixed. ⚠️ The count is now **absent
rather than corrected** — a number in prose is what went stale twice — so the comment points at the
manifests instead, the same rule `doc/arch/topology.md` already applies to the `members` list six
lines above it. Comments only; no `license` field, dependency or member was touched.

📌 **And the plan's own critical path was wrong on first draft, in the way it warns about.** M1
said publishing the four crates needed `publish = false` removed — no crate sets it, so there was
nothing to remove — and it missed a fatal blocker that `organon-core/Cargo.toml` already states in
a 🚨 block: `math.rs` has **7** `include_str!("../../assets/…")` sites reaching *out of* the
package root (3 network JSONs, 4 creature JSONs), and `cargo package` bundles only what is under
that root, so a published crate would fail to build for everyone depending on it. That is now
**M0**, ahead of M1, with the genuine decision it forces named rather than deferred — vendor a copy,
a build script, or split the runtime gallery from the compiled-in data, complicated by `deploy.sh`
installing those same network JSONs as the #226 gallery. 🚨 The failure is the one this document is
otherwise about: §3 says *ask the manifests, not the prose*, and M1 was written without asking the
manifest of the crate it names first. Until M0 lands, level 2 is reachable only by path or git
dependency — which works for a repo we control, and is not an ecosystem.
