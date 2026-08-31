#!/usr/bin/env python3
"""Generate a track on Suno from a style prompt and save it as an MP3.

This rides YOUR OWN Suno subscription through the same private endpoints the
web app uses. There is no official Suno API (as of 2026-08 Suno has only an
invite-only partner programme), so this is a reverse-engineered client and it
will break whenever Suno changes its client. That is not a defect of this
script; it is the deal you take when there is no supported API.

    export SUNO_COOKIE='<the whole Cookie header from suno.com/create>'
    ./suno_track.py doctor
    ./suno_track.py generate --style "slow doom jazz, upright bass, brushed kit" \
        --instrumental --out track.mp3

Stdlib only: no npm, no Playwright, no Docker, no 2Captcha account. The cost of
that is the one thing this script cannot do for you — see CAPTCHA in README.md.
"""

from __future__ import annotations

import argparse
import http.cookies
import json
import os
import random
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

AUTH_BASE = "https://auth.suno.com"
API_BASE = "https://studio-api.prod.suno.com"
JSDELIVR = "https://data.jsdelivr.com/v1/package/npm/@clerk/clerk-js"

# Clerk pins. Both are version handshakes Suno's auth host checks, and both are
# the usual reason a client that worked last month stops working: Suno moves its
# clerk-js and the old version string starts being refused. `--clerk-js auto`
# asks jsdelivr for the current one instead of trusting these.
CLERK_API_VERSION = os.environ.get("SUNO_CLERK_API_VERSION", "2025-11-10")
CLERK_JS_VERSION = os.environ.get("SUNO_CLERK_JS_VERSION", "5.117.0")

# Model codes rotate and are not discoverable from any endpoint. First entry is
# the default; `generate` walks the list when the server rejects one, so a
# retired code costs a retry rather than a failure.
MODELS = ["chirp-v5", "chirp-crow", "chirp-fenix", "chirp-v4-5", "chirp-v3-5"]

# Kept byte-for-byte from the implementation these endpoints were learned from.
# The UA says macOS while x-suno-client says Android, which looks wrong and is
# deliberate: that combination was measured to draw fewer CAPTCHAs than a
# self-consistent one. Override with SUNO_USER_AGENT if you want to experiment.
DEFAULT_UA = os.environ.get(
    "SUNO_USER_AGENT",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
)


class SunoError(RuntimeError):
    """A failure with a human-readable cause and, where known, a remedy."""

    def __init__(self, message: str, remedy: str | None = None) -> None:
        super().__init__(message)
        self.remedy = remedy


def log(message: str) -> None:
    print(message, file=sys.stderr, flush=True)


def parse_cookie_header(raw: str) -> dict[str, str]:
    """Accept either a full Cookie header or a bare __client value."""
    raw = raw.strip()
    if not raw:
        raise SunoError(
            "SUNO_COOKIE is empty.",
            "Open suno.com/create, DevTools > Network, click any request to "
            "studio-api.prod.suno.com, copy the whole Cookie request header.",
        )
    if "=" not in raw:
        # Someone pasted just the JWT.
        return {"__client": raw}
    jar: http.cookies.SimpleCookie = http.cookies.SimpleCookie()
    try:
        jar.load(raw)
    except http.cookies.CookieError as exc:
        raise SunoError(f"SUNO_COOKIE is not a parseable Cookie header: {exc}")
    cookies = {key: morsel.value for key, morsel in jar.items()}
    if "__client" not in cookies:
        raise SunoError(
            "SUNO_COOKIE has no __client cookie — that is the one that carries "
            f"your session. Found: {', '.join(sorted(cookies)) or '(nothing)'}.",
            "Copy the Cookie header from a request to studio-api.prod.suno.com "
            "while logged in, not from a logged-out page.",
        )
    return cookies


