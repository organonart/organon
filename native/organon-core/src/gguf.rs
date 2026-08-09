//! Pure-Rust GGUF **header** parser (#367 Tier 1 — the specimen).
//!
//! Reads only a GGUF file's *metadata + tensor directory* — never a single weight.
//! GGUF (the llama.cpp / GGML container) begins with a fixed header, a key/value
//! metadata map, and a tensor directory (name, shape, type, byte offset); the raw
//! tensor payload follows, aligned, at the end. We stop reading the moment the
//! directory is consumed, so opening a 30 GB model costs a few KB of I/O and no
//! large allocation. Metadata **arrays** (e.g. the 100k-entry tokenizer vocabulary)
//! are *skipped* — we record only their length, never their contents.
//!
//! The parser is generic over the architecture: it parses the KV map into a
//! `BTreeMap<String, GgufValue>`, reads `general.architecture` to learn the arch
//! prefix (`llama`, `qwen2`, `gemma`, …), and looks up the arch-prefixed dims
//! (`{arch}.block_count`, `{arch}.attention.head_count`, `{arch}.embedding_length`,
//! …). Vocab size is taken from `{arch}.vocab_size`, else the tokenizer token
//! array length, else the `token_embd.weight` tensor's first dimension.
//!
//! Format reference: <https://github.com/ggml-org/ggml/blob/master/docs/gguf.md>.
//! Supports the little-endian v2/v3 layout (u64 counts) that every real model uses.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::Path;

/// GGUF metadata value type tags (`gguf_metadata_value_type`).
mod vtype {
    pub const UINT8: u32 = 0;
    pub const INT8: u32 = 1;
    pub const UINT16: u32 = 2;
    pub const INT16: u32 = 3;
    pub const UINT32: u32 = 4;
    pub const INT32: u32 = 5;
    pub const FLOAT32: u32 = 6;
    pub const BOOL: u32 = 7;
    pub const STRING: u32 = 8;
    pub const ARRAY: u32 = 9;
    pub const UINT64: u32 = 10;
    pub const INT64: u32 = 11;
    pub const FLOAT64: u32 = 12;
}

/// A parsed GGUF metadata value. Scalar values are kept; arrays keep their element
/// type + length, and **small integer/bool arrays also keep their elements** (up to
/// [`MAX_ARRAY_VALUES`]) because some architectures declare per-layer head geometry
/// as an array (Gemma-4's `attention.head_count_kv`). String and oversized arrays
/// keep only type + length, so a giant tokenizer vocabulary still costs nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum GgufValue {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    String(String),
    /// `elem_type` is a `vtype::*` tag; `len` is the element count. `values` holds the
    /// elements for small integer/bool arrays (see [`MAX_ARRAY_VALUES`]) and is empty
    /// for string arrays, float arrays, and arrays too long to be head geometry.
    Array { elem_type: u32, len: u64, values: Vec<i64> },
}

/// The longest array whose elements we retain. Per-layer geometry is one entry per
/// block (tens); tokenizer arrays are tens of thousands and must stay dropped.
pub const MAX_ARRAY_VALUES: u64 = 4096;

impl GgufValue {
    /// Interpret any integer/bool value as `u64` (for dims/counts). `None` for
    /// strings/floats/arrays.
    pub fn as_u64(&self) -> Option<u64> {
        Some(match self {
            GgufValue::U8(v) => *v as u64,
            GgufValue::I8(v) => *v as u64,
            GgufValue::U16(v) => *v as u64,
            GgufValue::I16(v) => *v as u64,
            GgufValue::U32(v) => *v as u64,
            GgufValue::I32(v) => *v as u64,
            GgufValue::U64(v) => *v,
            GgufValue::I64(v) => *v as u64,
            GgufValue::Bool(b) => *b as u64,
            _ => return None,
        })
    }

    /// Interpret any integer/bool value as `i64`, sign-preserving. `None` for
    /// strings/floats/arrays.
    pub fn as_i64(&self) -> Option<i64> {
        Some(match self {
            GgufValue::U8(v) => *v as i64,
            GgufValue::I8(v) => *v as i64,
            GgufValue::U16(v) => *v as i64,
            GgufValue::I16(v) => *v as i64,
            GgufValue::U32(v) => *v as i64,
            GgufValue::I32(v) => *v as i64,
            GgufValue::U64(v) => *v as i64,
            GgufValue::I64(v) => *v,
            GgufValue::Bool(b) => *b as i64,
            _ => return None,
        })
    }

    /// The retained elements of a small integer/bool array, as `u64`s. `None` for a
    /// non-array, or for an array whose elements were not retained (string/float/too
    /// long) — which the caller must treat as *unknown*, not as empty.
    pub fn as_u64_vec(&self) -> Option<Vec<u64>> {
        match self {
            GgufValue::Array { len, values, .. } if values.len() as u64 == *len => {
                Some(values.iter().map(|v| *v as u64).collect())
            }
            _ => None,
        }
    }

    /// The string payload of a `String` value, else `None`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            GgufValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The element count of an `Array` value, else `None`.
    pub fn arr_len(&self) -> Option<u64> {
        match self {
            GgufValue::Array { len, .. } => Some(*len),
            _ => None,
        }
    }
}

/// One entry of the tensor directory — shape + type + offset, **no data**.
#[derive(Debug, Clone, PartialEq)]
pub struct GgufTensor {
    pub name: String,
    /// Dimensions as declared (GGML order: fastest-varying first).
    pub dims: Vec<u64>,
    /// GGML type tag (`ggml_type`; 0 = F32, 1 = F16, 2.. = quantized).
    pub ggml_type: u32,
    /// Byte offset of the tensor data within the tensor-data section.
    pub offset: u64,
}

impl GgufTensor {
    /// Declared element count = product of the dimensions (empty shape → 1).
    pub fn n_elements(&self) -> u64 {
        self.dims.iter().copied().product::<u64>().max(if self.dims.is_empty() { 1 } else { 0 })
    }

    /// On-disk byte size of this tensor's payload, from its `ggml_type` block
    /// layout (§ [`ggml_type_layout`]). The honest per-tensor size llama.cpp would
    /// allocate: `n_elements / block_elems · block_bytes`. For an unrecognised type
    /// we fall back to a **2-bytes-per-element** estimate (labeled by
    /// [`GgufHeader::has_unknown_quant`] so the atlas can flag it) rather than lying
    /// with a zero.
    pub fn byte_size(&self) -> u64 {
        let n = self.n_elements();
        match ggml_type_layout(self.ggml_type) {
            Some((be, bb)) => {
                // Round the element count up to a whole block (GGUF pads to block).
                let blocks = n.div_ceil(be.max(1));
                blocks.saturating_mul(bb)
            }
            None => n.saturating_mul(2),
        }
    }
}

// ---------------------------------------------------------------------------
// #423 Tier 1 — storage geometry from the header alone (no inference, no weights).
//
// The GGUF tensor directory declares each tensor's `ggml_type` (its quantization
// format). Every ggml type stores a fixed number of weights per *block* in a fixed
// number of *bytes* — so total weight bytes, effective bits-per-weight, and the
// quant-family mix are all pure derivations from the directory. This is the
// "structured state the brief asks for (weight bytes, dequant structure, kernel
// requirement), readable with the parser, no inference needed."
// ---------------------------------------------------------------------------

