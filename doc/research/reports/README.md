# Reports

What the models returned, one file per run. [`../README.md`](../README.md) owns the
system; this file owns the filing rules.

⚠️ **A report is evidence of what a model said, not a statement about this codebase.**
Nothing here is authoritative. The checked, adjudicated version of a claim lives in
[`../FINDINGS.md`](../FINDINGS.md), and that is what later work should cite.

## Filing one

Name it `<brief>-<model>-<YYYY-MM-DD>.md`, lowercase, with the model slug written the way
the vendor writes it (`gpt-5`, `claude-opus-5`, `gemini-3-pro`). If the same model is run
twice in a day, append `-2`.

Front matter is required and `--validate` enforces it:

```markdown
---
brief: doc-code-fidelity
model: claude-opus-5
model_surface: deep research          # the product/mode used, not just the model
run_date: 2026-08-14
commit: 4f2c1ab                       # the SHA from the dispatch prompt's fact pack
status: unreviewed                    # unreviewed | adjudicated | superseded
adjudicated_by:                       # who checked it, once someone has
notes:                                # anything odd about the run
---
```

Then the report body, **unedited**, exactly as the model returned it — including the parts
that turn out to be wrong. A tidied report is no longer evidence. If a model ignored the
output contract and returned no `## Claims` section, that is itself a result: say so in
`notes:` and add the section yourself with a line recording that you extracted the claims
rather than the model emitting them.

`status: unreviewed` is the honest default and there is no shame in it. It says an opinion
is stored and nobody has checked it, which is true of a report the moment it arrives.

## Rounds

| Round | Brief | Commit | Models | Status |
|---|---|---|---|---|
| — | — | — | — | no rounds run yet |

Add a row per round when its reports land, so the history of what was asked and when stays
readable without listing the directory.