class Suno:
    def __init__(self, cookies: dict[str, str], *, clerk_js: str, verbose: bool = False):
        self.cookies = dict(cookies)
        self.clerk_js = clerk_js
        self.verbose = verbose
        self.device_id = self.cookies.get("ajs_anonymous_id") or str(uuid.uuid4())
        self.session_id: str | None = None
        self.token: str | None = None
        self.ssl_context = ssl.create_default_context()

    # ---- transport ----------------------------------------------------

    def _headers(self, extra: dict[str, str] | None = None) -> dict[str, str]:
        headers = {
            "Affiliate-Id": "undefined",
            "Device-Id": f'"{self.device_id}"',
            "x-suno-client": "Android prerelease-4nt180t 1.0.42",
            "X-Requested-With": "com.suno.android",
            "sec-ch-ua": '"Chromium";v="130", "Android WebView";v="130", "Not?A_Brand";v="99"',
            "sec-ch-ua-mobile": "?1",
            "sec-ch-ua-platform": '"Android"',
            "User-Agent": DEFAULT_UA,
            "Accept": "application/json",
            "Cookie": "; ".join(f"{k}={v}" for k, v in self.cookies.items()),
        }
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        if extra:
            headers.update(extra)
        return headers

    def _request(
        self,
        method: str,
        url: str,
        *,
        body: dict | None = None,
        extra_headers: dict[str, str] | None = None,
        timeout: int = 30,
    ) -> dict:
        data = None
        headers = self._headers(extra_headers)
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        if self.verbose:
            log(f"  → {method} {url}")
        request = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(request, timeout=timeout, context=self.ssl_context) as response:
                payload = response.read()
                self._absorb_set_cookie(response.headers.get_all("Set-Cookie") or [])
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", "replace")[:800]
            raise SunoError(
                f"{method} {url} → HTTP {exc.code}. Body: {detail}",
                self._remedy_for_status(exc.code),
            ) from exc
        except urllib.error.URLError as exc:
            raise SunoError(f"{method} {url} → network error: {exc.reason}")
        if not payload:
            return {}
        try:
            return json.loads(payload)
        except json.JSONDecodeError:
            snippet = payload[:400].decode("utf-8", "replace")
            raise SunoError(
                f"{method} {url} returned non-JSON. First bytes: {snippet!r}",
                "An HTML body here usually means Cloudflare served a challenge "
                "page instead of the API. Retry from an IP Suno likes better, or "
                "refresh your cookie from a browser session on that IP.",
            )

    @staticmethod
    def _remedy_for_status(code: int) -> str | None:
        if code in (401, 403):
            return (
                "Your cookie is stale or was rejected. Re-copy the Cookie header "
                "from a live suno.com session. If it is fresh and still 403, Suno "
                "has changed its client handshake — try `--clerk-js auto`."
            )
        if code == 402:
            return "Out of credits on the account behind that cookie."
        if code == 429:
            return "Rate limited. Wait, then retry."
        return None

    def _absorb_set_cookie(self, set_cookie_headers: list[str]) -> None:
        for header in set_cookie_headers:
            jar: http.cookies.SimpleCookie = http.cookies.SimpleCookie()
            try:
                jar.load(header)
            except http.cookies.CookieError:
                continue
            for key, morsel in jar.items():
                self.cookies[key] = morsel.value

    # ---- auth ---------------------------------------------------------

    def _clerk_query(self) -> str:
        return urllib.parse.urlencode(
            {"__clerk_api_version": CLERK_API_VERSION, "_clerk_js_version": self.clerk_js}
        )

    def authenticate(self) -> None:
        """Cookie → Clerk session id → short-lived JWT for studio-api."""
        client = self._request(
            "GET",
            f"{AUTH_BASE}/v1/client?{self._clerk_query()}",
            extra_headers={"Authorization": self.cookies["__client"]},
        )
        session_id = (client.get("response") or {}).get("last_active_session_id")
        if not session_id:
            raise SunoError(
                "Clerk returned no last_active_session_id — the cookie did not "
                "resolve to a signed-in session.",
                "Re-copy SUNO_COOKIE from a logged-in suno.com tab. If it is "
                "fresh, run with `--clerk-js auto` in case the pinned clerk-js "
                f"version ({self.clerk_js}) is now refused.",
            )
        self.session_id = session_id
        self.refresh_token()

    def refresh_token(self) -> None:
        """JWTs from Clerk are short-lived; call before every studio-api hop."""
        if not self.session_id:
            raise SunoError("refresh_token() before authenticate()")
        renewed = self._request(
            "POST",
            f"{AUTH_BASE}/v1/client/sessions/{self.session_id}/tokens?{self._clerk_query()}",
            body={},
            extra_headers={"Authorization": self.cookies["__client"]},
        )
        token = renewed.get("jwt")
        if not token:
            raise SunoError(f"Clerk issued no jwt. Response keys: {sorted(renewed)}")
        self.token = token

    # ---- account ------------------------------------------------------

    def credits(self) -> dict:
        self.refresh_token()
        info = self._request("GET", f"{API_BASE}/api/billing/info/")
        return {
            "credits_left": info.get("total_credits_left"),
            "monthly_limit": info.get("monthly_limit"),
            "monthly_usage": info.get("monthly_usage"),
            "period": info.get("period"),
        }

    def captcha_required(self) -> bool:
        self.refresh_token()
        checked = self._request("POST", f"{API_BASE}/api/c/check", body={"ctype": "generation"})
        return bool(checked.get("required"))

    # ---- generation ---------------------------------------------------

    def generate(
        self,
        *,
        style: str,
        title: str | None = None,
        lyrics: str | None = None,
        instrumental: bool = False,
        model: str | None = None,
        negative_tags: str | None = None,
        captcha_token: str | None = None,
    ) -> list[dict]:
        """Kick off a generation. Returns the clip stubs Suno hands back.

        Two shapes, matching the two on the website:
          * description  — one free-text style prompt, Suno writes the lyrics
          * custom       — explicit `lyrics` as the prompt, `style` as the tags
        """
        candidates = [model] if model else list(MODELS)
        last_error: SunoError | None = None
        for candidate in candidates:
            self.refresh_token()
            payload: dict = {
                "make_instrumental": instrumental,
                "mv": candidate,
                "prompt": "",
                "generation_type": "TEXT",
                "token": captcha_token,
            }
            if lyrics is not None:
                payload["prompt"] = lyrics
                payload["tags"] = style
                payload["title"] = title or ""
                payload["negative_tags"] = negative_tags
            else:
                payload["gpt_description_prompt"] = style
            log(f"  submitting with model {candidate}")
            try:
                response = self._request(
                    "POST", f"{API_BASE}/api/generate/v2/", body=payload, timeout=60
                )
            except SunoError as exc:
                # A rejected model code reads as a 400 naming `mv`; anything else
                # is a real failure and should not burn the rest of the list.
                message = str(exc)
                if "HTTP 400" in message and not model:
                    log(f"  model {candidate} refused, trying the next one")
                    last_error = exc
                    continue
                raise
            clips = response.get("clips") or []
            if not clips:
                raise SunoError(
                    f"Suno accepted the request but returned no clips: {response}"
                )
            return clips
        raise SunoError(
            f"Every model code was refused: {', '.join(candidates)}. The codes in "
            "MODELS are stale.",
            "Watch the /api/generate/v2/ request in DevTools while generating on "
            "suno.com and copy the `mv` value, then pass it with --model.",
        )

    def poll(self, clip_ids: list[str], *, timeout: int = 600, interval: int = 8) -> list[dict]:
        """Wait until every clip has audio, or until `timeout` seconds pass."""
        deadline = time.monotonic() + timeout
        query = urllib.parse.urlencode({"ids": ",".join(clip_ids)})
        last: list[dict] = []
        while time.monotonic() < deadline:
            self.refresh_token()
            feed = self._request("GET", f"{API_BASE}/api/feed/v2?{query}")
            clips = feed.get("clips") or []
            last = clips
            statuses = [clip.get("status") for clip in clips]
            if clips and all(s == "error" for s in statuses):
                reasons = {
                    (clip.get("metadata") or {}).get("error_message") or "unspecified"
                    for clip in clips
                }
                raise SunoError(f"Suno failed the generation: {'; '.join(reasons)}")
            ready = [
                clip
                for clip in clips
                if clip.get("status") in ("streaming", "complete") and clip.get("audio_url")
            ]
            if ready and len(ready) == len(clips):
                return clips
            log(f"  {', '.join(s or '?' for s in statuses)} … {int(deadline - time.monotonic())}s left")
            time.sleep(interval + random.uniform(0, 2))
        raise SunoError(
            f"Timed out after {timeout}s. Last statuses: "
            f"{[c.get('status') for c in last]}. The tracks may still finish — "
            f"check suno.com, or re-poll with: fetch --ids {','.join(clip_ids)}"
        )

    def download(self, clip: dict, path: str) -> str:
        url = clip.get("audio_url") or f"https://cdn1.suno.ai/{clip['id']}.mp3"
        request = urllib.request.Request(url, headers={"User-Agent": DEFAULT_UA})
        with urllib.request.urlopen(request, timeout=180, context=self.ssl_context) as response:
            body = response.read()
        if len(body) < 1024:
            raise SunoError(f"{url} returned {len(body)} bytes — that is not an MP3.")
        with open(path, "wb") as handle:
            handle.write(body)
        return path


