# The approval protocol

**What this is.** How the Organon Console becomes the thing that answers "may I?" for an agent
running inside it — so an approval arrives as a **card in the conversation** with allow / deny /
allow-and-remember, instead of a tool silently bouncing.

**Status:** wire shapes **measured** 2026-08-12 on `claude.exe` 2.1.228, three headless runs.
Every request and response shape below is quoted from a live capture, not from documentation.
Nothing here is implemented yet.

---

## The measured findings, in the order that matters

### 🚨 1. MCP buys **no** permission exemption

A trivial, side-effect-free MCP tool bounced exactly like Bash:

```json
{"type":"system","subtype":"permission_denied","tool_name":"mcp__probe__echo_probe",
 "tool_use_id":"toolu_01SwDhC1uZLFQ6NwLuPbxvb5",
 "message":"Claude requested permissions to use mcp__probe__echo_probe, but you haven't granted it yet."}
```

**This refutes the reasoning that led here.** The case for MCP was argued as "an MCP tool has
its own permission identity, so it can be allowed narrowly instead of granting broad Bash" —
and that is *only* true once something answers approvals. MCP alone changes nothing.

📌 **The gate is entirely client-side.** The probe server's log proves it never received
`tools/call` — only `initialize` and `tools/list`. When permission is refused, **nothing reaches
our process at all**, so there is no server-side hook to build on.

### 🚨 2. `--permission-prompt-tool` works, and it gates **Bash too**

This is the decisive result. The flag is **absent from `--help`** on 2.1.228 but present in the
binary: *"MCP tool to use for permission prompts (only works with --print)"*.

`env | head -2` — a command that bounced unaided earlier the same day — was routed to the
handler and executed on approval. **So the console can render an approval card for everything
the agent does, not only for its own verbs.**

### 3. The request shape — everything a card needs, and nothing extra

```json
{"method":"tools/call","params":{"name":"approve_tool","arguments":{
  "tool_name":"mcp__probe__echo_probe",
  "input":{"text":"ALPHA"},
  "tool_use_id":"toolu_011k3yjKzmcedxXdEGWcwhpg"},
  "_meta":{"claudecode/toolUseId":"toolu_011k3yjKzmcedxXdEGWcwhpg","progressToken":2}},
 "jsonrpc":"2.0","id":2}
```

Three flat arguments: **`tool_name`, `input`, `tool_use_id`**. The last one is what lets a card
attach to the tool element already in the transcript — the same correlation the conversation
view already does for results.

### 4. The response shape — a JSON **string** inside ordinary MCP text content

```
allow: {"behavior":"allow","updatedInput":{"text":"ALPHA"}}
deny:  {"behavior":"deny","message":"probe handler denied this call on purpose"}
```

⚠️ **`updatedInput` is mandatory on allow** — echo the input back; it is re-validated against
the tool's schema. It is not ceremony: it means the console can **rewrite arguments at approval
time**, so a card can offer *"allow, but not that path"* rather than only yes or no. That is a
capability worth designing for rather than discovering.

A deny surfaces to the model as `"non_execution_kind":"permission-rule"`.

### 🚨 4b. The permission call times out after **60 seconds** — and progress notifications extend it

🚨 **Retraction.** §4 and §8 were written on the belief that the client waits indefinitely for a
permission answer — *"a card with no timeout, the client just waits."* **That is false**, and it
was found the only way it could be: James read a card, took longer than a minute, and the tool
failed underneath him with `Error calling tool (Write): The operation timed out.`

**Measured, 2026-08-12**, with a standalone probe that timestamps the request and watches the
socket:

```
[  5852 ms] conn #1 PERMISSION CALL  id=2 progressToken=Some("2")
[ 65862 ms] socket error at 60.010 s: connection forcibly closed (10054)
[ 67872 ms] conn #2 PERMISSION CALL  id=3 progressToken=Some("3")   ← the model retried
[127878 ms] socket error at 60.005 s
```

**60.0 s, twice, to the millisecond.** Both surfaced to the model as a tool-use timeout.

**The `progressToken` in `_meta` (§3) is the way out, and it works.** Answer the POST with
`text/event-stream` and emit `notifications/progress` against the request's own token:

| beat | stalled | outcome |
|---|---|---|
| 5 s | 90 s | answered `allow` at 90.088 s, 17 notifications, file written |
| 10 s | 300 s | answered `allow` at 300.142 s, 29 notifications, model reported success |

No abort at any point, and **no ceiling found at 5× the deadline**.

📌 **SSE is forced here, not chosen.** A server-initiated message about a request rides that
request's own response stream, and §8 established that the optional `GET /mcp` push stream can
be `405`ed. So the permission call — and only it — is answered on a stream; everything else
stays plain request/response.

