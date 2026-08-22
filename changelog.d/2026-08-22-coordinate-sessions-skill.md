### Process

- **The coordinator pattern is a skill now, and the verification bar grew a seventh leg.**
  `.claude/skills/coordinate-sessions/` writes down how one session drives worker sessions and
  subagents that stay in contact: which of the three messaging registries to use, the six-part
  brief template, verify-the-artifact-never-the-report, and the failure modes already paid for.
  It was being reinvented per session and reconstructed from memory in every handoff.

  🚨 **The bar in `CONTRIBUTING.md` gains the command that was missing, and its absence was
  invisible.** The targeted six-command substitute that circulates in briefs never ran the root
  crate's **324 lib tests**: the fourth command only `check`s that target and the fifth and sixth
  test *binaries*, so every unit test under `native/src/` was compiled and never executed. A change
  whose tests live there could report "the bar is green" in good faith with none of its own tests
  run. Found only because a contributor's new tests were entirely in that target and the counts
  never moved; confirmed independently on `main` — 324 passed, and nothing else in the bar reaches
  them.

  📌 The rule that closes it is a reporting one rather than a command: **say which leg ran your
  tests and what the number was.** "The bar is green" and "my tests ran" are different claims.

  ⚠️ Both files also record two things measured rather than assumed: a `spawn_task`-spawned
  session's `sessionId` is **not** a `SendMessage` address (reply on the channel the message
  arrived on), and a review that ran twice leaves **duplicate** threads — resolving only the pair
  you read leaves the merge blocked while looking like resolution failed.
