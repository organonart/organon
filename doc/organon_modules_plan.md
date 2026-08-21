# Organon modules — extensibility, and how a game gets delivered

> **Status: DRAFT v0.1 (2026-08-15).** A proposal for **how Organon is extended by someone who
> is not editing Organon**, and for the repository topology that follows. It exists because a
> concrete consumer arrived — a 6DOF game — and asking "where does this live" turned out to be
> the more valuable question than the game itself.
>
> **This is a level-1 concern and belongs in this repo**, even though most of what it describes
> ends up outside it. The module *host* is Organon's; the modules are not.
>
> **Reading order.** `doc/arch/topology.md` first — it already asked the repository question
> once, with data, and §2 below is careful about what that answer does and does not cover.
> `LICENSING.md` second, because the licence split turns out to be the thing that makes any of
> this legal. The game's own docs — the reconnaissance, its PRD, its texture-pipeline design and
> its build plan — are **not in this repository**: they are level-3 work and live in the game's
> own (private) repo, which is §1's rule applied to this document's own byproducts. §8 records
> what this plan changed about them.

---

## 1. The three levels of a contribution

Today Organon has exactly one shape of contribution: **a PR to this repo.** Everything that has
ever been built — Mind, the Console, 27 generators — arrived that way.

The proposal is that there are three, and that only the first exists:

| Level | What it is | Where it lives | Example |
|---|---|---|---|
| **1 — Core** | A change to Organon itself | This repo | A new generator; the 6DOF camera arm |
| **2 — Module** | New *capability* built on Organon's public surface, that Organon does not want to own | Its own repo | A game engine; a 6DOF FPS layer |
| **3 — Application** | A *thing made with* a module | Its own repo | Ascent — one game |

📌 **The point of the distinction is what it forecloses.** Without it, every game anyone makes,
and every module anyone writes, grows this repository — and a visualizer whose tree contains
four games is not a visualizer with an ecosystem, it is a monorepo with an identity problem.

🚨 **And there is a sharp, measurable test for which level a change is**, which falls out of
`doc/arch/topology.md`'s own measurement rather than from taste: **does the change need to touch
the param chain?** 96 % of the cross-crate churn in this repo is `params.rs` → `param_table.rs`
→ `to_shared()` → `ipc.rs` → shader — invariant #3, a chain that crosses three crates by
construction. Anything that must touch it is **level 1** and must not leave the repo, because
splitting it out makes every param addition a two-repo dance. Anything that does not is a
candidate for level 2. That test is why a game engine can leave and a generator cannot.

---

## 2. ⚠️ What `doc/arch/topology.md` already decided, and why this is not a reversal

The repository question has been asked here before and answered with a number: **73.6 % of
merges that touch a crate touch more than one**, and §2.4 reads ≳30 % as *"the engine is still
churning too hard to expose a stable API"* → **do not split repositories; one repo, crates
published to crates.io.**

That finding stands, and this proposal does not contradict it, because **it is a measurement of
the engine's internal seams, not of its downstream edge.** The dominant pair is
`organic-math-native × organon-core` and the driver is the param chain. A game engine module
touches none of that: it consumes a compiled API surface and never adds a parameter to the
visualizer.

So the two questions are genuinely different:

- *"Should the engine's own crates live in separate repos?"* — measured, answered **no**.
- *"Should a downstream consumer live in a separate repo?"* — not what was measured.

📌 **And the topology doc's preferred end state is the mechanism this needs anyway**: *"one repo,
crates published to crates.io."* Publication is precisely what turns a workspace member into
something an outside project can depend on. The two plans converge — this one just supplies the
consumer that makes publication worth doing.

⚠️ **One caveat, stated because it is the real cost.** Publishing means the engine crates acquire
semantic versions and a compatibility promise, and the churn number says that promise will be
expensive at first. The mitigation is to publish **narrowly**: `organon-core`, `organon-render`,
`organon-scene`, `organon-world` — the four a module needs — and not to promise anything about
the root crate, which is where the churn actually is.

---

## 3. 🚨 The licence split already makes this legal, and that is not luck

Checked against the manifests rather than assumed:

