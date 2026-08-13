# Fixtures — Claude Code's `stream-json` output

What `src/agent_event.rs` is tested against. Three of these are real; one is not, and
the difference is the point.

| File | Provenance |
|---|---|
| `claude_stream_two_tools.jsonl` | **Captured.** `claude.exe` 2.1.228, one prompt that read two files, `--output-format stream-json --include-partial-messages --verbose`. |
| `claude_stream_live_session.jsonl` | **Captured.** The same CLI in a persistent session — `-p --input-format stream-json --output-format stream-json --replay-user-messages` — with two human turns written to stdin 25 seconds apart. |
| `claude_stream_subagent.jsonl` | **Captured.** The same CLI and the same argv, given a prompt that dispatches two agents in parallel, one of which dispatches an agent of its own. |
| `claude_stream_edges.jsonl` | **Hand-written.** Shapes the schema permits, or a future CLI might send, that no capture on this machine happens to contain. |

## ⚠️ `tool_use_result` — two shapes are captured, and only one is readable

`tool_use_result` is an undocumented sibling of `message`. Four real ones exist in this
tree, in two files and in **two different shapes**:

| Where | Count | Shape | Read by `result_detail`? |
|---|---|---|---|
| `claude_stream_two_tools.jsonl` | 2 | `Read` — `{"type":"text","file":{"filePath","content","numLines","startLine","totalLines"}}` | **yes**, and `conversation::ResultDetail`'s four fields are exactly that list |
| `claude_stream_subagent.jsonl` | 2 | `Agent` — `{"status","prompt","agentId","agentType","content","resolvedModel","totalTokens","usage",…}`, **no `file` sub-object**, and no `type` key at all | **no** — both are declined and counted |

📌 **The second row was the first test of the bet the first row was written under, and the
bet lost cleanly.** `agent_map::result_detail` reads the `file` sub-object whatever `type`
the line claims — shape-stability assumed, not measured — with
`MapStats::tool_details_declined` as the canary. When the hand-written subagent fixture was
replaced by a real capture, that counter went from 0 to 2 and a test failed loudly, which is
the whole reason it exists. Nothing was mis-parsed and nothing was silently attached; an
`Agent` result simply renders no detail today, and what one *should* show is a card-design
question rather than a parsing one. `agent_map.rs`'s
`an_agent_shaped_detail_is_declined_and_counted_rather_than_part_read` pins it.

So what a `Bash`, a `Write` or an `Edit` puts in this object is still **unknown on this
machine** — and now known to be worth asking about rather than assumed. Two smaller notes
worth having before anyone "fixes" a test against the `two_tools` capture:

- **`numLines` is `4` for a three-line file.** The numbered `tool_result` text ends `4\t`,
  i.e. the trailing empty line is counted. That is the tool's own arithmetic and is passed
  through untouched.
- **`filePath` is a sanitised value** (`C:\work\demo\fx-a.txt`), per the section below — the
  *shape* is the capture's, the string is not.

## `claude_stream_subagent.jsonl` — what the capture settled

It replaced a hand-written reconstruction on 2026-08-13, the first time a real fan-out
was run through this argv. The reconstruction's **correlation** was right — that half
was the decoder's own measured field applied twice, and it survived untouched. Its
**wire shape** was wrong in five ways, each of which is now a named test in
`agent_map.rs`:

- 🚨 **The dispatch tool is called `Agent`, not `Task`.** Both spellings are in this one
  file: `system`/`init` advertises `"Task"` in its `tools` array, and every `tool_use`
  block that actually dispatches carries `"name":"Agent"`. Nothing in this crate routes
  on the name — correlation is `parent_tool_use_id` alone — which is the only reason the
  fixture's `"Task"` never surfaced as a failure. A view that special-cased the name
  would have matched nothing.
