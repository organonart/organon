# Security

## Reporting a vulnerability

**Report privately** through GitHub's advisory form:
<https://github.com/organonart/organon/security/advisories/new>. If that is unavailable
to you, mail `hello@organon.art` with `[security]` in the subject line.

Please don't open a public issue for something exploitable.

This is a small project with one maintainer, so what you get is a real best effort rather
than a service level: an acknowledgement within a few days, then either a fix or a written
explanation of why it is not one. You'll be credited in the advisory unless you'd rather
not be.

## What this software actually is

Organon is a desktop visualizer, an audio plugin and two standalone instruments. It is not
a server and it has no accounts. It opens exactly one outbound connection, to a model
endpoint you configure yourself (below); everything else about the threat model is
**local**: files you open, and things already running on your machine.

Worth knowing before you go looking:

- **The two processes talk through a file in `$TMPDIR`.** `ipc.rs` builds every IPC path as
  `$TMPDIR/<namespace>-<suffix>` and memory-maps it, created with the process umask rather
  than an explicit mode. On macOS `$TMPDIR` is already per-user; on Linux it is usually
  `/tmp`. So **anything running as another local user with read/write there can observe or
  drive the visual** — that is the current design, not a hidden bug, and it is why the
  `organon` CLI can drive a running instance without any authentication. Treat it as a
  single-user, single-trust-domain program. A report that tightens this is welcome; a
  report that it exists is answered by this paragraph.
- **GGUF parsing is the one place untrusted binary input meets a parser.** Organon Mind
  reads model files you point it at (`organon-core/src/gguf.rs`). Memory-safety or
  denial-of-service findings there are the most likely real vulnerabilities in this tree,
  and the most welcome.
- **Presets, galleries and material folders are parsed from disk** (JSON and PNG, via
  `preset.rs`). Same category, smaller surface.
- **Organon Console runs a terminal.** It spawns your shell in a PTY and executes what you
  type — that is the product. Command execution through it is not a vulnerability. Escapes
  from what the *rendered* grid is supposed to be able to do — a control sequence that
  reaches beyond the terminal emulation — would be.
- **The agent talks to a model server over plain HTTP** (`agent.rs`). It POSTs an
  OpenAI-shaped chat request to the endpoint in the `organic-math-agent.txt` sidecar,
  which defaults to `http://127.0.0.1:1234/…` — a local LM Studio or Ollama. Two things
  follow, both true today: there is **no TLS**, and `parse_url` enforces only the `http://`
  scheme, **not** that the host is loopback. So editing that sidecar to a remote address
  sends your prompts across the network in cleartext. It is your file and your choice, but
  the code is looser than its own comment claims, and that gap is written here rather than
  discovered.
- **Vendored third-party code lives under `native/vendor/`.** If the flaw is upstream's,
  say so and report it upstream too; we will still take the report here so the vendored
  copy gets updated.

## Supported versions

There are no releases yet. `main` is the supported version.

## Building from source

Every artifact here is built from this tree by you. There are no published binaries to
verify, and no signing keys to trust. On macOS the build scripts **ad-hoc sign** the
bundle (`codesign -s -`), which satisfies the loader — it is not an identity, and it
asserts nothing about provenance.
