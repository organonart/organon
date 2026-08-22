### The ruleset is live, and applying it found two errors in the file that described it

`main` is protected as of 2026-08-21: ruleset **21175994** (`deletion`,
`non_fast_forward`, `pull_request`), with **21175996** covering `refs/tags/v*`. Both are
`enforcement: active` with `current_user_can_bypass: "never"`, and an unauthenticated
`GET /repos/organonart/organon/rules/branches/main` returns all three rule types — the
check that proves the rules reached the *branch*, not merely that a ruleset exists.

🚨 **That endpoint is also the first correction: it is `rules/branches/…`, plural.**
`.github/rulesets/README.md` and the previous changelog fragment both said
`rules/branch/main`, which is a plausible-looking 404 — so the one command offered as
"the check that matters" did not work, in the document whose entire job was to make the
ruleset verifiable. Found by running it. Fixed in both files.

🚨 **The second: on Windows, PowerShell redirection silently corrupts the request body.**
`>` and `Out-File` default to UTF-16LE with a BOM in PowerShell 5.1, and `gh` forwards
those bytes verbatim, so GitHub answers `400 Problems parsing JSON` — an error that
reads like a malformed ruleset when the ruleset is fine. The README now says to fetch
with `curl.exe -o` and gives a first-four-bytes check (`7B` good, `FF FE` the BOM),
spelled the long way because `Format-Hex -Count` is PowerShell 6+.

⚠️ **GitHub filled in two `pull_request` parameters the file did not set**, so the file
no longer round-tripped to the live ruleset — exactly the drift the README warns about.
Both are now pinned explicitly. `allowed_merge_methods` came back as all three, which is
what "unconstrained" looks like and changes nothing. The other is
`require_extra_approval_for_unattributed_changes: true`, and it is the one parameter
here whose interaction with `required_approving_review_count: 0` is **not established**:
if a pull request carries commits GitHub cannot attribute to its author, it may demand
an approval a solo maintainer cannot supply. Kept `true` — the protection is real and
the deadlock is hypothetical — with the symptom written down so it is recognisable: a
merge button asking for an approval when the required count is zero.

📌 **This pull request is also the first live test of the gate.** It is the first change
to reach `main` through the ruleset rather than past it, so whether it merges cleanly is
the evidence for the paragraph above.