⚠️ **The beat is also a liveness check, and it must fail closed.** If the agent stops waiting —
turn cancelled, process gone, client gave up regardless — the gate **denies** and the card says
so. It must never allow on timeout: a console that approves things because the human was slow is
worse than one that asks twice.

⚠️ **Never leave an orphaned card.** A card still offering *allow* for a call that already failed
is worse than no card, because it invites an answer that cannot matter.

### 🚨 5. There is no persistence — every call is asked

Three calls to the same tool produced three separate handler requests. **"Allow and remember"
does not exist upstream; it is ours to build.** The console keeps its own decision memory and
auto-answers from it. That is a feature, not a workaround — it means the remembering happens
where the human made the decision, and can be shown and revoked in the same interface.

✅ **Built, and since widened.** The memory now holds per-call entries *and* one session-wide
allow — *"allow everything for the rest of this session"*, the fourth button on the card. It
is the same memory, not a second mechanism, and it is emphatically **not** a permission mode:
§9 makes `bypassPermissions` unreachable and `dontAsk` a refusal, whereas this is the console
answering *yes* to a question it is still being asked. The handler still runs, the card is
still drawn, every call is still recorded.

⚠️ **Two properties this document should be read as constraining.** A per-call decision
outranks the blanket one, so a remembered **deny** survives an "allow everything" — the wide
grant is the default for calls nobody decided, never an overrule of a specific refusal. And
scope is unchanged: it dies with the tab, nothing is written to disk. A blanket allow that
survived a restart would be inherited by a session nobody was watching.

### 6. `--allowedTools` works and **short-circuits the handler**

Exact spelling, literal namespaced name: `--allowedTools "mcp__probe__echo_probe"`. An
allowlisted tool executed even with the handler rigged to deny it, and the handler was never
consulted.

**So it is the blunt instrument, not the mechanism.** Useful for the console's own verbs at
spawn if we ever decide they need no human in the loop; wrong for anything else, because it
removes the card entirely.

---

### 🚨 7. The model cannot call the handler — Claude Code filters it out

The obvious way this design could have been decorative: the handler is an ordinary tool on an
ordinary MCP server, so if the **model** could call it, it could hand itself
`{"behavior":"allow"}` and route around the human entirely.

**Measured: it cannot.** With `approve_tool` wired as `--permission-prompt-tool`, `system/init`'s
`tools` array contained only `mcp__probe__echo_probe` — advertised in the *same* `tools/list`
response, from the *same* server. `approve_tool` appeared **zero** times in the init event.

Two ways this measurement could have fooled us, both checked:

- **Deferred listing.** MCP tools arrive deferred in this build, so absence might have meant
  "not preloaded" rather than "unreachable." Ruled out: in the earlier run `echo_probe` *was*
  listed in init while deferred. Absence is genuine.
- **A server that never started.** "No `tools/call` received" is indistinguishable from a dead
  server. Ruled out by the server log showing startup and a served `tools/list`, then zero calls.

Prompted to search for the tool by name and call it, the model's `ToolSearch` returned
`"matches":[]` and it stopped. **The route is closed at the tool-exposure layer** — the model
cannot form the call at all, rather than forming it and being refused, which is the stronger
place to close it.

⚠️ **The guarantee is tied to the flag.** Claude Code removes the handler from the model's tool
set *because* `--permission-prompt-tool` names it. **An approval-shaped tool that is not the
designated permission tool would be an ordinary model-callable tool with no such protection.**
Never serve a second one.

#### 🚨 7a. The re-measurement is now the console's own job — and it is OUTSTANDING for the server that serves capability tools

Added 2026-08-13, with the change that made §9 point 4 bite. The console's server used to
serve **only** the handler; it now serves the console's verbs alongside it, which is exactly
the change §9 point 4 says to re-measure against.

⚠️ **It has not been measured.** The session that made the change could not launch the console
or build a release binary, and a live check needs a real `claude.exe` session against a
running console. Nothing here should be read as saying the property still holds — it is
*expected* to (the filter keys on the flag, and the flag is unchanged), and expectation is not
measurement.

✅ **What was built instead is the check itself, per session.** `system/init` already reports
the model's whole tool list, so `McpServer::audit_exposure` compares it against the handler's
namespaced name and the console's own served names, and the verdict goes to the band's log and
to stderr at every init. Three states, because two would hide the dangerous one:

| State | The line |
|---|---|
| handler absent, our tools visible | `approvals: handler withheld from the model as measured, N of M console tools visible (K offered)` |
| handler **present** | `🚨 the approval handler is in the model's own tool list … do not trust this session's cards` |
| no tools reported at all | `the session reported no tools — the approval handler's exposure could not be checked this init` |

