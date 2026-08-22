### Three tests derived both sides of their comparison from the thing under test

- **A test that computes its expectation from the function it is testing cannot see that
  function change**, and it feels like coverage the whole time. Three in `organon-module`,
  found by looking for a shape the Ascent session hit in its own tree — a test comparing
  `source()` against `source()`, which agreed by construction and would have survived reverting
  the entire change it existed to protect.

  🚨 **The one that would have crossed the wire silently.** Every test of the key vocabulary
  compared macro-generated `Key::ALL` against macro-generated `from_wire`. Renumber `W` from
  `0x1A` to anything at all and both sides move together: every test passes, and **every
  keystroke changes meaning in the other process**. The usage ids are borrowed from USB HID
  precisely so that neither repository owns them, and Ascent maps its own `Source` onto these
  numbers — so a renumber here is a silent rebind there. They are now pinned as **literals**:
  the start of each run, plus HID's two genuine surprises, which is where an error would
  actually be made. ⚠️ `Digit0` is `0x27` — **after** `Digit9`, so the digits are not
  `Digit0 + n`. The arrows are **Right, Left, Down, Up**, which is nobody's writing order.
  📌 Sampled rather than duplicated on purpose: restating all hundred-odd rows would be a second
  table, not an independent statement of an external standard.

  🚨 **The one with the widest blast radius, and the mutation result is the argument.**
  `sim::verify` checks pixels against `sim::pixel`, and `SimProducer::draw` writes
  `sim::pixel` — so nothing could see the frame index stop reaching the output, after which
  `verify` passes on **torn** frames, which is the single thing the whole tear-detection suite
  is evidence for. Dropping the frame index from the picture fails **exactly one test, the new
  one**: all eighty others pass, the entire tear suite included, against a `verify` that can no
  longer detect a tear.

  The third was half-anchored — the producer-side reserved-key test asserted `Escape` by
  literal and derived the rest from `RESERVED` — and now compares against `[0x29, 0x44]`
  outright.

  📌 **Three faces of one class turned up in a single evening, and none of them fails; each one
  *looks* like the guard for exactly the thing it cannot see.** A **one-way table**, where the
  compiler enforces one direction and nothing enforces the other. A **widened condition under
  an old latch**, where the code did not change and its scope did. And a **self-referential
  assertion**, where both sides move together. ⚠️ The connective tissue is worth keeping: the
  invisible direction is the one where *something else is already helping you* — an exhaustive
  match, a passing neighbour test, a generated table — which is precisely why it feels covered.
