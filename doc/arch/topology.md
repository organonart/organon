# Organon — topology, churn, and publication (#626 §2)

> **What this is.** The repository-topology question #626 §2 raises, the measurement §2.4
> defers it to, and what that measurement turned out to say. Split out of the root
> `ARCHITECTURE.md` under the #590 Tier 3 pattern: the injected core hit **100% of its
> 200 KB budget** the moment this landed there, and CLAUDE.md's rule for that is to move a
> subsystem section here rather than let the budget drift past silently.
>
> **This file is NOT auto-injected. Open it deliberately** when the question is *"should
> this be one repo or several"*, *"can we publish to crates.io yet"*, or *"what did the
> churn number say"*. `ARCHITECTURE.md` §19.0.1 carries the headline and points here.

---


**74% of merges that touch a crate touch more than one.** 73.6% (203/276) over the last
400 first-parent merges, measured at `7e19bc8d` by `native/tools/crate-churn.py`.

⚠️ **The window slides with every merge**, so this number moves on its own — it went
73.9% → 73.6% between being written and the review round, purely because one commit
landed. That is why the measuring commit is pinned: a later reading that differs is drift,
not disagreement. Re-run the script; do not reason from this line.

```
CROSS-CRATE CHURN: 73.6%  (203/276)     §2.4 band: ≳30%   [at 7e19bc8d]
  179  organic-math-native × organon-core
  102  organic-math-native × organon-render
   88  organon-core        × organon-render
```

#626 §2.4 defers the whole repository-split decision to this number and reads ≳30% as
*"the engine is still churning too hard to expose a stable API."* **On that reading the
answer is: do not split repositories.** One repo, crates published to crates.io — §2.5's
own preferred end state — and the number is the argument for it rather than against.

> 📌 **organon#49 reached the same answer from the other end, and added a sixth crate on
> the way** (`organon-scene`, Tier 3). #49 asks how to distribute **Organon Console**
> without making a user meet the visualizer, and the finding was that Console is a
> GPL-3.0-or-later binary of the VST3 crate — `shell_main.rs` lives in
> `organic-math-native`, so it links nih-plug and inherits the licence from a plugin
> binding it never calls. That is a **crate-graph** problem, not a repository one, and the
> route is the same one this file argues for: move the seam, keep the repo. The two
> analyses are independent and agree, which is worth more than either alone.
>
> ⚠️ **The churn number above predates that work and will move because of it.** Splitting
> modules out of `organic-math-native` mechanically *raises* cross-crate churn for a while
> — a change that used to be one crate's is now two. Re-run
> `native/tools/crate-churn.py` before quoting it; a higher reading after #49 is the
> extraction showing up in the data, not the engine destabilising.

**The important half is WHY, because it means the number will not decay on its own.**
The dominant pair is `organic-math-native × organon-core`, and **96% of those 179 merges
touch a param-chain file**. That is invariant #3 — *a param is a chain, not a line* —
`params.rs` → `param_table.rs` → `to_shared()` → `ipc.rs` → the visual's
`build_uniforms`/shader. Those live in **three different crates now**: `params.rs` in
`organic-math-native`, `ipc.rs` in `organon-core`, the shader in `organon-render`.

So the most common kind of change in this codebase crosses the crate boundaries **by
construction**. Waiting will not settle it; only re-cutting the boundaries along the param
chain would, and nothing proposes that. **Split the repos and every param addition becomes
a three-repo dance** — which is exactly the failure mode §2.3 predicted from the churn
data, arriving through a seam it did not name.

⚠️ **Two ways to get a wrong answer from this script**, both of which produce a
plausible-looking number rather than an error, and both now guarded:

1. **Run it from a tree that predates the crates** and the rename map is empty, every path
   classifies as the root crate, and it reports a serene **0.0%**. Reproduced at `HEAD~30`
   before the guard existed.
2. **Use a commit range rather than `-n`.** `HEAD~400..HEAD` spans **1,599** commits here,
   not 400 — with merges, a range is everything reachable from one end and not the other.

It measures **first-parent merges** by default because §2.4's bands price a *two-PR
dance*, which is paid per unit of work, not per intermediate commit on a branch.
`--all-commits` answers the narrower question and gives 18%.


---

## The crate graph — who may depend on what

**Read from the manifests, not from memory** (`cargo tree -p <crate> --depth 1 -e normal`).
⚠️ **Do not trust a count in this prose either — ask `native/Cargo.toml`'s `members`.** This
paragraph said "five workspace members" through two crates being added, because a hand-written
tally is a second copy of a fact the manifest already states. Six members plus the root package
as of organon#49 Tier 4b:

```
organic-math-native  (root: plugin cdylib + its [[bin]] blocks, GPL-3.0-or-later)
  ├── organon-core ────────── the host-free spine
  ├── organon-render ──┐
  ├── organon-scene ───┤
  ├── organon-agent ───┼───── all depend on organon-core; organon-world also
  ├── organon-mind ────┤      depends on organon-mind, and on render/scene/agent
  ├── organon-world ───┤      when its `world` feature is on
  └── organon-console ─┘

organon-visual       (the one arrow that points BACK UP — organon#49 T4c-ii)
  ├── organon-world  { features = ["world"] }
  ├── organon-core
  └── organic-math-native ◄── for `agent::core_catalog()`, which reads `param_table`

organon-module       (the hosted-module contract; depends on NO member, BY RULE)
                       └── memmap2          — see the table below

organon-glyphs       (the glyph-ring producer — organon#217 T1; a leaf, like the
  ├── organon-core     mind-writer, that fills a ring the visual reads)
  ├── ttfx           ◄── git dependency pinned by rev; MIT; NOT on crates.io
  └── clap             (forced — see the table row)

xtask                          (build tooling; depends on no member)
```

⚠️ **`organon-module` depending on no member is a rule rather than an accident**, and it is the
only place in this graph where "keep the edges few" is enforced from outside this repository.
Every other leaf may grow an edge to `organon-core` the day it needs one; this one may not,
because a second repository's licence CI reads the same graph. See the table's row and
`CONSOLE_ARCHITECTURE.md` §1.20.

| Crate | Its own direct deps | The rule it is held to |
|---|---|---|
| `organon-core` | `bytemuck`, `glam`, `half`, `memmap2`, `serde`, `serde_json` | **no `nih_plug`, no `wgpu`, no `egui`** — `cargo tree -p organon-core` is the acceptance test |
| `organon-render` | `organon-core`, `bytemuck`, `glam`, `half`, `image`, `wgpu` | no `nih_plug`, no `egui`, no `winit` |
| `organon-scene` | `organon-core`, `bytemuck`, `glam` | no `nih_plug`, no `wgpu`, no `egui` — the substrate's own arithmetic, host-free like core |
| `organon-mind` | `organon-core`, `bytemuck`, `dirs`, `egui`, `memmap2` | no `nih_plug` |
| `organon-world` | `organon-core`, `organon-mind`, `bytemuck`, `egui`; **+ `organon-render`, `organon-scene`, `organon-agent`, `wgpu`, `winit`, … behind the `world` feature** | no `nih_plug` — the window layer *and*, since organon#49 T4c-ii, the world. ⚠️ Check the bar **with the feature on** (`cargo tree -p organon-world --features world`), or it says nothing about the 13.5k lines that matter |
| `organon-visual` | `organon-world` (`world`), `organon-core`, **`organic-math-native`**, `wgpu`, `winit`, `pollster` | 🚨 **the one member that depends UPWARD on the plugin crate**, and the only one exempt from the no-`nih_plug` bar. It holds `[[bin]] organic-math-visual`, which needs `agent::core_catalog()` (reads `param_table`, cannot descend). It exists so that need does not force the `world` feature onto the package that also builds the VST3 |
| `organon-module` | **`memmap2`, and nothing else** (`wgpu` behind an optional feature; `pollster` as a dev-dependency for one ignored GPU test) | 🚨 **the only crate here whose licence bar is another repository's.** `organonart/ascent` depends on it in order to *be* a hosted module, and Ascent's invariant 3 forbids an edge to `organon-visual` or `organic-math-native` — so `cargo tree -p organon-module` is the acceptance test **in both trees**, and a transitive arrow from here would break a second repo's posture rather than ours. Also no `serde` (the wire crosses two separately built binaries, so a derived encoding makes stability a property of a dependency version they cannot agree on) and no `organon-core` (`ipc::ns_file` is still the right way to *name* the channel, but resolving a `PathBuf` is one line at the console's call site and would otherwise drag `glam`, `half`, `bytemuck`, `serde` and `serde_json` into a game's build) |
| `organon-console` | `organon-core`, `alacritty_terminal`, `dirs`, `egui`, `portable-pty`, `serde`, `serde_json` | **no `nih_plug`, ever** — standalone-only permanently, so any `nih_plug` in this graph is a loaded gun pointed at Organon's VST3 class ID. 📌 **Unchanged by the exhibit (`CONSOLE_ARCHITECTURE.md` §1.13), which is the point**: showing a picture needs a decoder and showing Markdown needs a parser, and this crate gained *neither*. `image` is in the **root crate**, where `console_main` decodes off-thread and hands back a `TextureId` — the `SurfaceRequest`/`OrganonDraw` pattern, where the lib declares intent and the binary that owns the wgpu device does the work. The Markdown renderer is ~40 lines of line-matching rather than a crate, because a parser, an AST and an HTML model to make four kinds of line look different is not a trade this manifest makes |
| `organon-glyphs` | `organon-core`, **`ttfx`** (git, `rev = "7203e35"`), `clap` | **organon#217 T1 — the glyph-ring producer.** A leaf that grows exactly one edge to `organon-core` (for `glyph_ring` + `ipc::glyph_ring_path`) — this file's own rule — and links `ttfx`, which is **MIT and not on crates.io**, so it is a git dependency pinned by rev the way `nih_plug` and `baseview` are. Its own package **so ttfx touches no existing crate**: not the GPL root, not `organon-render`, not the Console. ⚠️ **`clap` is the third edge §6.1 said would not exist, and it is forced, not chosen**: ttfx's effect configs are clap `Args` whose defaults live only in `#[arg(default_value…)]` attributes, none implements `Default`, and ttfx does not re-export clap, so `EffectCommand` cannot be built by name without naming the crate. It is already in the workspace graph (the root crate's CLI), so the resolved crate set is unchanged; only this depth-1 list grew. ⚠️ **ttfx pins `clap_complete = "4.6.9"`**, and adding the member forced the lockfile's 4.6.7 up — a lockfile-only bump the root crate's `^4` still satisfies, but the first thing the next ttfx rev bump will hit again. **No nih-plug, no wgpu, no egui**; `cargo tree -p organon-glyphs -e normal --depth 1` is the acceptance test |

