### The connection to Unsloth Studio, and the first credential Organon has ever held (#147 T1)

`organon-core/src/unsloth.rs`. An endpoint, a bearer token, a `GET /api/health` probe, and — the
actual point of the tier — **three refusals that are not the same refusal**. *Not configured*,
*unreachable* and *unauthorized* are fixed three different ways (mint a key, start the app, rotate
the key), so each carries its own `StudioError` variant and its own `remedy()` sentence. Collapsing
them into "cannot connect" is what sends somebody to restart an app when the real problem is a key
they never minted. A test asserts the three are neither equal nor identically worded; mutating any
one of them into another fails two or three tests by name.

📌 **A fourth variant keeps the three honest.** A `503` is reachable *and* authorized — the Studio
answered — so it is `Refused`, not `Unreachable`. Calling it unreachable would send someone to
restart a service that is plainly running.

🚨 **Where the token lives is the decision, not a detail.** It is read from `UNSLOTH_API_KEY` — the
Studio's own name for it, so there is one place to rotate — and **never written anywhere by us**.
Not the preset store, because presets are shared, exported and recalled and a token in one preset is
a token in everyone's. Not `ui_theme.json`, and not a sidecar of our own: a file is readable by
exactly the same audience as the environment variable, so it buys no confidentiality while adding an
artifact that gets backed up, cloud-synced and attached to bug reports. What an attacker with read
access to `HKCU\Environment` gets is the key in full; that ceiling is stated in the module doc rather
than implied, and beating it needs an OS keychain — a dependency and a platform matrix, which is not
this tier's to spend.

🚨 **`StudioToken`'s `Debug` and `Display` are hand-written to redact.** A `#[derive(Debug)]` on
anything holding a credential leaks it into every `{:?}` in every error path, so no derive is used
and no `StudioError` variant carries the secret. Four tests pin it, and they were mutation-checked
both ways: printing the raw token fails `token_debug_redacts` *and* `config_debug_redacts`, and
folding the request text into an error message — the plausible-looking "add context to the failure"
edit — fails `no_error_variant_can_carry_the_token` with the bearer header quoted in the panic.

⚠️ **The probe cannot detect a bad key, by construction, and that is written down rather than
discovered later.** `/api/health` is *unauthenticated*: it answers `200` with a wrong key and with
none at all. So a green probe proves the Studio is **running**, never that our credential is
**good**, and rendering it as "connected" would be another status line that cannot be wrong.
`probe_cannot_detect_a_bad_token` pins the limitation, so that the day it stops being true — a
Studio that gates health, a proxy in front of one — a test fails instead of the behaviour changing
silently. The credential is nevertheless checked *before* the socket, because a probe that reports
healthy while no key is held is a green light on a connection that cannot carry one useful request.

📌 **Zero new dependencies.** Hand-rolled HTTP/1.1 over `std::net::TcpStream`, the house pattern
from `organon-agent`'s `HttpChatClient`; `StudioTransport` mirrors that crate's `ChatClient` split,
so every test runs with no network, no key and no Studio. Two of them still open a real socket
without needing one: a closed ephemeral port must read as `Unreachable`, and a listener the test
stands up itself asserts that the *server* received `Authorization: Bearer …` — the one thing a mock
cannot check.

⚠️ **`127.0.0.1`, and `localhost` is rewritten to it at parse.** A `localhost` lookup tries `::1`
first against an IPv4-only listener, measured at ~200 ms of wasted connect per request on
organon-one. A rewrite makes that trap unreachable rather than merely documented;
`StudioEndpoint::new` is the escape hatch for anyone who means the name. `https://` is refused by
name — there is no TLS client here, and silently downgrading would put a bearer token on the wire in
cleartext under a scheme the user believed was encrypted. A malformed `ORGANON_UNSLOTH_ENDPOINT` is
an error rather than a fall back to the default, because falling back points the client at the right
address while the person believes it is at the one they typed.

⚠️ **`unsloth::extract_body` duplicates `organon_agent::extract_http_body`, and cannot avoid it** —
`organon-agent` depends on `organon-core`, so the dependency cannot point the other way. The
de-duplication is for that crate to drop its copy onto this one, which is a change to the Performer's
live path and deliberately not made here.

🚨 **Nothing here has spoken to a running Studio.** It was not running on organon-one when this
landed — nothing listening on `127.0.0.1:8888` or on the LAN bind `192.168.0.7:8888`, and no
matching process — so the `Unauthorized` path in particular has never been produced by the actual
app, only by a canned `401`. Green and ready to try, not verified working.
