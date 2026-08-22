### Process

- **The coordinator skill stops shipping test counts, because they went stale in three hours.**
  Leg 7 read **324** when the seven-leg bar was written and **332** the same night, across three
  merges. A worker handed a stale number has to decide whether it found a regression or an
  out-of-date brief, and the cheap wrong answer is to assume the brief. The bar now carries no
  expected counts at all; instead it tells a worker to **measure `origin/main` before changing
  anything** — by stashing, not by remembering.

  📌 It also now asks for the **pair**, before and after, rather than a single number. One count
  proves nothing; the *delta* is what says tests were added and none were lost. Two workers have
  reported "the bar is green" with counts identical to the baseline — once correctly, because
  their tests were in another target, and once because that target was the hole in the bar. The
  pair tells those apart; a single number cannot.
