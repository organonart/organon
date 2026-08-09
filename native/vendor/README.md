# `native/vendor/` — third-party source kept in this tree

Two upstream crates are vendored here rather than taken from crates.io. Both are vendored
for the same class of reason: **cargo cannot express the change we need**, because the
change *is* a version change, and `[patch]` cannot replace a crates.io crate with a
different version of itself.

| directory | upstream | version | licence | why vendored |
|---|---|---|---|---|
| `nih_plug_egui/` | [nih-plug](https://github.com/robbert-vdh/nih-plug) | git | ISC | #541 — moves the editor from egui 0.31.1 to 0.33 by re-pinning the `egui-baseview` rev the upstream crate chooses for us |
| `egui-wgpu/` | [egui](https://github.com/emilk/egui/tree/main/crates/egui-wgpu) | 0.33.3 | MIT OR Apache-2.0 | #554 T4 — no published `egui-wgpu` accepts wgpu 30, which is what the renderer runs; ported in eight mechanical fixes |

Each directory carries its upstream licence text, and each crate's own module docs record
exactly what was changed and why:

- `egui-wgpu/src/lib.rs` — the port table, what was trimmed, and the two Organon-specific
  changes (`is_linear_target`, `Renderer::set_target_format`) that a future rebase will
  **not** get back for free.
- `nih_plug_egui/Cargo.toml` — the rev pin and its reasoning.

## Attribution

**This is redistributed third-party source.** The export manifest
(`scripts/mirror-platform.manifest`) carries `INCLUDE native/vendor`, so everything here
is republished in the public repo — and MIT, Apache-2.0 and ISC all require the
copyright notice and licence text to travel with the source. The root `NOTICE` names
both vendored crates; keep the two in step.

The copyright lines in the licence files here are taken from each crate's `authors`
metadata, which is the authoritative statement available in the vendored tree itself. They
are **not** byte-copies of upstream's own `LICENSE*` files — if you want those verbatim,
take them from the upstream repositories linked above. Nothing about the licence *terms*
differs; only the provenance of the notice text.

## Modifying anything in here

Don't, beyond what the module docs already record. Every edit is a permanent merge conflict
at that file's churn rate, and the point of keeping these files near-verbatim is that
deleting the directory becomes a one-line change the day upstream ships what we need.
Organon-specific changes are marked `ORGANON PATCH (#…)` for port fixes and
`ORGANON ADDITION (#…)` for behaviour upstream does not have.