/// Block layout `(elements_per_block, bytes_per_block)` for a `ggml_type` tag, or
/// `None` for a type we don't recognise. Mirrors llama.cpp's `type_traits` block
/// sizes (`QK_K = 256`; the legacy Q4/Q5/Q8 families block at 32). Codebook IQ
/// formats and K-quants both live here — the format *is* the storage geometry.
pub fn ggml_type_layout(ggml_type: u32) -> Option<(u64, u64)> {
    Some(match ggml_type {
        0 => (1, 4),      // F32
        1 => (1, 2),      // F16
        2 => (32, 18),    // Q4_0
        3 => (32, 20),    // Q4_1
        6 => (32, 22),    // Q5_0
        7 => (32, 24),    // Q5_1
        8 => (32, 34),    // Q8_0
        9 => (32, 36),    // Q8_1
        10 => (256, 84),  // Q2_K
        11 => (256, 110), // Q3_K
        12 => (256, 144), // Q4_K
        13 => (256, 176), // Q5_K
        14 => (256, 210), // Q6_K
        15 => (256, 292), // Q8_K
        16 => (256, 66),  // IQ2_XXS  (codebook)
        17 => (256, 74),  // IQ2_XS   (codebook)
        18 => (256, 98),  // IQ3_XXS  (codebook)
        19 => (256, 50),  // IQ1_S    (codebook)
        20 => (32, 18),   // IQ4_NL   (codebook)
        21 => (256, 110), // IQ3_S    (codebook)
        22 => (256, 82),  // IQ2_S    (codebook)
        23 => (256, 136), // IQ4_XS   (codebook)
        24 => (1, 1),     // I8
        25 => (1, 2),     // I16
        26 => (1, 4),     // I32
        27 => (1, 8),     // I64
        28 => (1, 8),     // F64
        29 => (256, 56),  // IQ1_M    (codebook)
        30 => (1, 2),     // BF16
        34 => (256, 54),  // TQ1_0    (ternary)
        35 => (256, 66),  // TQ2_0    (ternary)
        _ => return None,
    })
}

/// The eight bit-level quant families used for the constellation's colour axis. A
/// whole model is classified by its **dominant** tensor family (by weight bytes);
/// the ladder Q8→Q6→Q5→Q4→Q3→Q2→Q1 is the "quant family made visible as a path"
/// the brief describes. `Full` = 16/32-bit (F16/BF16/F32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Hash))]
pub enum QuantFamily {
    Full,
    Q8,
    Q6,
    Q5,
    Q4,
    Q3,
    Q2,
    Q1,
    Other,
}

impl QuantFamily {
    /// Classify a single `ggml_type` into its bit-level family.
    pub fn from_ggml_type(t: u32) -> QuantFamily {
        match t {
            0 | 1 | 28 | 30 => QuantFamily::Full, // F32/F16/F64/BF16
            8 | 9 | 15 => QuantFamily::Q8,        // Q8_0/Q8_1/Q8_K
            14 => QuantFamily::Q6,                // Q6_K
            6 | 7 | 13 => QuantFamily::Q5,        // Q5_0/Q5_1/Q5_K
            2 | 3 | 12 | 20 | 23 => QuantFamily::Q4, // Q4_0/Q4_1/Q4_K/IQ4_NL/IQ4_XS
            11 | 18 | 21 => QuantFamily::Q3,      // Q3_K/IQ3_XXS/IQ3_S
            10 | 16 | 17 | 22 => QuantFamily::Q2, // Q2_K/IQ2_XXS/IQ2_XS/IQ2_S
            19 | 29 | 34 | 35 => QuantFamily::Q1, // IQ1_S/IQ1_M/TQ1_0/TQ2_0
            _ => QuantFamily::Other,
        }
    }

    /// A stable ordinal 0..=8 (Full=0 … Q1=7, Other=8) — the ladder order, used as
    /// the constellation's `node_scalar` hue channel.
    pub fn ordinal(self) -> u32 {
        match self {
            QuantFamily::Full => 0,
            QuantFamily::Q8 => 1,
            QuantFamily::Q6 => 2,
            QuantFamily::Q5 => 3,
            QuantFamily::Q4 => 4,
            QuantFamily::Q3 => 5,
            QuantFamily::Q2 => 6,
            QuantFamily::Q1 => 7,
            QuantFamily::Other => 8,
        }
    }

    /// A short label for HUD/legend text.
    pub fn label(self) -> &'static str {
        match self {
            QuantFamily::Full => "F16",
            QuantFamily::Q8 => "Q8",
            QuantFamily::Q6 => "Q6",
            QuantFamily::Q5 => "Q5",
            QuantFamily::Q4 => "Q4",
            QuantFamily::Q3 => "Q3",
            QuantFamily::Q2 => "Q2",
            QuantFamily::Q1 => "Q1",
            QuantFamily::Other => "?",
        }
    }
}

impl GgufHeader {
    /// Total declared parameter count = Σ over tensors of `n_elements`.
    pub fn total_params(&self) -> u64 {
        self.tensors.iter().map(|t| t.n_elements()).sum()
    }

    /// Total on-disk weight bytes = Σ over tensors of [`GgufTensor::byte_size`] — the
    /// honest per-tensor quant mix (a model's `token_embd` may be Q6_K while its
    /// attention is Q4_K; this sums the real geometry, not a nominal average).
    pub fn total_weight_bytes(&self) -> u64 {
        self.tensors.iter().map(|t| t.byte_size()).sum()
    }

    /// Effective **bits per weight** across the whole tensor mix
    /// (`weight_bytes · 8 / params`). The Tier-1 quality *proxy* (labeled proxy) —
    /// the header can tell you how many bits each weight costs, not how good the
    /// model is. `0` when the model declares no parameters.
    pub fn bits_per_weight(&self) -> f64 {
        let p = self.total_params();
        if p == 0 {
            0.0
        } else {
            self.total_weight_bytes() as f64 * 8.0 / p as f64
        }
    }

    /// `true` if any tensor uses a `ggml_type` we couldn't size from the block table
    /// (so its bytes are the 2-B/elem estimate). The atlas flags such a model.
    pub fn has_unknown_quant(&self) -> bool {
        self.tensors.iter().any(|t| ggml_type_layout(t.ggml_type).is_none())
    }

    /// The model's **dominant** quant family — the family holding the most weight
    /// bytes (ties broken toward the lower-bit family, since that's the headline
    /// format). Drives the constellation node's colour. Empty model → `Other`.
    pub fn dominant_quant_family(&self) -> QuantFamily {
        // Accumulate bytes per family.
        let mut bytes: [u64; 9] = [0; 9];
        for t in &self.tensors {
            let fam = QuantFamily::from_ggml_type(t.ggml_type);
            bytes[fam.ordinal() as usize] = bytes[fam.ordinal() as usize].saturating_add(t.byte_size());
        }
        // Pick the max; on a tie prefer the higher ordinal (lower-bit / more-quantized).
        let mut best = QuantFamily::Other;
        let mut best_bytes = 0u64;
        for fam in [
            QuantFamily::Full,
            QuantFamily::Q8,
            QuantFamily::Q6,
            QuantFamily::Q5,
            QuantFamily::Q4,
            QuantFamily::Q3,
            QuantFamily::Q2,
            QuantFamily::Q1,
            QuantFamily::Other,
        ] {
            let b = bytes[fam.ordinal() as usize];
            if b >= best_bytes && b > 0 {
                best = fam;
                best_bytes = b;
            }
        }
        best
    }

