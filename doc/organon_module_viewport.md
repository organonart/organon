# A hosted module in a viewport — what "approve a repo" means, and the contract it buys

> **Status: PART BUILT.** T0 (all three frame-boundary numbers), T3 (`modules.json`,
> `organon-module.toml`, and the verbs), T2 (the contract crate) and T5 (the launcher, the
> texture, the four failure sentences) have landed; §9 is the order and carries what each rung's
> state is. ⚠️ §4.7 is CONTRACT rather than design — two strings two repositories must spell
> identically. Everything else here is still design. It answers four questions in writing so
> Organon's side and Ascent's side can be refactored against the same contract instead of against
> each other.
>
> ⚠️ **Where a section has been built, its living state is `CONSOLE_ARCHITECTURE.md`, not here.**
> This document keeps the *argument* — why hosted, why a commit, why the diff is the valuable
> verb — and says so at the top of each built section rather than being rewritten into a
> description of the code.
>
> James, 2026-08-21:
> *"I want to be able to open up Organon, point to the Ascent repo, and approve it as a module.
> Then the experience we currently get when we run the executable is available as a new viewport
> type — viewport type `ascent` — and this goes along with our new viewport configuration system.
> When it is first in the viewport it is in a paused start state with no sound. Clicking into it
> lets you interact with it still in the viewport. Then it must be very simple to scale it up
> first to fill the whole console border, then again to full screen, and back down, with some
> easy affordance."*
>
> **Reading order.** `doc/organon_modules_plan.md` §4, §10 and §11 first — they decide *what a
> module is*, *where the trust boundary falls* and *what the unit of trust is*, and this document
> assumes all three rather than re-deriving them. Then `CONSOLE_ARCHITECTURE.md` §1.14's producer
> seam, which is the one sentence this whole design lands on. `doc/organon_is_the_product.md` §2
> supplies §2.2's argument.

---

## 1. This is one question, not three

Three pieces of work were in flight separately when this arrived, and they are three faces of the
same thing: **Organon composing something it did not itself draw.**

| | What it is | The face it shows |
|---|---|---|
| **#124** | the panel table — Organon's own controls, in a region | a producer of *controls* Organon owns |
| **#123** | a tab is an instance with a layout | a producer of *conversation* Organon spawns |
| **this** | a module in a viewport | a producer of *pictures* Organon neither owns nor wrote |

📌 **Scope them against each other.** The failure available here is three parallel notions of
"a thing in a rectangle", each with its own registry, its own vocabulary and its own lifetime.
The console has already refused that shape twice — one `SceneMode` for two presentations of a
viewport, one command table for four front doors — and both refusals are why this design is
small.

---

## 2. The decision: **hosted**, and the licence is the weakest of the three arguments

`doc/organon_modules_plan.md` §4 settles that there are exactly two kinds — **linked** (a cargo
dependency, full engine access, rebuild to adopt) and **hosted** (a separate process, composited,
a protocol) — and rules out `dlopen`ing a Rust cdylib because Rust has no stable ABI and the
failure lands at runtime in a graphics driver on someone else's machine. That ruling stands and
nothing here reopens it.

Between the two, **hosted**. Three arguments, weakest last.

### 2.1 🚨 The linked arrow points the wrong way, and the modules plan never contemplated this direction

§4's *linked module* is a crate that **depends on Organon**: `organon-core`, `organon-render`,
`organon-scene`, `organon-world`, consumed by something downstream that produces its own binary.
Ascent is exactly that today, pinned by revision, and its own `CLAUDE.md` invariant 4 says so —
*"Organon is consumed, never forked or vendored."*

For Organon to host a linked module **inside its own process**, the dependency must run the other
way: `organic-math-native` would gain a dependency on `ascent`. That arrow has never existed and
the plan's §2 measurement does not cover it — 73.6 % cross-crate churn is a statement about the
engine's internal seams, and §2 is careful that it says nothing about the downstream edge. This
direction is not the downstream edge either. It is a *new* edge, from the public root crate into
a private application repo.

⚠️ **And it breaks the build for everyone who is not James.** `organonart/organon` is public;
`organonart/ascent` is private. A git dependency on a private repo in a public manifest is a
checkout that cannot resolve. That is a distribution defect before it is a licence one, and it
has no mitigation short of "the module list is empty in the public tree" — which is a mitigation
for the manifest, not for the design.

### 2.2 🚨 A linked module is the compile-time `Edition` wearing a different hat

This is the argument that actually decides it, and it comes from `doc/organon_is_the_product.md`
rather than from anything about modules. ✅ That document was **ratified as words on
2026-08-21** and `doc/organon_prd.md` is where the decision now lives as a product definition — so
this is a position the project has taken, not a proposal borrowed mid-argument.

That document's whole claim is that *"a window full of panels beside an instrument"* and *"a
window full of conversation"* stop being three programs and become three **arrangements of one
program**, because the arrangement is **data**, resolved at run time. Its §2 names the thing being
dissolved: `Edition` = `Full | Mind | Console`, chosen by cargo features — *"a runtime answer can
be switched, saved, shared and extended, which a cargo feature cannot."*

A linked module is adopted by **rebuilding**. Read James's sentence against that:

> *"open up Organon, point to the Ascent repo, and approve it as a module"*

Every verb there is performed by a **running program on a repository it is being shown**. "Open
up Organon" is a launch; "point to" and "approve" are done inside the thing that is already open.
A mechanism whose adoption step is `cargo build` answers none of them — it answers a different
sentence, *"add Ascent to the manifest and rebuild"*, which is a fourth edition arriving under a
new name three weeks after the document arguing that editions should go.

📌 So the ordering is: **hosted is what the sentence means**, and the licence and private-repo
problems are corroboration rather than the case.

### 2.3 What the licence actually contributes — and what it does not

Verified against the manifests on `main` rather than remembered:

| crate | licence |
|---|---|
| `organon-console`, `organon-core`, `organon-render`, `organon-scene`, `organon-world` | **MIT OR Apache-2.0** |
| `organon-visual`, root `organic-math-native` | **GPL-3.0-or-later** |

Ascent's invariant 3 forbids any edge to the second row, and it is currently satisfied
*structurally* rather than by care: `crates/ascent/Cargo.toml` names no Organon crate except an
optional `organon-render` for the material bake, and its CI gates both the default and
`--all-features` graphs.

⚠️ **A hosted module does not put that at risk, and it is worth being exact about why, because the
intuition "Organon's console binary is GPL, so anything it shows is" is wrong and common.** The
console front-end lives in `native/src/console_main.rs` — the **root crate**, GPL-3.0-or-later.
GPL obligations travel through **linking**, not through `CreateProcess`. A GPL program launching a
separate executable and reading bytes from it over a pipe or a shared mapping is the relationship
every shell has with every program it runs. The direction that would have been fatal — Ascent
linking Organon's GPL side — is the one hosted does not require, and the direction hosted *does*
require creates no obligation in either tree.

⚠️ **What this does not settle: the contract crate.** If both sides link a shared crate defining
the protocol's types and wire format, that crate must be on the permissive side — a **new member
of the console-side family**, never a module of the root crate. `cargo tree` is the acceptance
test **in both trees**, and Ascent's existing "no GPL edge" job already runs half of it.

