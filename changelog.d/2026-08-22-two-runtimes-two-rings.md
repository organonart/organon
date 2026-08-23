### Two runtimes, two rings — and the reader can finally name the one it means

A base model and its own fine-tune can now run at the same time, each writing its own
`MindFrame` activation ring, with something able to attach to a **named** one rather than
to whichever it happened to find. That is #191 Tier 1, the live half of #147.

📌 **Almost none of it is new machinery, and that is the finding.** `$ORGANON_IPC_NS`
already forks every `$TMPDIR` mmap and sidecar through `ipc::ns_file` — the mechanism that
lets an Organon session and a Mind session coexist. Two runtimes are two processes with
two namespaces, and two rings follow with **no `MindFrame` field saying which model wrote
it** and no `LAYOUT_VERSION` movement. Such a field would have been a permanent,
offset-sensitive commitment answering a question the namespace already answers, on a ring
that is transient and recreated each run.

**What was genuinely missing was the reader's half.** Every path function resolves *this
process's* namespace, which is exactly right while a process talks to its own peers and
useless the moment one process wants to look at another namespace's channel — which is
what a difference lens is. So: `ipc::ns_file_checked`, `ipc::mind_ring_path_in`,
`MindRingReader::open_ns`, and `MindRingWriter::create_ns` for a harness that needs both
rings inside one process.

🚨 **A named namespace is refused where the env var falls back, and the asymmetry is the
point.** `$ORGANON_IPC_NS` falls back to the edition when it is junk because a spawned
visual has to come up on *something*. A namespace typed by a caller is a mistake, and
quietly handing it the local ring would answer a question nobody asked — reporting *this*
model's trace under the *other* model's name, which is wrong and looks right. One
sanitizer serves both doors, so a named ring can never reach a `$TMPDIR` path the env var
could not.

⚠️ **`Err` and `Ok(not open)` are deliberately different.** An illegal name never resolves
itself; a legal name whose runtime has not started yet is the ordinary case while you are
still typing the second launch command. Conflating them sends someone hunting a spelling
mistake that is not there.

**The foot-gun that is now closed.** `organic-math-mind-writer` takes `--ns <name>`, and
both it and `mind_runtime` announce their namespace on the happy path. Two runtimes are
otherwise indistinguishable in two terminals — and the failure that hides is the expensive
one: both on one namespace overwrite each other's frames in a single ring, so a difference
lens reads a model against itself. Nothing errors. It looks like a working demo whose two
traces agree perfectly. (A PowerShell `$env:` assignment persists for the session, which is
precisely how you launch the second runtime onto the first one's ring without noticing.)

⚠️ **The HTTP port is not namespaced**, because it is a TCP listener rather than a
`$TMPDIR` path. The second runtime finds `ORGANIC_MATH_LLM_PORT` taken, says so, and comes
up with its OpenAI-compatible server **off** — the ring still fills, so a fan-out over HTTP
would quietly reach one model twice. Set it per runtime. Recorded now because Tier 2 is
where it bites.

🚨 **What has been seen, and what has not.** Two synthetic writers ran side by side on
organon-one: two ring files, both stamped `MIND`, both at `write_seq` 61 after three
seconds, byte-different — and the default `organic-math-mind.bin` **not created**, which is
what proves `--ns` diverted rather than merely printed. **No GPU drew any of it and no
model was loaded.** The synthetic writer sets no provenance flags, so *"two runtimes, both
reporting `activation tap MEASURED`"* is **not** demonstrated. What is pinned instead is
that provenance is **per ring**, so a measured base beside a proxy-fallback fine-tune stays
distinguishable rather than being silently averaged — which is the property the difference
lens actually depends on.
