//! The training strip and the run shelf — Unsloth Studio's live telemetry (#147 T4).
//!
//! [`crate::unsloth`] answered *can we talk to the Studio*. This module answers the next
//! question: **what is it doing right now, and what has it done before.** Three routes,
//! all bearer-authed, all read-only — this module never `POST`s and cannot start, stop or
//! alter a training run:
//!
//! | Route | Shape | Used for |
//! |---|---|---|
//! | `GET /api/train/progress` | **SSE**, named events, `id:`, `retry:`, honours `Last-Event-ID` | the live strip |
//! | `GET /api/train/metrics` | one JSON object of histories | backfill on connect **and on every reconnect** |
//! | `GET /api/train/runs` | an array of run summaries | the shelf, and the first authenticated call |
//!
//! # 🚨 What each state actually asserts, and why there is no health probe here
//!
//! T1 found that `GET /api/health` is **unauthenticated**: it answers `200` with a correct
//! key, a wrong key, and no key at all. So a green probe proves the Studio is *running* and
//! never that the credential is good — pinned over there by `probe_cannot_detect_a_bad_token`.
//!
//! ⚠️ The tempting shape for this tier would be *probe, go green, then stream*. That green
//! would be a status line that cannot be wrong. **So this module never calls
//! [`crate::unsloth::StudioClient::probe`] at all.** It opens with an *authenticated* call
//! ([`RUNS_PATH`]), because the first authenticated call is the first evidence the credential
//! works. Every state below is therefore a claim the wire actually supports:
//!
//! | [`LinkState`] | Studio running? | Credential good? | Run in progress? |
//! |---|---|---|---|
//! | [`LinkState::Unknown`] | unknown | unknown | unknown |
//! | [`LinkState::NotConfigured`] | **not asked** — nothing was sent | we hold none | unknown |
//! | [`LinkState::Unreachable`] | **no** — nothing answered | unknown, and stays unknown | unknown |
//! | [`LinkState::Unauthorized`] | **yes** — a `401` is an answer | **no**, and this is the only state that says so | unknown |
//! | [`LinkState::Refused`] | **yes** | unknown — a `500` is not a verdict on the key | unknown |
//! | [`LinkState::Malformed`] | something is there | unknown | unknown |
//! | [`LinkState::Idle`] | **yes** | **yes** — an authenticated `2xx` came back | **no** |
//! | [`LinkState::Live`] | **yes** | **yes** | **yes** |
//!
//! 📌 So the three sentences the reader has to be able to tell apart —
//! *"nothing is training"*, *"I cannot reach the Studio"*, *"my key is wrong"* — are
//! [`LinkState::Idle`], [`LinkState::Unreachable`] and [`LinkState::Unauthorized`], and they
//! are three variants with three [`LinkState::headline`] sentences and three
//! [`LinkState::remedy`] instructions. [`LinkState::asserts`] returns the row above in
//! prose, so a UI can show what a state means rather than implying it.
//!
//! # 📌 The Studio being absent is the normal case
//!
//! It is not running on organon-one most of the time and Organon must be comfortable with
//! that. Concretely: **no thread and no socket at all without a credential**
//! ([`LinkState::NotConfigured`] is reached with nothing sent), [`Severity::Quiet`] for every
//! absence state so nothing renders alarming, and a bounded reconnect — [`RECONNECT_MIN`]
//! doubling to [`RECONNECT_MAX`], giving up after [`RECONNECT_GIVE_UP_AFTER`] attempts and
//! parking until a person asks again ([`TrainingLink::retry_now`]). An unreachable Studio
//! costs a refused loopback connect every minute at worst, then nothing.
//!
//! ⚠️ [`LinkState::Unauthorized`] does **not** retry even once. A key does not become valid
//! by being resent, and on Windows a process cannot see a `UNSLOTH_API_KEY` rotated after it
//! started — so retrying would be a loop that provably cannot succeed.
//!
//! # 🚨 The wire is fiddlier than "read lines and parse JSON"
//!
//! Three framings are stacked on this socket and each one has a trap:
//!
//! 1. **HTTP/1.1 chunked transfer encoding.** A streaming FastAPI response is
//!    `Transfer-Encoding: chunked`, so the raw socket bytes carry hex size lines
//!    *interleaved with the SSE text*. Feeding those straight to an SSE parser makes the
//!    size lines look like malformed SSE fields — they parse as unknown fields and are
//!    silently ignored, which is the worst possible failure because the stream *appears* to
//!    work. [`ChunkedDecoder`] sits between the socket and the parser and is incremental,
//!    because a chunk boundary lands wherever the network puts it.
//! 2. **SSE framing.** Events are separated by a **blank line**; several `data:` lines in
//!    one event **concatenate with `\n`**; a line starting `:` is a **comment** and must be
//!    dropped without being mistaken for data; `event:` names the type; `id:` sets the
//!    reconnect cursor. [`SseParser`] follows the WHATWG rules including `\r\n` / `\n` / `\r`
//!    terminators and the leading-BOM strip.
//! 3. **Reads land anywhere.** A single `read()` can end mid-line, mid-chunk, mid-event, or
//!    on the `\r` of a `\r\n`. Every decoder here is fed-and-buffered rather than
//!    line-oriented, and the tests drive one byte at a time to prove it.
//!
//! 🚨 **Reconnect sends `Last-Event-ID`, and losing it loses steps silently** — a gap in a
//! loss curve reads as a flat spot, not as an error. [`build_stream_get`] emits the header
//! and [`SseReader::last_event_id`] is the cursor to carry across. Backfill from
//! [`METRICS_PATH`] runs on *every* connect for the same reason: it is the belt to the
//! header's braces, and [`TrainingStrip`] de-duplicates by step so the overlap is free.
//!
//! # Timeouts — deliberately not T1's
//!
//! ⚠️ [`crate::unsloth::TIMEOUT_SECS`] is 5 s because `/api/health` is a constant-time
//! answer. **A stream that is silent between heartbeats is healthy**, so reusing 5 s would
//! tear down a working connection every five seconds. This module uses two constants
//! instead: [`STREAM_POLL_SECS`] as the socket read timeout (short, so the stop flag is
//! honoured promptly) and [`STREAM_IDLE_SECS`] as the total silence budget (long, because
//! silence is normal). T1's constant is untouched.
//!
//! # Where the parts live
//!
//! Everything here is pure or `std`-only: parsing, the fold, the state machine, and one
//! worker thread over a [`std::net::TcpStream`]. **No new dependency**, and nothing that
//! could reach `nih_plug` / `wgpu` / `egui` / `winit`. The drawing is
//! `organon_mind::mind_train`, and the readout is editor-side — **no `Shared` field and no
//! `LAYOUT_VERSION` movement**, per `doc/organon_mind_training_lens.md` §3.
//!
//! 🚨 **Nothing here has ever spoken to a running Studio.** It was not running on organon-one
//! when this landed, so the payload field names for the SSE `progress` event in particular
//! are inferred from the documented `TrainingMetricsResponse` naming, not read off a live
//! wire. Every field is optional with aliases for the plausible spellings, so a mismatch
//! degrades to a missing number rather than to a failed parse — but a missing number is
//! exactly what a wrong guess looks like, and that is recorded rather than hidden.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::unsloth::{
    classify, connect_within, resolve_addrs, StudioConfig, StudioEndpoint, StudioError,
    StudioToken, TIMEOUT_SECS,
};

// ===========================================================================
// Routes and budgets
// ===========================================================================

/// The SSE progress stream.
pub const PROGRESS_PATH: &str = "/api/train/progress";

/// The history object used to backfill on connect and on every reconnect.
pub const METRICS_PATH: &str = "/api/train/metrics";

/// The run shelf, and — because it is authenticated — the call that proves the credential.
pub const RUNS_PATH: &str = "/api/train/runs";

/// Socket read timeout on the open stream.
///
/// ⚠️ **This is not a liveness budget, it is a responsiveness budget.** A read that returns
/// nothing within a second is normal; what the short timeout buys is that the worker gets
/// back to the top of its loop and can see [`TrainingLink`]'s stop flag, so closing the
/// editor does not wait on a quiet socket. Liveness is [`STREAM_IDLE_SECS`], counted across
/// however many of these expire in a row.
pub const STREAM_POLL_SECS: u64 = 1;

/// Total silence before the stream is treated as dead and reconnected.
///
/// ⚠️ **Deliberately much longer than [`crate::unsloth::TIMEOUT_SECS`].** That 5 s is right
/// for a constant-time route and wrong here: the Studio's own SSE emits a `heartbeat`, and a
/// stream between heartbeats is healthy, not stalled. 90 s is a few missed heartbeats rather
/// than one — a reconnect is cheap (the `Last-Event-ID` header plus the metrics backfill make
/// it lossless) but it is not free, and tearing down a working connection on one slow beat is
/// the failure this number exists to avoid.
pub const STREAM_IDLE_SECS: u64 = 90;

/// First reconnect delay.
pub const RECONNECT_MIN: Duration = Duration::from_secs(2);

/// Reconnect delay ceiling. Doubling from [`RECONNECT_MIN`] stops here.
pub const RECONNECT_MAX: Duration = Duration::from_secs(60);

/// Consecutive failures before the worker parks instead of retrying.
///
/// 📌 **The Studio being off is the normal case**, so the loop must end rather than knock at
/// a closed door forever. Eight attempts with the backoff above is a little over four
/// minutes of trying; after that the state stands and [`TrainingLink::retry_now`] is how a
/// person says "it is up now".
pub const RECONNECT_GIVE_UP_AFTER: u32 = 8;

/// How many points of each live curve are kept.
///
/// ⚠️ The **newest** are kept: a live strip is about the run's recent shape, and a run past
/// this length has its whole history in [`RunSummary::loss_sparkline`] on the shelf anyway.
pub const CURVE_CAP: usize = 4096;

/// Largest response body accepted from a one-shot route, in bytes.
///
/// ⚠️ Not paranoia about the Studio — a bound on what a *wrong thing on that port* can make
/// us hold. `read_to_end` on a socket has no limit of its own.
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

// ===========================================================================
// SSE framing
// ===========================================================================

/// One dispatched Server-Sent Event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {
    /// The `event:` field. **Empty means the default type**, which the SSE spec calls
    /// `message` — kept as `""` rather than substituted so a caller can tell an explicit
    /// `event: message` from an absent field if it ever matters.
    pub event: String,
    /// The `data:` field(s), several lines joined with `\n` and the trailing one stripped.
    pub data: String,
    /// The last-event-id **in force when this event dispatched** — which is not necessarily
    /// this event's own `id:` line, because the id persists across events that omit it. That
    /// is the spec's rule and it is what makes the reconnect cursor correct.
    pub id: Option<String>,
    /// The `retry:` reconnection time in milliseconds, if one has ever been sent.
    pub retry: Option<u64>,
}

impl SseEvent {
    /// The effective event type: `"message"` for an absent `event:` field.
    pub fn kind(&self) -> &str {
        if self.event.is_empty() {
            "message"
        } else {
            &self.event
        }
    }
}

/// An incremental Server-Sent Events parser.
///
/// 🚨 **Fed bytes, not lines.** A socket read ends wherever the network decided, including in
/// the middle of a field name, between the `\r` and the `\n` of a terminator, or between the
/// last `data:` line and the blank line that dispatches it. A parser that split each read on
/// newlines would work perfectly against a friendly server and corrupt against a real one,
/// so this one buffers and the tests feed it one byte at a time.
///
/// Follows the WHATWG rules: `\r\n` / `\n` / `\r` all terminate a line; a leading UTF-8 BOM is
/// dropped once, at the very start; a line beginning `:` is a comment and is discarded; a
/// field with no `:` has an empty value; one leading space is stripped from a value; several
/// `data:` lines join with `\n`; and **an event whose data buffer was never written does not
/// dispatch** — which is what makes a lone `id:`/`retry:` line update the cursor without
/// producing a phantom event.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    event: String,
    data: String,
    has_data: bool,
    last_id: Option<String>,
    retry: Option<u64>,
    bom_settled: bool,
}