✏️ **One inherited claim, corrected.** A note carried into this session said `organon-visual` has
zero files mentioning "viewport"; a later reading said one. Measured today: **zero `.rs` files**,
and **one** occurrence in the crate — a prose line in `organon-visual/Cargo.toml:45` noting that
the standalone *"does not launch the visual — its embedded viewport renders `World` directly."*
Both earlier statements were counting different things. The separation argument does not rest on
this count either way; §2.3's table does.

### 2.4 What hosted costs, stated before it is discovered

| | |
|---|---|
| **A frame boundary** | §4.4. The whole engineering content of this design, and the one number nobody has measured. |
| **Two processes to reason about** | crashes, hangs, orphans, and a viewport whose producer has died. §4.6. |
| **No shared `World`** | a hosted module gets no camera from `world.rs`, no `Shared` snapshot, no `organon set` lane. It is not a second view of Organon's scene; it is a different picture. |
| **Latency** | a copied frame is at least one frame behind, possibly more. §6 is where that stops being affordable. |

📌 **And the cost already paid.** The modules plan §4 makes this point and it holds: Organon's
visualizer already renders in a separate process and the console already composites the result
into a pane and can grow it. A hosted module is that shape with a different producer. The frame
copy is not a new tax on this architecture — it is the tax this architecture has been paying since
before any of this was proposed.

---

## 3. What "approve it as a module" means mechanically

> ✅ **BUILT — T3a (the data) and T3b (the verbs), 2026-08-22.** This section was written as a
> specification and every mechanism in it now exists:
> `native/organon-console/src/module.rs` (two files, two authors, the types that decide them) and
> `native/organon-console/src/module_work.rs` (`git`, `cargo`, and the four verbs), reached from
> all four front doors as `console module approve|build|diff|revoke`.
> `CONSOLE_ARCHITECTURE.md` §1.17 and §1.19 are its living state; **read those for what is true
> today**, and read the argument below for why it is shaped this way.
>
> Four things the build settled that the text below could not:
>
> - **§3.4's hole is accepted and said, per its own option 1.** `module_work::BUILD_TRUST` is one
>   constant reaching four surfaces — the dry run's sentence, the recorded approval's, the line
>   printed before a build, and `organon console module --help`. Nothing scans a `build.rs`.
> - **§3.1's "the manifest requests, the record grants" became a *gesture*, not only a pair of
>   types.** `approve` with no `grant` word is a **dry run** that records nothing and reports what
>   the repository asks for. Granting is the deliberate path; asking is the cheap one.
> - **§3.2 lives in one function** — `fetch_and_resolve` resolves a branch, a tag, a hash or
>   nothing at all to one forty-character hash before any record exists. `at main` is a fine thing
>   to type; the branch is stored beside the hash as provenance and is never the identity.
> - ⚠️ **A producer name turned out to be a directory name**, which none of the rules written for
>   it in T3a covered: `..` satisfied all four and named the store root's parent.
>   `check_producer_name` gained two path rules, checked at three gates.
>
> 🚨 **The general shape of that last one is worth carrying out of this document, because it will
> happen again.** A validator's rules are only valid for the **uses that existed when they were
> written**. T3a's four checks are all correct — every one is a true statement about a name
> surviving a whitespace-delimited wire — and T3b silently invalidated the set by widening what
> the value *is*, from a wire token to a path component. Nothing failed, nothing warned, and the
> rules went on passing; only the question they were answering had changed. So **widening what a
> value means is a change to every rule about it**, and the moment to re-read them is the moment
> a value gains a second use, not the moment something goes wrong.

### 3.1 Two files, two authors, and never one file

🚨 **The manifest requests; the approval record grants.** These must not be the same document and
must not be written by the same hand.

| | **`organon-module.toml`** | **`modules.json`** |
|---|---|---|
| Lives | in the module's repo, at its root | `<store_root>/modules.json`, beside `harnesses.json`, `layouts.json`, `preferences.json` |
| Written by | the module's author | Organon, at James's instruction |
| Says | *"I am a viewport producer called `ascent`; I need these things"* | *"this URL at this commit is approved, and holds these grants"* |
| Trust | **data, never instruction** | the console's own record |

A manifest that could grant itself anything is a permission system where the request and the
answer come from the same party. Everything in the manifest is a **request** and a **declaration
of identity**; nothing takes effect until a matching entry exists in `modules.json` and every
grant there was written by an approval a person performed.

**The shape of both follows `harness.rs`**, which the modules plan §4 already names as *"the
closest existing thing, and the right precedent"*: identity, launch command, how to detect it,
where to obtain it — with serde defaults and unknown fields tolerated, so a manifest written for a
newer Organon loads in an older one rather than failing. `layout.rs` supplies the other half:
**a corrupt library costs you your modules, never your console.**

### 3.2 The unit is a commit

Straight from the modules plan §11.3, which this design does not get to soften: a repo says *where
the bytes live*, a commit says *which bytes*. Tags move, branches move, force-push rewrites
history. `modules.json` records a **URL and a commit hash**, and a reference naming only a branch
is a reference that has not decided what it trusts.

📌 Ascent's own `CLAUDE.md` already lives by this in the other direction — it pins Organon by
revision and says why. The rule is symmetric and it is the same rule.

### 3.3 The affordance this buys, and it is the reason for the whole approach

§11.4: trust is not granted once, it is **renewed at every update**, and the update is the moment
that matters, because the code you audited is not the code that arrived. Git is the only
distribution mechanism where the console can answer:

> *"This module has changed 14 files since the commit you last trusted. Here they are."*

`git diff <approved>..<candidate>` is one command. That is the console verb worth building — not
"install", but **"show me what changed and ask again."**

> ✅ **BUILT, T3b — and "one command" turned out to be two, which is worth recording because the
> missing one fails in the least diagnosable direction.** Getting the candidate first needs a
> fetch, and **`git fetch <url> <sha>` is refused by most hosts**:
> `uploadpack.allowReachableSHA1InWant` is off by default, so a server declines to serve a bare
> object name while that object is perfectly reachable from the default branch. So a commit that
> *exists* reads as a commit that does not. The direct fetch is still tried first — it is the
> cheap case, and the only one that reaches a commit outside the default branch's history — with
> a full fetch and a local resolve as the fallback.
>
> 📌 And the affordance is finished by the *sentence*, not by the diff: it ends with the approve
> line that would trust the candidate, hash included, so renewing trust is one line a person can
> read rather than a verb they have to reassemble. **A diff nobody can act on is a report, not a
> gate.**

### 3.4 🚨 The hole in §10's clean table: **building from source is linked-level trust**

§10 draws the boundary crisply — linked has none, hosted has the process. §11.7 adds that source
is *required* for linked and *optional* for hosted, because the protocol bounds what a hosted
module can reach.

**Both are true of the module at run time and false of it at build time**, and this design walks
straight into it: "point Organon at a repo" means Organon has a repo, not a binary. Turning a repo
into a binary is `cargo build`, and §11.6 already states what that is — **`build.rs` runs at build
time with your privileges, and proc macros execute during compilation.** A module can take the
machine before a line of its code runs inside anything.

