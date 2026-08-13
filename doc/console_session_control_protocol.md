# The session-control protocol

**What this is.** Whether the Organon Console's status strip can *change* what it currently only
*displays* — the model and the permission mode — on a live `stream-json` session, without
respawning the process.

**Status:** wire shapes **measured** 2026-08-12 on `claude.exe` **2.1.228**
(`C:\Users\james\.local\bin\claude.exe`), eight invocations — six headless sessions driven with
the argv `agent_session.rs::ARGS` builds, plus `--version` and `--help`. Every request and
response below is quoted from a live capture.

✏️ **Implemented 2026-08-12**, and this document has been corrected by the build rather than
left as it was written. `agent_session.rs` sends `set_model`, `set_permission_mode` and
`initialize`; `agent_event.rs` decodes the responses; `conversation_view.rs` draws the two
pickers. Building against a measurement is the only thing that finds the places where the
measurement was read one way and meant another — **four such places were found, and each is
corrected in place below rather than appended as an erratum** (§2b's discriminator, §3's
nesting warning, §4b's status, and the two `pending_*_requests` lists). Where a correction
changes what a consumer should do, the old reading is stated too, because a reader who
remembers the first version needs to know it moved.

⏱ **A note on dates before anything else.** The capture files were written on **2026-08-12**
local time; the `timestamp` fields *inside* them read `2026-08-13T05:…Z` because they are UTC.
Same run, two clocks.

**Provenance markers used throughout:**

- **MEASURED** — observed in a capture retained beside this document and quoted from it.
- **INFERRED** — read from `--help` or from strings in the binary and *not* exercised.

The approval protocol's value is that its shapes came from live captures. Nothing below is
dressed up as more than it is, and where the original measurement pass blurred the line, this
document says so in place.

---

## 🚨 0. The headline

**Both are live-changeable, and neither needs a respawn.** `claude` speaks an undocumented
**control protocol** on the same two pipes the console already owns: the console writes a
`control_request` line to stdin and gets a `control_response` line back on stdout. `set_model`
and `set_permission_mode` are two of its twelve subtypes. The round trips measured **272 ms**
and **17 ms**.

⚠️ **But three things about it will make the feature look broken if they are not designed for,
and all three are measured below:** the new model arrives only in a *repeat* `system/init`, which
the console's first-init-wins guard drops (§4); the model switch emits a **user-role message**
the transcript will render as a human turn (§2b); and one selectable mode — `dontAsk` — **removes
the console's approval cards entirely while the strip still says the console is in charge**
(§10).

📌 **This document measures; it does not settle.** Two of the responses these findings invite were
recorded below as **PROPOSED, NOT DECIDED** (§4b and §10a). Both change something already agreed —
one amends a settled spec, one constrains what a control may offer — and both were James's call,
not this document's. ✏️ **Both have since been answered**, and the two were answered differently:
**§4b was ruled on and the ruling is written into the spec it amends**
(`doc/console_spike_execution_plan.md` §5.9.3 rule 3); **§10a was settled by a design decision in
the implementation**, which is a weaker thing and is labelled as such where it sits. Each section
says which it got.

---

## 1. The control protocol — the finding everything else hangs off

**MEASURED** (`p1.out.jsonl` line 16, `p1.timeline.txt` +4459/+4731 ms). Sent on the console's own
stdin, on a session spawned with exactly `ARGS` (`-p --input-format stream-json --output-format
stream-json --include-partial-messages --replay-user-messages --verbose`) and **nothing else** —
no `--mcp-config`, no SDK, and critically **no `initialize` handshake first**:

```json
{"type":"control_request","request_id":"req-model-1","request":{"subtype":"set_model","model":"sonnet"}}
```

came back on stdout as:

```json
{"type":"control_response","response":{"subtype":"success","request_id":"req-model-1"}}
```

**The envelope is three fields** — `type`, a caller-chosen `request_id` string, and a `request`
object whose `subtype` selects the verb. The `request_id` is **ours to invent**; the CLI echoes it
back verbatim. A `"req-model-1"` that is not a UUID was accepted.

📌 **No handshake is required, and the bar is lower than "no `initialize`".** In the `dontAsk`
session (`p4`) the very first line written to stdin was a `set_permission_mode` request; it was
answered `success` at +1324 ms and the session's **first `system/init` did not appear until after
that** (`p4.out.jsonl` line 1 is the `control_response`, line 4 is the init). So a control lands
before the session has announced itself at all.

⚠️ **The build leans slightly past this measurement, and says so rather than rounding it up.**
`agent_session.rs` sends `Control::Initialize` **at spawn**, before the first turn — which is what
§3 recommends and what §6's "the init only arrives once input is pending" makes valuable (a tab
nobody has typed into never announced itself, and this line *is* input, so the strip now learns
its model at spawn instead of at the first human turn). But the control measured landing before an
init was `set_permission_mode`, **not `initialize`**, and one verb answering early does not
establish that the heaviest one does. What makes the gap cheap rather than dangerous is the
deadline: an `initialize` that is never answered is retired after 20 s and the failure is **an
empty picker that says the list has not arrived** — never a wedged tab, because nothing anywhere
waits on the answer. Worth one probe when somebody is next capturing this pipe.

**INFERRED** (binary strings, not exercised). The full subtype set the CLI accepts *from* a
client:

```
i2y=new Set(["set_model","set_permission_mode","interrupt","set_max_thinking_tokens",
"rename_session","set_color","mcp_authenticate","mcp_oauth_callback_url","mcp_reconnect",
"apply_flag_settings","side_question","reload_plugins"]);
```

and an unrecognised one is answered, not ignored — `Unsupported control request subtype: …`.

**`interrupt` is in that list**, and it is worth naming separately: the console currently has no
way to stop a running turn, and the measured `system/init` advertises
`"capabilities":["interrupt_receipt_v1","interrupt_cancel_queued_v1","msg_lifecycle_v1"]`
(**MEASURED**, `p4.out.jsonl` line 4) — which is the CLI stating it can receive one. Never
exercised here.

**MEASURED.** Failure is a distinct response subtype, not a silent drop (§9 has the quote):
`{"subtype":"error","request_id":…,"error":"…"}`. So a strip control can always tell whether it
took.

### 1a. What the CLI may send back the other way

**MEASURED.** One reverse-direction subtype was exercised: `can_use_tool` (§11, §12).

**INFERRED** (binary string, sibling of the set above), the client→console direction:

