//! Activation-ring mmap protocol (#367 Tier 2 — the visible mind, live inference).
//!
//! A SEPARATE memory-mapped channel from `Shared`, carrying a running model's
//! per-token activations from a single writer to a single reader (the visual). It is
//! deliberately off `Shared` so Tier 2's model-free slice adds **no `Shared`
//! size/LAYOUT_VERSION change** — the `Live (streaming)` toggle rides the already-reserved
//! `Shared.mind[2]` slot, and this ring is its own file.
//!
//! Modeled exactly on `ipc.rs`'s `FeedbackWriter`/`FeedbackReader`: a small slot ring
//! in a fixed-size file, a monotonic `write_seq`, and a per-frame `seq`/`signature`
//! torn-read guard. The reader always takes the latest committed slot; a single
//! dropped frame is harmless (control-rate philosophy, same as `Shared`).
//!
//! Writers: the synthetic `organic-math-mind-writer` bin and the embedded llama.cpp
//! `organic-math-mind-runtime`. Readers: the visual's `topo == 5` graph-resolution
//! seam, which overwrites `NeuralGraph::node_scalar` from the latest frame so the #226
//! node-glow fires per token (no shader change), and the plugin editor's Mind-dashboard
//! (`mind_viz.rs`), which opens its own reader for the #482 Live-Telemetry widgets.
//!
//! **#482 Tier 2** grows `MindFrame` with the honest per-token next-token stats —
//! `entropy`, `confidence`, and a fixed-size **top-k** (id + prob + a decoded UTF-8
//! snippet). **#482 Tier 3** appends the **context / KV fuel gauge** (`ctx_used`,
//! `ctx_total`). Safe to grow: the ring is a transient `$TMPDIR` mmap recreated each
//! run (not `Shared`, not a saved artifact), and both writer and reader are ours.

use bytemuck::{Pod, Zeroable};
use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;

/// "MIND" — torn-read / wrong-file guard stamped in the ring header.
pub const MIND_RING_SIGNATURE: u32 = 0x4D_49_4E_44;

// ── `MindFrame.flags` bits ───────────────────────────────────────────────────
/// bit 0 — end of generation.
pub const FLAG_EOG: u32 = 0x1;
/// bit 1 (#522 Tier 1) — `layer_norm` holds **real** per-layer residual norms from the
/// `l_out-{N}` tensor tap, not the entropy proxy.
///
/// Carried in the frame rather than inferred, because the writer is the only party that
/// knows: a reader cannot tell a measured depth profile from a shaped one by looking at
/// the numbers, and guessing is exactly the dishonesty the provenance glyphs exist to
/// prevent. The #482 dashboard reads this to show `=` instead of `?`.
pub const FLAG_RESID_MEASURED: u32 = 0x2;
/// bit 2 (#522 Tier 1) — `mlp_act` holds **real** FFN-output norms (`ffn_out-{N}`).
///
/// Separate from [`FLAG_RESID_MEASURED`] because a capture can succeed for one and miss
/// the other on an unfamiliar graph, and a half-real frame should say so precisely.
pub const FLAG_MLP_MEASURED: u32 = 0x4;
/// Max transformer layers a frame carries (clamped by the writer).
pub const MR_MAX_LAYERS: usize = 64;
/// Max attention heads per layer a frame carries (clamped by the writer).
pub const MR_MAX_HEADS: usize = 64;
/// Ring depth. Small — the reader only ever wants the latest committed slot; the
/// extra slots just give the writer headroom so a reader mid-copy isn't clobbered.
pub const MR_SLOTS: usize = 4;
/// #482 Tier 2 — max next-token candidates a frame carries (the top-k bars).
pub const MR_TOPK: usize = 8;
/// #482 Tier 2 — bytes of decoded UTF-8 held per top-k candidate (a NUL-padded
/// snippet, truncated on a char boundary). Multiple of 4 to keep the frame's tail
/// arrays aligned with no `Pod` padding.
pub const MR_TOK_LEN: usize = 32;

// ── Phase B — the THREE-WAY append caps (#507 T2 / #505 T2 / #409 T2) ─────────
//
// These three blocks were laid out **in one sitting, before any of them was
// implemented**, precisely because a `MindFrame` offset mismatch is the failure
// mode with no symptom: writer and reader are separate binaries, so a wrong
// offset compiles, runs, and simply shows wrong numbers. See the block comments
// on the fields themselves for who owns what.
//
// **#541 S2 Tier 1 re-audited this and appended nothing.** S2's brief was to
// assign #505's expert routing and #507's `resid_proj` + top-k *together* so the
// two issues could not later fight over offsets — and Phase B had already done
// exactly that, plus #409's. Concretely: #505 Tier 2 owns Block B
// (`expert_count`/`expert_used`/`expert_id`/`expert_w`); #507 Tier 2 owns
// `resid_proj` and #507 Tier 3 owns the per-layer lens top-k (`lens_k`/`lens_id`/
// `lens_prob`) in Block A; the final-distribution top-k the model samples from is
// the older `topk_*` block. Nothing in S2 (the embedded viewport) or in the
// `Shared.mindview` pane selector needs a per-frame field, so the frame layout is
// unchanged and `frame_field_offsets_are_pinned` below is still the guard. A
// future `MindFrame` grower should append to the tail after Block C.