So the honest table has three columns:

| | Linked | **Hosted, built from source** | Hosted, prebuilt binary |
|---|---|---|---|
| Boundary at run time | none | the process | the process |
| Boundary **at build time** | **none** | **none** | n/a — you did not build it |
| Source available | required | yes, and it is why you can review | optional |

⚠️ **Naming it does not close it, and pretending the process boundary covers it would be the worst
available outcome** — a security property people believe they have. Three responses, and the right
one for James today is the first:

1. ✅ **Accept it and say it — TAKEN, T3b.** The approval gesture gates *building*; the process
   boundary gates *running*. For a repo James owns, on James's machine, that is the true state of
   affairs. It is said through `module_work::BUILD_TRUST`, one constant reaching four surfaces so
   that three copies of a security disclosure cannot become three chances for one of them to
   soften. 📌 And the corollary below was taken too: `BuildRecord` records the commit that was
   *built* and whether the tree was dirty, and `names_approved_bytes` is the single site where
   that is compared against the commit that was approved.
2. **Prebuilt binaries outside the innermost tier** — §11.7's "source optional for hosted"
   arriving as a policy rather than a permission.
3. **Build in a sandbox.** Real, and not now.

📌 The cheap corollary, which should be taken: **`modules.json` records the commit that was
*built*, not the commit that was approved**, and they must be the same value or the record
describes a binary that does not exist. If the tree was dirty at build time, record that rather
than recording a commit as though it named the bytes.

### 3.5 Revocation, and the rule it must satisfy

> ✅ **BUILT, T3b — and the rule below turned into a threading decision.** `revoke` is the one
> verb of the four that runs **synchronously on the frame thread**: it touches no network and no
> compiler, so the verb whose whole purpose is to withdraw trust cannot be queued behind a build,
> cannot fail because a worker thread died, and needs nothing to be reachable. ⚠️ Two things fell
> out that this text did not anticipate: a build finishing *after* a revocation must not
> resurrect the approval (`ModuleRegistry::record_build` answers `false` and the console says the
> build was dropped), and the **checkout is deliberately not deleted** — withdrawing trust is a
> statement about what Organon will run, not a licence to remove somebody's working tree.

§10 states the requirement and this design inherits it verbatim: **a layout referencing a module
you have stopped trusting must not fail to open.** `layout.rs` already has the shape — a load is a
transaction that refuses by name and leaves the current arrangement standing rather than
half-applying. A revoked module is not a corrupt layout; it is a **region whose producer declines
to run**, and §1.14's vacancy rule says what it draws: a sentence naming the module, saying it is
not approved, and naming the verb that would approve it again. Never a blank, never a stale
picture, never a window that will not open.

---

## 4. The viewport-producer contract

### 4.1 The seam it lands on already exists, in one sentence

`CONSOLE_ARCHITECTURE.md` §1.14:

> **A producer yields a texture the console can sample, at a size the console asks for.**

That is deliberately not *"a function that draws into our device"*, and the doc says why: the
in-process producer satisfies it trivially, and an out-of-process one satisfies it later **without
restructuring the region model**. This design is the collection of that promise. Nothing in
`Region`, `Layout`, `plan`, the lane, or the two presentations (portal, region) needs to change.

🚨 **Three things are preserved and one must move:**

- `Content::ThreeD` stays the content **word** (`3d`). It says *a 3D picture belongs here* and
  names no engine — the argument recorded at `region.rs:389`.
- `SceneMode::Workstation` stays. A module in a region is a bounded pane among widgets.
- The portal keeps taking the frame from a region — but see §4.5, because the *reason* changes.
- 🚨 **`Content::only_one_because` must move.** It answers today with a reason naming **Organon** —
  *"its producer is Organon, and Organon draws at most one frame per console frame."* Its own doc
  predicted this: *"a future producer might fill four regions happily, and would otherwise inherit
  a refusal it has no reason to obey."* That prediction arrives now.

### 4.2 🚨 The vocabulary: `3d <producer>`, not a content word called `ascent` — **BUILT (T4)**

> ✏️ **This section is now describing the tree rather than proposing to it.** Everything below
> shipped, with **two changes worth knowing before you read the rest** — both made because the
> console's own rules outranked this document's shorthand:
>
> 1. 🚨 **The spelling is `viewport left 3d producer ascent`, not `viewport left 3d ascent`.**
>    §1.8's grammar fills *required* arguments positionally and *optional* ones **by keyword**, at
>    every one of the four doors. #98 Tier C settled this one verb over: `ConsoleOp::Stack`'s
>    optional region is spelled `region <word>` *"because the slash grammar fills optional
>    arguments by keyword: a bare third word would make the typed line and the sidecar line
>    disagree, which is the drift the four doors exist to prevent."* The illustrative form below
>    would have needed a second grammar for one verb. **James, this is a departure from what you
>    read** — it is one decision to reverse, and reversing it means letting the grammar accept a
>    trailing positional optional for *every* verb.
> 2. 📌 **A `3d ascent` region draws a SENTENCE, not a picture.** Nothing renders a hosted
>    producer — there is no protocol and no process; §9's T3b and T5 own those. The rectangle
>    carries `ModuleState`'s line, which is §4.6's first two rows and the only two reachable with
>    nothing running.
>
> `CONSOLE_ARCHITECTURE.md` §1.14's *"`3d <producer>` — the producer qualifier, T4"* is the living
> description; what follows is the argument that produced it, kept because the argument is the
> part worth having.

James said *"viewport type `ascent`"*. Taken literally that is a fourth content word beside
`agent`, `panel` and `3d` — and it is the one thing `region.rs` argues against at length: `world`
lost to `3d` precisely because it *"names Organon's renderer, which is the one thing the word must
not do."* A word called `ascent` makes the same mistake with a different application's name.

**The shape that gives James what he asked for and keeps that argument intact is a producer
qualifier inside `3d`:**

```
viewport left 3d              # the default producer — Organon's World, today's behaviour
viewport left 3d ascent       # the same rectangle, a different producer
```

- ✅ `CONTENT_WORDS` stays a fixed four-word table, and
  `the_word_tables_and_the_resolvers_are_one_vocabulary` keeps passing unchanged.
- ✅ Producer names are a **second, dynamic vocabulary** sourced from the approved-module list —
  the shape §1.15 already built and measured for saved-layout names: a `NarrowFn` over a library,
  **cached rather than read straight**, because the candidate walk runs on the draw path and asks
  n + 1 times per call. That measurement is inherited; do not re-learn it. *(Built as
  `registry::viewport_options` over T3a's `ModuleRegistry::for_completion` — no second cache. The
  ring also reads the **content** word, so a producer offered beside `agent` answers `Ring::Empty`
  with the reason rather than staying silent.)*
- ✅ An omitted producer means `organon`, so every existing layout, every `layouts.json` written
  before this, and every line in the docs continues to mean exactly what it meant. *(Checked as
  bytes: Organon's producer contributes **no word at all**, so a captured layout stores `3d` and a
  sidecar line reads `viewport left 3d`, byte-identical to what they were.)*