    /// KV-cache bytes added by a **single** cached token — i.e. the cache at a context
    /// of 1, before any sliding window has filled. For a model with no windowed layers
    /// the cache is exactly this times the context; for a windowed one it is not, so
    /// prefer [`GgufHeader::kv_bytes_at_context`] for anything sizing real memory.
    /// Returns `0` if the head geometry is unknown.
    pub fn kv_bytes_per_token(&self, kv_elem_bytes: u64) -> u64 {
        self.kv_bytes_at_context(kv_elem_bytes, 1)
    }
}

/// A parsed GGUF header: raw KV map + tensor directory, plus the convenience dims
/// pulled out by architecture. No weights.
#[derive(Debug, Clone, Default)]
pub struct GgufHeader {
    pub version: u32,
    /// `general.architecture` (e.g. `"llama"`), the prefix for the dim keys.
    pub arch: String,
    /// `general.name` if present (else empty).
    pub name: String,
    pub metadata: BTreeMap<String, GgufValue>,
    pub tensors: Vec<GgufTensor>,
    // ── convenience dims (0 when the arch didn't declare the key) ──
    pub n_layers: u32,
    pub n_heads: u32,
    /// The largest per-layer KV head count (the scalar summary; see
    /// [`GgufHeader::n_heads_kv_per_layer`] for the honest per-layer geometry).
    pub n_heads_kv: u32,
    pub n_embd: u32,
    pub n_ff: u32,
    pub n_vocab: u32,
    pub context_length: u32,
    /// KV head count **per layer**. Most architectures declare one scalar (broadcast
    /// here across all layers); some (Gemma-4) declare an array because full-attention
    /// and sliding-window layers carry different KV widths. Always `n_layers` long.
    pub n_heads_kv_per_layer: Vec<u32>,
    /// Declared KV head dims (`attention.key_length` / `attention.value_length`), and
    /// their sliding-window counterparts. `0` when the arch doesn't state them — then
    /// `n_embd / n_heads` is the fallback, which is only correct when the head dim
    /// happens to divide the embedding evenly (it does not for Gemma-4).
    pub key_length: u32,
    pub value_length: u32,
    pub key_length_swa: u32,
    pub value_length_swa: u32,
    /// Sliding-window (local) attention span in tokens; `0` = none. On a window layer
    /// the KV cache stops growing once the context passes the window.
    pub sliding_window: u32,
    /// `true` at index `i` when layer `i` uses sliding-window attention. Empty when the
    /// model has no windowed layers. Always `n_layers` long otherwise.
    pub swa_layers: Vec<bool>,
    /// `false` when the head geometry had to be **guessed** — a KV key was present but
    /// in a form we could not read. The KV-derived numbers are then a proxy, not a
    /// derivation, and callers should say so rather than quote them as fact.
    pub kv_geometry_known: bool,
    /// `general.alignment` (default 32) — the padding the tensor-data section starts on.
    pub alignment: u64,
    /// Absolute byte offset of the **tensor-data section** = the directory's end rounded
    /// up to [`GgufHeader::alignment`]. Every `GgufTensor::offset` is relative to this.
    /// The header parser still never *reads* past the directory (#507 Tier 1's payload
    /// reader, `gguf_data.rs`, is the only thing that does) — it just records where the
    /// weights begin, which is knowledge the directory already implies.
    pub data_offset: u64,
}

impl GgufHeader {
    /// Look up an arch-prefixed metadata key (`{arch}.{suffix}`) as `u32`.
    pub fn arch_u32(&self, suffix: &str) -> Option<u32> {
        let key = format!("{}.{}", self.arch, suffix);
        self.metadata.get(&key).and_then(|v| v.as_u64()).map(|v| v as u32)
    }
}

/// A tiny counting reader so we always know how many bytes we've consumed (the
/// tensor-data section starts at the aligned end of the directory — we never reach
/// it, but the count makes the parser self-describing and testable).
struct Cursor<R: Read> {
    inner: R,
    pos: u64,
}

impl<R: Read> Cursor<R> {
    fn new(inner: R) -> Self {
        Cursor { inner, pos: 0 }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.inner.read_exact(buf)?;
        self.pos += buf.len() as u64;
        Ok(())
    }

    fn u8(&mut self) -> io::Result<u8> {
        let mut b = [0u8; 1];
        self.read_exact(&mut b)?;
        Ok(b[0])
    }

    fn u32(&mut self) -> io::Result<u32> {
        let mut b = [0u8; 4];
        self.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    fn u64(&mut self) -> io::Result<u64> {
        let mut b = [0u8; 8];
        self.read_exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    /// A GGUF string: u64 length prefix + raw UTF-8 bytes (lossy).
    fn string(&mut self) -> io::Result<String> {
        let len = self.u64()?;
        // Guard against a corrupt/huge length claim (cap at 16 MiB — real keys are tiny).
        if len > 16 * 1024 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "gguf string length absurd"));
        }
        let mut buf = vec![0u8; len as usize];
        self.read_exact(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    /// Skip `n` bytes without allocating them all (used to drop array payloads).
    fn skip(&mut self, mut n: u64) -> io::Result<()> {
        let mut scratch = [0u8; 4096];
        while n > 0 {
            let take = n.min(scratch.len() as u64) as usize;
            self.read_exact(&mut scratch[..take])?;
            n -= take as u64;
        }
        Ok(())
    }
}

/// Fixed byte size of a scalar metadata value type, for skipping array payloads.
fn scalar_size(t: u32) -> Option<u64> {
    Some(match t {
        vtype::UINT8 | vtype::INT8 | vtype::BOOL => 1,
        vtype::UINT16 | vtype::INT16 => 2,
        vtype::UINT32 | vtype::INT32 | vtype::FLOAT32 => 4,
        vtype::UINT64 | vtype::INT64 | vtype::FLOAT64 => 8,
        _ => return None, // STRING / ARRAY are variable
    })
}

/// Integer/bool value types — the ones whose array elements we retain (head geometry
/// is always integral; float arrays like rope factors are skipped).
fn is_int_like(t: u32) -> bool {
    matches!(
        t,
        vtype::UINT8
            | vtype::INT8
            | vtype::UINT16
            | vtype::INT16
            | vtype::UINT32
            | vtype::INT32
            | vtype::UINT64
            | vtype::INT64
            | vtype::BOOL
    )
}

fn read_value<R: Read>(c: &mut Cursor<R>, t: u32) -> io::Result<GgufValue> {
    Ok(match t {
        vtype::UINT8 => GgufValue::U8(c.u8()?),
        vtype::INT8 => GgufValue::I8(c.u8()? as i8),
        vtype::UINT16 => {
            let mut b = [0u8; 2];
            c.read_exact(&mut b)?;
            GgufValue::U16(u16::from_le_bytes(b))
        }
        vtype::INT16 => {
            let mut b = [0u8; 2];
            c.read_exact(&mut b)?;
            GgufValue::I16(i16::from_le_bytes(b))
        }
        vtype::UINT32 => GgufValue::U32(c.u32()?),
        vtype::INT32 => GgufValue::I32(c.u32()? as i32),
        vtype::FLOAT32 => GgufValue::F32(f32::from_bits(c.u32()?)),
        vtype::BOOL => GgufValue::Bool(c.u8()? != 0),
        vtype::STRING => GgufValue::String(c.string()?),
        vtype::UINT64 => GgufValue::U64(c.u64()?),
        vtype::INT64 => GgufValue::I64(c.u64()? as i64),
        vtype::FLOAT64 => GgufValue::F64(f64::from_bits(c.u64()?)),
        vtype::ARRAY => {
            let elem_type = c.u32()?;
            let len = c.u64()?;
            let mut values = Vec::new();
            if elem_type == vtype::STRING {
                for _ in 0..len {
                    let slen = c.u64()?;
                    c.skip(slen)?;
                }
            } else if elem_type == vtype::ARRAY {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "nested gguf arrays unsupported"));
            } else if let Some(sz) = scalar_size(elem_type) {
                // Retain short integer/bool arrays (per-layer head geometry); skip the
                // rest (floats, and anything vocabulary-sized) without storing them.
                if is_int_like(elem_type) && len <= MAX_ARRAY_VALUES {
                    values.reserve(len as usize);
                    for _ in 0..len {
                        let v = read_value(c, elem_type)?;
                        values.push(v.as_i64().unwrap_or(0));
                    }
                } else {
                    c.skip(len.saturating_mul(sz))?;
                }
            } else {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "unknown gguf array elem type"));
            }
            GgufValue::Array { elem_type, len, values }
        }
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown gguf value type {other}"),
            ))
        }
    })
}

