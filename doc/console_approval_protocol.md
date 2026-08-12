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

### 🚨 5. There is no persistence — every call is asked

Three calls to the same tool produced three separate handler requests. **"Allow and remember"
does not exist upstream; it is ours to build.** The console keeps its own decision memory and
auto-answers from it. That is a feature, not a workaround — it means the remembering happens
where the human made the decision, and can be shown and revoked in the same interface.

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

📌 Defence in depth, **plausible but unverified**: a client-initiated request carries a `_meta`
block the model has no way to author — `{"claudecode/toolUseId":…,"progressToken":…}` — so a
server could require it. This was *not* measured against a model-authored call, because
producing one would have meant deliberately re-opening the hole. Treat it as a belt, not the
braces.

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

## Open, and worth one run each when it matters

- Whether `--allowedTools` accepts a wildcard over a server (`mcp__probe__*`); the literal name
  is confirmed, the pattern form is untested.
- Whether a deny can be made non-terminal, or carry structured data back to the model beyond
  `message`.
- Whether a decision can be cached upstream by any response field — no evidence of a "remember"
  field, which is why §5 assigns remembering to us.