- ✅ An unapproved or unknown producer is **refused by name**, listing the approved ones. `3d` with a
  typo must never silently fall back to Organon's World — the person would get a picture, and the
  wrong one, which is worse than a refusal.
- ⚠️ **…at the COMMAND door only.** A producer read out of a **saved layout** is deliberately not
  checked against the approved set, because §3.5 requires that *"a layout referencing a module you
  have stopped trusting must not fail to open."* So there are two resolvers — `Producer::resolve`
  (typed, refuses by name) and `Producer::stored` (from a file, refuses only a word no manifest
  could have declared). A revoked module is a region whose producer declines to run, which is what
  §3.5 asks for; checking approval on load would have turned a revocation into an arrangement that
  will not come back.

📌 **And uniqueness becomes the producer's property, which is where its own doc said it belonged.**
`only_one_because` moves from `Content` to the producer: Organon answers with today's reason
(shared `frame_index` and TAA jitter phase), and a hosted module answers `None` unless it has a
reason of its own — a separate process rendering into its own texture has no jitter phase to
trade. **Two Ascent viewports are a thing the architecture permits**; whether anyone wants them is
a different question, and the refusal machinery should not answer it by accident.

✅ **Built, and §8's open question is closed.** It is a `Producer` enum — `Organon | Hosted(String)`
— and `region.rs`'s standing objection to inventing one is **discharged rather than overruled**:
that objection was to an enum *with one variant*, an unreachable arm pretending to be a design.
There are two now and both are reachable from a command a person types.
`Content::only_one_because` survives as a one-line forward so that `Layout::assign` and
`Layout::from_placements` ask one question rather than each unwrapping a `ThreeD` for itself.

⚠️ **The representation was the real work of the tier and it is not free.** A producer name is a
runtime string, so `Content` gave up `Copy` — and `ContentCmd`, `Layout` and `Placed` with it. The
two alternatives (an inline fixed-capacity name; an interned `&'static str`) are weighed in
`region.rs`'s header. 📌 The property `plan` leans on survives: every value that existed before T4
is still unit-shaped and clones with **no allocation**, so a console with no approved module costs
exactly what it cost before.

### 4.3 What Ascent gives up, and it is exactly three things

✏️ **Corrected by the Ascent session, which measured it rather than reading the header.** An
earlier draft said *"its host is ~600 lines"*. That is true only of the `ApplicationHandler`,
`main` and the window bring-up — about 260 lines plus setup. `crates/ascent/src/main.rs` is
**4 498 lines**, and the part that actually has to move is `struct Gpu` and its `impl` at lines
**2611–3664 — ~1 050 lines** on its own. The shape below is right; the tier is not a 600-line
shuffle and must not be scoped as one.

**The host is not thrown away.** §6 is why it must survive.

📌 **And the seam is cleaner than "give up the surface" suggests, which is a better argument
for hosted than this document originally made.** `struct Gpu` holds exactly **two** surface-specific
fields — `surface: wgpu::Surface<'static>` and `config: wgpu::SurfaceConfiguration`. Everything
else it holds is device state a texture-target host drives identically: device, queue, pipeline,
depth, vertex and index buffers, draw ranges, the camera uniform, the bind group, the `frame_layer`
indirection, the baked material arrays and the tuning floats. So the boundary this design asks for
is **two fields**, not a diffuse entanglement to be teased apart — which is the concrete reason to
believe the refactor is a re-homing rather than a rewrite.

| Gives up | Keeps |
|---|---|
| **The surface.** Renders into a texture the host names, not a swapchain it acquired. | Its device, its pipelines, its depth buffer, its shaders. |
| **The event loop.** Is *ticked* — `winit`'s `ApplicationHandler` becomes one caller of its step, not the owner of it. | Its fixed-step `ascent_engine::tick` and its interpolation. |
| **The input source.** Takes events it did not read from `winit`. | `ascent_engine::input` and everything downstream. |

🚨 **The refactor that satisfies all three is one shape: separate the game from its host.** The
`host` feature already draws that line — `crates/ascent/src/main.rs` is owned by A4, gated behind
`required-features = ["host"]`, and the library beneath it has no winit in its graph. What must
move is the boundary's *height*: today `main.rs` owns the device, the surface, the pipelines and
the loop. It must come to own only the **window** and the **event pump**, with everything below —
device, pipelines, render-into-a-texture-of-size-N, `step(dt)`, `feed(input)` — sitting in the
library where a second host can drive it.

📌 That is the same refactor whether the second host is Organon, a benchmark, or a headless frame
dump, which is the test of whether it is the right one. And `fly.ps1` keeps working throughout,
because the winit host stays.

### 4.4 The frame boundary — two mechanisms, and B is now measured

This is the whole engineering content, and it is the one place this document refuses to recommend
without a number.

**A — a shared GPU texture.** DXGI shared handle on D3D12, `VK_KHR_external_memory_win32` on
Vulkan, IOSurface on Metal, dmabuf on Linux. Zero copy. The end state.

- ⚠️ Needs `wgpu-hal` interop on **both** sides — `as_hal` / `create_texture_from_hal`, `unsafe`,
  per-backend. The tree already knows this surface exists and knows its shape:
  `organon-world/src/metal_island.rs` documents the `as_hal` handshake in detail and deliberately
  avoids it, which is the honest precedent for how expensive it is to do well.
- ⚠️ Needs both processes on the **same backend and the same adapter**. No `Backends::` restriction
  is set anywhere in this tree, so both sides take whatever wgpu picks — a thing to pin before it
  is a thing to debug.

**B — a shared-memory frame copy.** The producer reads back into a memory-mapped ring; the console
uploads. Portable, no `unsafe`, and it reuses the mechanism this project already runs on:
`ipc::ns_file` namespaces every mapping and sidecar, `$ORGANON_IPC_NS` overrides it at runtime,
and `term.rs` already injects that variable into every child the console spawns — so a module
process coexisting with an Organon session is solved rather than new.

- ⚠️ The cost is a GPU→CPU readback **and** a CPU→GPU upload per frame, plus the fence wait. The
  bandwidth arithmetic is easy and is not the risk; the **stall** is what decides it.

✏️ **Two of the three numbers this section asked for now exist**, measured on this machine and
recorded at `doc/measurements/module-frame-boundary-2026-08-21.md` (harness:
`native/organon-render/tests/frame_boundary.rs`, `#[ignore]`d, `cargo test -p organon-render
--release -- --ignored --nocapture`). RTX 5090, **Vulkan** — wgpu's own choice, since nothing in
this tree restricts `Backends::` — `Rgba8UnormSrgb`, 64 iterations after warm-up, median.

| | 640×360 | 1280×720 | 1920×1080 | 2560×1440 |
|---|---:|---:|---:|---:|
| **1. producer's added stall** | 0.06 ms | 0.13 ms | 0.20 ms | 0.35 ms |
| **3. full round trip** | 0.19 ms | 0.44 ms | 0.80–0.88 ms | 1.41–1.55 ms |
| … as a share of 16.7 ms | 1.1 % | **2.6 %** | 4.8–5.3 % | **8.4–9.3 %** |