/// GGUF magic bytes: ASCII `GGUF`.
pub const MAGIC: [u8; 4] = *b"GGUF";

/// Parse a GGUF header from any reader (the file's start is enough — tensor data is
/// never touched). Returns a fully-populated [`GgufHeader`].
pub fn parse<R: Read>(reader: R) -> io::Result<GgufHeader> {
    let mut c = Cursor::new(reader);

    let mut magic = [0u8; 4];
    c.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a GGUF file (bad magic)"));
    }
    let version = c.u32()?;
    if version < 2 || version > 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported GGUF version {version} (need 2 or 3)"),
        ));
    }
    let tensor_count = c.u64()?;
    let kv_count = c.u64()?;
    // Sanity caps so a corrupt count can't drive an unbounded loop.
    if tensor_count > 1_000_000 || kv_count > 1_000_000 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "gguf count implausibly large"));
    }

    let mut metadata: BTreeMap<String, GgufValue> = BTreeMap::new();
    for _ in 0..kv_count {
        let key = c.string()?;
        let vt = c.u32()?;
        let val = read_value(&mut c, vt)?;
        metadata.insert(key, val);
    }

    let mut tensors: Vec<GgufTensor> = Vec::with_capacity(tensor_count as usize);
    for _ in 0..tensor_count {
        let name = c.string()?;
        let n_dims = c.u32()?;
        if n_dims > 8 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "tensor with > 8 dims"));
        }
        let mut dims = Vec::with_capacity(n_dims as usize);
        for _ in 0..n_dims {
            dims.push(c.u64()?);
        }
        let ggml_type = c.u32()?;
        let offset = c.u64()?;
        tensors.push(GgufTensor { name, dims, ggml_type, offset });
    }

    // Where the weights begin: the directory's end, rounded up to `general.alignment`
    // (32 unless the writer says otherwise). We do NOT read there — but a caller that
    // wants one tensor's rows (#507's embedding galaxy) needs the base to seek from,
    // and it is a pure function of what we already parsed.
    let dir_end = c.pos;
    let alignment = metadata.get("general.alignment").and_then(|v| v.as_u64()).unwrap_or(32).max(1);
    let data_offset = dir_end.div_ceil(alignment).saturating_mul(alignment);

    let mut h = GgufHeader { version, metadata, tensors, alignment, data_offset, ..Default::default() };
    h.arch = h.metadata.get("general.architecture").and_then(|v| v.as_str()).unwrap_or("").to_string();
    h.name = h.metadata.get("general.name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    h.derive_dims();
    Ok(h)
}

/// Parse a GGUF header from a file path, reading only the header region.
pub fn parse_file<P: AsRef<Path>>(path: P) -> io::Result<GgufHeader> {
    let f = std::fs::File::open(path)?;
    parse(std::io::BufReader::new(f))
}

impl GgufHeader {
    /// Fill the KV-cache head geometry: per-layer KV head counts, the declared head
    /// dims, and which layers use sliding-window attention. Sets `kv_geometry_known`
    /// to `false` if anything was present but unreadable.
    ///
    /// Three shapes have to be handled, because the cache size is wrong (often by
    /// several fold) if any is missed:
    /// - `attention.head_count_kv` is a **scalar** on most archs but a **per-layer
    ///   array** on Gemma-4, whose full-attention layers use far fewer KV heads than
    ///   its local layers. Absent entirely = no GQA = one KV head per query head.
    /// - `attention.key_length` / `value_length` are **declared**; only fall back to
    ///   `n_embd / n_heads` when they are not.
    /// - `attention.sliding_window` + `sliding_window_pattern` mark layers whose cache
    ///   is capped at the window instead of growing with the context.
    fn derive_kv_geometry(&mut self) {
        let layers = self.n_layers as usize;

        // ── KV heads per layer ──
        let kv_key = format!("{}.attention.head_count_kv", self.arch);
        self.n_heads_kv_per_layer = match self.metadata.get(&kv_key) {
            // Scalar: broadcast across the stack.
            Some(v) if v.as_u64().is_some() => vec![v.as_u64().unwrap() as u32; layers],
            // Per-layer array (Gemma-4).
            Some(v) if v.as_u64_vec().is_some() => {
                let vals = v.as_u64_vec().unwrap();
                if vals.len() == layers {
                    vals.iter().map(|n| *n as u32).collect()
                } else {
                    // Declared, but not one entry per block — we can't trust the mapping.
                    self.kv_geometry_known = false;
                    vec![self.n_heads; layers]
                }
            }
            // Present but unreadable (e.g. an array longer than we retain).
            Some(_) => {
                self.kv_geometry_known = false;
                vec![self.n_heads; layers]
            }
            // Absent is not a guess: no GQA key means KV heads == query heads.
            None => vec![self.n_heads; layers],
        };
        self.n_heads_kv = self.n_heads_kv_per_layer.iter().copied().max().unwrap_or(self.n_heads);

        // ── declared head dims ──
        self.key_length = self.arch_u32("attention.key_length").unwrap_or(0);
        self.value_length = self.arch_u32("attention.value_length").unwrap_or(0);
        self.key_length_swa = self.arch_u32("attention.key_length_swa").unwrap_or(self.key_length);
        self.value_length_swa = self.arch_u32("attention.value_length_swa").unwrap_or(self.value_length);

        // ── sliding-window layers ──
        self.sliding_window = self.arch_u32("attention.sliding_window").unwrap_or(0);
        let pat_key = format!("{}.attention.sliding_window_pattern", self.arch);
        self.swa_layers = if self.sliding_window == 0 {
            Vec::new()
        } else {
            match self.metadata.get(&pat_key) {
                // Per-layer bool array (Gemma-4): true = this layer is windowed.
                Some(v) if v.as_u64_vec().is_some() => {
                    let vals = v.as_u64_vec().unwrap();
                    if vals.len() == layers {
                        vals.iter().map(|n| *n != 0).collect()
                    } else {
                        self.kv_geometry_known = false;
                        Vec::new()
                    }
                }
                // Scalar period `n` (llama.cpp's Gemma-3 form): every nth layer is
                // full-attention, the rest are windowed.
                Some(v) if v.as_u64().is_some_and(|n| n > 1) => {
                    let n = v.as_u64().unwrap();
                    (0..layers).map(|i| (i as u64 + 1) % n != 0).collect()
                }
                Some(_) => {
                    self.kv_geometry_known = false;
                    Vec::new()
                }
                // A window with no pattern = every layer is windowed (e.g. Mistral).
                None => vec![true; layers],
            }
        };
    }