| Crate | Licence |
|---|---|
| `organon-core`, `organon-render`, `organon-scene`, `organon-world`, `organon-mind`, `organon-console` | **MIT OR Apache-2.0** |
| `organic-math-native` (root: the VST3/CLAP cdylib and its bins) | **GPL-3.0-or-later**, forced by `vst3-sys` under `nih_export_vst3!` |
| `organon-visual` | 🚨 **depends upward on the GPL root crate** — the one member that does |

**Everything a game module needs is on the permissive side of that line.** The camera
finalization is in `organon-world`; the render stack is in `organon-render`; the host-free spine
is `organon-core`. An external module depending only on those is MIT/Apache and carries no GPL
obligation.

`native/Cargo.toml`'s own licence note (line 37) says the siblings must stay permissive because
*"they are the part of the engine an outside project can actually reuse"*; `LICENSING.md` makes
the same argument in its own words. **This proposal is that sentence's first actual use.** The
split was not built for this and it turns out to fit exactly, which is the strongest available
evidence that the seam is in the right place.

⚠️ **The one real trap, and it lands squarely on the game's Tier 1.** `organon-visual` holds the
`organic-math-visual` binary and depends on the GPL root crate for `agent::core_catalog()`. A
module that reuses the visual's host loop **inherits GPL-3.0-or-later**. So a game module must
build its **own** host — winit + wgpu over `organon-world` — rather than borrowing the visual's.
That is a concrete constraint on the "binary + host wiring" workstream, and it is cheap if known
up front and expensive if discovered late.

---

## 4. What a "module" actually is — two kinds, and Organon has neither

⚠️ **First, a naming collision to kill on sight.** In this project **"plugin" means VST3/CLAP** —
Organon *being* a plugin inside a DAW. An extension must never be called a plugin; it is a
**module**. Getting this wrong makes every future sentence ambiguous.

**What exists today** — three extension mechanisms, none of which is a module:

1. **Compile-time `Edition`** (`Full` | `Mind` | `Console`) — cargo features selecting behaviour
   inside one workspace. A fork, not an extension: you cannot add one from outside.
2. **The harness registry** (`organon-console/src/harness.rs`) — 📌 **the closest existing thing,
   and the right precedent.** A harness is *data*: identity, launch command, machine detection,
   where to obtain it; built-ins seeded in code, a user's `harnesses.json` merged over them by
   id with serde defaults and unknown fields tolerated. That is a real runtime extension point
   with forward-compatibility discipline already worked out.
3. **Two-process IPC** — the plugin and the visual are separate processes sharing a memory-mapped
   `Shared` snapshot, namespaced through `ipc::ns_file` so editions coexist.

**There is no dynamic loading anywhere** — no `libloading`, no `dlopen`, no wasm runtime, verified
against every manifest. So "pull a module in at runtime" is a **new capability**, and it is the
largest genuine unknown in this plan.

### The two module kinds worth building

| | **Linked module** | **Hosted module** |
|---|---|---|
| What it is | A Rust crate, a cargo dependency | A separate process, composited |
| Engine access | Full, zero overhead | Through a protocol |
| To adopt | Rebuild | Launch it |
| Language | Rust | Anything |
| Cost | Compile time | ~a texture copy per frame |
| Precedent here | none | **the visual, and the harness registry** |

🚨 **Rust has no stable ABI, so the tempting third option — `dlopen` a Rust cdylib — is a trap**:
it requires a hand-maintained C-ABI boundary or an `abi_stable`-style contract, and it fails at
*runtime*, in a graphics driver, on a user's machine. **Recommendation: do not build it.** The
two kinds above cover the same ground with mechanisms this project has already proven.

📌 **And the hosted-module design is unusually cheap here, because Organon already pays its
cost.** The visualizer *already* renders in a separate process and composites the result; the
Console's portal *already* shows that surface inside a pane and grows it to full screen. A hosted
module is that same shape with a different producer. The frame copy is not a new tax — it is the
tax this architecture has been paying since before any of this was proposed.

---

## 5. The end-state topology

