//! GGUF tensor **payload** reader + a deterministic 3-D projection of the vocabulary
//! embedding matrix (#507 Tier 1 — "the embedding galaxy").
//!
//! `gguf.rs` is deliberately a **pure header parser** — it reads the metadata KV map
//! and the tensor directory and stops, so opening a 30 GB model costs a few KB. This
//! module is the one place that crosses into the weights, and it keeps that property
//! honest in two ways:
//!
//! 1. **It never loads the whole matrix.** A 256k × 2048 Q6_K embedding table is
//!    hundreds of megabytes. We stride-sample token rows to [`GALAXY_MAX_POINTS`]
//!    (a *deterministic* stride, so the same model always samples the same tokens),
//!    fit the projection basis on a bounded sub-sample of those rows, and then stream
//!    the sampled rows one at a time through the basis. Peak resident cost is the
//!    basis sub-sample (capped at [`BASIS_MAX_FLOATS`] ≈ 32 MB) plus 3 floats per point.
//! 2. **It never invents data.** An embedding tensor in a `ggml_type` this module
//!    cannot dequantize is an `Err` with the type named, not a silently-wrong cloud.
//!    Supported: `F32`, `F16`, `Q8_0`, `Q4_0`, `Q4_K`, `Q6_K` — which covers
//!    essentially every `token_embd.weight` an LM Studio `.gguf` ships with.
//!
//! **The projection is a shadow, and we say so.** You cannot faithfully show 2048
//! dimensions in three; any 3-D view of an N-D vector discards almost everything.
//! What we *can* do is pick the *best* 3-D shadow — the top three principal
//! directions of the sampled embedding rows — and report how much of the variance it
//! actually captured ([`GalaxyProjection::explained`]) so the claim is auditable
//! rather than rhetorical. See PRD §4 principle 1 (every projection is labeled as a
//! projection) and §6 (the lens catalog).
//!
//! **Determinism is a hard requirement** (the same model must give the same galaxy on
//! every machine, every run): the basis starts from a fixed-seed SplitMix64 draw, runs
//! a fixed iteration count of subspace (block power) iteration with Gram–Schmidt
//! orthonormalization, and accumulates in `f64` in a fixed index order. No `HashMap`
//! iteration, no clock/rand seeding, no threads.
//!
//! Block-layout reference: llama.cpp's `ggml-common.h` (the `block_q*` structs) and
//! `ggml-quants.c` (`dequantize_row_q*`); the element/byte block sizes are the same
//! table [`crate::gguf::ggml_type_layout`] already carries.

use std::path::Path;

use crate::gguf::{ggml_type_layout, GgufHeader};

// ---------------------------------------------------------------------------
// Dequantization — the ggml block formats, element-for-element.
// ---------------------------------------------------------------------------

/// `ggml_type` tags this module can dequantize. Anything else is an honest error.
pub const SUPPORTED_TYPES: [u32; 6] = [
    0,  // F32
    1,  // F16
    2,  // Q4_0
    8,  // Q8_0
    12, // Q4_K
    14, // Q6_K
];

/// `true` if [`dequantize`] understands this `ggml_type`.
pub fn is_supported(ggml_type: u32) -> bool {
    SUPPORTED_TYPES.contains(&ggml_type)
}

/// Short name for a `ggml_type` — used in log lines and error messages so a
/// "unsupported" refusal names the format the user would have to re-quantize from.
pub fn ggml_type_name(ggml_type: u32) -> &'static str {
    match ggml_type {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        6 => "Q5_0",
        7 => "Q5_1",
        8 => "Q8_0",
        9 => "Q8_1",
        10 => "Q2_K",
        11 => "Q3_K",
        12 => "Q4_K",
        13 => "Q5_K",
        14 => "Q6_K",
        15 => "Q8_K",
        30 => "BF16",
        _ => "?",
    }
}

/// Read a little-endian IEEE half from two bytes.
#[inline]
fn f16_at(b: &[u8], i: usize) -> f32 {
    half::f16::from_bits(u16::from_le_bytes([b[i], b[i + 1]])).to_f32()
}

/// The K-quant super-block size (`QK_K` in ggml). Q4_K/Q6_K both block at 256.
const QK_K: usize = 256;

/// llama.cpp's `get_scale_min_k4`: unpack the 6-bit scale + 6-bit min for sub-block
/// `j` (0..8) out of a Q4_K block's 12 packed `scales` bytes. Getting this wrong is
/// the classic Q4_K dequant bug — the low four sub-blocks are stored plainly, the
/// high four steal their top two bits from the low four's spare bits.
#[inline]
fn q4k_scale_min(j: usize, q: &[u8]) -> (u8, u8) {
    if j < 4 {
        (q[j] & 63, q[j + 4] & 63)
    } else {
        (
            (q[j + 4] & 0x0F) | ((q[j - 4] >> 6) << 4),
            (q[j + 4] >> 4) | ((q[j] >> 6) << 4),
        )
    }
}