/// #507 Tier 3 — per-layer **logit-lens** candidates kept per frame.
///
/// Four, not eight: this is "what would it say if it stopped here", read across
/// 64 layers at once, so the interesting signal is *how fast the top-1 stabilizes*
/// up the stack, not a deep tail at every level. `MR_TOPK` (8) stays the richer
/// readout for the final distribution the model actually samples from.
pub const MR_LENS_K: usize = 4;

/// #505 Tier 2 — expert slots stored per layer.
///
/// **Sparse on purpose.** MoE models declare 8 (Mixtral) to 256 (DeepSeek-V3)
/// experts, but only `expert_used_count` fire per token — 2 to 8 across every
/// shipping family. Storing a dense `[layer][expert]` weight grid would cost
/// 64×256×4 = 64 KB a frame to carry ~8 non-zeros per layer. So the frame holds
/// the **fired** experts as (id, weight) pairs, which is also exactly what the
/// visual needs: #505 Tier 2 lights only the experts that fired.
pub const MR_MAX_EXPERTS_USED: usize = 8;

/// #409 Tier 2 — top firing SAE features carried per token.
///
/// An SAE dictionary is 16k–64k wide and ~100 features fire; 32 is the head of
/// that list, which is what the ticker and the semantic tint can actually show.
/// **Labels are not in the frame** — the frame carries ids, and the editor maps
/// id → name through the loaded feature-label corpus (a versioned artifact in its
/// own right, PRD §1.2/§13, not a blob to inline per token).
pub const MR_FEATURES: usize = 32;