```
  organonart/organon                        ← THIS REPO. Level 1.
    the engine, the four publishable crates,
    the module host protocol, the Console
                  │
                  │  crates.io (linked)  +  module manifest (hosted)
                  ▼
  organonart/organon-game-engine            ← Level 2. One repo, two crates.
    ├── organon-mod-engine   the game-engine module:
    │                        sim tick, entities, collision, assets,
    │                        positional audio, input actions
    └── organon-mod-6dof     the 6DOF FPS module:
                             the rig, flight model, cockpit,
                             weapons — depends on mod-engine
                  │
                  │  crates.io
                  ▼
  organonart/ascent                         ← Level 3. The game.
    Ascent — level formats, the texture
    pipeline, mission flow, art direction
```

**Why the two modules share one repo rather than getting three.** They are a stack with a single
consumer and they will churn together — which is `doc/arch/topology.md`'s own criterion, applied
honestly to a smaller graph rather than assumed away. If `organon-mod-6dof` ever gains siblings
(a 3rd-person module, a vehicle module) the repo already holds them; that is the shape it is
named for.

**Why Ascent is separate.** It is the level-3 proof that level 2 is real. A game engine whose
only game lives in its own repository has demonstrated nothing about being reusable.

⚠️ **Everything Descent-specific belongs in `ascent`, not in either module** — the `.RDL`/HOG/PIG
parsers, the style-guide texture pipeline, the mine-graph semantics. Each is a strong candidate
for *later promotion* down into `mod-engine` once a second consumer proves it general, and that
promotion is itself a level-2 contribution. Promoting on the first consumer is how a game engine
acquires one game's assumptions as its architecture.

---

## 6. Getting there — start merged, split on a trigger

**Recommendation: build it all in `organonart/ascent` first**, as one repo with the module
boundaries drawn as *crate* boundaries from day one, and split when a trigger fires rather than
on a date.

The reasoning is the same measurement that governs everything else here: early on, a change to
the 6DOF rig and a change to the game *will* land together, and paying a two-repo dance for that
is exactly the cost §2 warns about. **Crate boundaries give the discipline; repository boundaries
give the cost.** Take the first now and the second later.

**The split triggers, any one of which is sufficient:**

1. **A second consumer appears** for either module — the moment the strongest argument for
   separation stops being hypothetical.
2. **Cross-crate churn between `mod-*` and the game drops below ~30 %** — the same band
   `doc/arch/topology.md` §2.4 uses, measured with the same script.
3. **Someone outside wants the module without the game.** Ecosystem demand beats internal
   metrics.

📌 **What must be true from day one, though, is the licence discipline** — no crate in the module
stack may depend on `organic-math-native` or `organon-visual` (§3). That one is not deferrable,
because unwinding a GPL dependency later means rewriting whatever leaned on it.

---

## 7. What Organon owes, as level-1 work

This is the part that belongs in **this** repo, and it is the actual extensibility contribution.
It is small, which is the good news.

| # | Deliverable | Why |
|---|---|---|
| **M0** | 🚨 **Make `organon-core` packageable at all.** `math.rs` carries **7** `include_str!("../../assets/…")` sites reaching *out of* the package root — 3 network JSONs, 4 creature JSONs — and `cargo package` bundles only files under that root, so a published crate would fail to build for everyone who depended on it | **The real gate.** `organon-core/Cargo.toml` already flags this in a 🚨 block and defers it; the decision it names is genuine (vendor a copy under the package, a build script, or split the runtime gallery from the compiled-in data) and is complicated by `deploy.sh` installing `assets/networks/*.json` as the runtime gallery |
| **M1** | **Publish the four crates to crates.io** — `organon-core`, `organon-render`, `organon-scene`, `organon-world`. Semantic versions and a stated compatibility policy | Without it there is no linked-module story at all. Already `topology.md`'s preferred end state. ⚠️ **Not a flag flip:** no crate sets `publish = false`, so there is nothing to remove — the work is M0 plus versioning, and the other three need their own `cargo package` dry-run before anyone assumes they are clean |
| **M2** | **A public-API review of `organon-world`** — what a module may depend on, what is incidental. The 6DOF camera arm is the first thing to promote deliberately rather than by accident | An API nobody has ever consumed from outside is a guess |
| **M3** | **The hosted-module protocol** — a module *manifest* on the harness-registry pattern (identity, launch, detection, where to get it) plus surface compositing through `ns_file`-namespaced IPC | Turns "pull it in from the Console" from an aspiration into a mechanism, reusing two things already built |
| **M4** | **A Console verb to add and run a module** | The user-facing half of M3 |
| **M5** | **`CONTRIBUTING.md` gains the three levels** (§1) and the param-chain placement test | Otherwise the distinction lives only in this document and decays |