/// Dequantize `out.len()` elements of `ggml_type` from `src` into `out`.
///
/// `out.len()` must be a whole number of blocks for the type (which it always is for
/// a GGUF row: quantized tensors declare a row length that is a multiple of the block
/// size), and `src` must hold at least that many blocks' bytes. Returns an `Err`
/// describing the problem rather than filling `out` with plausible-looking garbage.
pub fn dequantize(ggml_type: u32, src: &[u8], out: &mut [f32]) -> Result<(), String> {
    let Some((block_elems, block_bytes)) = ggml_type_layout(ggml_type) else {
        return Err(format!("unknown ggml type {ggml_type}"));
    };
    if !is_supported(ggml_type) {
        return Err(format!(
            "ggml type {ggml_type} ({}) is not dequantizable here — supported: F32, F16, Q8_0, Q4_0, Q4_K, Q6_K",
            ggml_type_name(ggml_type)
        ));
    }
    let (be, bb) = (block_elems as usize, block_bytes as usize);
    if out.len() % be != 0 {
        return Err(format!("{} elements is not a whole number of {be}-element blocks", out.len()));
    }
    let blocks = out.len() / be;
    if src.len() < blocks * bb {
        return Err(format!("need {} bytes for {blocks} blocks, got {}", blocks * bb, src.len()));
    }

    match ggml_type {
        // F32 — raw little-endian floats.
        0 => {
            for (i, o) in out.iter_mut().enumerate() {
                let b = &src[i * 4..i * 4 + 4];
                *o = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
        }
        // F16 — raw little-endian halves.
        1 => {
            for (i, o) in out.iter_mut().enumerate() {
                *o = f16_at(src, i * 2);
            }
        }
        // Q4_0 — `{ half d; uint8_t qs[16]; }`, 32 elements per 18 bytes. Each byte
        // packs element j (low nibble) and element j+16 (high nibble), both biased by 8.
        2 => {
            for b in 0..blocks {
                let blk = &src[b * bb..b * bb + bb];
                let d = f16_at(blk, 0);
                let qs = &blk[2..18];
                let o = &mut out[b * be..b * be + be];
                for j in 0..16 {
                    o[j] = d * ((qs[j] & 0x0F) as i32 - 8) as f32;
                    o[j + 16] = d * ((qs[j] >> 4) as i32 - 8) as f32;
                }
            }
        }
        // Q8_0 — `{ half d; int8_t qs[32]; }`, 32 elements per 34 bytes.
        8 => {
            for b in 0..blocks {
                let blk = &src[b * bb..b * bb + bb];
                let d = f16_at(blk, 0);
                let o = &mut out[b * be..b * be + be];
                for (j, slot) in o.iter_mut().enumerate() {
                    *slot = d * (blk[2 + j] as i8) as f32;
                }
            }
        }
        // Q4_K — `{ half d; half dmin; uint8_t scales[12]; uint8_t qs[128]; }`,
        // 256 elements per 144 bytes: eight 32-element sub-blocks, each with its own
        // 6-bit scale and 6-bit min packed into `scales` (see `q4k_scale_min`).
        12 => {
            for b in 0..blocks {
                let blk = &src[b * bb..b * bb + bb];
                let d = f16_at(blk, 0);
                let dmin = f16_at(blk, 2);
                let scales = &blk[4..16];
                let qs = &blk[16..144];
                let o = &mut out[b * be..b * be + be];
                let mut is = 0usize;
                for j in (0..QK_K).step_by(64) {
                    let (sc1, m1) = q4k_scale_min(is, scales);
                    let (sc2, m2) = q4k_scale_min(is + 1, scales);
                    let (d1, min1) = (d * sc1 as f32, dmin * m1 as f32);
                    let (d2, min2) = (d * sc2 as f32, dmin * m2 as f32);
                    let q = &qs[j / 2..j / 2 + 32];
                    for l in 0..32 {
                        o[j + l] = d1 * (q[l] & 0x0F) as f32 - min1;
                        o[j + 32 + l] = d2 * (q[l] >> 4) as f32 - min2;
                    }
                    is += 2;
                }
            }
        }
        // Q6_K — `{ uint8_t ql[128]; uint8_t qh[64]; int8_t scales[16]; half d; }`,
        // 256 elements per 210 bytes: 6 bits per weight = 4 low bits in `ql` + 2 high
        // bits in `qh`, biased by 32, times an int8 per-16-element scale times `d`.
        14 => {
            for b in 0..blocks {
                let blk = &src[b * bb..b * bb + bb];
                let ql_all = &blk[0..128];
                let qh_all = &blk[128..192];
                let sc_all = &blk[192..208];
                let d = f16_at(blk, 208);
                let o = &mut out[b * be..b * be + be];
                for n in (0..QK_K).step_by(128) {
                    let ql = &ql_all[n / 2..];
                    let qh = &qh_all[n / 4..];
                    let sc = &sc_all[n / 16..];
                    for l in 0..32 {
                        let is = l / 16;
                        let q1 = ((ql[l] & 0x0F) | ((qh[l] & 3) << 4)) as i32 - 32;
                        let q2 = ((ql[l + 32] & 0x0F) | (((qh[l] >> 2) & 3) << 4)) as i32 - 32;
                        let q3 = ((ql[l] >> 4) | (((qh[l] >> 4) & 3) << 4)) as i32 - 32;
                        let q4 = ((ql[l + 32] >> 4) | (((qh[l] >> 6) & 3) << 4)) as i32 - 32;
                        o[n + l] = d * (sc[is] as i8) as f32 * q1 as f32;
                        o[n + l + 32] = d * (sc[is + 2] as i8) as f32 * q2 as f32;
                        o[n + l + 64] = d * (sc[is + 4] as i8) as f32 * q3 as f32;
                        o[n + l + 96] = d * (sc[is + 6] as i8) as f32 * q4 as f32;
                    }
                }
            }
        }
        _ => unreachable!("guarded by is_supported"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The embedding matrix — one memory-mapped tensor, read a row at a time.
// ---------------------------------------------------------------------------

/// A memory-mapped view of a model's `token_embd.weight` tensor. The mapping is lazy:
/// only the pages holding rows we actually ask for are ever faulted in, so sampling
/// 20k rows out of a 256k-row table touches tens of megabytes, not the whole tensor.
pub struct EmbeddingMatrix {
    mmap: memmap2::Mmap,
    /// Absolute byte offset of row 0 (`header.data_offset + tensor.offset`).
    base: usize,
    /// On-disk bytes per row (a whole number of quantization blocks).
    row_bytes: usize,
    /// Vocabulary size — the number of token rows.
    pub n_rows: usize,
    /// Embedding width (the model's `n_embd`) — the number of columns per row.
    pub n_dim: usize,
    /// The tensor's `ggml_type`.
    pub ggml_type: u32,
    /// The tensor's name as it appeared in the directory.
    pub name: String,
}

impl EmbeddingMatrix {
    /// Map the token-embedding tensor of an already-parsed model. `h` must be the
    /// header of *this* file (its `data_offset` is where the weights begin).
    ///
    /// The tensor is found by the same predicate `gguf.rs` uses for its vocab
    /// fallback: `token_embd.weight`, or anything ending in `tok_embeddings.weight`
    /// (the older llama naming).
    pub fn open(path: &Path, h: &GgufHeader) -> Result<EmbeddingMatrix, String> {
        let t = h
            .tensors
            .iter()
            .find(|t| t.name == "token_embd.weight" || t.name.ends_with("tok_embeddings.weight"))
            .ok_or_else(|| "no token_embd.weight tensor in this model".to_string())?;
        if t.dims.len() != 2 {
            return Err(format!("{} has {} dims, expected 2", t.name, t.dims.len()));
        }
        // GGML declares dims fastest-varying first: dims[0] = embedding width (the row),
        // dims[1] = vocabulary size (the row count).
        let n_dim = t.dims[0] as usize;
        let n_rows = t.dims[1] as usize;
        if n_dim == 0 || n_rows == 0 {
            return Err(format!("{} has an empty shape {:?}", t.name, t.dims));
        }
        let (be, bb) = ggml_type_layout(t.ggml_type)
            .ok_or_else(|| format!("{} has unknown ggml type {}", t.name, t.ggml_type))?;
        if !is_supported(t.ggml_type) {
            return Err(format!(
                "{} is {} — the galaxy dequantizer supports F32, F16, Q8_0, Q4_0, Q4_K, Q6_K \
                 (re-quantize the embedding, or use the specimen view)",
                t.name,
                ggml_type_name(t.ggml_type)
            ));
        }
        let (be, bb) = (be as usize, bb as usize);
        if n_dim % be != 0 {
            return Err(format!("{} row of {n_dim} is not a multiple of the {be}-element block", t.name));
        }
        let row_bytes = (n_dim / be) * bb;

        let file = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        // SAFETY: same contract as `ipc.rs` — a read-only mapping of a file we opened.
        // A concurrent truncation by another process would be UB, which is as true of
        // every other mmap in this crate; models are not written while being read.
        let mmap = unsafe { memmap2::Mmap::map(&file) }.map_err(|e| format!("mmap {}: {e}", path.display()))?;
        let base = (h.data_offset + t.offset) as usize;
        let need = base
            .checked_add(row_bytes.saturating_mul(n_rows))
            .ok_or_else(|| "tensor extent overflows".to_string())?;
        if need > mmap.len() {
            return Err(format!(
                "{} claims {need} bytes but the file is {} — truncated or a bad offset",
                t.name,
                mmap.len()
            ));
        }
        Ok(EmbeddingMatrix {
            mmap,
            base,
            row_bytes,
            n_rows,
            n_dim,
            ggml_type: t.ggml_type,
            name: t.name.clone(),
        })
    }

    /// Dequantize token row `r` into `out` (which must be `n_dim` long).
    pub fn row(&self, r: usize, out: &mut [f32]) -> Result<(), String> {
        if r >= self.n_rows {
            return Err(format!("row {r} out of range (n_rows {})", self.n_rows));
        }
        if out.len() != self.n_dim {
            return Err(format!("row buffer is {}, expected {}", out.len(), self.n_dim));
        }
        let start = self.base + r * self.row_bytes;
        dequantize(self.ggml_type, &self.mmap[start..start + self.row_bytes], out)
    }
}

// ---------------------------------------------------------------------------
// The deterministic 3-D basis (PCA by subspace iteration).
// ---------------------------------------------------------------------------

/// Fixed seed for the initial subspace. Changing it changes every galaxy, so it is a
/// constant, not a parameter.
pub const PCA_SEED: u64 = 0x5107_0117_9A1A_C1E5;
/// Fixed iteration count. Subspace iteration converges geometrically in the eigenvalue
/// gap; 12 is far past "visually settled" for embedding matrices and costs ~0.6 GFLOP
/// on a 4096 × 2048 sub-sample. Fixed (not a tolerance) so the result cannot drift.
pub const PCA_ITERS: usize = 12;

/// A mean vector + three orthonormal directions: the "best" 3-D shadow of an N-D point
/// cloud, in the least-squares sense. Held in `f64` so fit and projection agree exactly.
#[derive(Clone, Debug)]
pub struct Pca3 {
    /// Per-dimension mean of the fitted rows (the cloud is centered before projection).
    pub mean: Vec<f64>,
    /// The three orthonormal principal directions, most-variance first.
    pub basis: [Vec<f64>; 3],
    /// Variance along each direction (the eigenvalues of the covariance, estimated).
    pub eigen: [f64; 3],
    /// Fraction of the cloud's **total** variance each axis captures. This is the
    /// honesty number: `explained.sum()` is how much of the N-D structure the 3-D
    /// shadow keeps. Everything else was thrown away.
    pub explained: [f32; 3],
}

/// SplitMix64 — a tiny, exactly-specified PRNG. Used only to seed the initial
/// subspace; it must be bit-identical everywhere, which a 64-bit integer mixer is and
/// a floating-point generator would not necessarily be.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut s = 0.0;
    for i in 0..a.len().min(b.len()) {
        s += a[i] * b[i];
    }
    s
}

/// A deterministic unit vector orthogonal to everything in `prev` — the fallback when
/// a power iteration collapses (a rank-deficient cloud, e.g. all rows identical).
/// Walks the canonical axes in order, so it is reproducible.
fn fallback_axis(n_dim: usize, prev: &[Vec<f64>]) -> Vec<f64> {
    for cand in 0..n_dim {
        let mut u = vec![0.0f64; n_dim];
        u[cand] = 1.0;
        for p in prev {
            let d = dot(&u, p);
            for j in 0..n_dim {
                u[j] -= d * p[j];
            }
        }
        let n = dot(&u, &u).sqrt();
        if n > 1.0e-9 {
            for v in u.iter_mut() {
                *v /= n;
            }
            return u;
        }
    }
    vec![0.0f64; n_dim]
}

/// Gram–Schmidt the three candidate vectors into an orthonormal triple, returning the
/// pre-normalization norms (the un-scaled eigenvalue estimates).
fn orthonormalize3(v: &[Vec<f64>; 3], n_dim: usize) -> ([Vec<f64>; 3], [f64; 3]) {
    let mut out: Vec<Vec<f64>> = Vec::with_capacity(3);
    let mut norms = [0.0f64; 3];
    for k in 0..3 {
        let mut u = v[k].clone();
        for p in out.iter() {
            let d = dot(&u, p);
            for j in 0..n_dim {
                u[j] -= d * p[j];
            }
        }
        let n = dot(&u, &u).sqrt();
        norms[k] = n;
        if n > 1.0e-12 {
            for x in u.iter_mut() {
                *x /= n;
            }
        } else {
            u = fallback_axis(n_dim, &out);
        }
        out.push(u);
    }
    let mut it = out.into_iter();
    let (a, b, c) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
    ([a, b, c], norms)
}

/// Fit the top-3 principal directions of a row-major `n_rows × n_dim` matrix.
///
/// Method: mean-center, then **subspace (block power) iteration** — repeatedly apply
/// the covariance `Cᵀ = Xᵀ X` to a 3-vector block and re-orthonormalize. Both the seed
/// and the iteration count are fixed constants, the accumulation is `f64` in a fixed
/// index order, and there is no parallelism, so the output is bit-identical across
/// runs and machines. Pure — no I/O, unit-tested.
pub fn fit_pca3(rows: &[f32], n_rows: usize, n_dim: usize) -> Pca3 {
    let n_dim = n_dim.max(1);
    let n_rows = n_rows.min(if n_dim > 0 { rows.len() / n_dim } else { 0 });

    // Mean, then total variance (the denominator of `explained`).
    let mut mean = vec![0.0f64; n_dim];
    if n_rows > 0 {
        for i in 0..n_rows {
            let x = &rows[i * n_dim..(i + 1) * n_dim];
            for j in 0..n_dim {
                mean[j] += x[j] as f64;
            }
        }
        for m in mean.iter_mut() {
            *m /= n_rows as f64;
        }
    }
    let mut total_var = 0.0f64;
    for i in 0..n_rows {
        let x = &rows[i * n_dim..(i + 1) * n_dim];
        for j in 0..n_dim {
            let c = x[j] as f64 - mean[j];
            total_var += c * c;
        }
    }
    total_var /= (n_rows.max(1)) as f64;

    // Fixed-seed initial subspace, orthonormalized before the first application.
    let mut st = PCA_SEED;
    let mut v: [Vec<f64>; 3] = [vec![0.0; n_dim], vec![0.0; n_dim], vec![0.0; n_dim]];
    for k in 0..3 {
        for j in 0..n_dim {
            // Uniform in [-1, 1) from the top 53 bits — exact in f64, so identical
            // everywhere.
            let u = (splitmix64(&mut st) >> 11) as f64 / 9_007_199_254_740_992.0;
            v[k][j] = u * 2.0 - 1.0;
        }
    }
    let (mut v, _) = orthonormalize3(&v, n_dim);

    let mut eigen = [0.0f64; 3];
    if n_rows > 0 {
        for _ in 0..PCA_ITERS {
            let mut w: [Vec<f64>; 3] = [vec![0.0; n_dim], vec![0.0; n_dim], vec![0.0; n_dim]];
            for i in 0..n_rows {
                let x = &rows[i * n_dim..(i + 1) * n_dim];
                // t = Vᵀ (x − mean)
                let mut t = [0.0f64; 3];
                for j in 0..n_dim {
                    let c = x[j] as f64 - mean[j];
                    t[0] += c * v[0][j];
                    t[1] += c * v[1][j];
                    t[2] += c * v[2][j];
                }
                // W += (x − mean) tᵀ
                for j in 0..n_dim {
                    let c = x[j] as f64 - mean[j];
                    w[0][j] += t[0] * c;
                    w[1][j] += t[1] * c;
                    w[2][j] += t[2] * c;
                }
            }
            let (nv, norms) = orthonormalize3(&w, n_dim);
            v = nv;
            eigen = [
                norms[0] / n_rows as f64,
                norms[1] / n_rows as f64,
                norms[2] / n_rows as f64,
            ];
        }
    }

    let explained = if total_var > 0.0 {
        [
            (eigen[0] / total_var) as f32,
            (eigen[1] / total_var) as f32,
            (eigen[2] / total_var) as f32,
        ]
    } else {
        [0.0; 3]
    };
    Pca3 { mean, basis: v, eigen, explained }
}

/// Project one N-D row into the fitted 3-D basis (mean-centered).
pub fn project3(row: &[f32], p: &Pca3) -> [f32; 3] {
    let n = p.mean.len().min(row.len());
    let mut o = [0.0f64; 3];
    for j in 0..n {
        let c = row[j] as f64 - p.mean[j];
        o[0] += c * p.basis[0][j];
        o[1] += c * p.basis[1][j];
        o[2] += c * p.basis[2][j];
    }
    [o[0] as f32, o[1] as f32, o[2] as f32]
}

// ---------------------------------------------------------------------------
// The galaxy: sample → fit → stream-project.
// ---------------------------------------------------------------------------

/// How many token rows the galaxy samples. Every real vocabulary (32k–256k) is
/// stride-sampled down to at most this, deterministically, so the point cloud is
/// bounded no matter the model. ~20k points is dense enough to read as structure and
/// cheap enough to dequantize on the spot.
pub const GALAXY_MAX_POINTS: usize = 20_000;
/// Upper bound on the rows used to *fit* the basis (a sub-sample of the sampled rows).
const BASIS_MAX_ROWS: usize = 4096;
/// Hard cap on the fitting matrix in `f32`s (≈ 32 MB), so a wide model (`n_embd` 8192)
/// fits fewer rows rather than allocating hundreds of megabytes.
const BASIS_MAX_FLOATS: usize = 8 * 1024 * 1024;

/// The result of projecting a model's vocabulary embeddings into 3-D. Everything here
/// is measured or derived from the file — nothing is inferred, nothing is invented.
#[derive(Clone, Debug)]
pub struct GalaxyProjection {
    /// One projected point per sampled token, in basis units (not yet scene-scaled).
    pub points: Vec<[f32; 3]>,
    /// L2 norm of each sampled token's **full** N-D embedding vector. Real geometry
    /// the projection throws away, carried alongside it so the render can light by it.
    pub norms: Vec<f32>,
    /// The vocabulary index each point came from (the stride sample).
    pub token_ids: Vec<u32>,
    pub n_vocab: usize,
    pub n_dim: usize,
    /// The embedding tensor's quantization, e.g. `"Q6_K"`.
    pub quant: &'static str,
    /// How many rows the basis was fitted on.
    pub basis_rows: usize,
    /// Per-axis fraction of total variance captured — see [`Pca3::explained`].
    pub explained: [f32; 3],
}

impl GalaxyProjection {
    /// Fraction of the N-D variance the 3-D shadow keeps, in `[0,1]`. The single
    /// number that makes "this is a projection" checkable rather than a slogan.
    pub fn explained_total(&self) -> f32 {
        (self.explained[0] + self.explained[1] + self.explained[2]).clamp(0.0, 1.0)
    }

    /// A one-line log/HUD summary that states the projection *as* a projection.
    pub fn summary(&self) -> String {
        format!(
            "{} pts of {} tokens × {}-D {} — PCA(3) keeps {:.1}% of the variance (a shadow, not the space)",
            self.points.len(),
            self.n_vocab,
            self.n_dim,
            self.quant,
            self.explained_total() * 100.0
        )
    }
}

/// Deterministic stride sample of `0..n` down to at most `cap` indices.
fn stride_sample(n: usize, cap: usize) -> Vec<usize> {
    if n == 0 || cap == 0 {
        return Vec::new();
    }
    let stride = n.div_ceil(cap).max(1);
    (0..n).step_by(stride).take(cap).collect()
}

/// Read a model's `token_embd.weight`, project it into 3-D through a deterministic
/// PCA basis, and return the point cloud (#507 Tier 1).
///
/// Two passes, both bounded:
/// 1. **Fit** — dequantize a bounded sub-sample of the sampled rows into one matrix
///    and fit the top-3 principal directions ([`fit_pca3`]).
/// 2. **Project** — stream every sampled row through the basis one at a time, keeping
///    3 floats + a norm per point. The full matrix is never resident.
///
/// Deterministic for a given file: same stride, same sub-sample, same fixed-seed basis.
pub fn project_embedding_galaxy(
    path: &Path,
    h: &GgufHeader,
    max_points: usize,
) -> Result<GalaxyProjection, String> {
    let m = EmbeddingMatrix::open(path, h)?;
    let sampled = stride_sample(m.n_rows, max_points.max(1));
    if sampled.is_empty() {
        return Err("embedding matrix has no rows".to_string());
    }

    // ── Pass 1: fit the basis on a bounded sub-sample of the sampled rows. ──
    let budget = (BASIS_MAX_FLOATS / m.n_dim.max(1)).clamp(3, BASIS_MAX_ROWS);
    let fit_idx = stride_sample(sampled.len(), budget);
    let mut fit_rows: Vec<f32> = vec![0.0; fit_idx.len() * m.n_dim];
    for (slot, &si) in fit_idx.iter().enumerate() {
        let r = sampled[si];
        let off = slot * m.n_dim;
        m.row(r, &mut fit_rows[off..off + m.n_dim])?;
    }
    let pca = fit_pca3(&fit_rows, fit_idx.len(), m.n_dim);
    drop(fit_rows);

    // ── Pass 2: stream every sampled row through the basis. ──
    let mut points = Vec::with_capacity(sampled.len());
    let mut norms = Vec::with_capacity(sampled.len());
    let mut token_ids = Vec::with_capacity(sampled.len());
    let mut buf = vec![0.0f32; m.n_dim];
    for &r in &sampled {
        m.row(r, &mut buf)?;
        points.push(project3(&buf, &pca));
        let mut s = 0.0f64;
        for v in buf.iter() {
            s += (*v as f64) * (*v as f64);
        }
        norms.push(s.sqrt() as f32);
        token_ids.push(r as u32);
    }

    Ok(GalaxyProjection {
        points,
        norms,
        token_ids,
        n_vocab: m.n_rows,
        n_dim: m.n_dim,
        quant: ggml_type_name(m.ggml_type),
        basis_rows: fit_idx.len(),
        explained: pca.explained,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── dequantization: hand-built blocks, hand-computed expectations ─────────

    fn f16_bytes(v: f32) -> [u8; 2] {
        half::f16::from_f32(v).to_bits().to_le_bytes()
    }

    #[test]
    fn dequant_f32_and_f16_are_exact() {
        let src: Vec<u8> = [1.0f32, -2.5, 0.0, 1234.5].iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut out = [0.0f32; 4];
        dequantize(0, &src, &mut out).unwrap();
        assert_eq!(out, [1.0, -2.5, 0.0, 1234.5]);

        // F16 is exact for these values (all representable).
        let src: Vec<u8> = [1.0f32, -2.5, 0.5, 64.0].iter().flat_map(|v| f16_bytes(*v)).collect();
        let mut out = [0.0f32; 4];
        dequantize(1, &src, &mut out).unwrap();
        assert_eq!(out, [1.0, -2.5, 0.5, 64.0]);
    }

    #[test]
    fn dequant_q8_0_is_scale_times_int8() {
        // One block: d = 0.5, qs = [0, 1, -1, 127, -128, 2, 2, …].
        let mut blk = Vec::new();
        blk.extend_from_slice(&f16_bytes(0.5));
        let qs: [i8; 32] = {
            let mut q = [2i8; 32];
            q[0] = 0;
            q[1] = 1;
            q[2] = -1;
            q[3] = 127;
            q[4] = -128;
            q
        };
        blk.extend(qs.iter().map(|v| *v as u8));
        assert_eq!(blk.len(), 34);
        let mut out = [0.0f32; 32];
        dequantize(8, &blk, &mut out).unwrap();
        assert_eq!(out[0], 0.0);
        assert_eq!(out[1], 0.5);
        assert_eq!(out[2], -0.5);
        assert_eq!(out[3], 63.5);
        assert_eq!(out[4], -64.0);
        assert_eq!(out[31], 1.0);
    }

    #[test]
    fn dequant_q4_0_splits_nibbles_and_biases_by_eight() {
        // d = 2.0; qs[j] low nibble → element j, high nibble → element j+16, both −8.
        let mut blk = Vec::new();
        blk.extend_from_slice(&f16_bytes(2.0));
        let mut qs = [0u8; 16];
        qs[0] = 0x0F; // low 15 → (15−8)·2 = 14 ; high 0 → (0−8)·2 = −16
        qs[1] = 0x80; // low 0 → −16 ; high 8 → 0
        qs[15] = 0x9A; // low 10 → 4 ; high 9 → 2
        blk.extend_from_slice(&qs);
        assert_eq!(blk.len(), 18);
        let mut out = [0.0f32; 32];
        dequantize(2, &blk, &mut out).unwrap();
        assert_eq!(out[0], 14.0);
        assert_eq!(out[16], -16.0);
        assert_eq!(out[1], -16.0);
        assert_eq!(out[17], 0.0);
        assert_eq!(out[15], 4.0);
        assert_eq!(out[31], 2.0);
    }

    #[test]
    fn q4k_scale_min_unpacks_the_six_bit_pairs() {
        // Sub-blocks 0..3 read `scales[j] & 63` / `scales[j+4] & 63` plainly; sub-blocks
        // 4..7 splice the low four's top two bits onto the high four's low/high nibbles.
        let mut s = [0u8; 12];
        s[0] = 2 | (0b11 << 6); // sc0 = 2, and the spare 0b11 belongs to sub-block 4
        s[4] = 3 | (0b10 << 6); // m0  = 3, spare 0b10 belongs to sub-block 4's min
        s[8] = 0x56; // low nibble 6 → sc4 body, high nibble 5 → m4 body
        assert_eq!(q4k_scale_min(0, &s), (2, 3));
        // sc4 = (s[8] & 0xF) | ((s[0] >> 6) << 4) = 6 | 0b11_0000 = 54
        // m4  = (s[8] >> 4)  | ((s[4] >> 6) << 4) = 5 | 0b10_0000 = 37
        assert_eq!(q4k_scale_min(4, &s), (54, 37));
    }

    #[test]
    fn dequant_q4_k_applies_per_sub_block_scale_and_min() {
        // d = 1.0, dmin = 2.0. scales[0] = 2 → sub-block 0 scale 2; scales[4] = 3 → min 3.
        // scales[1] = 5 → sub-block 1 scale 5; scales[5] = 0 → min 0.
        let mut blk = vec![0u8; 144];
        blk[0..2].copy_from_slice(&f16_bytes(1.0));
        blk[2..4].copy_from_slice(&f16_bytes(2.0));
        blk[4] = 2; // sc0
        blk[5] = 5; // sc1
        blk[8] = 3; // m0
        blk[9] = 0; // m1
        // qs[0] = 0x71 → element 0 low nibble 1, element 32 high nibble 7.
        blk[16] = 0x71;
        // qs[31] = 0x0A → element 31 low nibble 10, element 63 high nibble 0.
        blk[16 + 31] = 0x0A;
        let mut out = [0.0f32; 256];
        dequantize(12, &blk, &mut out).unwrap();
        // element 0  = d·sc0·1 − dmin·m0 = 1·2·1 − 2·3 = −4
        assert_eq!(out[0], -4.0);
        // element 32 = d·sc1·7 − dmin·m1 = 1·5·7 − 0 = 35
        assert_eq!(out[32], 35.0);
        // element 31 = 1·2·10 − 6 = 14
        assert_eq!(out[31], 14.0);
        // element 63 = 1·5·0 − 0 = 0
        assert_eq!(out[63], 0.0);
        // Untouched sub-blocks have scale 0 → exactly 0 (mins are 0 there too).
        assert_eq!(out[128], 0.0);
    }

    #[test]
    fn dequant_q6_k_recombines_four_low_and_two_high_bits() {
        // d = 1.0, scales[0] = 2, the rest 1. ql[0] = 0x35, qh[0] = 0b00_01_10_11,
        // ql[32] = 0x07.
        let mut blk = vec![0u8; 210];
        blk[0] = 0x35; // ql[0]: low nibble 5, high nibble 3
        blk[32] = 0x07; // ql[32]: low nibble 7, high nibble 0
        blk[128] = 0b0001_1011; // qh[0]: pairs (from LSB) 3, 2, 1, 0
        for i in 0..16 {
            blk[192 + i] = 1;
        }
        blk[192] = 2; // scales[0]
        blk[208..210].copy_from_slice(&f16_bytes(1.0));
        let mut out = [0.0f32; 256];
        dequantize(14, &blk, &mut out).unwrap();
        // q1 = (5 | 3<<4) − 32 = 53 − 32 = 21 ; y[0]  = 1·scales[0]·21 = 42
        assert_eq!(out[0], 42.0);
        // q2 = (7 | 2<<4) − 32 = 39 − 32 = 7  ; y[32] = 1·scales[2]·7  = 7
        assert_eq!(out[32], 7.0);
        // q3 = (3 | 1<<4) − 32 = 19 − 32 = −13; y[64] = 1·scales[4]·−13 = −13
        assert_eq!(out[64], -13.0);
        // q4 = (0 | 0<<4) − 32 = −32          ; y[96] = 1·scales[6]·−32 = −32
        assert_eq!(out[96], -32.0);
    }

    #[test]
    fn unsupported_and_malformed_dequant_is_an_error_not_garbage() {
        let src = vec![0u8; 4096];
        let mut out = vec![0.0f32; 256];
        // Q5_K (13) is a real ggml type we deliberately do not implement.
        assert!(dequantize(13, &src, &mut out).is_err());
        // A type outside the table at all.
        assert!(dequantize(999, &src, &mut out).is_err());
        // Not a whole number of blocks.
        let mut short = vec![0.0f32; 100];
        assert!(dequantize(12, &src, &mut short).is_err());
        // Not enough source bytes.
        let tiny = vec![0u8; 8];
        assert!(dequantize(12, &tiny, &mut out).is_err());
    }

    // ── the projection basis ─────────────────────────────────────────────────

    /// A deterministic synthetic cloud: `n` rows in `d` dims, structured so the
    /// leading variance sits in a known low-dimensional subspace plus small noise.
    fn synth_rows(n: usize, d: usize) -> Vec<f32> {
        let mut rows = vec![0.0f32; n * d];
        for i in 0..n {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            for j in 0..d {
                let base = match j {
                    0 => 6.0 * t.cos(),
                    1 => 4.0 * t.sin(),
                    2 => 2.0 * (2.0 * t).sin(),
                    _ => 0.0,
                };
                rows[i * d + j] = base + 0.01 * ((i * 31 + j * 17) % 13) as f32;
            }
        }
        rows
    }

    #[test]
    fn pca_basis_is_bit_identical_across_runs() {
        let rows = synth_rows(200, 16);
        let a = fit_pca3(&rows, 200, 16);
        let b = fit_pca3(&rows, 200, 16);
        assert_eq!(a.mean, b.mean);
        for k in 0..3 {
            assert_eq!(a.basis[k], b.basis[k], "axis {k} drifted between runs");
        }
        assert_eq!(a.eigen, b.eigen);
        // …and the projection of a row is likewise identical.
        let r = &rows[7 * 16..8 * 16];
        assert_eq!(project3(r, &a), project3(r, &b));
    }

    #[test]
    fn pca_basis_is_orthonormal() {
        let rows = synth_rows(120, 24);
        let p = fit_pca3(&rows, 120, 24);
        for k in 0..3 {
            let n = dot(&p.basis[k], &p.basis[k]).sqrt();
            assert!((n - 1.0).abs() < 1.0e-9, "axis {k} norm {n}");
        }
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            let d = dot(&p.basis[a], &p.basis[b]).abs();
            assert!(d < 1.0e-9, "axes {a}/{b} not orthogonal: {d}");
        }
        // Ordered by variance, and the leading axes dominate a rank-3-ish cloud.
        assert!(p.eigen[0] >= p.eigen[1] && p.eigen[1] >= p.eigen[2]);
        let explained: f32 = p.explained.iter().sum();
        assert!(explained > 0.99, "explained {:?}", p.explained);
    }

    #[test]
    fn pca_recovers_a_known_plane() {
        // Rows spanning exactly the (e0, e1) plane: axis 0 and 1 must lie in it and the
        // third axis must carry ~no variance.
        let (n, d) = (64usize, 8usize);
        let mut rows = vec![0.0f32; n * d];
        for i in 0..n {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            rows[i * d] = 5.0 * t.cos();
            rows[i * d + 1] = 5.0 * t.sin();
        }
        let p = fit_pca3(&rows, n, d);
        // The first two axes live in the plane: their components outside dims 0/1 vanish.
        for k in 0..2 {
            let leak: f64 = (2..d).map(|j| p.basis[k][j].abs()).sum();
            assert!(leak < 1.0e-6, "axis {k} leaks {leak} out of the plane");
        }
        assert!(p.eigen[0] > 1.0 && p.eigen[1] > 1.0);
        assert!(p.eigen[2] < 1.0e-6, "third eigen {} should be ~0", p.eigen[2]);
    }

    #[test]
    fn degenerate_clouds_still_yield_an_orthonormal_basis() {
        // Every row identical → zero covariance. The fallback axes must still be a
        // valid orthonormal triple (not NaNs, not zeros).
        let rows = vec![3.0f32; 40 * 6];
        let p = fit_pca3(&rows, 40, 6);
        for k in 0..3 {
            let n = dot(&p.basis[k], &p.basis[k]).sqrt();
            assert!((n - 1.0).abs() < 1.0e-9, "axis {k} norm {n}");
        }
        assert_eq!(project3(&rows[0..6], &p), [0.0, 0.0, 0.0]);
        // Empty input is not a panic.
        let empty = fit_pca3(&[], 0, 4);
        assert_eq!(empty.mean.len(), 4);
    }

    #[test]
    fn stride_sampling_is_deterministic_and_bounded() {
        assert_eq!(stride_sample(10, 100).len(), 10); // cap above n → every row
        let s = stride_sample(1000, 100);
        assert_eq!(s.len(), 100);
        assert_eq!(s[0], 0);
        assert_eq!(s[1], 10);
        assert_eq!(stride_sample(1000, 100), stride_sample(1000, 100));
        assert!(stride_sample(0, 10).is_empty());
        assert!(stride_sample(10, 0).is_empty());
        // A vocabulary that doesn't divide evenly still stays under the cap.
        assert!(stride_sample(151_936, GALAXY_MAX_POINTS).len() <= GALAXY_MAX_POINTS);
    }

    // ── end-to-end: a synthetic .gguf on disk ────────────────────────────────

    /// Minimal GGUF v3 writer — just enough for an `F32` `token_embd.weight` whose
    /// payload we can predict. Mirrors the fixture builder in `gguf.rs`'s tests, plus
    /// the aligned tensor-data section this module actually reads.
    fn write_synthetic_gguf(path: &Path, n_vocab: usize, n_dim: usize, rows: &[f32]) {
        let mut buf: Vec<u8> = Vec::new();
        let mut kv: Vec<u8> = Vec::new();
        let gstr = |b: &mut Vec<u8>, s: &str| {
            b.extend_from_slice(&(s.len() as u64).to_le_bytes());
            b.extend_from_slice(s.as_bytes());
        };
        gstr(&mut kv, "general.architecture");
        kv.extend_from_slice(&8u32.to_le_bytes()); // STRING
        gstr(&mut kv, "llama");
        gstr(&mut kv, "llama.block_count");
        kv.extend_from_slice(&4u32.to_le_bytes()); // UINT32
        kv.extend_from_slice(&2u32.to_le_bytes());
        gstr(&mut kv, "llama.embedding_length");
        kv.extend_from_slice(&4u32.to_le_bytes());
        kv.extend_from_slice(&(n_dim as u32).to_le_bytes());

        let mut td: Vec<u8> = Vec::new();
        gstr(&mut td, "token_embd.weight");
        td.extend_from_slice(&2u32.to_le_bytes()); // 2 dims
        td.extend_from_slice(&(n_dim as u64).to_le_bytes());
        td.extend_from_slice(&(n_vocab as u64).to_le_bytes());
        td.extend_from_slice(&0u32.to_le_bytes()); // F32
        td.extend_from_slice(&0u64.to_le_bytes()); // offset within the data section

        buf.extend_from_slice(b"GGUF");
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&1u64.to_le_bytes()); // tensor count
        buf.extend_from_slice(&3u64.to_le_bytes()); // kv count
        buf.extend_from_slice(&kv);
        buf.extend_from_slice(&td);
        while buf.len() % 32 != 0 {
            buf.push(0); // pad to `general.alignment` (default 32)
        }
        for v in rows {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(path, &buf).unwrap();
    }

    fn tmp_path(tag: &str) -> std::path::PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("organon-galaxy-{tag}-{n}.gguf"))
    }

    #[test]
    fn reads_rows_out_of_a_real_file_at_the_aligned_data_offset() {
        let (n_vocab, n_dim) = (17usize, 8usize);
        let rows: Vec<f32> = (0..n_vocab * n_dim).map(|i| i as f32 * 0.25).collect();
        let p = tmp_path("rows");
        write_synthetic_gguf(&p, n_vocab, n_dim, &rows);
        let h = crate::gguf::parse_file(&p).unwrap();
        let m = EmbeddingMatrix::open(&p, &h).unwrap();
        assert_eq!((m.n_rows, m.n_dim, m.ggml_type), (n_vocab, n_dim, 0));
        let mut buf = vec![0.0f32; n_dim];
        m.row(0, &mut buf).unwrap();
        assert_eq!(buf, &rows[0..n_dim]);
        m.row(16, &mut buf).unwrap();
        assert_eq!(buf, &rows[16 * n_dim..17 * n_dim]);
        assert!(m.row(17, &mut buf).is_err()); // out of range, not a silent wrap
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn galaxy_projection_is_deterministic_and_bounded() {
        let (n_vocab, n_dim) = (300usize, 12usize);
        let rows = synth_rows(n_vocab, n_dim);
        let p = tmp_path("proj");
        write_synthetic_gguf(&p, n_vocab, n_dim, &rows);
        let h = crate::gguf::parse_file(&p).unwrap();

        let a = project_embedding_galaxy(&p, &h, 128).unwrap();
        let b = project_embedding_galaxy(&p, &h, 128).unwrap();
        // The stride is `ceil(300/128) = 3`, so the sample is `ceil(300/3) = 100` rows —
        // at most the cap, never more, and the same rows every time.
        assert_eq!(a.points.len(), 100);
        assert!(a.points.len() <= 128);
        assert_eq!(a.points, b.points, "the same file must give the same galaxy");
        assert_eq!(a.token_ids.len(), a.points.len());
        assert_eq!(a.norms.len(), a.points.len());
        assert_eq!(a.n_vocab, n_vocab);
        assert_eq!(a.n_dim, n_dim);
        assert_eq!(a.quant, "F32");
        assert!(a.explained_total() > 0.9, "explained {:?}", a.explained);
        assert!(a.summary().contains("shadow"));

        // Sampling every row (cap above the vocab) gives the whole vocabulary.
        let full = project_embedding_galaxy(&p, &h, 10_000).unwrap();
        assert_eq!(full.points.len(), n_vocab);
        assert_eq!(full.token_ids[0], 0);
        assert_eq!(*full.token_ids.last().unwrap(), (n_vocab - 1) as u32);
        // The carried norms are the FULL N-D vector lengths — finite and non-negative,
        // and (for this fixture, whose rows are non-zero) strictly positive.
        for (i, n) in full.norms.iter().enumerate() {
            assert!(n.is_finite() && *n > 0.0, "norm {n} at {i}");
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_model_without_a_token_embedding_is_an_honest_error() {
        // A header with no `token_embd.weight` must refuse, not fabricate a cloud.
        let h = crate::gguf::GgufHeader::default();
        let p = tmp_path("none");
        std::fs::write(&p, b"x").unwrap();
        let e = match EmbeddingMatrix::open(&p, &h) {
            Err(e) => e,
            Ok(_) => panic!("a header with no embedding tensor must not open"),
        };
        assert!(e.contains("token_embd"), "{e}");
        let _ = std::fs::remove_file(&p);
    }
}
