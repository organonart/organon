### The invisible direction is the one where something else is already helping you

- Three more faces of #146's meaning-drift class, all found in one night, none of which fails a
  test. They are in the review rubric now with the connective tissue that explains why each one
  *feels* covered: a compiler, a generator or a neighbouring green test is doing half the job, and
  the half it does not do is the one nobody looks at.

  ⚠️ **A one-way enum tag is invisible to the compiler in exactly one direction, and it is never the
  direction you are editing.** An exhaustive `match` demands the encode arm; nothing demands the
  decode arm, so a new variant encodes fine and decodes to `None` for ever. Guarding it takes two
  halves — an exhaustive match *and* a scan of the wire-code space — and neither alone is enough.

  🚨 **A test that compares the function under test against itself passes by construction.**
  Measured: a frame verifier checked against the same helper the writer used, so the **entire**
  tear-detection suite passed against a verifier that could no longer detect a tear. The fix's
  mutation run reads `80 passed; 1 failed` — every test that existed to prove tearing was caught did
  not catch it.

  ⚠️ **A value shared across a boundary cannot be pinned by either side's generator.** Two
  macro-generated tables agreeing says nothing about the numbers: renumber a key, both sides move
  together, every test passes, and every keystroke changes meaning in the other process.

  📌 The coordination skill gains the process half, which is the stronger claim: **each session found
  its own instance only after seeing the class in the other's tree** — three times, never
  unprompted. So: when a peer reports a defect class, search your own tree for its reciprocal before
  replying, and relay findings as shapes rather than as incidents. It also gains the stranded-commit
  check (`git merge-base --is-ancestor`), because a commit pushed while a PR is being merged lands on
  the branch after the merge commit, is absent from `main`, and nothing anywhere reports the gap.
