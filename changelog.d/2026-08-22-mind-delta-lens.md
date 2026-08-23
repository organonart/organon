### The specimen, lit by what a fine-tune moved

#147 Tier 2 landed the arithmetic an hour ago: point `lora.rs` at a PEFT LoRA adapter and
it answers, per adapted module, how far the weights moved (`‖ΔW‖_F`) and how concentrated
the movement was, both exact functions of the file. Nothing drew them. **Tier 3 is the
lens** — a new `Shared.mind[2]` view (`2`, beside `0` specimen and `1` embedding galaxy)
that builds the model's own architecture topology and lights each site by the movement
measured there. The BinDiff parallel `doc/organon_prd.md` §6.2 has been asking for, and
the first differential lens Organon Mind has.

It is written as the twin of the live-activation path, on purpose: `stream_frame_into_scalars`
builds the architecture topology and overwrites `node_scalar` from a **streamed frame**;
`delta_into_scalars` does the same from a **static adapter summary**. Both walk the same
`for_each_arch_node`, which is the single source of truth for node order — per layer
`[backbone, head_0 … head_n, mlp]`. A private copy of that walk would misattribute every
value on screen while still producing the right node *count*, so the order is pinned by
test rather than by comment: swapping the head and MLP arms fails with
`head node 1 got the attention value: 0.2`.

🚨 **Two lenses now drive one visual channel, and a viewer has to be able to tell which
they are looking at.** The #226 node glow renders `node_scalar` whether it came from an
activation ring — a *labeled proxy* for "this site is busy right now", `MIND_ARCHITECTURE.md`
§3's first recorded gap — or from an adapter file, which is **measured**. The mode
selector is off-screen from the viewport, so it cannot be the answer. Three things
separate them and each is decisive alone. **The silhouette**: the live lens rides the
skeleton unchanged, a straight-sided cylinder with every head ring at the same radius at
every depth, forever; the Delta lens displaces each off-axis site radially by its own
movement, so an adapter's footprint is a *profile* — bulging where it moved, pinched
toward the axis where it did not. **It holds still**: `world.rs`'s live seam is gated to
view 0, so an arriving frame can never repaint a Delta view. **The head ring is round**,
for the reason below. The trunk never bends: backbone nodes sit on the axis, so the
scaling is a no-op on them by construction.

⚠️ **"Holds still" had a second hole, and the ring gate does not cover it.** The #226
cascade sim is not gated by anything view-shaped: with a firing mode set it computes an
`activity` the glow uses *instead of* `node_scalar`, so a measured quantity would have
been replaced by a free-running procedural pulse — a proxy animation wearing a
measurement's shape, which is the exact failure this tier exists to prevent. `sim_on` now
excludes the Delta lens on the same reasoning that already excludes a live stream. 📌 The
embedding galaxy has the identical hole and it is deliberately left open: its node scalars
are full N-D embedding norms, equally real and equally paintable-over, but that is #507's
call rather than this tier's.

⚠️ **Uniform across heads is a limit, and the picture is what says so.** `q_proj` is one
tensor covering every head; resolving per-head needs per-output-row norms of `ΔW`, which
is the full `out × in` product Tier 2 stopped short of on purpose. So the head ring
carries a **per-layer attention** quantity drawn on per-head nodes, and it therefore
renders as a perfect circle where the live lens's ring is ragged because its heads really
do differ. A bright ring means *this layer's attention moved*, never *these heads moved*.

🚨 **The normalisation refuses two things, and both refusals are the feature.** The
displayed quantity is root-mean-square displacement **per weight**, `‖ΔW‖_F / sqrt(out·in)`.
*Not raw `‖ΔW‖_F`*: Frobenius norms grow with entry count, so a `14336×4096` MLP projection
outweighs a `4096×4096` attention projection by ~1.87× before any training happens at all
— lighting the specimen with raw norms would paint every model's MLP brightest and invite
"fine-tuning moves the MLP most", an artifact of matrix shape wearing the clothes of a
measurement. Dropping the divisor fails with
`same per-weight movement ⇒ same site value (4.096 vs 7.6629…)`, which is that artifact,
measured. It also composes exactly — over a group of modules the RMS is
`sqrt(Σ‖ΔW‖²_F / Σ(out·in))`, the RMS over the concatenation of their entries, which is
how a site pools its modules and how the backbone pools a whole layer. *And not a
per-adapter maximum*: that puts `1.0` at the top of every adapter, so a barely-trained
LoRA and a heavily-trained one render identically, destroying exactly the comparison the
lens exists for. Substituting one fails with `a louder adapter must not look identical`.
The mapping to brightness is a **fixed** five-decade log window (1e-6 … 1e-1), the same
for every adapter ever loaded and the one step here that is a display choice rather than
an exact function of the file; `DeltaSites::rms_range()` reports the real extremes so a
readout can print what was actually measured.

