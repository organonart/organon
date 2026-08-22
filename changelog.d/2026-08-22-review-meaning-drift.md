### Process

- **The review rubric learns the class that produced three defects in one night**
  (`.github/organon-review-guide.md`). None of the three failed a test; two were caught only by
  review, and the third only because someone rendered a picture and looked at it.

  🚨 **The shape: an edit that changes what a value *is*, where a comment *sits*, or what a guard
  *covers* does not have to touch the line that then becomes wrong.** A producer name checked by
  four rules — each a true statement about surviving a whitespace-delimited wire — became a
  **directory** name, and `..` satisfies all four. A comment naming its neighbour by position
  (*"the arm directly above"*) was made false by an unrelated insertion, during a clean merge, with
  no conflict. A say-it-once latch generalised from one refusal to two meant the first occurrence
  of either silenced the other permanently — and the new kind was routine by design, so a genuine
  fault an hour later was refused in total silence.

  📌 Reviewing the diff alone cannot catch any of them; the rubric now says to ask what the
  *unchanged* code was relying on. It also gains the doc-comment case — a helper inserted beside
  its caller can land between an existing doc block and the function it documents, re-homing a
  whole argument onto a two-line lookup — which happened here and was caught.

  ⚠️ And a reporting check: **"the bar is green" and "my tests ran" are different claims.** If a PR
  adds tests under `native/src/` and every reported count sits at baseline, its own tests very
  likely did not run.