⚠️ **M0 → M1 → M2 is the blocking chain; M3–M5 are not.** A linked module works the moment the
crates are published, and the crates cannot be published until M0 is answered. The hosted path is
what makes it *feel* like an ecosystem, and it can land later without changing anything built
against M1.

🚨 **M0 was missing from the first draft of this table, and the way it was missing is the point.**
§3 of this document practises the opposite — *checked against the manifests rather than assumed* —
and the rule this same change wrote into `LICENSING.md` is to ask the manifests rather than a
hand-written table. M1 was written without asking the manifest of the very crate it names first,
which states the blocker in a 🚨 block. A plan that
prices its own critical path off remembered structure repeats the exact failure the rest of this
document is about. 📌 Until M0 lands, **level 2 is reachable only by path or git dependency**,
which works for a repo we control and is not an ecosystem.

---

## 8. Consequences for the work already planned

Four documents written before this one — the reconnaissance, the game's PRD, its texture-pipeline
design and its build plan — assumed the game lands in this repo as a fourth edition. That
assumption is now wrong. They **have since moved to the game's own private repo**, which is why
they are not in this tree; the corrections recorded here travelled with them:

- **`Edition::Deep` and `deep-edition` should not exist.** A fourth compile-time edition was the
  right answer for a product *inside* this workspace and is the wrong answer for a module outside
  it. Ascent is a binary that depends on crates; it is not a build configuration of Organon.
  🚨 This deletes Tier 0's main deliverable and replaces it with M1 + M2.
- **The name question changes shape.** `Deep` was chosen to avoid colliding with `mind-edition`
  as a cargo feature — a constraint that evaporates once there is no feature. **Ascent** is the
  working name for the game, and it is better: it carries the heritage, inverts it, and is ours.
  ⚠️ Worth a trademark search before it is load-bearing; it is not an unused word.
- **The tier spine survives almost intact.** Tiers 1–6 describe *what gets built*, and moving
  repositories does not change the order — but Tier 1's "binary + host wiring" now must build its
  own winit/wgpu host rather than reusing the visual's (§3).
- **The docs followed the code, and this was the first test of §1.** They were briefly committed
  here and then moved out. 📌 There is a precedent for exactly that relocation:
  `doc/arch/topology.md` records that the Console's product docs live in the **private annex**
  while only its architecture doc crosses into the public tree. Where a doc lives is already
  treated here as a decision rather than a default. ⚠️ **And the mechanics of undoing it are worth
  recording**, because a level-2 contributor will hit this: removing a file in a follow-up commit
  does **not** unpublish it — the content stays in the branch's history and is fetchable until the
  branch itself is rewritten. The correct move is to rebuild the branch from `main` with the file
  never present, which is what was done here.
- **This document stays.** It is level-1.

---

## 9. Open questions

1. **Ratify the three levels** (§1) and the param-chain placement test. Everything else follows.
2. **`ascent` first, or the split repos immediately?** Recommendation: merged, with crate
   boundaries and split triggers (§6).
3. **Trademark check on "Ascent."**
4. **Does M1 (crates.io publication) happen now or after the game proves the API?** ⚠️ These
   conflict: publishing early means versioning an API no one has stressed; publishing late means
   the game vendors a path dependency and the module story stays theoretical. **Recommendation:
   publish `0.x` early and say plainly that 0.x means no compatibility promise** — which is what
   0.x means, and what the churn number honestly supports.
5. **Who owns the module registry** once there is more than one module — a file, a repo, an
   index? Not urgent, and cheap to defer until M3.

---

## 10. ✏️ Amendment — trust is the axis this plan was missing, and it is the same decision as §4

