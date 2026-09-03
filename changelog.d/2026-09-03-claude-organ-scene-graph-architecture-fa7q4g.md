### Organon as a content framework — the design doc

`doc/organon_content_framework.md` answers a question that came up while planning Organon
for the desktop: does creating real-time 3-D content on the fly need a scene model of its
own — an engine, a scene graph, a logical hierarchy of every capability — and can it be
reached by widening what exists? It is design only; nothing is built, and §12 keeps the
measured claims apart from the reasoned ones.

🚨 **The reframing is the finding: the visualizer is an app, not the framework.** The
generator/surface/material triad over one flat `Shared` snapshot is the right model for an
instrument — one form, infinitely parameterized, every parameter host-automatable — and the
wrong foundation for "anything you could imagine that could be visualized." The tree shows
what happens when general composition is asked of it: every new *kind* of thing on screen
was wired in as a hand-made sibling (the Scenery layer, the Demo scene builder, the glyph
ring), each costing a `Shared` block, and `organon-render`'s `Surface` is now a **60-field
union of every special case**. The arity of the scene is being encoded in the IPC layout.

📌 **The reference is React, not Unity, and the tree already knew it.** Organon's first
incarnation was a react-three-fiber app, and R3F is exactly the shape proposed: a declarative
element tree, a reconciler that diffs successive trees into a retained scene, and a host
renderer beneath that never learns what the tree is. Organon has the renderer — already a
separate crate with the right boundary — and the platform around it. It has neither the tree
nor the reconciler, and those two are the framework. A generator becomes one element kind
among `Mesh`, `Text`, `Light`, `Camera`, `Material`, `Environment`; the beat system becomes
one signal source among input, clock and audio.

⚠️ **Two facts worth knowing before quoting the older docs.** `ARCHITECTURE.md` §3 still
says the web port compiles `math.rs` *"via `native/organon-wasm`"*; that crate is no longer
in the workspace, so no WASM toolchain should be assumed here — the doc files sandboxed
components as a tier to *measure*, not a capability to lean on. And the crate name the tree
type wants, `organon-scene`, is taken by the console's substrate look; §11 names that
collision as a decision to make before the first tier rather than during it.

The tiers are ordered so the visualizer is never at risk: the tree and registry as pure data
(T0), a draw list in the renderer under byte-identical goldens (T1), the reconciler and the
first rendered scene (T2), generators as elements (T3), signals and bindings (T4), and
execution — out-of-process producers and the WASM measurement — last (T5), because nothing
before it depends on it.
