# Organon as a content framework — the tree, the reconciler, and the renderer beneath

**The question.** Organon began as a physics-and-mathematics visualizer for music and sound,
and its scene model — a *generator* emitting geometry, a *surface* deciding how nodes are
drawn, a *material* deciding how they are shaded, all read from one flat parameter snapshot
— was shaped by that use. It is now going onto a desktop (`doc/organon_desktop_plan.md`) as
a way of **creating real-time 3-D content on the fly**: an agent or a person describes
something that could be visualized, and it is. The ask is whether that needs a scene model
of its own — "an engine, a scene graph, a logical hierarchy of all the capabilities we have,
the way a typical engine has one" — and whether it can be reached by widening what exists.

**The short answer: it needs one, it cannot be reached by widening the visualizer, and the
right reference is not Unity but React.** React is an element tree, a reconciler that diffs
successive trees into a retained host scene, and a host renderer that knows nothing about
the tree; react-three-fiber is exactly that shape over a 3-D scene graph, and it is what
this project's first incarnation was (`ARCHITECTURE.md` §3, the legacy `/src` R3F app).
What Organon has is the host renderer — a PBR/HDR/ray-traced stack that is already a
separate crate with the right boundary — and a platform around it that can host, composite
and operate content. What it does not have is the tree or the reconciler, and the
visualizer's triad is not a substitute for either: it is one **app** the framework would be
able to express, and the only way to keep it is to stop letting it define the framework.

> ⚠️ **Provenance.** Every file, line and count below was read out of this tree on
> 2026-09-03 and is **measured** in this document's sense. Everything from §5 onward is
> **reasoned** — a design, not a description of the tree. §12 is the ledger separating the
> two, and §13 lists what is not known. **Nothing here is built.** `CLAUDE.md`'s product
> count is not changed by this document: it proposes a *crate*, and whether a product grows
> on it is a later decision.

---

## 1. The correction: the visualizer is an app, not the framework

The generator/surface/material triad is a **domain model**. It answers "what does a
mathematical field look like when sound drives it," and for that question it is the right
model: one form, infinitely parameterized, every parameter host-automatable, MIDI-learnable,
beat-pumped. An instrument has a patch, not a scene, and `Shared` is a patch — **177
fixed-size float blocks, ~1,370 parameters, append-only and byte-pinned across the
plugin↔visual boundary** (`ipc.rs`, `LAYOUT_VERSION`, invariant 2).

The tree already shows what happens when general composition is asked of that model. Every
new *kind* of thing on screen has been met by wiring a new sibling into the one frame by
hand, and each one costs a `Shared` block:

| Need | How the tree met it | What it cost |
|---|---|---|
| a second generator alongside the first | the **Scenery layer** — a "second, concurrent generator category" with its own material, surface and palette (`ARCHITECTURE.md` §8) | a `Shared.scenery[16]` block, a second draw with a patched copy of the scene uniforms |
| several objects with their own mesh and material | the **Demo generator** — `math::demo_scene` emits explicit box/sphere/cylinder instances partitioned into per-`(mesh, material)` `DemoBatch`es, plus placeable `DemoLight`s | a hand-authored Rust builder that bypasses the parameter chain entirely |
| lit text | the **glyph ring** — a third concurrent producer with a per-instance emission attribute (`organon#217`) | a ring, a producer binary, a vertex-layout addition |