/// One activation frame: a snapshot of the model's per-layer / per-(layer,head)
/// activity at the moment a single token was produced. Row-major head summaries so
/// `head_summ[l * MR_MAX_HEADS + h]` is layer `l`, head `h`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct MindFrame {
    /// This frame's monotonic id (0 = empty slot). Matches the ring's `write_seq`
    /// only when the slot is fully written (the torn-read guard).
    pub seq: u32,
    /// Which generated token produced this frame.
    pub token_index: u32,
    /// Active layer count, clamped to `MR_MAX_LAYERS` by the writer.
    pub n_layers: u32,
    /// Active head count, clamped to `MR_MAX_HEADS` by the writer.
    pub n_heads: u32,
    /// Bit flags: [`FLAG_EOG`] (end of generation), plus the #522 Tier 1 provenance bits
    /// [`FLAG_RESID_MEASURED`] / [`FLAG_MLP_MEASURED`] saying whether `layer_norm` /
    /// `mlp_act` are real tapped tensors or the entropy proxy. Unset bits are reserved.
    pub flags: u32,
    pub _pad: u32,
    /// Per-layer residual/activation norm.
    pub layer_norm: [f32; MR_MAX_LAYERS],
    /// Per-layer MLP activity.
    pub mlp_act: [f32; MR_MAX_LAYERS],
    /// Per-(layer,head) attention summary, row-major `[l * MR_MAX_HEADS + h]`.
    pub head_summ: [f32; MR_MAX_LAYERS * MR_MAX_HEADS],
    // ── #482 Tier 2 — the honest per-token next-token stats ──────────────────
    // Appended to the frame (the ring is a transient `$TMPDIR` mmap recreated each
    // run — not `Shared`, not a saved artifact — so growing `MindFrame` is safe; both
    // writer and reader are ours). 0 on frames from a writer that doesn't compute them.
    /// Next-token softmax **entropy**, normalized to `[0,1]` (1 = maximally uncertain).
    pub entropy: f32,
    /// Top token probability — the model's **confidence** — `[0,1]`.
    pub confidence: f32,
    /// Number of valid entries in the `topk_*` arrays (`≤ MR_TOPK`).
    pub topk_count: u32,
    /// Reserved (keeps the tail 4-byte aligned; a future measured/proxy flag).
    pub _pad2: u32,
    /// Top-k next-token candidate ids, descending by probability.
    pub topk_id: [u32; MR_TOPK],
    /// The candidates' probabilities `[0,1]`, descending, parallel to `topk_id`.
    pub topk_prob: [f32; MR_TOPK],
    /// Each candidate's decoded UTF-8 snippet, NUL-padded, row-major (`[k][byte]`).
    pub topk_text: [[u8; MR_TOK_LEN]; MR_TOPK],
    // ── #482 Tier 3 — the context / KV fuel gauge ────────────────────────────
    /// Tokens currently occupying the KV cache (prompt + generated so far).
    pub ctx_used: u32,
    /// The active context-window size (tokens). `0` ⇒ unknown / no gauge.
    pub ctx_total: u32,

    // ═════════════════════════════════════════════════════════════════════════
    // Phase B — the THREE-WAY append. Assigned together, once, BEFORE any of the
    // three was implemented (buildplan §5 / invariant #3).
    //
    // Why in one sitting: writer and reader are separate binaries reading the
    // same mmap by byte offset. If two issues each append independently, the two
    // layouts disagree and **nothing fails** — no compile error, no failing test,
    // just wrong numbers on screen. Reserving all three now means the offsets are
    // fixed before the first implementer can move them.
    //
    // Every block is **zero = absent**, so a writer that fills none of them (the
    // synthetic writer, or a runtime without the tap) produces exactly today's
    // behaviour, and each block can be implemented independently and land in any
    // order.
    // ═════════════════════════════════════════════════════════════════════════

    // ── Block A — #507 Tier 2/3: the residual trajectory + the logit lens ────
    /// Number of leading layers with a valid `resid_proj` entry. **0 ⇒ no
    /// trajectory** (the visual draws no path). Clamped to `MR_MAX_LAYERS`.
    pub resid_layers: u32,
    /// Valid per-layer entries in `lens_id`/`lens_prob`, `≤ MR_LENS_K`.
    /// **0 ⇒ no logit lens.**
    pub lens_k: u32,
    /// #507 Tier 2 — each layer's residual vector projected to 3-D through the
    /// **same basis the Tier-1 embedding galaxy uses** (`gguf_data`'s deterministic
    /// PCA), so the trajectory is drawn in the same space as the token cloud and
    /// the two are comparable. Row-major `[l*3 + axis]`.
    ///
    /// A projection, and labelled as one wherever it is displayed (PRD §4.1c) —
    /// the exact, un-projected quantities live in #507 Tier 4's geometry scalars.
    pub resid_proj: [f32; MR_MAX_LAYERS * 3],
    /// #507 Tier 3 — per-layer logit-lens candidate token ids, row-major
    /// `[l*MR_LENS_K + k]`, descending by probability within each layer.
    pub lens_id: [u32; MR_MAX_LAYERS * MR_LENS_K],
    /// The lens candidates' probabilities, parallel to `lens_id`.
    pub lens_prob: [f32; MR_MAX_LAYERS * MR_LENS_K],

    // ── Block B — #505 Tier 2: live sparse expert routing ────────────────────
    /// Total experts the model declares (`{arch}.expert_count`). **0 ⇒ dense**,
    /// and the whole block is inert. Carried so the visual knows the denominator
    /// (how dark the unlit bank should read) without re-reading the header.
    pub expert_count: u32,
    /// Valid `(expert_id, expert_w)` entries **per layer**, `≤ MR_MAX_EXPERTS_USED`.
    /// This is the model's `expert_used_count` (top-k routing), clamped.
    pub expert_used: u32,
    /// Which experts fired, row-major `[l*MR_MAX_EXPERTS_USED + i]`. Indices into
    /// the layer's expert bank, **not** node indices — the visual maps them.
    pub expert_id: [u32; MR_MAX_LAYERS * MR_MAX_EXPERTS_USED],
    /// Their router weights, parallel to `expert_id`. Descending within a layer.
    pub expert_w: [f32; MR_MAX_LAYERS * MR_MAX_EXPERTS_USED],

    // ── Block C — #409 Tier 2: SAE feature meaning ───────────────────────────
    /// Valid entries in `feat_id`/`feat_act`, `≤ MR_FEATURES`. **0 ⇒ no SAE
    /// loaded**, and the ticker / semantic tint stay dark.
    pub feat_count: u32,
    /// Which layer's residual the SAE encoded. An SAE is trained for one depth,
    /// so this is a property of the loaded dictionary, not of the token.
    pub feat_layer: u32,
    /// SAE **reconstruction error** for this token — the honest companion to the
    /// feature list (#409: the decomposition is lossy, and the shortfall is a
    /// number, so it is shown rather than omitted). `0` when no SAE is loaded.
    pub feat_recon_err: f32,
    /// Reserved — keeps the tail 4-byte aligned and leaves a flag slot for the
    /// label-provenance bit (#409: imported vs established-by-us) without another
    /// append.
    pub _pad3: u32,
    /// The top firing feature ids, descending by activation. Names are resolved
    /// editor-side through the feature-label corpus; ids are the wire format.
    pub feat_id: [u32; MR_FEATURES],
    /// Their activations, parallel to `feat_id`.
    pub feat_act: [f32; MR_FEATURES],
}

