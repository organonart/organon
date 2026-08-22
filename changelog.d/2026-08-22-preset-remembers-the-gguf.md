### A preset remembers which `.gguf` it was showing

Loading a `.gguf` and arranging it — Neural Network generator, Connectome topology, the
`nw_*` dials, a material, a palette — produced a view that could already be saved as an
ordinary Organon preset. Recalling it gave you back everything *except the model*: the
generator came back, every dial came back, and the stage was empty. James's framing was
that a GGUF view "is really just an Organon preset", and it was almost true; the missing
piece was one path.

`PresetValues` now carries **`model_path`** beside `hdr_path`, captured from the model
sidecar in `capture()` (the GUI-thread capture — it is a file read, so the audio-thread
`capture_params_only` still leaves it empty) and restored on recall by writing the
sidecar and bumping `model_gen` in `Shared.mind[1]`, which `bin/mind_runtime.rs` and the
visual already edge-detect. No `Shared` change was needed: the counter has existed since
#367 Tier 1 and this only gives it a second producer.

📌 **The specimen rides the Generator bucket, not Environment.** `hdr_path` is the one
existing field of this kind and it is re-driven for Scene and Environment recalls,
because an `.hdr` is an Environment thing — but a model is not. `generator`, `nw_topology`
and every one of the ~40 `nw_*` dials is a **Generator** field, and a specimen is what
those dials are drawn *of*; a Generator preset that restored the dials over an empty
stage would be exactly the bug this closes, one tab down. Getting the scope wrong is
silent in both directions — too narrow and saving a Scene quietly drops the model, too
wide and recalling an unrelated Environment preset yanks it out from under a running
runtime — and both read as "presets are flaky" rather than as a scope bug. So the answer
is a named, tested function rather than a `matches!` at the call site:
`preset::recall_redrives(scope, owner_tab, value)`, which derives the Scene relation from
`EditorTab::SCENE` instead of restating it.

🚨 **An empty `model_path` re-drives nothing, and that is a safety property rather than a
convenience.** It is the same rule `hdr_path` follows, but for a much sharper reason: a
preset that could *clear* the model would be a saved **look** that unloads a multi-GB
specimen as a side effect. Recall points at a model; it never points away from one.

⚠️ **The Key Map deliberately does not follow the model.** A MIDI-held Scene preset
already swaps the `.hdr` on key-down and swaps it back on release — cheap and reversible
for an IBL image. A held note is the last place to start a multi-GB load, and "revert on
release" would mean two of them. `overlay_tabs` copies neither out-of-band field, and the
`.hdr` responder thread now says in as many words why it has no model equivalent.

⚠️ **`param_table.rs`'s partition drift-guard now drops two names, not one**, and the
comment that called `hdr_path` "the single deliberately out-of-band field" would have
become false the moment a second one appeared. It now names the exception as an exception:
a field dropped from that guard is a field it stops watching, so it earns the drop by
having its own restore path and its own test — which is the decision, not the bookkeeping.

Subset (per-tab) saves keep each out-of-band field only in the bucket that owns it, by
the same rule that decides recall — a Generator subset keeps `model_path` and drops
`hdr_path`, and an Environment subset does the reverse.

Five new tests in `preset.rs`, each written by breaking the thing it claims. The one worth
naming is the backward-compatibility guard: `PresetValues` is serde-deserialized from
files people already have on disk, and `model_path` without `#[serde(default)]` makes
**every** existing preset fail to load — damage found by a person, not by CI. Removing
that attribute fails eight tests, including `a_preset_saved_before_the_field_existed_still_loads`
with `an old preset must still load: left: 0, right: 1`.