```
iBb=new Set(["can_use_tool","request_user_dialog","elicitation"]);
```

Corroborated from the measured side: the `initialize` response envelope carries
`pending_permission_requests` and `pending_user_dialog_requests` (§3). `request_user_dialog` and
`elicitation` were never seen on the wire here.

---

## 2. `set_model` — measured working, mid-session

**MEASURED.** Full sequence from one session (`p1.timeline.txt`, elapsed ms from spawn):

```
[     13 ms] -> {"type":"user",…"text":"Reply with exactly: OK1"}
[   4457 ms] <- "type":"result"                                   (turn 1 complete)
[   4459 ms] -> {"type":"control_request","request_id":"req-model-1",
                 "request":{"subtype":"set_model","model":"sonnet"}}
[   4731 ms] <- req-model-1                                       (272 ms round trip)
```

Turn 1's assistant message reported `"model":"claude-opus-5"`. Turn 2's reported:

```json
{"type":"assistant","message":{"model":"claude-sonnet-5",…,
 "diagnostics":{"cache_miss_reason":{"type":"model_changed","cache_missed_input_tokens":48862}}}}
```

**The switch is real and takes effect on the very next turn.** The `model` field accepted an alias
(`"sonnet"`).

**INFERRED** (schema string in the binary), and worth knowing because it is the reset path:

> `model: … "Model to switch to. Omitted, null, or 'default' resets to the session default model."`

### 2a. The cost the strip cannot avoid

**MEASURED.** A model switch **invalidates the prompt cache**. That is what
`cache_miss_reason: model_changed` above says, and the money follows it. Turn 1 read 25 282 cached
tokens; turn 2 read **0** and re-created 69 228.

The per-model costs are quoted from the `modelUsage` block of the final `result` line, not
inferred from the running total — ⚠️ `total_cost_usd` is **cumulative** (`0.301588` then
`0.7170369999999999`), so reading it as a per-turn price would double-count:

| model | cacheRead | cacheCreation | `costUSD` |
|---|---|---|---|
| `claude-opus-5[1m]` | 25 282 | 28 823 | 0.301006 |
| `claude-sonnet-5` | 0 | 69 228 | 0.415449 |

**$0.30 then $0.42, for the same three-token reply.**

📌 This is not a property of the control protocol; a respawn would pay it too. But it means a
model plate that is easy to click is a plate that is easy to spend money with, and the first turn
after a switch is the expensive one. Worth a design thought, not a blocker.

### 2b. 🚨 The switch emits a **user-role message** the transcript will render as a human turn

**MEASURED** (`p1.out.jsonl` line 15 — the request was written at +4459 ms and the ack is line 16):

```json
{"type":"user","message":{"role":"user","content":"<local-command-stdout>Set model to sonnet (claude-sonnet-5)</local-command-stdout>"},
 "session_id":"124488fd-…","parent_tool_use_id":null,"uuid":"23c14490-…",
 "timestamp":"2026-08-13T05:07:11.002Z","isReplay":true}
```

Three things about it, each of which matters to the conversation view:

1. It arrives **before** the `control_response` — line 15 versus line 16 in the same capture — so
   it is not something the console can suppress by waiting for the ack.
2. Its `content` is a **bare string, not an array of blocks**. `agent_event.rs::content_blocks`
   already handles that case (`Some(Value::String(text)) => vec![ContentBlock::Text(text.clone())]`),
   so it decodes — it does not fail, it just *renders*, as a user bubble containing literal
   `<local-command-stdout>…</local-command-stdout>` markup.
3. It carries `isReplay: true`, the same flag §5.9.3 rule 2 attaches to the human's own echoed
   turn — so **`isReplay` alone cannot distinguish "the human said this" from "the CLI is
   narrating a control action"**.

🚨 **Correction, found by implementing it: the wrapper is the ONLY discriminator, and the
`content` shape is not part of it.** This section originally said the discriminator was *"the
string `content` shape plus the `<local-command-stdout>` wrapper"*. The string half does not
survive contact:

- **The console's own `user_message_line` sends the array form**, so "string means the CLI, array
  means the human" is not a rule the console's own traffic obeys — and both forms collapse to the
  same `Vec<ContentBlock>` before anything can look at which arrived.
- Nothing measured says the CLI will not narrate through the array form later. One observation of
  a string is an observation, not a guarantee, and a predicate resting on it fails **open** — the
  fake turn comes back with no way to tell it apart.

**What is implemented** (`agent_event.rs::UserTurn::local_command_output`, whose doc argues its
own narrowness): *the line's entire text, trimmed, is one `<local-command-stdout>…` element and
nothing else.* Three parts, each closing a way of being wrong:

- **Exactly one text element.** Both wire shapes decode to one, so this is shape-agnostic; a line
  carrying prose *and* a wrapper is two and is not matched.
- **`strip_prefix` and `strip_suffix`, never `contains`.** A human asking *"what does
  `<local-command-stdout>` mean?"* fails the prefix test, so the tag stays safe to quote, discuss
  or paste inside a larger message.
- **`isReplay` is deliberately not tested at all.** It is `true` here *and* on every genuine human
  turn — the replay is how a human turn reaches the transcript in the first place — so requiring
  it would exclude nothing real while letting a future unflagged narration straight through.

⚠️ The residual false positive is a human whose **entire** message is a verbatim
`<local-command-stdout>…</local-command-stdout>` pair. That is the price of there being no flag,
and it is the cheaper failure by a wide margin: a stray line is visible and reportable, a
swallowed sentence is not. It is also *watchable* — `MapStats::local_commands_suppressed` counts
every suppression separately from `unmapped`, precisely so that a number climbing while somebody
is typing is how a predicate that started eating real turns would be caught.

⚠️ **This is a rendering bug that ships with the feature unless it is caught first, and rule 2 is
exactly why it slips through.** §5.9.3 rule 2 is *"the composer writes to stdin and renders
nothing; the transcript renders only what returns."* That rule is right, and it is what makes the
transcript's ordering free rather than a splice-and-hope. But it also means the transcript trusts
the stream completely — so a `user`-role line the console never sent becomes a human turn with no
further check. The feature works; the conversation acquires a sentence the human did not say.

📌 The silver lining: this line is the **only** place the CLI states the *resolved* model
(`sonnet` → `claude-sonnet-5`). The `set_model` ack carries no `response` body at all. If the strip
wants to confirm what it got rather than what it asked for, this is one of the two sources — the
other is §4's repeat init.