impl Default for MindFrame {
    fn default() -> Self {
        Self::zeroed()
    }
}

/// Copy a token snippet into a fixed `MR_TOK_LEN` slot: as many leading bytes of `s`
/// as fit **without splitting a UTF-8 char**, NUL-padding the rest. Shared by both
/// writers so the top-k text is encoded one way. (#482 Tier 2)
pub fn write_snippet(dst: &mut [u8; MR_TOK_LEN], s: &str) {
    *dst = [0u8; MR_TOK_LEN];
    let mut end = s.len().min(MR_TOK_LEN);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    dst[..end].copy_from_slice(&s.as_bytes()[..end]);
}

/// Decode a `MR_TOK_LEN` snippet slot back to a `String`: trim at the first NUL, then
/// lossy-decode (a torn/truncated write can't panic the display). (#482 Tier 2)
pub fn snippet_str(bytes: &[u8; MR_TOK_LEN]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(MR_TOK_LEN);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// The ring header + slot array in the mmap.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct MindRing {
    pub signature: u32,
    /// Total frames ever written; the latest lives in slot `(write_seq - 1) % MR_SLOTS`.
    pub write_seq: u32,
    pub n_slots: u32,
    /// `size_of::<MindFrame>()` as the writer understood it — the **layout guard**
    /// (was a spare `_pad`, always zero, so no existing ring is misread by adding it).
    ///
    /// Why this exists: the writer (`mind_runtime` / `mind_writer`) and the reader
    /// (the visual, and the editor's dashboard) are **separate binaries** indexing
    /// the same mmap by byte offset. A stale writer beside a fresh reader is not a
    /// crash — the frame stride silently differs and every field after the
    /// divergence reads as garbage that still looks like plausible floats. That is
    /// the exact failure mode the three-way append is most exposed to, so the
    /// reader now checks this and reports **no signal** instead of nonsense.
    pub frame_bytes: u32,
    pub frames: [MindFrame; MR_SLOTS],
}

impl Default for MindRing {
    fn default() -> Self {
        let mut r = Self::zeroed();
        r.signature = MIND_RING_SIGNATURE;
        r.n_slots = MR_SLOTS as u32;
        r.frame_bytes = std::mem::size_of::<MindFrame>() as u32;
        r
    }
}

const MR_SIZE: usize = std::mem::size_of::<MindRing>();

/// The activation-ring path (`$TMPDIR/organic-math-mind.bin`). Re-exported from
/// `ipc.rs` for symmetry with the other sidecars — use `ipc::mind_ring_path()`.
pub fn mind_ring_path() -> PathBuf {
    organon_core::ipc::mind_ring_path()
}

/// Ring writer (the synthetic bin now, the embedded runtime later). Created once,
/// then `write_frame` per token.
pub struct MindRingWriter {
    map: memmap2::MmapMut,
    seq: u32,
}

impl MindRingWriter {
    /// Create/truncate the ring file, zero it, and stamp `signature` + `n_slots`.
    pub fn create() -> io::Result<MindRingWriter> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(mind_ring_path())?;
        file.set_len(MR_SIZE as u64)?;
        // SAFETY: file is sized to MR_SIZE; we are the sole writer.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        map[..MR_SIZE].copy_from_slice(bytemuck::bytes_of(&MindRing::default()));
        Ok(MindRingWriter { map, seq: 0 })
    }

    /// Create the ring at an explicit path (used by tests for isolation).
    pub fn create_at(path: &std::path::Path) -> io::Result<MindRingWriter> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.set_len(MR_SIZE as u64)?;
        // SAFETY: file is sized to MR_SIZE; we are the sole writer.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        map[..MR_SIZE].copy_from_slice(bytemuck::bytes_of(&MindRing::default()));
        Ok(MindRingWriter { map, seq: 0 })
    }

    /// Publish a frame: bump `seq`, stamp it on the frame, write the frame slot,
    /// THEN publish `write_seq` — so a reader never observes a half-filled latest slot.
    pub fn write_frame(&mut self, f: &MindFrame) {
        self.seq = self.seq.wrapping_add(1);
        let slot = (self.seq as usize - 1) % MR_SLOTS;
        let mut frame = *f;
        frame.seq = self.seq;
        // Slot offset within the mmap: header, then frames[slot].
        let hdr = std::mem::size_of::<u32>() * 4;
        let fsize = std::mem::size_of::<MindFrame>();
        let base = hdr + slot * fsize;
        self.map[base..base + fsize].copy_from_slice(bytemuck::bytes_of(&frame));
        // Publish the new latest by bumping write_seq (offset 4 = second u32).
        self.map[4..8].copy_from_slice(bytemuck::bytes_of(&self.seq));
    }
}

