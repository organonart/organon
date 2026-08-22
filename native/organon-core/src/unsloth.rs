//! The connection to Unsloth Studio — an endpoint, a credential, and three refusals (#147 T1).
//!
//! Unsloth Studio (Desktop) is a **local service the user installed**: a native-Windows app
//! serving a 351-route FastAPI backend on `127.0.0.1:8888`. Organon does not ship it, spawn
//! it, composite it or link it — we are a *client* of something already running, which
//! `doc/organon_mind_training_lens.md` §2 argues is a fourth kind of extension the taxonomy
//! did not have. Its boundary is a socket and a bearer token.
//!
//! This module is the whole of that boundary and nothing else. It answers one question —
//! **can we talk to the Studio, and if not, why not** — because #147's T2–T5 (the adapter
//! catalog, the training strip, the divergence lens) each begin by needing the answer and
//! none of them should re-derive it.
//!
//! # 🚨 The three refusals, which are the point of the tier
//!
//! *Not configured*, *unreachable* and *unauthorized* are three different states that a
//! person fixes three different ways, and collapsing them into "cannot connect" sends
//! somebody to restart an app when the real problem is a key they never minted. Each has
//! its own [`StudioError`] variant, its own sentence, and its own [`StudioError::remedy`].
//!
//! ⚠️ **A health probe cannot, by construction, detect the third one.** `GET /api/health`
//! is *unauthenticated* — it answers `200` with a wrong key, with an expired key, and with
//! no key at all. So [`StudioClient::probe`] returning `Ok` proves the Studio is **there**,
//! never that our credential is **good**. That limitation is pinned by a test
//! (`probe_cannot_detect_a_bad_token`) rather than left as a comment, because the tempting
//! next step — treating a green probe as "connected" in a UI — would be a status line that
//! cannot be wrong, which is a shape this codebase has been bitten by repeatedly.
//! `Unauthorized` is reachable and tested here; it becomes *reachable in anger* the moment
//! T4/T5 call an authenticated route through [`StudioClient::get`].
//!
//! # 🚨 Where the token lives, and why not anywhere else
//!
//! Organon has never held a credential, so the placement is the decision rather than a
//! detail. **The token is read from the `UNSLOTH_API_KEY` environment variable and is
//! never written anywhere by us.**
//!
//! - **Not the preset store.** Presets are shared, exported, posted and recalled. A token
//!   in one preset is a token in everyone's.
//! - **Not `ui_theme.json`,** and not any other sidecar of ours. A file is a new artifact
//!   that gets backed up, cloud-synced, and attached to a bug report — and it would buy
//!   *no* confidentiality over the environment variable, because both are readable by
//!   exactly the same audience: anything running as this user. A file is therefore strictly
//!   worse at the same security level, which is why there is no `--save-token`.
//! - **`UNSLOTH_API_KEY` is the Studio's own name for it**, so there is one place to rotate
//!   and rotation is immediate and real — the property the modules plan §11.9 records that
//!   git-sourced trust does not give us.
//!
//! **What an attacker with read access to that location gets: the key, in full.** A
//! User-scope variable on Windows lives at `HKCU\Environment`; on macOS/Linux it is
//! whatever shell profile exported it. Anything already running as this user can read it.
//! That is the honest ceiling of this design, and it is the same ceiling every alternative
//! available without a new dependency has. A real improvement needs an OS keychain
//! (DPAPI / Keychain / Secret Service), which is a dependency and a platform matrix, and is
//! not T1's to spend. What this design *does* buy is that the secret never lands in a file
//! we created, never enters a preset, and never survives the process.
//!
//! 🚨 **The token is never logged, printed, or formatted.** [`StudioToken`] implements
//! `Debug` and `Display` **by hand** to redact — a `#[derive(Debug)]` on anything holding
//! it would leak it into every `{:?}` in every error path — and no [`StudioError`] variant
//! carries it. Tests assert all of that (`token_debug_redacts`, `token_display_redacts`,
//! `config_debug_redacts`, `no_error_variant_can_carry_the_token`).
//!
//! ⚠️ The one place the raw secret legitimately appears is the request text built by
//! [`build_get`], which must carry `Authorization: Bearer …` to work. **That string must
//! never reach a log, an error message or a panic.** It is returned to exactly one caller
//! ([`StudioTransport::send`]), which writes it to a socket and drops it.
//!
//! # Zero new dependencies
//!
//! Hand-rolled HTTP/1.1 over [`std::net::TcpStream`], the house pattern established by
//! `organon-agent`'s `HttpChatClient`, `organon-console`'s `mcp_http` and
//! `bin/mind_runtime.rs`. [`StudioTransport`] mirrors that crate's `ChatClient` split — a
//! trait with a real impl and a mock — so every test here runs with no network and no key.
//!
//! ⚠️ **[`extract_body`] duplicates `organon_agent::extract_http_body`, and cannot avoid
//! it**: `organon-agent` depends on `organon-core`, so the dependency cannot point the
//! other way. The natural de-duplication is for that crate to drop its copy onto this one,
//! which is a change to the Performer's live path and deliberately not made in T1.
//!
//! # Why this module is in `organon-core`
//!
//! It sits beside [`crate::lora`], the other half of the same tier: that module reads what a
//! fine-tune moved, this one finds the adapters to read. Core's invariant is **no
//! `nih_plug`, no `wgpu`, no `egui`, no `winit`** — a `std::net` socket touches none of
//! them, and no dependency is added. Every consumer of #147 is above core (the Mind lens,
//! a Console dock, the `organon` CLI), so core is the only crate all of them can see.
//!
//! ⚠️ The honest cost: core is the crate published to crates.io, so this module's public
//! surface is a standing commitment — which is a reason to keep it as small as it is, not a
//! reason to hide it in a crate half the consumers cannot reach.
//!
//! # Scope
//!
//! `GET /api/health` and the machinery to reach it. **No other route.** Training, adapters,
//! checkpoints and SSE are T4/T5 and each needs its own thinking about shape and provenance;
//! [`StudioClient::get`] is the seam they will use, and it is public precisely so they need
//! not re-open this module's insides.

use std::fmt;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

// ===========================================================================
// Constants
// ===========================================================================

/// The Studio's loopback host.
///
/// ⚠️ **`127.0.0.1`, never `localhost`.** A `localhost` lookup resolves `::1` first, and
/// the Studio's listener is IPv4-only — measured at ~200 ms of wasted connect per request
/// on organon-one. [`StudioEndpoint::parse`] normalizes the name away for the same reason.
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// The Studio's default port.
///
/// ⚠️ **Unqualified, and the Studio's only collision handling is a printed warning** — so a
/// second app on 8888 is a real possibility and the endpoint is configurable from the first
/// commit rather than after somebody hits it.
pub const DEFAULT_PORT: u16 = 8888;

/// The unauthenticated health route. See the module doc for what a `200` here does and does
/// not prove.
pub const HEALTH_PATH: &str = "/api/health";

