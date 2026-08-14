# Findings ledger

**The adjudicated output of the research rounds — and the only part of
`doc/research/` that later work should cite.** The reports next door are raw model output
and are kept as evidence; a claim earns its place here by being checked against the tree.

If you are an agent picking up work in this repository: read this file, not the reports. A
row below has been verified by a person against a specific commit. A paragraph in a report
has not.

## How a row gets here

1. A round is dispatched — the same brief to several models from different labs.
2. Each report ends with a numbered `## Claims` list, one line each, self-labelled
   `verified` / `inferred` / `speculative` by the model that wrote it.
3. Someone checks each claim against the tree at the pinned commit and writes a verdict.
   **The model's own label is not the verdict** — a claim a model called `verified` and
   that turns out to be wrong is the most valuable row in the table, because it is the one
   that calibrates how much the next report is worth.

Verdicts:

| Verdict | Means |
|---|---|
| `confirmed` | Checked against the tree and true at that commit. |
| `refuted` | Checked and false. Keep it — a refuted claim is the score. |
| `open` | Cannot be settled from the repository (judgement, prediction, or about the outside world). Not a failure; some briefs are mostly this by design. |
| `fixed` | Was `confirmed`, and the thing it identified has since been changed. Links the commit or PR that closed it. |

## The ledger

| # | Round | Claim | Models | Verdict | Evidence | Outcome |
|---|---|---|---|---|---|---|
| — | — | *no rounds adjudicated yet* | — | — | — | — |

**Models** names every model that made the claim independently — that column is the
cross-model signal, and it is why the same brief goes to more than one lab. A claim three
labs found independently and that survives adjudication is about as strong as this system
gets. A claim one lab found, alone, that also survives is the most *interesting* row in the
table: it is the one the others missed.

## Scores

Per model, per scoreable round: **precision** is confirmed ÷ (confirmed + refuted) over
the claims that model labelled `verified`. **Unique-confirmed** counts the claims it found
that no other model did.

| Round | Model | Claims | Confirmed | Refuted | Open | Precision | Unique-confirmed |
|---|---|---|---|---|---|---|---|
| — | — | — | — | — | — | — | — |

⚠️ **These numbers are small-sample and about this repository only.** A model that scores
badly on a drift audit of one Rust workspace has not been shown to be worse at anything
else, and a table of two rounds is an anecdote with arithmetic on it. They are here to
decide which model to dispatch the *next* round to, and for nothing else.
