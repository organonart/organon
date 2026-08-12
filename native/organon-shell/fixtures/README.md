# Fixtures — Claude Code's `stream-json` output

What `src/agent_event.rs` is tested against. Two of these are real; one is not, and
the difference is the point.

| File | Provenance |
|---|---|
| `claude_stream_two_tools.jsonl` | **Captured.** `claude.exe` 2.1.228, one prompt that read two files, `--output-format stream-json --include-partial-messages --verbose`. |
| `claude_stream_live_session.jsonl` | **Captured.** The same CLI in a persistent session — `-p --input-format stream-json --output-format stream-json --replay-user-messages` — with two human turns written to stdin 25 seconds apart. |
| `claude_stream_edges.jsonl` | **Hand-written.** Shapes the schema permits, or a future CLI might send, that no capture on this machine happens to contain. |

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
