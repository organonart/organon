### The Delta lens has a producer: `organon mind adapter`

`world.rs`'s `build_mind_graph` has read `ipc::adapter_sidecar_path()` since #147 T3 and **nothing
has ever written it**, so the lens has only ever printed *"no adapter selected"* and cleared the
graph. Three verbs on the `organon` CLI are the missing half:

```
organon mind adapter <PATH>     # check it, then write it
organon mind adapter --clear    # empty the sidecar: "no adapter selected"
organon mind adapter --show     # what is selected, and where that answer came from
```

Writing the file **is** the whole trigger — the reader re-reads whenever the string changes, so
there is no counter to bump, no `Shared` field, and nothing to drain. No `LAYOUT_VERSION` movement.

🚨 **The check before the write is the tier, not a nicety.** `build_mind_graph`'s failure arm sets
`*delta = None` — and that field *is* the cache key which would otherwise suppress a re-read — so a
bad path in the sidecar is re-read and re-refused **on every frame**, in the visual, into a terminal
nobody is watching. The CLI is the one place a person is reading output, so the refusal lands here
and names what is wrong with *this* directory. `select_adapter` does the check and the write in one
function so that "refused ⇒ nothing written" is a property of one place rather than of every caller
remembering the order; reversing them is one of the nine mutations run against these tests.

📌 **It runs everything `lora::read_adapter_dir` does except the arithmetic** — `lora`'s own
`parse_adapter_config` (which is what refuses DoRA by name) and its own `parse_safetensors_index`,
bounded by `MAX_HEADER_BYTES` — and deliberately does **not** stream the `lora_A`/`lora_B` payloads.
So a refusal is conclusive and an acceptance is not: an adapter can still fail in the visual on a
tensor pair or an unsupported dtype. The header parse earns its bytes on one common real failure — a
HuggingFace clone made without git-lfs leaves a ~130-byte **text pointer** named
`adapter_model.safetensors`, which exists, opens, and is not an adapter. Deleting that call makes
the test suite *accept* such a directory.

⚠️ **The path is written absolute**, because the visual is a different process with a different
working directory. On Windows `canonicalize` returns a verbatim path (`\\?\C:\…`), so `de_verbatim`
shortens the drive-letter form — and deliberately leaves a UNC share alone, since the obvious
`strip_prefix(r"\\?\UNC\")` yields `server\share`, a **relative** path and precisely the failure the
absolutising exists to prevent. That mutation fails the test with
`left: "server\\share\\lora"`.

🚨 **The sidecar is namespaced, and that is how this verb can fail while looking like it worked.**
`ipc::adapter_sidecar_path()` resolves through `$ORGANON_IPC_NS`, or else the namespace of the
edition the binary was **compiled** as — `organic-math`, `organon-mind`, `organon-shell`. So an
`organon` built for one edition writes a file another edition never reads, and the symptom is
identical to not having run the command at all. Nothing in the CLI process can decide that for the
caller, so every form prints the path *and* the namespace it used, and `--help` names the variable.

📌 **No "no live Organon" warning here, unlike the `CliOp` lane, and the difference is real rather
than an omission.** A queued op is dropped if the visual starts later; this is a *file*, and the
lens reads it the first time Delta is selected — now or tomorrow. Choosing an adapter with nothing
running is supported, so warning about it would be false.

⚠️ **No Studio-listing verb.** `organon mind adapters` over `/api/models/loras` was scoped and
dropped: the Studio is not running on this machine, neither this repo nor unsloth-buddy carries a
schema for that response, and a parser written against a guessed shape is the kind of thing that
ships wrong and stays wrong until the day it is needed. The local-path verb is what unblocks the
lens; discovery over the API waits for one session with the Studio up.

🚨 **Nothing here has been drawn.** No real adapter has been parsed on any machine — the fixture is a
synthetic 2×2 `F32` pair the tests build byte by byte — and no GPU has rendered a Delta frame. What
is true is that the sidecar now has a writer, that a synthetic adapter round-trips through it, and
that six refusals are pinned by mutation.
