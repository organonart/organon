### A module's picture was going to come out dark, and every counter read clean

- **`FrameTexture` handed egui a view that decodes, and egui decodes again.** The wire carries
  sRGB-encoded bytes and nothing else does — `PixelFormat` has two variants and both are sRGB,
  because that is what a producer's swapchain-shaped texture holds. The destination texture was
  created with `view_formats: &[]` and viewed through `TextureViewDescriptor::default()`, so the
  view's format was the texture's and sRGB was converted to linear **at every sample**; egui's
  shader then linearized its own samples on top. Two decodes. A mid-grey 128 arrives as roughly 55.

  🚨 **The console had already written the rule down, ten lines from the top of the file it was
  broken in.** `BACKDROP_SAMPLE_FORMAT`: *"render the world through the sRGB format, hand egui a
  non-sRGB view of the same bytes — egui's shader linearizes its samples itself, and a
  decoded-on-sample view would linearize twice and come out dark."* That constant is named nine
  times in `console_main.rs`. `render_backdrop`, `snapshot_live_backdrop`, `upload_exhibit` and
  `make_surface_texture` all obey it. **The module path was the tenth and the only one that did
  not.**

  📌 **A new shape of an old class: not *the code changed and its meaning did not*, but *a new path
  arrived beside an old invariant and nothing connected the two*.** The rule was documented, the
  bytes were correct, no counter could see it — `torn_reads`, `corrupt_reads` and `allocations` all
  read clean — and the only available report was a person saying the viewport looked murky. It was
  found by reading the file's own comment against the code beneath it, which is the only instrument
  that works on this class.

  ⚠️ **A comment made it easy to read past, and rewording it quietly would have wasted the lesson.**
  The line above the egui registration said *"Linear, like every other picture the console
  samples"* — which is `FilterMode::Linear`, the **sampling filter**, a different question with the
  same word in it. Standing alone at that site it made the colour-space question look asked. It now
  says which of the two it answers and points at what answers the other.

  🚨 **It could not be fixed at the call site, which is why the contract crate moved.** Choosing a
  different frame format cannot help — both are sRGB, so the bytes were always right and only the
  *reinterpretation* was missing — and `view_formats` is fixed when the texture is created and wgpu
  validates `create_view` against it, so an empty list makes a non-decoding view illegal however the
  caller asks for one. `organon-module` gains `linear_view_format` and `FrameTexture::sampled_linear`.

  📌 **`new()` is deliberately unchanged, and that is a judgement rather than caution.** A consumer
  that composites normally *wants* linear values out of the sampler and is right to take the
  default. This is a second legitimate consumer being served, not a bug fixed for everybody —
  a distinction that is invisible until it is stated, and the one most likely to be got wrong by
  "fixing" it globally.

  📌 **`sampled_linear()` takes no argument**, on `module_work::Tool`'s reasoning one door along:
  `view_formats` accepts only a format differing from the texture's in its sRGB-ness, so a general
  `sampled_as(fmt)` would be an API whose wrong answers are a wgpu validation error at texture
  creation. A closed set is the point; an open one buys only the ways to get it wrong.

  ⚠️ **Two claims about other trees were made and retracted in the course of this, and both were
  the same defect.** That the finding came from the Ascent session (it came from a review), and that
  `organonart/ascent` pinning `organon-module` meant the fix needed a second repository to bump
  before a hosted module painted correctly. The second had become a scheduling constraint before it
  was checked. It is false: `FrameTexture` is the **console's** half, the console compiles this
  repo's copy as a path dependency (`native/Cargo.toml`), the only construction in either tree is
  `console_main.rs`, and nothing on the wire moved. **One event.** The shape worth keeping is *a
  claim about a tree you cannot see, asserted in the register of an observation* — cheap to make,
  expensive to un-assert, and in this case checkable in twenty seconds.

  ✏️ **And the mutation run caught a defect in the test written to catch the defect.** The first
  draft pinned both answers as literals *and then* looped below them re-checking that the answer
  decodes nothing and that the channel order had not moved. Running the mutations showed that loop
  was **unreachable** — once both answers are pinned, the function cannot return anything else for
  those inputs, so no later assertion about them can fail. It read as extra coverage and was
  decoration. Cut. 📌 The general form is worth more than the instance: **a mutation run does not
  only tell you whether a test bites, it tells you which assertion bit** — and an assertion that
  never gets the chance is `region.rs`'s unreachable arm, arriving in a test rather than in a
  match.

  ⚠️ **Derived, not observed, and the falsifier is one glance.** Nobody has seen a hosted module's
  picture. This rests on the rule quoted above, on nine sibling paths obeying it, and on
  `create_view` with a default descriptor yielding the texture's own format. To settle it: a solid
  mid-grey frame from `organon-module-sim` beside an Organon viewport.
