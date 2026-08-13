//! The console's MCP server, served over loopback HTTP **inside the console process**.
//!
//! [`crate::mcp`] is the protocol as a value: messages in, messages out, no connection and
//! no thread. This module is the only thing that gives it one. The transport is HTTP rather
//! than stdio for a single reason, measured in `doc/console_approval_protocol.md` §8: a
//! stdio server is a **separate process** with no access to the console's UI or state, so
//! every approval would have to cross a process boundary and come back — an IPC design with
//! a race and a lifetime per request. Over HTTP the client connects *out* to us, so the
//! permission hook is a direct call into the state the UI is already drawing.
//!
//! # The handshake, as measured
//!
//! Claude Code 2.1.228 does exactly this against one endpoint (§8; there is no respawn,
//! because there is no process to spawn):
//!
//! | # | Request | Answer |
//! |---|---|---|
//! | 1 | `POST /mcp` `server/discover` at `2026-07-28` | `200` carrying JSON-RPC `-32601` |
//! | 2 | `POST /mcp` `initialize` at `2025-11-25` | `200` |
//! | 3 | `POST /mcp` `notifications/initialized` (no `id`) | **`202`, empty body** |
//! | 4 | `GET /mcp` `Accept: text/event-stream` | **`405` — measured fine, the client carried on** |
//! | 5 | `POST /mcp` `tools/list`, then `tools/call` | `200` |
//!
//! Step 3 is the one worth stating: a notification owes no answer, and
//! [`crate::mcp::McpServer::handle`] returns `None` for one — but HTTP still owes a
//! *status*, so `None` becomes `202` with no body rather than a `200` with `null` in it.
//!
//! Step 4 is why this is request/response only. Holding that GET open is what a server
//! would need to **push** — the client advertises `elicitation` and `roots.listChanged` —
//! and the console has nothing to push, so `405` is the honest answer rather than a
//! half-implemented stream.
//!
//! # Threads, and the one that must not block
//!
//! 🚨 **The permission hook blocks until a human answers.** That is the intended shape (a
//! card with no timeout, the client simply waiting on a JSON-RPC response), and it is why
//! none of this may run on the UI thread. So:
//!
//! * an **accept thread** owns the listener,
//! * a **connection thread** per connection reads requests and writes answers,
//! * one `Mutex` around the [`Endpoint`] serialises the protocol itself.
//!
//! ⚠️ A pending approval therefore holds that mutex, so a second concurrent MCP request
//! waits behind the card. That is not a bug being tolerated: the agent asking the question
//! cannot proceed either way, and the alternative — a second `McpServer` per connection —
//! would give one client two negotiated states.
//!
//! # What is testable without a socket
//!
//! [`Endpoint::answer`] takes a parsed [`Head`] and a body and returns the raw HTTP bytes,
//! so the whole table above is a headless test. [`parse_head`], [`http_response`] and
//! [`mcp_config_json`] are pure. One test does open a real loopback port, because "we can
//! bind, accept and answer" is not provable from pure functions.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use crate::mcp::{Heartbeat, McpServer, ToolDispatch, PARSE_ERROR};

/// The path the config points the client at, and the only path this server answers on.
pub const ENDPOINT: &str = "/mcp";

/// How long a connection read waits before looking at the shutdown flag again. Long
/// enough that an idle keep-alive connection costs nothing, short enough that closing a
/// tab is not perceptibly delayed.
const POLL: Duration = Duration::from_millis(250);

// ---------------------------------------------------------------------------
// The pure part — HTTP framing
// ---------------------------------------------------------------------------

/// A request line plus the one header that decides how much body follows.
///
/// Nothing else is kept, deliberately. `Accept` is `application/json, text/event-stream`
/// on every measured request and we answer JSON regardless; `Mcp-Session-Id` is echoed
/// back by the client but never validated by us, so remembering it would be a handle
/// nothing holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    pub method: String,
    /// The path with any query string removed.
    pub path: String,
    pub content_length: usize,
}

