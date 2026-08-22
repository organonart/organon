### Design

- **A hosted module in a viewport — the contract, in writing** (`doc/organon_module_viewport.md`).
  Ascent asked to be composited into a console region, and the four questions that implies are
  answered before either side is refactored: what "approve a repo" means mechanically, what the
  producer contract is, where the trust boundary really falls, and whether "paused, no sound" is a
  property of this module or of the protocol. Nothing is built and no identifier moved.

  The load-bearing conclusions: **hosted, not linked** — and the deciding argument is not the
  licence but that a linked module is adopted by *rebuilding*, which cannot answer a sentence
  whose every verb is performed by a running program. The word stays **`3d`** with a **producer
  qualifier** (`3d ascent`), because `region.rs` chose `3d` over `world` precisely so the
  vocabulary would never name a renderer — and `Content::only_one_because` moves to the producer,
  which is what its own doc predicted. **`engine_plan`'s invariant is untouched**: a hosted module
  renders no `World` and shares no jitter phase, which is the best available evidence that §1.14's
  producer seam was drawn in the right place.

  Two holes are named rather than papered over: **building a module from source is linked-level
  trust** even though running it is not, and **"no sound" is an expectation, not an enforcement**,
  because a separate process can open the audio device without asking Organon. `doc/organon_modules_plan.md`
  gains a §12 pointer carrying both back to the plan whose §10 table they qualify.