/// The environment variable holding the bearer token. **The Studio's own CLI uses this
/// name**, so there is one key and one place to rotate it.
pub const TOKEN_ENV: &str = "UNSLOTH_API_KEY";

/// Optional endpoint override, e.g. `192.168.0.7:8888` or `http://127.0.0.1:9000`.
/// Unset means [`StudioEndpoint::default`].
pub const ENDPOINT_ENV: &str = "ORGANON_UNSLOTH_ENDPOINT";

/// The budget for each stage of a request: connecting, writing, reading.
///
/// ⚠️ **A peer that never answers must not wedge the caller**, which is what no timeout
/// means. Short on purpose: `/api/health` is a constant-time answer from a local process, so
/// anything past a few seconds is a fault, not slowness. It is *not* `organon-agent`'s 300 s
/// — that number exists for a reasoning model that thinks before it speaks, and copying it
/// here would turn a dead socket into a five-minute stall.
///
/// 🚨 **This covers the CONNECT too, and that is not free.** `TcpStream::connect` has no
/// timeout of its own: a host that neither accepts nor refuses — a firewall dropping the SYN
/// — blocks for the OS retry ceiling, tens of seconds to minutes. Loopback hides it (a closed
/// local port answers `ECONNREFUSED` at once) but [`ENDPOINT_ENV`] is configurable to a LAN
/// address and this module's own example is one, so [`connect_within`] spends this as a
/// **total** budget across every resolved address rather than letting `connect` decide.
///
/// ⚠️ **One stage is outside the budget and cannot be brought inside without a dependency:
/// DNS.** [`resolve_addrs`] parses a literal IP with no syscall at all — which is every
/// default and every documented endpoint here — but a *name* goes through
/// `std::net::ToSocketAddrs`, which `std` offers no bounded form of. Named, not hidden.
pub const TIMEOUT_SECS: u64 = 5;

/// The smallest remaining budget worth spending on a connect attempt.
///
/// ⚠️ Not arbitrary: `TcpStream::connect_timeout` **rejects a zero duration** ("cannot set a
/// 0 duration timeout"), so an exhausted budget has to be recognised before the call rather
/// than discovered inside it — where it would surface as a confusing OS message instead of
/// the honest "we ran out of time".
const MIN_CONNECT_ATTEMPT: Duration = Duration::from_millis(1);

// ===========================================================================
// The credential
// ===========================================================================

/// A bearer token for the Studio, held in memory and never persisted by us.
///
/// 🚨 `Debug` and `Display` are **hand-written to redact**. Do not add a `derive`, and do
/// not add an accessor that returns the raw string for convenience — the only reader is
/// [`StudioToken::header_value`], which exists to be written straight to a socket.
#[derive(Clone, PartialEq, Eq)]
pub struct StudioToken(String);

impl StudioToken {
    /// Wrap a raw token. Returns `None` for empty or whitespace-only input.
    ///
    /// ⚠️ The blank check is not tidiness: an unset variable read as `""` would otherwise
    /// produce a `Some` token that sends `Authorization: Bearer `, which the Studio answers
    /// with `401` — reporting *unauthorized* for what is really *not configured*, i.e.
    /// exactly the confusion this tier exists to prevent.
    pub fn new(raw: impl Into<String>) -> Option<StudioToken> {
        let raw = raw.into();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(StudioToken(trimmed.to_string()))
        }
    }

    /// Read the token from [`TOKEN_ENV`]. `None` when unset or blank.
    ///
    /// ⚠️ **On Windows a process does not see a User-scope variable set after it started.**
    /// A key minted and exported while Organon is running is invisible until Organon is
    /// restarted; [`StudioError::remedy`] says so, because the symptom is
    /// indistinguishable from never having set it.
    pub fn from_env() -> Option<StudioToken> {
        std::env::var(TOKEN_ENV).ok().and_then(StudioToken::new)
    }

    /// The `Authorization` header value. **The only way the secret leaves this type**, and
    /// its one legitimate destination is a socket.
    pub fn header_value(&self) -> String {
        format!("Bearer {}", self.0)
    }

    /// The token's length in bytes — enough to tell "I set something" from "I set nothing"
    /// in a diagnostic, without disclosing any of it.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always `false` — [`StudioToken::new`] rejects blank input. Present because clippy
    /// asks for it beside `len`, and because it documents that invariant at the type.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for StudioToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 🚨 Hand-written on purpose. A derive here leaks the key into every `{:?}` in
        // every error path that ever formats a struct containing one.
        write!(f, "StudioToken(<redacted>)")
    }
}

impl fmt::Display for StudioToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Redacted too: `{}` is at least as easy to reach for as `{:?}`, and a type whose
        // Debug is safe while its Display is not is a trap rather than a guard.
        write!(f, "<redacted>")
    }
}

// ===========================================================================
// The endpoint
// ===========================================================================

/// Where the Studio is: a host and a port. Plain `http` — there is no TLS client here, and
/// the intended target is a service on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioEndpoint {
    pub host: String,
    pub port: u16,
}

impl Default for StudioEndpoint {
    fn default() -> Self {
        StudioEndpoint {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
        }
    }
}

impl StudioEndpoint {
    /// Construct verbatim. **Does not normalize the host** — the escape hatch for anyone
    /// who genuinely wants a name resolved at connect time.
    pub fn new(host: impl Into<String>, port: u16) -> StudioEndpoint {
        StudioEndpoint {
            host: host.into(),
            port,
        }
    }

    /// Parse `host`, `host:port`, `http://host:port` or `http://host:port/`.
    ///
    /// 📌 **A bare `localhost` is rewritten to `127.0.0.1`.** The two are identical for an
    /// IPv4 listener, and the name costs ~200 ms per request here because `::1` is tried
    /// first — a measured trap that a rewrite makes unreachable rather than merely
    /// documented. [`StudioEndpoint::new`] skips the rewrite for anyone who means it.
    ///
    /// ⚠️ `https://` is rejected by name rather than silently downgraded: there is no TLS
    /// client in this codebase, and pretending otherwise would send a bearer token in
    /// cleartext to a host the user believed was encrypted.
    pub fn parse(s: &str) -> Result<StudioEndpoint, EndpointError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(EndpointError::Empty);
        }
        if let Some(rest) = s.strip_prefix("https://") {
            return Err(EndpointError::NoTls(rest.to_string()));
        }
        let rest = s.strip_prefix("http://").unwrap_or(s);
        // Drop any path — the routes are ours to choose, not the config's.
        let authority = rest.split('/').next().unwrap_or("").trim();
        if authority.is_empty() {
            return Err(EndpointError::Empty);
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => {
                let port = p
                    .parse::<u16>()
                    .map_err(|_| EndpointError::BadPort(p.to_string()))?;
                if port == 0 {
                    return Err(EndpointError::BadPort(p.to_string()));
                }
                (h, port)
            }
            None => (authority, DEFAULT_PORT),
        };
        if host.is_empty() {
            return Err(EndpointError::Empty);
        }
        let host = if host.eq_ignore_ascii_case("localhost") {
            DEFAULT_HOST.to_string()
        } else {
            host.to_string()
        };
        Ok(StudioEndpoint { host, port })
    }

    /// Read [`ENDPOINT_ENV`]; unset means the default.
    ///
    /// ⚠️ A **malformed** value is an error, not a silent fall back to the default. Falling
    /// back would point the client at the right address while the person believes it is at
    /// the one they typed, and every subsequent refusal would name the wrong cause.
    pub fn from_env() -> Result<StudioEndpoint, EndpointError> {
        match std::env::var(ENDPOINT_ENV) {
            Ok(v) if !v.trim().is_empty() => StudioEndpoint::parse(&v),
            _ => Ok(StudioEndpoint::default()),
        }
    }

    /// `host:port`, for a `Host:` header or a log line. Carries no secret.
    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Why an endpoint string could not be read. Never carries a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointError {
    /// Nothing to parse.
    Empty,
    /// The port was absent-but-colon'd, non-numeric, out of range, or zero.
    BadPort(String),
    /// `https://` — see [`StudioEndpoint::parse`].
    NoTls(String),
}

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EndpointError::Empty => write!(
                f,
                "{ENDPOINT_ENV} is empty — expected something like 127.0.0.1:{DEFAULT_PORT}"
            ),
            EndpointError::BadPort(p) => write!(
                f,
                "{ENDPOINT_ENV} has a bad port {p:?} — expected a number from 1 to 65535"
            ),
            EndpointError::NoTls(rest) => write!(
                f,
                "{ENDPOINT_ENV} names an https:// endpoint ({rest:?}) and there is no TLS \
                 client here — use http:// for a local Studio"
            ),
        }
    }
}

