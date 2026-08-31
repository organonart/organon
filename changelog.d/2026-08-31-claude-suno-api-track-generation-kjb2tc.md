### A style prompt in, an MP3 out — Suno automation that says what broke

`tools/suno/suno_track.py` takes a style prompt and writes a track. It rides a Suno
subscription through the same private endpoints `suno.com` uses, because **there is no
official Suno API**: as of 2026-08 Suno runs an invite-only partner programme with no
self-serve portal, no key, no console and no announced date. Everything that calls itself
a "Suno API" is therefore either a reverse-engineered client against
`studio-api.prod.suno.com` or a paid relay running one for you, and this is the first kind.

⚠️ **A reverse-engineered client is a maintenance commitment, not a feature.** The thing
worth building was never the happy path — that is forty lines — but the part that tells you
*which* handshake Suno changed this month. `doctor` walks the stages in order and stops at
the first failure: does the cookie parse, does Clerk resolve a session from it, does it mint
a JWT, is billing reachable, is the CAPTCHA gate down. Each failure carries its own remedy,
because "it stopped working" spans a stale cookie, a moved `clerk-js` version, an exhausted
credit balance and a captcha wall, and those want four different responses.

📌 **The pinned constants are the rot, and they are pinned in one place on purpose.** Two
Clerk version strings and the `mv` model codes (`chirp-v5`, `chirp-crow`, `chirp-fenix`, …)
have no discovery endpoint, so they are guesses that were right when written. `--clerk-js
auto` asks jsdelivr for the current `clerk-js` rather than trusting the pin, and `generate`
walks the model list when the server refuses one — so a retired code costs a retry instead
of a failure, and a wholly stale list fails with the DevTools recipe for finding the new
one rather than with a 400.

🚨 **The CAPTCHA gate is reported, never solved, and that is the design.** Suno demands an
hCaptcha solve before generation. Answering it in code means either paying a
human-powered solver or automating a browser convincingly enough to pass for one —
`gcui-art/suno-api`, the wrapper most self-hosted setups ran, does both, which is how
"free, on my own subscription" became "plus a 2Captcha balance plus `rebrowser-playwright`
in the loop"; its last commit is 2026-03-07 and its tracker is a queue of auth breakages.
Neither belongs behind a one-file script, so `doctor` names the gate and the README gives
the two ways through it — generate one track by hand to clear it for a while, or lift the
token out of DevTools and pass `--captcha-token`. Wanting this unattended is the honest
signal to stop self-hosting and pay a relay, and the README says so plainly instead of
implying an automation that does not exist.

Stdlib-only Python, one file, no npm and no Docker — the point is that when it breaks you
can read all of it. Unit-tested for cookie parsing, header and auth-flow assembly, and
multi-take output naming; **not run end to end**, because the sandbox it was written in has
no egress to `suno.com` and was never given a cookie. The README's status section says that
in those words rather than implying a green run.
