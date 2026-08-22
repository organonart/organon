### `main` is protected, and the ruleset that protects it is a file you can read

`main` had nothing in front of it — no ruleset, no legacy branch protection — on a
public repository. `.github/rulesets/` now carries the two rulesets as JSON, written
against the REST API's create-ruleset body so the file *is* the request:

```bash
gh api --method POST repos/organonart/organon/rulesets --input .github/rulesets/main.json
gh api --method POST repos/organonart/organon/rulesets --input .github/rulesets/release-tags.json
```

`main.json` blocks deletion and force-pushes and requires a pull request, with every
review thread resolved before merge — which is what turns the automated review cycle
into a gate rather than a suggestion. `release-tags.json` stops a published `v*` tag
being deleted or moved; there are zero tags today, which is the reason to land it now
rather than after the first release.

⚠️ **A file here is a record, not the enforcement.** The ruleset lives in repository
settings; this directory is the source it was built from. Editing one without the other
is how the two drift, so treat them like the architecture docs and change both in the
same PR. `gh api repos/organonart/organon/rules/branch/main` is the check that matters —
it asks what rules the *branch* has, so it catches a ruleset whose condition never
matched, which from the settings page looks identical to one that works.

📌 **`required_approving_review_count` is 0, and that is not an oversight.** GitHub will
not let you approve your own pull request and `CODEOWNERS` names one maintainer, so any
non-zero count makes `main` unmergeable by the only person who can merge to it. Zero
still buys the whole gate: no direct pushes, a diff and a checks page per change, a
revertable merge commit. It goes to 1 — along with `require_code_owner_review` — the day
a second maintainer has write access.

**Four rules are deliberately off, each with the trigger that would turn it on.** The
sharpest is `required_status_checks`: `ci.yml` is `paths-ignore`-filtered, so a
docs-only PR never starts the matrix and a required check would wait forever for a
report that never comes. That deadlock is already documented in the workflow's own
header, and this ruleset honours the decision rather than quietly overriding it from
another file. `required_linear_history` is off because half of `main` is merge commits;
`required_signatures` is off only until commit signing works on every machine that
pushes here, and it is the one most worth revisiting.

**`.github/rulesets/README.md` also carries the part a ruleset cannot do** — secret
scanning with push protection, private vulnerability reporting (which `SECURITY.md`
already links to, and which 404s if the setting is off), auto-delete of merged head
branches (currently off, and the mechanism invariant 5 depends on for retargeting a
stacked PR), fork-workflow approval for all external contributors, and SHA-pinning the
three third-party actions.

🚨 **One live exposure is recorded there rather than fixed here.** `claude.yml`'s `if:`
tests whether a comment *contains* `@claude`, not who wrote it. `issue_comment` runs
from the default branch with full access to repository secrets — including on pull
requests from forks — so on a public repository any user who can type a comment can
spend `CLAUDE_CODE_OAUTH_TOKEN` and drive a job holding `contents: write`. The fix is an
`author_association` test on each arm; the README has the snippet. Recorded, not
silently patched, because it is a change to what the workflow will answer and that is a
decision rather than a typo.
