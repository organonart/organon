//! #452 Tier 3 — **"the eyes"**: the `snap` / `record` request+reply protocol.
//!
//! Unlike `set` / `do` / `release` (fire-and-forget `CliOp`s through the override lane),
//! `snap` and `record` need the VISUAL to do GPU work and hand a file path **back**. The
//! CLI is still never an IPC writer: it appends a request line to [`ipc::eyes_cmd_path`]
//! (`<nonce> <verb>`), the visual drains it, acts, and appends a reply line to
//! [`ipc::eyes_reply_path`] (`<nonce> ok|err <text>`). The CLI polls the reply file for
//! its own nonce. Both channels are append-only text with the same file-length/cursor
//! drain discipline as the command channel.
//!
//! **Everything here is pure — parse and format only.** `bin/ctl.rs` owns the nonce, the
//! I/O and the poll loop; `world.rs` owns the drain and the GPU work.
//!
//! ## Why it lives in core (organon#49 T4c-i)
//!
//! It was `cli::EyesReq`, and `cli.rs` cannot descend — it reaches `recipe`, `clip` and
//! `preset`, which are the plugin's own surface. But this protocol reaches **nothing**:
//! no plugin types, no params, not even `serde`. It is a wire format, which is precisely
//! what `organon-core` is for, and it sits beside the two path functions it is the format
//! *of* — [`ipc::eyes_cmd_path`] and [`ipc::eyes_reply_path`], whose doc comment already
//! pointed at this type before it lived here.
//!
//! ⚠️ **It is a separate module rather than more of `ipc.rs` on purpose.** `ipc.rs` owns
//! the `Shared` layout, which is append-only across a process boundary and whose byte
//! offsets are load-bearing; a text protocol with no layout at all does not belong inside
//! that invariant's file just because it shares a transport directory.
//!
//! `crate::cli` re-exports all four names, so every existing `cli::EyesReq` path resolves.
//!
//! [`ipc::eyes_cmd_path`]: crate::ipc::eyes_cmd_path
//! [`ipc::eyes_reply_path`]: crate::ipc::eyes_reply_path

/// A parsed `organon` "eyes" request (the payload of one request line, minus the
/// leading nonce).
#[derive(Debug, Clone, PartialEq)]
pub enum EyesReq {
    /// Read one frame back to a PNG at this (absolute) path.
    Snap { path: String },
    /// Start the in-app recorder; `bars` = beat-synced auto-stop length (0 = free-run).
    RecordStart { bars: u32 },
    /// Stop the in-app recorder.
    RecordStop,
}

impl EyesReq {
    /// The request line for a given nonce (single line, newline-free).
    pub fn to_line(&self, nonce: &str) -> String {
        match self {
            EyesReq::Snap { path } => format!("{nonce} snap {path}"),
            EyesReq::RecordStart { bars } => format!("{nonce} record start {bars}"),
            EyesReq::RecordStop => format!("{nonce} record stop"),
        }
    }

    /// Parse a request line → `(nonce, req)`. Rejects unknown verbs / bad bars.
    pub fn parse(line: &str) -> Option<(String, EyesReq)> {
        let line = line.trim();
        let (nonce, rest) = line.split_once(' ')?;
        let req = if let Some(p) = rest.strip_prefix("snap ") {
            let p = p.trim();
            if p.is_empty() {
                return None;
            }
            EyesReq::Snap { path: p.to_string() }
        } else if let Some(b) = rest.strip_prefix("record start ") {
            EyesReq::RecordStart { bars: b.trim().parse().ok()? }
        } else if rest.trim() == "record stop" {
            EyesReq::RecordStop
        } else {
            return None;
        };
        Some((nonce.to_string(), req))
    }
}

/// Format one reply line (visual → CLI) for a nonce.
pub fn eyes_reply_line(nonce: &str, result: &Result<String, String>) -> String {
    match result {
        Ok(t) => format!("{nonce} ok {t}"),
        Err(e) => format!("{nonce} err {e}"),
    }
}

/// Scan a reply-file body for `nonce`, returning its outcome (the path text on
/// `ok`, the message on `err`) or `None` if no reply for it has landed yet.
pub fn find_eyes_reply(body: &str, nonce: &str) -> Option<Result<String, String>> {
    for line in body.lines() {
        let line = line.trim();
        let Some((n, rest)) = line.split_once(' ') else {
            continue;
        };
        if n != nonce {
            continue;
        }
        if let Some(t) = rest.strip_prefix("ok ") {
            return Some(Ok(t.to_string()));
        }
        if let Some(e) = rest.strip_prefix("err ") {
            return Some(Err(e.to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eyes_requests_round_trip_and_replies_match_by_nonce() {
        // Round-trip every request kind through its line form.
        for req in [
            EyesReq::Snap { path: "/tmp/a b.png".into() },
            EyesReq::RecordStart { bars: 8 },
            EyesReq::RecordStop,
        ] {
            let line = req.to_line("N1");
            assert!(!line.contains('\n'));
            assert_eq!(EyesReq::parse(&line), Some(("N1".to_string(), req)));
        }
        // Bad/empty verbs reject.
        assert!(EyesReq::parse("N1 snap ").is_none());
        assert!(EyesReq::parse("N1 record start xx").is_none());
        assert!(EyesReq::parse("N1 frobnicate").is_none());
        assert!(EyesReq::parse("noverb").is_none());

        // Reply scanning keys strictly off the nonce, ok vs err.
        let body = "N0 ok /x.png\nN1 err ffmpeg missing\nN2 ok /y.png\n";
        assert_eq!(find_eyes_reply(body, "N0"), Some(Ok("/x.png".into())));
        assert_eq!(find_eyes_reply(body, "N1"), Some(Err("ffmpeg missing".into())));
        assert_eq!(find_eyes_reply(body, "N2"), Some(Ok("/y.png".into())));
        assert_eq!(find_eyes_reply(body, "N9"), None);
        // A nonce prefix must not match a different nonce.
        assert_eq!(find_eyes_reply("N10 ok /z.png\n", "N1"), None);
        // A half-written trailing line is simply ignored until complete.
        assert_eq!(find_eyes_reply("N1 o", "N1"), None);
    }
}
