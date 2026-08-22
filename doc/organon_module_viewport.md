# A hosted module in a viewport — what "approve a repo" means, and the contract it buys

> **Status: DESIGN. Nothing is built, no identifier has been renamed, and no code has been
> touched by this document.** It answers four questions in writing so Organon's side and Ascent's
> side can be refactored against the same contract instead of against each other. §9 is the order
> the work goes in.
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

1. **Accept it and say it.** The approval gesture gates *building*; the process boundary gates
   *running*. For a repo James owns, on James's machine, that is the true state of affairs.
2. **Prebuilt binaries outside the innermost tier** — §11.7's "source optional for hosted"
   arriving as a policy rather than a permission.
3. **Build in a sandbox.** Real, and not now.

📌 The cheap corollary, which should be taken: **`modules.json` records the commit that was
*built*, not the commit that was approved**, and they must be the same value or the record
describes a binary that does not exist. If the tree was dirty at build time, record that rather
than recording a commit as though it named the bytes.

### 3.5 Revocation, and the rule it must satisfy

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

### 4.2 🚨 The vocabulary: `3d ascent`, not a content word called `ascent`

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

- `CONTENT_WORDS` stays a fixed four-word table, and
  `the_word_tables_and_the_resolvers_are_one_vocabulary` keeps passing unchanged.
- Producer names are a **second, dynamic vocabulary** sourced from the approved-module list —
  the shape §1.15 already built and measured for saved-layout names: a `NarrowFn` over a library,
  **cached rather than read straight**, because the candidate walk runs on the draw path and asks
  n + 1 times per call. That measurement is inherited; do not re-learn it.
- An omitted producer means `organon`, so every existing layout, every `layouts.json` written
  before this, and every line in the docs continues to mean exactly what it meant.
- An unapproved or unknown producer is **refused by name**, listing the approved ones. `3d` with a
  typo must never silently fall back to Organon's World — the person would get a picture, and the
  wrong one, which is worse than a refusal.

📌 **And uniqueness becomes the producer's property, which is where its own doc said it belonged.**
`only_one_because` moves from `Content` to the producer: Organon answers with today's reason
(shared `frame_index` and TAA jitter phase), and a hosted module answers `None` unless it has a
reason of its own — a separate process rendering into its own texture has no jitter phase to
trade. **Two Ascent viewports are a thing the architecture permits**; whether anyone wants them is
a different question, and the refusal machinery should not answer it by accident.

### 4.3 What Ascent gives up, and it is exactly three things

Its host is ~600 lines: an `ApplicationHandler`, an adapter/device/surface bring-up, a depth
buffer, a vertex format, one WGSL shader, and a resize path. **It is not thrown away.** §6 is why
it must survive.

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

### 4.4 The frame boundary — two mechanisms, and **nobody has measured either**

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

🚨 **What must be measured before either is chosen, and it needs a GPU and therefore this machine:**

1. Wall-clock cost of a readback of a region-sized texture on the 5090, including the fence wait —
   *on the producer's queue*, because that is the number that becomes stutter in the game.
2. Frames of latency between "the module drew it" and "the console painted it", counted rather
   than reasoned about.
3. What that does to the console's own frame budget, against the 16.7 ms figure §1.15 already
   measures other things against.

📌 **A gives the better answer and B gives an answer this week**, and they are not exclusive: the
contract is *"a producer yields a texture the console can sample, at a size the console asks
for"*, which says nothing about how the bytes travel. **B first, behind the same seam, and A when
the measurement says the copy is what hurts** — with the recorded measurement as the reason,
because a zero-copy path adopted without one is `unsafe` per-backend code bought on a hunch.

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
| **T0** | **Measure the frame boundary** (§4.4's three numbers) on this machine. Nothing that fixes a mechanism starts until these exist. | a GPU |
| **T1** | **Ascent's refactor** — the library owns the device, the pipelines, `render_into(texture, size)`, `step(dt)` and `feed(input)`; `main.rs` keeps the window and the pump, and `fly.ps1` keeps working. | the parallel Ascent session |
| **T2** | **The contract crate** — permissive, console-side, both trees depend on it, `cargo tree` gates both. | T0's answer to *which* mechanism |
| **T3** | **`modules.json`, `organon-module.toml`, and the approve verb** — on the harness precedent, with `layout.rs`'s refusal discipline. Approve, build, record the built commit, diff, revoke. | — |
| **T4** | **The producer qualifier** — `3d <producer>`, the dynamic ring cached per §1.15's measurement, `only_one_because` moved, `engine_plan`'s boolean corrected and tested. | T3, for a producer to name |
| **T5** | **Lifecycle and input** — `Attached`/`Running`, the click latch, the way out, the four failure sentences. | T3, T4 |
| **T6** | **The ladder** — rung 2 is already legal; rung 3 is the handoff, or the portal's full-screen tier, and that is a decision T0 informs. | §6 |

⚠️ **T0 before T2 is not caution, it is the ordering that stops a wire format being designed for a
mechanism that turns out to be the wrong one.**

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
3. **`3d ascent`, not a content word called `ascent`** (§4.2). You said "viewport type `ascent`";
   this reads it as *a viewport whose producer is Ascent* rather than as a fourth kind of region,
   because the word `3d` was chosen specifically so it would not name a renderer. If you meant the
   literal fourth word, that is a real disagreement and it should be settled before T4.
4. **Full screen may be a handoff rather than a grow** (§6). Ascent's own window, over the console,
   using the host it already has — which buys the flight feel outright and costs the animated
   transition. The alternative is the portal's unbuilt full-screen rectangle plus whatever §4.4
   measures.

And one thing nobody can answer from here: **§4.4's three numbers do not exist**, and this
document declines to recommend a frame mechanism without them.