---

## 3. Where a model LIST comes from — there is one, and it is good

**MEASURED.** The `initialize` control_request — sent as an ordinary control_request on the same
stdin, *after* everything else in the session, with no arguments:

```json
{"type":"control_request","request_id":"req-init-1","request":{"subtype":"initialize"}}
```

The reply nests one level deeper than a bare read of it suggests, and the nesting is worth stating
because a consumer that reaches for `response.models` finds nothing. The **outer** `response`
carries:

```
subtype, request_id, response, pending_permission_requests, pending_user_dialog_requests
```

and the **inner** `response` is the payload:

🚨 **Correction: the double nesting is the CONTROL PROTOCOL's, not `initialize`'s.** This warning
was written inside the `initialize` section and read as a quirk of that one heavy verb. It is not.
`set_permission_mode` has exactly the same envelope — §8's quoted ack is
`{"response":{"subtype":"success","request_id":…,"response":{"mode":"acceptEdits"}}}`, and its
`mode` lives at `response.response.mode` for the same reason `models` lives at
`response.response.models`. The rule is per *verb*, and it is about presence rather than depth:

| Verb | Outer envelope | Inner payload |
|---|---|---|
| `set_model` | `subtype`, `request_id` | 🚨 **none at all** — a bare ack. This is what forces the console to keep showing the *confirmed* model and mark the destination separately (§2) |
| `set_permission_mode` | same | `{"mode":"acceptEdits"}` — the verb states its own result |
| `initialize` | same, **plus** `pending_permission_requests` / `pending_user_dialog_requests` | the 23 KB object holding `models` |

So a consumer needs the two nestings **named apart**, not special-cased for one verb.
`agent_event.rs::ControlResponse` does it with `body` (the envelope, kept whole so an unread field
is not lost) and `payload()` (the inner object, `None` for a bare ack and for every error), with
`mode()` and `models()` reading through the latter.

```
commands, agents, output_style, available_output_styles, models, account, pid,
current_permission_mode, remote_control_auto_enable, remote_control_auto_on_by_default,
ide_rc_auto_enable_gate, fast_mode_state, fast_mode_disabled_reason
```

`models` is exactly the menu the plate needs. One row, verbatim:

```json
{"value":"default","resolvedModel":"claude-opus-5[1m]","displayName":"Default (recommended)",
 "description":"Opus 5 with 1M context · Best for everyday, complex tasks","supportsEffort":true,
 "supportedEffortLevels":["low","medium","high","xhigh","max"],"supportsAdaptiveThinking":true,
 "supportsFastMode":true,"supportsAutoMode":true}
```

and the five rows this account was offered:

| `value` | `resolvedModel` | `displayName` |
|---|---|---|
| `default` | `claude-opus-5[1m]` | Default (recommended) |
| `opus[1m]` | `claude-opus-5[1m]` | Opus (1M context) |
| `claude-fable-5[1m]` | `claude-fable-5` | Fable |
| `sonnet` | `claude-sonnet-5` | Sonnet |
| `haiku` | `claude-haiku-4-5-20251001` | Haiku |

⚠️ Effort support is **per row, not universal**: the first four carry `supportsEffort: true` with
`supportedEffortLevels`, and the `haiku` row carries none of those keys at all. A plate that reads
them must treat them as optional.

🚨 **So the console does NOT need a hardcoded model list.** The list is per-account, it carries
display names and descriptions written for humans, and `resolvedModel` exists precisely so a host
can match a persisted explicit id back to the alias row that covers it — the schema says so:

> `resolvedModel: … "Canonical wire model id this row's 'value' resolves to (e.g. 'sonnet' → 'claude-sonnet-5'). Lets hosts match a persisted explicit id against the alias row that covers it."` **(INFERRED — schema string)**

⚠️ **`unavailable_models` is INFERRED, not measured, and the console will not receive it.** The
key does **not appear in the capture** — it is absent from the inner payload above and the string
`unavailable_models` occurs zero times in `p1.out.jsonl`. It exists in the binary's schema, and
that schema is explicit about who gets it:

```
unavailable_models: … "@internal Models the account can see but not select (disabled: true,
reason folded into description — e.g. a model the org's Zero Data Retention setting excludes).
Disjoint from `models`, which stays selectable-only so consumers without disabled rendering are
unaffected. Populated only for allowlisted 1P hosts that render these rows (currently the VS Code
extension — UNAVAILABLE_MODELS_HOST_ENTRYPOINTS); empty for every other consumer. Omitted when
empty."
```

Read plainly: `models` is already selectable-only, and a third-party host is *expected* to see
nothing else. Design the plate against `models` alone and treat `unavailable_models` as absent.

### 3a. ⚠️ `pending_permission_requests` and `pending_user_dialog_requests` have NO measured element shape

**Both keys are on the envelope in the capture and both were EMPTY.** Nothing here records what one
of their elements looks like, because no session in this pass had an approval or a dialog
outstanding when `initialize` was asked — the request was sent last, after every turn had finished.
So what is established is exactly two things: the keys exist, and they are arrays.

🚨 **A consumer needs a fresh measurement, not a guess.** The obvious guess — that an element is
shaped like the `can_use_tool` control_request of §11 — is plausible and completely unverified, and
these are the two lists through which a **held approval** would reach a console that reconnects to
a session already asking for one. Guessing wrong there means either dropping a question the agent
is blocked on or drawing a card for something that was never asked.

**What is implemented, and it deliberately decides nothing:**
`ControlResponse::pending_permission_requests()` / `::pending_user_dialog_requests()` return
`&[serde_json::Value]` — the raw elements, unparsed and un-modelled. Nothing in the console reads
them today. The probe that would settle it is small and needs staging rather than cleverness: raise
a permission prompt, leave it unanswered, and send `initialize` while it is outstanding.

⚠️ **Two more caveats before treating `initialize` as free.** It is a **heavy** response — it also
returns every slash command with its full description, and every agent; the captured line was
**23 824 bytes on one line**. And sending it mid-session was measured **safe here** (275 ms,
`subtype:"success"`, nothing reset, the session then closed cleanly) but was deliberately sent
**last**, so "safe to send at any point" is **not** established. If the strip wants the list, the
honest place to ask is **once, at spawn**, before the first turn.