⚠️ **The leaf crates are siblings, not a stack — with exactly one edge between them.**
`organon-world` depends on `organon-mind` (`egui_platform` reaches `mind_shell::PointerTarget`),
and that direction is the one Mind's own manifest records: consumers depend on Mind, never the
reverse. Every *other* edge between leaves would have to go through the root crate, which is why
the console's binary lives in the root package (it renders `World`) while its compositor lib does
not — that would drag `nih_plug` into a package forbidden to have it.

⚠️ **`nih_plug` and the whole window stack — `winit`, `wgpu`'s surface, the vendored
`egui-wgpu` — belong to the ROOT crate alone.** That is what keeps the four members
`MIT OR Apache-2.0` while the root is GPL: see `LICENSING.md`, and note that "make the
licences consistent" would relicense the reusable engine onto a plugin binding's terms.

---

## The fifth member — `organon-console` (Console #3 T1, 2026-08-07)

**Organon Console** joined the workspace as its own crate (the organon-mind pattern:
a nih_plug-free lib, the `required-features`-gated bin in the root crate beside
`mind_main.rs`, edition feature forwarded to `organon-core` where `EDITION`
resolves). Three topology facts, so the tables above age predictably:

- **Exported like every other member.** `mirror-platform.manifest` carries
  `INCLUDE native/organon-console`, so the crate crosses byte-identical and the root
  manifest's forwarding feature `console-edition = ["organon-core/console-edition"]`
  stays valid on the far side, because organon-core crosses too and carries the
  target feature. A new member is one manifest decision, not two — see §"the export"
  in `CLAUDE.md`. ⚠️ That manifest lives in the **private** tree; `scripts/` does not
  exist in this repo, so the line above cannot be checked from here.
- **Publishable in principle**, and no longer blocked by anything structural — but
  ⚠️ **not on the two-dependency footing this section used to claim.** It said
  "organon-core + egui only"; the crate has since taken `serde`/`serde_json` (the #4
  session schema *is* serde types), `dirs` (the one store-path resolver) and, at
  Console #10 T1, `portable-pty` + `alacritty_terminal` — the real PTY and VT state
  machine. Seven direct dependencies, all on crates.io, none of them a host or a
  window: the *publishability* claim survives, the *smallness* claim did not, and
  those are different claims.
- **Its product docs stay in the private annex** — `doc/organon_shell_prd.md` and
  `doc/organon_shell_buildplan.md`, which `EXCLUDE doc` keeps out. ⚠️ Those two
  filenames keep the old product name because that is what the files are actually
  called; renaming the citation without renaming the annex would only dangle it. Only
  the code and `CONSOLE_ARCHITECTURE.md` cross into the public tree, so a public reader
  sees what the Console *is* and what exists today, but not the tactical sequencing.

### What the crate has grown since (2026-08-13)

`native/organon-console/src` is **29 modules**, not the Tier-1 handful this section was
written against. The ones that arrived after it and that a topology reader will look for:

- **`theme.rs`** — one `Theme` of `Color32` fields; every colour the console paints.
- **`posture.rs`** — how the console holds itself, the second axis beside the theme.
- **`prefs.rs`** — `preferences.json` beside `harnesses.json` in the store root, so a
  choice can outlive a launch instead of being an `ORGANON_SHELL_*` variable read once.
- **`kind.rs` is NOT here — it is in `organon-core`**, and that placement is a topology
  fact rather than a filing preference: the three copies of the kind vocabulary lived in
  *different crates* (`cli.rs` in the root, `block_panel.rs` and `conversation.rs` in the
  console), so the wire copy could not import the paint ones. `organon-core` is the only
  crate all three can see, and a closed set of words needs no host, GPU or UI.

`CONSOLE_ARCHITECTURE.md` owns what each of them does; this file owns only where they sit.
