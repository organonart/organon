### PBR text T2 — the legibility harness: §9's two laws as a number, verified without a GPU

`doc/pbr_text_engine.md` §9 says a glyph-grid preset may replace a cell with almost anything
provided two things hold — the cell's energy stays in the cell, and its integrated brightness
tracks what TTE said the cell was — and that both are measurable. They now are.
`native/organon-render/src/legibility.rs` (organon#217, T2) downsamples any image to the cell
grid in **linear light** (Rec. 709 luma, area-weighted at fractional pixel boundaries, the
**2:1 cell aspect** taken from the fixture rather than assumed), correlates the per-cell luma
against a hand-readable fixture file, reads inter-cell **bleed** off the blank cells, and judges
the three numbers against a `Thresholds` that is a parameter. Two fixtures land under
`organon-render/tests/fixtures/`: Omarchy's logo, which reproduces §3's census (337 `█`, 32 `▀`,
32 `▄`) on the padded 81×10 grid, and a small asymmetric "L" with a colour gradient — because
the logo is nearly mirror-symmetric and a harness that flipped its rows would score it almost
perfectly. Orientation is *written* in the fixture (`order top-down` or `bottom-up`, the latter
TTE's native row order), never assumed.

📌 **It is one of the rare things here that `cargo test` fully verifies.** A CPU painter
renders a fixture as flat tiles at the cell aspect and then degrades it on purpose — Gaussian
blur of σ cells, additive noise, a brightness gain, a per-cell scramble — so the metric is
tested against inputs whose right answer is known: a perfect render scores 1, a one-cell blur
fails law 1, a scramble fails law 2 with an identical energy budget, and a 6× gain (§4's
phosphor above paper white) moves nothing. Every invariant was mutation-tested — the harness
broken on purpose and the failure message read. With the downsample's rows flipped, the
upside-down asymmetric render scored `corr 1.0000 · PASS` and the test failed with *"a vertical
flip must not score as legible"*; with Pearson's centring dropped, the affine-fog test failed.
What no test here can say is what a real render scores: the entry points `assess` and
`assess_readback_rgba8` are wired nowhere until T3 decides where the preset gate lives.

⚠️ **Three findings the numbers produced that the spec did not anticipate.** Pearson is
invariant to an *affine* map, not only a gain — a uniform fog over the frame scores **exactly
1.0** on correlation and is caught by stray/bleed alone, which is why `pass()` needs all three.
A gamma-wrong render (emission taken as `fg/255` instead of decoded) still clears the 0.90
correlation default at 0.9145; only the lit-only coefficient sees it clearly, and it has no
threshold yet. And an 8-bit readback **clips a gain above 1**, which on a gradient destroys the
shape the lit-only number measures (0.178 through bytes, 1.000 through `f32` at 6×) — so the
gate wants the HDR buffer, not the swapchain. Also corrected in passing: the spec's bleed
phrasing — "the fraction of each lit cell's energy outside its footprint" — is not measurable
from a multi-cell image, since a pixel does not say which cell lit it; `bleed_max` is the
grid-readable form (a dark cell's luma over its lit neighbours' mean) and `spill_fraction`
answers the literal question for a one-cell render. The blur sweep the tests print calibrates
the defaults: `max_bleed 0.25` is about σ = 0.21 cells of halation, and `min_correlation 0.90`
trips only near a full cell.