impl std::error::Error for EndpointError {}

// ===========================================================================
// The configuration
// ===========================================================================

/// Endpoint plus credential: everything needed to reach the Studio.
///
/// 🚨 `Debug` is hand-written so the token cannot escape through it.
#[derive(Clone, PartialEq)]
pub struct StudioConfig {
    pub endpoint: StudioEndpoint,
    /// `None` means *not configured* — the first of the three refusals.
    pub token: Option<StudioToken>,
}

impl Default for StudioConfig {
    fn default() -> Self {
        StudioConfig {
            endpoint: StudioEndpoint::default(),
            token: None,
        }
    }
}

impl StudioConfig {
    /// Build from the environment: [`ENDPOINT_ENV`] and [`TOKEN_ENV`].
    ///
    /// A missing token is **not** an error here — it is a state the caller reports through
    /// [`StudioError::NotConfigured`], which carries the sentence that tells a person what
    /// to do about it. Failing at construction would move that message somewhere it cannot
    /// be shown beside the connection it describes.
    pub fn from_env() -> Result<StudioConfig, EndpointError> {
        Ok(StudioConfig {
            endpoint: StudioEndpoint::from_env()?,
            token: StudioToken::from_env(),
        })
    }

    /// Whether a credential is held. Says nothing about whether it is *valid* — see the
    /// module doc.
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }
}

impl fmt::Debug for StudioConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StudioConfig")
            .field("endpoint", &self.endpoint)
            // The field prints as `Some(StudioToken(<redacted>))` / `None`, so a diagnostic
            // can still distinguish "no key" from "a key" without disclosing one.
            .field("token", &self.token)
            .finish()
    }
}

// ===========================================================================
// The three refusals
// ===========================================================================

/// Why a Studio request did not produce an answer.
///
/// 🚨 The first three variants are the tier: *not configured*, *unreachable*, *unauthorized*.
/// They are separate because a person fixes them three different ways — mint a key, start
/// the app, rotate the key — and one "cannot connect" sends two thirds of them to the wrong
/// remedy. [`StudioError::remedy`] carries the sentence.
///
/// 🚨 **No variant carries the token**, and none may ever be given a field that could.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudioError {
    /// No credential is held. Nothing was sent.
    NotConfigured,
    /// The socket never produced a reply: refused, timed out, DNS, reset. `detail` is the
    /// `std::io::Error` text and nothing else.
    Unreachable { authority: String, detail: String },
    /// The Studio answered, and rejected our credential (`401`/`403`).
    Unauthorized { status: u16 },
    /// The Studio answered with some other non-2xx status. Distinct from the three on
    /// purpose: a `500` is the Studio's problem, not the connection's, and calling it
    /// "unreachable" would send someone to restart a service that is plainly running.
    Refused { status: u16, reason: String },
    /// A 2xx whose bytes were not what the route promises — a truncated body, a proxy's
    /// HTML, a JSON shape that is not the health object.
    Malformed { detail: String },
}

impl StudioError {
    /// What a person does about it. One sentence, imperative, naming the actual knob.
    pub fn remedy(&self) -> String {
        match self {
            StudioError::NotConfigured => format!(
                "Mint an API key in Unsloth Studio and set {TOKEN_ENV}, then restart Organon \
                 — a process does not see an environment variable set after it started."
            ),
            StudioError::Unreachable { authority, .. } => format!(
                "Nothing answered at {authority}. Start Unsloth Studio, or set \
                 {ENDPOINT_ENV} to where it is actually serving."
            ),
            StudioError::Unauthorized { .. } => format!(
                "The Studio rejected the key. Mint a new one in the Studio and update \
                 {TOKEN_ENV} — rotating it revokes the old one immediately."
            ),
            StudioError::Refused { status, .. } => format!(
                "The Studio is reachable and answered {status}; the connection is fine and \
                 the fault is on its side. Check the Studio's own log."
            ),
            StudioError::Malformed { .. } => {
                "The Studio is reachable but answered something this build does not \
                 recognize — check that it is Unsloth Studio and not another service on \
                 that port."
                    .to_string()
            }
        }
    }
}

impl fmt::Display for StudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StudioError::NotConfigured => {
                write!(f, "not configured: no {TOKEN_ENV} is set")
            }
            StudioError::Unreachable { authority, detail } => {
                write!(f, "unreachable: {authority}: {detail}")
            }
            StudioError::Unauthorized { status } => {
                write!(f, "unauthorized: the Studio answered {status}")
            }
            StudioError::Refused { status, reason } => {
                write!(f, "refused: the Studio answered {status} {reason}")
            }
            StudioError::Malformed { detail } => {
                write!(f, "malformed reply: {detail}")
            }
        }
    }
}

impl std::error::Error for StudioError {}

// ===========================================================================
// The health object
// ===========================================================================

/// `GET /api/health`'s body. Unknown fields are ignored, so a Studio that grows the object
/// does not break this reader.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Health {
    /// `"healthy"` on the instance measured for #147. Compared case-insensitively.
    pub status: String,
    /// `"Unsloth UI Backend"` on that instance. Defaulted rather than required: a field
    /// this reader does not strictly need must not be able to fail the probe.
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub version: Option<String>,
}

impl Health {
    /// Whether the Studio calls itself healthy.
    pub fn is_healthy(&self) -> bool {
        self.status.eq_ignore_ascii_case("healthy")
    }
}

