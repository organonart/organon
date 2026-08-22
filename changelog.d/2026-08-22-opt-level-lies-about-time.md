### Process

- 🚨 **The bar's fast-test flag is safe for logic and dangerous for time, and both published copies
  now say so.** `CARGO_PROFILE_TEST_OPT_LEVEL=0` turns ~43 minutes into ~70 seconds and is
  codegen-only — so no test's *verdict* changes, **unless the test's subject is time**, in which
  case an unoptimised binary is simply a different experiment.

  Measured 2026-08-22, and the cost was nearly real: the module staleness rig's simulator cannot
  draw 1280×720 in 4 ms unoptimised, so every cadence in a sweep collapsed to one achieved period,
  **the lever was connected to nothing**, and the rig concluded *"staleness is the TRANSPORT"* — a
  recommendation to buy `unsafe` per-backend GPU interop, on a false premise, from a green run made
  exactly as `CONTRIBUTING.md` advised.

  📌 The two rules that follow are the durable part. **A timing rig must measure the quantity it
  varies rather than the knob it set** — read the achieved period off the data, never off the flag
  — and it must **fail naming the real cause** when the sweep did not sweep. The corrected rig now
  confirms the model on an unoptimised run *and* fails separately with the right diagnosis.

  ⚠️ Added to prose in both `CONTRIBUTING.md` and the worker-facing `BRIEF.md`.
  `bar-agreement-check.sh` pins the two copies on their **command block** only — deliberately,
  since the surrounding paragraphs address different readers — so prose is exactly where the two
  can drift unchecked. Both were edited in this change and the hook passes.