/// Parse the head of an HTTP/1.1 request — everything before the blank line.
///
/// Liberal about what it accepts and strict about what it reports: a missing or
/// unparseable `Content-Length` is zero (a body-less request), and a header line with no
/// colon is skipped rather than failing the request. `None` means there was not even a
/// request line, which is the only genuinely unusable input.
pub fn parse_head(text: &str) -> Option<Head> {
    let mut lines = text.split("\r\n");
    let mut parts = lines.next()?.split_whitespace();
    let method = parts.next()?.to_ascii_uppercase();
    let target = parts.next().unwrap_or("/");
    let path = target.split('?').next().unwrap_or(target).to_string();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.trim().eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    Some(Head { method, path, content_length })
}

/// One HTTP response, framed.
///
/// `Content-Length` is always written, including as `0`, because a client that has to
/// guess where a body ends will hold the connection open waiting for one.
pub fn http_response(status: u16, reason: &str, content_type: Option<&str>, body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} {reason}\r\n");
    if let Some(kind) = content_type {
        out.push_str(&format!("Content-Type: {kind}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn json_response(status: u16, reason: &str, value: &Value) -> Vec<u8> {
    http_response(status, reason, Some("application/json"), value.to_string().as_bytes())
}

/// The `--mcp-config` document, which is two fields and nothing else (§8).
///
/// No auth and no headers, because the endpoint is loopback-only and the port is
/// ephemeral. `type` is `http`, which is the transport that let the console serve MCP
/// in-process at all.
pub fn mcp_config_json(server_name: &str, url: &str) -> String {
    json!({ "mcpServers": { server_name: { "type": "http", "url": url } } }).to_string()
}

// ---------------------------------------------------------------------------
// The endpoint
// ---------------------------------------------------------------------------

/// The protocol server plus its dispatch, behind one HTTP path.
///
/// Separated from the socket so the whole measured handshake is a headless test.
pub struct Endpoint {
    server: McpServer,
    dispatch: Box<dyn ToolDispatch + Send>,
    path: String,
}

impl Endpoint {
    pub fn new(server: McpServer, dispatch: Box<dyn ToolDispatch + Send>) -> Self {
        Self { server, dispatch, path: ENDPOINT.to_string() }
    }

    pub fn server(&self) -> &McpServer {
        &self.server
    }

    /// Is this body the call that has to be answered on a stream, and what token may its
    /// keep-alives be addressed to?
    ///
    /// `Some(None)` — a permission call carrying no `progressToken` — is a real case and a
    /// deliberate one. The deadline cannot be held open without a token, but the *other*
    /// job of the stream still applies: an open connection is how the console finds out the
    /// client has gone, and a card that never finds out is the orphan this whole change
    /// exists to prevent. Measured clients always send a token; this is what happens if one
    /// stops.
    pub fn long_call(&self, body: &[u8]) -> Option<Option<Value>> {
        let message: Value = serde_json::from_slice(body).ok()?;
        self.server
            .is_permission_call(&message)
            .then(|| self.server.permission_progress_token(&message))
    }

    /// Answer one request while `beat` holds the connection open.
    ///
    /// Returns the JSON-RPC response *value* rather than framed bytes, because the caller
    /// has already committed to a `text/event-stream` body by the time this is called and
    /// owes the answer as one more event, not as a second set of headers.
    pub fn answer_streamed(&mut self, body: &[u8], beat: &mut dyn Heartbeat) -> Option<Value> {
        match serde_json::from_slice::<Value>(body) {
            Ok(message) => self.server.handle_with(&message, &mut *self.dispatch, beat),
            // Unreachable in practice — the caller parsed this body to find the token —
            // but a parse error is still a JSON-RPC answer, never silence.
            Err(error) => Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": PARSE_ERROR, "message": error.to_string() },
            })),
        }
    }

    /// Answer one request. Returns the complete response bytes, headers and all.
    pub fn answer(&mut self, head: &Head, body: &[u8]) -> Vec<u8> {
        if head.path != self.path {
            return http_response(404, "Not Found", None, b"");
        }
        match head.method.as_str() {
            "POST" => match serde_json::from_slice::<Value>(body) {
                Ok(message) => match self.server.handle(&message, &mut *self.dispatch) {
                    Some(response) => json_response(200, "OK", &response),
                    // A notification owes no JSON-RPC answer but still owes a status.
                    None => http_response(202, "Accepted", None, b""),
                },
                Err(error) => json_response(
                    400,
                    "Bad Request",
                    &json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": { "code": PARSE_ERROR, "message": error.to_string() },
                    }),
                ),
            },
            // §8 step 4. The optional server-push stream; we have nothing to push.
            "GET" => http_response(405, "Method Not Allowed", None, b""),
            // Never seen in a measured run, and harmless to honour: there is no
            // per-session state to tear down, because the server holds none.
            "DELETE" => http_response(204, "No Content", None, b""),
            _ => http_response(405, "Method Not Allowed", None, b""),
        }
    }
}

