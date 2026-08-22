### `build()` recorded a success for a binary an earlier commit had left behind

- **Existing is only half of what has to be true.** `module_work::build` already refused a build
  that produced no binary — cargo skips a `[[bin]]` whose `required-features` are unmet, silently,
  with exit 0. What it could not see is the same skip **with a binary from a previous commit still
  in `target/`**: `file_exists` passes, `modules.json` records a built commit, and the console then
  launches bytes no record names. That defeats §3.4's entire reason for recording a commit, and
  every indicator stays green while it happens.

  🚨 **The obvious mechanism is wrong, and wrong in the expensive direction.** Stamping the build's
  start and comparing the artifact's mtime was the proposed fix and it **refuses an ordinary repeat
  build** — cargo does not relink when nothing changed, so the artifact is older than the build that
  just succeeded. Three candidates were built as a scratch crate with a `required-features` bin and
  run in all four states rather than argued about:

  | | binary on disk | `compiler-artifact` | `file_exists` | mtime | shipped |
  |---|---|---|---|---|---|
  | skipped, nothing there before | no | none | ✓ | ✓ | ✓ |
  | a real build | yes | yes | ✓ | ✓ | ✓ |
  | **an up-to-date rebuild** | yes | yes, `fresh` | ✓ | **✗** | ✓ |
  | **a stale leftover, now skipped** | yes | none | **✗** | ✓ | ✓ |

  📌 **Rows one and four are misconfigurations; row three is Tuesday.** A check that refuses a
  legitimate rebuild is withdrawn within a day — **and takes the row-four hole with it.** So it does
  not merely fail, it fails in the way that discredits the check it was meant to be. That is a
  category of wrongness worth naming separately from "has a false positive", and it is the reason
  the mechanism changed rather than being patched.

  ⚠️ **And it is the staleness rig's own lesson in a different currency: measure the quantity you
  care about, not a proxy for it.** The quantity is *did this build produce this binary*. mtime is a
  proxy that is right three times in four and wrong on the common case — the same shape as a sweep
  whose lever was connected to nothing while every number looked plausible.

  ⚠️ **`json-render-diagnostics`, not plain `json`, and testing the failure path is what caught it.**
  Both put machine-readable records on stdout; plain `json` *also* moves the compiler's diagnostics
  there, leaving stderr with `Compiling…` and `could not compile … due to 1 previous error`.
  `ToolOutput::tail` prefers stderr — so the change would have improved one refusal while **silently
  gutting another in the same file**, and `BuildFailed` would have carried the summary instead of
  the error. Measured on a deliberately broken crate: plain `json` leaves **zero** rendered
  diagnostics on stderr. Only breaking a build and looking at what the person would be shown could
  have surfaced that; a green run never would.

  📌 **`fresh` is deliberately not consulted**, with a comment saying so, because it *looks* exactly
  like what a staleness check should gate on. It is cargo asserting the on-disk binary **is** current
  for these inputs — the question being asked, not a reason for suspicion — and gating on it
  re-introduces row three. Pinned by a test rather than left to a paragraph.

  ⚠️ **Matched on file NAME rather than the full path**, because a store root reached through a
  symlink, a junction or a Windows short path is a different spelling of the same file, and an exact
  compare would refuse a good build — row three's failure by another road. On this machine junctions
  into repositories are standing configuration, so that is the **less** exotic branch. 📌 The rule
  behind the call: **when one error direction discredits the check and the other is exotic, take the
  exotic one.**

  📌 **Two refusals, two sentences, because they are two diagnoses**: *no binary at all* is a fact
  about the **repository** failing §4.7's published obligation; *a binary this build did not produce*
  is a fact about the **checkout**. ⚠️ The second has to concede the exit code or nobody believes it
  — the first reaction is *"but it built fine"*, and it did, honestly.

  ⚠️ **A dirty tree stays a separate question**, and the doc now says so rather than leaving it to be
  inferred. §3.4 decided it by recording rather than refusing; a dirty build still emits an artifact,
  so the two never interact. They look adjacent enough that someone will try to unify them, and doing
  so would make one of the two refusals unavailable.

  📌 **The test double gained a deliberate pair.** `Fake::produces` answers `file_exists` — *is a
  binary there?* — and `cargo_built` answers the new check — *did this build make it?* A real build
  says yes to both, and the whole of the stale case is a `Fake` that says yes to the first and no to
  the second. One combined switch would have made the case this change exists for unrepresentable.

  🚨 **And the new variant exposed that `every_refusal_says_what_to_do` had stopped being true.**
  Its list of faults is written by hand; Rust checks the *enum* and nothing checked the *list*, so
  three variants added by T5 — `NoBinary`, `ChannelFailed`, `LaunchFailed` — were never added to it.
  A test promising **every** refusal was asserting about fifteen of eighteen, and it passed the
  whole time, which is what made it invisible. A **one-way table**: the direction the compiler helps
  with stays correct and the other rots silently. `all_variants_listed` is the compiler's half now —
  an uncalled `match` with **no wildcard arm**, so adding a variant stops the file compiling until
  it is listed, on `module.rs`'s coherence-tripwire precedent. ⚠️ A `_ =>` arm would restore the
  defect exactly, which is why the comment says so.

  ✏️ **The doc's count was already wrong before this change**, and recounting rather than
  incrementing is what caught it: the paragraph said seventeen when `main` had eighteen. ⚠️ Counting
  it *by a pattern* then nearly produced a second wrong number — the obvious regex matches `Name {`
  and `Name,` and silently skips the one tuple variant, `Module(ModuleFault)`. 📌 A count in prose is
  a promise to re-measure it, **and the measurement needs checking too.**

  ⚠️ **One near-miss worth recording because a compiler warning caught it, not a test.** Inserting
  the new test immediately above `fn every_refusal_says_what_to_do` put it between that function and
  its own `#[test]` attribute — so the new test had two, and **the old one silently stopped being a
  test at all**. `warning: duplicated attribute` was the only sign; the suite stayed green and the
  count did not move, because one test was gained exactly as another was lost. Anchor above the
  attribute, not below it.

  ⚠️ **An insertion can invalidate a comment it does not touch, and the diff shows nothing moved.**
  Putting the new test above `every_refusal_says_what_to_do` reassigned that function's doc comment
  to it — two added functions, no modified comment, and a contract claim silently relocated onto
  code that does not check it. 📌 Same family as `§4.7`'s *"verifies the binary exists"* becoming
  half-true when the check gained a second half, reached from the opposite direction: there, adding
  **elsewhere** falsified a sentence; here, adding **between** reassigned one. Caught in review,
  after the compiler had already caught the attribute-stealing half of the same insertion.
