### Added

- **A viewport can say who draws it: `viewport left 3d producer ascent`.** T4 of
  `doc/organon_module_viewport.md` — the producer behind a `3d` region was implicitly Organon's
  `World` and is now **sayable**, over the approved-module list T3a landed. Every existing command,
  every existing `layouts.json` and every doc line means exactly what it meant: **an omitted
  producer is Organon**, checked as bytes rather than as an intention — a captured layout still
  stores `"3d"` and a sidecar line still reads `viewport left 3d`.

  🚨 **A qualifier inside `3d`, never a fifth content word.** `CONTENT_WORDS` is untouched and
  `the_word_tables_and_the_resolvers_are_one_vocabulary` passes unedited. `region.rs` argued at
  length that `world` lost to `3d` precisely so the content vocabulary would never name a renderer;
  a word called `ascent` makes the same mistake with an application's name.

  ⚠️ **Two departures from §4.2 as written, both flagged rather than left to land quietly.**
  The spelling is **keyword-tagged** — `producer ascent` on the wire, `--producer` at the CLI,
  `producer ascent` in the composer — not the design document's bare `viewport left 3d ascent`:
  §1.8's grammar fills required arguments positionally and optional ones by keyword at all four
  doors, and #98 Tier C settled the identical question one verb over for `console stack … region
  <word>`. And an unapproved producer is refused at the **command** door only; a producer read out
  of a saved layout is deliberately not checked, because §3.5 requires that a layout naming a
  module you have revoked still opens.

  🚨 **An unknown producer is refused by name, listing the approved ones, and never falls back to
  Organon.** The person would get a picture, and the wrong one, which is worse than a refusal.

### Changed

- **`only_one_because` moved from the content kind to the producer**, which is where that
  function's own doc predicted it would go: *"a future producer might fill four regions happily,
  and would otherwise inherit a refusal it has no reason to obey."* Organon keeps today's reason
  word for word — shared `frame_index`, shared TAA jitter phase — and a hosted module answers
  `None`, because a separate process rendering into its own texture has no jitter phase to trade.
  **Two `3d ascent` regions are legal.**

  📌 `region.rs`'s standing objection to inventing a `Producer` enum is **discharged, not
  overruled**: it was about an enum with one variant, an unreachable arm pretending to be a design.
  There are two now and both are reachable from a command a person types.

- **`engine_plan`'s second input is now "a region holds `3d` *whose producer is Organon*".**
  Getting this wrong is silent: the console would render a `World` frame for a rectangle nobody
  paints and starve the backdrop for it, and a wasted frame is not an error. `region_showing_world`
  is split out of `Console` as a pure function so the answer is testable with no window.
  ⚠️ `the_engine_is_asked_for_at_most_one_frame` is **not** widened — §4.5 is explicit that a hosted
  module is not a claimant on that invariant, so widening it would assert something false.

- **`Content` is `Clone` rather than `Copy`**, and `ContentCmd`, `Layout` and `Placed` with it — a
  producer name is a runtime string out of `modules.json`. The two alternatives (an inline
  fixed-capacity name, an interner) are weighed in `region.rs`'s header. 📌 The property `plan`
  leans on survives: `Agent`, `Panel` and `3d`-with-Organon are unit-shaped and clone with **no
  allocation**, so a console with no approved module costs exactly what it cost before, and only a
  hosted producer clones a short string.

### Notes

- **Nothing draws a hosted producer's picture, and nothing was meant to.** There is no protocol and
  no process — T3b and T5 own those. A region holding `3d ascent` paints `ModuleState`'s sentence:
  *not approved*, or *approved and nothing built from it*, each naming the module and the verb.
  Never a blank and never the stale texture, which is the easiest wrong thing to paint because the
  texture is still there and still valid.

  ⚠️ **That sentence is in tension with a rule set the day before** — `paint_region_notice` lost
  four explanatory sentences on 2026-08-21 (*"We never want text just pasted in explaining
  something into the UI"*). Every sentence that went described a **working** console and was a
  consequence of something visible elsewhere in the window; this one is not, and it says the one
  thing nothing else in the window can. It is one line to cut, and the sentence comes from
  `module.rs` rather than the paint site so cutting it changes one place.

- **No second cache.** The producer ring reads `ModuleRegistry::for_completion`, which T3a moved
  into `module.rs` carrying §1.15's measurement — the candidate walk runs on the draw path and asks
  n + 1 times per call, 10.1 ms for a hundred entries against a 16.7 ms frame when read straight
  from disk. The region walk's vacancy lookup now shares it: one cache, two draw-path callers.

- **Nothing here has been looked at on a screen.** Whether a sentence in a rectangle reads as
  deliberate or as broken is James's call and no test reaches it.