/// The offsets of the next complete line in `buf`: `(line_len, terminator_len)`.
///
/// ⚠️ **Returns `None` for a buffer ending in a bare `\r`**, which is the subtle half. A
/// trailing `\r` may yet turn out to be the first byte of a `\r\n`, and treating it as a
/// terminator would dispatch an event one read early and then read an empty line from the
/// `\n` that arrives next — a spurious blank line, i.e. a spurious dispatch.
fn next_line(buf: &[u8]) -> Option<(usize, usize)> {
    let i = buf.iter().position(|&b| b == b'\n' || b == b'\r')?;
    if buf[i] == b'\n' {
        return Some((i, 1));
    }
    match buf.get(i + 1) {
        Some(b'\n') => Some((i, 2)),
        Some(_) => Some((i, 1)),
        None => None,
    }
}

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

impl SseParser {
    /// A fresh parser.
    pub fn new() -> SseParser {
        SseParser::default()
    }

    /// A parser resuming a stream, so its cursor is right before the first event arrives.
    pub fn resuming(last_id: Option<String>) -> SseParser {
        SseParser {
            last_id,
            ..SseParser::default()
        }
    }

    /// The reconnect cursor: the most recent `id:` seen. **Send this as `Last-Event-ID`.**
    pub fn last_event_id(&self) -> Option<&str> {
        self.last_id.as_deref()
    }

    /// The server's requested reconnection delay in milliseconds, if it sent a `retry:`.
    pub fn retry_ms(&self) -> Option<u64> {
        self.retry
    }

    /// Feed raw (already de-chunked) stream bytes; returns every event that completed.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(bytes);
        self.strip_bom();
        let mut out = Vec::new();
        while let Some((len, term)) = next_line(&self.buf) {
            let line = String::from_utf8_lossy(&self.buf[..len]).into_owned();
            self.buf.drain(..len + term);
            if let Some(ev) = self.line(&line) {
                out.push(ev);
            }
        }
        out
    }

    /// Drop a UTF-8 BOM, once, and only if it is the very first thing on the stream.
    fn strip_bom(&mut self) {
        if self.bom_settled {
            return;
        }
        if self.buf.len() >= BOM.len() {
            if self.buf.starts_with(&BOM) {
                self.buf.drain(..BOM.len());
            }
            self.bom_settled = true;
        } else if !self.buf.is_empty() && !BOM.starts_with(&self.buf[..]) {
            // Whatever arrived cannot be the start of a BOM, so stop watching for one.
            self.bom_settled = true;
        }
    }

    fn line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        // 🚨 A comment. The Studio's keep-alives may arrive this way, and a parser that let
        // one through as data would push a garbage point onto the loss curve.
        //
        // ✏️ **Measured by mutation: deleting this line changes nothing today, and it stays
        // anyway.** With the field split below taking the FIRST colon, `: keep-alive` falls
        // out as field `""` / value `"keep-alive"` — an unknown field, discarded. So the
        // guard's redundancy is *contingent on that split*, and the shape it protects
        // against is the naive parser this module's doc warns about: one that treats every
        // non-blank line as data. Removing it would make correctness here depend on a
        // property two screens away, which is how a defensive line becomes a bug later.
        if line.starts_with(':') {
            return None;
        }
        let (field, raw) = match line.split_once(':') {
            Some((f, v)) => (f, v),
            None => (line, ""),
        };
        let value = raw.strip_prefix(' ').unwrap_or(raw);
        match field {
            "event" => self.event = value.to_string(),
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
                self.has_data = true;
            }
            // A NUL in an id is ignored by the spec, and it would also be a header-injection
            // shaped value on the way back out as `Last-Event-ID`.
            "id" => {
                if !value.contains('\0') {
                    self.last_id = Some(value.to_string());
                }
            }
            "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                self.retry = value.parse().ok();
            }
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if !self.has_data {
            // Not an event: a blank line after only `id:`/`retry:`/comments updates the
            // cursor and produces nothing, which is the spec's rule and also the sane one.
            self.event.clear();
            self.data.clear();
            return None;
        }
        if self.data.ends_with('\n') {
            self.data.pop();
        }
        let ev = SseEvent {
            event: std::mem::take(&mut self.event),
            data: std::mem::take(&mut self.data),
            id: self.last_id.clone(),
            retry: self.retry,
        };
        self.has_data = false;
        Some(ev)
    }
}

// ===========================================================================
// HTTP/1.1 chunked framing
// ===========================================================================

/// An incremental `Transfer-Encoding: chunked` decoder.
///
/// 🚨 **Why this exists at all.** `unsloth::extract_body` de-chunks a *complete* body; a
/// stream has no complete body, so the size lines have to be removed as they arrive. Skip
/// this and the SSE parser sees lines like `2f` and `0` between the events — they parse as
/// unknown SSE fields and are dropped without complaint, so the stream looks like it is
/// working while every chunk boundary silently corrupts whichever event it lands inside.
#[derive(Debug, Default)]
pub struct ChunkedDecoder {
    buf: Vec<u8>,
    /// Bytes still owed by the chunk currently being read.
    remaining: usize,
    /// True once the terminating zero-length chunk has been seen.
    done: bool,
    /// The CRLF that follows a chunk body, still to be consumed.
    trailing_crlf: usize,
}

impl ChunkedDecoder {
    /// A decoder at the start of a chunked body.
    pub fn new() -> ChunkedDecoder {
        ChunkedDecoder::default()
    }

    /// Whether the terminating `0` chunk has arrived.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Feed raw socket bytes; returns the decoded body bytes available so far.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            if self.done {
                break;
            }
            if self.trailing_crlf > 0 {
                let take = self.trailing_crlf.min(self.buf.len());
                self.buf.drain(..take);
                self.trailing_crlf -= take;
                if self.trailing_crlf > 0 {
                    break;
                }
                continue;
            }
            if self.remaining > 0 {
                if self.buf.is_empty() {
                    break;
                }
                let take = self.remaining.min(self.buf.len());
                out.extend_from_slice(&self.buf[..take]);
                self.buf.drain(..take);
                self.remaining -= take;
                if self.remaining == 0 {
                    self.trailing_crlf = 2;
                }
                continue;
            }
            // Reading a size line. A partial one waits for more bytes.
            let Some((len, term)) = next_crlf_line(&self.buf) else {
                break;
            };
            let head = String::from_utf8_lossy(&self.buf[..len]).into_owned();
            self.buf.drain(..len + term);
            // A chunk extension (`1a;name=value`) is legal and ignored.
            let size_str = head.split(';').next().unwrap_or("").trim();
            if size_str.is_empty() {
                // The CRLF that followed the previous chunk body, if a server sent the pair
                // separately from the body. Harmless: loop again for the real size line.
                continue;
            }
            match usize::from_str_radix(size_str, 16) {
                Ok(0) => {
                    self.done = true;
                    break;
                }
                Ok(n) => self.remaining = n,
                Err(_) => {
                    // Not a chunked body after all, or corrupt. Stop rather than emit
                    // garbage; the caller's idle timer turns this into a reconnect.
                    self.done = true;
                    break;
                }
            }
        }
        out
    }
}

/// The next `\r\n`-terminated line, or `None` if the buffer does not hold a whole one.
/// ⚠️ Chunked framing is strictly CRLF — unlike SSE, a bare `\n` is not a terminator here.
fn next_crlf_line(buf: &[u8]) -> Option<(usize, usize)> {
    let i = buf.windows(2).position(|w| w == b"\r\n")?;
    Some((i, 2))
}

// ===========================================================================
// Response head
// ===========================================================================

/// The parts of an HTTP/1.1 response head this module acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHead {
    pub status: u16,
    pub reason: String,
    /// `Transfer-Encoding` names `chunked`.
    pub chunked: bool,
    /// Lowercased `Content-Type`, value only (no parameters stripped).
    pub content_type: String,
}

/// Split a response head out of `buf`, returning it and the offset the body starts at.
/// `None` while the terminating blank line has not arrived — heads arrive in pieces too.
pub fn split_head(buf: &[u8]) -> Option<(ResponseHead, usize)> {
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or("").trim();
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    let status = if version.starts_with("HTTP/") {
        parts.next().and_then(|c| c.parse::<u16>().ok()).unwrap_or(0)
    } else {
        0
    };
    let reason = parts.next().unwrap_or("").trim().to_string();
    let mut chunked = false;
    let mut content_type = String::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        } else if name == "content-type" {
            content_type = value.to_ascii_lowercase();
        }
    }
    Some((
        ResponseHead {
            status,
            reason,
            chunked,
            content_type,
        },
        end + 4,
    ))
}

// ===========================================================================
// The composed reader: head → de-chunk → SSE
// ===========================================================================

enum BodyDecoder {
    Identity,
    Chunked(ChunkedDecoder),
}

/// Raw socket bytes in, [`SseEvent`]s out — head split, de-chunked and parsed.
///
/// Kept separate from the worker thread so the *whole* wire path is testable with no socket:
/// a test hands it a byte string, in whatever slices it likes, and asserts on the events.
pub struct SseReader {
    head: Option<ResponseHead>,
    pending: Vec<u8>,
    decoder: BodyDecoder,
    sse: SseParser,
    failed: bool,
}

impl SseReader {
    /// A reader for a fresh connection, resuming from `last_id` if there is one.
    pub fn new(last_id: Option<String>) -> SseReader {
        SseReader {
            head: None,
            pending: Vec::new(),
            decoder: BodyDecoder::Identity,
            sse: SseParser::resuming(last_id),
            failed: false,
        }
    }

    /// The response head, once enough bytes have arrived to have one.
    pub fn head(&self) -> Option<&ResponseHead> {
        self.head.as_ref()
    }

    /// The reconnect cursor to carry into the next connection.
    pub fn last_event_id(&self) -> Option<&str> {
        self.sse.last_event_id()
    }

    /// The server's `retry:` hint, in milliseconds.
    pub fn retry_ms(&self) -> Option<u64> {
        self.sse.retry_ms()
    }

    /// Whether the chunked body reached its terminating chunk (the server closed the stream).
    pub fn body_done(&self) -> bool {
        match &self.decoder {
            BodyDecoder::Chunked(c) => c.is_done(),
            BodyDecoder::Identity => false,
        }
    }

    /// Feed raw socket bytes.
    ///
    /// The `Err` is produced **once**, the moment a head with a non-2xx status is parsed, and
    /// it uses T1's taxonomy so `401` on this route means exactly what it means on any other.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, StudioError> {
        if self.failed {
            return Ok(Vec::new());
        }
        if self.head.is_none() {
            self.pending.extend_from_slice(bytes);
            let Some((head, offset)) = split_head(&self.pending) else {
                return Ok(Vec::new());
            };
            let body = self.pending.split_off(offset);
            self.pending.clear();
            if head.status == 0 {
                self.failed = true;
                return Err(StudioError::Malformed {
                    detail: "no HTTP status line on the progress stream".to_string(),
                });
            }
            if !(200..=299).contains(&head.status) {
                self.failed = true;
                let err = match head.status {
                    401 | 403 => StudioError::Unauthorized {
                        status: head.status,
                    },
                    s => StudioError::Refused {
                        status: s,
                        reason: head.reason.clone(),
                    },
                };
                self.head = Some(head);
                return Err(err);
            }
            self.decoder = if head.chunked {
                BodyDecoder::Chunked(ChunkedDecoder::new())
            } else {
                BodyDecoder::Identity
            };
            self.head = Some(head);
            return Ok(self.push_body(&body));
        }
        Ok(self.push_body(bytes))
    }

    fn push_body(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        match &mut self.decoder {
            BodyDecoder::Identity => self.sse.feed(bytes),
            BodyDecoder::Chunked(c) => {
                let decoded = c.feed(bytes);
                self.sse.feed(&decoded)
            }
        }
    }
}