**To close this out**: run the console, open a conversation tab, read that one line off stderr
or the band, and record the result here. If it opens with 🚨, the model can answer its own
approvals and the console's cards mean nothing.

⚠️ Note what the audit is and is not: it is the console checking **what the CLI reports**. A
CLI that reported its tool list wrongly would fool it. It is strictly more than a measurement
nobody re-runs, and strictly less than an independent one.

📌 Defence in depth, **plausible but unverified**: a client-initiated request carries a `_meta`
block the model has no way to author — `{"claudecode/toolUseId":…,"progressToken":…}` — so a
server could require it. This was *not* measured against a model-authored call, because
producing one would have meant deliberately re-opening the hole. Treat it as a belt, not the
braces.

---

### 🚨 8. The transport is HTTP, and it lets the console serve MCP **in-process**

Measured on 2.1.228. `claude mcp add --help` lists three transports — `stdio, sse, http` — and
`--mcp-config` accepts an `http` entry that the client connects **out** to.

```json
{ "mcpServers": { "organon": { "type": "http", "url": "http://127.0.0.1:8931/mcp" } } }
```

Two fields. No auth, no headers for loopback. Passed with `--strict-mcp-config` and
`--permission-prompt-tool mcp__organon__approve`.

**Verified end to end:** `system/init` reported `"mcp_servers":[{"name":"probe","status":"connected"}]`
with the server's tool in the model's list and the **approval tool correctly withheld** (§7's
filtering holds over HTTP too). A permission prompt then arrived at the HTTP server as a real
`tools/call`, carrying `tool_name`, `input`, `tool_use_id` and the `_meta` block; the server
answered `{"behavior":"allow","updatedInput":{…}}` and the write went through.

**Why this decides the architecture.** A stdio server is a *separate process* with no access to
the console's UI or state, so every approval would have to cross a process boundary and come
back — an IPC design with a race and a lifetime per request. Over HTTP the console serves MCP
**inside itself**, and the permission hook becomes a direct call into the state the UI is
already drawing.

**The handshake to serve** (both phases hit the same endpoint — there is no respawn, because
there is no process to spawn):

1. `POST /mcp` — `server/discover`, `mcp-protocol-version: 2026-07-28`.
2. `POST /mcp` — `initialize` at `2025-11-25`.
3. `POST /mcp` — `notifications/initialized`, no `id` → answer `202`, empty body.
4. `GET /mcp` with `Accept: text/event-stream` — the optional push stream. **Returning `405` is
   fine**; the client carried on. Request/response POST alone is sufficient.
5. `POST /mcp` — `tools/list`, then `tools/call`.

Client `Accept` is `application/json, text/event-stream`; plain `application/json` with a
`Content-Length` works throughout. An `Mcp-Session-Id` header is echoed back by the client — a
free per-connection handle if the console wants one.

⚠️ **Two traps that each cost a measurement run, and will cost a developer an afternoon:**

- **`echo` never prompts.** Safe read-only Bash is auto-approved, so `--permission-prompt-tool`
  looks dead when it is working perfectly. A first probe ran `echo HELLO` with zero traffic
  reaching the server.
- **Writes inside the session's own scratchpad never prompt either.** Asked for "a file
  probe.txt", the model picked a pre-blessed path. **Only an explicit absolute path outside it
  triggers the prompt.** When testing the console's approval card, pick a target the harness
  cannot pre-approve.

**Not determined:** `sse` (untested — `http` answered the question); session lifetime and
whether `Mcp-Session-Id` is validated or merely echoed; the `DELETE /mcp` teardown, never seen
in short runs; and whether the GET/SSE stream becomes mandatory for server-initiated traffic —
the client advertises `elicitation` and `roots.listChanged`, so a console that ever wants to
*push* will need to hold that GET open rather than `405` it.

---

## What this decides

**The console runs one MCP server serving two distinct things:**

1. **The capability tools** — `organon console …` verbs as named tools, generated from the same
   `CommandSpec` table the CLI is generated from (`console_spike_execution_plan.md` §5.9.25's
   rule: one vocabulary, many renderings, never a hand-written second copy).
2. **The permission handler** — pointed at by `--permission-prompt-tool`, answering for *every*
   tool the agent calls.

**MCP's real value is legibility, not permission.** Since approvals are answered either way, the
argument for exposing our verbs as MCP tools is that an approval card can then name a
**capability** — *"show a control panel"* — instead of displaying a shell command the human has
to parse. That is a genuine UX difference and it is the honest reason to do it.