⚠️ **A module-name table is a silent-failure surface**, so the layer's `Backbone` node
carries *every* module the adapter touched, recognised or not, and unrecognised names are
**reported** rather than guessed onto a site. An architecture spelled a way nobody listed
must look like an unrecognised architecture, not like a layer nobody trained; falling back
to `Mlp` instead fails with `nor onto the MLP node`.

🚨 **And a generic leaf must never outvote its parent — the first draft of that table got
this wrong twice.** The exact-tail match runs *before* the container fallback, so a leaf
name reused by the other kind of site does not merely mis-label: it overrides the parent
that would have got it right and lands a real measurement on the wrong node as a confident
picture, which is strictly worse than not recognising the name at all. **`dense`** was in
the attention table, and HuggingFace BERT names the attention output *and* both FFN
projections with a bare `dense` leaf — `intermediate.dense` and `output.dense` are
feed-forward and are not under an `attention.` parent, so both were being drawn on the
attention ring. Auditing the rest of the table on the same rule turned up **`wo`**: T5's
FFN output projection is `…DenseReluDense.wo` while its *attention* output is
`…SelfAttention.o`, so that entry moved T5's feed-forward update onto the attention ring
too. ⚠️ Neither removal loses anything, which is the tell that both were redundant from
the start — Falcon's `self_attention.dense` and Meta-llama's `attention.wo` are still
caught by the container fallback, pinned by regression tests that fail if that fallback is
deleted. An entry that is redundant on its true positives and wrong on its false ones is
all cost. The admission rule — *a leaf belongs in a table only if it cannot name a site of
the other kind in any architecture in circulation* — is now written above the tables, with
the verdict on every remaining entry, so nobody re-audits it from scratch. The tables are
hoisted to module consts so a test can assert they are disjoint, which is the same defect
one layer in. Modules with no layer index
(`lm_head`, an embedding adapter) are reported too, rather than folded into layer 0 —
there is no node for them, and attributing them to a real layer would brighten it with a
measurement that is not about it.

📌 **No `Shared` change and no `LAYOUT_VERSION` movement.** The view rides the `mind[2]`
slot that has existed since #367 Tier 1, and the adapter *directory* rides a new sidecar,
`ipc::adapter_sidecar_path()`, because a path is not a control-rate value. ⚠️ The write
clamp in `lib.rs` and the read decoder in `math::mind_view_mode` now both say `2` — a view
added to one and not the other is either a selector that silently does nothing or a value
nothing decodes.

⚠️ **Nothing writes that sidecar yet**, so selecting the view today clears the graph and
prints `no adapter selected`. That is the honest failure, on the rule the embedding galaxy
already follows: substituting the specimen would show the user a different thing than the
one they asked for. The picker is a later tier, as is the checkpoint scrub the same
builder gives for free.

> ✏️ **Superseded inside this same release** — `organon mind adapter <PATH>` writes it now
> (T3½, its own entry below). The paragraph is left standing because it is what was true
> when the lens landed, and because the *reason* it gives for the empty state is unchanged:
> with no adapter selected the lens still says so rather than substituting the specimen.
> What is no longer true is only "nothing writes it".

🚨 **And nothing here has been seen.** No adapter has been read on any machine, no GPU has
drawn this, and every claim is arithmetic and geometry checked offline against synthetic
fixtures. Green and ready to try, not verified working.

Also corrected on the way: `math::mind_view_mode`'s doc comment still described `1` as
"Live streaming" and `2` as the galaxy — the encoding #520 retired — so a reader trusting
it would have taken today's galaxy for a live stream. The neighbouring test had already
been updated and the prose had not, which is the two-descriptions-of-one-thing failure in
its quietest form.