// ===========================================================================
// Payloads
// ===========================================================================

/// Read a step index that may arrive as an integer or as a float.
///
/// ⚠️ A `Vec<u64>` field rejects `[1.0, 2.0]` outright, which would fail the *whole* backfill
/// object and lose the curve over a JSON-number spelling. Since nothing here has been read
/// off a live Studio, that is a plausible enough shape to absorb — but a value that is
/// neither is still an error, so a genuinely wrong body is still reported rather than
/// quietly shortened.
fn de_steps<'de, D>(d: D) -> Result<Vec<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    let raw = Vec::<serde_json::Value>::deserialize(d)?;
    raw.into_iter()
        .map(|v| match &v {
            serde_json::Value::Number(n) => n
                .as_u64()
                .or_else(|| n.as_f64().map(|f| f.max(0.0) as u64))
                .ok_or_else(|| D::Error::custom(format!("step {v} is not a number"))),
            other => Err(D::Error::custom(format!("step {other} is not a number"))),
        })
        .collect()
}

/// One `progress` event's payload.
///
/// 🚨 **Every field is optional and aliased, and that is a statement about confidence, not
/// generosity.** The documented shape here is [`TrainingMetrics`] (the backfill route); the
/// SSE payload's own field names have never been read off a running Studio. Aliases cover the
/// spellings the metrics object uses for the same quantities, so a mismatch shows up as a
/// blank number in the strip rather than as a parse failure that discards the whole event.
/// ⚠️ **A blank number is also what a wrong guess looks like**, which is why
/// [`TrainingStrip::events_seen`] is displayed beside the values: events arriving while every
/// number stays empty is the tell.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ProgressEvent {
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default, alias = "current_step")]
    pub step: Option<u64>,
    #[serde(default, alias = "max_steps")]
    pub total_steps: Option<u64>,
    #[serde(default, alias = "current_loss")]
    pub loss: Option<f64>,
    #[serde(default, alias = "lr", alias = "current_lr")]
    pub learning_rate: Option<f64>,
    #[serde(default, alias = "grad_norm_value")]
    pub grad_norm: Option<f64>,
    #[serde(default)]
    pub epoch: Option<f64>,
    #[serde(default, alias = "phase")]
    pub status: Option<String>,
    #[serde(default, alias = "message")]
    pub detail: Option<String>,
}

/// `GET /api/train/metrics` — the histories, used for backfill.
///
/// ⚠️ **`grad_norm_history` is indexed by `grad_norm_step_history`, not by `step_history`.**
/// The Studio serves two step vectors because the gradient norm is not logged at every step
/// a loss is. Pairing the gradient norms with the loss's steps would draw a curve whose x
/// axis is quietly wrong — the shape survives, the alignment does not, and nothing errors.
/// [`TrainingStrip::apply_metrics`] uses the right vector for each and
/// [`TrainingStrip::misaligned_backfill`] records when a vector did not line up at all.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct TrainingMetrics {
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub current_loss: Option<f64>,
    #[serde(default)]
    pub current_lr: Option<f64>,
    #[serde(default)]
    pub current_step: Option<u64>,
    #[serde(default)]
    pub loss_history: Vec<f64>,
    #[serde(default)]
    pub lr_history: Vec<f64>,
    #[serde(default)]
    pub grad_norm_history: Vec<f64>,
    #[serde(default, deserialize_with = "de_steps")]
    pub grad_norm_step_history: Vec<u64>,
    #[serde(default, deserialize_with = "de_steps")]
    pub step_history: Vec<u64>,
}

/// One row of the run shelf: a finished (or running) training run as a persistent object.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct RunSummary {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub final_loss: Option<f64>,
    #[serde(default)]
    pub final_step: Option<u64>,
    #[serde(default)]
    pub total_steps: Option<u64>,
    #[serde(default)]
    pub loss_sparkline: Vec<f64>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub dataset_name: Option<String>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub can_resume: bool,
}

impl RunSummary {
    /// A short human duration: `1m 04s`, `2h 13m`, `41s`. `"—"` when the Studio sent none.
    pub fn duration_label(&self) -> String {
        let Some(s) = self.duration_seconds.filter(|d| d.is_finite() && *d >= 0.0) else {
            return "—".to_string();
        };
        let total = s.round() as u64;
        if total < 60 {
            format!("{total}s")
        } else if total < 3600 {
            format!("{}m {:02}s", total / 60, total % 60)
        } else {
            format!("{}h {:02}m", total / 3600, (total % 3600) / 60)
        }
    }

    /// `Some(0.0..=1.0)` when both step counts are known and the total is positive.
    pub fn progress(&self) -> Option<f32> {
        let total = self.total_steps.filter(|t| *t > 0)?;
        let done = self.final_step?;
        Some((done as f32 / total as f32).clamp(0.0, 1.0))
    }

    /// Whether the Studio calls this run finished-and-good.
    pub fn is_complete(&self) -> bool {
        matches!(
            self.status.to_ascii_lowercase().as_str(),
            "complete" | "completed" | "finished" | "success" | "succeeded"
        )
    }

    /// Whether the Studio calls this run failed or cancelled.
    pub fn is_failed(&self) -> bool {
        matches!(
            self.status.to_ascii_lowercase().as_str(),
            "failed" | "error" | "errored" | "cancelled" | "canceled" | "stopped" | "aborted"
        )
    }

    /// Whether the Studio calls this run still going.
    pub fn is_running(&self) -> bool {
        matches!(
            self.status.to_ascii_lowercase().as_str(),
            "running" | "training" | "in_progress" | "started"
        )
    }
}

/// Read `GET /api/train/runs`, accepting either a bare array or `{"runs": [...]}`.
///
/// ⚠️ The documented shape is the bare array. The envelope is accepted too because the whole
/// route has never been called here, and a one-word difference between the spec and the
/// build on this machine would otherwise present as an empty shelf — a state indistinguishable
/// from "you have never trained anything", which is the wrong sentence to show someone with
/// forty runs on disk.
pub fn parse_runs(body: &str) -> Result<Vec<RunSummary>, StudioError> {
    #[derive(Deserialize)]
    struct Envelope {
        #[serde(default)]
        runs: Vec<RunSummary>,
    }
    if let Ok(v) = serde_json::from_str::<Vec<RunSummary>>(body) {
        return Ok(v);
    }
    match serde_json::from_str::<Envelope>(body) {
        Ok(e) => Ok(e.runs),
        Err(e) => Err(StudioError::Malformed {
            detail: clip(&e.to_string(), 200),
        }),
    }
}

fn clip(s: &str, max: usize) -> String {
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
// The link state
// ===========================================================================

/// How loudly a state should read. **Absence is quiet** — see the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Normal, including every form of "not here". Nothing to alarm anybody with.
    Quiet,
    /// A run is delivering.
    Active,
    /// Something a person can and should fix.
    Attention,
}

/// What the link to the Studio currently is. **Read the module doc's table** — each variant
/// asserts a specific, different thing, and the whole point of the tier is that they do not
/// collapse into "cannot connect".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LinkState {
    /// Nothing has been attempted yet.
    #[default]
    Unknown,
    /// No `UNSLOTH_API_KEY`. **Nothing was sent** — this is not a failed connection.
    NotConfigured,
    /// ✏️ **A sixth state T1's five refusals do not cover, and it took building a UI to
    /// notice.** `ORGANON_UNSLOTH_ENDPOINT` is unreadable — a bad port, an `https://` — so
    /// there is no address to try. T1 reports that as an [`crate::unsloth::EndpointError`]
    /// from `StudioConfig::from_env`, which is a *different type* from [`StudioError`] and
    /// therefore has no place in a taxonomy built out of the latter. Folding it into
    /// [`LinkState::Malformed`] would have been the easy move and would have told somebody
    /// with a typo in an environment variable to go and check what is listening on a port.
    Misconfigured { detail: String },
    /// The socket produced no reply. Says nothing about the credential.
    Unreachable { authority: String, detail: String },
    /// An **authenticated** route answered `401`/`403`. The only state that judges the key.
    Unauthorized { status: u16 },
    /// An authenticated route answered some other non-2xx. The Studio's fault, not ours.
    Refused { status: u16, reason: String },
    /// An authenticated 2xx whose body was not what the route promises.
    Malformed { detail: String },
    /// An authenticated route answered. **The credential is proven good** and nothing is
    /// training.
    Idle,
    /// The credential is proven good and progress is arriving.
    Live,
}

impl LinkState {
    /// The one line a person reads first.
    pub fn headline(&self) -> String {
        match self {
            LinkState::Unknown => "not connected yet".to_string(),
            LinkState::NotConfigured => "no Unsloth Studio key".to_string(),
            LinkState::Misconfigured { .. } => "Studio endpoint setting is unreadable".to_string(),
            LinkState::Unreachable { .. } => "Unsloth Studio is not running".to_string(),
            LinkState::Unauthorized { .. } => "Unsloth Studio rejected the key".to_string(),
            LinkState::Refused { status, .. } => {
                format!("Unsloth Studio answered {status}")
            }
            LinkState::Malformed { .. } => "unrecognised reply on that port".to_string(),
            LinkState::Idle => "connected — nothing is training".to_string(),
            LinkState::Live => "training".to_string(),
        }
    }

