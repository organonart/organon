### Deep research evals — the repository as the fixed content

An eval normally holds the content fixed and varies the model, to score the model. This
inverts it: **the repository is the content, several models are run over it, and the
reports are kept as artifacts of the repository.** `doc/research/` is the new home —
briefs (the questions, hand-written and reviewed), reports (raw model output), and
`FINDINGS.md` (claims adjudicated against the tree).

`native/tools/research-brief.py` welds a brief to a **fact pack measured at dispatch** —
commit, crates, binaries and their feature gates, catalog counts, the durable docs, the
last thirty commits — so a report can always be pinned to the tree it actually saw. The
dispatch prompt itself is deliberately **not** checked in: it changes on nearly every
commit, so committing it would produce either constant churn or a drift test that always
fails, and both train people to ignore it. Briefs and reports are checked in; the thing
between them is a build artifact.

Four briefs ship. Only one of them — `doc-code-fidelity`, a documentation-drift audit —
has ground truth in the tree, and saying so is the point: it is the leg that scores the
models (precision over the claims they marked `verified`) at the same time as it scores
the docs. `architecture-critique` and `newcomer-comprehension` are partly checkable;
`product-landscape` is judgement about the outside world and is not scored at all.
Pretending all four were evals is how this would have become theatre.

⚠️ **The hazard this is built around.** A model's essay about the codebase is the least
trustworthy document in a repository whose `ARCHITECTURE.md` is injected into every AI
session and whose `doc/reference/` is generated and drift-tested — and from a directory
listing, an agent cannot tell them apart. So every report carries front matter declaring
its `status` (`unreviewed` / `adjudicated` / `superseded`) and the commit it describes, a
CI job refuses a report that does not, and `FINDINGS.md` — not the reports — is what later
work cites. It is the same posture Mind takes toward every quantity it displays: an
unlabelled number is worse than a missing one.

**One leg is automated: `.github/workflows/research.yml` hands a brief to Claude with the
repository checked out.** File access is what makes it worth having — a model that can
only read a prompt must guess about source, while this one greps the tree, which is the
difference between an essay about the documentation and an audit of it. It is the only
configuration in which `doc-code-fidelity`, scored on precision and demanding both halves
of every finding quoted with `path:line`, can honestly be attempted.

⚠️ **The model never holds a writable token.** The research job runs at `contents: read`;
attaching to a release and opening the pull request that lands the report happen in a
separate job with no model in it, consuming an artifact. That is the boundary
`claude-review.yml` already relies on and documents: the tool allowlist is not the security
boundary, `permissions:` is.

**A report becomes an artifact of the repository by being merged, not by a bot committing
it.** The publish job opens a *draft* PR whose body says outright that merging files the
evidence and does not endorse it. `status: unreviewed` is what that PR asks a person to
change.

`.github/workflows/research.yml` validates the contracts on every PR that touches the
directory, and builds the dispatch prompts on demand and on every published release
(attached to the release, and pasted into the job summary to copy) for the models CI cannot
reach — the eval premise wants several labs, and one vendor's token buys one vendor's
opinion. Adjudication stays a human step in every case: it is the half that produces the
value, and automating dispatch while leaving it undone is what would fill the directory
with unreviewed essays. **No research run fires on a pull request** — one per PR would fill the
directory faster than anyone could adjudicate it — and a hosted round still wants several
labs, which one vendor's token cannot buy.

A **local** model running continuously is recorded as a later tier rather than built. The
argument for it is economic, not technical: every leg here is priced per run, which makes a
round an event. Zero marginal cost buys something no vendor sells at a sensible price —
a standing check on every commit, most of which find nothing, which is exactly why nobody
would pay for them and exactly why they are worth doing when free.