/// Ring reader (the visual). `latest` returns `None` until the writer has created +
/// committed at least one frame; the visual re-opens lazily via `MindRingReader::open`.
pub struct MindRingReader {
    map: Option<memmap2::Mmap>,
}

impl MindRingReader {
    /// Best-effort open — returns a reader whose `latest()` yields `None` until the
    /// file exists and is at least `MR_SIZE` bytes.
    pub fn open() -> MindRingReader {
        Self::open_at(&mind_ring_path())
    }

    /// Open at an explicit path (used by tests for isolation).
    pub fn open_at(path: &std::path::Path) -> MindRingReader {
        let map = OpenOptions::new()
            .read(true)
            .open(path)
            .ok()
            .and_then(|f| {
                if f.metadata().map(|m| m.len() as usize >= MR_SIZE).unwrap_or(false) {
                    // SAFETY: file is at least MR_SIZE bytes.
                    unsafe { memmap2::Mmap::map(&f).ok() }
                } else {
                    None
                }
            });
        MindRingReader { map }
    }

    pub fn is_open(&self) -> bool {
        self.map.is_some()
    }

    /// The latest fully-committed frame, or `None` (no writer yet, wrong signature, empty
    /// ring, or a torn read where the slot's `seq` no longer matches `write_seq`).
    pub fn latest(&self) -> Option<MindFrame> {
        let m = self.map.as_ref()?;
        let ring: MindRing = bytemuck::pod_read_unaligned(&m[..MR_SIZE]);
        if ring.signature != MIND_RING_SIGNATURE {
            return None;
        }
        // Layout guard: a writer built against a different `MindFrame` lays the
        // slots out on a different stride, so every field past the divergence
        // would decode as plausible-looking garbage. Refuse the ring instead —
        // "no signal" is honest, wrong numbers are not. (A ring written before
        // this field existed has 0 here, which is also correctly refused: its
        // frames genuinely are a different size.)
        if ring.frame_bytes as usize != std::mem::size_of::<MindFrame>() {
            return None;
        }
        if ring.write_seq == 0 {
            return None;
        }
        let slot = (ring.write_seq as usize - 1) % MR_SLOTS;
        let frame = ring.frames[slot];
        // Torn-read guard: only trust the slot if it belongs to the published seq.
        (frame.seq == ring.write_seq).then_some(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "organic-math-mind-test-{}-{}.bin",
            std::process::id(),
            tag
        ))
    }

    fn frame(token: u32, n_layers: u32, n_heads: u32) -> MindFrame {
        let mut f = MindFrame::default();
        f.token_index = token;
        f.n_layers = n_layers;
        f.n_heads = n_heads;
        for l in 0..n_layers as usize {
            f.layer_norm[l] = 0.1 * (l as f32 + 1.0) + token as f32;
            f.mlp_act[l] = 0.2 * (l as f32 + 1.0) + token as f32;
            for h in 0..n_heads as usize {
                f.head_summ[l * MR_MAX_HEADS + h] = (l * 10 + h) as f32 + token as f32;
            }
        }
        f
    }

    #[test]
    fn round_trip_latest_is_third() {
        let path = tmp_path("rt");
        let _ = std::fs::remove_file(&path);
        {
            let mut w = MindRingWriter::create_at(&path).unwrap();
            w.write_frame(&frame(0, 4, 3));
            w.write_frame(&frame(1, 4, 3));
            w.write_frame(&frame(2, 4, 3));
        }
        let r = MindRingReader::open_at(&path);
        let got = r.latest().expect("latest present");
        assert_eq!(got.token_index, 2, "3rd frame is latest");
        assert_eq!(got.seq, 3, "write_seq monotonic → 3 frames");
        assert_eq!(got.n_layers, 4);
        assert_eq!(got.n_heads, 3);
        // Norms match the 3rd frame (token 2).
        assert!((got.layer_norm[0] - (0.1 + 2.0)).abs() < 1e-5);
        assert!((got.head_summ[MR_MAX_HEADS + 1] - (11.0 + 2.0)).abs() < 1e-5);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn monotonic_write_seq() {
        let path = tmp_path("mono");
        let _ = std::fs::remove_file(&path);
        let mut w = MindRingWriter::create_at(&path).unwrap();
        for i in 0..10u32 {
            w.write_frame(&frame(i, 2, 2));
            let r = MindRingReader::open_at(&path);
            let got = r.latest().unwrap();
            assert_eq!(got.seq, i + 1, "write_seq advances by one per frame");
            assert_eq!(got.token_index, i);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_file_yields_none() {
        let path = tmp_path("empty");
        let _ = std::fs::remove_file(&path);
        // Create a correctly-sized but never-written (write_seq == 0) ring.
        let _w = MindRingWriter::create_at(&path).unwrap();
        let r = MindRingReader::open_at(&path);
        assert!(r.latest().is_none(), "write_seq == 0 → None");
        // A totally missing file → open ok, latest None.
        let missing = tmp_path("missing");
        let _ = std::fs::remove_file(&missing);
        let r2 = MindRingReader::open_at(&missing);
        assert!(!r2.is_open());
        assert!(r2.latest().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn topk_and_stats_round_trip() {
        let path = tmp_path("topk");
        let _ = std::fs::remove_file(&path);
        let mut f = frame(0, 4, 3);
        f.entropy = 0.42;
        f.confidence = 0.73;
        f.topk_count = 3;
        f.topk_id = [10, 11, 12, 0, 0, 0, 0, 0];
        f.topk_prob = [0.5, 0.3, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0];
        f.ctx_used = 137;
        f.ctx_total = 4096;
        write_snippet(&mut f.topk_text[0], "the");
        write_snippet(&mut f.topk_text[1], " a");
        // Over-long + multi-byte: must truncate on a char boundary, never split.
        write_snippet(&mut f.topk_text[2], &"é".repeat(40));
        {
            let mut w = MindRingWriter::create_at(&path).unwrap();
            w.write_frame(&f);
        }
        let got = MindRingReader::open_at(&path).latest().expect("frame");
        assert!((got.entropy - 0.42).abs() < 1e-6);
        assert!((got.confidence - 0.73).abs() < 1e-6);
        assert_eq!(got.topk_count, 3);
        assert_eq!(got.topk_id[1], 11);
        assert!((got.topk_prob[0] - 0.5).abs() < 1e-6);
        assert_eq!(got.ctx_used, 137);
        assert_eq!(got.ctx_total, 4096);
        assert_eq!(snippet_str(&got.topk_text[0]), "the");
        assert_eq!(snippet_str(&got.topk_text[1]), " a");
        // 32 bytes / 2 bytes-per-'é' = 16 chars, cleanly (no replacement char).
        let s = snippet_str(&got.topk_text[2]);
        assert_eq!(s, "é".repeat(16));
        assert!(!s.contains('\u{FFFD}'), "never splits a UTF-8 char");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn torn_seq_mismatch_yields_none() {
        let path = tmp_path("torn");
        let _ = std::fs::remove_file(&path);
        {
            let mut w = MindRingWriter::create_at(&path).unwrap();
            w.write_frame(&frame(0, 2, 2));
        }
        // Corrupt the latest slot's seq to simulate a torn write (slot written but
        // stale) — the reader must reject it.
        let mut ring: MindRing = {
            let bytes = std::fs::read(&path).unwrap();
            bytemuck::pod_read_unaligned(&bytes[..MR_SIZE])
        };
        assert_eq!(ring.write_seq, 1);
        ring.frames[0].seq = 999; // mismatch vs write_seq == 1
        std::fs::write(&path, bytemuck::bytes_of(&ring)).unwrap();
        let r = MindRingReader::open_at(&path);
        assert!(r.latest().is_none(), "seq mismatch → torn read rejected");
        // ⚠️ WINDOWS: this reader must die before the file is rewritten below.
        // `fs::write` truncates, and truncating a file that has a live mapped section
        // fails there with ERROR_USER_MAPPED_FILE (os error 1224) — a thing Unix
        // permits silently, which is why this stood for as long as it did.
        //
        // **Test-only hazard, not a product one.** The real writer publishes THROUGH
        // its own mapping and never truncates a file a reader holds; only a test that
        // fabricates a corrupt ring reaches for `fs::write`. So the fix belongs here,
        // not in `MindRingWriter`.
        //
        // Note what makes it a Windows problem specifically: the two sibling tests that
        // do the same rewrite (`frame_size_mismatch_is_refused_not_misread`) pass,
        // because they open the reader as a *temporary* inside `assert!` and it drops
        // at the end of the statement. This one binds it to `r`, so it lives to the end
        // of the scope. Found by the very first run of the `build (windows)` CI leg
        // (organon#658 Tier 1). The drop is a no-op on Unix.
        drop(r);
        // A wrong signature also rejects.
        ring.frames[0].seq = 1;
        ring.signature = 0;
        std::fs::write(&path, bytemuck::bytes_of(&ring)).unwrap();
        let r2 = MindRingReader::open_at(&path);
        assert!(r2.latest().is_none(), "bad signature → None");
        // Same rule for the delete: Windows refuses to unlink a file with a live
        // mapped section. `let _ =` would swallow that, so leaving it would leak one
        // stray file into `%TEMP%` per run rather than fail — quiet, but still wrong.
        drop(r2);
        let _ = std::fs::remove_file(&path);
    }

    // ── Phase B: the three-way append ────────────────────────────────────────

    /// **The load-bearing test of the whole append.**
    ///
    /// A `MindFrame` offset mistake has no symptom — writer and reader are separate
    /// binaries, so inserting a field instead of appending one still compiles, still
    /// runs, and just shows wrong numbers. Pinning every offset is what converts that
    /// into a failing test.
    ///
    /// If this fails, do **not** update the numbers to match: a changed offset means
    /// a field was inserted or reordered rather than appended, which is invariant #3.
    /// Append to the tail instead.
    #[test]
    fn frame_field_offsets_are_pinned() {
        use std::mem::offset_of;
        // Pre-existing fields — these offsets are load-bearing across the
        // plugin↔visual boundary and must never move.
        assert_eq!(offset_of!(MindFrame, seq), 0);
        assert_eq!(offset_of!(MindFrame, token_index), 4);
        assert_eq!(offset_of!(MindFrame, n_layers), 8);
        assert_eq!(offset_of!(MindFrame, n_heads), 12);
        assert_eq!(offset_of!(MindFrame, flags), 16);
        assert_eq!(offset_of!(MindFrame, layer_norm), 24);
        assert_eq!(offset_of!(MindFrame, mlp_act), 280);
        assert_eq!(offset_of!(MindFrame, head_summ), 536);
        assert_eq!(offset_of!(MindFrame, entropy), 16920);
        assert_eq!(offset_of!(MindFrame, confidence), 16924);
        assert_eq!(offset_of!(MindFrame, topk_count), 16928);
        assert_eq!(offset_of!(MindFrame, topk_id), 16936);
        assert_eq!(offset_of!(MindFrame, topk_prob), 16968);
        assert_eq!(offset_of!(MindFrame, topk_text), 17000);
        assert_eq!(offset_of!(MindFrame, ctx_used), 17256);
        assert_eq!(offset_of!(MindFrame, ctx_total), 17260);
        // The three-way append starts here — everything above is untouched.
        assert_eq!(
            offset_of!(MindFrame, resid_layers),
            17264,
            "the append must begin exactly at the old frame size"
        );
        // Block A (#507), Block B (#505), Block C (#409), in that order.
        assert_eq!(offset_of!(MindFrame, lens_k), 17268);
        assert_eq!(offset_of!(MindFrame, resid_proj), 17272);
        assert_eq!(offset_of!(MindFrame, lens_id), 18040);
        assert_eq!(offset_of!(MindFrame, lens_prob), 19064);
        assert_eq!(offset_of!(MindFrame, expert_count), 20088);
        assert_eq!(offset_of!(MindFrame, expert_used), 20092);
        assert_eq!(offset_of!(MindFrame, expert_id), 20096);
        assert_eq!(offset_of!(MindFrame, expert_w), 22144);
        assert_eq!(offset_of!(MindFrame, feat_count), 24192);
        assert_eq!(offset_of!(MindFrame, feat_layer), 24196);
        assert_eq!(offset_of!(MindFrame, feat_recon_err), 24200);
        assert_eq!(offset_of!(MindFrame, feat_id), 24208);
        assert_eq!(offset_of!(MindFrame, feat_act), 24336);
    }

    /// `Pod` forbids padding bytes, but a wrong cap could still make the struct a
    /// size nobody intended. Pin the total, and pin that it is the sum of its parts.
    #[test]
    fn frame_size_is_exactly_its_fields() {
        let want = 17264                                    // everything before Phase B
            + 4 + 4 + MR_MAX_LAYERS * 3 * 4                 // resid_layers, lens_k, resid_proj
            + MR_MAX_LAYERS * MR_LENS_K * 4 * 2             // lens_id + lens_prob
            + 4 + 4 + MR_MAX_LAYERS * MR_MAX_EXPERTS_USED * 4 * 2 // expert block
            + 4 * 4 + MR_FEATURES * 4 * 2; // feature block (4 scalars + 2 arrays)
        assert_eq!(std::mem::size_of::<MindFrame>(), want);
        assert_eq!(std::mem::size_of::<MindFrame>(), 24464);
    }

    /// Every appended block is **zero = absent**, so a writer that fills none of
    /// them behaves exactly as it did before the append. This is what lets the three
    /// blocks be implemented independently and land in any order.
    #[test]
    fn appended_blocks_default_to_absent() {
        let f = MindFrame::default();
        assert_eq!(f.resid_layers, 0, "no trajectory");
        assert_eq!(f.lens_k, 0, "no logit lens");
        assert_eq!(f.expert_count, 0, "dense model");
        assert_eq!(f.expert_used, 0);
        assert_eq!(f.feat_count, 0, "no SAE loaded");
        assert_eq!(f.feat_recon_err, 0.0);
        assert!(f.resid_proj.iter().all(|&v| v == 0.0));
        assert!(f.expert_w.iter().all(|&v| v == 0.0));
        assert!(f.feat_act.iter().all(|&v| v == 0.0));
    }

    /// All three blocks survive a real writer→reader round trip through the mmap.
    /// Written as one test because the point is that the blocks coexist at their
    /// assigned offsets — filling them one at a time would not catch an overlap.
    #[test]
    fn three_way_append_round_trips() {
        let path = tmp_path("append3");
        let _ = std::fs::remove_file(&path);
        let mut f = frame(7, 4, 3);
        // Block A — #507: a 4-layer trajectory + a 2-wide lens.
        f.resid_layers = 4;
        f.lens_k = 2;
        for l in 0..4usize {
            f.resid_proj[l * 3] = l as f32;
            f.resid_proj[l * 3 + 1] = l as f32 + 0.5;
            f.resid_proj[l * 3 + 2] = -(l as f32);
            for k in 0..2usize {
                f.lens_id[l * MR_LENS_K + k] = (100 + l * 10 + k) as u32;
                f.lens_prob[l * MR_LENS_K + k] = 0.9 - 0.1 * k as f32;
            }
        }
        // Block B — #505: 8-expert model, top-2 routing.
        f.expert_count = 8;
        f.expert_used = 2;
        for l in 0..4usize {
            for i in 0..2usize {
                f.expert_id[l * MR_MAX_EXPERTS_USED + i] = (l + i) as u32;
                f.expert_w[l * MR_MAX_EXPERTS_USED + i] = 0.75 - 0.25 * i as f32;
            }
        }
        // Block C — #409: three features firing at layer 12.
        f.feat_count = 3;
        f.feat_layer = 12;
        f.feat_recon_err = 0.125;
        for i in 0..3usize {
            f.feat_id[i] = (41200 + i) as u32;
            f.feat_act[i] = 1.5 - 0.5 * i as f32;
        }
        {
            let mut w = MindRingWriter::create_at(&path).unwrap();
            w.write_frame(&f);
        }
        let got = MindRingReader::open_at(&path)
            .latest()
            .expect("frame present");

        // The pre-existing fields still read correctly — an overlapping append
        // would corrupt these, not just the new ones.
        assert_eq!(got.token_index, 7);
        assert_eq!(got.ctx_total, f.ctx_total);
        assert_eq!(got.layer_norm[0], f.layer_norm[0]);

        assert_eq!(got.resid_layers, 4);
        assert_eq!(got.lens_k, 2);
        assert_eq!(got.resid_proj[3 * 3 + 1], 3.5);
        assert_eq!(got.lens_id[2 * MR_LENS_K + 1], 121);
        assert_eq!(got.lens_prob[0], 0.9);

        assert_eq!(got.expert_count, 8);
        assert_eq!(got.expert_used, 2);
        assert_eq!(got.expert_id[3 * MR_MAX_EXPERTS_USED + 1], 4);
        assert_eq!(got.expert_w[0], 0.75);

        assert_eq!(got.feat_count, 3);
        assert_eq!(got.feat_layer, 12);
        assert_eq!(got.feat_recon_err, 0.125);
        assert_eq!(got.feat_id[2], 41202);
        assert_eq!(got.feat_act[0], 1.5);

        let _ = std::fs::remove_file(&path);
    }

    /// The layout guard: a ring whose `frame_bytes` disagrees with this build is
    /// refused outright. Without it, a stale writer beside a fresh reader decodes
    /// every field past the divergence as plausible-looking garbage — the silent
    /// failure the three-way append is most exposed to.
    #[test]
    fn frame_size_mismatch_is_refused_not_misread() {
        let path = tmp_path("guard");
        let _ = std::fs::remove_file(&path);
        {
            let mut w = MindRingWriter::create_at(&path).unwrap();
            w.write_frame(&frame(1, 4, 3));
        }
        assert!(
            MindRingReader::open_at(&path).latest().is_some(),
            "sanity: a matching ring reads"
        );
        let mut ring: MindRing = {
            let bytes = std::fs::read(&path).unwrap();
            bytemuck::pod_read_unaligned(&bytes[..MR_SIZE])
        };
        assert_eq!(ring.frame_bytes as usize, std::mem::size_of::<MindFrame>());
        // Simulate a writer built against a different frame layout.
        ring.frame_bytes += 4;
        std::fs::write(&path, bytemuck::bytes_of(&ring)).unwrap();
        assert!(
            MindRingReader::open_at(&path).latest().is_none(),
            "frame-size mismatch → no signal, never wrong numbers"
        );
        // A ring predating the guard (0) is refused for the same reason.
        ring.frame_bytes = 0;
        std::fs::write(&path, bytemuck::bytes_of(&ring)).unwrap();
        assert!(MindRingReader::open_at(&path).latest().is_none());
        let _ = std::fs::remove_file(&path);
    }
}