    /// 🚨 **What this state actually asserts** — the module doc's table, in prose, so a UI can
    /// show the claim instead of leaving a viewer to infer one. A green line that means less
    /// than it looks is the failure mode T1 named and this is the antidote.
    pub fn asserts(&self) -> &'static str {
        match self {
            LinkState::Unknown => "Nothing has been asked yet.",
            LinkState::NotConfigured => {
                "No request was sent. Nothing is known about whether the Studio is running."
            }
            LinkState::Misconfigured { .. } => {
                "No request was sent, because the configured address could not be read. \
                 This is a setting on this machine, not anything about the Studio."
            }
            LinkState::Unreachable { .. } => {
                "Nothing answered at that address. Whether the key is good is still unknown — \
                 we never got far enough to find out."
            }
            LinkState::Unauthorized { .. } => {
                "The Studio is running — a 401 is an answer — and it rejected this key."
            }
            LinkState::Refused { .. } => {
                "The Studio is running and the connection is fine. The status is its own \
                 fault, and it is not a verdict on the key."
            }
            LinkState::Malformed { .. } => {
                "Something is listening on that port and answered, but not with what this \
                 route promises."
            }
            LinkState::Idle => {
                "An authenticated route answered, so the Studio is running AND this key is \
                 good. It reports no run in progress."
            }
            LinkState::Live => {
                "An authenticated route answered and progress events are arriving. The \
                 Studio is running, the key is good, and a run is under way."
            }
        }
    }

    /// What a person does about it, when there is anything to do.
    pub fn remedy(&self) -> Option<String> {
        match self {
            LinkState::NotConfigured => Some(format!(
                "Mint an API key in Unsloth Studio and set {}, then restart Organon — a \
                 process does not see an environment variable set after it started.",
                crate::unsloth::TOKEN_ENV
            )),
            LinkState::Unreachable { authority, .. } => Some(format!(
                "Start Unsloth Studio, or set {} to where it is actually serving. Nothing \
                 answered at {authority}.",
                crate::unsloth::ENDPOINT_ENV
            )),
            LinkState::Unauthorized { .. } => Some(format!(
                "Mint a new key in the Studio and update {}, then restart Organon.",
                crate::unsloth::TOKEN_ENV
            )),
            LinkState::Refused { .. } => {
                Some("Check the Studio's own log — the connection is fine.".to_string())
            }
            LinkState::Malformed { .. } => Some(
                "Check that Unsloth Studio, and not another service, is on that port."
                    .to_string(),
            ),
            LinkState::Misconfigured { detail } => Some(format!(
                "Fix or unset {} — unset means {}:{}. ({detail})",
                crate::unsloth::ENDPOINT_ENV,
                crate::unsloth::DEFAULT_HOST,
                crate::unsloth::DEFAULT_PORT,
            )),
            LinkState::Unknown | LinkState::Idle | LinkState::Live => None,
        }
    }

    /// How loudly to draw it.
    pub fn severity(&self) -> Severity {
        match self {
            // 📌 Unreachable is QUIET on purpose. The Studio is off most of the time on the
            // machine this was written for, and painting that red would train everyone to
            // ignore the one colour that should mean something.
            LinkState::Unknown
            | LinkState::NotConfigured
            | LinkState::Unreachable { .. }
            | LinkState::Idle => Severity::Quiet,
            LinkState::Live => Severity::Active,
            // ⚠️ Misconfigured IS warm, unlike the other "nothing was sent" state: somebody
            // typed a value and it is not being honoured, which is worth interrupting for.
            LinkState::Unauthorized { .. }
            | LinkState::Refused { .. }
            | LinkState::Malformed { .. }
            | LinkState::Misconfigured { .. } => Severity::Attention,
        }
    }

    /// Whether an authenticated call has actually succeeded. 🚨 **Only [`LinkState::Idle`]
    /// and [`LinkState::Live`] may say yes** — a health probe could never produce either.
    pub fn credential_proven(&self) -> bool {
        matches!(self, LinkState::Idle | LinkState::Live)
    }

    /// Whether the Studio itself answered something — true even when the answer was a
    /// refusal, because a `401` proves the app is up as surely as a `200` does.
    pub fn studio_answered(&self) -> bool {
        matches!(
            self,
            LinkState::Unauthorized { .. }
                | LinkState::Refused { .. }
                | LinkState::Malformed { .. }
                | LinkState::Idle
                | LinkState::Live
        )
    }

    /// Whether retrying could possibly change the answer.
    ///
    /// ⚠️ `false` for [`LinkState::Unauthorized`] and [`LinkState::NotConfigured`]: a key does
    /// not become valid by being resent, and on Windows a process cannot see a rotated
    /// `UNSLOTH_API_KEY` at all. A retry loop there is a loop that provably cannot succeed.
    pub fn worth_retrying(&self) -> bool {
        matches!(
            self,
            LinkState::Unknown
                | LinkState::Unreachable { .. }
                | LinkState::Refused { .. }
                | LinkState::Malformed { .. }
                | LinkState::Idle
                | LinkState::Live
        )
    }
}

impl fmt::Display for LinkState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.headline())
    }
}

/// Map T1's error taxonomy onto this module's states, so a `401` means the same thing on
/// every route.
pub fn state_from_error(err: &StudioError) -> LinkState {
    match err {
        StudioError::NotConfigured => LinkState::NotConfigured,
        StudioError::Unreachable { authority, detail } => LinkState::Unreachable {
            authority: authority.clone(),
            detail: detail.clone(),
        },
        StudioError::Unauthorized { status } => LinkState::Unauthorized { status: *status },
        StudioError::Refused { status, reason } => LinkState::Refused {
            status: *status,
            reason: reason.clone(),
        },
        StudioError::Malformed { detail } => LinkState::Malformed {
            detail: detail.clone(),
        },
    }
}

// ===========================================================================
// The fold
// ===========================================================================

/// Everything the readout draws: the link's state, the live numbers, three curves and the
/// shelf. Pure — every mutation is a method taking data the worker already fetched.
#[derive(Debug, Clone, Default)]
pub struct TrainingStrip {
    pub state: LinkState,
    pub job_id: Option<String>,
    pub step: Option<u64>,
    pub total_steps: Option<u64>,
    pub loss: Option<f64>,
    pub lr: Option<f64>,
    pub grad_norm: Option<f64>,
    pub epoch: Option<f64>,
    pub phase: Option<String>,
    /// `(step, loss)`, oldest first, at most [`CURVE_CAP`] points.
    pub loss_curve: Vec<(u64, f64)>,
    pub lr_curve: Vec<(u64, f64)>,
    pub grad_curve: Vec<(u64, f64)>,
    /// The run shelf, newest first as the Studio served it.
    pub runs: Vec<RunSummary>,
    /// ⚠️ Set from an SSE `error` event. **This is the RUN failing, not the link** — the
    /// connection is healthy and the credential is good, and the state stays [`LinkState::Idle`].
    /// Conflating the two would send someone to check a socket over a bad dataset path.
    pub run_error: Option<String>,
    /// Progress events folded in. Displayed beside the numbers because *events arriving while
    /// every number is blank* is the signature of a payload field-name guess that missed.
    pub events_seen: u64,
    /// Heartbeats seen. 🚨 A heartbeat is **not** data and never touches a curve.
    pub heartbeats_seen: u64,
    /// Comments (`: …` lines) never reach here at all — the parser drops them.
    pub reconnects: u64,
    /// True when a backfill's value vector and its step vector did not line up, so the x
    /// axis of that curve is index-derived rather than reported.
    pub misaligned_backfill: bool,
    /// True once [`METRICS_PATH`] has been folded in at least once.
    pub backfilled: bool,
}

/// Pair a value series with its step series.
///
/// Returns `(points, aligned)`. **`aligned` is false whenever the x axis was invented** —
/// either because no step vector was served (indices stand in) or because the two vectors
/// were different lengths (the common prefix is taken). A curve whose x axis is a guess is
/// still worth drawing and is not worth drawing *silently*.
pub fn pair_series(values: &[f64], steps: &[u64]) -> (Vec<(u64, f64)>, bool) {
    if values.is_empty() {
        return (Vec::new(), true);
    }
    if steps.is_empty() {
        return (
            values
                .iter()
                .enumerate()
                .map(|(i, v)| (i as u64, *v))
                .collect(),
            false,
        );
    }
    let n = values.len().min(steps.len());
    let aligned = values.len() == steps.len();
    (
        (0..n).map(|i| (steps[i], values[i])).collect(),
        aligned,
    )
}

/// Append a point, de-duplicating by step and truncating to [`CURVE_CAP`].
///
/// 🚨 **De-duplication is not tidiness, it is what makes reconnect lossless.** Backfill runs
/// on every connect and `Last-Event-ID` replays from the cursor, so the same step arrives
/// twice by design; without this the curve would grow a doubled tail at every reconnect. A
/// step *lower* than the last one is a different run (or a resumed one restarting its count),
/// so the curve is cleared rather than drawn as a line travelling backwards.
pub fn push_point(curve: &mut Vec<(u64, f64)>, step: u64, value: f64) {
    if !value.is_finite() {
        return;
    }
    match curve.last() {
        Some(&(last, _)) if last == step => {
            let n = curve.len();
            curve[n - 1] = (step, value);
            return;
        }
        Some(&(last, _)) if step < last => curve.clear(),
        _ => {}
    }
    curve.push((step, value));
    if curve.len() > CURVE_CAP {
        let excess = curve.len() - CURVE_CAP;
        curve.drain(..excess);
    }
}

impl TrainingStrip {
    /// A strip that has done nothing.
    pub fn new() -> TrainingStrip {
        TrainingStrip::default()
    }

    /// Move to a new link state. Leaving [`LinkState::Live`] does **not** clear the curves —
    /// the last shape of a finished run is the thing worth still looking at.
    pub fn set_state(&mut self, state: LinkState) {
        self.state = state;
    }

    /// Fold `GET /api/train/metrics` in — on connect and on every reconnect.
    pub fn apply_metrics(&mut self, m: &TrainingMetrics) {
        let (loss, a1) = pair_series(&m.loss_history, &m.step_history);
        let (lr, a2) = pair_series(&m.lr_history, &m.step_history);
        // ⚠️ The gradient norm has its OWN step vector. See [`TrainingMetrics`].
        let (grad, a3) = pair_series(&m.grad_norm_history, &m.grad_norm_step_history);
        for (s, v) in loss {
            push_point(&mut self.loss_curve, s, v);
        }
        for (s, v) in lr {
            push_point(&mut self.lr_curve, s, v);
        }
        for (s, v) in grad {
            push_point(&mut self.grad_curve, s, v);
        }
        self.misaligned_backfill = !(a1 && a2 && a3);
        if m.job_id.is_some() {
            self.job_id = m.job_id.clone();
        }
        if m.current_step.is_some() {
            self.step = m.current_step;
        }
        if m.current_loss.is_some() {
            self.loss = m.current_loss;
        }
        if m.current_lr.is_some() {
            self.lr = m.current_lr;
        }
        self.backfilled = true;
    }

    /// Fold one already-parsed progress payload in.
    pub fn apply_progress(&mut self, p: &ProgressEvent) {
        self.events_seen = self.events_seen.saturating_add(1);
        if p.job_id.is_some() {
            self.job_id = p.job_id.clone();
        }
        if p.total_steps.is_some() {
            self.total_steps = p.total_steps;
        }
        if p.status.is_some() {
            self.phase = p.status.clone();
        }
        if p.epoch.is_some() {
            self.epoch = p.epoch;
        }
        if p.loss.is_some() {
            self.loss = p.loss;
        }
        if p.learning_rate.is_some() {
            self.lr = p.learning_rate;
        }
        if p.grad_norm.is_some() {
            self.grad_norm = p.grad_norm;
        }
        if let Some(step) = p.step {
            self.step = Some(step);
            if let Some(v) = p.loss {
                push_point(&mut self.loss_curve, step, v);
            }
            if let Some(v) = p.learning_rate {
                push_point(&mut self.lr_curve, step, v);
            }
            if let Some(v) = p.grad_norm {
                push_point(&mut self.grad_curve, step, v);
            }
        }
    }

    /// Fold one SSE event in, dispatching on its type.
    ///
    /// 🚨 **`heartbeat` is counted and dropped.** It is the server saying the socket is alive,
    /// not a sample; treating it as a `progress` event would push whatever its payload
    /// deserializes to — most likely all-`None`, i.e. a step-less no-op today and a garbage
    /// point the moment the payload grows a field whose name matches one of ours.
    pub fn apply_event(&mut self, ev: &SseEvent) {
        match ev.kind() {
            "heartbeat" | "ping" | "keepalive" => {
                self.heartbeats_seen = self.heartbeats_seen.saturating_add(1);
            }
            "complete" => {
                // The run finished. The link is still perfectly good, so it goes to Idle and
                // the curves stay on screen.
                self.state = LinkState::Idle;
                self.phase = Some("complete".to_string());
            }
            "error" => {
                // ⚠️ The RUN failed, not the link. See [`TrainingStrip::run_error`].
                let detail = serde_json::from_str::<ProgressEvent>(&ev.data)
                    .ok()
                    .and_then(|p| p.detail.or(p.status))
                    .unwrap_or_else(|| clip(ev.data.trim(), 200));
                self.run_error = Some(if detail.is_empty() {
                    "the Studio reported a training error with no detail".to_string()
                } else {
                    detail
                });
                self.state = LinkState::Idle;
            }
            // "message" is the default type, and the Studio's own name is "progress".
            "progress" | "message" => {
                if let Ok(p) = serde_json::from_str::<ProgressEvent>(&ev.data) {
                    self.state = LinkState::Live;
                    self.run_error = None;
                    self.apply_progress(&p);
                }
            }
            _ => {}
        }
    }

