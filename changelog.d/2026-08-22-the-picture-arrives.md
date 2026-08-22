### A hosted module now draws into a region, instead of saying that it cannot

- **`viewport left 3d producer ascent` has been half-true since T4**: you could approve a
  repository, build it, and name it in a layout, and the rectangle said the module was not
  running. It now **launches the binary, takes its frames and paints them.** The console still
  knows nothing about what the module is — a producer name, a rectangle, a size, a texture and
  a channel, which is §4.6's *never* list unchanged.

  **The launch is a seam, not a call.** `Workshop` grew `spawn`, a different shape from `run`
  rather than a variant of it: nothing is collected and nothing is waited for, and the only two
  questions ever asked of the handle are *has it exited* and *stop*. Every decision around it is
  still a pure function of a string, so the whole tier is unit-tested with no window and no GPU.

  🚨 **The binary is derived, never named by the module**:
  `<checkout>/target/release/<producer>[.exe]`, behind `check_producer_name`, beside
  `artifact_dir` and for its reason plus a stronger one — a `binary = ` key in
  `organon-module.toml` would be somebody else's string arriving at `Command::new`, which is
  exactly what `Tool`'s two-variant enum exists to prevent. So a module's package must produce a
  binary named after its producer. **The handoff is `$ORGANON_MODULE_CHANNEL`**, a constant in
  the contract crate because it is an agreement between two separately built binaries in two
  repositories — and an environment variable rather than an argument, so a module's own `clap`
  never has to know it exists and *presence* is what distinguishes being hosted from being run
  by a person.

  🚨 **Two verdicts, and the order between them is the design.** `organon-module`'s
  `presence.rs` writes down its own limit — hung and exited-without-a-farewell are
  indistinguishable from inside a shared mapping, and the process handle is the thing that
  genuinely knows. So `observe` asks the **handle first** and the channel second: a producer that
  dies quietly leaves counters that stop, and the channel says `Live`, then `Stalled` after a
  second, then `Lost` after five — right, but late and vague — while the handle knows this frame
  and knows the exit code. Mutation-tested by deleting the early return: two tests fail, one
  reporting `None` where `Exited { status: Some(0) }` was owed.

  **All four of §4.6's rows are live, and the last two were unreachable rather than unbuilt** —
  a launcher is what makes them reachable. Six sentences from three authors, and **none written
  twice**: the registry's two are `ModuleState`'s, the producer's are `Presence`'s, and only the
  two a launcher alone can reach are phrased in the new file.

  🚨 **Never the last good frame is not a rule at the paint site.** `poll` makes `Poll::Frame`
  unreachable once a producer is judged stalled, lost or gone; `FrameTexture` takes a whole
  `Poll` and drops the picture itself; `view()` is then `None`; the rectangle draws the sentence
  whenever it is. `no_verdict_that_forbids_a_picture_can_also_deliver_one` publishes a **second
  whole frame** before going silent — so the ring genuinely holds a valid unread one, which is
  precisely what a console trusting its ring would paint — and asserts `Forget` at +2 s, +6 s
  and +30 s.

  ⚠️ **A module arrives `Attached` and there is no method that changes it.** Invariant 4 as
  structure rather than as a rule: `set_lifecycle` exists one crate down and nothing in this tier
  reaches it. Interaction is the next tier, and the tier was split to say so.

  📌 **`engine_plan` is untouched and `the_engine_is_asked_for_at_most_one_frame` did not
  widen**, exactly as the design predicted — a hosted producer renders no `World`, so widening
  that test would *assert* it was a claimant. The claim went into `hosted_producers(&Layout)`,
  `region_showing_world`'s complement.

### `console module restart`, and the verb argument that went the other way

- **`presence.rs` predicted `the_verb_constants_and_the_action_words_are_one_table` would fail
  when a fifth verb arrived, and predicted the answer would be to keep `restart` out of
  `MODULE_ACTIONS`** — *"a thing the console does to a producer, not a thing a person approves."*

  The premise is right and the conclusion does not follow. That table is not a list of approvals
  — `diff` and `revoke` are not approvals either — it is the **grammar** of `console module`,
  read by clap's value parser, by the slash ring and by `ModuleCmd::resolve`. And the grammar is
  precisely what the constant is spent on: a dead rectangle ends *"`console module restart` to
  restart it"*, so a person reads that verb and types it. Leaving it out would print a verb the
  grammar refuses — the exact drift those constants exist to catch, arriving through the door
  built to stop it. **So the test got a fifth row on both sides rather than a nudge**, and the
  correction is recorded back in `presence.rs` where the prediction was made.

  ⚠️ It runs **synchronously on the frame thread**, on `revoke`'s rule: no network, no compiler,
  microseconds. The verb a person reaches for *because* something is broken must not be queueable
  behind the build that broke it. 🚨 And it **stops without starting** — the next frame launches
  it — because a launch there would be a second launch site, which is how a producer comes to be
  started twice against one channel.

### The frame boundary's third number, taken at last — and it is not the copy

- **§4.4 named three numbers, T0 measured two, and was explicit that the third could not be
  attempted**: staleness needs a second process and a protocol that did not exist. Both now do.
  `organon-module-sim` is a producer in its own program, `FrameView::age` is the subtraction, and
  `doc/measurements/module-staleness-2026-08-22.md` is the answer.

  **The frame the console takes is 8–11 ms old at a 60 Hz consumer — half a frame — p90 15–16 ms,
  and flat across nine times the pixels.** 1440p is not reliably worse than 640×360; 1080p came
  in below 720p.

  🚨 **That is not what a transport cost looks like, so it was tested rather than asserted.**
  Hold the size fixed and move the producer's cadence: staleness is
  `≈0.6 × min(producer period, poll interval)` — set by the two loops' **phase**, with the frame
  size absent from the expression. The reading that would have forced mechanism A (*staleness is
  the copy, therefore buy `unsafe` per-backend interop*) is not what the measurement says, and
  §6's handoff keeps a weaker, more honest justification: not that the transport is intolerable,
  but that **nothing has measured input-to-photon**, which is what flying actually turns on.

  ✏️ **The model in the first draft was wrong and the measurement said so.** It predicted
  median ≈ *P*/2, passed at 4, 8 and 16 ms and failed at 33 ms. The reason is structural: once
  the producer is slower than the consumer, the consumer is no longer the thing sampling, so the
  age spreads over the *poll* interval instead — half of whichever loop is **faster**. The failure
  was the useful part.

  ⚠️ **The rig set a trap for itself, and it produced a credible wrong table.** Two tests on two
  threads of one process, both measuring 1280×720, with the channel file named from
  `(pid, width, height)` — so they shared **one mapping**. It did not present as an error: one
  test skipped a row, the other reported a median 30 % better than its own solo run of the same
  condition. Fixed twice, because the halves differ — a counter in the filename removes the
  corruption, and `--test-threads=1` in the documented command removes the interference, which a
  unique filename does nothing about.