    /// The K and V head dims for a full-attention or sliding-window layer. Prefers the
    /// **declared** `key_length`/`value_length`; falls back to `n_embd / n_heads` only
    /// when the arch states neither.
    fn kv_dims(&self, swa: bool) -> (u64, u64) {
        let fallback = if self.n_heads > 0 { (self.n_embd / self.n_heads) as u64 } else { 0 };
        let (k, v) = if swa {
            (self.key_length_swa, self.value_length_swa)
        } else {
            (self.key_length, self.value_length)
        };
        let k = if k > 0 { k as u64 } else { fallback };
        let v = if v > 0 { v as u64 } else { fallback };
        (k, v)
    }

    /// Total **KV-cache bytes** held at a given context fill, for a KV element size in
    /// bytes (`f16` cache = 2). Summed per layer, because layers differ: each holds
    /// `n_heads_kv · (k_dim + v_dim) · elem` per cached token, and a sliding-window
    /// layer caches at most `sliding_window` tokens no matter how long the context is.
    ///
    /// This is deliberately **not** `per_token · context`: with windowed layers the
    /// cache is sub-linear in context, and treating it as linear overstates the cache
    /// (and understates tok/s) by several fold on a long context.
    /// Returns `0` if the head geometry is unknown.
    pub fn kv_bytes_at_context(&self, kv_elem_bytes: u64, context_tokens: u32) -> u64 {
        if self.n_heads == 0 || self.n_layers == 0 || self.n_embd == 0 {
            return 0;
        }
        let ctx = context_tokens as u64;
        let mut total: u64 = 0;
        for i in 0..self.n_layers as usize {
            let swa = self.swa_layers.get(i).copied().unwrap_or(false);
            let heads = self.n_heads_kv_per_layer.get(i).copied().unwrap_or(self.n_heads).max(1) as u64;
            let (k_dim, v_dim) = self.kv_dims(swa);
            let cached = if swa { ctx.min(self.sliding_window as u64) } else { ctx };
            total = total.saturating_add(heads * (k_dim + v_dim) * kv_elem_bytes * cached);
        }
        total
    }