def latest_clerk_js() -> str:
    with urllib.request.urlopen(JSDELIVR, timeout=20) as response:
        data = json.loads(response.read())
    version = (data.get("tags") or {}).get("latest")
    if not version:
        raise SunoError("jsdelivr did not report a latest @clerk/clerk-js version")
    return version


def connect(args: argparse.Namespace) -> Suno:
    cookies = parse_cookie_header(os.environ.get("SUNO_COOKIE", ""))
    clerk_js = CLERK_JS_VERSION
    if args.clerk_js == "auto":
        clerk_js = latest_clerk_js()
        log(f"clerk-js: {clerk_js} (latest, per jsdelivr)")
    elif args.clerk_js:
        clerk_js = args.clerk_js
    client = Suno(cookies, clerk_js=clerk_js, verbose=args.verbose)
    client.authenticate()
    return client


def slugify(text: str, limit: int = 48) -> str:
    kept = [c.lower() if c.isalnum() else "-" for c in text]
    slug = "".join(kept)
    while "--" in slug:
        slug = slug.replace("--", "-")
    return slug.strip("-")[:limit] or "track"


# ---- commands ---------------------------------------------------------


def cmd_doctor(args: argparse.Namespace) -> int:
    """Walk every stage in order and name the first one that breaks."""
    stages = []

    def record(name: str, fn):
        try:
            value = fn()
        except SunoError as exc:
            stages.append((name, "FAIL", str(exc), exc.remedy))
            return None
        stages.append((name, "ok", value, None))
        return value

    cookies = record("cookie parses", lambda: f"{len(parse_cookie_header(os.environ.get('SUNO_COOKIE', '')))} cookies, __client present")
    holder: dict[str, Suno] = {}
    if cookies:
        record("clerk session", lambda: _doctor_auth(args, holder))
    client = holder.get("client")
    if client:
        record("jwt issued", lambda: f"{client.token[:24]}…")
        record("billing reachable", lambda: json.dumps(client.credits()))
        required = record("captcha gate", lambda: "REQUIRED" if client.captcha_required() else "not required")
        if required == "REQUIRED":
            stages.append((
                "captcha gate",
                "BLOCKED",
                "Suno is demanding an hCaptcha solve for generation.",
                "This script cannot solve it. See CAPTCHA in README.md — either "
                "pass --captcha-token from a browser solve, or use a relay API.",
            ))

    width = max(len(name) for name, *_ in stages)
    failed = False
    for name, status, detail, remedy in stages:
        mark = {"ok": "  ok  ", "FAIL": " FAIL ", "BLOCKED": "BLOCK "}[status]
        print(f"[{mark}] {name.ljust(width)}  {detail}")
        if remedy:
            print(f"{' ' * (width + 11)}↳ {remedy}")
        if status != "ok":
            failed = True
    return 1 if failed else 0