// ===========================================================================
// Transport (mockable, mirroring organon-agent's ChatClient split)
// ===========================================================================

/// One HTTP round trip. The request is composed by [`build_get`] and handed over whole, so
/// a mock can assert on the exact bytes — including that the `Authorization` header is
/// present and correct — without a socket.
///
/// 🚨 Implementations must never log the request: it carries the bearer token.
pub trait StudioTransport {
    /// Send `request` to `host:port` and return the raw response (status line, headers,
    /// body). `Err` is a socket-level failure and its text is the `std::io::Error` only.
    fn send(&self, host: &str, port: u16, request: &str) -> Result<String, String>;
}

/// Resolve an endpoint's host to the addresses worth trying, in the order to try them.
///
/// 📌 **A literal IP short-circuits with no syscall at all** — no DNS, no blocking, nothing
/// to time out. That is every default here and every endpoint this module's docs use, so the
/// [`TIMEOUT_SECS`] budget is the *whole* cost of a connect on the ordinary path.
///
/// ⚠️ **A name goes through `ToSocketAddrs`, which can block for as long as the resolver
/// takes**, and `std` has no bounded form of it. Bounding it needs a thread or a dependency
/// and T1 spends neither; the limit is stated at [`TIMEOUT_SECS`] rather than papered over.
///
/// ⚠️ **A name may yield several addresses and the resolver's order is kept**, IPv6-first
/// included. That is deliberate: reordering would second-guess a resolver that may be right,
/// and the one case measured on organon-one — `localhost` costing ~200 ms because `::1` is
/// tried against an IPv4-only listener — is already handled *upstream* by
/// [`StudioEndpoint::parse`] rewriting the name to `127.0.0.1`, so it never reaches here.
/// **That rewrite is what keeps the default path off this branch entirely.**
///
/// A name that resolves to nothing is an `Err`, never an empty success — a caller handed an
/// empty list would loop zero times and report a refusal with no cause in it.
pub fn resolve_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve: {host}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("resolve: {host} resolved to no addresses"));
    }
    Ok(addrs)
}

/// Connect to the first address that answers, spending `budget` **in total** across all of
/// them rather than per attempt.
///
/// 🚨 The total is the point. `TcpStream::connect` blocks on the OS SYN-retry ceiling with no
/// timeout of its own, and a per-address timeout would multiply by however many addresses a
/// name happened to resolve to — so a caller promised five seconds could wait fifteen.
///
/// Errors are `Err(detail)` for [`StudioError::Unreachable`], and each names a cause a person
/// can act on: a refusal quotes the OS, an exhausted budget says what silence means.
pub fn connect_within(addrs: &[SocketAddr], budget: Duration) -> Result<TcpStream, String> {
    let Some(first) = addrs.first() else {
        return Err("connect: no address to try".to_string());
    };
    let deadline = Instant::now() + budget;
    let mut last: Option<String> = None;
    for addr in addrs {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining < MIN_CONNECT_ATTEMPT {
            return Err(format!(
                "connect: gave up after {}s — {addr} answered neither an acceptance nor a \
                 refusal, which is what a dropped packet looks like (a firewall, or a host \
                 that is not there)",
                budget.as_secs_f32()
            ));
        }
        match TcpStream::connect_timeout(addr, remaining) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = Some(format!("connect: {addr}: {e}")),
        }
    }
    Err(last.unwrap_or_else(|| format!("connect: {first}: no attempt was made")))
}

/// The real client: HTTP/1.1 over [`std::net::TcpStream`], no dependency, no TLS.
pub struct TcpTransport;

impl StudioTransport for TcpTransport {
    fn send(&self, host: &str, port: u16, request: &str) -> Result<String, String> {
        use std::io::{Read, Write};

        let timeout = Duration::from_secs(TIMEOUT_SECS);
        // 🚨 Never a bare `TcpStream::connect` — see [`TIMEOUT_SECS`]. It has no timeout, so
        // a dropped SYN would blow the budget this function is meant to hold to.
        let addrs = resolve_addrs(host, port)?;
        let mut stream = connect_within(&addrs, timeout)?;
        // ⚠️ Both directions. A server that accepts and then never speaks is the case a
        // read timeout exists for; a write timeout covers the mirror image, a peer whose
        // receive window never opens.
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        stream
            .write_all(request.as_bytes())
            // 🚨 The io error only. Never `{request}` — it holds the token.
            .map_err(|e| format!("write: {e}"))?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|e| format!("read: {e}"))?;
        Ok(String::from_utf8_lossy(&raw).into_owned())
    }
}

/// A canned transport for tests: returns a fixed result and records what it was asked to
/// send, so a test can assert on the composed request without a network.
pub struct MockTransport {
    reply: Result<String, String>,
    sent: Mutex<Vec<String>>,
}

impl MockTransport {
    /// A transport that answers with this raw HTTP response.
    pub fn ok(raw: impl Into<String>) -> MockTransport {
        MockTransport {
            reply: Ok(raw.into()),
            sent: Mutex::new(Vec::new()),
        }
    }

    /// A transport that fails at the socket, as an unreachable host would.
    pub fn fail(detail: impl Into<String>) -> MockTransport {
        MockTransport {
            reply: Err(detail.into()),
            sent: Mutex::new(Vec::new()),
        }
    }

    /// Every request text this transport was handed.
    pub fn sent(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }
}

impl StudioTransport for MockTransport {
    fn send(&self, _host: &str, _port: u16, request: &str) -> Result<String, String> {
        self.sent.lock().unwrap().push(request.to_string());
        self.reply.clone()
    }
}

// ===========================================================================
// Request composition and response classification (pure)
// ===========================================================================

/// Compose an HTTP/1.1 `GET`.
///
/// 🚨 **The returned string carries the bearer token.** Write it to a socket; never log it,
/// never put it in an error, never include it in a panic message.
///
/// `Connection: close` because the reader is `read_to_end` — a kept-alive connection would
/// simply block until the timeout with the whole reply already in hand.
pub fn build_get(endpoint: &StudioEndpoint, path: &str, token: Option<&StudioToken>) -> String {
    let mut req = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\n\
         User-Agent: organon/{}\r\nConnection: close\r\n",
        endpoint.authority(),
        env!("CARGO_PKG_VERSION"),
    );
    if let Some(t) = token {
        req.push_str(&format!("Authorization: {}\r\n", t.header_value()));
    }
    req.push_str("\r\n");
    req
}

/// The numeric status from an HTTP/1.1 status line, plus its reason phrase.
pub fn parse_status(raw: &str) -> Option<(u16, String)> {
    let line = raw.lines().next()?.trim();
    let mut parts = line.splitn(3, ' ');
    let version = parts.next()?;
    if !version.starts_with("HTTP/") {
        return None;
    }
    let code = parts.next()?.parse::<u16>().ok()?;
    let reason = parts.next().unwrap_or("").trim().to_string();
    Some((code, reason))
}