📌 There is a second, cheaper source: `/model` with no argument (§5) prints
`Available: sonnet, opus, haiku, fable, best, sonnet[1m], opus[1m], fable[1m], opusplan, default, or a full model ID.`
That is a *string* to parse and it carries no display names, no `resolvedModel`, and no effort
data. Use `initialize`.

---

## 4. 🚨 The trap: the new model arrives only in a **repeat `system/init`**, which the console drops

This is the one most likely to make the feature look broken.

**MEASURED.** Every `system/init` in the `p1` session, in order:

```
init  line=1   model=claude-opus-5[1m]  permissionMode=default      tools=33   mcp=0
   … set_model sonnet + set_permission_mode acceptEdits …
init  line=19  model=claude-sonnet-5    permissionMode=acceptEdits  tools=128  mcp=4
init  line=32  model=claude-sonnet-5    permissionMode=acceptEdits  tools=128  mcp=4
```

Same `session_id` throughout (`124488fd-61ae-4041-b185-b5c18f3de4f0`). The **field set is
identical** across all three (22 keys each) — a repeat init is not a partial init, it is a
complete restatement of session identity with the new values in it.

**And the console currently keeps the first one forever.** `agent_map.rs` is explicit about it:
`record_init` is documented *"The caller guarantees this is the **first** init; a later one
returns before reaching here, so identity cannot be overwritten mid-stream"*; the counter
`repeat_session_starts` exists to count what is thrown away; and two tests pin the behaviour by
name — `a_live_session_maps_two_turns_and_ignores_the_second_init` and
`a_second_init_does_not_overwrite_the_first`.

🚨 **So today: click the model plate, the model genuinely changes, and the plate keeps saying
`claude-opus-5[1m]` until the tab is closed.** The strip would be lying about the one fact it
exists to report.

⚠️ **Do not "fix" this by adopting the whole later init.** Note `tools=33 → 128` and `mcp=0 → 4`
between init 1 and init 2 **in a session where nothing was asked to change about tools**. MCP tools
arrive **deferred** (the approval protocol already records this), so an init also recurs simply
because more tools finished loading. The `p3` session shows the same growth with **no model change
at all** — `tools=102` at line 2, then `tools=131` at lines 33 and 63, model `claude-opus-5[1m]`
throughout. **An init is not a change notification.** It is a restatement, and it recurs for
reasons that have nothing to do with the strip.

### 4a. The mode has a narrow signal; the model does not

**MEASURED** (`p1.out.jsonl` line 18). A permission-mode change also emits a dedicated, tiny
event:

```json
{"type":"system","subtype":"status","status":null,"permissionMode":"acceptEdits",
 "uuid":"b1368c81-…","session_id":"124488fd-…"}
```

Note `"status":null` — this is the same `system/status` shape whose other observed value is
`{"status":"requesting"}` (seen at lines 2, 20 and elsewhere), with `permissionMode` present only
on the mode-change instance.

📌 **The MODE plate has a clean event source and the MODEL plate does not, and that asymmetry is a
design input rather than an accident to paper over.** For permission mode the console has a cheap,
unambiguous subscription. For the model it has none: the model appears only in a repeat
`system/init`, in the next `assistant` message's `message.model`, and in the
`<local-command-stdout>` narration of §2b. They are not symmetric and should not be implemented as
though they were.

### 4b. ✅ RESOLVED — splitting the init handling

✅ **James ruled, the ruling is written into the spec this section proposed amending
(`doc/console_spike_execution_plan.md` §5.9.3 rule 3), and it is implemented.** The proposal below
is kept verbatim because it is the argument the ruling was made on — but it is no longer open, and
this heading used to say it was.

🚨 **A document asserting that a settled question is open is worse than one that never raised it.**
A reader who trusts it either re-opens a decision that was made or, worse, implements the
unamended rule and produces the exact failure §4 describes. That is why this is corrected in place
rather than annotated at the bottom.

**What was decided**, in one line: `model` and `permission_mode` are **latest-init-wins**; `cwd`,
`cli_version`, `tools` and `mcp_servers` stay **first-init-wins**. That answers both of the two
questions this section listed as genuinely open —

- **`tools` and `mcp_servers` stay first-init-wins.** The deferred-loading growth *is* the
  different case: 33 → 128 and 0 → 4 across two inits of one session with nothing asked to change,
  and 102 → 131 in another with no model change at all. Adopting the later figure would make the
  count *look* live while actually reporting load progress.
- **The tests were re-scoped, not deleted.** `a_second_init_does_not_overwrite_the_first` is now
  `a_second_init_does_not_overwrite_the_sessions_identity` and asserts the same thing about the
  fields that did not move; `repeat_session_starts` still counts rather than discards, exactly as
  this section predicted would survive either answer.

Implemented as `SessionFacts::record_init` (identity, first only) and
`SessionFacts::record_repeat_init` (two fields, and an empty string is absence rather than a
change, so a later init reporting no model leaves the plate alone).

---

**The proposal, as it was written.** **§5.9.3 rule 3 is not wrong — it is under-specified.** It
reads, verbatim:

> 🚨 **`system/init` recurs mid-stream** — a second one arrived before turn two of the live
> session, same `session_id`, different field count. Only the first establishes identity; a later
> one must not reset or re-initialise the transcript.

It was written from a capture where the second init carried no change worth adopting. Nothing in
that measurement says identity *fields* may not be refreshed, and the two halves are separable.