🚨 **The arity of the scene is being encoded in the IPC layout.** That is the measurement
that decides this document. `organon-render`'s `Surface<'a>` — the struct that says what
the frame draws — has **60 public fields**, and they are a *union of every special case*:
`rt_instances`/`rt_tints`, `swept_verts`/`swept_idx`, `mem_pos`/`mem_norm`/`mem_col`/`mem_idx`,
`arm_caps`, `plexus_node_caps`/`plexus_edge_caps`/`plexus_batches`/four plexus buffers,
`neural_batches`/`neural_capsule`, `creature`. Each producer added its own fields rather than
becoming an instance of a general one, because there is no general one. `World::frame_body`
is **7,471 lines of linear dispatch on one `GeneratorMode`** in a 14,062-line `world.rs`,
and the stateful generators (Boids, FDTD, the bell) keep their state as fields on `World`.
None of that is a defect in the visualizer. It is what "one form, infinitely parameterized"
looks like when it is honest, and it is precisely why it cannot be the foundation for
"anything you could imagine that could be visualized."

---

## 2. The reference model, word by word

React's essence is four things, and react-three-fiber (R3F) shows they survive a 3-D host
unchanged: a React reconciler over three.js's scene graph, where `<mesh>` and
`<pointLight>` are elements and three.js never learns what React is.

| React | What it is | Organon equivalent | Exists today? |
|---|---|---|---|
| **element tree** | a declarative, typed description of what should be on screen | the **scene tree** (§4) | ✗ |
| **reconciler** | diffs tree *N* against tree *N+1* and mutates a retained host scene | the **retained scene + diff** (§5) | ✗ — `World` is immediate-mode over one snapshot per frame |
| **host renderer** (DOM, three.js) | draws the retained scene; knows nothing of the tree | **`organon-render`** (§6) | ✓ with one change: a draw list instead of a 60-field union |
| **components** | the vocabulary of things you can put in the tree | the **element vocabulary** (§4) | partially — generators, demo primitives, glyphs, each as a special case |
| **props** | typed attributes on an element | typed attributes, with `describe` | ✓ the discipline exists for params (`organon describe`, the generated reference) |
| **state / hooks** | what a component reads that changes over time | **signals** — input, clock, audio, beat (§7) | ✓ the sources exist (`organon-module::input`, the audio ring, the PLL beat clock) |
| **JSX** | the authoring syntax | a serialized tree document plus the verb grammar (§8) | ✗ |
| **the ecosystem** | third-party components | **modules** — linked, hosted, skill (`doc/organon_prd.md` §7) | ✓ the trust model; ✗ the payload |

📌 **The framework is the middle two rows.** The tree is the contract and the reconciler is
the machinery; everything else is either already here or is a component written against
the contract. That is also the reason the visualizer can be kept whole: **a generator is a
component**, a `Generator` element that emits instances into its node's frame — the way a
`ParticleSystem` is a component in Unity, a procedural emitter among meshes and lights —
and the beat system becomes one signal source among several rather than the world's clock.

---

## 3. What carries over, and what does not

**Carries over — and is most of the value.**

- **`organon-render`.** Already a separate crate with the right boundary (`organon-core`,
  `wgpu`, `glam`, `bytemuck`, `image`, `half`; no `nih_plug`, no `egui`, no `winit`), already
  taking what a retained scene would hand it: instance matrices, tints, per-instance
  emission, mesh geometry, a material set, lights, an environment, post. The PBR/HDR/RT/IBL
  stack and its 50 naga-validated shaders are the asset.
- **The platform.** The console's regions and content kinds (`kind.rs` — *a name the console
  resolves, never a command, never a path*); the `3d <producer>` qualifier that already lets
  a region name a producer other than the visualizer (`doc/organon_module_viewport.md` §4.2,
  built); the hosted-module frame ring and input ring (`organon-module`); the IPC namespace
  discipline (`ns_file`); the four-front-door command table and the habit of making every
  capability answer `describe`.
- **The verification culture.** The legibility harness (`organon-render/src/legibility.rs`)
  is the first real automated *visual* regression in the tree, and a scene document with a
  fixed grid is exactly the kind of input it can judge.

**Does not carry over — and must not be dragged along.**

- **`Shared` as the world state.** A patch of floats is the right shape for an instrument's
  live lane and the wrong shape for a tree of arbitrary depth. It stays as the live lane
  (§8); it does not become the scene.
- **The parameter chain as the way anything new arrives.** `doc/organon_prd.md` §7's own
  test — *does the change need to touch the parameter chain?* — is the line: a new element
  kind or a new composition must not need it, or every arrangement is a core change.
- **One generator, one surface, one material, one render path per frame.** The frame becomes
  a draw list (§6).
- **The generator as the unit of content.** It becomes one element kind among many.

---

## 4. The element vocabulary — the "logical hierarchy of capabilities"

Every node carries a **name**, a **path** (`menu/ring/item3`), a **transform** relative to
its parent, and **visibility**. Paths are what let a fire-and-forget lane address a node
without a return path — the same reason the region vocabulary is words a person says
(`doc/organon_prd.md` §5.1) — and they are the *only* thing that argument demands of a tree.
A transform hierarchy is otherwise cheap on this renderer: every instance is already a
world-space `Mat4` per frame, so a parent transform is one multiply at lowering time.

| Kind | Element | Yields | Notes |
|---|---|---|---|
| structural | `Scene` | — | the root; holds exactly one `Environment` and one active `Camera` |
| structural | `Group` | — | a transform and children; nothing else |
| geometry | `Mesh` | instances | built-in primitives first (box, sphere, cylinder, plane, capsule — the `DemoMesh` set plus what the impostor paths draw); loaded meshes are a later tier |
| geometry | `Instances` | instances | a `Mesh` repeated over an explicit list of transforms and tints — the shape `lower_strands` already produces |
| geometry | `Text` | instances | a run of glyphs laid out into cells, each a `Mesh` with emission — the glyph ring's model, generalized |
| geometry | `Image` | instances | a textured quad; the console's exhibit path is the precedent |
| geometry | `Generator` | instances **or** a membrane | the visualizer's generators as one element kind: `mode`, a surface, a param delta; emits into its node's frame |
| geometry | `Field` | a raymarch pass | Mandelbulb, KIFS, Minimal, Neural, Lens, Creature — the sibling render paths that draw no nodes. **At most one per frame** (§5) |
| appearance | `Material` | — | the PBR set (albedo, metallic, roughness, IOR, dispersion, SSS, iridescence, clearcoat, emission) and the four material types; referenced by name from geometry |
| appearance | `Environment` | the background passes | sky, IBL/HDR, terrain, ocean, stars, chamber — what `RenderFrame::Background` holds today |
| light | `Light` | a light | point / area / emissive; `DemoLight` is the placeable precedent, the key/fill rig is the global one |
| camera | `Camera` | the view | the rig; an orbit path is an *animator on its transform*, not a property of the world |

📌 **The registry is the hierarchy you asked for, and it has to be one table.** For each
element kind: what it yields (instances / membrane / raymarch / light / camera), whether
hardware ray tracing can see it (triangles enter a BLAS; impostors do not —
`doc/pbr_text_engine.md`), whether it carries simulation state, and which topology it
emits. Today that knowledge lives in match arms and comments; an agent composing on the fly
cannot ask it, and so it asks for a lofted membrane on a Mandelbulb. The table is generated
the way `core_catalog()` is generated from `param_block!` slot lists — one source, no second
hand-maintained copy — and it answers `organon describe <element>` exactly as parameters do.

⚠️ **Deliberately absent from the vocabulary:** per-node scripts, a shader graph, a physics
component. The Field Engine is already a small language for *fields* and the phrase plan for
*moves*; structure is declarative data, the way `omarchy-menu.jsonc` and `layouts.json` are.
Behaviour is signals bound to attributes (§7). If a script tier ever exists it is §7's
execution decision, not a node kind.

---

## 5. The reconciler — where the framework lives

Between the tree and `RenderFrame` sits a **retained scene**: GPU-resident meshes, instance
buffers partitioned into draws, material blocks, the RT acceleration structures, laid-out
text, resolved world transforms. The reconciler's job is to take tree *N+1*, diff it against
tree *N*, and mutate the retained scene **minimally**:

- a changed attribute on a `Material` rewrites one block, not every instance buffer;
- a moved `Group` re-propagates transforms below it and re-uploads the affected instance
  ranges, and — because hardware RT binds a `wgpu::Tlas` over per-mesh BLASes — updates
  instance transforms in the TLAS without rebuilding any BLAS;
- new or removed geometry rebuilds exactly the BLAS it touches;
- a `Text` whose content changed is re-laid-out; one whose colour changed is not.

🚨 **This is the part that does not exist and the part that is the engineering.** `World`
today is immediate-mode: it reads one `Shared` snapshot, regenerates strands, lowers them,
and hands slices to the renderer every frame. That is the right shape for a field that
changes every frame under audio. It is the wrong shape for a menu whose one moving part is
the highlight, and it is exactly the shape a reconciler exists to replace: **knowing what
changed is what makes retained rendering, RT, and path-trace convergence
(`world.rs`'s accumulation restart) tractable.** The pbr-text design already wanted a
content generation counter added to the `pt_content` tuple; a reconciler is that counter
made precise, per node.

**Two refusals, stated so that principle 6 holds.** *At most one `Field` per frame* — each is
a fullscreen pass with its own depth, and composing two is not a feature this tier claims;
the refusal names both. *A `Generator` with simulation state may appear once* until its
state has moved off `World` and into the node (§10 T3).

---

## 6. The renderer's draw list — the one renderer change

Today the renderer takes **one** material set (`Uniforms`), **one** `RenderPath`, and a
`Surface` whose 60 fields are the special cases. Two things in the tree already prove the
general shape is near:

- `DemoBatch` partitions one `instances` buffer into per-`(mesh, material)` sub-batches, and
  the renderer draws them in order (§1's table).
- The scenery draw patches mat/IOR/glow/opacity/SSS/iridescence/palette onto a **copy** of the
  scene uniforms for a second draw — per-draw uniforms, done once by hand.

The target is the general case of both:

```text
RenderFrame {
    environment: Environment,               // Background today: sky, IBL, terrain, ocean, stars
    camera: Camera,
    lights: &[Light],
    draws: &[Draw],                         // Draw { mesh, material, instance_range, emits }
    field: Option<FieldPass>,               // at most one raymarch element
    post, fx, temporal, …                   // per-frame, unchanged
}
```

📌 **The 60-field `Surface` becomes the draw list, and the special cases become element
kinds.** Swept tubes are a `Mesh` built by `lower_strands`; the membrane is a `Mesh` built by
`loft_membrane`; plexus nodes and edges are two `Instances`; arm caps are an `Instances` of
capsules. Nothing in the shaders changes for the instanced path — `cube.wgsl` already reads a
per-instance `mat4` + `tint` + `emit`; the change is that a frame carries *several* draws
with *several* material blocks instead of one of each. What is per-frame stays per-frame:
bloom, tonemap, TAA, the composite.

⚠️ **This is the visualizer's regression surface, and it is why T1 is inert.** The visualizer's
current frame must be expressible as a draw list — one draw, one material, today's path —
and produce a byte-identical frame under the goldens before anything else lands (§10).

---

## 7. Where the component function runs — the decision that decides the rest

React's power is that a component is *code with state*, not static data. That is also the
trust question, and `doc/organon_prd.md` §7 has already framed it: three units of extension,
three trust profiles, and *"how far do I trust this?"* and *"which kind is this?"* are one
question asked twice. The same three answers apply to who may produce a tree:

| | **Data only** | **Sandboxed components** | **Out-of-process producers** |
|---|---|---|---|
| What runs inside Organon | nothing — a whole tree arrives, the reconciler diffs it | a WASM function per component, called by the reconciler | nothing — a hosted module writes trees over the module channel instead of textures |
| Who authors | an agent, a person, a script outside the process | a component author | a module author |
| Boundary | the parser | the sandbox | the process |
| PRD §7 kind | a **skill** (the agent is the runtime) | between linked and hosted — a *new* profile the PRD does not have | **hosted** |
| Order | **first** — the reconciler is needed under every later option anyway | third, and a measurement (§13) | second — the channel exists |

📌 **Data only is not a compromise; it is the Omarchy use case exactly.** An agent that
already emits structured events for the console to render natively is an agent that can
emit a scene document. Everything an agent can do in React it does by producing the next
tree, and the reconciler makes that cheap.

⚠️ **WASM is the honest shape for the third column, and its status has to be stated
carefully.** The modules plan rules out `dlopen` (no stable ABI; it fails in a graphics
driver on someone else's machine) and records *"no wasm runtime"* as a **current fact**, not
a prohibition (`doc/organon_modules_plan.md` §4). A sandbox is a boundary you can point at,
which is the property §10 of that plan says survives someone having a bad day. But
`ARCHITECTURE.md` §3's *"compiled to WASM via `native/organon-wasm`"* describes a crate that
**is no longer in the workspace** (measured: absent from `native/Cargo.toml`, no file under
`native/`), so no WASM toolchain should be assumed to exist here. It is a tier to measure,
not a capability to lean on.

**Signals** are what components read, whichever column they run in: the input ring's
`InputEvent`s (`organon-module::input` — pointer, buttons, keys, already specified for hosted
modules), a monotonic clock, the audio ring, and the PLL beat clock. An attribute may be
**bound** to a signal (`rotation.y ← clock * 0.2`, `emission ← audio.rms`) so that a static
tree animates without being re-sent — which is what keeps the data-only column from meaning
"resend the world at 60 Hz."

---

## 8. Transport and state — two lanes, and invariant 2 untouched

A variable-depth tree cannot live in `Shared`: fixed slots would bloat it and anything else
breaks the append-only layout. So there are **two lanes**, and they are the two things the
tree already has:

| Lane | Carries | Changes | Mechanism |
|---|---|---|---|
| **structural** | the scene document | on edit | a document — a sidecar through `ns_file`, or the module channel; the same road presets, plans and the agent config already travel |
| **live** | signal values and the visualizer's parameters | per frame | `Shared`, exactly as today, plus **one generation counter** naming which tree is current |

Signal bindings (§7) are what make the split hold: the structural lane is quiet while the
scene animates, and the live lane never carries structure.

**The verbs** follow the console's grammar — required arguments positional, optional by
keyword, one dispatch behind every front door (principle 2), a name a person says at every
node:

```text
organon scene load <path>                       # the transaction: validated whole, or nothing moves
organon scene add <path> <element> [key value…]  # scene add menu/ring mesh shape box
organon scene set <path> <key> <value>           # scene set menu/ring/item3 material brass
organon scene bind <path> <key> <signal> [gain]  # scene bind menu/ring rotation.y clock 0.2
organon scene remove <path>
```

`load` is a transaction (principle 7) validated against the registry before anything
applies, and its refusals name the path and the rule — a second `Field`, an unknown element,
a `Material` nobody references — rather than half-applying.

---

## 9. What Omarchy looks like from this side

`doc/organon_desktop_plan.md`'s tiers each become **a scene document rather than a new Rust
sibling on `World`**:

- **The radial menu (T2 there).** A `Group` of `Text` and `Mesh` items around a ring, a
  `Light` rig, a `Camera`; the highlight is one `set` on one path; the summon is a `load`.
  The menu is "already data" (`omarchy-menu.jsonc`) — the scene document is the data it
  renders *as*.
- **Organon inside Quickshell (T4).** The producer that draws into the module frame ring is
  the reconciler drawing a scene; what the `QQuickItem` samples does not change.
- **The screensaver (`doc/pbr_text_engine.md`).** A `Text` element bound to a cell-grid
  signal; the phosphor-behind-faceplate material is a `Material`; the converge-on-hold
  behaviour is the reconciler reporting *nothing changed* to the path tracer.
- **Tiles (T1).** A console region holding `3d producer scene` — the producer qualifier
  already built for `ascent` — with a document loaded at launch by the same env-var route
  the desktop plan proposes for layouts.

---

## 10. The tiers

Each is independently shippable and inert by default (`CONTRIBUTING.md`'s tier pattern,
principle 8). Ordered by conviction-per-cost.

**T0 — the tree and the registry, as pure data.** A host-free crate (the `organon-scene`
boundary: `organon-core` and `serde`, no `wgpu`, no `egui`; `cargo tree` is the acceptance
test): the element types, the document format, path resolution, the capability registry,
and validation with named refusals. Tests: round-trip, every refusal, a `describe` for every
element kind. **Nothing renders.**

**T1 — the draw list.** `organon-render` gains `Draw` and `RenderFrame` gains `draws`; the
visualizer's frame is expressed as one draw. **The bar is byte-identical goldens** under
`verify.sh`. `Surface`'s fields migrate into draws one producer at a time behind that bar;
the struct is gone when the last one has.

**T2 — the reconciler, and the first rendered scene.** The retained scene over `Draw`;
diffing; transform propagation; a scene of `Mesh`, `Text`, `Light`, `Camera`, `Material`,
`Environment` rendered in a console region as a new producer. Test: the legibility harness
on a text scene; a frame-boundary measurement the way the module ring was measured.

**T3 — `Generator` as an element.** The visualizer's stateless generators emit into a node's
frame; parameters as a delta over the live lane (the pattern `substrate_materials` and the
scenery uniforms patch already use). Stateful generators move their state into the node,
one at a time. The visualizer's own frame becomes *a scene with one `Generator`*, and stays
byte-identical.

**T4 — signals and bindings.** Input, clock, audio, beat as sources; attribute bindings; the
`bind` verb.

**T5 — execution.** Out-of-process producers writing trees over the module channel, and the
WASM measurement. 🚨 **A measurement, not a product, and the one that can come back
"no."** Nothing in T0–T4 depends on it.

**Non-goals, stated.** Rewriting the visualizer. Replacing `Shared`. A shader graph. Per-node
scripting before T5. Loaded meshes and glTF before T2 has rendered a primitive. A second
plugin identity — this is a crate under the standalone products, never a VST3.

---

## 11. Naming and placement — a collision to resolve first

- **The tree crate** wants the `organon-scene` boundary and, by any ordinary reading, the
  `organon-scene` **name** — which is taken: today's `organon-scene` is the console's
  *substrate look* (four materials, two rigs, the camera rig, the epoch ledger), a params
  builder over `Shared`. Either that crate is renamed to what it is (`organon-substrate`) and
  the name is freed, or the tree takes another word. **A decision for James, made before T0,
  not discovered during it.**
- **The reconciler** needs `wgpu` and so cannot live in the tree crate. It lives in
  `organon-render` (it is the renderer's retained side) or in a crate directly above it; the
  crate graph in `doc/arch/topology.md` decides, and the same-change rule applies to it.
- **`world.rs` is not the place**, and the reason is measured rather than aesthetic: it is
  14,062 lines organised as one linear dispatch on one generator, and the reconciler is the
  opposite shape. The visualizer reaches the framework through T3, not by the framework
  growing inside it.

---

## 12. What is measured, what is reasoned

🚨 **Read this before depending on a sentence above.**

**Measured** — read from this tree on 2026-09-03: `Surface<'a>` in
`native/organon-render/src/render.rs` has 60 `pub` fields, and the field names quoted in §1
are among them; `organon-render/Cargo.toml`'s dependencies are the six named in §3;
`World::frame_body` starts at `world.rs:2365` in a 14,062-line file (its 7,471-line length is
`ARCHITECTURE.md` §9's figure, not re-measured here); `Shared` in
`native/organon-core/src/ipc.rs` declares 177 `[f32; N]` blocks and `LAYOUT_VERSION` is
`0x0_2_8_5`; the ~1,370 parameter count is `doc/equations_into_light.md`'s; `DemoMesh`,
`DemoBatch`, `DemoLight` and `demo_scene`'s signature in `math.rs`; `RenderPath`'s variants;
`Background<'a>` and `LightTransport<'a>`'s fields; `organon-module`'s modules (`channel`,
`gpu`, `input`, `map`, `presence`, `ring`, `sim`, `wire`) and `input::InputEvent`; the
`3d <producer>` qualifier being built (`doc/organon_module_viewport.md` §4.2's own ✏️ note);
`kind.rs`'s rule; the absence of `organon-wasm` from `native/Cargo.toml` and from `native/`;
`organon-scene`'s five modules and its module doc's boundary statement; the four `pack_*`
blocks `core_catalog()` reads; `PlanMove`'s two variants.

**Quoted, not re-derived:** the R3F origin (`ARCHITECTURE.md` §3, "Legacy `/src`"); the
PRD's principles, §5.1's tree argument, §6's producer boundary, §7's three units and §12's
"no dynamic loading"; the modules plan's `dlopen` ruling; the desktop plan's tiers; the
pbr-text design's RT-and-impostor finding and its `pt_content` proposal.

**Reasoned, unverified** — each is a place this document could be wrong:

- **That a draw list can be introduced under byte-identical goldens.** Per-draw uniforms
  are a bind-group change; the claim that no shader changes is an expectation from reading
  `cube.wgsl`'s per-instance inputs, not a diff.
- **That TLAS instance updates without BLAS rebuilds are reachable through `wgpu`'s RT API
  as used by the five `rt_*` modules.** Read, not exercised.
- **That the reconciler belongs in `organon-render`.** It may want a crate of its own the
  moment it holds text layout; that is a topology decision the doc defers.
- **That `Text` can be "the glyph ring generalized."** The ring is a fixed grid of cells; a
  `Text` element wants layout. The legibility harness is what would say whether a laid-out
  run still reads.
- **The whole of §7's third column.** No WASM runtime has been linked, sized, or timed here.

---

## 13. What is not known

1. **The cost of a diff at 60 Hz for a data-only producer.** A radial menu is tiny; a text
   scene of a few thousand cells is not, and whether a whole-tree resend per frame is
   acceptable *before* bindings exist (T4) decides whether T2 can ship without T4.
2. **Whether path-trace convergence survives a retained scene.** Today accumulation restarts
   on camera, resize and content-setting changes only; a reconciler can report geometry
   changes precisely, but the restart policy has to be designed so that a blinking
   highlight does not reset a photograph.
3. **What the first non-visualizer producer is.** The radial menu is the strongest candidate
   (it needs nothing the desktop does not already have) but it is a desktop-plan tier, and
   this document does not reorder that plan.
4. **The `organon-scene` name** (§11).
5. **Whether a WASM sandbox is a tier or a fork.** Sizing, startup, and the GPU-facing
   surface a component may touch are all unmeasured.

---

## 14. Provenance

Written 2026-09-03 from a conversation with James on the same day, on the branch
`claude/organ-scene-graph-architecture-fa7q4g`. The reframing — *the visualizer is an app
on the framework, not the framework* — is James's, and it corrected two earlier drafts of
this argument that tried to generalize the visualizer's scene instead. The measurements are
the author's; the design is reasoned from them and from the documents quoted, and §12 is
where the two are kept apart.
