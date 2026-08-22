### The Delta lens's two parked coverage gaps are closed, against a tree that was read rather than remembered

`classify_site` maps a LoRA-adapted module name onto a node of the specimen. When its tables were
last audited, two gaps were deliberately left open: closing them needed HuggingFace names nobody
was willing to guess at, and a guess in the table whose entire purpose is not-guessing is the same
mistake in a new costume. The names have now been read out of an installed **transformers 5.5.0**
(`transformers/models`, 453 packages).

✏️ **`fc1` / `fc2` joined the MLP leaf table.** OPT-style decoders declare them **directly on the
layer**, so the path is `…decoder.layers.N.fc1` with no `mlp`/`ffn` segment for the container
fallback to catch — and `classify_site` is handed the tail *after* the layer index, which for OPT
is the bare leaf. They classified as `Unclassified`: a layer whose MLP node stayed dark while a
real measurement for it existed. **155** classes in that tree define `self.fc1` and every one is
feed-forward. Exactly one has *Attention* in its name — `Mask2FormerMaskedAttentionDecoderLayer` —
and it is not a counterexample: there `fc1`/`fc2` are the `dim_feedforward` pair while attention is
`self_attn` / `cross_attn`. `fc2` scans identically.

✏️ **`densereludense` joined a new `MLP_CONTAINERS` table.** T5 and its family name the
feed-forward block `self.DenseReluDense`. This *finishes* an earlier fix rather than adding a new
one: removing `wo` from the attention leaves stopped T5's FFN down-projection being drawn on the
attention ring, but left it unrecognised — the container is what puts it on the node it belongs to.
The attention container is checked first and catches T5's `SelfAttention` / `EncDecAttention`, so
the two can never fight over one name.

🚨 **A table entry that can never match is exactly as bad as a wrong one, and it is invisible.** The
obvious companion to `DenseReluDense` is `DenseGatedActDense`, and it is dead: `T5DenseGatedActDense`
is a *class*, and the attribute it is bound to is `self.DenseReluDense` in **both** the gated and the
ungated variant, across all seven families using the layout (`t5`, `mt5`, `umt5`, `longt5`, `udop`,
`pop2piano`, `pix2struct`). `self.DenseGatedActDense` occurs **zero** times in the tree, so no
module path can ever carry that segment. Measured rather than argued: adding it and skipping one
guard leaves **654/654** tests passing — it compiles, reads as thorough, and covers nothing.

✏️ **So the container patterns now have a standing reachability guard.**
`an_unmatchable_table_entry_is_dead_weight` requires every entry in `MLP_CONTAINERS` to be matched
by a name in circulation, so the next unmatchable pattern fails at the point it is added instead of
sitting in the table forever looking like coverage. Both additions are mutation-tested one at a
time; the disjointness guard between the two leaf tables is unchanged and still passes.
