# Licensing

Organon is **split-licensed on purpose**: the engine is permissive, the plugin is
GPL, and the line between them is drawn by a dependency rather than by preference.

| What | Licence | Why |
|---|---|---|
| `native/organon-core` · `native/organon-render` · `native/organon-mind` · `native/organon-console` | **MIT OR Apache-2.0** (your choice) | The engine. No plugin bindings anywhere in it. |
| `native/xtask` | **MIT OR Apache-2.0** | The build tool. Does not link plugin bindings. |
| `native/` root crate (`organic-math-native`) — the plugin, the standalone, the visual, the `organon` CLI | **GPL-3.0-or-later** | Forced. See below. |

Full texts: [`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE),
[`LICENSE-GPL`](LICENSE-GPL). Third-party material: [`NOTICE`](NOTICE).

## Why the root crate is GPL

Not a choice, and worth stating precisely so nobody "fixes" it:

- **nih-plug itself is ISC** — permissive, no obligation.
- **`nih_export_vst3!` is implemented over [`vst3-sys`](https://github.com/RustAudio/vst3-sys), which is GPLv3.** nih-plug's own README says it outright: *"any VST3 plugins built with NIH-plug need to be able to comply with the terms of the GPLv3 license."*
- The root crate invokes that macro (`native/src/lib.rs`), so it is GPL — and so is everything built from it, because the standalone, the visual and the CLI are all binaries of that same crate.
- **CLAP is not the constraint.** `clap-sys` is MIT/Apache-2.0. Only the VST3 arm forces GPL. A build that dropped `nih_export_vst3!`, or replaced the bindings, would be free of it.

## What this means for you

**Using the engine in your own project** — take `organon-core`, `organon-render`,
`organon-mind` or `organon-console` under MIT or Apache-2.0, whichever suits you.
That is the ~100k lines worth reusing, and it is deliberately unencumbered: the
math, the renderer, the GGUF reader, the IPC spine. Nothing in them links a plugin
binding, and `cargo tree -p organon-core` is the test that keeps it that way.

**Shipping a VST3 built from this repo** — you are distributing GPLv3 software.
Comply with it: source availability, same-licence derivatives, the usual terms.

**Contributing** — you license your contribution under the terms of the crate you
touched. There is no CLA. If a change would move code *from* an engine crate *into*
the root crate, say so in the PR: that direction relicenses it, and it is the one
licensing mistake that is easy to make by accident.

## Trademark

Licences grant copyright, not trademark. The **Organon** name and its marks remain the
author's, and the brand assets are not part of this repository. Fork the code, ship it,
sell it — under your own name, not this one.

## Open question, recorded rather than hidden

A software-class trademark check on "Organon" has not been done (there is a large
pharmaceutical company by that name, in an unrelated class). This affects the
project's own naming, not your rights under the licences above.
