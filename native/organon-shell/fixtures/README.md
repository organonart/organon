# Fixtures — Claude Code's `stream-json` output

What `src/agent_event.rs` is tested against. Two of these are real; two are not, and
the difference is the point.

| File | Provenance |
|---|---|
| `claude_stream_two_tools.jsonl` | **Captured.** `claude.exe` 2.1.228, one prompt that read two files, `--output-format stream-json --include-partial-messages --verbose`. |
| `claude_stream_live_session.jsonl` | **Captured.** The same CLI in a persistent session — `-p --input-format stream-json --output-format stream-json --replay-user-messages` — with two human turns written to stdin 25 seconds apart. |
| `claude_stream_edges.jsonl` | **Hand-written.** Shapes the schema permits, or a future CLI might send, that no capture on this machine happens to contain. |
| `claude_stream_subagent.jsonl` | **Hand-written.** A `Task` call and the subagent inside it, including one that dispatches its own. See the warning below. |

## ⚠️ `claude_stream_two_tools.jsonl` holds the tree's only `tool_use_result` objects, and both are `Read`s

`tool_use_result` is an undocumented sibling of `message`, and this file is the only place
in the repository where a real one exists — twice, both carrying
`{"type":"text","file":{"filePath","content","numLines","startLine","totalLines"}}` for a
`Read`. `conversation::ResultDetail`'s four fields are exactly that list and stop there.

So what a `Bash`, a `Write` or an `Edit` puts in this object is **unknown on this machine**.
`agent_map::result_detail` reads the `file` sub-object whatever `type` the line claims,
which is a bet on shape-stability rather than a measurement, and
`MapStats::tool_details_declined` is what catches the day the bet is wrong. Two smaller
notes worth having before anyone "fixes" a test against this file:

- **`numLines` is `4` for a three-line file.** The numbered `tool_result` text ends `4\t`,
  i.e. the trailing empty line is counted. That is the tool's own arithmetic and is passed
  through untouched.
- **`filePath` is a sanitised value** (`C:\work\demo\fx-a.txt`), per the section below — the
  *shape* is the capture's, the string is not.

## 🚨 `claude_stream_subagent.jsonl` is a reconstruction, and nobody has checked it against a real fan-out

It was written from the schema and from the one real subagent line in the tree —
`claude_stream_edges.jsonl`'s, which is itself hand-written. **No capture on this
machine contains a `Task` call at all**, so the thing the conversation view's whole
subagent path is tested against is a shape we reasoned to, not one we observed.

What that does and does not undermine:

- **Sound.** The correlation itself. `parent_tool_use_id` naming the spawning
  `tool_use.id` is decoded and asserted against the real CLI's own field, and the
  depth-2 chain is just that field applied twice.
- ⚠️ **Unverified.** Whether a real subagent emits exactly these line *kinds*, in this
  order, and nothing else. In particular whether it ever emits a `result` or a
  `system` line of its own, which this fixture does not contain and `agent_map.rs`
  therefore declines by default.
- 🚨 **Deliberately absent, because it was measured absent.** Any `stream_event` from a
  subagent. §5.9.1 measured that token deltas are never forwarded, so the fixture has
  none and `MapStats::subagent_stream_events` exists to catch the day that stops being
  true. A fixture containing them would be inventing the very thing the design rests on
  not existing.

**Re-capture this one the first time a real fan-out runs through the console**, and if
the shapes differ, the fixture is what is wrong.

## Sanitisation

The captures ran in a real session on a real machine, so they carried absolute paths,
session UUIDs and dollar costs. Those values are replaced; **nothing structural is.**
Key order is untouched — including the `result` object's, whose `"type"` sits fourth
from the end and is the reason the decoder parses a line whole before dispatching on
it. Unknown fields, null-vs-absent, mixed snake_case and camelCase keys, the
interleaving of `user` results with a later block's deltas, and the leading non-JSON
stdin warning are all exactly as captured.

The `tools` and `slash_commands` arrays in `system`/`init` are shortened. They are not
personal, just long; a few entries prove the shape as well as ninety do.

## Editing these

Don't hand-repair a captured line to make a test pass — that turns measurement back
into documentation, which is the failure this module was written to avoid. Either
re-capture, or put the case you need in `claude_stream_edges.jsonl` where its
synthetic provenance is declared.
