# Organon — Product Requirements Document (PRD)

> **Status: DRAFT v1.0 (2026-08-21).** The **product definition for Organon** — the north star
> that sits *above* the issues and determines what the product is. Meant to be stable: we iterate
> to nail it, then it guides the build.
>
> 🚨 **This supersedes `doc/organon_mind_prd.md`, which is absorbed here.** Under
> `doc/organon_is_the_product.md` there is one product, so there is one product definition. Three
> PRDs for three products was the same category error as three binaries, made one level up where
> it is more expensive: a PRD is explicitly designed to be handed to a fresh session with the
> instruction *"implement this"*, so two peer PRDs mean the next session correctly implements a
> separate product from an authoritative document. Mind's content survives in full — §6.2, §8's
> Mind requirements, §11's settled decisions and §13's glossary are its, largely verbatim. What
> does not survive is its claim to be a separate product with its own posture.
>
> ⚠️ **The Organon Console PRD (v3.2) is in the private annex and is not absorbed here**, because
> it does not travel with this tree. It needs the same treatment on the same grounds.
>
> **What this doc is, and how it relates to the others.**
>
> | Doc | Owns |
> |---|---|
> | **This PRD** | *What the product is* — vision, audience, principles, the experience, requirements. Above the issues |
> | `doc/organon_is_the_product.md` | The **decision** this PRD is written from, and its sequencing |
> | `doc/organon_modules_plan.md` | **Extension** — the three levels, the units of extension, trust and distribution. §7 here points at it and deliberately does not restate it |
> | `ARCHITECTURE.md`, `doc/arch/*` | The **living state** of the engine — what exists now |
> | `CONSOLE_ARCHITECTURE.md`, `MIND_ARCHITECTURE.md` | The living state of two layouts' machinery, pending the restructure in **#111** |
> | `CONTRIBUTING.md` | The process — scoping, tiers, review, the verification bar |
>
> 🚨 **What this PRD does NOT decide**, because `doc/organon_is_the_product.md` §6 reserves them:
> the name of the collapsed application's default layout, whether *Console* or *Mind* survive as
> layout names, whether Mind's visual behaviours are layout state or something narrower, and
> whether the plugin eventually hosts a layout inside its editor window. Where this document
> needs to refer to those arrangements it uses today's names as **descriptions, not decisions**.
>
> ⚠️ **This is a product definition, not a status report.** Where a property is already enforced
> in code, it says so and names the enforcement, because an unenforced principle and an enforced
> one are different kinds of claim. Everything else is direction. §9 is the verification bar and
> §12 is the honest state of play.

---

## 1. Vision

### 1.1 What Organon is — the canonical wording

