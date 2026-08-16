### A region can hold a live 3D viewport, and the portal becomes one presentation of it

`organon console viewport <region> 3d` — `/viewport left 3d` from a conversation composer — gives
a region a live, orbitable 3D picture beside the transcript. Drag inside it to orbit, wheel over
it to zoom, and `organon set` / `generator` / `recipe` typed at a prompt in the same console drive
what it shows, with no wiring at all: that lane already drains inside the render the region runs.

🚨 **The generalized 3D viewport is the thing being built, and Organon is a particular application
of it** — James's own ordering, and it decides most of what follows. The word is `3d` rather than
`world` precisely because `world` would name today's only renderer in the vocabulary a person
types; a region says *a 3D picture belongs here*, and which engine draws it is the producer's
business. `scene` was the better register and is taken: in this tree it already means the
substrate painted *behind* the glyphs.

**The producer seam is one sentence, and there is deliberately no machinery behind it.** *A
producer yields a texture the console can sample, at a size the console asks for.* An in-process
producer satisfies that trivially; an out-of-process one satisfies it later by importing a shared
texture, **without restructuring the region model** — which is the whole accommodation, and it
costs a boundary rather than a layer. No producer enum with one variant, no trait methods nothing
calls, no vocabulary word for choosing one. An unreachable arm is an untested branch pretending to
be a design.

🚨 **The "only one" limit belongs to Organon, not to viewports.** At most one region may hold `3d`,
and the earlier plan recorded that as a property of the *content kind*. Under the ordering above
that is backwards: it is **Organon** that cannot be drawn twice in a console frame, because its
two targets share `frame_index` and the TAA jitter phase riding on it. A future producer might
fill four regions happily and would otherwise inherit a refusal it has no reason to obey. So the
single site that decides answers with a **reason** rather than a bool, and the refusal a person
reads names Organon and its shared jitter phase. ⚠️ What is *not* available is attributing it in
the type system — inventing a one-variant `Producer` to hang it off is the untested branch this
module already declined to build — so the attribution is a reason string, a doc and a test that
the refusal quotes it. ⚠️ A **widening** is still allowed: `full 3d` while `left` holds `3d`
displaces `left` and stands, because the check is asked of what *survives* the assignment rather
than of what is held now.

🚨 **Two claimants now want the one World frame, and the portal wins.** `engine_plan` is widened to
arbitrate and to answer *which* presentation renders rather than a bool, and the proof that the
engine is asked for at most one frame per console frame is widened with it — to the full
2 × 2 × 3 × 2 cross product, because widening the function and leaving the loop at its old arity
is exactly a test that keeps reporting green about a space it no longer covers. The portal wins on
the argument it already had: it is **temporary and dismissable**, so the state where it holds the
frame ends with one word in the same ring as the word that got you there, while the region is the
persistent thing a person arranged and is still arranged underneath. Nothing is written, so
closing the portal hands the frame straight back. The two rejected rules are recorded rather than
left to be re-derived — *the region wins* makes `/portal open` a verb that silently does nothing,
and *refuse the second by name* makes it look broken, both because this lane is fire-and-forget
and the refusal reaches nobody. ⚠️ **The loser paints a notice and never a stale texture**: the
yielded region says what holds the world, why, and the command that gives it back. A rectangle
that *was* rendering a world and now is not is precisely what a broken viewport looks like.

**The portal is unchanged from a person's point of view, and nothing is implemented twice.** Same
verb, same two states, same screen-anchored rect, same wheel claim, same "shows the World"
correctness argument. What changed is the description: a **viewport** is a producer plus a camera
plus a texture, the portal is one way of presenting one — floating, summoned, dismissable — and a
region is another — placed, persistent, arranged by hand. `SceneMode` has modelled that
distinction since before either existed, so it is the seam rather than a parallel notion invented
here. One texture, one render, one paint, one gesture accumulator, one pixel-ratio, one pointer
test widened to a list. ⚠️ **The texture release moved to a single site and that is a
consolidation, not an omission**: closing a portal used to free it on the spot, and a `3d` region
can stop being live by three further routes — cleared, displaced, or the layout reset — so a
release per route is how the one nobody remembered comes to leak. The render gate asks the plan
rather than asking what just changed, so it is total over every route by construction.

⚠️ **The wheel is region-aware now, which is the second consumer the region tier predicted.** The
terminal reads the wheel from raw input, so nothing about egui's layer order keeps a scroll over a
picture out of the scrollback — only an explicit rect test does, and there is now more than one
rectangle to test. Both rectangles are in the list even though at most one is live, because a
yielded region is still not the transcript. The rects are computed before anything is drawn rather
than read back from the region walk: the walk visits regions in a fixed order, so relying on the
viewport having already consumed the scroll would be relying on the layout's alphabet.

**One camera, and a region does not get a second.** There is one `World`, so a region viewport and
the portal are two windows onto the same viewpoint rather than two viewpoints — one gesture
accumulator, drained once per frame, and the hand-outranks-an-agent arbitration needed no widening
at all, because a drag is a drag whichever rectangle it landed in. ⚠️ The visibility advisory did
need widening: without it, `console camera` would have warned *"nothing on screen is showing the
world"* at somebody watching a live picture. `console.camera.read` reports `region_3d` as its own
key beside `portal_open` — a separate fact, because the portal is something an agent may close and
a region viewport is something a hand arranged, and folding them together would invite
`console.portal close` aimed at a rectangle it cannot touch.

**Deliberately not built.** The **external-process portal** — opening a portal launching a separate
process in its own window, as Organon's visual already does — is James's stated next step and is
recorded with the four facts it will need (`spawn_visual()` probes by file name, the two processes
share the `Shared` mmap, `ipc.rs::ns_file` namespaces every channel, and `$ORGANON_IPC_NS` is the
runtime override the console already injects into every tab). No stub, no arm, and today's portal
was not changed in anticipation of it. Out-of-process producers, `media` content, the `panel`
body, drag-to-resize dividers and saved layouts are all still absent.