def _doctor_auth(args: argparse.Namespace, holder: dict) -> str:
    client = connect(args)
    holder["client"] = client
    return f"session {client.session_id}"


def cmd_credits(args: argparse.Namespace) -> int:
    print(json.dumps(connect(args).credits(), indent=2))
    return 0


def cmd_generate(args: argparse.Namespace) -> int:
    client = connect(args)
    log("authenticated")

    if client.captcha_required() and not args.captcha_token:
        raise SunoError(
            "Suno is requiring an hCaptcha solve before it will generate.",
            "Nothing in this script can solve it. Either generate one track by "
            "hand on suno.com (which usually clears the gate for a while) and "
            "retry, or grab the h-captcha token from the /api/generate/v2/ "
            "request body in DevTools and pass it with --captcha-token.",
        )

    lyrics = None
    if args.lyrics:
        lyrics = args.lyrics
    elif args.lyrics_file:
        with open(args.lyrics_file, encoding="utf-8") as handle:
            lyrics = handle.read()

    log(f"generating: {args.style!r}")
    clips = client.generate(
        style=args.style,
        title=args.title,
        lyrics=lyrics,
        instrumental=args.instrumental,
        model=args.model,
        negative_tags=args.negative_tags,
        captcha_token=args.captcha_token,
    )
    ids = [clip["id"] for clip in clips]
    log(f"queued {len(ids)} clip(s): {', '.join(ids)}")

    if args.no_wait:
        print(json.dumps(clips, indent=2))
        return 0

    finished = client.poll(ids, timeout=args.timeout)
    return _save(client, finished, args)