📌 **So the copy is affordable at region size and still affordable at the full pane, and
that settles the ordering rather than the mechanism.** B first, behind the same seam; A when the
measurement says the copy is what hurts. It does **not** say A is unnecessary — it says A is not
yet *justified*, which is the standard this section itself set for buying `unsafe` per-backend
interop.

⚠️ **Two conditions on that reading, both of which would overturn it:**

1. **A preallocated ring, not a per-frame allocation.** Measured: fresh staging buffer plus
   destination texture per iteration costs **2.72 ms at 1440p against 1.37 ms reused**, and 3.3×
   of the gap is `memcpy out` copying *identical bytes* — first-touch page faulting, not
   bandwidth. 🚨 A naive per-frame path therefore measures 2.7 ms, reads that as a verdict on
   mechanism B, and buys `unsafe` interop to fix an allocator problem.
2. **60 Hz.** At 120 Hz the full pane at 1440p is 17–19 % of a frame and would want another
   look. A region still would not.

✏️ **The third number was taken in T5** — `doc/measurements/module-staleness-2026-08-22.md`. It
needed a second process and a protocol; T2 built the protocol, T5 built the launcher, and
`organon-module-sim` is a producer in its own program. **The picture on screen is `≈0.50 × the
producer's period` old** — 8–11 ms when both ends run at 60 Hz, half a frame, flat across nine
times the pixels; **50 ms from a producer drawing at 10 Hz to be cheap.** ⚠️ The poll interval does
not enter: polling faster does not make a frame younger, it shortens how long a stale one stays up.

🚨 **The control is what actually decides §6.** Size barely moved it, so the hypothesis had to be
tested rather than asserted: hold the size fixed and move the producer's cadence. Staleness is
`≈0.50 × the producer's period` (the poll interval does **not** enter) — set by the two loops' **phase**, with the frame size
absent from the expression. So the reading that would have forced mechanism A — *staleness is the
copy, therefore buy `unsafe` per-backend interop* — is **not** what the measurement says, and
mechanism A stays not-yet-justified on this evidence as well as on T0's. ⚠️ It was still not
measured at all: nothing here says a shared GPU texture is faster, slower, or works.

⚠️ **What that number does and does not license.** It measures publish → the consumer holds the
pixels, which is the part the *protocol* owns. It excludes the console's own frame — egui,
`write_texture`, the render pass, the present — and the producer's render. So it says the transport
is not the thing in the way; it does **not** say that flying at full screen feels right, which is a
question about input-to-photon and is answered by no measurement yet taken.

⚠️ Nor was the shared-memory ring itself — its synchronisation, double-buffering and tearing are
untouched, and `memcpy out` here lands in process-local memory rather than a memory-mapped file.
The GPU was otherwise idle throughout; a real producer's copy competes with its own render and a
real console's re-upload competes with `World`. And `memcpy out` is a **CPU** number on the fastest
consumer GPU available — it will not travel to another machine, so nothing above should be quoted
as *"the cost of a frame copy"* without the adapter line beside it.

### 4.5 The frame arbiter, and the one thing that genuinely changes

`engine_plan` is `(portal_open, region_holds_world, backdrop, patches_want_image) ->
(BackdropSource, Option<ViewportTarget>)`, and it exists to guarantee **at most one `World` render
per frame** — proved over the whole input space by `the_engine_is_asked_for_at_most_one_frame`.

🚨 **A hosted module is not a claimant on that.** It does not render `World`, does not touch
`frame_index`, and does not share the TAA jitter phase. So `engine_plan`'s invariant is untouched
and its test does not widen — which is the strongest available evidence that the seam is in the
right place.

⚠️ **What does change is which rectangle asks Organon for a frame.** A region holding `3d ascent`
must not count as `region_holds_world`, or the console renders a `World` frame nobody paints and
starves the backdrop for it. That is one call site and one boolean, and getting it wrong is
silent — a wasted frame is not an error. It wants its own test, and the test is cheap because
`engine_plan` is pure.

✅ **Done in T4, and the prediction about the invariant held exactly.**
`Console::region_showing_world` is now a free `region_showing_world(&Layout)` — split out so the
answer can be tested with no window — and it asks for `Content::ThreeD(Producer::Organon)`
precisely. `the_engine_is_asked_for_at_most_one_frame` is **unedited**: widening its input space
for a hosted producer would assert that a hosted module is a claimant, which is the opposite of
what this section says. The claim lives in
`a_hosted_producer_does_not_make_the_console_render_a_world_frame`, where it is about the arbiter's
**input** rather than about the arbiter.

### 4.6 What the console must never learn, and what it must always be able to say

