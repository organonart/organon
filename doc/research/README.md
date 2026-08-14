# Deep research evals

**An eval usually holds the content fixed and varies the model, in order to score the
model. This inverts that: the repository is the fixed content, several models are run over
it, and what comes back is a first-class artifact of the repository.** The same machinery
answers both questions, which is the point — a round tells you something about the project
*and* something about the models, and neither is free.

The parts:

| | What it is | Checked in? |
|---|---|---|
| **Brief** | `briefs/<id>.md` — the question. Hand-written, reviewed like any other doc. | yes |
| **Fact pack** | measured repo state at dispatch: commit, crates, binaries, counts, the last 30 commits. | no — rebuilt every run |
| **Dispatch prompt** | brief + fact pack, welded by `native/tools/research-brief.py`. | no — a build artifact |
| **Report** | `reports/<brief>-<model>-<date>.md` — what a model returned. | yes |
| **Findings ledger** | [`FINDINGS.md`](FINDINGS.md) — claims from the reports, adjudicated against the tree. | yes |

## The hazard, first

A research report is **the least trustworthy document in this repository**, and it lives
next to the most trusted ones. `ARCHITECTURE.md` is injected into every AI session before
anything else happens; `doc/reference/` is generated from the Rust and a test fails the
build if it drifts. A model's essay about the codebase has neither property, and a future
agent skimming `doc/` has no way to know that from the directory listing alone.

So three rules, and they are the reason this directory is safe to have:

1. **Every report declares its status in front matter.** `unreviewed` means nobody has
   checked it — an opinion, stored. `adjudicated` means its claims have been checked
   against the tree and the results are in the ledger. `superseded` means a later run
   replaced it. A report without front matter fails `--validate` and cannot merge.
2. **Every report is pinned to a commit.** The tree moves; a claim with no commit cannot
   be re-checked, and re-checking is the entire point of keeping these.
3. **Agents read [`FINDINGS.md`](FINDINGS.md), not the reports.** The ledger holds claims
   that were checked, with the verdict attached. The reports are the raw material and are
   kept for provenance and for comparing models — they are not a source of truth about
   this codebase and must never be cited as one.

This is the same discipline Organon Mind applies to everything it displays
(`MIND_ARCHITECTURE.md` §3): a quantity carries how it was obtained, because an unlabelled
number is worse than a missing one. A checked-in essay by a language model is a `proxy` at
best. Label it.

## Which briefs are actually evals

Not all of them, and pretending otherwise is how this becomes theatre. The distinction is
whether the question has **ground truth in the tree**:

| Brief | Scoreable | What a round produces |
|---|---|---|
| [`doc-code-fidelity`](briefs/doc-code-fidelity.md) | **yes** | Every claim resolves to a file. Models get a precision score; the repo gets a drift list. This is the real eval leg. |
| [`architecture-critique`](briefs/architecture-critique.md) | partial | Structural claims are checkable; predictions are not — they are dated and re-read later. |
| [`newcomer-comprehension`](briefs/newcomer-comprehension.md) | partial | Stall points are checkable. A stall every model hits is a documentation defect; one only a single model hits is about that model. |
| [`product-landscape`](briefs/product-landscape.md) | **no** | Judgement about the outside world. Cross-model spread is a confidence signal, nothing more. |

For the unscoreable ones the value is the report itself. For the scoreable ones the value
is in the disagreements: **two models reading the same tree and reaching incompatible
conclusions is a finding either way** — one of them is wrong, or the tree is genuinely
ambiguous there, and both are worth knowing.

## Running a round

```bash
python3 native/tools/research-brief.py --list                      # what exists
python3 native/tools/research-brief.py --facts                     # the measured half alone
python3 native/tools/research-brief.py --brief doc-code-fidelity   # one prompt, to stdout
python3 native/tools/research-brief.py --all --out /tmp/dispatch    # all of them, to files
python3 native/tools/research-brief.py --validate                  # the CI gate
```

Then, per round:

1. **Dispatch.** Paste the prompt into each model's deep-research or long-context mode.
   Use at least three, from **different labs** — two models from one family agreeing tells
   you about the family, not about the repository.
2. **File the reports.** One file per model in `reports/`, named
   `<brief>-<model>-<YYYY-MM-DD>.md`, with the front matter [`reports/README.md`](reports/README.md)
   specifies. Paste the report **unedited** — a report you have tidied is no longer
   evidence of what the model said. Status starts at `unreviewed`.
