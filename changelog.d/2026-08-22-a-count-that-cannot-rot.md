### Process

- **`CONTRIBUTING.md` stops naming a test count, because the one it named was wrong within a day.**
  It read *"the root crate's 324 lib tests never run"*; the figure was 324 when written, 332 that
  evening and 336 the next morning, because several sessions merge in parallel here.

  📌 The count was never the point — *that the fourth command does not run them* is the point, and
  it is true at every count. A literal there has to be re-measured by whoever notices, and the
  person most likely to notice is a contributor deciding whether they have just found a regression.
  It now says "several hundred", with the history recorded beside it so the next person can see
  why it is deliberately not a number.

  ⚠️ Found by the worker whose PR re-ran the bar and got **893 / 635 / 68 / 21 / 336** against a
  brief expecting 853 / 594 / 66 / 19 / 324 — every count stale. It flagged the durable doc rather
  than editing it, which was the right call: the bar's two published copies are pinned against each
  other by `.claude/hooks/bar-agreement-check.sh`, and that hook covers the **command block** only,
  so prose is exactly where a well-meant edit can drift unchecked. The hook passes on this change.
