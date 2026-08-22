### Mind can watch a fine-tune happen: the training strip and the run shelf (#147 Tier 4)

Organon Mind reads a model at rest and lights it up while it thinks. It now also reads the half of
a model's life where the weights were *fit*: `organon-core/src/train.rs` holds Unsloth Studio's
progress stream open on a worker thread, backfills from its metrics route, and lists its run
history; `organon-mind/src/mind_train.rs` draws them into the Live Telemetry dock as one row above
the existing three columns, with the shelf folded away behind it. Read-only against the Studio —
three `GET`s and no `POST` anywhere, so nothing here can start, stop or alter a run. No `Shared`
field, no `LAYOUT_VERSION` movement, no new dependency.

🚨 **No state in this readout can be reached by a health probe, because it never calls one.** T1
established that `GET /api/health` is unauthenticated — it answers `200` with a correct key, a
wrong key and no key at all — so a green probe proves the Studio is *running* and never that the
credential is good. The tempting shape for a dashboard is *probe, go green, then stream*, and that
green would be a status line that cannot be wrong. So this tier opens with an **authenticated**
call (`/api/train/runs`) instead, and only an authenticated `2xx` may set `Idle` or `Live` — the
two states that claim the key works. A test asserts the other six cannot.

📌 **The three sentences are three variants.** *"Nothing is training"* is `Idle`, *"I cannot reach
the Studio"* is `Unreachable`, *"my key is wrong"* is `Unauthorized`, each with its own headline,
its own remedy naming the actual knob, and a `LinkState::asserts()` sentence saying what it does
and does not claim about the Studio, the credential and the run — shown on hover, so the claim
behind the colour is not something a viewer has to infer. ⚠️ A `401` is also *evidence the Studio
is up*, which is why `studio_answered()` is a different question from `credential_proven()`.

📌 **The Studio being absent is the normal case and is drawn that way.** With no `UNSLOTH_API_KEY`
the link spawns no thread and opens no socket at all. Unreachable is grey, not red — a red that
fires every day is a red nobody reads. Reconnect doubles from 2 s to 60 s and then *stops*, parking
until a person presses "Ask the Studio again". ⚠️ `Unauthorized` never retries even once: a key does
not become valid by being resent, and on Windows a process cannot see a `UNSLOTH_API_KEY` rotated
after it started, so that loop provably cannot succeed and the button is not offered.

🚨 **Three framings are stacked on one socket and two of them fail silently if you get them
wrong.** A streaming FastAPI response is `Transfer-Encoding: chunked`, so the raw bytes carry hex
size lines *interleaved with the SSE text* — feed those to an SSE parser and they parse as unknown
fields and are dropped without complaint, so the stream **appears to work** while every chunk
boundary corrupts whichever event it landed inside. `ChunkedDecoder` sits between them and is
incremental, because a boundary lands wherever the network puts it. And SSE itself: a `:` line is a
comment and must not become data, several `data:` lines join with `\n`, and a read can end on the
`\r` of a `\r\n` — which, treated as a terminator, dispatches an event one read early and then
reads a phantom blank line from the `\n` that follows. Every decoder is fed-and-buffered, and the
tests drive the whole wire path one byte at a time and assert the result is identical to one read.

🚨 **Reconnect carries `Last-Event-ID`, and losing it loses steps in a way that reads as a flat
spot rather than as an error.** The metrics backfill runs on *every* connect as the belt to that
header's braces, and the fold de-duplicates by step so the overlap is free. ⚠️ `grad_norm_history`
is indexed by `grad_norm_step_history`, **not** by `step_history` — the Studio serves two step
vectors because the gradient norm is not logged at every step a loss is, and pairing it with the
wrong one draws a curve whose x axis is quietly wrong while the shape still looks right. When a
value series arrives with no matching step vector at all the points are placed by index and the
strip says so on the legend, rather than presenting an invented axis as a reported one.

⚠️ **The stream gets its own timeouts and T1's are untouched.** `unsloth::TIMEOUT_SECS` is 5 s
because `/api/health` answers in constant time; a stream that is silent between heartbeats is
*healthy*, so reusing it would tear down a working connection every five seconds. `STREAM_POLL_SECS`
(1 s) is the socket read timeout — a responsiveness budget, so the worker sees its stop flag
promptly — and `STREAM_IDLE_SECS` (90 s) is the liveness budget, counted across however many polls
expire in a row.

✏️ **A sixth refusal, and it took building a UI to notice it.** T1 ships five (*not configured*,
*unreachable*, *unauthorized*, *refused*, *malformed*), and a malformed `ORGANON_UNSLOTH_ENDPOINT`
is none of them: `StudioConfig::from_env` reports that as an `EndpointError`, a *different type*
from `StudioError`, so it has no place in a taxonomy built out of the latter. Folding it into
`Malformed` would have been the easy move and would have told somebody with a typo in an
environment variable to go and inspect what is listening on a port. It is `Misconfigured`, it is
warm rather than quiet because someone typed a value that is not being honoured, and it does not
retry.

⚠️ **Nothing here has ever spoken to a running Studio** — it was not running on organon-one when
this landed, and it still is not. The documented shape is `TrainingMetricsResponse`; the SSE
`progress` payload's own field names are **inferred** from it. Every field is optional and aliased,
so a mismatch degrades to a blank number rather than a failed parse — and because a blank number is
exactly what a wrong guess looks like, the strip counts events and says so out loud once three have
arrived carrying nothing it recognises. `/api/train/runs` is likewise accepted as a bare array *or*
as `{"runs": […]}`, because a one-word difference between the spec and the build on this machine
would otherwise present as an empty shelf — which is the wrong sentence to show someone with forty
runs on disk.

⚠️ **`TrainingLink` holds its receiver in a `Mutex` and that is not decoration.**
`mpsc::Receiver` is `Send` but not `Sync`, and nih-plug's `create_egui_editor` requires the whole
editor-state struct to be `Sync` — so a bare receiver in `PresetUi` fails to compile at the *host*
boundary, hundreds of files away, with an error naming a private message type. A test in core pins
it where the fix belongs.

**Green and ready to try. No GPU has drawn any of this**, no Studio has answered any of it, and the
strip's placement, sizes and colours are taste calls that need a person and a running Studio.