**Never:** what the module *is*. No `ascent` arm, no game-shaped verb, no knowledge of a HOG file,
a mine, a ship or a level. The console knows a producer name, a rectangle, a size, a texture and
an input stream. Anything else is Ascent's assumptions becoming Organon's architecture — the
failure the three-level model exists to foreclose, and the modules plan warns about the identical
thing one level down (*"promoting on the first consumer is how a game engine acquires one game's
assumptions as its architecture"*).

**Always:** the console owns every one of the module's failure modes as a **sentence in the
rectangle**, because §1.14's vacancy rule applies with more force to a picture than to an empty
quarter — a rectangle that was rendering and now is not is precisely what a broken viewport looks
like. Four states, each with its own line rather than a shared "something went wrong":

| | The rectangle says |
|---|---|
| not approved | the module, that it is not approved, and the verb that approves it |
| approved, not built | the module, the commit, and the verb that builds it |
| launched, not yet producing | that it is starting — and this one must **time out** into the next row rather than sitting forever |
| died / hung / stopped producing | that it stopped, and the verb that restarts it. **Never the last good frame.** |

⚠️ A stale texture is the single worst thing this design can paint, and the easiest to paint by
accident, because the texture is still there and still valid. The console made this call once
already for the portal-versus-region loser, and made it the same way.

---

### 4.7 🚨 Starting a module: the two strings both trees must agree on — **BUILT (T5)**

Everything above describes what crosses the boundary once a module is running. This is how it comes
to be running, and it is **two strings and nothing else**. They are here rather than only in the
code because they are the one part of this design that **two repositories must spell identically**,
and a convention that lives in one tree's source plus a conversation is a convention that drifts
the moment a third party arrives. Both were agreed independently by the Organon and Ascent sessions
before either had read the other's code, which is evidence they are the obvious answers — and no
protection at all against the fourth session that guesses differently.

#### 1. The channel address: `ORGANON_MODULE_CHANNEL`

The console passes the **absolute path** of the channel file in the environment:

```
ORGANON_MODULE_CHANNEL=<absolute path to the .frames file>
```

The console composes it (`organon-core::ipc::ns_file` + `organon_module::channel_file_name(producer,
instance)`) and hands over the finished result. **The producer resolves nothing and joins nothing.**

🚨 **The argument is structural rather than a preference, and it is the reason the namespace is
*not* what gets passed.** The full path is `ns_file(channel_file_name(producer, instance))`. A
producer has `channel_file_name` — it is in `organon-module`, the crate it links. It does **not**
have `ns_file`: that lives in `organon-core`, and `organon-module`'s manifest forbids depending on
it, because *"taking a dependency on the engine's spine to get one `PathBuf` would put `glam`,
`half`, `bytemuck`, `serde` and `serde_json` into a game's build for a string."* So handing over a
namespace would require a producer to **re-implement a rule owned by a crate it may not link**, and
the drift shows up as a channel that opens nothing with no error saying why. `channel.rs` already
settles the near half of this — *"this crate names the file and the console places it"* — and this
is the far half.

⚠️ **`instance` is deliberately not passed separately.** The path already contains it, and a second
statement of one fact is what this whole choice exists to avoid.

📌 **An environment variable rather than an argument, and the deciding constraint belongs to the
other repository.** A module keeps its own `main.rs`, its own pump and its own CLI — Ascent's takes
`--hog`, `--level`, `--segment`, and `fly.ps1` keeps working — so an argument the console appended
could collide with a `clap` definition Organon cannot see. An environment variable cannot, and it
does not appear in `ps`. **Presence is therefore the discriminator**: a binary that finds this set
is being hosted, one that does not was run by a person. That should be the branch in a module's
`main`, not a new flag.

⚠️ **`ORGANON_IPC_NS` is set on the child as well, and it is NOT the channel address.** It is there
for the reason `term.rs` already sets it on every tab the console spawns: anything *Organon-shaped*
the module itself runs must address this console's session rather than the default namespace. A
module should ignore it for channel purposes. Both are named here, with which one is the address
said out loud, because that is exactly the confusion this paragraph exists to prevent.

⚠️ **The working directory is set to the module's own checkout, and nothing may depend on it.** A
module that reads a file beside its binary is doing the ordinary thing and should keep working; a
module that requires a particular cwd is relying on something the contract does not promise.

#### 2. The binary: derived, never declared

```
<store_root>/modules/<producer>/target/release/<producer>[.exe]
```

> 🚨 **The obligation, stated as a requirement because it is one:**
> **a module's repository must produce a release binary named for its producer from a plain
> `cargo build --release`, run at the root of its checkout.**

⚠️ **Plain** is the load-bearing word, and it is not pedantry. The console runs exactly
`cargo build --release` with no `--features`, no `-p` and no `--bin` — because every one of those
would be a string the console chose on a module's behalf, and a `features = [...]` key in
`organon-module.toml` to supply them is **refused**: the manifest requests and declares, it does not
configure the host's tooling. So a `[[bin]]` behind `required-features` that a module's own default
features do not enable **is not built**, and the module does not meet this requirement.

🚨 **And cargo will not say so.** It skips such a target with no warning, no diagnostic and **exit
0** — measured in a clean Ascent tree: remove `target/release/ascent.exe`, `cargo build --release`,
exit 0 in 0.12 s, file still absent. That is why `module_work::build` verifies the binary exists
before recording a build rather than trusting the exit code: without it, `modules.json` records a
successful build of a commit whose binary does not exist, and the failure surfaces two layers later
as *"launched, not yet producing"* timing out, with nothing naming the cause.

📌 Publishing the requirement is what makes that refusal **fair rather than arbitrary** — a module
is failing a stated obligation, not colliding with an undocumented assumption.

🚨 **Derived rather than read from the manifest, and the reason is §3.1's rather than tidiness.**
`organon-module.toml` is data written by somebody else. A `binary = ` key in it would be that
somebody's string arriving at `std::process::Command::new` — which is precisely what
`module_work.rs`'s two-variant `Tool` enum exists to make impossible for `git` and `cargo`, and the
rule does not weaken because the program is the module's own. The producer name is the only
influence a manifest has over this path; it is a name the **person** typed, the repository agreed to
it (`IdentityMismatch` refuses otherwise), and `check_producer_name` gates it before it is a path
component.

📌 It is also `artifact_dir`'s argument one step further on: a recorded path is a second statement
of where the build went, and a store restored from a backup or a `target` cleaned by hand makes the
two disagree. A function of the store root and the producer name cannot.

#### The startup order, which the console guarantees

**The channel is created before the process is started.** `ModuleChannel::create` writes the header,
seeds the two producer-owned words and lays down the slots; only then is the binary spawned. So
`ProducerChannel::open` should succeed **first try**, and a producer meeting a `WireFault` at open
is looking at a launcher bug rather than a version mismatch.

⚠️ **A producer that cannot open its channel should exit non-zero rather than retry-loop.** The
console times `Starting` out into `Lost` after `Timings::start_within` (10 s), so a silent retry
turns a legible failure into a hang — and §4.6's third row exists specifically because *"a rectangle
that says 'starting…' indefinitely is the failure state that looks most like working software."*

#### What is deliberately NOT here, and must not be added quietly

🚨 **Nothing tells a hosted module its own configuration.** Under `fly.ps1` a person types
`--hog <path>`; under module hosting nobody types anything, and this contract has no path for it.
That is a real hole and it is named rather than papered over. **Do not invent a config channel** —
not a manifest field, not a second environment variable, not a settings file. `doc/organon_modules_plan.md`
§10 is explicit that **every verb added to the protocol is a grant**, and a configuration channel is
the widest possible one: it is "the host may tell the module anything", which is a permission nobody
argued for. When a module needs configuration, it gets designed across both ends at once.

## 5. Paused, no sound, click to interact

### 5.1 The lifecycle belongs to the contract; the *default* is the invariant

James's *"paused start state with no sound"* is asked about Ascent, and the right answer is
neither "a property of this module" nor "a property of viewports" but **a property of the
protocol** — which is `CLAUDE.md` invariant 4 arriving where it always applies:

> **New capability defaults to inert.**

So the contract carries a lifecycle — `Attached` (the producer exists and is drawing, but time is
not advancing) → `Running` (ticking) — and **`Attached` is what a module gets on arrival, always,
with no way for the manifest to ask otherwise.** A module that could declare itself auto-running
has a manifest that grants itself something, which §3.1 forbids.

📌 **Organon's own `World` answers this trivially and that is not a counter-example.** It has no
pause state and needs none: it is ambient, it is silent, and it is already the thing the console
renders. It satisfies the default by being the default. The rule is not "every viewport must
pause"; it is "**a producer the console did not write starts inert**", and the one producer the
console did write is exempt for a reason it can state.

### 5.2 🚨 Sound is a **grant**, and it is *promised* rather than *enforced* — say so out loud

The strong form of "no sound" is not asking politely. Modules plan §10: **the protocol is the
permission set** — a hosted module can do exactly what the protocol lets it do. So v1 exposes **no
audio path**, and a module cannot make a sound *through Organon* because there is no channel to
make one through.

⚠️ **And that is not sufficient, which must be written down rather than discovered.** A hosted
module is a separate process on the machine. Nothing in a texture-and-input protocol prevents it
opening WASAPI itself, and Ascent **already does** — `host` pulls in `ascent-engine/audio`. So:

- Audio is a **declared grant** in the manifest and an **approved grant** in `modules.json`, and
  the module is expected to honour it.
- 🚨 **It is honoured, not enforced.** The process boundary bounds what a module reaches *through
  the protocol*; it bounds nothing about what it reaches *on the machine*. Same asymmetry as
  §3.4's build-time hole, in the other direction, and it deserves the same treatment: named, not
  papered over. A trust model whose audio guarantee is really an expectation should use the word.
- 📌 The mitigation that *is* enforceable is OS-level (a job object, an audio session policy) and
  is not now. What is now is that Ascent is ours, honouring the grant is one branch in its audio
  init, and the contract says which branch.
- ⚠️ **And that branch has to be added rather than preserved**, which is the difference
  between a cheap tier and a forgotten one. Reported by the Ascent session: its host does not
  merely *have* audio, it starts level music **unconditionally at load** and prints a line saying
  so. The default today is the opposite of the default the contract requires, and the standalone
  binary's behaviour must be left exactly as it is while the hosted path gains the gate.

### 5.3 "Clicking into it" is an interaction latch, and the console has already argued it

*"Clicking into it lets you interact with it still in the viewport"* is a claim on input, and the
precedent is `portal`'s: **a scene patch is a picture, a portal is an instrument.** A picture that
stole the wheel would break scrolling; an instrument that did not take the wheel would be
unreachable.

A module viewport is an instrument, and a **more** demanding one than the portal — a 6DOF game
wants the keyboard, wants the pointer captured, and wants Escape. So:

- **Before the click** it is a picture: no key claim, no pointer capture, wheel scrolls the page.
  This is also what makes `Attached` legible — a paused picture that does not eat your keystrokes.
- **After the click** it is an instrument: it takes the input the grant allows, and it is
  `Running`.
- 🚨 **The way out must be decided before the way in is built.** The console has exactly one
  precedent and it is a warning: §1.12's `console screen` chose F11 as the way out and needed an
  argument to do it, and the portal's unbuilt states already carry the note that **Escape must be
  consumed state-conditionally** — `consume_key` `retain`s out of the same `i.events` vector
  `term_view` clones, and those states are precisely the ones that need it. A game that swallows
  Escape and a console that needs Escape are the same key, and this is the first place they meet.
  ⚠️ Whatever is chosen must be a key the module is **told it will never receive**, rather than a
  key we hope it ignores.

---

## 6. The scale ladder — and the affordance changes what it costs

*"scale it up first to fill the whole console border, then again to full screen, and back down."*

Three rungs, and the console already owns two of the three axes they sit on:

| Rung | Mechanism | State |
|---|---|---|
| in a region | `region.rs` — the pane divided | **built** |
| filling the console's pane | `viewport full 3d ascent` — a displacement, already legal | **built** |
| filling the display | 🚨 two different things, and they are not the same work | see below |

⚠️ **"Full screen" is ambiguous here in exactly the way `CONSOLE_ARCHITECTURE.md` warns about, and
the two readings have different costs.** §1.12's `console screen` is the **window** covering the
display — a third orthogonal axis, `screen.rs`, which suppresses nothing inside the window. The
portal's unbuilt "full screen" is the **rectangle** suppressing the tab strip, the glyph grid and
the scrim, and the doc records that no path does that today. §1.12's verb is named `console
screen` rather than `console fullscreen` **precisely so the second reading keeps the phrase.**

So rung 3 is `console screen` (built) **plus** the portal's unbuilt full-screen rectangle, which
is genuinely new and is a portal tier rather than a module tier. They compose, and the composition
is the state James is describing.

### 📌 The rung that de-risks the frame boundary: **hand the display over**

The synthesis worth taking seriously, because it turns §4.4's hardest constraint into a choice
rather than a wall.

A copied frame at one or two frames of latency is **fine for a paused picture in a region, and
fine for clicking around in one**. It is **not** fine for flying a 6DOF ship at full screen —
which is the single quality attribute Ascent's PRD calls the most important in the product, and
the one no test can reach.

But at full screen the module does not need to be composited at all: nothing of the console is
visible behind it. So rung 3 can be a **handoff** — the module's own borderless window, over the
console, driven by the winit host it already has and which §4.3 keeps. Zero copy, zero latency,
zero new graphics code, and the flight feel is the one Ascent's own host already produces.

⚠️ **What that costs, stated so nobody discovers it in a demo:** the transition is a window
appearing rather than a rectangle growing, so the *"animated grow"* between rungs 2 and 3 either
does not exist or is faked. And the two rungs run different code paths, so a bug can live in one
and not the other. Both are real, and both are cheaper than a zero-copy shared texture bought
before anything has been measured.

✏️ **Something has now been measured, and it moves this paragraph without settling it**
(`doc/measurements/module-staleness-2026-08-22.md`). The picture on screen is
**`≈0.50 × the producer's period` old**, flat across nine times the pixels — half a frame when both
ends run at 60 Hz. So the sentence above that reads *"a copied frame at one or two frames of
latency"* is right about the magnitude and wrong about the **cause**: it is not the copy, and 1440p
is not worse than 640×360.

🚨 **And the poll interval does not enter, which is the part with a consequence.** Polling faster
does not make a frame younger — it shortens how long a stale one stays up. So a module that idles
cheaply is the thing that puts an old picture on the glass: **a producer drawing at 10 Hz shows a
50 ms frame however often the console looks.** ⚠️ On a *still* scene that costs nothing, because
nothing is moving and nothing is late; what it actually buys is how far a resize trails the border
being dragged. That is a feel rather than a figure, and it wants a hand on a border.

🚨 **Which changes what the handoff is FOR.** It was framed as the escape from a transport cost
that would be intolerable at full screen; that cost is not there. What is still there — and is now
the only argument for it — is that **nothing has measured input-to-photon**, the quantity flying
actually turns on, and that a handoff avoids the question entirely rather than answering it. That
is still a good reason to keep Ascent's winit host, which is why §4.3 stands. ⚠️ But it is a
weaker and more honest reason than the one written above, and anyone reaching for mechanism A on
the strength of this section should notice that the measurement points away from it.

---

## 7. Is a hosted module a kind of `media`, or its own thing?

`media` is already queued as a fourth content word, waiting on §1.13's placement question, and
`region.rs` records that absence as scope rather than oversight. Deciding this before either is
built is cheaper than deciding it after.

**They are different, and the discriminator is clean: `media` is a *file*, a module is a
*process*.** An exhibit names a path a human typed and the console reads bytes; a module viewport
names a producer that runs, ticks, takes input, can die, and has a trust relationship. Nothing in
`media`'s eviction policy, its off-thread reads or its `Failed` state generalises to something
with a process lifetime, and nothing in a producer's lifecycle helps show a PNG.

📌 So: `media` lands as its own word on its own schedule; a module lands as a **producer inside
`3d`**. Two absences, two answers, no merge. And the word `3d` has now paid for itself twice —
once by refusing `world`, once by absorbing a producer it was never told about.

---

## 8. What this does not decide

- **The protocol's wire format.** Types, framing, versioning. A tier, not a paragraph, and it
  should be written against §4.4's measurement rather than before it.
- **Whether a module may produce anything other than a picture.** A module contributing a *panel*,
  a *command* or a *harness* is each a separate grant and a separate design; every verb added to
  the protocol is a grant (§10), which is the reason to add them one at a time and never as a set.
- **Anything about skills**, which the modules plan §12 establishes as a **third** unit of
  extension alongside the two module kinds — an agent extended by text rather than by a process.
  ⚠️ The two meet at exactly one point, worth naming so nobody merges them: a hosted module is a
  thing Organon *runs*, a skill is a thing the resident agent *reads*, and a module that shipped a
  skill would be asking for a grant on the **agent** rather than on the viewport. That is §12's
  question, not this one's.
- **`only_one_because`'s new home in the type system.** §4.2 says it moves to the producer;
  whether that is an enum, a trait or a second reason string is an implementation call, and
  `region.rs` has a standing objection to inventing a `Producer` enum with one variant — which
  should be re-read when there are two.
- **Anything about trust tiers beyond this one module.** §10 is explicit that the tiers want a real
  second producer before they are designed, and this design deliberately produces exactly one.
- **Whether any of it is any good.** Whether a game in half a console window earns its half,
  whether the paused state reads as deliberate or broken, and whether flying it inside a region
  feels like anything at all, are James's calls and no test reaches them.

---

## 9. The order

A spine rather than a schedule; each rung is independently useful and none needs the next.

| | | Wants |
|---|---|---|
| ✅ **T0** | **Measure the frame boundary** — **done, all three.** Numbers 1 and 3 in `doc/measurements/module-frame-boundary-2026-08-21.md`; ✏️ **number 2, cross-process staleness, in `doc/measurements/module-staleness-2026-08-22.md`** once T2 and T5 had made two processes exist. | a GPU |
| **T1** | **Ascent's refactor** — the library owns the device, the pipelines, `render_into(texture, size)`, `step(dt)` and `feed(input)`; `main.rs` keeps the window and the pump, and `fly.ps1` keeps working. | the parallel Ascent session |
| **T2** | **The contract crate** — permissive, console-side, both trees depend on it, `cargo tree` gates both. **B**, per T0, with a preallocated ring rather than a per-frame allocation — that condition is the measurement's, not a preference. | T1's real signatures |
| ✅ **T3** | **`modules.json`, `organon-module.toml`, and the approve verb** — **done.** T3a landed the data (`module.rs`); T3b landed the verbs (`module_work.rs`), on the harness precedent with `layout.rs`'s refusal discipline: approve, build, record the built commit, diff, revoke. `CONSOLE_ARCHITECTURE.md` §1.17 and §1.19. ⚠️ Nothing launches and nothing draws — §4.6's *launched, not yet producing* and *died* states are unreachable because no process exists to be in them. | — |
| ✅ **T4** | **The producer qualifier** — `3d <producer>`, the dynamic ring cached per §1.15's measurement, `only_one_because` moved, `engine_plan`'s boolean corrected and tested. **All four landed** (`CONSOLE_ARCHITECTURE.md` §1.14). ⚠️ Two departures from §4.2 as written: the spelling is keyword-tagged (`producer ascent`), and a *stored* producer is not checked against the approved set — §3.5. 📌 It drew no picture; a hosted region carried `ModuleState`'s sentence, and T5 replaced it with the module's own. | T3, for a producer to name |
| ✅ **T5** | **The picture arrives** — launch, consume, paint, and **all four failure sentences**, the last two of which were unreachable rather than unbuilt (`CONSOLE_ARCHITECTURE.md` §1.21). `console module restart` is the fifth verb; `engine_plan` is untouched. ⚠️ **The tier was scoped down from "lifecycle and input" to lifecycle alone**: a module arrives `Attached` and there is no method that changes it, which is invariant 4 as structure. 📌 §5.3's click latch, `Running`, and the way out are now **T5b** — deliberately split, because getting a paused picture on screen is what makes every one of those testable against something real. | T3, T4 |
| **T5b** | **Interaction** — the click latch, `Running`, the way out, and §5.2's audio grant honoured in a producer that has something to say. | T5 |
| **T6** | **The ladder** — rung 2 is already legal; rung 3 is the handoff, or the portal's full-screen tier. ✏️ **T0's number 2 now informs it and does not settle it**: staleness is the two loops' phase rather than the copy, so the transport is not what stands between a region and full screen — but nothing has measured input-to-photon, which is the quantity flying actually turns on. | §6 |

⚠️ **T0 before T2 is not caution, it is the ordering that stops a wire format being designed for a
mechanism that turns out to be the wrong one.**

✏️ **That ordering has now paid.** T0 answered before T2 was written, and it changed T2's brief in
two ways a wire format would have had to be rebuilt for: the mechanism is **B**, and the ring must
be **preallocated**, because the naive shape costs double and fails in a way that reads like a
verdict on the mechanism rather than on the allocator.

---

## 10. For James

Four things this document assumed on your behalf. None blocks the tiers; each changes what gets
built if it is wrong.

1. **Hosted, not linked** (§2). The deciding argument is your own sentence — "open up Organon,
   point to the repo, and approve it" describes a running program, and a linked module is adopted
   by rebuilding. If what you actually want is a rebuild, this whole design is the wrong one and
   it is much cheaper to say so now.
2. **"Approve" includes building it here, from source, at a pinned commit** (§3.4) — which means
   `build.rs` and any proc macro in the module's graph run as you, before anything is composited.
   For your own private repo that is obviously fine. It is written down because the sentence "the
   process is the boundary" would otherwise be believed further than it is true.
3. 🚨 **`3d ascent`, not a content word called `ascent`** (§4.2) — **and this is a
   departure from your own words, flagged rather than left to land quietly.** You said "viewport
   type `ascent`"; this reads it as *a viewport whose producer is Ascent* rather than as a fourth
   kind of region, because `region.rs` chose `3d` over `world` specifically so the content
   vocabulary would never name a renderer, and `ascent` makes the same mistake with a different
   name. The Ascent session independently agreed and raised the same objection to itself:
   **two agents agreeing with each other is not the same as you agreeing**, and a vocabulary word
   is cheap now and permanent later. If you meant the literal fourth word, say so before T4.
   ⚠️ **T4 has now shipped it, and it shipped with a second departure this item did not
   anticipate: the producer is a KEYWORD, so the line is `viewport left 3d producer ascent`.**
   §1.8's grammar tags optional arguments by name at all four doors — `console stack … region
   <word>` is the same shape, settled in #98 Tier C — and a bare third word would have made the
   typed line and the sidecar line disagree. Both departures are one decision each to reverse and
   neither is load-bearing on anything below it; §4.2's banner has the detail.
4. **Full screen may be a handoff rather than a grow** (§6). Ascent's own window, over the console,
   using the host it already has — which buys the flight feel outright and costs the animated
   transition. The alternative is the portal's unbuilt full-screen rectangle plus whatever §4.4
   measures.

✏️ **And the thing this document originally declined to answer, it now can.** §4.4's numbers 1
and 3 were measured on the 5090 the day this landed: the copy is affordable at region size (2.6 %
of a frame at 1280×720) and still affordable at the full pane (8–9 % at 1440p), so **mechanism B**
is the one to build, with a preallocated ring. What is still open is **number 2** — how *stale* the
painted frame is across two processes — which cannot be taken until there are two processes, and
which is the number §6's full-screen handoff exists to sidestep.