/// Split a raw HTTP/1.1 response's body out of it: de-chunk `Transfer-Encoding: chunked`,
/// else honor `Content-Length`, else take the remainder.
///
/// ⚠️ Duplicates `organon_agent::extract_http_body` because the dependency runs the other
/// way — see the module doc.
pub fn extract_body(raw: &str) -> String {
    let Some((head, body)) = raw.split_once("\r\n\r\n") else {
        return raw.to_string();
    };
    let chunked = head.lines().any(|l| {
        let l = l.trim().to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    if chunked {
        return dechunk(body);
    }
    if let Some(len) = head.lines().find_map(|l| {
        let l = l.trim();
        let low = l.to_ascii_lowercase();
        low.starts_with("content-length:")
            .then(|| l[l.find(':')? + 1..].trim().parse::<usize>().ok())
            .flatten()
    }) {
        let bytes = body.as_bytes();
        let end = len.min(bytes.len());
        return String::from_utf8_lossy(&bytes[..end]).into_owned();
    }
    body.to_string()
}

/// De-chunk a `Transfer-Encoding: chunked` body.
fn dechunk(body: &str) -> String {
    let bytes = body.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(rel) = bytes[i..].windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let size_str = std::str::from_utf8(&bytes[i..i + rel])
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let Ok(size) = usize::from_str_radix(&size_str, 16) else {
            break;
        };
        i += rel + 2;
        if size == 0 {
            break;
        }
        let end = (i + size).min(bytes.len());
        out.extend_from_slice(&bytes[i..end]);
        i = end + 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Turn a raw response into a body or one of the refusals.
///
/// 🚨 This is where *unauthorized* is separated from *refused*, and it is the whole reason
/// the status line is parsed rather than the body being handed straight to serde.
pub fn classify(raw: &str) -> Result<String, StudioError> {
    let Some((status, reason)) = parse_status(raw) else {
        return Err(StudioError::Malformed {
            detail: "no HTTP status line in the reply".to_string(),
        });
    };
    match status {
        200..=299 => Ok(extract_body(raw)),
        401 | 403 => Err(StudioError::Unauthorized { status }),
        _ => Err(StudioError::Refused { status, reason }),
    }
}

// ===========================================================================
// The client
// ===========================================================================

/// A client for the Studio.
///
/// 🚨 Deliberately **not** `Debug`: it holds a [`StudioConfig`], and while that type
/// redacts, a derive here would also demand `T: Debug` and invite an impl on a transport
/// that has seen the raw request bytes.
pub struct StudioClient<T: StudioTransport> {
    config: StudioConfig,
    transport: T,
}

impl StudioClient<TcpTransport> {
    /// The real client, configured from the environment.
    pub fn from_env() -> Result<StudioClient<TcpTransport>, EndpointError> {
        Ok(StudioClient {
            config: StudioConfig::from_env()?,
            transport: TcpTransport,
        })
    }
}

impl<T: StudioTransport> StudioClient<T> {
    /// A client over any transport — how tests get one with no network.
    pub fn new(config: StudioConfig, transport: T) -> StudioClient<T> {
        StudioClient { config, transport }
    }

    /// The configuration in force.
    pub fn config(&self) -> &StudioConfig {
        &self.config
    }

    /// `GET` an authenticated route. **T4/T5's seam**; nothing in T1 calls it with any path
    /// but [`HEALTH_PATH`].
    ///
    /// Refuses before touching the socket when no credential is held, because every route
    /// past health needs one and a `401` is a worse way to learn that a key was never set.
    pub fn get(&self, path: &str) -> Result<String, StudioError> {
        let Some(token) = self.config.token.as_ref() else {
            return Err(StudioError::NotConfigured);
        };
        self.send(path, Some(token))
    }

    /// Probe the Studio: is it there, and do we hold a credential for it?
    ///
    /// 🚨 **`Ok` does not mean the token is valid.** [`HEALTH_PATH`] is unauthenticated —
    /// it answers `200` to a wrong key and to no key at all — so this proves reachability
    /// and nothing more. Do not render it as "connected"; render it as "the Studio is
    /// running". The first authenticated call is what tests the credential, and it is the
    /// one that can return [`StudioError::Unauthorized`].
    ///
    /// The credential is nevertheless checked **first**, before the socket: a probe that
    /// reports healthy while no key is held would be a green light on a connection that
    /// cannot carry a single useful request.
    pub fn probe(&self) -> Result<Health, StudioError> {
        let Some(token) = self.config.token.as_ref() else {
            return Err(StudioError::NotConfigured);
        };
        // The header is sent even though the route ignores it: if a future Studio, or a
        // reverse proxy in front of one, ever does gate health, we want the honest 401
        // rather than a puzzle.
        let body = self.send(HEALTH_PATH, Some(token))?;
        serde_json::from_str::<Health>(&body).map_err(|e| StudioError::Malformed {
            // serde's message quotes the *body*, which is the Studio's, never ours — the
            // token appears only in the request. Still bounded, so it cannot paste a
            // megabyte of proxy HTML into a UI.
            detail: truncate(&e.to_string(), 200),
        })
    }

    /// Compose, send, classify.
    fn send(&self, path: &str, token: Option<&StudioToken>) -> Result<String, StudioError> {
        let req = build_get(&self.config.endpoint, path, token);
        let raw = self
            .transport
            .send(&self.config.endpoint.host, self.config.endpoint.port, &req)
            .map_err(|detail| StudioError::Unreachable {
                authority: self.config.endpoint.authority(),
                detail,
            })?;
        classify(&raw)
    }
}

/// Clip a diagnostic string, on a char boundary.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

// ===========================================================================
// Tests — no network, no key, no Studio
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in secret. Distinctive so a redaction test cannot pass by coincidence.
    const SECRET: &str = "usk-live-9f3c2e11-DO-NOT-LEAK";

    fn tok() -> StudioToken {
        StudioToken::new(SECRET).expect("non-blank")
    }

    fn health_200() -> String {
        let body = r#"{"status":"healthy","service":"Unsloth UI Backend","version":"2026.8.19"}"#;
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    // -- redaction ---------------------------------------------------------

    #[test]
    fn token_debug_redacts() {
        let d = format!("{:?}", tok());
        assert_eq!(d, "StudioToken(<redacted>)");
        assert!(!d.contains(SECRET), "Debug leaked the token: {d}");
        assert!(!d.contains("usk-"), "Debug leaked a token prefix: {d}");
    }

    #[test]
    fn token_display_redacts() {
        let d = format!("{}", tok());
        assert_eq!(d, "<redacted>");
        assert!(!d.contains(SECRET), "Display leaked the token: {d}");
    }

    #[test]
    fn config_debug_redacts() {
        let cfg = StudioConfig {
            endpoint: StudioEndpoint::default(),
            token: Some(tok()),
        };
        let d = format!("{cfg:?}");
        assert!(!d.contains(SECRET), "StudioConfig Debug leaked the token: {d}");
        // It must still distinguish "a key is held" from "none is".
        assert!(d.contains("Some"), "a redacted Debug still has to say a key is held: {d}");
        let none = format!(
            "{:?}",
            StudioConfig {
                endpoint: StudioEndpoint::default(),
                token: None
            }
        );
        assert!(none.contains("None"), "{none}");
    }

    /// 🚨 The guard that matters most: no refusal, however it was produced, may carry the
    /// secret — through `Display`, through `Debug`, or through `remedy()`.
    #[test]
    fn no_error_variant_can_carry_the_token() {
        let cfg = StudioConfig {
            endpoint: StudioEndpoint::new("127.0.0.1", 8888),
            token: Some(tok()),
        };
        let errors = vec![
            StudioError::NotConfigured,
            StudioClient::new(
                StudioConfig {
                    endpoint: cfg.endpoint.clone(),
                    token: Some(tok()),
                },
                MockTransport::fail("connect: refused"),
            )
            .probe()
            .unwrap_err(),
            StudioClient::new(
                StudioConfig {
                    endpoint: cfg.endpoint.clone(),
                    token: Some(tok()),
                },
                MockTransport::ok("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n"),
            )
            .probe()
            .unwrap_err(),
            StudioClient::new(
                StudioConfig {
                    endpoint: cfg.endpoint.clone(),
                    token: Some(tok()),
                },
                MockTransport::ok("HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n"),
            )
            .probe()
            .unwrap_err(),
            StudioClient::new(
                StudioConfig {
                    endpoint: cfg.endpoint.clone(),
                    token: Some(tok()),
                },
                MockTransport::ok("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]"),
            )
            .probe()
            .unwrap_err(),
        ];
        for e in &errors {
            for rendering in [format!("{e}"), format!("{e:?}"), e.remedy()] {
                assert!(
                    !rendering.contains(SECRET),
                    "a refusal leaked the token: {rendering}"
                );
                assert!(
                    !rendering.contains("usk-"),
                    "a refusal leaked a token prefix: {rendering}"
                );
            }
        }
        assert_eq!(errors.len(), 5, "one rendering per refusal shape");
    }

    // -- the three refusals ------------------------------------------------

    #[test]
    fn refusal_not_configured() {
        let client = StudioClient::new(
            StudioConfig {
                endpoint: StudioEndpoint::default(),
                token: None,
            },
            MockTransport::ok(health_200()),
        );
        let err = client.probe().unwrap_err();
        assert_eq!(err, StudioError::NotConfigured);
        // Nothing was sent — the refusal happens before the socket.
        assert!(
            client.transport.sent().is_empty(),
            "a missing credential must not produce a request"
        );
        assert!(err.remedy().contains(TOKEN_ENV), "{}", err.remedy());
    }

    #[test]
    fn refusal_unreachable() {
        let client = StudioClient::new(
            StudioConfig {
                endpoint: StudioEndpoint::new("127.0.0.1", 8888),
                token: Some(tok()),
            },
            MockTransport::fail("connect: Connection refused (os error 111)"),
        );
        let err = client.probe().unwrap_err();
        match &err {
            StudioError::Unreachable { authority, detail } => {
                assert_eq!(authority, "127.0.0.1:8888");
                assert!(detail.contains("refused"), "{detail}");
            }
            other => panic!("expected Unreachable, got {other:?}"),
        }
        assert!(err.remedy().contains("127.0.0.1:8888"), "{}", err.remedy());
    }

    #[test]
    fn refusal_unauthorized() {
        let client = StudioClient::new(
            StudioConfig {
                endpoint: StudioEndpoint::default(),
                token: Some(tok()),
            },
            MockTransport::ok(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\
                 Content-Length: 30\r\n\r\n{\"detail\":\"Not authenticated\"}",
            ),
        );
        let err = client.probe().unwrap_err();
        assert_eq!(err, StudioError::Unauthorized { status: 401 });
        assert!(err.remedy().contains("rotat"), "{}", err.remedy());
    }

    /// 🚨 The distinction test the brief asks for: the three must not be interchangeable.
    /// A guard that cannot tell *unreachable* from *unauthorized* is the bug, not the fence.
    #[test]
    fn the_three_refusals_are_distinguishable() {
        let cfg = |t: Option<StudioToken>| StudioConfig {
            endpoint: StudioEndpoint::new("127.0.0.1", 8888),
            token: t,
        };
        let not_configured = StudioClient::new(cfg(None), MockTransport::ok(health_200()))
            .probe()
            .unwrap_err();
        let unreachable = StudioClient::new(cfg(Some(tok())), MockTransport::fail("connect: refused"))
            .probe()
            .unwrap_err();
        let unauthorized = StudioClient::new(
            cfg(Some(tok())),
            MockTransport::ok("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n"),
        )
        .probe()
        .unwrap_err();

        assert_ne!(not_configured, unreachable);
        assert_ne!(unreachable, unauthorized);
        assert_ne!(not_configured, unauthorized);
        // And they must READ differently, not merely compare differently — the sentence is
        // what a person acts on.
        let sentences = [
            not_configured.remedy(),
            unreachable.remedy(),
            unauthorized.remedy(),
        ];
        assert_ne!(sentences[0], sentences[1]);
        assert_ne!(sentences[1], sentences[2]);
        assert_ne!(sentences[0], sentences[2]);
        for (e, want) in [
            (&not_configured, "Mint an API key"),
            (&unreachable, "Nothing answered"),
            (&unauthorized, "rejected the key"),
        ] {
            assert!(e.remedy().contains(want), "{}", e.remedy());
        }
    }

    /// A 5xx is reachable and authorized; calling it either would send someone to the
    /// wrong fix.
    #[test]
    fn a_server_fault_is_not_one_of_the_three() {
        let err = StudioClient::new(
            StudioConfig {
                endpoint: StudioEndpoint::default(),
                token: Some(tok()),
            },
            MockTransport::ok("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n"),
        )
        .probe()
        .unwrap_err();
        assert_eq!(
            err,
            StudioError::Refused {
                status: 503,
                reason: "Service Unavailable".to_string()
            }
        );
        assert!(err.remedy().contains("fault is on its side"), "{}", err.remedy());
    }

    // -- the probe's documented limitation ---------------------------------

    /// 🚨 Pins the limitation the module doc states: `/api/health` is unauthenticated, so a
    /// green probe says nothing about the credential. If this test ever starts failing it
    /// means the probe learned to validate the token — good news, and the doc, the UI
    /// wording and `StudioError::Unauthorized`'s reachability all have to change with it.
    #[test]
    fn probe_cannot_detect_a_bad_token() {
        let client = StudioClient::new(
            StudioConfig {
                endpoint: StudioEndpoint::default(),
                token: StudioToken::new("obviously-not-a-real-key"),
            },
            // The real Studio answers exactly this to a wrong key on this route.
            MockTransport::ok(health_200()),
        );
        let health = client.probe().expect("health is unauthenticated");
        assert!(health.is_healthy());
        assert_eq!(health.service, "Unsloth UI Backend");
    }

    // -- request composition -----------------------------------------------

    #[test]
    fn the_request_carries_the_bearer_header() {
        let req = build_get(&StudioEndpoint::default(), HEALTH_PATH, Some(&tok()));
        assert!(req.starts_with("GET /api/health HTTP/1.1\r\n"), "{req}");
        assert!(req.contains("Host: 127.0.0.1:8888\r\n"), "{req}");
        assert!(
            req.contains(&format!("Authorization: Bearer {SECRET}\r\n")),
            "the header is the whole point of holding a token"
        );
        assert!(req.contains("Connection: close\r\n"), "{req}");
        assert!(req.ends_with("\r\n\r\n"), "headers must terminate: {req:?}");
    }

    #[test]
    fn no_token_means_no_authorization_header() {
        let req = build_get(&StudioEndpoint::default(), HEALTH_PATH, None);
        assert!(!req.to_ascii_lowercase().contains("authorization"), "{req}");
    }

    #[test]
    fn the_client_actually_sends_the_header() {
        let client = StudioClient::new(
            StudioConfig {
                endpoint: StudioEndpoint::default(),
                token: Some(tok()),
            },
            MockTransport::ok(health_200()),
        );
        client.probe().unwrap();
        let sent = client.transport.sent();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains(&format!("Bearer {SECRET}")), "{}", sent[0]);
    }

    // -- the credential ----------------------------------------------------

    #[test]
    fn a_blank_token_is_no_token() {
        assert!(StudioToken::new("").is_none());
        assert!(StudioToken::new("   \t\n").is_none());
        // Whitespace around a real key is stripped, not preserved into the header — a
        // trailing newline from a shell export would otherwise 401 forever.
        let t = StudioToken::new("  abc\n").unwrap();
        assert_eq!(t.header_value(), "Bearer abc");
        assert_eq!(t.len(), 3);
        assert!(!t.is_empty());
    }

    // -- the endpoint ------------------------------------------------------

    #[test]
    fn endpoint_defaults_to_loopback_8888() {
        let d = StudioEndpoint::default();
        assert_eq!(d.host, "127.0.0.1");
        assert_eq!(d.port, 8888);
        assert_eq!(d.authority(), "127.0.0.1:8888");
    }

    #[test]
    fn endpoint_parses_the_forms_in_circulation() {
        for (input, host, port) in [
            ("127.0.0.1:8888", "127.0.0.1", 8888u16),
            ("http://127.0.0.1:8888", "127.0.0.1", 8888),
            ("http://127.0.0.1:8888/", "127.0.0.1", 8888),
            ("http://192.168.0.7:8888/api/health", "192.168.0.7", 8888),
            ("192.168.0.7", "192.168.0.7", 8888),
            ("  127.0.0.1:9000  ", "127.0.0.1", 9000),
        ] {
            let e = StudioEndpoint::parse(input).unwrap_or_else(|err| panic!("{input}: {err}"));
            assert_eq!((e.host.as_str(), e.port), (host, port), "input {input}");
        }
    }

    /// 📌 The measured trap: `localhost` costs ~200 ms per request against an IPv4-only
    /// listener because `::1` is tried first. Parsing rewrites it away.
    #[test]
    fn localhost_is_rewritten_to_the_v4_loopback() {
        assert_eq!(StudioEndpoint::parse("localhost").unwrap().host, "127.0.0.1");
        assert_eq!(
            StudioEndpoint::parse("http://LocalHost:8888").unwrap().host,
            "127.0.0.1"
        );
        // …but the explicit constructor is an escape hatch and does not second-guess.
        assert_eq!(StudioEndpoint::new("localhost", 8888).host, "localhost");
    }

    #[test]
    fn endpoint_rejects_what_it_cannot_honor() {
        assert_eq!(StudioEndpoint::parse(""), Err(EndpointError::Empty));
        assert_eq!(StudioEndpoint::parse("   "), Err(EndpointError::Empty));
        assert!(matches!(
            StudioEndpoint::parse("127.0.0.1:nope"),
            Err(EndpointError::BadPort(_))
        ));
        assert!(matches!(
            StudioEndpoint::parse("127.0.0.1:0"),
            Err(EndpointError::BadPort(_))
        ));
        assert!(matches!(
            StudioEndpoint::parse("127.0.0.1:99999"),
            Err(EndpointError::BadPort(_))
        ));
        match StudioEndpoint::parse("https://studio.example:8888") {
            Err(e @ EndpointError::NoTls(_)) => {
                assert!(e.to_string().contains("no TLS client"), "{e}");
            }
            other => panic!("https:// must be refused by name, got {other:?}"),
        }
    }

    // -- response classification -------------------------------------------

    #[test]
    fn status_line_is_read_not_guessed() {
        assert_eq!(
            parse_status("HTTP/1.1 200 OK\r\n\r\n"),
            Some((200, "OK".to_string()))
        );
        assert_eq!(
            parse_status("HTTP/1.0 404 Not Found\r\n"),
            Some((404, "Not Found".to_string()))
        );
        assert_eq!(parse_status("HTTP/1.1 204\r\n"), Some((204, String::new())));
        assert_eq!(parse_status("garbage"), None);
        assert_eq!(parse_status(""), None);
    }

    #[test]
    fn classify_maps_status_to_the_right_refusal() {
        let body = classify(&health_200()).expect("200 is not a refusal");
        assert!(serde_json::from_str::<Health>(&body).unwrap().is_healthy());
        assert_eq!(
            classify("HTTP/1.1 401 Unauthorized\r\n\r\n"),
            Err(StudioError::Unauthorized { status: 401 })
        );
        assert_eq!(
            classify("HTTP/1.1 403 Forbidden\r\n\r\n"),
            Err(StudioError::Unauthorized { status: 403 })
        );
        assert_eq!(
            classify("HTTP/1.1 404 Not Found\r\n\r\n"),
            Err(StudioError::Refused {
                status: 404,
                reason: "Not Found".to_string()
            })
        );
        assert!(matches!(
            classify("not http at all"),
            Err(StudioError::Malformed { .. })
        ));
    }

    #[test]
    fn body_extraction_handles_both_framings() {
        assert!(extract_body(&health_200()).starts_with(r#"{"status":"healthy""#));
        // Three chunks of 9, 2 and 9 bytes, sizes in hex, terminated by a 0-chunk.
        let chunked = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                       9\r\n{\"status\"\r\n2\r\n:\"\r\n9\r\nhealthy\"}\r\n0\r\n\r\n";
        assert_eq!(extract_body(chunked), "{\"status\":\"healthy\"}");
        // Content-Length shorter than the bytes present clips rather than over-reads.
        assert_eq!(
            extract_body("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nabcdef"),
            "ab"
        );
    }

    #[test]
    fn health_ignores_fields_it_does_not_know() {
        let h: Health = serde_json::from_str(
            r#"{"status":"healthy","service":"Unsloth UI Backend","studio_root_id":"x","extra":[1,2]}"#,
        )
        .unwrap();
        assert!(h.is_healthy());
        assert_eq!(h.service, "Unsloth UI Backend");
        assert_eq!(h.version, None);
        // A body missing `status` is not a health object.
        assert!(serde_json::from_str::<Health>(r#"{"service":"x"}"#).is_err());
    }

    #[test]
    fn a_2xx_that_is_not_the_health_object_is_malformed() {
        let err = StudioClient::new(
            StudioConfig {
                endpoint: StudioEndpoint::default(),
                token: Some(tok()),
            },
            MockTransport::ok("HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n<html>hi</html>"),
        )
        .probe()
        .unwrap_err();
        assert!(matches!(err, StudioError::Malformed { .. }), "{err:?}");
        assert!(err.remedy().contains("another service on that port"), "{}", err.remedy());
    }

    #[test]
    fn a_malformed_detail_is_bounded() {
        assert_eq!(truncate("short", 200), "short");
        let long = "x".repeat(500);
        let t = truncate(&long, 200);
        assert_eq!(t.chars().count(), 201);
        assert!(t.ends_with('…'));
        // Multi-byte input must not panic on a mid-char boundary.
        let wide = "é".repeat(200);
        assert!(truncate(&wide, 5).ends_with('…'));
    }

    // -- the real socket, without a server ---------------------------------

    /// The one test that opens a real socket, and it is deterministic **because nothing is
    /// listening**: bind an ephemeral port to learn a number the OS just confirmed is free,
    /// drop the listener, then connect to it. Passes on a machine with no Studio, which is
    /// every machine but this one.
    #[test]
    fn tcp_transport_reports_a_closed_port_as_unreachable() {
        let port = {
            let l = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral bind");
            l.local_addr().unwrap().port()
        };
        let client = StudioClient::new(
            StudioConfig {
                endpoint: StudioEndpoint::new("127.0.0.1", port),
                token: Some(tok()),
            },
            TcpTransport,
        );
        let err = client.probe().unwrap_err();
        match &err {
            StudioError::Unreachable { authority, detail } => {
                assert_eq!(authority, &format!("127.0.0.1:{port}"));
                assert!(detail.starts_with("connect: "), "{detail}");
            }
            other => panic!("expected Unreachable from a closed port, got {other:?}"),
        }
        assert!(!format!("{err:?}").contains(SECRET));
    }

    // -- the connect budget ------------------------------------------------

    #[test]
    fn a_literal_ip_resolves_without_dns() {
        let v4 = resolve_addrs("127.0.0.1", 8888).unwrap();
        assert_eq!(v4, vec![SocketAddr::from(([127, 0, 0, 1], 8888))]);
        let lan = resolve_addrs("192.168.0.7", 8888).unwrap();
        assert_eq!(lan, vec![SocketAddr::from(([192, 168, 0, 7], 8888))]);
        // An IPv6 literal too — it is not a name and must not reach the resolver either.
        let v6 = resolve_addrs("::1", 8888).unwrap();
        assert_eq!(v6.len(), 1);
        assert!(v6[0].is_ipv6());
    }

    /// A name nothing can resolve must still produce an `Unreachable` naming the cause —
    /// not a panic, and not a silent fall through to some default address.
    #[test]
    fn an_unresolvable_name_is_an_actionable_refusal() {
        // `.invalid` is reserved by RFC 2606 and guaranteed never to resolve.
        let err = resolve_addrs("studio.invalid", 8888).unwrap_err();
        assert!(err.starts_with("resolve: "), "{err}");
        assert!(err.contains("studio.invalid"), "{err}");

        let client = StudioClient::new(
            StudioConfig {
                endpoint: StudioEndpoint::new("studio.invalid", 8888),
                token: Some(tok()),
            },
            TcpTransport,
        );
        match client.probe().unwrap_err() {
            StudioError::Unreachable { authority, detail } => {
                assert_eq!(authority, "studio.invalid:8888");
                assert!(detail.starts_with("resolve: "), "{detail}");
            }
            other => panic!("expected Unreachable from an unresolvable name, got {other:?}"),
        }
    }

    /// 🚨 The budget is spent as a **total**, and an exhausted one is recognised *before*
    /// `connect_timeout` is called rather than inside it.
    ///
    /// Deterministic without a firewalled host: point it at a listener that is genuinely
    /// accepting, hand it a zero budget, and assert both that it refused **and that the
    /// listener never saw a connection**. A version that spent the budget per-attempt, or
    /// that dropped the guard and let the OS reject a zero duration, fails on the wording;
    /// a version that connected anyway fails on the accept.
    #[test]
    fn an_exhausted_budget_refuses_before_it_connects() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let addr = listener.local_addr().unwrap();

        let err = connect_within(&[addr], Duration::ZERO).unwrap_err();
        assert!(err.starts_with("connect: gave up after 0s"), "{err}");
        assert!(err.contains("dropped packet"), "the cause has to be nameable: {err}");
        assert!(err.contains(&addr.to_string()), "{err}");
        assert!(
            listener.accept().is_err(),
            "a spent budget must not open a connection"
        );

        // …and the same listener, with a real budget, is reached at once — so the refusal
        // above is the budget talking and not a broken address.
        connect_within(&[addr], Duration::from_secs(TIMEOUT_SECS)).expect("a live listener");
    }

    #[test]
    fn an_empty_address_list_is_a_refusal_not_a_panic() {
        let err = connect_within(&[], Duration::from_secs(1)).unwrap_err();
        assert_eq!(err, "connect: no address to try");
    }

    /// The other real-socket test, and the one that keeps [`TcpTransport`] from being
    /// present-but-untrodden code: a listener we stand up ourselves plays the Studio for one
    /// request. It exercises the whole path — [`build_get`] → socket → [`classify`] →
    /// serde — and it asserts the **server** saw the `Authorization` header, which the mock
    /// can only assert about the string handed to it.
    ///
    /// Needs no Studio and no key: the listener is this test's own.
    #[test]
    fn tcp_transport_round_trips_against_a_local_listener() {
        use std::io::{BufRead, BufReader, Write};

        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(&stream);
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                headers.push(line);
            }
            let body =
                r#"{"status":"healthy","service":"Unsloth UI Backend","version":"2026.8.19"}"#;
            let mut out = &stream;
            let _ = out.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
            let _ = out.flush();
            headers
        });

        let client = StudioClient::new(
            StudioConfig {
                endpoint: StudioEndpoint::new("127.0.0.1", port),
                token: Some(tok()),
            },
            TcpTransport,
        );
        let health = client.probe().expect("a well-formed 200 is not a refusal");
        assert!(health.is_healthy());
        assert_eq!(health.service, "Unsloth UI Backend");
        assert_eq!(health.version.as_deref(), Some("2026.8.19"));

        let headers = server.join().expect("server thread");
        assert!(
            headers[0].starts_with("GET /api/health HTTP/1.1"),
            "{:?}",
            headers[0]
        );
        assert!(
            headers
                .iter()
                .any(|h| h.trim() == format!("Authorization: Bearer {SECRET}")),
            "the server never saw the bearer header: {headers:?}"
        );
        assert!(
            headers.iter().any(|h| h.trim() == format!("Host: 127.0.0.1:{port}")),
            "{headers:?}"
        );
    }
}