// ---------------------------------------------------------------------------
// The socket
// ---------------------------------------------------------------------------

/// A live loopback MCP server. **Dropping it stops the server.**
pub struct McpHttp {
    port: u16,
    alive: Arc<AtomicBool>,
}

impl McpHttp {
    /// Bind an ephemeral loopback port and start serving.
    ///
    /// The port is chosen by the OS (`127.0.0.1:0`) rather than picked from a range: a
    /// fixed port would collide between two console windows, and probing for a free one
    /// races anything else on the machine between the probe and the bind.
    pub fn start(server: McpServer, dispatch: Box<dyn ToolDispatch + Send>) -> io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let port = listener.local_addr()?.port();
        let alive = Arc::new(AtomicBool::new(true));
        let endpoint = Arc::new(Mutex::new(Endpoint::new(server, dispatch)));

        let accept_alive = alive.clone();
        std::thread::Builder::new().name("mcp-http".into()).spawn(move || {
            for stream in listener.incoming() {
                if !accept_alive.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let endpoint = endpoint.clone();
                let alive = accept_alive.clone();
                let _ = std::thread::Builder::new()
                    .name("mcp-http-conn".into())
                    .spawn(move || serve_connection(stream, endpoint, alive));
            }
        })?;

        Ok(Self { port, alive })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The URL the `--mcp-config` document carries.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}{ENDPOINT}", self.port)
    }
}

impl Drop for McpHttp {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        // `incoming()` is blocked in `accept`; nothing else wakes it. One throwaway
        // connection does, and the flag is already false by the time it is examined.
        let _ = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)));
    }
}

fn serve_connection(mut stream: TcpStream, endpoint: Arc<Mutex<Endpoint>>, alive: Arc<AtomicBool>) {
    let _ = stream.set_read_timeout(Some(POLL));
    let mut buf: Vec<u8> = Vec::new();
    loop {
        // The head, then exactly `content_length` more bytes. The buffer survives the
        // iteration because HTTP/1.1 keep-alive means the next request may already be
        // sitting behind this one's body.
        let head_end = loop {
            if let Some(at) = find(&buf, b"\r\n\r\n") {
                break at + 4;
            }
            match fill(&mut stream, &mut buf, &alive) {
                Ok(true) => {}
                _ => return,
            }
        };
        let Some(head) = parse_head(&String::from_utf8_lossy(&buf[..head_end])) else { return };
        while buf.len() < head_end + head.content_length {
            match fill(&mut stream, &mut buf, &alive) {
                Ok(true) => {}
                _ => return,
            }
        }
        let body = buf[head_end..head_end + head.content_length].to_vec();
        buf.drain(..head_end + head.content_length);

        // 🚨 A permission call is the one request that can outlive the client's patience,
        // and it is answered on a stream so the wait can be reported. Everything else is
        // request/response, as measured.
        let long = match endpoint.lock() {
            Ok(endpoint) if head.method == "POST" && head.path == ENDPOINT => {
                endpoint.long_call(&body)
            }
            _ => None,
        };
        if let Some(token) = long {
            let mut call = SseCall::new(&mut stream, &mut buf, token);
            if call.open().is_err() {
                return;
            }
            // The lock is held across the answer, which is where the permission hook
            // blocks for as long as a human takes. See the module doc: that is the design.
            let value = match endpoint.lock() {
                Ok(mut endpoint) => endpoint.answer_streamed(&body, &mut call),
                Err(_) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": PARSE_ERROR, "message": "the server lost its protocol state" },
                })),
            };
            if call.finish(value.as_ref()).is_err() {
                return;
            }
            continue;
        }

        let response = match endpoint.lock() {
            Ok(mut endpoint) => endpoint.answer(&head, &body),
            // A poisoned mutex means an earlier request panicked mid-protocol. Saying so
            // beats answering with a body built from an unknown state.
            Err(_) => http_response(500, "Internal Server Error", None, b""),
        };
        if stream.write_all(&response).is_err() || stream.flush().is_err() {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// The long call
// ---------------------------------------------------------------------------

/// One permission call, answered as a `text/event-stream` body.
///
/// 🚨 **This exists because there is a deadline, and it was not in the design.**
/// [`crate::mcp::CLIENT_DEADLINE_SECS`] records what was measured: the client aborts the socket 60 s
/// after asking, the model is told *"The operation timed out"*, and the tool fails while
/// the card is still on screen asking a question whose answer can no longer matter.
///
/// Answering a POST with SSE instead of JSON is what makes a keep-alive expressible at
/// all: `notifications/progress` is a **server-initiated** message, and over this
/// transport a server-initiated message related to a request rides that request's own
/// response stream. There is no other channel — the console `405`s the optional `GET`
/// push stream (§8 step 4), so nothing is held open outside a call.
///
/// ⚠️ **The stream must be terminated even when the answer is an error**, or the client
/// waits on a body that never ends — the chunked-encoding restatement of §9.1's rule that
/// an empty response still owes a `Content-Length`.
struct SseCall<'a> {
    stream: &'a mut TcpStream,
    /// Anything the client sends *during* the call. Read only to notice a hangup, kept
    /// because throwing away a pipelined request would corrupt the connection.
    spill: &'a mut Vec<u8>,
    /// `None` for a permission call that carried no `progressToken`: the stream is then a
    /// hangup detector and nothing more. See [`Endpoint::long_call`].
    token: Option<Value>,
    sent: u64,
}

