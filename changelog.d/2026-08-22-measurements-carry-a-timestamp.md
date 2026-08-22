### A measurement of a moving artifact carries a timestamp whether or not it prints one

- The coordinator skill gains the sharpest rule of the night, and it was learned the expensive way:
  a coordinator reported a PR review thread as unanswered, and the worker's reply had landed
  **sixteen seconds** after the read. The reading was accurate and stale, and it was delivered as
  though it were a reliability problem in the worker rather than a timestamp problem in the report.

  🚨 **The fix is not "re-measure before acting" — that only helps the person who does it. It is:
  when you report a measurement of someone else's branch, say what commit you read it at.**
  `origin/module/verbs @ 4ad11f5` would have been recognised as stale in one glance instead of
  having to be re-derived from the other side.

  ⚠️ With a companion trap that makes the misreading easy: **`gh` authenticates as the user, so a
  worker's PR reply and the coordinator's are indistinguishable by author.** A thread whose authors
  read `[github-actions, <user>, <user>]` may be two of one and none of the other. Never infer
  "nobody replied" from an author list — read the bodies and the timestamps.
