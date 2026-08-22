# Rulesets — what protects `main`, and why each rule is the one it is

Two JSON files here, both written against the **REST API's create-ruleset body**
(`POST /repos/{owner}/{repo}/rulesets`), which is also the shape GitHub's *Import a
ruleset* button accepts:

| File | Target | What it stops |
|---|---|---|
| `main.json` | the default branch (`~DEFAULT_BRANCH`) | direct pushes, force-pushes, deletion |
| `release-tags.json` | `refs/tags/v*` | a published release tag being deleted or moved |

⚠️ **GitHub is the authority, not this directory.** A ruleset lives in repository
settings; these files are the *source* they were created from and the record of the
reasoning. Editing a file here changes nothing until someone re-applies it, and editing
the ruleset in the web UI silently makes the file here stale. If you change one, change
both in the same PR — the same discipline the architecture docs are under.

## Applying them

The CLI path is exact — the file *is* the request body, so there is no transcription
step and nothing to get wrong:

```bash
gh api --method POST repos/organonart/organon/rulesets --input .github/rulesets/main.json
gh api --method POST repos/organonart/organon/rulesets --input .github/rulesets/release-tags.json
```

The web path: **Settings → Rules → Rulesets → New ruleset → Import a ruleset**, then
pick the file. Both need repository **admin**; a token with only `contents` gets a bare
`403 Resource not accessible by integration`, which is what an agent session sees.

Verify afterwards — an active ruleset that matches nothing looks identical to a working
one from the settings page:

```bash
gh api repos/organonart/organon/rulesets --jq '.[] | {id, name, target, enforcement}'
gh api repos/organonart/organon/rules/branch/main --jq '[.[].type]'   # what actually applies to main
```

That second call is the one that matters. It asks *"what rules does this branch have"*
rather than *"what rulesets exist"*, so it catches a condition that never matched.

📌 **There is no emergency bypass, on purpose.** `bypass_actors` is empty, so the rules
apply to the admin too — which is the entire point when the admin is also the only
person who could bypass them. The valve is `enforcement`: an admin can flip a ruleset to
`disabled` (or `evaluate`, which reports without blocking) in one click, do the thing,
and flip it back. That leaves a trail in the audit log; a standing bypass does not.

## `main.json`, rule by rule

**`deletion` — the branch cannot be deleted.** Cheap and absolute. Without it, `main` is
one mis-aimed `git push --delete` or one API call from gone.

**`non_fast_forward` — no force-pushes.** The rule that actually protects history. A
force-push to `main` rewrites commits other clones already have and silently drops
whatever landed in between.

**`pull_request` — every change to `main` arrives as a pull request.** This encodes what
`CLAUDE.md` invariant #5 and `CONTRIBUTING.md` already ask for in prose (branch off
`main`, PR it, merge to `main`). Its parameters are where the judgement is:

- **`required_approving_review_count: 0`.** Deliberate, and the thing most likely to
  look like an oversight. GitHub will not let you approve your own pull request, and
  `CODEOWNERS` names exactly one maintainer — so any non-zero count makes `main`
  unmergeable by the only person who can merge to it. Zero still buys the whole gate:
  no direct pushes, a diff and a checks page for every change, a revertable merge
  commit. **Raise this to 1 the day a second maintainer has write access**, and not
  before.
- **`required_review_thread_resolution: true`.** Every review conversation must be
  resolved before merge. This is the rule that makes the automated review cycle
  (`.github/workflows/claude-review.yml`) load-bearing instead of advisory: its inline
  findings have to be answered or dismissed rather than scrolled past, which is what
  `CONTRIBUTING.md` says the review cycle is for. It is also the rule most likely to
  irritate you on a busy PR — if it does, turn *this* off rather than weakening the
  gate itself.
- **`dismiss_stale_reviews_on_push: true`.** Inert at zero required approvals, correct
  the moment that count goes up, and free either way.