impl<'a> SseCall<'a> {
    fn new(stream: &'a mut TcpStream, spill: &'a mut Vec<u8>, token: Option<Value>) -> Self {
        Self { stream, spill, token, sent: 0 }
    }

    /// The response head. Sent before the hook is called, because the client starts its
    /// clock when it asks and a stream that opens late has already lost time.
    fn open(&mut self) -> io::Result<()> {
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                    Cache-Control: no-cache\r\nTransfer-Encoding: chunked\r\n\r\n";
        self.stream.write_all(head.as_bytes())?;
        self.stream.flush()
    }

    /// The answer, then the end of the chunked body — which returns the connection to
    /// ordinary request/response for whatever the client asks next.
    fn finish(&mut self, value: Option<&Value>) -> io::Result<()> {
        if let Some(value) = value {
            self.event(&value.to_string())?;
        }
        self.stream.write_all(b"0\r\n\r\n")?;
        self.stream.flush()
    }

    fn event(&mut self, json: &str) -> io::Result<()> {
        let frame = format!("event: message\ndata: {json}\n\n");
        self.stream.write_all(format!("{:x}\r\n{frame}\r\n", frame.len()).as_bytes())?;
        self.stream.flush()
    }

    /// Has the client gone? Read once, without waiting: end-of-stream or a socket error
    /// is a hangup, a would-block is a client that is simply still listening, and
    /// anything else is a request that arrived early and is kept for the loop above.
    fn client_present(&mut self) -> bool {
        let restore = self.stream.read_timeout().ok().flatten();
        if self.stream.set_read_timeout(Some(Duration::from_millis(1))).is_err() {
            return true;
        }
        let mut chunk = [0u8; 4096];
        let present = match self.stream.read(&mut chunk) {
            Ok(0) => false,
            Ok(n) => {
                self.spill.extend_from_slice(&chunk[..n]);
                true
            }
            Err(e) => matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
            ),
        };
        let _ = self.stream.set_read_timeout(restore);
        present
    }
}