    /// Replace the shelf.
    pub fn set_runs(&mut self, runs: Vec<RunSummary>) {
        self.runs = runs;
    }

    /// `Some(0.0..=1.0)` when the live run's step counts are both known.
    pub fn progress(&self) -> Option<f32> {
        let total = self.total_steps.filter(|t| *t > 0)?;
        let step = self.step?;
        Some((step as f32 / total as f32).clamp(0.0, 1.0))
    }
}

// ===========================================================================
// The worker
// ===========================================================================

/// One thing the worker learned. The UI thread drains these; it never blocks on a socket.
#[derive(Debug, Clone)]
pub enum LinkMsg {
    State(LinkState),
    Runs(Vec<RunSummary>),
    Metrics(Box<TrainingMetrics>),
    Event(SseEvent),
    /// The stream dropped and the worker is waiting before trying again.
    Reconnecting { attempt: u32, delay: Duration },
    /// The worker has stopped trying. Nothing further will arrive without
    /// [`TrainingLink::retry_now`].
    GaveUp,
}

/// Compose the SSE `GET`, carrying the bearer token and the reconnect cursor.
///
/// 🚨 **The returned string carries the token.** Write it to a socket; never log it.
///
/// ⚠️ `Accept: text/event-stream` matters — a FastAPI route may content-negotiate, and
/// `Connection: keep-alive` is required because the whole point is that the response does not
/// end. (T1's `build_get` sends `Connection: close`, which is right for a constant-time route
/// and wrong for this one; that is why this is a separate builder rather than a parameter.)
pub fn build_stream_get(
    endpoint: &StudioEndpoint,
    path: &str,
    token: &StudioToken,
    last_event_id: Option<&str>,
) -> String {
    let mut req = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\n\
         Cache-Control: no-cache\r\nUser-Agent: organon/{}\r\nConnection: keep-alive\r\n",
        endpoint.authority(),
        env!("CARGO_PKG_VERSION"),
    );
    if let Some(id) = last_event_id {
        // 🚨 Losing this loses steps, and a gap in a loss curve reads as a flat spot rather
        // than as an error. Newlines are stripped because the value comes off the wire and a
        // header value carrying one would be request splitting.
        let clean: String = id.chars().filter(|c| *c != '\r' && *c != '\n').collect();
        if !clean.is_empty() {
            req.push_str(&format!("Last-Event-ID: {clean}\r\n"));
        }
    }
    req.push_str(&format!("Authorization: {}\r\n\r\n", token.header_value()));
    req
}

/// The next backoff delay: double, clamped to [`RECONNECT_MAX`].
pub fn backoff_for(attempt: u32) -> Duration {
    let doubled = RECONNECT_MIN
        .checked_mul(2u32.saturating_pow(attempt.min(16)))
        .unwrap_or(RECONNECT_MAX);
    doubled.min(RECONNECT_MAX)
}

/// A live connection to the Studio's training telemetry, running on its own thread.
///
/// 🚨 **Nothing here blocks the UI thread.** [`TrainingLink::drain`] is a `try_recv` loop, so
/// the caller's cost is bounded by how much arrived since the last frame, and a Studio that
/// accepts a connection and then says nothing costs the editor exactly nothing.
pub struct TrainingLink {
    /// ⚠️ **The `Mutex` is what makes this type `Sync`, and that is not decoration.**
    /// `std::sync::mpsc::Receiver` is `Send` but **not** `Sync`, and nih-plug's
    /// `create_egui_editor` requires the whole editor-state struct to be `Sync` — so a bare
    /// receiver inside `PresetUi` fails to compile at the *host* boundary, a long way from
    /// here, with an error naming a private type. It is never contended: `drain` takes
    /// `&mut self`, so there is exactly one reader by construction.
    rx: Mutex<Receiver<LinkMsg>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    /// The state the link was born in, for a link that never started a thread.
    born: LinkState,
}

impl TrainingLink {
    /// Open a link from the environment's configuration.
    ///
    /// 📌 **With no credential this spawns no thread and opens no socket.** The state is
    /// [`LinkState::NotConfigured`] immediately, and that is the ordinary case on a machine
    /// with no Studio — Organon should cost nothing there.
    pub fn open(config: StudioConfig) -> TrainingLink {
        let Some(token) = config.token.clone() else {
            return TrainingLink::inert(LinkState::NotConfigured);
        };
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_worker = Arc::clone(&stop);
        let endpoint = config.endpoint.clone();
        let handle = std::thread::Builder::new()
            .name("organon-train-link".to_string())
            .spawn(move || worker(endpoint, token, tx, stop_worker))
            .ok();
        TrainingLink {
            rx: Mutex::new(rx),
            stop,
            handle,
            born: LinkState::Unknown,
        }
    }

    /// A link that will never produce anything, standing in the given state.
    pub fn inert(state: LinkState) -> TrainingLink {
        let (_tx, rx) = mpsc::channel();
        TrainingLink {
            rx: Mutex::new(rx),
            stop: Arc::new(AtomicBool::new(true)),
            handle: None,
            born: state,
        }
    }

    /// The state a link with no worker reports.
    pub fn born_state(&self) -> &LinkState {
        &self.born
    }

    /// Whether a worker thread is running.
    pub fn is_running(&self) -> bool {
        self.handle.is_some() && !self.stop.load(Ordering::Relaxed)
    }

    /// Drain everything that has arrived and fold it into `strip`. Never blocks.
    pub fn drain(&mut self, strip: &mut TrainingStrip) -> usize {
        let mut n = 0usize;
        // ⚠️ A poisoned lock cannot happen (one reader, no panics inside), but recovering
        // rather than unwrapping means a link can never take the UI thread down with it.
        let rx = match self.rx.get_mut() {
            Ok(rx) => rx,
            Err(poisoned) => poisoned.into_inner(),
        };
        loop {
            match rx.try_recv() {
                Ok(msg) => {
                    n += 1;
                    match msg {
                        LinkMsg::State(s) => strip.set_state(s),
                        LinkMsg::Runs(r) => strip.set_runs(r),
                        LinkMsg::Metrics(m) => strip.apply_metrics(&m),
                        LinkMsg::Event(e) => strip.apply_event(&e),
                        LinkMsg::Reconnecting { .. } => {
                            strip.reconnects = strip.reconnects.saturating_add(1)
                        }
                        LinkMsg::GaveUp => {}
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.handle.is_some() {
                        self.stop.store(true, Ordering::Relaxed);
                    }
                    break;
                }
            }
        }
        n
    }

    /// Stop the worker. Idempotent; returns immediately (the thread notices within
    /// [`STREAM_POLL_SECS`]).
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Whether a person asking for a retry could achieve anything.
    pub fn can_retry(&self, strip: &TrainingStrip) -> bool {
        !self.is_running() && strip.state.worth_retrying()
    }

    /// Ask again: stop whatever is left and open a fresh link from the same configuration.
    pub fn retry_now(&mut self, config: StudioConfig) -> TrainingLink {
        self.stop();
        TrainingLink::open(config)
    }
}

impl Drop for TrainingLink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // ⚠️ Deliberately NOT joined. The worker checks the flag between reads, so it exits
        // within STREAM_POLL_SECS on its own; joining here would put up to a second of socket
        // wait on whatever thread dropped the link — including the UI thread on shutdown.
    }
}

/// One-shot authenticated `GET`, for [`RUNS_PATH`] and [`METRICS_PATH`].
///
/// Its own function rather than [`crate::unsloth::StudioClient`] only because it must share
/// the worker's stop flag and body bound; the request shape, the timeout and the error
/// taxonomy are T1's.
fn get_once(
    endpoint: &StudioEndpoint,
    token: &StudioToken,
    path: &str,
) -> Result<String, StudioError> {
    use std::io::{Read, Write};

    let unreachable = |detail: String| StudioError::Unreachable {
        authority: endpoint.authority(),
        detail,
    };
    let timeout = Duration::from_secs(TIMEOUT_SECS);
    let addrs = resolve_addrs(&endpoint.host, endpoint.port).map_err(unreachable)?;
    let mut stream = connect_within(&addrs, timeout).map_err(unreachable)?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    // 🚨 T1's builder, so `Connection: close` and the same header set. Never log `req`.
    let req = crate::unsloth::build_get(endpoint, path, Some(token));
    stream
        .write_all(req.as_bytes())
        .map_err(|e| unreachable(format!("write: {e}")))?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&chunk[..n]);
                if raw.len() > MAX_BODY_BYTES {
                    return Err(StudioError::Malformed {
                        detail: format!("reply exceeded {MAX_BODY_BYTES} bytes"),
                    });
                }
            }
            Err(e) => return Err(unreachable(format!("read: {e}"))),
        }
    }
    classify(&String::from_utf8_lossy(&raw))
}

fn worker(
    endpoint: StudioEndpoint,
    token: StudioToken,
    tx: mpsc::Sender<LinkMsg>,
    stop: Arc<AtomicBool>,
) {
    let mut attempt: u32 = 0;
    let mut cursor: Option<String> = None;
    while !stop.load(Ordering::Relaxed) {
        // ── 1. The first AUTHENTICATED call. This, not a health probe, is what tells us
        //       whether the credential works — see the module doc.
        match get_once(&endpoint, &token, RUNS_PATH).and_then(|b| parse_runs(&b)) {
            Ok(runs) => {
                let _ = tx.send(LinkMsg::Runs(runs));
                let _ = tx.send(LinkMsg::State(LinkState::Idle));
                attempt = 0;
            }
            Err(e) => {
                let state = state_from_error(&e);
                let retry = state.worth_retrying();
                let _ = tx.send(LinkMsg::State(state));
                if !retry {
                    let _ = tx.send(LinkMsg::GaveUp);
                    return;
                }
                if !sleep_or_stop(&tx, &stop, &mut attempt) {
                    return;
                }
                continue;
            }
        }

        // ── 2. Backfill, on EVERY connect. Belt to `Last-Event-ID`'s braces.
        if let Ok(body) = get_once(&endpoint, &token, METRICS_PATH) {
            if let Ok(m) = serde_json::from_str::<TrainingMetrics>(&body) {
                let _ = tx.send(LinkMsg::Metrics(Box::new(m)));
            }
        }

        // ── 3. The stream.
        match stream_once(&endpoint, &token, cursor.clone(), &tx, &stop) {
            Ok(next_cursor) => {
                cursor = next_cursor;
                attempt = 0;
            }
            Err(e) => {
                let state = state_from_error(&e);
                let retry = state.worth_retrying();
                let _ = tx.send(LinkMsg::State(state));
                if !retry {
                    let _ = tx.send(LinkMsg::GaveUp);
                    return;
                }
            }
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if !sleep_or_stop(&tx, &stop, &mut attempt) {
            return;
        }
    }
}

/// Wait out the backoff, checking the stop flag as it goes. `false` means stop or give up.
fn sleep_or_stop(
    tx: &mpsc::Sender<LinkMsg>,
    stop: &Arc<AtomicBool>,
    attempt: &mut u32,
) -> bool {
    if *attempt >= RECONNECT_GIVE_UP_AFTER {
        let _ = tx.send(LinkMsg::GaveUp);
        return false;
    }
    let delay = backoff_for(*attempt);
    let _ = tx.send(LinkMsg::Reconnecting {
        attempt: *attempt + 1,
        delay,
    });
    *attempt += 1;
    let deadline = Instant::now() + delay;
    while Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100).min(deadline - Instant::now()));
    }
    !stop.load(Ordering::Relaxed)
}