- 🚨 **The wire stops at depth 1.** The second agent dispatched its own; that dispatch
  appears exactly twice, as a `tool_use` and a `tool_result`, both scoped to *its
  parent*. The grandchild's own lines never arrive — its `tool_use.id` is never once a
  `parent_tool_use_id`. The reconstruction's depth-2 chain cannot occur. The flattening
  machinery is kept (nothing promises the CLI will keep withholding those lines) but it
  is `conversation.rs`'s synthetic tests that cover it, with their provenance declared.
- ⚠️ **No subagent in this capture ever said anything.** Every subagent-scoped
  `assistant` line carried a `tool_use` block and nothing else; a subagent's answer
  reaches the console only as its parent's `tool_result`. So `Subagent::Said` — which the
  reconstruction exercised twice — is now backed by no observation at all. It is kept
  because the schema permits it, but a view must not assume a card fills with prose.
- ⚠️ **A subagent-scoped `user` line carries the Task prompt as human text**, ahead of
  any work. Two of them here, and they are the whole of `subagent_unrendered`: declined
  on purpose, because the card already shows those arguments in full.
- ⚠️ **An `Agent` result carries a `tool_use_result` of its own**, and it is a shape no
  card can read — see the table above. The reconstruction had none, so the test written
  against it asserted "this capture declines nothing"; the real one declines two.

What the capture **confirmed**:

- 🚨 **Zero `stream_event` lines from a subagent**, as §5.9.1 measured. Of the file's 77
  lines, 8 carry a non-null `parent_tool_use_id` and every one of them is an `assistant`
  or a `user`; all 41 `stream_event`s are main-scoped, including the ones streaming the
  dispatch's own arguments. `MapStats::subagent_stream_events` stays the canary on that.
- **No `result` and no `system` line from a subagent either** — the question the old
  honesty split left open. `parent_tool_use_id` is absent as a *key* on every `system`
  line in the file, so a subagent's session bookkeeping is not merely declined here, it
  is never sent.

📌 **What the capture opened rather than closed:** five `system` subtypes nobody had
seen — `task_started`, `task_progress`, `task_updated`, `task_notification`,
`task_summary`. They are main-scoped (no `parent_tool_use_id` key at all) and correlate
by a `tool_use_id` field of their own, and they carry live progress a card would want:
`description` ("Reading one.txt"), `last_tool_name`, `usage.tool_uses`, `duration_ms`,
`status`. All five currently decode to `Notice` and render nothing. See
`SHELL_ARCHITECTURE.md`'s honesty ledger.

## Sanitisation

The captures ran in a real session on a real machine, so they carried absolute paths,
session UUIDs and dollar costs. Those values are replaced; **nothing structural is.**
Key order is untouched — including the `result` object's, whose `"type"` sits fourth
from the end and is the reason the decoder parses a line whole before dispatching on
it. Unknown fields, null-vs-absent, mixed snake_case and camelCase keys, the
interleaving of `user` results with a later block's deltas, and the leading non-JSON
stdin warning are all exactly as captured.

The `tools`, `slash_commands`, `skills` and `agents` arrays in `system`/`init` are
shortened, and MCP server names are replaced. They are not personal, just long; a few
entries prove the shape as well as ninety do. `tools` keeps `Task` — that entry is the
evidence for the naming finding above.

⚠️ **One substitution in the subagent capture is not a straight replacement, and it is
the only one.** A dispatched agent's prompt contains the working directory, and the
`input_json_delta` fragments carrying it **split mid-path**, so no per-fragment
replacement can match — the whole path is contiguous only in their concatenation. Those
fragments were scrubbed as one string and re-split into the same number of pieces at the
same proportional offsets. The delta count, the block indices, and "the fragments
concatenate to exactly the settled `input`" all survive, which is everything the decoder
and its tests read.

## Editing these

Don't hand-repair a captured line to make a test pass — that turns measurement back
into documentation, which is the failure this module was written to avoid. Either
re-capture, or put the case you need in `claude_stream_edges.jsonl` where its
synthetic provenance is declared.