    /// Fill the convenience dims from the arch-prefixed KV keys (varying by arch),
    /// with graceful fallbacks (esp. vocab, which many archs don't state directly).
    fn derive_dims(&mut self) {
        self.n_layers = self.arch_u32("block_count").unwrap_or(0);
        self.n_heads = self.arch_u32("attention.head_count").unwrap_or(0);
        self.n_embd = self.arch_u32("embedding_length").unwrap_or(0);
        self.n_ff = self.arch_u32("feed_forward_length").unwrap_or(0);
        self.context_length = self.arch_u32("context_length").unwrap_or(0);
        self.kv_geometry_known = true;
        self.derive_kv_geometry();

        // Vocab: prefer an explicit key, else the tokenizer token array length, else
        // the token-embedding tensor's leading dim.
        self.n_vocab = self
            .arch_u32("vocab_size")
            .or_else(|| self.metadata.get("tokenizer.ggml.tokens").and_then(|v| v.arr_len()).map(|n| n as u32))
            .or_else(|| {
                self.tensors
                    .iter()
                    .find(|t| t.name == "token_embd.weight" || t.name.ends_with("tok_embeddings.weight"))
                    .and_then(|t| t.dims.last().copied())
                    .map(|d| d as u32)
            })
            .unwrap_or(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── A minimal, valid in-memory GGUF fixture builder (v3, little-endian). ──
    struct Builder {
        buf: Vec<u8>,
    }
    impl Builder {
        fn new() -> Self {
            Builder { buf: Vec::new() }
        }
        fn u32(&mut self, v: u32) {
            self.buf.extend_from_slice(&v.to_le_bytes());
        }
        fn u64(&mut self, v: u64) {
            self.buf.extend_from_slice(&v.to_le_bytes());
        }
        fn gstr(&mut self, s: &str) {
            self.u64(s.len() as u64);
            self.buf.extend_from_slice(s.as_bytes());
        }
        /// key + a UINT32 metadata value.
        fn kv_u32(&mut self, key: &str, v: u32) {
            self.gstr(key);
            self.u32(vtype::UINT32);
            self.u32(v);
        }
        /// key + a STRING metadata value.
        fn kv_str(&mut self, key: &str, v: &str) {
            self.gstr(key);
            self.u32(vtype::STRING);
            self.gstr(v);
        }
        /// key + a STRING ARRAY metadata value (length recorded; contents present).
        fn kv_str_array(&mut self, key: &str, items: &[&str]) {
            self.gstr(key);
            self.u32(vtype::ARRAY);
            self.u32(vtype::STRING);
            self.u64(items.len() as u64);
            for it in items {
                self.gstr(it);
            }
        }
        /// key + a UINT32 ARRAY metadata value (per-layer geometry).
        fn kv_u32_array(&mut self, key: &str, items: &[u32]) {
            self.gstr(key);
            self.u32(vtype::ARRAY);
            self.u32(vtype::UINT32);
            self.u64(items.len() as u64);
            for &it in items {
                self.u32(it);
            }
        }
        /// key + a BOOL ARRAY metadata value (e.g. a sliding-window pattern).
        fn kv_bool_array(&mut self, key: &str, items: &[bool]) {
            self.gstr(key);
            self.u32(vtype::ARRAY);
            self.u32(vtype::BOOL);
            self.u64(items.len() as u64);
            for &it in items {
                self.buf.push(it as u8);
            }
        }
        /// A tensor directory entry.
        fn tensor(&mut self, name: &str, dims: &[u64], ggml_type: u32, offset: u64) {
            self.gstr(name);
            self.u32(dims.len() as u32);
            for &d in dims {
                self.u64(d);
            }
            self.u32(ggml_type);
            self.u64(offset);
        }
    }

    /// Build a tiny but structurally complete `tinyllama`-style GGUF: 3 layers,
    /// 4 heads, embd 8, ff 16, a 5-token vocab (via the tokenizer array), and a
    /// couple of per-layer tensors.
    fn tiny_gguf() -> (Vec<u8>, ()) {
        let mut kv = Builder::new();
        // Two-pass: we must write the header (counts) before the body, so build
        // the KV + tensor body first, counting entries, then prepend the header.
        kv.kv_str("general.architecture", "llama");
        kv.kv_str("general.name", "tiny-test");
        kv.kv_u32("llama.block_count", 3);
        kv.kv_u32("llama.attention.head_count", 4);
        kv.kv_u32("llama.attention.head_count_kv", 2);
        kv.kv_u32("llama.embedding_length", 8);
        kv.kv_u32("llama.feed_forward_length", 16);
        kv.kv_u32("llama.context_length", 2048);
        kv.kv_str_array("tokenizer.ggml.tokens", &["<s>", "</s>", "a", "b", "c"]);
        let kv_count = 9u64; // architecture, name, block_count, head_count, head_count_kv,
                             // embedding_length, feed_forward_length, context_length, tokens



        let mut td = Builder::new();
        let mut n_tensors = 0u64;
        td.tensor("token_embd.weight", &[8, 5], 0, 0);
        n_tensors += 1;
        for l in 0..3u64 {
            td.tensor(&format!("blk.{l}.attn_q.weight"), &[8, 8], 0, l * 1000);
            td.tensor(&format!("blk.{l}.ffn_up.weight"), &[8, 16], 0, l * 2000);
            n_tensors += 2;
        }

        let mut out = Builder::new();
        out.buf.extend_from_slice(&MAGIC);
        out.u32(3); // version
        out.u64(n_tensors);
        out.u64(kv_count);
        out.buf.extend_from_slice(&kv.buf);
        out.buf.extend_from_slice(&td.buf);
        (out.buf, ())
    }

    #[test]
    fn parses_tiny_gguf_header() {
        let (bytes, _) = tiny_gguf();
        let h = parse(&bytes[..]).expect("parse tiny gguf");
        assert_eq!(h.version, 3);
        assert_eq!(h.arch, "llama");
        assert_eq!(h.name, "tiny-test");
        assert_eq!(h.n_layers, 3);
        assert_eq!(h.n_heads, 4);
        assert_eq!(h.n_heads_kv, 2);
        assert_eq!(h.n_embd, 8);
        assert_eq!(h.n_ff, 16);
        assert_eq!(h.context_length, 2048);
        // vocab from the tokenizer token array length.
        assert_eq!(h.n_vocab, 5);
        // 1 embedding + 3 layers × 2 = 7 tensors.
        assert_eq!(h.tensors.len(), 7);
        let q0 = h.tensors.iter().find(|t| t.name == "blk.0.attn_q.weight").unwrap();
        assert_eq!(q0.dims, vec![8, 8]);
        assert_eq!(q0.n_elements(), 64);
        assert_eq!(q0.ggml_type, 0);
    }

    #[test]
    fn records_the_tensor_data_section_base_offset() {
        // #507 Tier 1: the payload reader seeks to `data_offset + tensor.offset`, so the
        // base must be the directory's end rounded UP to `general.alignment` (32 by
        // default) — off by one byte and every dequantized row is garbage.
        let (bytes, _) = tiny_gguf();
        let h = parse(&bytes[..]).unwrap();
        assert_eq!(h.alignment, 32);
        // The fixture's directory ends at exactly `bytes.len()` (no payload written).
        assert_eq!(h.data_offset, bytes.len().div_ceil(32) as u64 * 32);
        assert!(h.data_offset >= bytes.len() as u64);
        assert_eq!(h.data_offset % 32, 0);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut b = Builder::new();
        b.buf.extend_from_slice(b"XXXX");
        b.u32(3);
        b.u64(0);
        b.u64(0);
        assert!(parse(&b.buf[..]).is_err());
    }

    #[test]
    fn rejects_bad_version() {
        let mut b = Builder::new();
        b.buf.extend_from_slice(&MAGIC);
        b.u32(1); // v1 unsupported
        b.u64(0);
        b.u64(0);
        assert!(parse(&b.buf[..]).is_err());
    }

    #[test]
    fn vocab_falls_back_to_token_embd_dim() {
        // No tokenizer array, no vocab_size key → derive from token_embd.weight.
        let mut kv = Builder::new();
        kv.kv_str("general.architecture", "qwen2");
        kv.kv_u32("qwen2.block_count", 1);
        kv.kv_u32("qwen2.attention.head_count", 2);
        kv.kv_u32("qwen2.embedding_length", 4);
        let kv_count = 4u64;
        let mut td = Builder::new();
        td.tensor("token_embd.weight", &[4, 99], 0, 0);

        let mut out = Builder::new();
        out.buf.extend_from_slice(&MAGIC);
        out.u32(3);
        out.u64(1);
        out.u64(kv_count);
        out.buf.extend_from_slice(&kv.buf);
        out.buf.extend_from_slice(&td.buf);

        let h = parse(&out.buf[..]).unwrap();
        assert_eq!(h.arch, "qwen2");
        assert_eq!(h.n_vocab, 99);
        assert_eq!(h.n_heads_kv, 2); // defaults to head_count when kv key absent
    }

    #[test]
    fn parse_file_errors_on_missing_or_bad_path() {
        // Bugbot #390 (finding 1) precondition: a failed model load must be an `Err`
        // so the visual's edge-detect branch CLEARS the specimen graph (drops
        // `neural_loaded` back to the empty/procedural state) instead of leaving a
        // stale connectome on screen. A path that does not exist must error.
        let missing = std::env::temp_dir().join("organon-nonexistent-model-xyzzy.gguf");
        let _ = std::fs::remove_file(&missing);
        assert!(parse_file(&missing).is_err(), "missing path must error");

        // A file whose bytes are not a GGUF header must also error (not silently
        // succeed with a stale/empty header).
        let bad = std::env::temp_dir().join(format!("organon-bad-model-{}.gguf", now_millis_testonly()));
        std::fs::write(&bad, b"not a gguf file at all").unwrap();
        assert!(parse_file(&bad).is_err(), "garbage bytes must error");
        let _ = std::fs::remove_file(&bad);
    }

    fn now_millis_testonly() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    }

    // ── #423 Tier 1 storage-geometry derivations ──────────────────────────

    #[test]
    fn ggml_layout_known_and_unknown() {
        assert_eq!(ggml_type_layout(0), Some((1, 4))); // F32
        assert_eq!(ggml_type_layout(1), Some((1, 2))); // F16
        assert_eq!(ggml_type_layout(12), Some((256, 144))); // Q4_K
        assert_eq!(ggml_type_layout(17), Some((256, 74))); // IQ2_XS (codebook)
        assert_eq!(ggml_type_layout(999), None); // unknown
    }

    #[test]
    fn tensor_byte_size_f32_and_q4k() {
        // F32: bytes = elements × 4.
        let f32t = GgufTensor { name: "a".into(), dims: vec![8, 8], ggml_type: 0, offset: 0 };
        assert_eq!(f32t.n_elements(), 64);
        assert_eq!(f32t.byte_size(), 64 * 4);
        // Q4_K: 256 elems / 144 B block. A 512-elem tensor = 2 blocks = 288 B.
        let q4k = GgufTensor { name: "b".into(), dims: vec![512], ggml_type: 12, offset: 0 };
        assert_eq!(q4k.byte_size(), 2 * 144);
        // Partial block rounds up to a whole block (300 elems → 2 blocks of 256).
        let q4k_partial = GgufTensor { name: "c".into(), dims: vec![300], ggml_type: 12, offset: 0 };
        assert_eq!(q4k_partial.byte_size(), 2 * 144);
        // Unknown type falls back to 2 B/elem.
        let unk = GgufTensor { name: "d".into(), dims: vec![10], ggml_type: 999, offset: 0 };
        assert_eq!(unk.byte_size(), 20);
    }

    #[test]
    fn header_weight_bytes_params_and_bpw_are_consistent() {
        let (bytes, _) = tiny_gguf();
        let h = parse(&bytes[..]).unwrap();
        // tiny_gguf is all-F32: token_embd 8×5=40, per layer q(8×8=64)+ffn(8×16=128)=192 ×3.
        let params = 40 + 192 * 3;
        assert_eq!(h.total_params(), params);
        assert_eq!(h.total_weight_bytes(), params * 4); // F32 → 4 B/weight
        assert!((h.bits_per_weight() - 32.0).abs() < 1e-9); // 4 B/weight = 32 bits
        assert!(!h.has_unknown_quant());
        assert_eq!(h.dominant_quant_family(), QuantFamily::Full);
    }

    #[test]
    fn quant_family_classification_and_ladder_order() {
        assert_eq!(QuantFamily::from_ggml_type(0), QuantFamily::Full);
        assert_eq!(QuantFamily::from_ggml_type(8), QuantFamily::Q8);
        assert_eq!(QuantFamily::from_ggml_type(12), QuantFamily::Q4); // Q4_K
        assert_eq!(QuantFamily::from_ggml_type(23), QuantFamily::Q4); // IQ4_XS
        assert_eq!(QuantFamily::from_ggml_type(10), QuantFamily::Q2); // Q2_K
        assert_eq!(QuantFamily::from_ggml_type(16), QuantFamily::Q2); // IQ2_XXS (codebook)
        assert_eq!(QuantFamily::from_ggml_type(19), QuantFamily::Q1); // IQ1_S
        // Ladder ordinals ascend as bits fall.
        assert!(QuantFamily::Full.ordinal() < QuantFamily::Q4.ordinal());
        assert!(QuantFamily::Q4.ordinal() < QuantFamily::Q2.ordinal());
    }

    #[test]
    fn dominant_family_follows_the_byte_mass() {
        // A mostly-Q4_K model with a small F16 embedding: dominant = Q4.
        let mut kv = Builder::new();
        kv.kv_str("general.architecture", "llama");
        kv.kv_u32("llama.block_count", 1);
        kv.kv_u32("llama.attention.head_count", 4);
        kv.kv_u32("llama.embedding_length", 256);
        let kv_count = 4u64;
        let mut td = Builder::new();
        td.tensor("token_embd.weight", &[256, 32], 1, 0); // F16, 8192 elems
        td.tensor("blk.0.attn_q.weight", &[256, 256], 12, 0); // Q4_K, 65536 elems
        td.tensor("blk.0.ffn_up.weight", &[256, 1024], 12, 0); // Q4_K, 262144 elems
        let mut out = Builder::new();
        out.buf.extend_from_slice(&MAGIC);
        out.u32(3);
        out.u64(3);
        out.u64(kv_count);
        out.buf.extend_from_slice(&kv.buf);
        out.buf.extend_from_slice(&td.buf);
        let h = parse(&out.buf[..]).unwrap();
        assert_eq!(h.dominant_quant_family(), QuantFamily::Q4);
        // KV bytes/token: 2·layers·kv_heads·head_dim·elem. head_dim = 256/4 = 64.
        // kv_heads defaults to head_count = 4. f16 cache (2 B): 2·1·4·64·2 = 1024.
        // No windowed layers here, so the cache really is this × context.
        assert_eq!(h.kv_bytes_per_token(2), 1024);
        assert_eq!(h.kv_bytes_at_context(2, 512), 1024 * 512);
    }

    // --- KV-cache head geometry (the atlas's roofline depends on it) ----------
    // Gemma-4 declares all three of the awkward shapes at once: a per-layer KV head
    // array, head dims that are declared rather than `n_embd / n_heads`, and mixed
    // local/global attention. Miss any one and the cache is wrong several-fold, which
    // silently moves every model on the roofline.

    /// A Gemma-4-shaped header, using the real geometry of
    /// `gemma-4-12B-it-QAT-Q4_0.gguf`: 48 blocks, every 6th one full-attention.
    fn gemma4_like() -> Vec<u8> {
        let mut kv_heads = Vec::new();
        let mut pattern = Vec::new();
        for i in 0..48u32 {
            let global = (i % 6) == 5;
            kv_heads.push(if global { 1 } else { 8 });
            pattern.push(!global); // true = sliding-window (local) layer
        }
        let mut kv = Builder::new();
        kv.kv_str("general.architecture", "gemma4");
        kv.kv_u32("gemma4.block_count", 48);
        kv.kv_u32("gemma4.embedding_length", 3840);
        kv.kv_u32("gemma4.attention.head_count", 16);
        kv.kv_u32_array("gemma4.attention.head_count_kv", &kv_heads);
        kv.kv_u32("gemma4.attention.key_length", 512);
        kv.kv_u32("gemma4.attention.value_length", 512);
        kv.kv_u32("gemma4.attention.key_length_swa", 256);
        kv.kv_u32("gemma4.attention.value_length_swa", 256);
        kv.kv_u32("gemma4.attention.sliding_window", 1024);
        kv.kv_bool_array("gemma4.attention.sliding_window_pattern", &pattern);
        let mut out = Builder::new();
        out.buf.extend_from_slice(&MAGIC);
        out.u32(3);
        out.u64(0); // no tensors — this fixture is about head geometry
        out.u64(11); // kv count
        out.buf.extend_from_slice(&kv.buf);
        out.buf
    }

    #[test]
    fn per_layer_kv_head_array_is_read_not_defaulted_to_mha() {
        let h = parse(&gemma4_like()[..]).unwrap();
        assert!(h.kv_geometry_known);
        assert_eq!(h.n_heads_kv_per_layer.len(), 48);
        assert_eq!(h.n_heads_kv_per_layer[0], 8); // local layer
        assert_eq!(h.n_heads_kv_per_layer[5], 1); // global layer — far narrower
        // The summary is the widest layer, never the query-head count (16) that an
        // unread key would have fallen back to.
        assert_eq!(h.n_heads_kv, 8);
        assert_ne!(h.n_heads_kv, h.n_heads);
    }

    #[test]
    fn declared_head_dims_beat_the_embedding_over_heads_fallback() {
        let h = parse(&gemma4_like()[..]).unwrap();
        // The fallback would be 3840/16 = 240 — not this model's head dim at all.
        assert_eq!(h.n_embd / h.n_heads, 240);
        assert_eq!(h.kv_dims(false), (512, 512));
        assert_eq!(h.kv_dims(true), (256, 256));
    }

    #[test]
    fn sliding_window_layers_cap_the_cache_so_it_is_sublinear_in_context() {
        let h = parse(&gemma4_like()[..]).unwrap();
        assert_eq!(h.swa_layers.iter().filter(|s| **s).count(), 40);
        assert_eq!(h.swa_layers.iter().filter(|s| !**s).count(), 8);

        // 40 local layers pinned at the 1024-token window + 8 global layers at full ctx.
        let local: u64 = 40 * 8 * (256 + 256) * 2 * 1024;
        let global: u64 = 8 * 1 * (512 + 512) * 2 * 4096;
        assert_eq!(h.kv_bytes_at_context(2, 4096), local + global);

        // Doubling the context does not double the cache: only the global layers grow.
        let at4k = h.kv_bytes_at_context(2, 4096);
        let at8k = h.kv_bytes_at_context(2, 8192);
        assert_eq!(at8k, local + 2 * global);
        assert!(at8k < 2 * at4k);
    }

    #[test]
    fn windowed_cache_is_far_smaller_than_the_naive_linear_estimate() {
        let h = parse(&gemma4_like()[..]).unwrap();
        // What an unread head_count_kv (→ 16 KV heads) + an n_embd/n_heads head dim
        // (→ 240) + no window awareness produces: every layer full-width, growing
        // forever. This is the shape of the bug this geometry exists to prevent.
        let naive: u64 = 2 * 48 * 16 * 240 * 2 * 4096;
        let honest = h.kv_bytes_at_context(2, 4096);
        assert!(naive > honest * 7, "naive {naive} vs honest {honest}");
    }

    #[test]
    fn absent_kv_head_key_means_mha_and_is_not_a_guess() {
        // No head_count_kv at all: per the GGUF spec that means KV heads == query
        // heads (plain multi-head attention). That is knowledge, not a fallback, so
        // the geometry stays "known" — unlike the unreadable case above.
        let mut kv = Builder::new();
        kv.kv_str("general.architecture", "llama");
        kv.kv_u32("llama.block_count", 2);
        kv.kv_u32("llama.embedding_length", 256);
        kv.kv_u32("llama.attention.head_count", 4);
        let mut out = Builder::new();
        out.buf.extend_from_slice(&MAGIC);
        out.u32(3);
        out.u64(0);
        out.u64(4);
        out.buf.extend_from_slice(&kv.buf);
        let h = parse(&out.buf[..]).unwrap();
        assert!(h.kv_geometry_known);
        assert_eq!(h.n_heads_kv, h.n_heads);
        assert!(h.swa_layers.is_empty()); // no sliding_window key = no windowed layers
    }

    #[test]
    fn unreadable_kv_head_geometry_is_flagged_rather_than_silently_guessed() {
        // An array that isn't one entry per block: we cannot map it onto layers, so
        // we fall back — but we must say so, or the roofline quotes a guess as fact.
        let mut kv = Builder::new();
        kv.kv_str("general.architecture", "llama");
        kv.kv_u32("llama.block_count", 48);
        kv.kv_u32("llama.embedding_length", 3840);
        kv.kv_u32("llama.attention.head_count", 16);
        kv.kv_u32_array("llama.attention.head_count_kv", &[8, 8, 8, 8]);
        let mut out = Builder::new();
        out.buf.extend_from_slice(&MAGIC);
        out.u32(3);
        out.u64(0);
        out.u64(5);
        out.buf.extend_from_slice(&kv.buf);
        let h = parse(&out.buf[..]).unwrap();
        assert!(!h.kv_geometry_known);
        assert_eq!(h.n_heads_kv_per_layer, vec![16; 48]);
    }

    #[test]
    fn scalar_sliding_window_pattern_marks_every_nth_layer_global() {
        // llama.cpp's Gemma-3 form states a period, not a per-layer array: with 6,
        // five of every six layers are local and the sixth is full-attention.
        let mut kv = Builder::new();
        kv.kv_str("general.architecture", "gemma3");
        kv.kv_u32("gemma3.block_count", 12);
        kv.kv_u32("gemma3.embedding_length", 1024);
        kv.kv_u32("gemma3.attention.head_count", 8);
        kv.kv_u32("gemma3.attention.sliding_window", 512);
        kv.kv_u32("gemma3.attention.sliding_window_pattern", 6);
        let mut out = Builder::new();
        out.buf.extend_from_slice(&MAGIC);
        out.u32(3);
        out.u64(0);
        out.u64(6);
        out.buf.extend_from_slice(&kv.buf);
        let h = parse(&out.buf[..]).unwrap();
        assert!(h.kv_geometry_known);
        assert_eq!(h.swa_layers.len(), 12);
        assert_eq!(h.swa_layers.iter().filter(|s| **s).count(), 10);
        assert!(!h.swa_layers[5] && !h.swa_layers[11]); // every 6th is global
    }

    #[test]
    fn a_window_without_a_pattern_makes_every_layer_local() {
        // Mistral's shape: one window, applied to the whole stack.
        let mut kv = Builder::new();
        kv.kv_str("general.architecture", "llama");
        kv.kv_u32("llama.block_count", 4);
        kv.kv_u32("llama.embedding_length", 256);
        kv.kv_u32("llama.attention.head_count", 4);
        kv.kv_u32("llama.attention.sliding_window", 100);
        let mut out = Builder::new();
        out.buf.extend_from_slice(&MAGIC);
        out.u32(3);
        out.u64(0);
        out.u64(5);
        out.buf.extend_from_slice(&kv.buf);
        let h = parse(&out.buf[..]).unwrap();
        assert_eq!(h.swa_layers, vec![true; 4]);
        // Past the window the cache stops growing entirely.
        assert_eq!(h.kv_bytes_at_context(2, 100), h.kv_bytes_at_context(2, 10_000));
    }

    #[test]
    fn vocabulary_sized_arrays_keep_only_their_length() {
        // The retention cap is what keeps a 262k-token vocabulary from being held in
        // memory just so we can read a 48-entry geometry array.
        let big: Vec<u32> = vec![7; (MAX_ARRAY_VALUES + 1) as usize];
        let mut kv = Builder::new();
        kv.kv_str("general.architecture", "llama");
        kv.kv_u32_array("tokenizer.ggml.token_type", &big);
        let mut out = Builder::new();
        out.buf.extend_from_slice(&MAGIC);
        out.u32(3);
        out.u64(0);
        out.u64(2);
        out.buf.extend_from_slice(&kv.buf);
        let h = parse(&out.buf[..]).unwrap();
        let v = h.metadata.get("tokenizer.ggml.token_type").unwrap();
        assert_eq!(v.arr_len(), Some(MAX_ARRAY_VALUES + 1));
        assert_eq!(v.as_u64_vec(), None); // length known, elements deliberately dropped
    }
}