/// Hold one progress stream open until it ends. Returns the cursor to resume from.
fn stream_once(
    endpoint: &StudioEndpoint,
    token: &StudioToken,
    cursor: Option<String>,
    tx: &mpsc::Sender<LinkMsg>,
    stop: &Arc<AtomicBool>,
) -> Result<Option<String>, StudioError> {
    use std::io::{Read, Write};

    let unreachable = |detail: String| StudioError::Unreachable {
        authority: endpoint.authority(),
        detail,
    };
    let addrs = resolve_addrs(&endpoint.host, endpoint.port).map_err(unreachable)?;
    let mut stream =
        connect_within(&addrs, Duration::from_secs(TIMEOUT_SECS)).map_err(unreachable)?;
    // ⚠️ The SHORT timeout, so the stop flag is seen promptly. Silence is not death here;
    // STREAM_IDLE_SECS below is what decides that.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(STREAM_POLL_SECS)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(TIMEOUT_SECS)));
    let req = build_stream_get(endpoint, PROGRESS_PATH, token, cursor.as_deref());
    stream
        .write_all(req.as_bytes())
        .map_err(|e| unreachable(format!("write: {e}")))?;

    let mut reader = SseReader::new(cursor);
    let mut chunk = [0u8; 8192];
    let mut last_byte = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(reader.last_event_id().map(str::to_string));
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(reader.last_event_id().map(str::to_string)),
            Ok(n) => {
                last_byte = Instant::now();
                for ev in reader.feed(&chunk[..n])? {
                    if tx.send(LinkMsg::Event(ev)).is_err() {
                        return Ok(reader.last_event_id().map(str::to_string));
                    }
                }
                if reader.body_done() {
                    return Ok(reader.last_event_id().map(str::to_string));
                }
            }
            Err(e) => {
                let kind = e.kind();
                let idle = matches!(
                    kind,
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                );
                if !idle {
                    return Err(unreachable(format!("read: {e}")));
                }
                if last_byte.elapsed() >= Duration::from_secs(STREAM_IDLE_SECS) {
                    return Err(unreachable(format!(
                        "read: no bytes for {STREAM_IDLE_SECS}s, not even a heartbeat"
                    )));
                }
            }
        }
    }
}