3. **Adjudicate** (scoreable briefs only). Take the `## Claims` list from each report,
   check each against the tree at the pinned commit, and write the verdicts into
   [`FINDINGS.md`](FINDINGS.md). Flip the reports to `adjudicated`.
4. **Act.** A `confirmed` finding becomes an issue or a fix like anything else. The ledger
   records which round it came from.

The `Research dispatch` workflow (`.github/workflows/research.yml`) builds the prompts for
you — run it from the Actions tab, or let it fire on a published release — and attaches
them to the run. It validates on every PR that touches this directory.

## The local-model leg

One leg is automated, and it is the one that needs no vendor: a model **running on your
machine**, reached over loopback.

```bash
# LM Studio (1234) / Ollama (11434) / this repo's own organic-math-mind-runtime
python3 native/tools/research-run.py --brief doc-code-fidelity --model qwen3-8b
python3 native/tools/research-run.py --brief doc-code-fidelity --model llama3 \
    --endpoint http://127.0.0.1:11434/v1/chat/completions
python3 native/tools/research-run.py --brief doc-code-fidelity --model m --dry-run
```

It writes a report into `reports/` with the front matter filled in and `status:
unreviewed`. There was no convention to invent: `agent.rs` settled it, POSTing the
OpenAI-compatible shape to `http://127.0.0.1:1234/v1/chat/completions` by default — so
every backend the Performer agent already works with works here, and pointing it at
`organic-math-mind-runtime` makes an **Organon-hosted model audit Organon**.

**`http://` and loopback only, enforced.** A remote host is refused rather than
configured, which keeps "no script sends this repository to a vendor" true without anyone
having to trust a flag. For a hosted model's opinion, paste the prompt in by hand.

⚠️ **What a local leg can honestly be asked.** A model on `/v1/chat/completions` has **no
file access** — it sees the prompt and nothing else. So the runner packs the durable docs
alongside the fact pack and tells the model that `verified` is available only for text it
can actually see. It *can* check prose against the measured numbers and against other
documents: stale counts, internal contradictions, build claims the manifest data refutes.
It *cannot* verify anything about a source file it was never shown — and on
`doc-code-fidelity`, which is scored on precision, a confident source claim from a local
run is a hallucination and should be refuted on adjudication. **A local report that comes
back mostly `inferred` is the system working.**

The workflow's `local-run` job does the same on a self-hosted runner. It is opt-in twice
(`workflow_dispatch` only, then `local_run: true`), never fires on a PR or a release, and
uploads the report as an artifact rather than committing it — filing a report is a pull
request like any other change.

## Why the rest still has a human in the loop

The workflow builds prompts, checks contracts, and can drive a local model. It calls **no
hosted model**, and that is a decision rather than a gap.

- **The eval premise requires several labs.** One vendor's token buys one vendor's
  opinion, and a single-model round loses the cross-model signal that makes the scoreable
  briefs worth running at all. The local leg is one leg, not a round.
- **Deep research is mostly not an API.** The strongest versions of this capability are
  interactive products; automating the weakest available substitute would produce reports
  that look like the real thing and are not.
- **Adjudication is the expensive half, and it is judgement.** Deciding whether a claim
  holds against the tree is the step that produces the value. Automating dispatch while
  leaving that undone would fill this directory with unreviewed essays, which is the
  failure mode the status field exists to make visible.

A hosted-model leg — a Claude Code run over the same dispatch prompt, posting an
`unreviewed` report — is a reasonable later increment, and it is deliberately not tier
one. New capability starts inert here.

## Adding a brief

Copy the shape of an existing one. Required front matter: `id` (matching the filename),
`title`, `one_line`, `scoreable` (`yes`/`partial`/`no`), `cadence`, and — if scoreable is
`yes` or `partial` — `ground_truth`, one line saying where an adjudicator checks. Required
sections: `## Question`, `## Scope`, `## Method`, `## Deliverable`. `--validate` enforces
all of it, and runs in CI.

Two things that make a brief good rather than merely long:

- **Ask for something falsifiable.** "Assess the architecture" returns an essay.
  "Sort these constraints into load-bearing, inherited and accidental, and justify each
  from the code" returns claims you can check.
- **Name the failure mode you expect.** Every brief here tells the model which specific
  wrong answer it is at risk of producing. That instruction does more work than any amount
  of added scope.