✅ **Both are now served** (2026-08-13). The console builds its server from `console_specs()`
and answers permissions from the same one. ⚠️ Two things this document should not be read as
claiming: no agent has yet *called* a capability tool, so the better card is built and unseen;
and §7a's re-measurement against this shape is outstanding.

---

## Implementation notes, measured

- **Claude Code probes twice, in two processes.** First a `server/discover` call at
  `protocolVersion 2026-07-28`; a `-32601` (method not found) reply is harmless, after which it
  **respawns** and performs a normal `initialize` at `2025-11-25`. The server must tolerate an
  unknown method and a cold second start.
- Config that worked: `{"mcpServers":{"probe":{"type":"stdio","command":…,"args":[…]}}}` passed
  with `--mcp-config`. Pair it with `--strict-mcp-config` so the user's other servers are not
  pulled in.
- **stdout carries JSON-RPC only.** Log anywhere else.
- ⚠️ **A built-in safe-command classifier auto-approves some calls without consulting the
  handler** — plain `echo hi` ran untouched. Do not mistake that for the handler working; test
  with something that actually needs approval.
- ⚠️ **Tools that request user interaction are rejected outright**: the binary carries *"MCP tool
  requires user interaction; not supported via --permission-prompt-tool"*. **Our own MCP tools
  must therefore be non-interactive** — they act and return, and any conversation happens in our
  UI, not through MCP elicitation.
- MCP tools arrive **deferred** in this build: the model spent a `ToolSearch` call before it
  could invoke one. Worth knowing when judging latency.

---

## §9 — What building against this document revealed it was missing

Added 2026-08-12, after the first implementation. **Nothing above was wrong** — every shape held
exactly as quoted. These are gaps, and each cost time or would have.

1. 🚨 **§8 enumerates the handshake but not the HTTP framing you owe back.** A notification
   (`handle` → `None`) still owes a **status**: `202` with an empty body, **not** `200 null`.
   And every response needs an explicit `Content-Length: 0` when empty, or the client holds the
   connection waiting for a body that never comes.
2. **The client frames its requests with `Content-Length`, never chunked.** §8 characterises the
   client's `Accept` header and says nothing about how it sends. Defensive chunked decoding is
   wasted work.
3. 🚨 **The request shape in §3 is complete, but its *timing* is not stated — and the timing
   decides where the card goes.** The permission `tools/call` arrives **after** the tool's
   `content_block_stop` and after the complete `tool_use` in the `assistant` message. So the
   tool card already exists in the transcript when the question lands, and the approval card
   belongs directly **beneath it**. Had it arrived first, "appearing where the agent is working"
   would have needed an entirely different mechanism.
4. **§7's withholding guarantee was measured against a *probe* server; re-measure it per
   server.** Done for ours *when it served nothing else*: `tools mentioning 'organon' = []`
   out of 36 offered. It is the security property, it is one line of output, and it is cheap
   enough to check every time. ⚠️ **That measurement no longer describes the current server**
   — it now serves the console's verbs too, and §7a is the outstanding re-check plus the
   machinery that performs it automatically from here.
5. **The empty-capability-table case is not discussed and is what an approvals tier actually
   wants.** A server whose `tools/list` returns **only the handler** still reports
   `status: connected`, and the model simply sees no tools from it. That is the *safest* shape —
   answer for everything, expose nothing — and §"What this decides" wrongly implies both must be
   served together. ✅ Still reachable and still honest: `Capabilities::none()` is that shape,
   and it is what every test and every caller with no verbs to offer passes.
6. 🚨 **"Serve the capability tools" reads like one line and is not.** The tool table was
   generated, the schemas were generated, the argument checking was generated — and the
   console still constructed its server with an empty slice for weeks, because *connecting*
   them needs a dispatch that can act on a console verb from the **serve thread**, and the
   `CommandService` that validates one borrows the session log on the **UI thread**. The
   answer is neither a channel nor a second service: the dispatch writes the op onto the
   console's own command sidecar, which the frame loop already drains through that very
   service. One transport, one audited apply path, no process spawned. ⚠️ The consequence to
   state out loud is that the tool returns **accepted**, not **applied** — the op lands on the
   next frame.

📌 **One API consequence, forced by §8's own conclusion.** The serve loop must not be the UI
thread, so the responder crosses one: `PermissionResponder` is now `+ Send` and `McpServer` is
`Send`.

---

## Open, and worth one run each when it matters

- Whether `--allowedTools` accepts a wildcard over a server (`mcp__probe__*`); the literal name
  is confirmed, the pattern form is untested.
- Whether a deny can be made non-terminal, or carry structured data back to the model beyond
  `message`.
- Whether a decision can be cached upstream by any response field — no evidence of a "remember"
  field, which is why §5 assigns remembering to us.