// ===========================================================================
// Tests — no network, no key, no Studio
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── SSE framing ────────────────────────────────────────────────────────

    #[test]
    fn a_plain_event_parses() {
        let mut p = SseParser::new();
        let evs = p.feed(b"event: progress\ndata: {\"step\":1}\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].event, "progress");
        assert_eq!(evs[0].data, "{\"step\":1}");
    }

    #[test]
    fn several_data_lines_join_with_newlines() {
        let mut p = SseParser::new();
        let evs = p.feed(b"data: one\ndata: two\ndata: three\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "one\ntwo\nthree");
    }

    #[test]
    fn a_comment_line_is_not_data() {
        let mut p = SseParser::new();
        // 🚨 The mutation to watch: treating `:` lines as data pushes the keep-alive text
        // into the payload, and `serde_json` then fails on every event.
        let evs = p.feed(b": keep-alive\n: another\ndata: real\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "real");
    }

    #[test]
    fn a_comment_alone_produces_no_event() {
        let mut p = SseParser::new();
        assert!(p.feed(b": ping\n\n").is_empty());
        assert!(p.feed(b":\n\n").is_empty());
    }

    #[test]
    fn an_event_split_across_reads_still_parses_once() {
        // 🚨 THE case this parser exists for. A socket read ends wherever it ends.
        //
        // ⚠️ **Assert the WHOLE event, not the field you were thinking about.** The first
        // version of this test checked `data` and `id` only, and it passed against a
        // deliberately broken `next_line` that treats a buffer-final `\r` as a terminator.
        // That mutation's actual symptom is subtler than "the data is wrong": the orphaned
        // `\n` arrives next and reads as an **empty line**, i.e. a spurious dispatch, which
        // clears the event-type buffer — so a split immediately after `event: progress\r`
        // yields an event with the right payload and no type at all. Comparing against the
        // unsplit parse is what makes the assertion total.
        let raw = b"event: progress\r\nid: 42\r\ndata: {\"step\":7,\"loss\":1.5}\r\n\r\n";
        let want = SseParser::new().feed(raw);
        assert_eq!(want.len(), 1);
        for split in 1..raw.len() {
            let mut p = SseParser::new();
            let mut got: Vec<SseEvent> = p.feed(&raw[..split]);
            got.extend(p.feed(&raw[split..]));
            assert_eq!(got, want, "split at {split} did not parse as one whole event");
            assert_eq!(got[0].event, "progress", "split at {split} lost the event type");
            assert_eq!(got[0].data, "{\"step\":7,\"loss\":1.5}", "split at {split}");
            assert_eq!(got[0].id.as_deref(), Some("42"), "split at {split}");
        }
    }

    #[test]
    fn a_multi_line_crlf_event_split_anywhere_stays_one_event() {
        // The narrower probe for the same defect, where the symptom is unmissable: an early
        // dispatch splits ONE event carrying two data lines into TWO events carrying one
        // each, which is a loss curve growing points that were never sent.
        let raw = b"event: progress\r\ndata: alpha\r\ndata: beta\r\n\r\n";
        for split in 1..raw.len() {
            let mut p = SseParser::new();
            let mut got: Vec<SseEvent> = p.feed(&raw[..split]);
            got.extend(p.feed(&raw[split..]));
            assert_eq!(got.len(), 1, "split at {split} produced {got:?}");
            assert_eq!(got[0].data, "alpha\nbeta", "split at {split}");
            assert_eq!(got[0].event, "progress", "split at {split}");
        }
    }

    #[test]
    fn one_byte_at_a_time_is_identical_to_one_read() {
        let raw = b": hi\nevent: progress\ndata: a\ndata: b\nid: 9\nretry: 2500\n\ndata: c\n\n";
        let mut whole = SseParser::new();
        let want = whole.feed(raw);
        let mut drip = SseParser::new();
        let mut got = Vec::new();
        for b in raw {
            got.extend(drip.feed(&[*b]));
        }
        assert_eq!(got, want);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].retry, Some(2500));
        assert_eq!(got[1].id.as_deref(), Some("9"), "the id persists across events");
    }

    #[test]
    fn a_lone_cr_at_the_end_of_a_read_does_not_dispatch_early() {
        let mut p = SseParser::new();
        // "data: x\r" — the \r may yet be the first half of \r\n.
        assert!(p.feed(b"data: x\r").is_empty());
        let evs = p.feed(b"\n\r\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "x");
    }

    #[test]
    fn all_three_line_terminators_work() {
        for term in [&b"\n"[..], &b"\r\n"[..], &b"\r"[..]] {
            let mut p = SseParser::new();
            let mut raw = Vec::new();
            raw.extend_from_slice(b"data: v");
            raw.extend_from_slice(term);
            raw.extend_from_slice(term);
            // A bare \r needs a following byte to be recognised as a terminator.
            raw.extend_from_slice(b"data: w");
            raw.extend_from_slice(term);
            raw.extend_from_slice(term);
            raw.push(b'x');
            let evs = p.feed(&raw);
            assert_eq!(evs.len(), 2, "terminator {term:?}");
            assert_eq!(evs[0].data, "v");
        }
    }

    #[test]
    fn an_id_only_block_moves_the_cursor_without_making_an_event() {
        let mut p = SseParser::new();
        assert!(p.feed(b"id: 77\n\n").is_empty());
        assert_eq!(p.last_event_id(), Some("77"));
    }

    #[test]
    fn a_field_with_no_colon_has_an_empty_value() {
        let mut p = SseParser::new();
        let evs = p.feed(b"data\ndata: x\n\n");
        assert_eq!(evs[0].data, "\nx");
    }

    #[test]
    fn only_one_leading_space_is_stripped() {
        let mut p = SseParser::new();
        let evs = p.feed(b"data:  two spaces\n\n");
        assert_eq!(evs[0].data, " two spaces");
    }

    #[test]
    fn a_leading_bom_is_dropped_once() {
        let mut p = SseParser::new();
        let mut raw = vec![0xEF, 0xBB, 0xBF];
        raw.extend_from_slice(b"data: x\n\n");
        let evs = p.feed(&raw);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "x");
    }

    #[test]
    fn a_retry_that_is_not_digits_is_ignored() {
        let mut p = SseParser::new();
        p.feed(b"retry: soon\n\n");
        assert_eq!(p.retry_ms(), None);
        p.feed(b"retry: 1500\n\n");
        assert_eq!(p.retry_ms(), Some(1500));
    }

    #[test]
    fn an_id_containing_a_nul_is_ignored() {
        let mut p = SseParser::new();
        p.feed(b"id: good\n\n");
        p.feed(b"id: ba\0d\n\n");
        assert_eq!(p.last_event_id(), Some("good"));
    }

    // ── Chunked framing ────────────────────────────────────────────────────

    #[test]
    fn chunked_bodies_decode() {
        let mut d = ChunkedDecoder::new();
        let out = d.feed(b"5\r\nhello\r\n5\r\nworld\r\n0\r\n\r\n");
        assert_eq!(String::from_utf8_lossy(&out), "helloworld");
        assert!(d.is_done());
    }

    #[test]
    fn a_chunk_split_anywhere_decodes_the_same() {
        let raw = b"a\r\ndata: 12\n\r\n2\r\n\n\r\n0\r\n\r\n";
        let mut whole = ChunkedDecoder::new();
        let want = whole.feed(raw);
        for split in 1..raw.len() {
            let mut d = ChunkedDecoder::new();
            let mut got = d.feed(&raw[..split]);
            got.extend(d.feed(&raw[split..]));
            assert_eq!(got, want, "split at {split}");
        }
    }

    #[test]
    fn chunk_extensions_are_ignored() {
        let mut d = ChunkedDecoder::new();
        let out = d.feed(b"5;name=value\r\nhello\r\n0\r\n\r\n");
        assert_eq!(String::from_utf8_lossy(&out), "hello");
    }

    #[test]
    fn a_chunked_sse_stream_is_not_polluted_by_its_size_lines() {
        // 🚨 The whole reason ChunkedDecoder exists. Without it the SSE parser sees `1a`
        // and `0` as SSE field lines and silently ignores them — and eats the real event
        // whose body the boundary falls inside.
        //
        // ⚠️ **The chunk boundary must land MID-EVENT or this test proves nothing.** The
        // first version put one whole event in one chunk, and it passed with the decoder
        // deliberately disabled: a size line sitting *between* events parses as an unknown
        // SSE field and is discarded, so the events come out intact by luck. The damage is
        // to whatever the boundary cuts through — here, a `data:` line severed in the
        // middle of its JSON.
        let part1 = "event: progress\ndata: {\"ste";
        let part2 = "p\":3}\n\n";
        let raw = format!(
            "{:x}\r\n{part1}\r\n{:x}\r\n{part2}\r\n0\r\n\r\n",
            part1.len(),
            part2.len()
        );
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                    Transfer-Encoding: chunked\r\n\r\n";
        let mut r = SseReader::new(None);
        let mut evs = r.feed(head.as_bytes()).unwrap();
        evs.extend(r.feed(raw.as_bytes()).unwrap());
        assert_eq!(evs.len(), 1, "got {evs:?}");
        assert_eq!(evs[0].event, "progress");
        assert_eq!(
            evs[0].data, "{\"step\":3}",
            "the chunk boundary cut this data line in half"
        );
        // And the payload the strip would actually read must survive intact.
        let p: ProgressEvent = serde_json::from_str(&evs[0].data).unwrap();
        assert_eq!(p.step, Some(3));
    }

    // ── Response head ──────────────────────────────────────────────────────

    #[test]
    fn a_head_split_across_reads_still_parses() {
        let head = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        for split in 1..head.len() {
            let mut r = SseReader::new(None);
            r.feed(&head[..split]).unwrap();
            r.feed(&head[split..]).unwrap();
            assert!(r.head().is_some(), "split at {split}");
            assert!(r.head().unwrap().chunked, "split at {split}");
        }
    }

    #[test]
    fn a_401_on_the_stream_is_unauthorized_not_unreachable() {
        let mut r = SseReader::new(None);
        let err = r
            .feed(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
            .unwrap_err();
        assert_eq!(err, StudioError::Unauthorized { status: 401 });
        assert_eq!(
            state_from_error(&err),
            LinkState::Unauthorized { status: 401 }
        );
    }

    #[test]
    fn a_503_on_the_stream_is_refused_not_unauthorized() {
        let mut r = SseReader::new(None);
        let err = r
            .feed(b"HTTP/1.1 503 Service Unavailable\r\n\r\n")
            .unwrap_err();
        assert!(matches!(err, StudioError::Refused { status: 503, .. }));
    }

    #[test]
    fn an_unchunked_stream_still_delivers_events() {
        let mut r = SseReader::new(None);
        let evs = r
            .feed(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: x\n\n")
            .unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "x");
    }

    // ── Reconnect ──────────────────────────────────────────────────────────

    #[test]
    fn a_reconnect_sends_last_event_id() {
        // 🚨 Losing this loses steps, and the gap looks like a flat spot in the curve.
        let ep = StudioEndpoint::default();
        let tok = StudioToken::new("k").unwrap();
        let req = build_stream_get(&ep, PROGRESS_PATH, &tok, Some("41"));
        assert!(req.contains("Last-Event-ID: 41\r\n"), "request was:\n{req}");
    }

    #[test]
    fn a_first_connection_sends_no_last_event_id() {
        let ep = StudioEndpoint::default();
        let tok = StudioToken::new("k").unwrap();
        let req = build_stream_get(&ep, PROGRESS_PATH, &tok, None);
        assert!(!req.contains("Last-Event-ID"));
    }

    #[test]
    fn the_cursor_carried_into_a_reconnect_is_the_one_the_reader_ended_on() {
        let mut r = SseReader::new(None);
        r.feed(b"HTTP/1.1 200 OK\r\n\r\nid: 5\ndata: a\n\ndata: b\n\n")
            .unwrap();
        assert_eq!(r.last_event_id(), Some("5"));
        let ep = StudioEndpoint::default();
        let tok = StudioToken::new("k").unwrap();
        let req = build_stream_get(&ep, PROGRESS_PATH, &tok, r.last_event_id());
        assert!(req.contains("Last-Event-ID: 5\r\n"));
    }

    #[test]
    fn a_cursor_with_a_newline_cannot_split_the_request() {
        let ep = StudioEndpoint::default();
        let tok = StudioToken::new("k").unwrap();
        let req = build_stream_get(&ep, PROGRESS_PATH, &tok, Some("1\r\nX-Evil: yes"));
        assert!(req.contains("Last-Event-ID: 1X-Evil: yes\r\n"), "{req}");
        assert!(!req.contains("\r\nX-Evil:"));
    }

    #[test]
    fn the_stream_request_carries_the_bearer_token_and_keeps_the_connection_open() {
        let ep = StudioEndpoint::default();
        let tok = StudioToken::new("secret-key").unwrap();
        let req = build_stream_get(&ep, PROGRESS_PATH, &tok, None);
        assert!(req.contains("Authorization: Bearer secret-key\r\n"));
        assert!(req.contains("Accept: text/event-stream\r\n"));
        assert!(req.contains("Connection: keep-alive\r\n"));
        assert!(!req.contains("Connection: close"));
    }

    #[test]
    fn backoff_doubles_and_stops_at_the_ceiling() {
        assert_eq!(backoff_for(0), RECONNECT_MIN);
        assert_eq!(backoff_for(1), RECONNECT_MIN * 2);
        assert_eq!(backoff_for(2), RECONNECT_MIN * 4);
        assert_eq!(backoff_for(30), RECONNECT_MAX);
        for a in 0..64u32 {
            assert!(backoff_for(a) <= RECONNECT_MAX, "attempt {a}");
            assert!(backoff_for(a) >= RECONNECT_MIN, "attempt {a}");
        }
    }

    // ⚠️ `allow` on purpose: clippy is right that these fold to constants, and that is the
    // point — the test exists so that *editing one of the constants* fails by name. A const
    // block would turn the same mutation into a compile error, which is a worse report.
    #[allow(clippy::assertions_on_constants)]
    #[test]
    fn the_stream_timeout_is_not_the_health_timeout() {
        // ⚠️ Reusing T1's 5 s would tear down a healthy stream every five seconds.
        assert!(
            STREAM_IDLE_SECS > TIMEOUT_SECS * 4,
            "the stream's silence budget must be far longer than the health probe's: a \
             stream between heartbeats is healthy"
        );
        assert!(
            STREAM_POLL_SECS < STREAM_IDLE_SECS,
            "the socket poll is a responsiveness budget, not a liveness one"
        );
    }

    // ── The fold ───────────────────────────────────────────────────────────

    fn ev(kind: &str, data: &str) -> SseEvent {
        SseEvent {
            event: kind.to_string(),
            data: data.to_string(),
            id: None,
            retry: None,
        }
    }

    #[test]
    fn a_progress_event_lands_on_the_curve() {
        let mut s = TrainingStrip::new();
        s.apply_event(&ev(
            "progress",
            r#"{"step":1,"loss":2.5,"learning_rate":0.0002,"grad_norm":1.1}"#,
        ));
        assert_eq!(s.state, LinkState::Live);
        assert_eq!(s.loss_curve, vec![(1, 2.5)]);
        assert_eq!(s.lr_curve, vec![(1, 0.0002)]);
        assert_eq!(s.grad_curve, vec![(1, 1.1)]);
        assert_eq!(s.events_seen, 1);
    }

    #[test]
    fn a_heartbeat_is_counted_and_is_not_data() {
        // 🚨 Mutation target. Routing "heartbeat" into the progress arm makes the beat a
        // sample, and the curve then grows a point for every keep-alive.
        let mut s = TrainingStrip::new();
        s.apply_event(&ev("progress", r#"{"step":1,"loss":2.0}"#));
        s.apply_event(&ev("heartbeat", r#"{"step":99,"loss":0.0}"#));
        s.apply_event(&ev("heartbeat", "{}"));
        assert_eq!(s.heartbeats_seen, 2);
        assert_eq!(s.events_seen, 1, "a heartbeat is not a progress event");
        assert_eq!(s.loss_curve, vec![(1, 2.0)], "the curve never saw step 99");
        assert_eq!(s.step, Some(1));
    }

    #[test]
    fn a_default_typed_event_is_treated_as_progress() {
        let mut s = TrainingStrip::new();
        s.apply_event(&ev("", r#"{"step":4,"loss":1.0}"#));
        assert_eq!(s.loss_curve, vec![(4, 1.0)]);
    }

    #[test]
    fn a_complete_event_returns_the_link_to_idle_and_keeps_the_curve() {
        let mut s = TrainingStrip::new();
        s.apply_event(&ev("progress", r#"{"step":1,"loss":2.0}"#));
        s.apply_event(&ev("complete", "{}"));
        assert_eq!(s.state, LinkState::Idle);
        assert_eq!(s.loss_curve.len(), 1, "the finished run stays on screen");
    }

    #[test]
    fn an_error_event_is_the_run_failing_not_the_link() {
        // ⚠️ The distinction the strip exists to keep: a bad dataset path must not read as
        // a socket problem.
        let mut s = TrainingStrip::new();
        s.apply_event(&ev("error", r#"{"detail":"dataset not found"}"#));
        assert_eq!(s.run_error.as_deref(), Some("dataset not found"));
        assert_eq!(s.state, LinkState::Idle);
        assert!(s.state.credential_proven());
    }

    #[test]
    fn a_replayed_step_does_not_double_the_curve() {
        let mut s = TrainingStrip::new();
        for step in 1..=3u64 {
            s.apply_event(&ev(
                "progress",
                &format!(r#"{{"step":{step},"loss":{}}}"#, 10 - step),
            ));
        }
        // A reconnect replays from the cursor, and the backfill overlaps too.
        s.apply_metrics(&TrainingMetrics {
            loss_history: vec![9.0, 8.0, 7.0],
            step_history: vec![1, 2, 3],
            ..Default::default()
        });
        assert_eq!(s.loss_curve, vec![(1, 9.0), (2, 8.0), (3, 7.0)]);
    }

    #[test]
    fn the_same_step_arriving_twice_replaces_rather_than_appends() {
        // ⚠️ The narrow probe, and it took a mutation to notice it was missing. The broader
        // replay test above happens to travel through the *backwards-step* branch — the
        // backfill's first point is lower than the live curve's last, so the curve is
        // cleared and rebuilt, and the de-duplication never runs at all. Deleting the
        // replace arm therefore passed it. This one cannot go through that branch.
        let mut c: Vec<(u64, f64)> = Vec::new();
        push_point(&mut c, 7, 2.0);
        push_point(&mut c, 8, 1.5);
        push_point(&mut c, 8, 1.4);
        assert_eq!(
            c,
            vec![(7, 2.0), (8, 1.4)],
            "a resent step is one point with the newer value, not two points"
        );
    }

    #[test]
    fn a_reconnect_replaying_from_the_cursor_does_not_double_the_tail() {
        // The same invariant at the level it actually bites: `Last-Event-ID` replays from
        // the cursor, so the steps either side of the seam arrive twice.
        let mut s = TrainingStrip::new();
        for (step, loss) in [(1u64, 3.0), (2, 2.5), (3, 2.2)] {
            s.apply_event(&ev("progress", &format!(r#"{{"step":{step},"loss":{loss}}}"#)));
        }
        // The reconnect replays 3 and then continues.
        for (step, loss) in [(3u64, 2.2), (4, 2.0)] {
            s.apply_event(&ev("progress", &format!(r#"{{"step":{step},"loss":{loss}}}"#)));
        }
        assert_eq!(s.loss_curve, vec![(1, 3.0), (2, 2.5), (3, 2.2), (4, 2.0)]);
    }

    #[test]
    fn a_step_going_backwards_starts_a_fresh_curve() {
        let mut s = TrainingStrip::new();
        for step in [5u64, 6, 7] {
            s.apply_event(&ev("progress", &format!(r#"{{"step":{step},"loss":1.0}}"#)));
        }
        s.apply_event(&ev("progress", r#"{"step":1,"loss":9.0}"#));
        assert_eq!(s.loss_curve, vec![(1, 9.0)]);
    }

    #[test]
    fn the_curve_is_capped_and_keeps_the_newest() {
        let mut c: Vec<(u64, f64)> = Vec::new();
        for i in 0..(CURVE_CAP as u64 + 10) {
            push_point(&mut c, i, i as f64);
        }
        assert_eq!(c.len(), CURVE_CAP);
        assert_eq!(c.last().unwrap().0, CURVE_CAP as u64 + 9);
        assert_eq!(c.first().unwrap().0, 10);
    }

    #[test]
    fn a_non_finite_value_never_reaches_a_curve() {
        let mut c: Vec<(u64, f64)> = Vec::new();
        push_point(&mut c, 1, f64::NAN);
        push_point(&mut c, 2, f64::INFINITY);
        assert!(c.is_empty());
    }

    #[test]
    fn grad_norm_is_paired_with_its_own_step_vector() {
        // ⚠️ The silent one: pairing grad norm with `step_history` draws a curve whose x
        // axis is wrong without anything failing.
        let mut s = TrainingStrip::new();
        s.apply_metrics(&TrainingMetrics {
            loss_history: vec![3.0, 2.0, 1.0],
            step_history: vec![10, 20, 30],
            grad_norm_history: vec![0.5, 0.4],
            grad_norm_step_history: vec![10, 30],
            ..Default::default()
        });
        assert_eq!(s.loss_curve, vec![(10, 3.0), (20, 2.0), (30, 1.0)]);
        assert_eq!(s.grad_curve, vec![(10, 0.5), (30, 0.4)]);
        assert!(!s.misaligned_backfill);
    }

    #[test]
    fn a_missing_step_vector_is_recorded_as_a_guessed_axis() {
        let mut s = TrainingStrip::new();
        s.apply_metrics(&TrainingMetrics {
            loss_history: vec![3.0, 2.0],
            ..Default::default()
        });
        assert_eq!(s.loss_curve, vec![(0, 3.0), (1, 2.0)]);
        assert!(s.misaligned_backfill, "an invented x axis must be declared");
    }

    #[test]
    fn pair_series_takes_the_common_prefix_of_mismatched_lengths() {
        let (pts, aligned) = pair_series(&[1.0, 2.0, 3.0], &[5, 6]);
        assert_eq!(pts, vec![(5, 1.0), (6, 2.0)]);
        assert!(!aligned);
    }

    #[test]
    fn step_histories_accept_integers_or_floats() {
        let m: TrainingMetrics = serde_json::from_str(
            r#"{"step_history":[1,2,3],"grad_norm_step_history":[1.0,3.0]}"#,
        )
        .unwrap();
        assert_eq!(m.step_history, vec![1, 2, 3]);
        assert_eq!(m.grad_norm_step_history, vec![1, 3]);
    }

    #[test]
    fn a_step_history_of_the_wrong_type_is_an_error_not_a_shrug() {
        assert!(serde_json::from_str::<TrainingMetrics>(r#"{"step_history":["a"]}"#).is_err());
    }

    #[test]
    fn unknown_progress_fields_are_ignored_rather_than_fatal() {
        let mut s = TrainingStrip::new();
        s.apply_event(&ev(
            "progress",
            r#"{"step":2,"loss":1.0,"something_new":{"nested":true}}"#,
        ));
        assert_eq!(s.loss_curve, vec![(2, 1.0)]);
    }

    #[test]
    fn the_metrics_aliases_reach_the_same_fields_from_a_progress_event() {
        let mut s = TrainingStrip::new();
        s.apply_event(&ev(
            "progress",
            r#"{"current_step":3,"current_loss":0.5,"current_lr":1e-4}"#,
        ));
        assert_eq!(s.loss_curve, vec![(3, 0.5)]);
        assert_eq!(s.lr_curve, vec![(3, 1e-4)]);
    }

    // ── The shelf ──────────────────────────────────────────────────────────

    #[test]
    fn runs_parse_from_a_bare_array() {
        let runs = parse_runs(
            r#"[{"id":"r1","status":"complete","final_loss":0.9,"total_steps":60,
                 "final_step":60,"loss_sparkline":[3.0,1.0],"duration_seconds":95.0,
                 "can_resume":true}]"#,
        )
        .unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].duration_label(), "1m 35s");
        assert_eq!(runs[0].progress(), Some(1.0));
        assert!(runs[0].is_complete());
        assert!(runs[0].can_resume);
    }

    #[test]
    fn runs_also_parse_from_an_envelope() {
        let runs = parse_runs(r#"{"runs":[{"id":"r1","status":"running"}]}"#).unwrap();
        assert_eq!(runs.len(), 1);
        assert!(runs[0].is_running());
    }

    #[test]
    fn an_empty_shelf_is_a_success_not_a_failure() {
        assert_eq!(parse_runs("[]").unwrap().len(), 0);
    }

    #[test]
    fn a_body_that_is_not_runs_is_malformed() {
        assert!(matches!(
            parse_runs("<html>not the studio</html>"),
            Err(StudioError::Malformed { .. })
        ));
    }

    #[test]
    fn duration_labels_cover_the_three_magnitudes() {
        let d = |s: f64| RunSummary {
            duration_seconds: Some(s),
            ..Default::default()
        }
        .duration_label();
        assert_eq!(d(41.0), "41s");
        assert_eq!(d(64.0), "1m 04s");
        assert_eq!(d(8000.0), "2h 13m");
        assert_eq!(
            RunSummary::default().duration_label(),
            "—",
            "an absent duration is not a zero one"
        );
    }

    // ── What the states assert ─────────────────────────────────────────────

    #[test]
    fn only_an_authenticated_success_proves_the_credential() {
        // 🚨 The T1 constraint, carried into this tier's vocabulary. Nothing short of an
        // authenticated 2xx may claim the key is good — and this module never probes health
        // at all, so no state here can be reached by one.
        for s in [
            LinkState::Unknown,
            LinkState::NotConfigured,
            LinkState::Unreachable {
                authority: "127.0.0.1:8888".into(),
                detail: "refused".into(),
            },
            LinkState::Unauthorized { status: 401 },
            LinkState::Refused {
                status: 503,
                reason: "Service Unavailable".into(),
            },
            LinkState::Malformed {
                detail: "html".into(),
            },
        ] {
            assert!(!s.credential_proven(), "{s:?} must not claim the key is good");
        }
        assert!(LinkState::Idle.credential_proven());
        assert!(LinkState::Live.credential_proven());
    }

    #[test]
    fn the_three_sentences_are_three_different_sentences() {
        let idle = LinkState::Idle;
        let gone = LinkState::Unreachable {
            authority: "127.0.0.1:8888".into(),
            detail: "connection refused".into(),
        };
        let bad = LinkState::Unauthorized { status: 401 };
        let hs: Vec<String> = [&idle, &gone, &bad].iter().map(|s| s.headline()).collect();
        assert_eq!(hs.len(), 3);
        for i in 0..3 {
            for j in (i + 1)..3 {
                assert_ne!(hs[i], hs[j], "states {i} and {j} read identically");
            }
        }
        assert_ne!(idle.asserts(), gone.asserts());
        assert_ne!(gone.asserts(), bad.asserts());
        assert!(gone.remedy().unwrap().contains("Start Unsloth Studio"));
        assert!(bad.remedy().unwrap().contains("Mint a new key"));
        assert!(idle.remedy().is_none(), "nothing is wrong, so nothing to fix");
    }

    #[test]
    fn an_absent_studio_is_quiet_and_a_bad_key_asks_for_attention() {
        // 📌 The Studio being off is the normal case; painting it red trains people to
        // ignore red.
        assert_eq!(
            LinkState::Unreachable {
                authority: "a".into(),
                detail: "b".into()
            }
            .severity(),
            Severity::Quiet
        );
        assert_eq!(LinkState::NotConfigured.severity(), Severity::Quiet);
        assert_eq!(LinkState::Idle.severity(), Severity::Quiet);
        assert_eq!(LinkState::Live.severity(), Severity::Active);
        assert_eq!(
            LinkState::Unauthorized { status: 401 }.severity(),
            Severity::Attention
        );
    }

    #[test]
    fn a_401_still_proves_the_studio_is_running() {
        assert!(LinkState::Unauthorized { status: 401 }.studio_answered());
        assert!(!LinkState::Unreachable {
            authority: "a".into(),
            detail: "b".into()
        }
        .studio_answered());
    }

    #[test]
    fn a_bad_key_is_never_retried() {
        // ⚠️ A key does not become valid by being resent, and on Windows a rotated
        // UNSLOTH_API_KEY is invisible to a running process.
        assert!(!LinkState::Unauthorized { status: 401 }.worth_retrying());
        assert!(!LinkState::NotConfigured.worth_retrying());
        assert!(LinkState::Unreachable {
            authority: "a".into(),
            detail: "b".into()
        }
        .worth_retrying());
    }

    #[test]
    fn every_state_that_can_be_fixed_says_how() {
        for s in [
            LinkState::NotConfigured,
            LinkState::Unreachable {
                authority: "a".into(),
                detail: "b".into(),
            },
            LinkState::Unauthorized { status: 401 },
            LinkState::Refused {
                status: 500,
                reason: "x".into(),
            },
            LinkState::Malformed {
                detail: "y".into(),
            },
            LinkState::Misconfigured {
                detail: "bad port".into(),
            },
        ] {
            let r = s.remedy().unwrap_or_default();
            assert!(!r.is_empty(), "{s:?} has no remedy");
            assert!(!s.asserts().is_empty(), "{s:?} claims nothing");
        }
    }

    #[test]
    fn a_bad_endpoint_setting_is_not_confused_with_a_bad_reply() {
        // ✏️ The sixth state. Folding this into Malformed would send someone with a typo in
        // an environment variable to go and inspect what is listening on a port.
        let mis = LinkState::Misconfigured {
            detail: "bad port \"nope\"".into(),
        };
        let mal = LinkState::Malformed {
            detail: "html".into(),
        };
        assert_ne!(mis.headline(), mal.headline());
        assert_ne!(mis.asserts(), mal.asserts());
        assert!(mis.remedy().unwrap().contains(crate::unsloth::ENDPOINT_ENV));
        assert!(!mis.studio_answered(), "nothing was sent");
        assert!(!mis.credential_proven());
        assert!(!mis.worth_retrying(), "an env var does not fix itself");
        assert_eq!(mis.severity(), Severity::Attention);
    }

    // ── The link ───────────────────────────────────────────────────────────

    #[test]
    fn no_credential_means_no_thread_and_no_socket() {
        // 📌 The ordinary case on a machine with no Studio must cost nothing.
        let link = TrainingLink::open(StudioConfig::default());
        assert!(!link.is_running());
        assert_eq!(link.born_state(), &LinkState::NotConfigured);
    }

    #[test]
    fn the_link_is_send_and_sync_because_the_editor_state_must_be() {
        // ⚠️ Pinned here rather than discovered at the host boundary. `mpsc::Receiver` is
        // `Send` but NOT `Sync`, and nih-plug's `create_egui_editor` requires the whole
        // editor-state struct to be `Sync` — so dropping the `Mutex` fails to compile in
        // `lib.rs`, hundreds of files away, with an error naming a private message type.
        fn require<T: Send + Sync>() {}
        require::<TrainingLink>();
        require::<TrainingStrip>();
    }

    #[test]
    fn draining_an_inert_link_never_blocks_and_changes_nothing() {
        let mut link = TrainingLink::inert(LinkState::NotConfigured);
        let mut strip = TrainingStrip::new();
        let before = strip.clone();
        assert_eq!(link.drain(&mut strip), 0);
        assert_eq!(strip.state, before.state);
        assert_eq!(strip.loss_curve, before.loss_curve);
    }

    #[test]
    fn the_studio_error_taxonomy_maps_onto_link_states_one_for_one() {
        assert_eq!(
            state_from_error(&StudioError::NotConfigured),
            LinkState::NotConfigured
        );
        assert_eq!(
            state_from_error(&StudioError::Unauthorized { status: 403 }),
            LinkState::Unauthorized { status: 403 }
        );
        assert_eq!(
            state_from_error(&StudioError::Refused {
                status: 503,
                reason: "S".into()
            }),
            LinkState::Refused {
                status: 503,
                reason: "S".into()
            }
        );
    }

    #[test]
    fn read_only_by_construction() {
        // The tier is read-only against the Studio; no route here is anything but a GET.
        for p in [PROGRESS_PATH, METRICS_PATH, RUNS_PATH] {
            assert!(p.starts_with("/api/train/"));
        }
        let ep = StudioEndpoint::default();
        let tok = StudioToken::new("k").unwrap();
        assert!(build_stream_get(&ep, PROGRESS_PATH, &tok, None).starts_with("GET "));
    }
}