> Added 2026-08-20. §9.5 defers *"who owns the module registry"* as not urgent. That deferral
> is still right, but it hid a question that is **not** about a registry and is not deferrable
> in the same way, because it decides §4's answer rather than following from it.
>
> James, 2026-08-20: *"I can imagine a system of modules and a trust system whereby you have
> your core modules, but then you might have modules that you trust because you're working with
> someone … peer-to-peer connections, starting with those you trust, such as your family first,
> and then your friends, and then your people that you are friendly with in the workforce …
> and then open source contributors."*

### 🚨 The process boundary is the trust boundary

§4 settles that there are exactly two module kinds and rules out the tempting third
(`dlopen`ing a Rust cdylib — no stable ABI, fails at runtime in a graphics driver on someone
else's machine). What §4 presents as an engineering trade-off is also, and more importantly, a
**security** one:

| | **Linked module** | **Hosted module** |
|---|---|---|
| §4's framing | a cargo dependency, full engine access, zero overhead, rebuild to adopt | a separate process, composited, a protocol, ~a texture copy per frame |
| **What that means for trust** | **no boundary exists.** It is your address space, your filesystem, your GPU. Auditing the source is the *only* control | **the process is the boundary.** What it may touch is what the protocol exposes, and that is enforceable rather than promised |

📌 **So "how far do I trust this?" and "which kind of module is this?" are one question asked
twice.** A trust tier does not select a *policy* applied to a module; it selects the module's
**kind**. That is why this belongs in §4 rather than in a registry design.

### The tiers, and where the line falls

- **Core** — in this repo, in this workspace. Linked by definition.
- **Family-level trust** — someone whose code you would run without reading. Linked is
  *defensible* here, and the honest framing is that you are trusting a person, not a mechanism.
- **Everything further out** — friends, colleagues, open-source contributors — **hosted**. Not
  because those people are suspect, but because a boundary you can point at is the only thing
  that survives one of them having a bad day, a compromised account, or a dependency that did.

⚠️ **The failure mode to design against is social, not technical.** Trust tiers make it *easy*
to promote a module as a favour. A system where "I know them" is spelled the same as "grant
full address-space access" will drift upward until the tiers mean nothing. Whatever the
mechanism, **promoting to linked should be visibly different from installing**, and it should
say what it is granting.

### What this repo already has, and what it does not

**Has**, and both are load-bearing precedents §4 already names:

- **The harness registry** (`organon-console/src/harness.rs`) — identity, launch command,
  detection, where to obtain it; built-ins in code, a user's file merged over them by id, with
  serde defaults and unknown fields tolerated. A module manifest is that shape (M3 says so).
- **`ipc.rs::ns_file`** — every channel namespaced, which is what already lets three products
  run at once and is the same mechanism a hosted module's surface would arrive through.

**Does not have**, and none of it should be built before #98's tiers land:

- Any notion of a module *identity* that survives a machine — a key, a signature, an author.
- Any statement of what a hosted module may ask for. ⚠️ **The protocol is the permission set.**
  A hosted module can do exactly what the protocol lets it do, so the protocol's surface *is*
  the security model, and every verb added to it is a grant. That is worth writing down before
  the first verb, not after the tenth.
- Any answer to revocation — what happens to a layout that references a module you have since
  stopped trusting. ⚠️ It must not be "the window fails to open."

### Where the peer-to-peer half already lives

**#6 (`organon-remote` — one session, many clients)** was filed as *"the console on a phone"*
and is really the collaboration primitive: a session with an authority and attached clients.
Trust tiers and P2P module distribution both land on it rather than beside it, and it should
be read as that rather than as a phone feature when either is scoped.

### Consequences for §9's open questions

- **§9.5 gains a prerequisite.** Whoever owns the registry has to say what an entry *asserts*
  about its module. An index of names is not a trust model, and a registry that implies one
  without having one is worse than no registry.
- **§9.4 is unaffected.** Publishing `0.x` early is orthogonal to trust: crates.io already has
  its own identity and revocation story, and a linked module is a cargo dependency whether or
  not we publish.

### 🚨 Not now

Nothing here is scheduled, and the ordering is deliberate. Modules want at least one real
second producer to exist before the protocol is designed against imagination — §4 makes that
point about the game and it applies twice as hard to a permission surface. **Build #98's tiers,
get a layout worth sharing, and design this against something real.**
