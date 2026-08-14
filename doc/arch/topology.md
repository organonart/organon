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

## The fifth member — `organon-shell` (Shell #3 T1, 2026-08-07)

**Organon Shell** joined the workspace as its own crate (the organon-mind pattern:
a nih_plug-free lib, the `required-features`-gated bin in the root crate beside
`mind_main.rs`, edition feature forwarded to `organon-core` where `EDITION`
resolves). Three topology facts, so the tables above age predictably:

- **Exported like every other member.** `mirror-platform.manifest` carries
  `INCLUDE native/organon-shell`, so the crate crosses byte-identical and the root
  manifest's forwarding feature `shell-edition = ["organon-core/shell-edition"]`
  stays valid on the far side, because organon-core crosses too and carries the
  target feature. A new member is one manifest decision, not two — see §"the export"
  in `CLAUDE.md`.
- **Publishable in principle** (organon-core + egui only — the window stack lives
  with the root-crate bin), and no longer blocked by anything structural.
- **Its product docs stay in the private annex** — `doc/organon_shell_prd.md` and
  `doc/organon_shell_buildplan.md`, which `EXCLUDE doc` keeps out. Only the code and
  `SHELL_ARCHITECTURE.md` cross into the public tree, so a public reader sees what
  Shell *is* and what exists today, but not the tactical sequencing.
