### Organon Mind can read what a fine-tune moved

A LoRA adapter is the one artifact Mind cannot obtain any other way: two models with the
same architecture, different weights, and a known cause for the difference. `organon-core`
now reads one. Point `lora::read_adapter_dir` at a PEFT directory — `adapter_config.json`
plus `adapter_model.safetensors` — and per adapted module it returns how far the weights
moved (`‖ΔW‖_F`) and how concentrated the movement was (the effective rank of the update),
with the layer index and module name parsed out of the state-dict path. Nothing draws it
yet; #147 T3 is the lens.

🚨 **`ΔW` is never materialized, and that is correctness rather than speed.** The update is
`s·B·A` with `s = alpha/r`, and `ΔW` is `out × in` — 4096×4096 for one attention
projection, hundreds of times over. Every number goes through the r×r middle instead:
`‖BA‖²_F = trace((BᵀB)(AAᵀ))` collapses to an elementwise sum of two r×r Grams, and the
singular values are those of `R_B R_Aᵀ` after a Householder QR of each factor with `Q`
never formed. ⚠️ **The per-neuron version has no such shortcut** — a per-output-row norm
needs the whole product, because the answer has `out` numbers in it. That cliff is named in
the module doc rather than discovered halfway into a lens.

**"Effective rank" names at least three different quantities**, so the module says which
one it computes: Roy & Vetterli (2007), `exp` of the Shannon entropy of the normalised
spectrum — exactly `r` for `r` equal singular values, exactly `1` for a rank-one update,
continuous in between. `stable_rank` (`‖M‖²_F / σ²_max`) is reported beside it because the
two genuinely disagree — stable rank is dominated by the largest singular value where
entropy rank counts the whole tail — and the raw singular values are exposed so a fourth
definition needs no change here.

⚠️ **Four ways to get a plausible wrong number, each closed by name.** `data_offsets` in a
safetensors header are relative to the **end of the header**, not the start of the file;
reading them as absolute decodes header JSON as floats and yields finite garbage.
**`BF16` is not `F16`** — a `bf16` 1.0 read as `f16` is 32.0. **rsLoRA divides by
`sqrt(r)`**, so reading it as `alpha/r` understates every norm by a factor of `sqrt(r)`.
And the **rank comes from the tensor shapes, never from the config** — `rank_pattern` can
give a module a rank the top-level `r` does not state, and a shape cannot be wrong about
it (a disagreement is reported rather than hidden). **DoRA is refused outright**: its
update is not `(alpha/r)·B·A`, so reading it as LoRA would produce numbers instead of an
error, which is the one outcome worth refusing.

**Zero new dependencies.** The safetensors container is 8 bytes of little-endian length,
that much UTF-8 JSON, then the payload — `serde_json` and `half` were already here, and
`cargo tree -p organon-core` still shows no `nih_plug`, `wgpu` or `egui`. Reading is
streaming: the header, then only the two byte ranges of one module at a time, so peak
memory is one module's factors rather than the adapter.

Forty tests, and one of them exists because the suite failed a mutation. A deliberately
broken `frobenius_of_product` — one summing only the **diagonal** of `(BᵀB)(AAᵀ)` — passed
everything, because every fixture built `B` with orthogonal columns and `A` with orthogonal
rows, and the rotations meant to make them dense are precisely the transformations that
leave both Grams alone. The suite now carries a hand-computable non-orthogonal pair
(`‖BA‖_F = sqrt(7)`, singular values `(3 ± sqrt(5))/2`) and a comparison against an
explicitly materialized `B·A` on dense random factors of three shapes — including one where
`r` exceeds `out`, so the `min(out, r, in)` reasoning is exercised rather than assumed.

📌 **Nothing has been read from a real adapter.** Every fixture is synthetic, built byte by
byte by the tests. What this closes is "no arithmetic exists"; #147's *"nothing here has
been run"* is still true of the file format itself.