impl Heartbeat for SseCall<'_> {
    /// One `notifications/progress` against the request's own token, then: is the client
    /// still there?
    ///
    /// The `message` is the honest one: nothing is *progressing*, a human is deciding. It
    /// is carried anyway because a client that surfaces progress text should say why the
    /// call is slow rather than imply work is happening.
    ///
    /// ⚠️ **The liveness check runs even when there is no token to beat against.** Holding
    /// the deadline open and noticing the client leave are two jobs, and only the first
    /// needs a token.
    fn beat(&mut self) -> bool {
        if let Some(token) = self.token.clone() {
            self.sent += 1;
            let note = json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": {
                    "progressToken": token,
                    "progress": self.sent,
                    "message": "waiting for a human at the Organon Console",
                },
            });
            if self.event(&note.to_string()).is_err() {
                return false;
            }
        }
        self.client_present()
    }
}

/// Read once into `buf`. `Ok(false)` is end of stream *or* shutdown — both mean stop.
fn fill(stream: &mut TcpStream, buf: &mut Vec<u8>, alive: &AtomicBool) -> io::Result<bool> {
    let mut chunk = [0u8; 8192];
    loop {
        if !alive.load(Ordering::Relaxed) {
            return Ok(false);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(false),
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                return Ok(true);
            }
            // The read timeout firing on an idle keep-alive connection is the normal
            // case, not an error — it is the only thing that lets shutdown be noticed.
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e),
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// The config file
// ---------------------------------------------------------------------------

/// A `--mcp-config` document on disk for the lifetime of one agent session.
///
/// Per session rather than per install because the URL carries an **ephemeral port**: a
/// shared file would be stale the moment a second console opened, and the failure would
/// read as "the agent cannot see our server" long after the cause.
pub struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    /// Write `json` into `dir` under a name unique to this process and port.
    pub fn write(dir: &Path, stem: &str, json: &str) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{stem}.json"));
        std::fs::write(&path, json)?;
        Ok(Self { path })
    }

    /// The conventional name: process and port, so two consoles never collide and a
    /// leftover file names the run it came from.
    pub fn stem_for(port: u16) -> String {
        format!("organon-console-mcp-{}-{port}", std::process::id())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ConfigFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{
        NoDispatch, PermissionDecision, PermissionRequest, PermissionResponder,
        DEFAULT_PERMISSION_TOOL,
    };

    fn endpoint(responder: Box<dyn PermissionResponder + Send>) -> Endpoint {
        Endpoint::new(McpServer::new(&[], responder), Box::new(NoDispatch))
    }

    fn allow_all() -> Box<dyn PermissionResponder + Send> {
        Box::new(PermissionDecision::allow_unchanged as fn(&PermissionRequest) -> _)
    }

    fn post(body: &str) -> (Head, Vec<u8>) {
        (
            Head {
                method: "POST".into(),
                path: ENDPOINT.into(),
                content_length: body.len(),
            },
            body.as_bytes().to_vec(),
        )
    }

    /// `(status, body)` out of a raw response.
    fn split(response: &[u8]) -> (u16, String) {
        let text = String::from_utf8(response.to_vec()).expect("utf-8");
        let (head, body) = text.split_once("\r\n\r\n").expect("a blank line");
        let status: u16 = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .expect("a status code");
        (status, body.to_string())
    }

    #[test]
    fn a_request_line_and_its_content_length_are_read_back() {
        let head = parse_head(
            "POST /mcp?x=1 HTTP/1.1\r\nHost: 127.0.0.1:8931\r\n\
             Accept: application/json, text/event-stream\r\nContent-Length: 42\r\n\r\n",
        )
        .expect("a head");
        assert_eq!(head.method, "POST");
        assert_eq!(head.path, "/mcp", "the query string is not part of the path");
        assert_eq!(head.content_length, 42);

        // Liberal where being strict would only lose requests: no length is no body, a
        // junk length is no body, and a malformed header line is skipped.
        let bare = parse_head("GET /mcp HTTP/1.1\r\nAccept: text/event-stream\r\n\r\n").unwrap();
        assert_eq!(bare.content_length, 0);
        let junk =
            parse_head("POST /mcp HTTP/1.1\r\nnonsense\r\nContent-Length: many\r\n\r\n").unwrap();
        assert_eq!(junk.content_length, 0);
        assert_eq!(junk.method, "POST");
        assert_eq!(parse_head(""), None, "no request line at all is unusable");
    }

    #[test]
    fn a_response_always_states_its_length() {
        let empty = String::from_utf8(http_response(202, "Accepted", None, b"")).unwrap();
        assert!(empty.starts_with("HTTP/1.1 202 Accepted\r\n"));
        assert!(empty.contains("Content-Length: 0\r\n"));
        assert!(empty.ends_with("\r\n\r\n"), "an empty body is still a terminated head");
        assert!(!empty.contains("Content-Type"));

        let json = String::from_utf8(http_response(200, "OK", Some("application/json"), b"{}")).unwrap();
        assert!(json.contains("Content-Type: application/json\r\n"));
        assert!(json.ends_with("\r\n\r\n{}"));
    }

    /// 🚨 **The whole measured handshake** (`doc/console_approval_protocol.md` §8), in
    /// order, against one endpoint — including the two answers that look like failures
    /// and are not: `-32601` to the discover probe, and `405` to the optional GET stream.
    #[test]
    fn the_measured_handshake_is_answered_step_by_step() {
        let mut endpoint = endpoint(allow_all());

        // 1. `server/discover` at the probe's revision.
        let (head, body) = post(
            r#"{"jsonrpc":"2.0","id":0,"method":"server/discover","params":{"protocolVersion":"2026-07-28"}}"#,
        );
        let (status, text) = split(&endpoint.answer(&head, &body));
        assert_eq!(status, 200, "a JSON-RPC error is still an HTTP success");
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["error"]["code"], -32601);

        // 2. `initialize` at the revision the client negotiates.
        let (head, body) = post(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
        );
        let (status, text) = split(&endpoint.answer(&head, &body));
        assert_eq!(status, 200);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(endpoint.server().negotiated_protocol_version(), Some("2025-11-25"));

        // 3. The notification: a status, and nothing else.
        let (head, body) = post(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        let (status, text) = split(&endpoint.answer(&head, &body));
        assert_eq!(status, 202, "a notification owes no answer, but HTTP owes a status");
        assert!(text.is_empty());

        // 4. The optional push stream. Measured: refusing it is fine.
        let get = Head { method: "GET".into(), path: ENDPOINT.into(), content_length: 0 };
        assert_eq!(split(&endpoint.answer(&get, b"")).0, 405);

        // 5. `tools/list` — the handler is served, and with an empty spec table it is the
        //    only thing served. Claude Code then withholds it from the model (§7).
        let (head, body) = post(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let (status, text) = split(&endpoint.answer(&head, &body));
        assert_eq!(status, 200);
        let value: Value = serde_json::from_str(&text).unwrap();
        let tools = value["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], DEFAULT_PERMISSION_TOOL);

        // …and the call that is the point of all of it.
        let (head, body) = post(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"{DEFAULT_PERMISSION_TOOL}","arguments":{{"tool_name":"Bash","input":{{"command":"rm -rf /tmp/x"}},"tool_use_id":"toolu_1"}}}}}}"#
        ));
        let (status, text) = split(&endpoint.answer(&head, &body));
        assert_eq!(status, 200);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value["result"]["content"][0]["text"],
            r#"{"behavior":"allow","updatedInput":{"command":"rm -rf /tmp/x"}}"#
        );
    }

    #[test]
    fn another_path_is_not_this_server() {
        let mut endpoint = endpoint(allow_all());
        let head = Head { method: "POST".into(), path: "/".into(), content_length: 0 };
        assert_eq!(split(&endpoint.answer(&head, b"")).0, 404);
    }

    /// A body that is not JSON is reported, never swallowed, and the endpoint survives it
    /// — the HTTP restatement of the stdio server's parse-error rule.
    #[test]
    fn a_malformed_body_is_a_parse_error_and_the_endpoint_still_works() {
        let mut endpoint = endpoint(allow_all());
        let (head, body) = post("not json at all");
        let (status, text) = split(&endpoint.answer(&head, &body));
        assert_eq!(status, 400);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["error"]["code"], PARSE_ERROR);
        assert_eq!(value["id"], Value::Null);

        let (head, body) = post(r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#);
        assert_eq!(split(&endpoint.answer(&head, &body)).0, 200);
    }

    #[test]
    fn the_config_is_the_two_fields_the_client_needs() {
        let json = mcp_config_json("organon", "http://127.0.0.1:8931/mcp");
        assert_eq!(
            json,
            r#"{"mcpServers":{"organon":{"type":"http","url":"http://127.0.0.1:8931/mcp"}}}"#
        );
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["mcpServers"]["organon"].as_object().unwrap().len(), 2, "no auth, no headers");
    }

    #[test]
    fn a_config_file_names_its_process_and_port_and_is_removed_with_it() {
        let dir = std::env::temp_dir().join(format!("organon-mcp-http-{}", std::process::id()));
        let stem = ConfigFile::stem_for(4242);
        assert!(stem.contains(&std::process::id().to_string()));
        assert!(stem.ends_with("-4242"));

        let path = {
            let file = ConfigFile::write(&dir, &stem, "{}").expect("written");
            assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "{}");
            file.path().to_path_buf()
        };
        assert!(!path.exists(), "the config must not outlive the session it describes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- the long call, on a real socket ------------------------------------

    /// A permission `tools/call`, framed exactly as the client sends one (doc §3) — the
    /// `_meta` block included, because the `progressToken` in it is what decides that this
    /// request is answered on a stream.
    fn permission_call(command: &str) -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": DEFAULT_PERMISSION_TOOL,
                "arguments": {
                    "tool_name": "Bash",
                    "input": { "command": command },
                    "tool_use_id": "toolu_slow",
                },
                "_meta": { "claudecode/toolUseId": "toolu_slow", "progressToken": 2 },
            },
        })
        .to_string()
    }

    fn post_bytes(port: u16, body: &str) -> TcpStream {
        let mut stream =
            TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).unwrap();
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Accept: application/json, text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        stream
    }

    /// Which requests get a stream, and which do not. Pure, and the discriminator the
    /// connection loop asks first.
    #[test]
    fn only_a_permission_call_is_answered_on_a_stream() {
        let endpoint = endpoint(allow_all());
        assert_eq!(
            endpoint.long_call(permission_call("ls").as_bytes()),
            Some(Some(json!(2))),
            "the token the keep-alives are addressed to"
        );

        // ⚠️ A permission call with no `_meta` is **still** streamed. The deadline cannot be
        // held open without a token, but the stream's other job — noticing the client has
        // gone — needs only an open connection, and a card that never notices is the orphan
        // this whole path exists to prevent.
        let bare = json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": DEFAULT_PERMISSION_TOOL, "arguments": { "tool_name": "Bash", "input": {} } },
        })
        .to_string();
        assert_eq!(endpoint.long_call(bare.as_bytes()), Some(None));

        // Everything else returns at once and has nothing to report — a token on one of
        // them is not an invitation to hold a connection open.
        for other in [
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"progressToken":9}}}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"_meta":{"progressToken":9}}}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"viewport_set","_meta":{"progressToken":9}}}"#,
            "not json",
        ] {
            assert_eq!(endpoint.long_call(other.as_bytes()), None, "{other}");
        }
    }

    /// 🚨 **The fix, end to end on a real socket.** A human takes longer than the client's
    /// deadline; the call is held open by `notifications/progress` against the request's own
    /// token; the answer arrives when they click, and it is the answer they gave.
    ///
    /// Measured 2026-08-12 against `claude.exe` 2.1.228: a stalled permission call is
    /// aborted at **60.0 s** without progress, and answered at **300.1 s** with it.
    #[test]
    fn a_pending_call_is_held_open_by_progress_and_answered_when_the_human_clicks() {
        let (gate, inbox) = crate::approval::approval_channel();
        let gate = gate.beating_every(Duration::from_millis(20));
        let http = McpHttp::start(McpServer::new(&[], Box::new(gate)), Box::new(NoDispatch))
            .expect("bound a loopback port");

        let mut stream = post_bytes(http.port(), &permission_call("cargo build"));
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();

        // The question reaches the UI end, and the response is a stream rather than a body.
        let pending = inbox.recv_timeout(Duration::from_secs(5)).expect("a question");
        assert_eq!(pending.request().tool_name, "Bash");

        // Read until several keep-alives have arrived — the client's clock is being reset
        // meanwhile, which is the whole point.
        let mut seen = Vec::new();
        let mut chunk = [0u8; 4096];
        while count(&seen, "notifications/progress") < 3 {
            let n = stream.read(&mut chunk).expect("the stream stays open");
            assert!(n > 0, "the server closed instead of waiting");
            seen.extend_from_slice(&chunk[..n]);
        }
        let text = String::from_utf8_lossy(&seen).to_string();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        assert!(text.contains("Content-Type: text/event-stream\r\n"), "{text}");
        assert!(text.contains(r#""progressToken":2"#), "addressed to the request's own token");
        assert!(!pending.is_abandoned(), "the client is still there; the human is just slow");

        // …and then a human clicks.
        let allow = PermissionDecision::allow_unchanged(pending.request());
        pending.answer(allow);
        while !seen.ends_with(b"0\r\n\r\n") {
            let n = stream.read(&mut chunk).expect("the answer arrives");
            assert!(n > 0, "the server closed without answering");
            seen.extend_from_slice(&chunk[..n]);
        }
        let text = String::from_utf8_lossy(&seen).to_string();
        assert!(
            text.contains(r#"{\"behavior\":\"allow\",\"updatedInput\":{\"command\":\"cargo build\"}}"#),
            "the answer is the one the human gave, on the same stream: {text}"
        );
        // ⚠️ The terminating zero-length chunk, which is the streaming restatement of §9.1's
        // "an empty response still owes a `Content-Length`": without it the client waits on
        // a body that never ends, and the answer it already has never resolves the call.
        assert!(text.ends_with("0\r\n\r\n"), "the chunked body is terminated: {text:?}");
    }

    /// ⚠️ **And when the client hangs up first, the card stops asking.** This is the state a
    /// human actually saw: the tool had already failed, and the question was still on screen
    /// offering to allow it.
    #[test]
    fn a_client_that_hangs_up_abandons_the_question_instead_of_leaving_it_open() {
        let (gate, inbox) = crate::approval::approval_channel();
        let gate = gate.beating_every(Duration::from_millis(20));
        let http = McpHttp::start(McpServer::new(&[], Box::new(gate)), Box::new(NoDispatch))
            .expect("bound a loopback port");

        let mut stream = post_bytes(http.port(), &permission_call("rm -rf /"));
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let pending = inbox.recv_timeout(Duration::from_secs(5)).expect("a question");

        // Wait for the stream to be genuinely open before killing it, so the test is about
        // a hangup rather than a race with the response head.
        let mut chunk = [0u8; 4096];
        assert!(stream.read(&mut chunk).unwrap() > 0);
        drop(stream);

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !pending.is_abandoned() {
            assert!(std::time::Instant::now() < deadline, "the gate never noticed the hangup");
            std::thread::sleep(Duration::from_millis(10));
        }
        // The card can now say what happened; nothing about it still invites a click.
        assert!(pending.is_abandoned());
    }

    fn count(haystack: &[u8], needle: &str) -> usize {
        String::from_utf8_lossy(haystack).matches(needle).count()
    }

    /// The one thing pure functions cannot show: that it binds, accepts, and answers. A
    /// real socket, a real request, no external process.
    #[test]
    fn a_live_loopback_server_answers_a_real_request() {
        let http = McpHttp::start(McpServer::new(&[], allow_all()), Box::new(NoDispatch))
            .expect("bound a loopback port");
        assert!(http.port() > 0);
        assert_eq!(http.url(), format!("http://127.0.0.1:{}/mcp", http.port()));

        let mut stream =
            TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, http.port()))).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#;
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();

        // Read until the head *and* the whole declared body have arrived — a response
        // that happens to arrive in two segments must not read as a truncated one.
        let mut got = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            if let Some(at) = find(&got, b"\r\n\r\n") {
                let head = parse_head(&String::from_utf8_lossy(&got[..at + 4])).unwrap();
                if got.len() >= at + 4 + head.content_length {
                    break;
                }
            }
            let n = stream.read(&mut chunk).expect("a response");
            assert!(n > 0, "the server closed without answering");
            got.extend_from_slice(&chunk[..n]);
        }
        let (status, text) = split(&got);
        assert_eq!(status, 200);
        assert!(text.contains("2025-11-25"), "{text}");
    }
}