- **`require_last_push_approval: false`.** At zero approvals it has nothing to dismiss;
  at one approval with a solo maintainer it would deadlock (your own last push would
  need someone else's blessing).
- **`require_code_owner_review: false`.** `CODEOWNERS` lists one name on every line, so
  requiring code-owner review is the self-approval deadlock again, wearing a different
  hat. It flips to `true` alongside the approval count when ownership is genuinely
  shared — that is what the file was drawn for.

**Merge methods are not constrained.** The `pull_request` rule can restrict them; this
one does not, so the repository's own settings (merge, squash and rebase all enabled)
continue to decide. `main` is 103 merge commits out of 216, so a rule that quietly
forbade merge commits would break the established workflow — new capability defaults to
inert.

## What is deliberately NOT in `main.json`

Four rules that a generic hardening guide would tell you to turn on, and the specific
reason each is off here. Each has a stated trigger for revisiting it — an absence with
no trigger is just an oversight with better prose.

🚨 **`required_status_checks` — omitted, because it would deadlock docs-only PRs.**
`.github/workflows/ci.yml` is `paths-ignore`-filtered: a PR whose whole diff is prose
never *starts* the matrix, so its checks never report. A required check that never
reports is not a slow merge, it is a permanent one — GitHub waits forever. The
workflow's own header states the decision ("these checks are NOT marked required … they
are advisory — read the run, then merge") and this ruleset honours it rather than
quietly overriding it from another file.

> **The trigger.** Breaking the deadlock takes one of two changes, both in `ci.yml`, and
> the rule can only be added *after* one of them lands: drop `paths-ignore` entirely and
> let every PR pay the matrix, or add a companion workflow carrying the same `name:` and
> job names with the *inverse* path filter and a body that just reports success. The
> second is the standard pattern and the cheap one.
>
> ⚠️ Until then, **not required means you have to look.** Nothing blocks a merge on a
> red run.

**`required_linear_history` — omitted, because this repository merges.** Half of `main`
is merge commits, produced by the "merge pull request" button the workflow depends on.
Turning this on would force squash-or-rebase and reject the merge commits the history is
already made of. *Trigger:* only if the project deliberately switches to squash-merging,
in which case the rule stops being a behaviour change and becomes a lock on a decision
already made.

**`required_signatures` — omitted, but the closest to worth having.** Requiring signed
commits is the strongest single defence against a commit forged under your name, and on
a public repo that is a real threat rather than a theoretical one. It is off here only
because it is not free: every machine you commit from needs SSH or GPG signing
configured first (`git config --global gpg.format ssh` plus a signing key uploaded to
GitHub), and any commit pushed by a workflow or an agent must come through the GitHub
API to be signed. Turn it on as a deliberate step, after signing works on every machine
you actually use — not in the same change as everything else here, or the first failure
will be indistinguishable from a broken ruleset.

**`creation` and `update` — omitted, and they are not what they sound like.** Neither is
a branch-protection rule in the sense wanted here; on a default-branch ruleset they only
serve to block the branch's own creation, which already happened.

## `release-tags.json`

There are zero tags today, which is exactly why this is worth landing now: the rule has
to exist *before* the first `v*` tag to have protected it. `research.yml` already
triggers on `release: published`, so releases are planned rather than hypothetical.

It blocks deletion and force-updates of `refs/tags/v*` — the two ways a published
release quietly starts pointing at different code than the one people downloaded. Tag
*creation* is untouched, so cutting a release works normally.

## The rest of the checklist — what no file in this repository can carry

A ruleset protects the branch. It does not protect the repository. Everything below is a
settings toggle or a workflow edit, in rough order of how much it buys.

### Now, in Settings

1. **Settings → Code security → enable Secret scanning *and* Push protection.** Free on
   public repos. Push protection is the half that matters: it rejects a commit
   containing a recognised credential at push time instead of telling you about it
   afterwards, by which point the secret is public and must be rotated regardless.
   This repository holds `CLAUDE_CODE_OAUTH_TOKEN` in Actions secrets, so a paste into
   a workflow file is a plausible accident, not a paranoid one.
2. **Settings → Code security → enable Private vulnerability reporting.** `SECURITY.md`
   already tells reporters to use
   `https://github.com/organonart/organon/security/advisories/new`. **If the setting is
   off, that link 404s** and the documented private-disclosure path silently does not
   exist — worth checking first, because the failure is invisible from the outside.
3. **Settings → Code security → enable Dependabot alerts and security updates.** Alerts
   are the free half; the updates half opens PRs for vulnerable dependencies only, which
   is low-noise.
4. **Settings → General → Pull Requests → tick "Automatically delete head branches".**
   Currently off (`delete_branch_on_merge: false`). This is not tidiness — `CLAUDE.md`
   invariant #5 warns that a PR stacked on another PR's branch can land on a dead branch
   and never reach `main`. Auto-delete is what makes GitHub retarget a child PR when its
   base merges, which is the mechanism that invariant depends on.
5. **Settings → Actions → General → Fork pull request workflows → "Require approval for
   all external contributors."** The public-repo default only gates *first-time*
   contributors; once someone has one merged PR, their later fork PRs run runners
   automatically.
6. **Settings → Actions → General → Workflow permissions → "Read repository contents
   permission"**, and untick **"Allow GitHub Actions to create and approve pull
   requests"**. Every workflow here already declares its own `permissions:` block, so
   the restrictive default costs nothing and closes the gap for the next workflow, whose
   author might forget.

### Now, in the workflows

🚨 **`claude.yml` has no actor guard, and on a public repo that is the live exposure.**
Its `if:` only tests whether the comment *contains* `@claude` — not who wrote it. An
`issue_comment` event runs from the default branch **with full access to repository
secrets, including on pull requests from forks**, so any GitHub user who can type a
comment can spend `CLAUDE_CODE_OAUTH_TOKEN` and drive a job holding `contents: write`.
Nothing about the workflow being on the default branch prevents this; that property only
stops a fork from editing the workflow itself. Add an association test to each arm:

```yaml
    if: |
      (github.event_name == 'issue_comment'
        && contains(github.event.comment.body, '@claude')
        && contains(fromJSON('["OWNER","MEMBER","COLLABORATOR"]'), github.event.comment.author_association))
      || …
```

📌 **Two workflows that look risky and are not — recorded so nobody "fixes" them.**
`label.yml` uses `pull_request_target`, the trigger normally flagged as dangerous; it is
safe **because it checks out no code and runs none**, which its own header says and a
`grep -c actions/checkout .github/workflows/label.yml` confirms is still true.
`claude-review.yml` uses plain `pull_request`, which gets no secrets from a fork — so it
will simply not review outside contributions, and that is the safe failure direction.

**Pin third-party actions to commit SHAs.** `Swatinem/rust-cache@v2`,
`dtolnay/rust-toolchain@stable` and `anthropics/claude-code-action@v1` are all mutable
refs, and `@stable` is a *branch* — whoever controls that repository controls what runs
against this one's secrets. Pinning to a SHA with the version in a trailing comment
(`uses: Swatinem/rust-cache@<sha>  # v2.7.3`) makes the supply chain immutable, and
Dependabot's `github-actions` ecosystem keeps the pins current so they do not rot:

```yaml
# .github/dependabot.yml
version: 2
updates:
  - package-ecosystem: github-actions
    directory: /
    schedule: { interval: weekly }
```

⚠️ Adding that file turns version-update PRs on the moment it merges — a deliberate
choice about PR volume, which is why it is a snippet here rather than a committed file.
A `cargo` entry is the obvious next one, but weigh it first: this is a large workspace
and `nih-plug` is a git dependency, so the noise is real. `cargo audit` / `cargo deny`
in CI answers the security half without the PR stream.

### Already done — no action needed

`SECURITY.md`, `CONTRIBUTING.md`, `CODEOWNERS`, `LICENSE*` + `LICENSING.md` + `NOTICE`,
issue templates and a `permissions:` block on all five workflows. That is the public-repo
paperwork most projects are missing; this one is not.

### One thing that is not a repository setting at all

**Require 2FA for the `organonart` organisation** (Organisation settings → Authentication
security), and check that the account owning this repository is not also the account
running unattended tokens. A branch ruleset is enforced by GitHub, so it is exactly as
strong as the authentication in front of the account that can turn it off.
