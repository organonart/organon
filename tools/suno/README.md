# Generating tracks with Suno, automatically

`suno_track.py` takes a style prompt and writes an MP3. It rides **your own Suno
subscription** through the private endpoints the website itself uses.

```bash
export SUNO_COOKIE='<the whole Cookie header from a suno.com request>'
./suno_track.py doctor
./suno_track.py generate --style "slow doom jazz, upright bass, brushed kit, 60bpm" \
    --instrumental --out track.mp3
```

Stdlib-only Python 3.10+. No npm, no Docker, no Playwright, no captcha-solving
subscription. It is one file so that when it breaks you can read the whole thing.

---

## Why yours stopped working

There is no official Suno API. As of 2026-08 Suno has an **invite-only partner
programme** and no self-serve developer portal — no key, no console, no docs, no
announced date. Everything that calls itself a "Suno API" is either a
reverse-engineered client against `studio-api.prod.suno.com`, or a paid relay
that runs such a client for you.

So anything you had working was reverse-engineered, and it broke for one of four
reasons. `doctor` tells you which, by walking the stages in order and stopping at
the first failure:

| Stage | What breaks it |
|---|---|
| `cookie parses` | You copied a logged-out or partial Cookie header, so there is no `__client` in it |
| `clerk session` | The cookie expired, **or** Suno moved its clerk-js and the pinned version string is now refused — retry with `--clerk-js auto` |
| `jwt issued` | Clerk resolved the session but would not mint a token: near-certainly a stale cookie |
| `billing reachable` | Auth is fine; you are out of credits, or rate-limited |
| `captcha gate` | **The likely one.** Suno now demands an hCaptcha solve before it will generate |

The fourth is the one that killed most self-hosted setups. `gcui-art/suno-api`,
the wrapper nearly everyone ran, answered it by adding a headless browser
(`rebrowser-playwright` + ghost-cursor) driving a paid **2Captcha** account, so
"free, on my own subscription" quietly became "free plus a captcha-solving
balance plus a browser in the loop". Its last commit is 2026-03-07 and its issue
tracker is a queue of auth breakages.

## CAPTCHA — the one thing this script will not do for you

`doctor` reports the gate; it cannot open it. That is deliberate: solving it
means either paying a human-powered solver service or automating a browser well
enough to look human, and both are large, fragile, and out of proportion to
"give me an MP3". Two ways through:

1. **Generate one track by hand** on suno.com. The gate usually stays down for a
   while afterwards, and the script works in that window. Good enough for
   occasional use.
2. **Pass a token.** Generate on suno.com with DevTools open, find the
   `POST /api/generate/v2/` request, copy the `token` field out of its JSON body,
   and hand it over as `--captcha-token`. Single use.

If neither is tolerable — if you want this unattended, in a pipeline, on a
schedule — stop self-hosting. A paid relay (kie.ai, sunoapi.org, PiAPI,
musicapi.ai, AIML API) absorbs exactly this maintenance for a few cents a track,
which is the whole product. The trade is that your prompts go through someone
else's account and you inherit their terms.

## Getting the cookie

1. Open <https://suno.com/create>, logged in.
2. DevTools → Network.
3. Click any request to `studio-api.prod.suno.com`.
4. Under Request Headers, copy the **entire** `Cookie` value.

It is a session credential: it authorises anything your account can do. Keep it
in your environment or a secret store, never in a committed file. It expires —
re-copying it is the first thing to try when `doctor` fails.

## Usage

```bash
# Description mode: one prompt, Suno writes the lyrics.
./suno_track.py generate --style "cold synthwave, analogue drift, no vocals" --instrumental

# Custom mode: your lyrics, style as tags.
./suno_track.py generate --title "Hyperscope" --style "post-rock, 7/8, build" \
    --lyrics-file lyrics.txt --out hyperscope.mp3

./suno_track.py credits
./suno_track.py fetch --ids <id>,<id>     # re-download a run that timed out
./suno_track.py generate --style "…" -v   # log every HTTP hop
```

Suno returns **two takes** per request. With `--out track.mp3` you get
`track.mp3` and `track-2.mp3`; without it, files are named from the title and
clip id.

## What is pinned, and will therefore rot

Three constants at the top of the script are version handshakes with no
discovery endpoint, so they are guesses that were right when this was written:

- `CLERK_API_VERSION` / `CLERK_JS_VERSION` — override via env or `--clerk-js auto`,
  which asks jsdelivr for the current clerk-js release.
- `MODELS` — the `mv` codes (`chirp-v5`, `chirp-crow` for v5, `chirp-fenix` for
  v5.5, …). `generate` walks the list when one is refused, so a retired code
  costs a retry. When they are all stale, watch a real generation in DevTools,
  copy the `mv` value, and pass `--model`.
- The `x-suno-client` / `sec-ch-ua` header block, copied verbatim from a client
  known to work. The user-agent says macOS while `x-suno-client` says Android;
  that mismatch is intentional upstream — it was measured to draw fewer
  CAPTCHAs.

## Status, and what the tests do and do not cover

```bash
python3 tools/suno/test_suno_track.py
```

`test_suno_track.py` is stdlib `unittest`, no dependencies, 26 cases. It covers
the part that is pure: cookie parsing (full header, bare JWT, and both rejection
paths), header and Clerk-query assembly, `Set-Cookie` absorption, slugging,
multi-take output naming, and the remedy text on each mapped HTTP status —
because `doctor`'s whole value is that a failure names its own cause.

⚠️ **It does not cover a single network hop, and no run has ever reached Suno.**
This was built in a sandbox with no egress to `suno.com`, and no cookie was ever
handed to it. So the endpoints, the payload shapes and the auth handshake are
written from the documented behaviour of a client known to work — reasoned, not
observed. The first real run is `doctor`, and it is built to tell you precisely
where it stands.