def cmd_fetch(args: argparse.Namespace) -> int:
    """Re-poll and download clips from an earlier run that timed out."""
    client = connect(args)
    finished = client.poll([i.strip() for i in args.ids.split(",") if i.strip()], timeout=args.timeout)
    return _save(client, finished, args)


def _save(client: Suno, clips: list[dict], args: argparse.Namespace) -> int:
    # Suno returns two takes per request. With an explicit --out the first take
    # takes that name and the rest get -2, -3 suffixes, so nothing is silently
    # dropped and nothing overwrites.
    written = []
    for index, clip in enumerate(clips):
        if args.out:
            base, dot, ext = args.out.rpartition(".")
            if not dot or "/" in ext:  # no extension, or the only dot is in a directory
                base, ext = args.out, "mp3"
            path = args.out if index == 0 else f"{base}-{index + 1}.{ext}"
        else:
            title = clip.get("title") or (clip.get("metadata") or {}).get("gpt_description_prompt") or "track"
            path = f"{slugify(title)}-{clip['id'][:8]}.mp3"
        client.download(clip, path)
        duration = (clip.get("metadata") or {}).get("duration")
        written.append(path)
        print(f"{path}  ({clip.get('model_name')}, {duration}s, {clip.get('title')!r})")
    log(f"wrote {len(written)} file(s)")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="suno_track.py",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--clerk-js", metavar="VERSION|auto",
                        help="override the pinned clerk-js version; `auto` asks jsdelivr")
    parser.add_argument("-v", "--verbose", action="store_true", help="log every HTTP hop")
    sub = parser.add_subparsers(dest="command", required=True)

    doctor = sub.add_parser("doctor", help="check each stage and name what is broken")
    doctor.set_defaults(func=cmd_doctor)

    credits = sub.add_parser("credits", help="show remaining credits")
    credits.set_defaults(func=cmd_credits)

    gen = sub.add_parser("generate", help="style prompt in, MP3 out")
    gen.add_argument("--style", required=True,
                     help="description mode: the whole prompt. custom mode (--lyrics): the style tags.")
    gen.add_argument("--title", help="track title (custom mode)")
    gen.add_argument("--lyrics", help="explicit lyrics; switches to custom mode")
    gen.add_argument("--lyrics-file", help="read lyrics from a file; switches to custom mode")
    gen.add_argument("--negative-tags", help="styles to steer away from (custom mode)")
    gen.add_argument("--instrumental", action="store_true", help="no vocals")
    gen.add_argument("--model", help=f"force a model code (default tries {', '.join(MODELS)})")
    gen.add_argument("--captcha-token", help="an h-captcha token lifted from DevTools")
    gen.add_argument("--out", help="output path; extra takes get -2, -3 suffixes")
    gen.add_argument("--timeout", type=int, default=600, help="seconds to wait for audio (default 600)")
    gen.add_argument("--no-wait", action="store_true", help="print clip stubs and exit")
    gen.set_defaults(func=cmd_generate)

    fetch = sub.add_parser("fetch", help="download clips from an earlier run")
    fetch.add_argument("--ids", required=True, help="comma-separated clip ids")
    fetch.add_argument("--out", help="output path")
    fetch.add_argument("--timeout", type=int, default=600)
    fetch.set_defaults(func=cmd_fetch)

    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except SunoError as exc:
        log(f"\nerror: {exc}")
        if exc.remedy:
            log(f"  ↳ {exc.remedy}")
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