**The proposal:** keep first-init-wins for **transcript identity** (session id, cwd, the "this is
where the conversation began" anchor), and make `model` and `permissionMode` **latest-wins**.

🚨 **This amends a settled spec** (`doc/console_spike_execution_plan.md` §5.9.3 rule 3), which is
James's call and not this document's. Two things about it are genuinely open, beyond the
amendment itself — ✏️ *both answered above*:

- whether `tools` and `mcp_servers` should also be latest-wins, or whether the deferred-loading
  growth above makes them a different case;
- whether the two named tests are re-scoped or joined by a third.

Both of the existing tests assert on `repeat_session_starts`, which counts rather than discards —
so the counter survives either answer.

---

## 5. `/model` as literal user text — interpreted, and free

**MEASURED.** Injecting `{"type":"user",…,"text":"/model"}` on stdin, in `stream-json` input mode,
is interpreted as a **command**. It is expanded locally, never reaches a model, and answers as a
synthetic assistant message:

```json
{"type":"assistant","message":{"id":"ffe130b1-…","model":"<synthetic>","role":"assistant",
 "stop_reason":"stop_sequence","usage":{"input_tokens":0,"output_tokens":0,…},
 "content":[{"type":"text","text":"Current model: Sonnet 5 (session override from plan mode)\nBase model: Opus 5 (1M context) (default)\nUsage: /model <name>. Available: sonnet, opus, haiku, fable, best, sonnet[1m], opus[1m], fable[1m], opusplan, default, or a full model ID."}]}}
```

followed by a `result` line with `{"is_error":false,"duration_api_ms":0,"num_turns":0,…}` — and
`total_cost_usd` was **unchanged** from the previous turn (`0.7170369999999999` both times).
**A slash command over stdin costs nothing and took ~290 ms** (`duration_ms: 17` at the CLI, ~290
ms wall from the timeline).

**So slash commands work over the console's pipe.** But `set_model` is strictly better as a
*control*: it returns a machine-readable ack against a `request_id` the console chose, whereas
`/model sonnet` returns prose to parse and a `<local-command-stdout>` user bubble to hide. Use the
control protocol to act; `/model` remains useful as evidence that the CLI's own local command
surface is reachable from a conversation tab — which is its own finding.

⚠️ Note the parenthetical in the capture: `Current model: Sonnet 5 (session override from plan
mode)`. **`set_model` is classified internally as a session-scoped override**, and the phrase
"from plan mode" appears to be a mislabel in the CLI's own reporting — nothing in this session
ever entered plan mode. Don't surface that string to a user.

---

## 6. Respawn and resume — measured, with one number deliberately not reported

The control protocol made the respawn path unnecessary, but "is resume cheap?" governs every
future recovery decision, so it was measured anyway. **Zero model tokens were spent**: each spawn
was fed a single `/model` (§5) purely to give the CLI something to process.

⚠️ **A necessary aside that cost a run:** `system/init` is **not** emitted on spawn. Three sessions
were started and left idle for 90 s each and produced zero bytes on stdout and stderr; the init
arrives only once the first input line is pending. **A console that waits for `system/init` before
considering a tab "ready" will wait forever on a tab nobody has typed into.** (That idle run was
superseded by the driver that replaced it, so no capture file for it survives — the retained
`probe2.ps1` is the version that sends `/model` first. The three retained `p2.*` captures are
consistent with it: in every one, the init follows the input.)

**MEASURED**, spawn → first `system/init`:

| spawn | `session_id` | `model` | `permissionMode` | tools | mcp |
|---|---|---|---|---|---|
| fresh | `b93c4255-…` (new) | `claude-opus-5[1m]` | `default` | 99 | 4 |
| `--resume 124488fd-…` | `124488fd-…` (**same**) | `claude-sonnet-5` | `default` | 33 | 0 |
| `--resume 124488fd-… --model haiku` | `124488fd-…` (**same**) | `claude-haiku-4-5-20251001` | `default` | 128 | 4 |

Four things follow:

1. **`--resume` does not change the session id.** Without `--fork-session` the id is preserved, so
   a resumed tab keeps its identity. (`--fork-session` exists — **INFERRED**, `--help`: *"When
   resuming, create a new session ID instead of reusing the original (use with --resume or
   --continue)"* — and was not exercised.)
2. **`--resume` + `--model` works and wins.** The resumed session came back on `haiku`.
3. **A mid-session model override survives resume.** The plain resume came back on
   `claude-sonnet-5`, the value `set_model` had installed in the previous process. The override is
   persisted with the session, not with the process.
4. 🚨 **The permission mode does NOT survive resume.** All three came back `default`, including the
   resume of the session that had been left in `acceptEdits`. Model and mode persist differently,
   and a console that restores a tab must not assume the mode plate's last value is still true.

### ✅ The resume-cost number, declined

**Resume cost is deliberately not reported here.** Spawn → first `system/init` came out
**3258 / 1294 / 2686 ms** for the three rows above, and those numbers do not support the
conclusion anyone would draw from them.

The tool counts in the table (99 / 33 / 128) show these runs loaded wildly different tool sets, and
MCP and skill warmup dominates the interval. **The variance between two resumes of the same
session (1294 vs 2686 ms) is larger than the gap between fresh and resume.** Transcript re-read
was never isolated and is not measurable this way. All that can honestly be said is that the three
sit in a **1.3–3.3 s band**.

(The three millisecond figures were printed by `probe2.ps1` to its own stdout and are not in a
retained capture file, which is a second reason not to build on them. The `session_id`, `model`,
`permissionMode` and tool counts in the table above *are* in the retained `p2.*.jsonl`.)

📌 Recorded rather than dropped because **a measurement declined for a stated reason is worth more
than a number nobody can trust** — and because the next person to wonder "is resume cheap?" should
find out that the question needs a better instrument, not re-derive three untrustworthy numbers.

---

## 7. Permission modes, exactly as spelled on the wire

**MEASURED**, wire values seen live in `system/init.permissionMode` and
`system/status.permissionMode`: `default`, `acceptEdits`, `dontAsk`. `bypassPermissions` was sent
and rejected (§9), which confirms its spelling on the request side.

**INFERRED** — the authoritative enum, from the binary's own schema string:

```
Dr(["default","acceptEdits","bypassPermissions","plan","dontAsk","auto"])
  "Permission mode for controlling how tool executions are handled.
   'default' - Standard behavior, prompts for dangerous operations.
   'acceptEdits' - Auto-accept file edit operations.
   'bypassPermissions' - Bypass all permission checks (requires allowDangerouslySkipPermissions).
   'plan' - Planning mode, no actual tool execution.
   'dontAsk' - Don't prompt for permissions, deny if not pre-approved.
   'auto' - Use a model classifier to approve/deny permission prompts."
```

🚨 **The flag and the wire disagree, and a strip must not confuse them.** `--help` gives
`--permission-mode` the choices
`"acceptEdits", "auto", "bypassPermissions", "manual", "dontAsk", "plan"` — **`manual` instead of
`default`**. The wire enum has `default` and no `manual`. So `manual` is the flag-side spelling of
the wire's `default`. The fixture `native/organon-shell/fixtures/claude_stream_live_session.jsonl`
shows `"permissionMode":"default"`, and that is the one the strip reads and the one
`set_permission_mode` takes.

### 7a. `--permission-mode` as a spawn flag

**INFERRED** (`--help`, quoted verbatim):

```
--permission-mode <mode>    Permission mode to use for the session (choices:
                            "acceptEdits", "auto", "bypassPermissions", "manual",
                            "dontAsk", "plan")
```

Not exercised — the control protocol made it unnecessary.

---

## 8. `set_permission_mode` — measured working, with a confirming ack

**MEASURED** (`p1`, +4732 → +4749 ms):

```json
-> {"type":"control_request","request_id":"req-perm-1","request":{"subtype":"set_permission_mode","mode":"acceptEdits"}}
<- {"type":"control_response","response":{"subtype":"success","request_id":"req-perm-1","response":{"mode":"acceptEdits"}}}
<- {"type":"system","subtype":"status","status":null,"permissionMode":"acceptEdits","uuid":"b1368c81-…","session_id":"124488fd-…"}
```

**17 ms round trip.** Unlike `set_model`, the ack carries a **`response` body naming the resulting
mode** — so the strip can confirm rather than assume. The `system/status` line follows
independently, which is what a second console pane (or a later reader) would key off. The same
pair was captured twice more, in two other sessions and with two other modes (`m-accept` in `p3`,
`m-dontask` in `p4`), with the same shape both times.

---

## 9. 🚨 `bypassPermissions` is **unreachable** from a console session — and that is a gift

**MEASURED** (`p3.out.jsonl` line 62). On a session spawned as the console spawns one:

```json
{"type":"control_response","response":{"subtype":"error","request_id":"m-bypass",
 "error":"Cannot set permission mode to bypassPermissions because the session was not launched with --dangerously-skip-permissions"}}
```

⚠️ **That sentence is quoted evidence — the CLI's own refusal text — and the flag name inside it is
part of the quote, not a suggestion.** It is reproduced because the console should be able to show
the user *why* a mode is unavailable in the CLI's own words. Nothing in this document recommends
launching with it.

The session stayed in `acceptEdits`; the write it was asked for still raised a prompt at the
handler, and the following `system/init` still read `permissionMode=acceptEdits`.

**So the worst version of the fear cannot happen.** The console is the approval authority because
it passes `--permission-prompt-tool`; it does not pass the flag named in the refusal above;
therefore **no control on the strip can put the session into the one mode that would blanket-
disable every check.** The refusal is at the CLI, not at our discretion, and it is loud.

📌 Design consequence, and it is cheap: **a mode picker should render `bypassPermissions` as
present-but-unavailable rather than hiding it**, with the CLI's own sentence as the reason. A mode
that silently isn't in the list looks like a console bug; a greyed one that explains itself does
not.

---

## 10. 🚨🚨 `dontAsk` **silently disarms the console**

**MEASURED**, and it is the finding that should govern the design of the mode control.

⚠️ **Method, stated precisely because the original write-up ran two sessions together.** This was
the `p4` session — a **fresh spawn**, not a continuation of §9's or §11's session (`p4` is
`74017cdb-…`; `p3` is `d4ca25ac-…`). Both used `--permission-prompt-tool stdio` (see §12) so every
permission prompt is observable on the stream itself. ⚠️ `p4`'s argv also **omitted
`--include-partial-messages`**, which the console does pass; that flag governs token-level deltas
and nothing in the permission path, but it is a difference from `ARGS` and is recorded rather than
glossed.

Under `default` and under `acceptEdits` (§11, the `p3` session), a `Write` to a path outside the
working directory produced a permission prompt that reached the handler. In `p4`:

```
-> {"type":"control_request","request_id":"m-dontask","request":{"subtype":"set_permission_mode","mode":"dontAsk"}}
<- {"type":"control_response","response":{"subtype":"success","request_id":"m-dontask","response":{"mode":"dontAsk"}}}
<- {"type":"system","subtype":"status","status":null,"permissionMode":"dontAsk",…}
-> {"type":"user",…"Use the Write tool once to create the file C:\Users\james\AppData\Local\Temp\organon-b9\b9_dontask.txt …"}
```

**Zero permission prompts reached the handler.** Instead:

```json
{"type":"system","subtype":"permission_denied","tool_name":"Write",
 "tool_use_id":"toolu_01QAH8z5Ut17RpddeLCDD1mj","decision_reason_type":"mode",
 "message":"Permission to use Write has been denied because Claude Code is running in don't ask mode. …"}
```

and the same text came back to the model as an ordinary failed `tool_result` (`"is_error":true`).
The file was not created (`b9_dontask.txt exists: False`).

**`"decision_reason_type":"mode"` is the console's tell.** The decision was made by the mode,
upstream of the handler — the same shape as `--allowedTools` short-circuiting the handler in
approval-protocol §6, except this one denies instead of allowing, and unlike `--allowedTools` it is
**reachable from a control the strip would offer**.

🚨 **The danger is real and inverted from the obvious guess.** The worry going in was that a
*permissive* mode would make the console approve things without asking. The measured hazard is the
*restrictive* one: put a session in `dontAsk` from the strip and **every tool that would have
raised a card is silently refused instead**, while the console still passes
`--permission-prompt-tool`, still holds the handler, and still looks like the authority. Every
approval card becomes a silent refusal. The user's experience is "the agent suddenly can't do
anything and nobody asked me why."

📌 One rendering consequence is not in doubt, whatever is decided about the control itself: the
console should render `system/permission_denied` carrying `decision_reason_type: "mode"` as **its
own thing**, not as a generic red tool error. It is the console's own setting talking back, and it
is the only way the human learns which of their clicks caused it.

⚠️ ⏸ **A useful corroboration, and its limit.** `p4` also captured a `system/post_turn_summary`
naming the situation in plain language — `"status_category":"blocked"`,
`"status_detail":"Write tool denied; session in 'don't ask' mode"`,
`"needs_action":"approve Write tool or request alternative"`. That is excellent card copy sitting
right there on the stream. It is also held for milestone 2 by §5.9.3's own "held" list, so it is
noted, not proposed.

### 10a. ANSWERED IN THE BUILD, and not by the axis this proposed — the mode-safety policy

✏️ **The control is implemented, so the questions below are answered in the sense that something
had to be built.** ⚠️ **That is weaker than §4b's status and is labelled differently on purpose:**
§4b was ruled on and the ruling is written into the spec it amends; this was settled by a design
decision inside an implementation, against James's brief *"we need to make what it does
unmistakable for the don't ask policy."* Nothing here is a written ruling, and a later one may
overturn it.

**What the build does, against the three open questions in order:**

- **`dontAsk` is offered, with its consequence as its label rather than as a warning.** The picker
  has exactly three rows and each is titled by *what happens*, not by the mode's name — `dontAsk`
  reads *"no approval cards at all — anything needing permission is refused, and the console is
  never asked."* The sentence is the label, not a tooltip: a hover puts the one thing that matters
  behind a gesture nobody makes while deciding.
- **An unmeasured mode is withheld from the picker and still shown when it arrives.** `plan` and
  `auto` are not offered — the control that governs authority is the wrong place to guess — and
  `bypassPermissions` is not offered because §9's refusal would make it a dead button. But a mode
  arriving from *outside* the picker (a session spawned with `--permission-mode`) is reported on
  the plate and marked like any other. **The shortlist governs what can be chosen, never what can
  be shown**, and an unrecognised mode gets a marker precisely because it is the case where "the
  console may not be the one being asked" cannot be ruled out.
- 🚨 **The third question — whether "preserves the console's authority" is even the right axis —
  is answered NO, and that is the substantive change.** This section proposed *removing* modes
  that take the console's authority away. The build instead **keeps the choice and makes the
  consequence impossible to miss**: whenever the reported mode is not `default`, a marker sits on
  the band for as long as that stays true. Not a confirmation dialog — a dialog clicked through at
  the moment of choosing is exactly the warning people stop reading, and the hazard is not that
  moment, it is the hours afterwards when the band still looks like the authority. ⚠️ It is amber
  and not red for the same reason: this band is looked at constantly, and a permanent klaxon is
  one the eye learns to skip, which would leave the console back where it started.

📌 The marker is *derived* (`conversation_view::strip_content` builds it from the reported mode
every frame) rather than raised as an event, so it cannot get stuck on, cannot get stuck off, and
cannot be dismissed. And §10's rendering consequence — `system/permission_denied` with
`decision_reason_type: "mode"` drawn as its own thing rather than as a red tool error — is **not
built**, and remains the honest gap in this: the band says the mode is silencing approvals, and
the individual refusal it causes still arrives looking like an ordinary tool failure.

---

**The proposal, as it was written.** The mode strip offers only modes that preserve the console's
authority — i.e.
modes under which a gated tool still reaches the console's handler and still produces a card.
`dontAsk` does not qualify on the measurement above; `default` and `acceptEdits` did qualify for
the one gate reason tested (§11); `bypassPermissions` cannot be selected at all (§9); `plan` and
`auto` are unmeasured, see immediately below.

**What is genuinely open**, and why this is not a decision to make inside an implementation PR:

- whether `dontAsk` is **removed** from the picker, or **offered with a consequence statement** —
  in the strip and in the transcript — saying that approvals are now refused automatically and the
  console will no longer ask;
- whether an unmeasured mode is offered by default or withheld until measured;
- whether "preserves the console's authority" is even the right axis, given that a mode the user
  genuinely wants is not made safer by being unreachable from the interface that owns the session.

⏸ **`plan` and `auto` were not probed.** Both are plausibly in the same class as `dontAsk` — a mode
that decides upstream of the handler. **`auto` is the one to measure next**, because a *classifier*
answering in the console's place is the same authority problem wearing a friendlier face: the
console would still hold the handler, still show the strip, and still never be asked. The probe is
exactly the one in this section with `mode` changed, and it costs one session and one turn.

---

## 11. `acceptEdits` did **not** short-circuit the handler — with an important qualification

**MEASURED** (`p3`, session `d4ca25ac-…`). Under `default`, a `Write` outside the working directory
raised a prompt at the handler:

```json
{"type":"control_request","request_id":"63852e1b-…","request":{"subtype":"can_use_tool",
 "tool_name":"Write","display_name":"Write",
 "input":{"file_path":"C:\\Users\\james\\AppData\\Local\\Temp\\organon-b9\\b9_default.txt","content":"HELLO"},
 "description":"~\\AppData\\Local\\Temp\\organon-b9\\b9_default.txt",
 "permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"},
                           {"type":"addDirectories","directories":["C:\\Users\\james\\AppData\\Local\\Temp\\organon-b9"],"destination":"session"}],
 "decision_reason":"Path is outside allowed working directories","decision_reason_type":"workingDir",
 "tool_use_id":"toolu_014j4iw5vgDoCt5rpEzKyf43"}}
```

After switching to `acceptEdits`, the **same shape of write raised the prompt again** — the handler
was consulted, answered `allow`, and the file was written.

⚠️ **Qualify this correctly: it was measured against exactly one gate reason.** It does **not**
show that `acceptEdits` never short-circuits. It shows that `acceptEdits` does not cover a
**`workingDir`** gate — and the CLI itself says as much in the capture. Under `default` the prompt
offered `{"type":"setMode","mode":"acceptEdits"}` as a suggestion; once the session *was* in
`acceptEdits`, **that suggestion was gone**, leaving only `addDirectories`. The CLI knew the mode
would not help for this reason.

**So the honest general statement is: a permission mode short-circuits the handler exactly for the
gates it covers, and the gate that fires is named on the wire.** Every prompt carries
`decision_reason` and `decision_reason_type` (`workingDir` here, `mode` in §10) — which is both the
diagnostic and, incidentally, excellent card copy.

**NOT MEASURED, and the probe that would settle it:** whether `acceptEdits` suppresses a prompt
whose `decision_reason_type` is **edit**-related rather than `workingDir` — which is the case the
mode is actually named for. ⚠️ It needs a cwd where a plain `Edit`/`Write` prompts under
`default`, and that is awkward to stage: **approval-protocol §8's second trap is that writes
inside the session's own scratchpad are pre-blessed and never prompt**, so the naive setup
produces a silent pass that looks like a result. One session, two turns, in a throwaway git repo:
write inside cwd under `default`, capture the `decision_reason_type`, switch to `acceptEdits`,
repeat.

---

## 12. An aside that is too useful to bury: `--permission-prompt-tool stdio`

**MEASURED.** `--permission-prompt-tool` accepts the literal value **`stdio`**, and it routes every
permission prompt as a `can_use_tool` **`control_request` on the session's own stdout** — answered
with a `control_response` on stdin. **No MCP server, no port, no config file, no second process.**
That is how §10 and §11 were measured.

The answer shape is the approval protocol's `behavior`/`updatedInput` contract wrapped in a
control_response:

```json
{"type":"control_response","response":{"subtype":"success","request_id":"63852e1b-…",
 "response":{"behavior":"allow","updatedInput":{"file_path":"…","content":"HELLO"}}}}
```

**INFERRED** (binary string) — it is mutually exclusive with a named MCP tool: *"canUseTool
callback cannot be used with permissionPromptToolName. Please use one or the other."* Both flow
through one `permissionPromptToolName` slot (`YKi(Y?"stdio":t.permissionPromptTool)`), which is the
reason §10's and §11's results should transfer to the console's MCP handler: the *transport*
differs, the decision pipeline that chooses whether to consult it does not.

📌 It is also what the first-party desktop client uses: `--permission-prompt-tool stdio` appears in
the live argv of the Claude Code process the desktop app runs on this machine (**MEASURED** — a
process listing; note that copy is version 2.1.227, a separately bundled binary).

### The comparison this deserves — and what it is not

⚠️ **This is not a recommendation to switch, and it is emphatically not a recommendation to remove
the MCP server.** State the tradeoff honestly:

| | `--permission-prompt-tool stdio` | in-process MCP over HTTP (approval protocol §8) |
|---|---|---|
| setup | none — a flag | ephemeral port, `--mcp-config` file, `--strict-mcp-config`, an HTTP server |
| permission prompts | `can_use_tool` on the session's own stdout | `tools/call` at the server |
| the console's **capability tools** | **cannot be served at all** | served on the same connection |
| 60 s deadline / progress extension | not measured on this path | measured, and answered by SSE progress |

🚨 **The decisive asymmetry is the third row.** The approval protocol chose HTTP/MCP for two
reasons, and only one of them is about permissions: serving in-process so the handler is a direct
call into UI state, **and** carrying the console's `organon console …` verbs as named tools so an
approval card can name a *capability* rather than a shell command. `stdio` answers permission
prompts and nothing else — so adopting it would mean either giving up the capability tools or
running the MCP server anyway, at which point the saving is gone.

What is genuinely established is that **a cheaper path exists**, that it needs no port and no
config file, and that it is what the first-party client uses. That deserves its own comparison
**before the next tier hardens more wiring onto the HTTP path** — a decision made once, deliberately,
rather than by accretion. It also means **anyone measuring permission behaviour in future can do it
in twenty lines instead of building a server**, which is the immediate value and is why this
section exists.

---

## What the measurements settle

These follow from the captures and are not proposals:

1. **Both plates can become controls over the control protocol.** No respawn, no session-continuity
   problem, no resume. `set_model` and `set_permission_mode`, on the pipe the console already
   holds, with no `initialize` handshake — and, per §1, before the session's first `system/init`.
2. **A model list is available from the CLI**, per-account and with display names, from one
   `initialize` control_request (§3). No hardcoded table is required. Ask once at spawn: mid-session
   safety was measured only for a request sent last.
3. **The mode plate has a clean event source (`system/status`); the model plate does not** (§4a).
   Any implementation of the two will be asymmetric.
4. **`bypassPermissions` cannot be selected from a console session** (§9), and the refusal is the
   CLI's, not ours.
5. **`dontAsk` removes the console's approval cards while leaving it looking like the authority**
   (§10).
6. **A `<local-command-stdout>` user message lands on the stream on every model switch** and will
   render as a fake human turn unless it is filtered or specially rendered (§2b). ✏️ It is now
   filtered, on the wrapper alone — **not** on the `content` shape, which §2b's correction explains
   is not usable — and every suppression is counted so the predicate is watchable.

## Open, and worth one run each when it matters

- **`auto` mode** — the classifier mode is the remaining unmeasured way the console could stop
  being the authority while continuing to look like it. A model deciding in the console's place is
  the same problem as §10 with a friendlier face. One session, one turn — the §10 probe verbatim,
  with `mode` changed.
- **`plan` mode**, same probe, same cost.
- **Whether `acceptEdits` suppresses an edit-reason prompt** (§11 names the exact staging, and the
  pre-blessed-scratchpad trap that makes it awkward).
- **Whether `initialize` is safe to send at an arbitrary point**, or only pre-first-turn. It was
  measured safe when sent **last**; nothing establishes the general case.
- ⚠️ **Whether `initialize` in particular is answered before a session's first `system/init`** —
  which is what the build now does at spawn (§1). The verb measured landing that early was
  `set_permission_mode`; `initialize` is the heaviest of the twelve and was never sent first. The
  20 s deadline is what makes a wrong guess here degrade to an **empty model picker** rather than
  to a stuck tab, so this is a measurement worth taking and not a risk worth holding the feature
  for.
- ⚠️ **The element shape of `pending_permission_requests` / `pending_user_dialog_requests`** (§3a).
  Both were empty in every capture; both are the path a *held* approval would arrive by. Staging,
  not cleverness: leave a permission prompt outstanding and send `initialize` while it is.
- **Rendering `system/permission_denied` with `decision_reason_type: "mode"` as its own thing**
  (§10's closing note). The band now says the mode is silencing approvals; the individual refusal
  it causes still looks like an ordinary red tool error.
- **`interrupt`** — in the accepted subtype set and advertised in `init.capabilities`, never
  exercised, and the console has no other way to stop a running turn.
- **`--fork-session`**, for the case where a tab should branch rather than continue.
- **`--permission-prompt-tool stdio` versus the in-process MCP server** as the console's permanent
  approval transport (§12) — the comparison, not the switch.
✏️ **Removed from this list**: the two proposals (§4b, §10a), which were decisions rather than
measurements and have both been taken — §4b by a ruling written into the spec, §10a by a design
decision in the build. Each section carries which, and how strong a claim that is.

---

### Provenance

Eight `claude.exe` invocations on 2026-08-12: six headless `stream-json` sessions plus `--version`
and `--help`. Captures and driver scripts sit in the measuring session's scratchpad as
`probe1.ps1` … `probe4.ps1` and `p1.*` … `p4.*` — `p1` is §§1–5, `p2` the three resume spawns of
§6, `p3` §§9/11, `p4` §10. Binary strings were read with `grep -a` against
`C:\Users\james\.local\bin\claude.exe`; every such quote is marked **INFERRED**. Claims about
console code (`agent_session.rs::ARGS`, `agent_map.rs::record_init`, `agent_event.rs::content_blocks`,
`fixtures/claude_stream_live_session.jsonl`, `console_spike_execution_plan.md` §5.9.3) were
re-checked against the tree at capture time. No `claude.exe` run created or modified a repo file.