📌 **This section is the single source for the three lengths.** The README, the sites, `CLAUDE.md`
and any deck quote it rather than re-authoring it. The identity claim is currently spelled five
different ways across the tree (`README.md`, `doc/equations_into_light.md`,
`doc/how_organon_works.md` twice, and `CLAUDE.md`'s naming section), which is how it came to be
stale in five places at once. One source, several front doors — the same discipline
`registry.rs` already applies to the command vocabulary, applied to prose.

**One or two sentences:**

> **Organon is one native GPU application whose identity is data: you divide the window into
> regions, declare what each holds, and save the arrangement under a name — and no arrangement is
> valid without a live agent in it, taught by loadable skills to operate the application it is
> running inside.** It is agent-first, extensible from within, and the generative-math visualizer
> it grew out of is one of the things an arrangement can hold rather than what Organon is.

**One paragraph:**

> Organon is one native application built on the observation that an "app" is an arrangement, and
> that every arrangement needs someone to talk to. It divides its single pane into named
> regions — an agent conversation, a column of instrument panels, a live 3D viewport — and saves
> the result under a name; that named arrangement is what someone means when they say which
> program they are running. An agent harness is not one of the things you may put in a layout, it
> is the thing a layout cannot legally omit: the region model refuses any command that would
> leave no agent, and a saved layout that names none is rejected on load. The agent is a
> first-class operator rather than a chat window bolted on — it reaches the same command
> vocabulary a human types and a script calls, it is given a working directory chosen by stated
> rules so its skills and project instructions actually load, and a skill can teach it a new part
> of the application without recompiling anything. Underneath sits an engine of permissively
> licensed crates that a project outside the repository can link and build on, which is the
> second half of the same idea: the visualizer is a thing built on Organon that happens to be
> built in, and the next ones can be built by the agents living inside it.

**Two paragraphs:**

> Organon is one native application whose identity is assembled at runtime rather than compiled
> in. It divides its pane into up to six addressable regions, each declaring what it holds, and an
> arrangement of regions can be given a name and written to disk — so what used to be three
> separate programs (a music-synced visualizer of generative math, a mode that loads a `.gguf` and
> draws the model's real topology lit up as it reasons, a workstation for operating AI agents) are
> three layouts of one program. Every one of them contains a working agent, because that is
> enforced rather than encouraged: a command whose result would leave no agent region is refused,
> and a saved layout naming none does not load. The one thing that deliberately cannot be a layout
> is the VST3/CLAP plugin — inside a DAW a host owns the window, the audio thread has hard
> real-time constraints, and the plugin's identity appears in saved sessions that outlive any
> decision we make. Different lifetime, different artifact.
>
> What makes it agent-first is not that an agent is present but that it is a peer operator. There
> is one command table with several front doors — a CLI, an agent's tool call, a slash command
> typed into the composer, a two-word control inside the region it acts on — and tests assert that
> a verb cannot exist for an agent and not for the person at the keyboard. The harness itself is
> data: which agent CLI a tab runs, how to detect it on this machine, where to get it, and what
> directory to start it in are a registry entry a user file can override, so the application is
> Pi-first and Pi-not-required. Skills extend the agent's competence at the application without
> touching the binary: a skill already teaches it to drive Organon through its CLI on a
> see → act → see loop where a rendered frame is its eyes, and to change the console it is running
> inside. Which is the direction of travel — Organon dispatching agents and agent teams to build
> other products and to build Organon itself, from within Organon. Beneath all of it is an engine
> of permissively licensed crates that a project outside the repository can link, so what is built
> from inside does not have to stay inside.

### 1.2 🚨 The correction this document exists to make

**Organon is not the visualizer.** The music-synced generative-math visualizer it began as — the
generators, the PBR/HDR/ray-traced render stack, the audio-driven motion — is **one thing Organon
hosts**. It is built in, and conceptually it is a module like any other. Organon is the renderer,
the layout system, the command vocabulary, the agent surface and the module system, and it could
host an unlimited number of things with no relationship to that original visualizer, all using the
same renderer and the same layout machinery.

📌 **The test for any description of this product: would it still be true if the visualizer were
deleted?** If not, it has described an instance as an identity.

⚠️ **The tree does not currently pass that test, and saying so is the point of stating it.** The
layout, region, command, theme and agent machinery would all survive the deletion. The *content*
vocabulary would not: `3d` would lose its only producer, and `panel` holds Organon's own editor
panels. That is not a reason to describe Organon as the visualizer. It is the reason not to claim
a plurality of hosted things in the present tense — there is exactly one, and §12 says so.

**The code has already made the demotion, which is the strongest available evidence the framing is
right rather than aspirational.** `Content::only_one_because` attributes the one-live-viewport
limit to *that renderer's* shared frame index and TAA jitter phase, explicitly not to viewports
being singular; and the region content word is **`3d`** rather than `world` precisely so the
vocabulary would not bake in today's only answer — *"the generalized 3D viewport is what is being
built, and Organon is a particular application of it."*

### 1.3 The arc

Organon began as a faithful reimplementation of *Organic Math*, a cube-field visualizer, and grew
far past it: 27 generators, a full PBR/HDR/ray-traced render stack, beat- and audio-driven motion.
It then grew a mode that loads a `.gguf` and draws the model's true wiring, read from the file,
lit up while it runs. Then a console for operating AI agents natively, GPU-composited. Then — on
seeing a panel stack and a live 3D viewport in one window, assembled by typing — the recognition
that these were never separate products:

> *"it's not Organon Console. This is Organon. This is the one thing that it is. It starts with
> this console capability, but we can use it to build outward and build up any sort of application
> that we want."* — James, 2026-08-20

A **layout** is the unit of identity. Divide the pane into regions, declare what each region holds,
save it under a name. An "app" is a layout.

### 1.4 The trajectory — four stages

The north star includes all four, and the ordering is a dependency chain rather than a roadmap
preference.

1. **Arrange** — regions, content, saved layouts, a vocabulary that reaches all of it. *Largely
   built.*
2. **Operate** — the resident agent as a peer operator of everything in stage 1: same verbs, real
   permissions, honest arbitration when a hand and an agent both act. *Largely built.*
3. **Extend from within** — skills that teach the agent new parts of the application, and the
   application changed from inside itself. *Partly built; the mechanism that makes it possible is,
   the practice is beginning.*
4. **Dispatch** — Organon as the place agents and agent teams are launched, coordinated and
   watched, building other products and building Organon itself. *Direction. The console renders
   a coordinator's dispatched agents well; Organon is not yet the dispatcher.*

⚠️ **Stage 4 is the claim most likely to be read as shipped, and it is not.** §12 carries the
honest line, and any public copy must inherit it.

## 2. The product in one screen

A dark, dense, legible native application in the spirit of a scientific instrument. One window,
one pane, divided into up to six regions on a three-column by two-row grid — every region
addressable by a word a person says (`left`, `topcenter`, `bottomright`, and each with a short
form). A region holds an agent conversation, a scrolling column of instrument panels, a live 3D
viewport, or a piece of media. At least one region always holds an agent. The arrangement has a
name and a file, and loading one either applies whole or refuses with a sentence.

You type to it — in a composer, at a shell, or through an agent that types the same words you
would. You point at it — a drag on the viewport outranks whatever the agent was doing with the
camera. And when it needs permission, it asks in a card in the flow rather than failing.

## 3. Who it's for

**Dual, and honesty is the bridge.** The same faithfulness that lets a practitioner trust what
they see is what lets a newcomer be genuinely rather than misleadingly impressed. The product must
never buy accessibility with a lie, nor rigor with opacity.

**Design consequence: accessible defaults, depth on demand.** A first-time user gets a legible
default arrangement with plain-language labels; a practitioner opens the numeric readouts, verifies
counts against the source, exports the data, and drills in. Neither is a separate "mode" — one
product with progressive disclosure.

**Representative users**

- *The builder.* Runs an agent in one region, the thing being built in another, and the controls
  in a third — and increasingly builds Organon this way, inside Organon.
- *The interpretability practitioner.* Loads a MoE model, watches which experts fire per token,
  verifies routing and layer/head counts against the config, exports per-token entropy.
- *The performer.* Drives the visualizer from a DAW or standalone, with the visual owning a
  projector.
- *The curious viewer.* Watches a model hesitate on a hard word and cruise through easy text —
  and understands, correctly, what the glow means.
- *The educator.* Shows attention and the residual stream as real, to-scale structure rather than
  a cartoon.

## 4. Design principles (non-negotiable)

1. 🚨 **Agent-first, and it is an invariant rather than a feature.** Every arrangement contains a
   working agent. This is enforced, not encouraged: any command whose *result* would leave no
   `agent` region is refused — `full off`, `full panel` and `left panel` from a default console
   are the same eviction by different names, closed by one invariant checked on the resulting
   layout — and *"nothing holding `agent`"* is one of the eight refusals a saved layout is
   rejected on. **A console with no agent region is a window with nothing to talk to**, and the
   verb that would fix it is typed at an agent.
2. 🚨 **One vocabulary, several front doors.** A verb exists for everyone or for no one. The CLI,
   an agent's tool call, a human's slash command and an in-region control converge on one
   dispatch, and two tests hold it there — that every console verb is typeable as a slash command,
   and that all surfaces of a verb produce the same operation value. ⚠️ **This is not tidiness. It
   was measured**: a command typed as prose cost thirteen seconds and a chunk of context passing
   through inference, a tool search, and an approval card asking a person to approve their own
   command.
3. **The hand outranks the agent, and it is enforced rather than remembered.** Where a person and
   an agent can both act on the same thing, the person's action wins by construction, and the
   surface says so when it moves something nobody is looking at.
4. **An agent's power is bounded by approval, not by good behaviour.** Every tool an agent
   calls — shell commands included — passes one permission card, answered in-process so the hook
   is a direct call into the state the UI is already drawing. A skill can teach an agent what to
   want; it cannot get it past the card.
5. **Scientific honesty, three commitments.** (a) Structure is read from the source — counts and
   wiring, never invented. (b) A live signal comes from the real process, not a decorative
   animation. (c) Every projection is labeled *as* a projection. A **provenance marker**
   (*measured / derived / proxy / projection*) is attached to every displayed quantity, and a
   proxy says so where a person is looking, not only in a document.
6. **Nothing on screen may be silent about being empty or refused.** A region that draws nothing
   is indistinguishable from one that is broken, so vacancy is a sentence with the word a person
   would type to fill it; a refusal names the obstacle and the command that clears it; an empty
   ring carries the reason it is empty, enforced by a type that cannot be constructed without one.
7. **A change either applies whole or does not apply.** A saved layout that cannot be drawn says
   so and leaves the current one standing — never half-applies — and that is a property of the
   signature rather than discipline at the call site.
8. **New capability defaults to inert.** Off, or set to a value that reproduces today's behaviour.
   This is what lets large features land one tier at a time over weeks.
9. **Linked views.** One subject, many synchronized views: selecting or hovering an element in any
   region highlights it everywhere it appears. This is what makes an arrangement an instrument
   rather than several windows near each other.
10. **Progressive disclosure.** Legible for a newcomer by default; every layer of depth one step
    away. No dumbing-down, no wall of numbers on first run.
11. **Reproducibility.** A run is defined by its inputs and can be re-run and exported. Analysis
    you cannot reproduce or export is not science.
12. **Performance is a feature.** Rendering, inference and composition share one GPU; the
    application stays responsive, throttles gracefully, and never blocks the UI on a frame,
    a token or a tool call.
13. **Interventions are honest and reversible.** Where the product acts on its subject rather than
    reading it, every intervention is explicit, logged and reversible, and the views show what it
    changed. Acting never compromises the honesty of what is shown.
14. **No anthropomorphizing beyond the honest signal.** The product shows effort, uncertainty,
    structure and real internals. It is not a character and makes no claim to feelings or intent.
    ⚠️ **This is not in tension with principle 1** — see §10's non-goal on the distinction between
    an agent as *operator* and an agent as *character*.

## 5. The experience

### 5.1 A layout is the unit of identity

Divide the pane into regions, declare what each holds, save it under a name. That named
arrangement is what a person means when they say which program they are running — so what is
written to disk is not a window position, it is a product identity.

**The region model, and what its shape buys.** Twelve region words over a 3×2 grid, **derived as a
cross product of column-spans and row-spans rather than curated** — the discriminator being that a
region must be a word a person says, which is why the two-column runs have no names. A region is a
set of cells; two may coexist **iff their cell sets are disjoint**, so there is no layout
arithmetic to get wrong. Assignment onto something already held is resolved by containment: the
contained region gives up its place and the displacement is reported; a *partial* overlap is
refused by name, quoting both, because it has no unambiguous thing to take away.

⚠️ **Flat, never nested, and the reason is the vocabulary rather than the geometry.** A tree is the
obvious model and it is the wrong one here, because **a tree has no names**: `/viewport left agent`
is a sentence a person says and an agent writes, while the same intent in a tree is a path through
splits that must already exist — and the command lane is fire-and-forget with no return path, so a
caller cannot ask what the tree looks like in order to describe a place in it. What that costs is
stated rather than hidden: no uneven splits and no dragging a divider. Those are a later tier's.

**Loading a layout is a transaction** (principle 7), validated whole against eight refusals before
a single assignment applies.

### 5.2 The agent is a peer operator

Not a chat window bolted to an application, and the difference is visible in four places:

- **It shares the vocabulary** (principle 2) rather than being the only thing that can speak.
- **It can read, not only write.** Reads that a fire-and-forget lane cannot answer are served
  in-process, so an agent can ask what the camera is doing or what layouts exist and get an
  answer — reported as separate facts rather than folded together, because an agent acts on them
  differently.
- **It is given somewhere to stand.** Which directory an agent starts in is resolved by four
  stated rules with a unit test each — an explicit per-tab answer, a per-launch environment
  answer, the nearest project root at or above the launch directory, then the launch directory —
  and the resolution is *always* reported, along with which rule chose it and a warning when the
  directory satisfies no project marker at all. ⚠️ **This exists because the paradigm was failing
  silently at its first step**: an agent asked to use a skill answered *"Unknown skill"*, because
  it had been started in the application's own directory and could see no project instructions,
  and **nothing anywhere said so**. The only symptom is an agent that seems oddly ignorant.
- **It is arbitrated, not trusted** (principles 3 and 4).

**The harness is data, and the product is harness-pluggable.** Identity, launch command, how to
detect it on this machine, where to obtain it, what directory to start it in, and whether it runs
inside WSL are fields of a registry entry — built-ins seeded in code, a user's file merged over
them by id, with serde defaults and unknown fields tolerated. **Pi-first, Pi-not-required**; the
plain login shell is the entry every registry carries.

**Agent teams are a first-class thing to watch.** A subagent is not a turn — it is *something a
tool call is doing* — so its activity folds onto the card that spawned it rather than acquiring a
place in the flow of its own, with depth flattened to one and recorded, because cards inside cards
have no bottom. ⚠️ **And what such a card honestly shows is that an agent is running, which tool
spawned it, and what it did — never a live feed**, because token-level deltas from a subagent are
not forwarded at all. A canary counter guards that measurement rather than a comment asserting it.

### 5.3 Skills — extending the agent without touching the binary

A **skill** is instructions: text that teaches the resident agent a part of the application, loaded
from the project the agent is standing in. It is the unit that makes "extensible from within" real
between recompiles, and this repository already treats one as a durable document with a
same-change obligation — the `organon-cli` skill is in the hook table, accountable to the CLI's own
source files, exactly like an architecture doc.

What a good one encodes is a *loop*, not an API listing: **see → act → see** — read the live state,
make a change, then look again, with a rendered frame as the agent's eyes and never an assumption
that a change did what was intended. And the live catalog is the authority over the skill's own
prose, so the skill teaches the grammar and points at the tool for the vocabulary.

📌 **The self-modification case is already written down**: an agent running in a tab *"does live
inside it, and can change it"*, with the living architecture doc named as the authority to consult
before the tree.

### 5.4 Visual identity

Warm and scientific, deliberately **not** the overdone cyan/magenta cyberpunk. A warm graphite
shell (near-black with a hint of brown, never blue-black), charcoal panels, taupe hairlines,
bone-white and brushed-titanium type. It should read as a premium lab instrument, not a sci-fi HUD.

Data speaks in a perceptually-uniform scientific colormap — the magma/inferno family — for
continuous scalars, which is what real tooling uses and so is instantly credible as well as
beautiful. Discrete elements use a muted earthy categorical set; selection is a single warm-white
or gold pop. Every colour is one that gets implemented as a real colormap or LUT rather than
described.

⚠️ **The theme is a value the application owns**, editable live while looking at what it changes,
and the editor may not assign the palette — a seam kept deliberately.

## 6. What a layout can hold

📌 **A content kind is a *producer*, and the boundary is deliberately small: a producer yields a
texture the console can sample, at a size the console asks for.** Not "a function that draws into
our device" — the in-process case satisfies it trivially and an out-of-process one satisfies it
later by importing a shared texture, **without restructuring the region model**. 🚨 There are no
speculative arms behind that boundary, and that is the point: the generality is in where the
boundary is drawn, not in machinery behind it.

### 6.1 The generative visualizer — built in, conceptually a module

The original subject and still the only producer of a 3D region: 27 generators, surfaces and
materials over a PBR/HDR/ray-traced stack with 50+ shaders, driven by MIDI, tempo and audio. Its
algorithm — rotate-then-translate composition and an accumulating fourth strand — is the source of
truth in one pure, unit-tested module.

⚠️ **Its limits are attributed to it, not generalized into the platform.** One live viewport is
*this renderer's* constraint and says so in the refusal a person reads. A second producer changes
one function and the site that renders — not the region model, not the layout, not the vocabulary.

### 6.2 The Mind layout — a reverse-engineering workbench for a running model

**Point it at a model file, give it a prompt, and see the model's true architecture and its live
internal operation rendered as something you can both *feel* and *measure*.** It is beautiful
because it is faithful: every shape is read from the real model, every live signal comes from the
real forward pass, and every projection is labeled as the shadow it is.

🚨 **The category is a reverse-engineering workbench whose interface happens to be a rendering
instrument** — not "a visualizer that also has tools." That reframe answers the sceptic's *what is
it for*, and it governs feature choice: if a capability would belong in a reverse-engineering tool,
it belongs here; if it is decoration, it does not.

**This is the field's own framing, not an imported metaphor.** Mechanistic interpretability defines
itself as reverse-engineering neural networks into human-understandable algorithms, and the
comparison to decompiling a stripped binary appears in its own self-description. **The IDA Pro
workflow is the closest product analogue**, and five parallels are load-bearing:

1. **An artifact without source.** A stripped binary and a `.gguf` are both "the thing that runs"
   with human-legible intent removed. Neither was authored to be read.
2. 🚨 **The annotation database is the real product.** IDA's disassembly regenerates in seconds;
   what analysts guard, version and trade is the `.idb`. Our equivalent is the **feature-label
   corpus** — and it is a first-class, versioned, shareable artifact, not a config file. Names
   arrive two ways and the difference matters: **imported** (someone else's inferred claim,
   credited and versioned) or **established by us** by contrast-pair experiment where a release
   ships weights and no labels at all. **Neither is `measured`.**
3. **Static and dynamic, which we already have.** The specimen read from the file at rest, and the
   live activation ring. The split was arrived at independently; the analogy confirms it.
4. **Cross-references.** *"Show me everywhere this is touched"* is IDA's most-used view, the
   equivalent writes itself, and we do not have it. A strong near-term lens candidate.
5. **BinDiff.** Structurally comparing two binaries maps onto comparing a model with its own
   quantization.

**What mech interp supplies that IDA does not: the unit of analysis.** A transformer's weights are
fixed and every prompt runs the same matmuls, so there is no control flow to recover — but that
does not mean there is no graph. The field's units are the **feature** (a direction in activation
space, not a neuron), **superposition** (more features than dimensions, so most neurons are
polysemantic — which is *why* reading raw activations fails), the **circuit** (a subgraph
implementing a legible algorithm — the call-graph analogue), and the **attribution graph** (the
per-prompt causal graph of which features influenced which). 📌 **The attribution graph is the
answer to "what varies per prompt if the weights are fixed"** — the causally active subgraph
varies, and that is the renderable object. Published graphs for open-weight models are obtainable
without training anything, existing frontends are 2-D web UIs, and node-link graphs hairball past a
few hundred nodes. **Rendering large structured fields legibly at scale is Organon's founding
competence, so the differentiator is precisely there.**

🚨 **Where the analogy breaks — state it before anyone else does.** Disassembly is lossless,
deterministic and unique; feature decomposition is none of the three. There is no source that ever
existed: a model was fit, not written, so our features are a description we impose rather than the
recovery of something discarded, and that gap never closes. Instructions have crisp semantics;
features have statistical tendencies. And the field itself has not achieved comprehensive reverse
engineering of production models. **We inherit that caution: a label asserting intent or reasoning
is a contested claim and must be marked as one, never rendered as measurement.**

**Terminology (settled).** **Reverse engineering** for the activity — it promises effort, not
success. **Feature / circuit / superposition / attribution graph** for the objects, matching the
literature. **Debugger** is fair for the live half. 🚨 **Never "disassembler"** in shipped copy or
UI: it over-claims in exactly the direction principle 5 exists to prevent.

**The lenses.** Each viewport pane hosts one lens; every lens carries provenance markers.

| Lens | Shows | Real / projected |
|---|---|---|
| **Specimen** | The model's true architecture: residual axle, per-layer head rings, MLP rail; MoE expert fans; GQA/SWA structure; recurrent motifs | Real (from file) |
| **Embedding galaxy** | The vocabulary embedding space projected to 3-D; semantic clusters | Real geometry, **projected** |
| **Residual trajectory** | The token's live path up the stack through the galaxy | Real activations, **projected** |
| **Logit lens** | Per-layer "what it would predict if it stopped here"; the answer resolving | Real (unembedding) |
| **Geometry scalars** | Exact per-layer measures: rotation angle, residual↔token alignment, norm growth | Real, exact (no projection) |
| **Raw component band** | The literal activation vector as a luminous band | Real, literal |
| **Attention river** | Tokens as nodes, causal attention edges, live | Real (attention rows) |
| **Effort glow** | The stack lit by uncertainty + confidence | 🚨 **Proxy** (labeled), until the real tap is confirmed |

**The analytics.** Oscilloscope-grade live telemetry off the same run: tokens/sec, logit-entropy
across the generation, top-k for the current token, a per-layer activity heat-strip, per-head
summaries, KV-cache and memory readout, a logit-lens table. All linked, all exportable. This is the
surface that most makes it a measurement tool.

🚨 **The standing honesty gap, recorded because a HUD asserting "activity" is a stronger claim than
a dashboard about provenance.** The per-layer generation glow is a *labeled proxy* — entropy plus
confidence, not real activations. The real tap exists and prints which it got on the first token;
**as of this writing nobody has run it.** Running it is a prerequisite for any feature whose
headline is model activity, not a nicety: if it reports proxy, that feature's claim is false as
designed.

### 6.3 The agent conversation

A native rendering of an agent's structured event stream — not a terminal grid pretending to be
one. The transcript is a log, and a **control is not a log entry**: panels go in a column that
scrolls independently, because a control that scrolls away mid-drag was never usable. Success is
quiet; only a departure from normal takes weight.

### 6.4 Instrument panels

A region can hold a scrolling column of the application's own control panels, added and removed by
verb, sized to the region and nothing else — so a small corner scrolls twenty panels exactly as a
full-height column does. That property is what makes assigning a small region worth doing at all.

### 6.5 Media, and what is not yet a content kind

A picture or a document from a path a person typed. ⚠️ **Everything else is not a content kind
yet** — including the Mind dashboard as a region, which is filed and gated on the honesty
prerequisite above.

## 7. Extension — how something arrives that Organon did not write

📌 **`doc/organon_modules_plan.md` owns this in full; this section states only what the product
definition depends on.** The word **plugin** is taken here — it means Organon *being* a VST3/CLAP
inside a DAW — so an extension is never called a plugin. It is a **module**.

**Three levels, with a mechanical test.** Core is a change to Organon itself; a module is
capability built on Organon's public surface that Organon does not want to own; an application is
a thing made with a module. The test is not taste: **does the change need to touch the parameter
chain?** That chain crosses three crates by construction and accounts for the overwhelming majority
of cross-crate churn, so anything that must touch it stays in core — otherwise every parameter
addition becomes a two-repository dance. That question is why a game engine can leave and a
generator cannot.

**Three units of extension, and they have three different trust profiles.**

| | **Linked module** | **Hosted module** | **Skill** |
|---|---|---|---|
| What it is | a crate, a cargo dependency | a separate process, composited | instructions the resident agent loads |
| Boundary | 🚨 **none** — your address space, filesystem, GPU | **the process** | none of its own — it steers an agent that already holds your permissions |
| The control | source audit, and it is the only one | **the protocol is the permission set** | 🚨 **the approval** |
| Source | **required** | optional — and that is a feature, not a concession | it *is* source |
| Review target | `build.rs`, proc macros, the transitive tree — not "the module" | the verbs the protocol grants | what it tells the agent to want |

🚨 **So "how far do I trust this?" and "which kind is this?" are one question asked twice.** A trust
tier does not select a policy applied to a module; it selects the module's *kind*. Core is linked
by definition; someone whose code you would run without reading is defensibly linked, and the
honest framing is that you are trusting a person rather than a mechanism; everything further out is
hosted — not because those people are suspect, but because a boundary you can point at is the only
thing that survives one of them having a bad day. ⚠️ **The failure mode to design against is
social**: promoting to linked must be visibly different from installing, and must say what it is
granting, or *"I know them"* drifts into *"full address-space access"* until the tiers mean nothing.

**Distribution is git, and the unit is a commit rather than a repo** — a repo says where the bytes
live, a commit says which bytes, and tags move, branches move, force-push rewrites history. Two
consequences: the registry question largely dissolves, because a URL is an identity and a commit
hash is a content address; and we get **the affordance no package manager ships** — trust is
renewed at every update, and `git diff <last-trusted>..<candidate>` answers *"this changed fourteen
files since the commit you trusted; here they are."* ⚠️ **Visibility is not review** — every major
package ecosystem is completely source-visible and compromised constantly — so source distribution
buys the *possibility* of controls and the diff is the cheapest one to actually build. ⚠️ **And git
does not supply revocation**: whatever the trust model becomes, that part must be designed, under
the constraint that a layout referencing a module you have stopped trusting **must not fail to
open**.

⚠️ **The tempting third module kind — `dlopen` a Rust cdylib — is ruled out and stays ruled out.**
No stable ABI; it fails at runtime, in a graphics driver, on someone else's machine.

## 8. Functional requirements

Grouped and testable. **MUST** = the product is not itself without it; **SHOULD** = wanted, and
schedulable.

📌 **Mind's FR numbers are preserved verbatim from the absorbed PRD** so existing issue references
still resolve. Platform-level requirements introduced by this document carry letter prefixes.

**Layout and regions**

- FR-L1 (MUST) Divide the pane into named regions; every region addressable by a word a person
  says, with a short form accepted at every front door and listed at none.
- FR-L2 (MUST) Declare a region's content by verb; refuse a partial overlap by name, resolve a
  containment by displacement and report it.
- FR-L3 (MUST) Save an arrangement under a name, load it, delete it, list what exists.
- FR-L4 (MUST) A load applies whole or refuses with one sentence naming the obstacle.
- FR-L5 (MUST) Report every *unassigned* region with its rectangle, coalesced largest-first, so
  vacancy is a sentence rather than a blank.
- FR-L6 (MUST) Refuse any command whose result would leave no agent region.
- FR-L7 (SHOULD) Record a layout's panel-column composition, not only that a region holds panels.
- FR-L8 (later) Uneven splits and a draggable divider.

**The agent**

- FR-A1 (MUST) At least one agent harness runnable in a region, chosen from a registry that is
  data, with built-ins in code and a user's file merged over them by id.
- FR-A2 (MUST) Detect which harnesses are present on this machine and say where to get the ones
  that are not.
- FR-A3 (MUST) Resolve the agent's working directory by stated rules, report the answer and which
  rule chose it *unconditionally*, and warn when it satisfies no project marker.
- FR-A4 (MUST) Answer every tool-permission request in one card in the flow, covering every tool
  the agent calls including shell commands, with allow / allow-and-remember / allow-everything /
  deny and a deadline that does not silently expire the request.
- FR-A5 (MUST) Expose the console's verbs to the agent from the same table the human's commands
  are generated from.
- FR-A6 (MUST) Serve reads an agent needs that a fire-and-forget lane cannot answer.
- FR-A7 (MUST) A human's direct manipulation outranks an agent's, and the surface says so when it
  moves something nobody is looking at.
- FR-A8 (MUST) Render dispatched subagents on the card that spawned them, flattening depth and
  never implying a live feed where none exists.
- FR-A9 (SHOULD) Load skills from the project the agent is standing in, including a skill that
  teaches it to operate and to modify this application.
- FR-A10 (later) Dispatch and coordinate agent teams *from* Organon rather than rendering a
  coordinator that runs elsewhere.

**Extension**

- FR-X1 (SHOULD) Publish the engine crates so a linked module needs no path or git dependency.
- FR-X2 (SHOULD) A hosted-module protocol whose surface is written down as a permission set before
  its first verb, plus a manifest on the harness-registry pattern.
- FR-X3 (SHOULD) A verb that adds and runs a module.
- FR-X4 (later) Show what changed since the commit you last trusted, at the moment of update.

**Model I/O** *(absorbed, verbatim)*

- FR-1 (MUST) Load a `.gguf`; show parsed architecture (layers, heads, experts, dims, vocab) with provenance.
- FR-2 (MUST) Draw the specimen faithfully for dense models; (SHOULD) MoE + GQA/SWA; (later) recurrent.
- FR-3 (SHOULD) Browse/scan a models directory.

**Inference**

- FR-4 (MUST) Enter a prompt and generate; stream tokens to the log and the viewport.
- FR-5 (MUST) Expose sampling params + seed; a run is reproducible from (model, prompt, params, seed).
- FR-6 (MUST) Per-token entropy, confidence, chosen token and top-k. (MUST) Real per-layer activations via the tap.
- FR-7 (SHOULD) Token-rate cap + graceful GPU throttle; the UI never blocks on a token.

**Viewport and compositor**

- FR-8 (MUST) An in-application viewport rendering the scene.
- FR-9 (MUST) Split into panes; per-pane lens selection. ✏️ **Now a consequence of FR-L1–L4**, not a Mind-only mechanism.
- FR-10 (MUST) Mirror to an external display; toggle on and off.
- FR-11 (SHOULD) Orbit/zoom per pane; pin a pane to a token or step.

**Lenses** — FR-12 (MUST) Specimen + effort glow (proxy, labeled). FR-13 (MUST) Embedding galaxy. FR-14 (SHOULD) Residual trajectory + logit lens + geometry scalars. FR-15 (later) Raw component band + attention river.

**Analytics** — FR-16 (MUST) Live tokens/sec, entropy plot, top-k, per-layer heat-strip. FR-17 (SHOULD) Per-head, KV/memory, logit-lens table.

**Linked selection** — FR-18 (MUST) A shared selection/hover model across all panes and analytics. FR-19 (SHOULD) Pin and compare across tokens.

**Sessions and export** — FR-20 (MUST) Append a log of every run. FR-21 (MUST) Export per-token analytics. FR-22 (SHOULD) Save and load a saved analysis. FR-23 (SHOULD) Snapshot a pane; (later) record a clip.

**Shell** — FR-24 (MUST) The dock shell. ✏️ **Subsumed by regions** (FR-L1). FR-25 (SHOULD) Savable layouts + command palette. ✏️ **Landed, as FR-L3 and the command registry.** FR-26 (MUST) Progressive disclosure.

**Provenance** — FR-27 (MUST) Every displayed quantity carries a provenance marker.

**Platform** — FR-28 (MUST) macOS / Apple Silicon. FR-29 (SHOULD) Stay portable; Windows is the anticipated first port. ✏️ **Overtaken by events** — the console and the visualizer already build and ship on Windows and Linux, and CI covers all three.

**Interventions** — FR-30 Apply steering vectors, activation edits or ablation to the live forward pass; each explicit and toggleable. FR-31 Every intervention is part of the run definition and the views show before and after.

**Model profile** — FR-32 Derive and display resource-aware inference geometry (roofline, memory traffic, KV-cache cost, quantization tradeoffs) from the file header plus a hardware profile, with provenance. No forward pass required.

## 9. Verification bar

**Without a GPU** (CI, most cloud sessions): `cargo build --release` and `cargo test --workspace`,
where `--workspace` is load-bearing rather than tidy — a bare `cargo test` runs the root package
only. Shaders are parsed and validated offline, which catches binding, type and uniformity errors
with no device. ⚠️ **Default-off features are not compiled by that**, so a change touching shared
ground must build the other configurations too, or a broken one lands green.

**That is the ceiling.** It does not catch pipeline or layout mismatches, runtime GPU behaviour,
UI layout, or the actual look. 🚨 **A finished PR from such a session is "green and ready to
deploy", never "verified working" — say it that way.**

**With a GPU**, the bar is higher and it is on you: deploy, drive the application yourself — the
CLI first — and report what you *saw*, with evidence. The frame harness turns frames into pass/fail
against committed goldens.

⚠️ **And some things are only ever answered by a hand and a screen**: whether a three-column
arrangement reads as an instrument or as a cramped imitation of one, whether a control column at a
given width is usable, whether an agent's card density is calm or busy. Those go in an honesty
ledger rather than being asserted.

## 10. Non-goals

- 🚨 **The plugin is not a layout, and never will be.** A VST3/CLAP inside a DAW has a host-owned
  window, a host-controlled lifetime, an audio thread with hard real-time constraints and a
  saved-session identity. **No second plugin class ID, ever** — Mind and the Console are
  standalone-only on purpose, and adding a plugin identity is not a feature, it is a new lifetime
  commitment.
- 🚨 **Not a character or a persona.** ⚠️ **This is the non-goal most likely to be read as reversed
  by principle 1, and it is not.** The distinction is between an agent as **operator** — it holds
  the same verbs a person does, bounded by approval, outranked by a hand — and an agent as
  **character**: a persona, a voice, a claim to intent, a thing that "plays" Organon. The first is
  the product. The second stays out. 📌 This is consistent with what was already settled: building
  an agent *into* Organon was superseded by giving an external agent a plain local command surface
  to drive, which is precisely the shape principle 2 generalizes.
- **Not a `dlopen` plugin host** (§7).
- **Not a monorepo for everything built on Organon.** A game, a module, an application built with
  one — each lives in its own repository. A visualizer whose tree contains four games is not a
  visualizer with an ecosystem.
- **No commercial layer yet** — accounts, licensing and pricing are deferred.
- **Not a training tool**, though model-operations are an explicit later stage rather than a
  permanent exclusion.
- **No extended-desktop / multi-region external output** — the external display is a mirror.

## 11. Decisions and open questions

**Settled (2026-08-21, this document):**

- **There is one PRD, because there is one product.** `doc/organon_mind_prd.md` is absorbed; the
  Console PRD in the annex needs the same treatment.
- **Agent-first is a product principle, not a feature of one arrangement**, and it is already
  enforced by the last-agent invariant at both the command and the saved-layout door.
- **A skill is a third unit of extension** with its own trust profile, alongside the linked and
  hosted module kinds. `doc/organon_modules_plan.md` §4's two-kind table is amended by §12 there.
- **§1.1 is the canonical wording** and every other surface quotes it.

**Settled (2026-08-20):**

- **Organon is the product; a layout is what used to be an app.** The compile-time edition
  approximates at build time what a layout now expresses at run time — and a runtime answer can be
  switched, saved, shared and extended.
- **The process boundary is the trust boundary**, so a trust tier selects a module's kind.
- **The unit of trust is a commit**, and git is the distribution mechanism.
- ⚠️ **The sequencing is deliberately unexciting**: ratify the words first, finish the region
  tiers, build saved layouts, and only then collapse the editions — because the editions are what
  currently make the three arrangements work, and a rename that outruns the mechanism leaves
  documents describing a thing that does not exist.

**Settled earlier, and carried forward:**

- **The Mind arrangement's category is a reverse-engineering workbench**, with the field's
  terminology fixed and *"disassembler"* refused in shipped copy.
- **The feature-label corpus is a first-class versioned artifact**, not a config file.
- **Attribution-graph import is the bridge to acting on models**: we consume published graphs and
  do not train transcoders — the compute is out of reach and it is not our contribution.
  **Rendering a large graph legibly is.**
- **Real activations are a MUST**, and the proxy is labeled until the tap is confirmed.
- **Interventions are in**, as an explicit later stage, logged and reversible.
- **The agent track was moved out of the Mind arrangement** and answered with a local command
  surface an external agent drives.

**Still open:**

- The **name of the default layout**, and whether *Console* and *Mind* survive as layout names.
- Whether Mind's visual behaviours are **layout state or something narrower**.
- Whether the **plugin eventually hosts a layout** inside its editor window. Not ruled out.
- **What supplies the default IPC namespace** once there is one application — the mechanism that
  lets two sessions coexist is instance identity wearing a product's clothes, and it must be
  redesigned *before* what supplies it today is removed.
- **Who owns a module registry**, and what an entry *asserts* about its module. An index of names
  is not a trust model.
- **Revocation** — the part git does not supply.

## 12. State of play (2026-08-21)

🚨 **Read this before quoting §1.1 anywhere public.** The descriptions are accurate about what
Organon *is*; this section is what it currently *does*.

**Enforced today.** Regions and the twelve-word vocabulary; saved layouts with a transactional
load; the last-agent invariant at both doors; one command table with four front doors and the tests
that hold them to one dispatch; the harness registry; working-directory resolution with its
unconditional report; in-process approvals covering every tool an agent calls; hand-outranks-agent
arbitration; subagent rendering; the panel column; the live theme editor; and a skill that teaches
an agent to operate Organon and to change the console it is running inside.

**Designed and not built.** There is **no dynamic loading anywhere in the tree** — no `dlopen`, no
wasm. The hosted-module protocol and the verb that runs one do not exist. Publishing the engine
crates is blocked on a packaging defect in the host-free crate. The compile-time edition still
exists with three values and three binaries; collapsing it is **#111**, not started. The built-in
layout library **ships empty**, so no arrangement is yet *named* as an app — naming the presets is
a product decision, and a preset nobody has looked at on a screen is worse than none.

**Direction, not mechanism.** Organon dispatching agent teams. What exists is a console that
renders a coordinator's dispatched agents well; Organon is not yet the dispatcher.

**Proven downstream.** The linked path works today: a separate repository consumes four permissively
licensed engine crates pinned by commit, with a licence-graph gate in CI as the enforcement. ⚠️ Two
findings from it belong in any honest description of the linked kind — the consumer adopted
*pin a commit* before this repository did, and **a linked module inherits its host's dependency
versions, not just its crates**, where a mismatch presents as a wrong-signature error rather than a
version error.

## 13. Glossary

- **Layout** — an arrangement of regions with a name. The unit of product identity; an "app".
- **Region** — one of twelve addressable places in the pane, each a set of grid cells.
- **Content kind** — what a region holds. Each is backed by a **producer**.
- **Producer** — anything that yields a texture the application can sample, at a size it asks for.
  The whole boundary, deliberately.
- **Harness** — an agent CLI a region can run. Data, not code: identity, launch, detection, where
  to obtain it, where to start it.
- **Skill** — instructions that teach the resident agent a part of the application, loaded from the
  project it is standing in. The third unit of extension.
- **Module** — capability built on Organon's public surface that Organon does not own. **Linked**
  (a crate, no boundary) or **hosted** (a process, the protocol is the boundary). Never called a
  plugin.
- **Plugin** — Organon *being* a VST3/CLAP inside a DAW. The one thing that cannot be a layout.
- **Lens** — a visualization technique selectable per viewport pane.
- **Linked lenses** — synchronized selection and hover across every pane and the analytics.
- **Specimen** — a model's true architecture drawn from the file.
- **Provenance marker** — *measured / derived / proxy / projection*, attached to every displayed
  quantity.
- **Proxy** — a labeled stand-in for something not yet instrumented.
- **Mirror** — the optional external-display surface fed the in-application viewport's scene.

*Mechanistic-interpretability terms, used as the field uses them:*

- **Reverse engineering** — recovering human-understandable structure from weights and activations.
  Names an effort, not a guaranteed result. **The approved umbrella term; "disassembler" is not.**
- **Feature** — a *direction* in activation space corresponding to an interpretable concept. Not a
  neuron.
- **Superposition** — more features than dimensions, so features overlap and most neurons are
  **polysemantic**. The reason raw activations are unreadable and the SAE step exists.
- **Circuit** — a subgraph of features connected by weights implementing a legible algorithm. The
  call-graph analogue, and the field's own term.
- **Attribution graph** — the per-prompt causal graph of feature influence with non-contributing
  features pruned. What varies per prompt when the weights do not.
- **Feature-label corpus** — the versioned, shareable file of names assigned to feature indices.
  The annotation-database analogue and the asset that compounds. A name is either **imported**
  (someone else's inferred claim, credited and versioned) or **established by us** by contrast-pair
  experiment. **Neither is `measured`**, and the UI must distinguish them, because they have
  different authors and different reliability.
