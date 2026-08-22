//! The LoRA adapter reader — what a fine-tune actually moved, per adapted module (#147 T2).
//!
//! A PEFT LoRA adapter is a directory holding `adapter_config.json` (rank `r`,
//! `lora_alpha`, `target_modules`) and `adapter_model.safetensors` (a `lora_A [r, in]`
//! and a `lora_B [out, r]` per adapted module). The update it represents is exact:
//!
//! ```text
//!     ΔW = s · B · A          s = alpha / r     (alpha / sqrt(r) under rsLoRA)
//! ```
//!
//! This module reads that directory and answers two questions per module: **how far the
//! weights moved** (`‖ΔW‖_F`) and **how concentrated the movement was** (the effective
//! rank of the update). Both are exact functions of the file, so both are **measured**
//! in `MIND_ARCHITECTURE.md` §3's sense. Neither is *importance* — see §"What these
//! numbers are not" below, which is the part that must survive being rendered.
//!
//! # 🚨 ΔW is never materialized, and that is a correctness rule rather than a speed one
//!
//! `ΔW` is `out × in` — 4096×4096 for one attention projection, and there are hundreds
//! of them. Every number here is computed through the **r × r** middle instead, where
//! `r` is 8–64:
//!
//! - **Frobenius norm.** `‖BA‖²_F = trace((BᵀB)(AAᵀ))`, and both Gram matrices are
//!   r×r. Because both are symmetric this collapses to an elementwise sum,
//!   `Σᵢⱼ (BᵀB)ᵢⱼ (AAᵀ)ᵢⱼ`. Cost `O((out + in)·r²)`.
//! - **Singular values.** `B = Q_B R_B` and `Aᵀ = Q_A R_A` (Householder QR, `R` only —
//!   `Q` is never formed). Since `Q_B` and `Q_A` have orthonormal columns,
//!   `BA = Q_B (R_B R_Aᵀ) Q_Aᵀ` has exactly the singular values of the small
//!   `R_B R_Aᵀ`. Same cost, plus an r×r SVD.
//!
//! ⚠️ **The per-neuron version is the cliff, and it is deliberately not here.** A
//! per-output-row norm of `ΔW` needs the full `out × in` product; there is no r×r
//! shortcut for it, because the answer has `out` numbers in it. #147 names this and
//! this module does the cheap tier only, so nobody discovers the wall halfway through
//! a lens.
//!
//! # Which "effective rank" — Roy & Vetterli, and why that one
//!
//! "Effective rank" names at least three different quantities in circulation, so the
//! choice is stated rather than implied. [`effective_rank`] implements the
//! **entropy-based effective rank of Roy & Vetterli (2007)**: normalise the singular
//! values into a distribution `pᵢ = σᵢ / Σσⱼ`, take its Shannon entropy `H`, and report
//! `exp(H)`. It is the definition that owns the *name* in the literature, and it has
//! the properties a readout wants:
//!
//! | Spectrum | `effective_rank` |
//! |---|---|
//! | `r` equal singular values | exactly `r` |
//! | one nonzero singular value | exactly `1` |
//! | anything else | strictly between, continuous in the spectrum |
//!
//! [`stable_rank`] is offered beside it — `‖M‖²_F / σ²_max`, the other cheap scalar
//! people mean by "effective rank" — because the two genuinely disagree and a consumer
//! choosing one should be able to see the other. Stable rank is dominated by the largest
//! singular value; entropy rank counts the whole tail. Raw [`AdaptedModule::singular_values`]
//! are exposed so a third definition needs no change here.
//!
//! # What these numbers are not
//!
//! 🚨 **A large `‖ΔW‖_F` is not importance and a low effective rank is not meaning.**
//! "This fine-tune changed layer 12 the most" is **derived**; "layer 12 holds the skill"
//! is a **contested claim** under principle 5 and must be marked as one wherever it is
//! rendered. The arithmetic here is exact about *displacement in weight space* and says
//! nothing whatever about function.
//!
//! ⚠️ **The base may be quantized.** An adapter trained on a 4-bit base is a delta
//! against a base that is not the released weights. This module reports
//! [`AdapterConfig::base_model_name_or_path`] as the file states it and knows nothing
//! more; #147 T1's `/api/models/checkpoints` supplies `is_quantized`, and the readout is
//! the place that owes the sentence.
//!
//! # Scope
//!
//! Reads `safetensors` — **not** GGUF. llama.cpp's `lora_adapter_init` takes a GGUF
//! adapter and is a different path entirely (#147 T5); the two files are produced by
//! different exports and nothing here touches `llama-cpp-4`. No network, no `Shared`,
//! no renderer: this module is arithmetic over a directory.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a directory is not a readable LoRA adapter, in the words of whoever pointed at it.
///
/// 🚨 **Every arm names the thing that was asked for.** A serde error ("expected value at
/// line 1 column 1") or an EOF describes bytes rather than a decision, and lands in a
/// console where several things could have produced it — the same rule `ExhibitError`
/// keeps two modules over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoraError {
    /// A file in the adapter directory could not be opened or read.
    Io { path: PathBuf, why: String },
    /// `adapter_config.json` is not JSON, or not an object.
    ConfigJson { path: PathBuf, why: String },
    /// `adapter_config.json` parsed but is missing something the maths needs.
    ConfigMissing { path: PathBuf, key: &'static str },
    /// The safetensors file ended before a structure it declared. `need` is the file
    /// length the declaration implies; ⚠️ there is deliberately no `have`, because at a
    /// short read the number of bytes actually present is not knowable without a second
    /// syscall, and a confident-looking wrong figure is worse than none.
    Truncated { what: String, need: u64 },
    /// The safetensors header is not the JSON object the format requires.
    HeaderJson { why: String },
    /// One tensor's directory entry is malformed.
    BadEntry { tensor: String, why: String },
    /// A dtype this reader does not decode. `F32`, `F16` and `BF16` are the three
    /// adapters ship in.
    UnsupportedDtype { tensor: String, dtype: String },
    /// A `lora_A` with no `lora_B`, or the reverse. Both halves are needed; one alone
    /// cannot state a `ΔW`.
    Unpaired { module: String, have: &'static str },
    /// `lora_A [r, in]` and `lora_B [out, r]` disagree about `r`, or either is not 2-D.
    ShapeMismatch { module: String, a: Vec<usize>, b: Vec<usize> },
    /// DoRA. `ΔW` is not `(alpha/r)·B·A` for a DoRA adapter — there is a magnitude
    /// vector on top and the update is a directional decomposition — so the norms this
    /// module computes would be *wrong numbers rather than an error*, which is the one
    /// outcome worth refusing.
    UnsupportedMethod { method: String },
    /// The file parsed and held no `lora_A`/`lora_B` pair at all.
    NoAdaptedModules { path: PathBuf },
}

impl fmt::Display for LoraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoraError::Io { path, why } => write!(f, "cannot read `{}`: {why}", path.display()),
            LoraError::ConfigJson { path, why } => {
                write!(f, "`{}` is not a PEFT adapter config: {why}", path.display())
            }
            LoraError::ConfigMissing { path, key } => write!(
                f,
                "`{}` has no `{key}` -- a LoRA scale is alpha/r and both are required",
                path.display()
            ),
            LoraError::Truncated { what, need } => write!(
                f,
                "adapter_model.safetensors ends early: {what} needs the file to reach {need} bytes"
            ),
            LoraError::HeaderJson { why } => {
                write!(f, "adapter_model.safetensors has no readable tensor directory: {why}")
            }
            LoraError::BadEntry { tensor, why } => {
                write!(f, "tensor `{tensor}` has a malformed directory entry: {why}")
            }
            LoraError::UnsupportedDtype { tensor, dtype } => write!(
                f,
                "tensor `{tensor}` is {dtype}; this reader decodes F32, F16 and BF16"
            ),
            LoraError::Unpaired { module, have } => write!(
                f,
                "`{module}` has only its {have} factor -- a LoRA update needs both lora_A and lora_B"
            ),
            LoraError::ShapeMismatch { module, a, b } => write!(
                f,
                "`{module}` shapes do not compose: lora_A is {a:?} and lora_B is {b:?}, \
                 which must be [r, in] and [out, r] with one r"
            ),
            LoraError::UnsupportedMethod { method } => write!(
                f,
                "this adapter is {method}, whose update is not (alpha/r)*B*A -- \
                 reading it as LoRA would produce plausible wrong norms rather than an error"
            ),
            LoraError::NoAdaptedModules { path } => write!(
                f,
                "`{}` holds no lora_A/lora_B pair -- nothing in it is an adapted linear module",
                path.display()
            ),
        }
    }
}

impl std::error::Error for LoraError {}

// ---------------------------------------------------------------------------
// adapter_config.json
// ---------------------------------------------------------------------------

/// The parsed `adapter_config.json`, carrying only what the arithmetic and the readout
/// need. Unknown keys are ignored on purpose — PEFT adds fields every release and a
/// strict struct would refuse tomorrow's adapters for no gain.
#[derive(Clone, Debug, PartialEq)]
pub struct AdapterConfig {
    /// `peft_type`, verbatim (`"LORA"` for everything this module accepts).
    pub peft_type: Option<String>,
    /// `r` as the file declares it. ⚠️ Not what the scale is computed from — see
    /// [`AdaptedModule::r`].
    pub declared_r: Option<usize>,
    /// `lora_alpha`.
    pub lora_alpha: f64,
    /// `use_rslora`. Changes the scale from `alpha/r` to `alpha/sqrt(r)`.
    pub use_rslora: bool,
    /// `target_modules`, whether the file gave a list or a single regex string.
    pub target_modules: Vec<String>,
    /// `base_model_name_or_path` — reported, never interpreted.
    pub base_model_name_or_path: Option<String>,
    /// `alpha_pattern`: per-module alpha overrides. See [`AdapterConfig::alpha_for`].
    pub alpha_pattern: BTreeMap<String, f64>,
}

impl AdapterConfig {
    /// The alpha that applies to one adapted module.
    ///
    /// ⚠️ **PEFT matches `alpha_pattern` keys as regexes** (`.*\.{key}$`); this matches
    /// them as **literal suffixes** — a key hits if the module name equals it or ends
    /// with `.` + it. That covers every hand-written key in circulation
    /// (`"q_proj"`, `"layers.0.self_attn.k_proj"`) and does not cover a key with regex
    /// metacharacters in it, which would then silently fall back to the global
    /// `lora_alpha`. Named because a wrong alpha is a wrong norm and not an error.
    /// [`AdaptedModule::alpha_from_pattern`] records which way each module went, so a
    /// readout can say so.
    pub fn alpha_for(&self, module_name: &str) -> (f64, bool) {
        // Longest match wins, so a fully qualified key beats a bare module name.
        let mut best: Option<(&String, f64)> = None;
        for (k, v) in &self.alpha_pattern {
            let hit = module_name == k.as_str()
                || (module_name.len() > k.len() + 1
                    && module_name.ends_with(k.as_str())
                    && module_name.as_bytes()[module_name.len() - k.len() - 1] == b'.');
            if hit && best.map_or(true, |(bk, _)| k.len() > bk.len()) {
                best = Some((k, *v));
            }
        }
        match best {
            Some((_, v)) => (v, true),
            None => (self.lora_alpha, false),
        }
    }

    /// The scale `s` in `ΔW = s · B · A`, for a module whose **measured** rank is `r`.
    ///
    /// `alpha / r`, or `alpha / sqrt(r)` when `use_rslora` is set — rsLoRA's whole
    /// content is that second denominator, so reading it as the first understates every
    /// norm by `sqrt(r)` and reports no error at all.
    pub fn scale(&self, alpha: f64, r: usize) -> f64 {
        if r == 0 {
            return 0.0;
        }
        if self.use_rslora {
            alpha / (r as f64).sqrt()
        } else {
            alpha / r as f64
        }
    }
}

/// Parse `adapter_config.json` from its text.
///
/// `path` is carried only so a refusal can name the file.
pub fn parse_adapter_config(json: &str, path: &Path) -> Result<AdapterConfig, LoraError> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| LoraError::ConfigJson { path: path.to_path_buf(), why: e.to_string() })?;
    let obj = v.as_object().ok_or_else(|| LoraError::ConfigJson {
        path: path.to_path_buf(),
        why: "the top level is not a JSON object".into(),
    })?;

    let peft_type = obj.get("peft_type").and_then(|v| v.as_str()).map(str::to_string);
    if let Some(t) = &peft_type {
        if !t.eq_ignore_ascii_case("lora") {
            return Err(LoraError::UnsupportedMethod { method: t.clone() });
        }
    }
    // DoRA rides inside peft_type == "LORA" as a flag, so the flag is the only tell.
    if obj.get("use_dora").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err(LoraError::UnsupportedMethod { method: "DoRA (use_dora)".into() });
    }

    let lora_alpha = obj
        .get("lora_alpha")
        .and_then(|v| v.as_f64())
        .ok_or(LoraError::ConfigMissing { path: path.to_path_buf(), key: "lora_alpha" })?;

    let declared_r = obj.get("r").and_then(|v| v.as_u64()).map(|r| r as usize);

    // `target_modules` is a list in every adapter written by `save_pretrained`, and a
    // bare string when someone passed a regex. Both are legal PEFT.
    let target_modules = match obj.get("target_modules") {
        Some(serde_json::Value::Array(a)) => {
            a.iter().filter_map(|v| v.as_str()).map(str::to_string).collect()
        }
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    };

    let mut alpha_pattern = BTreeMap::new();
    if let Some(serde_json::Value::Object(m)) = obj.get("alpha_pattern") {
        for (k, v) in m {
            if let Some(f) = v.as_f64() {
                alpha_pattern.insert(k.clone(), f);
            }
        }
    }

    Ok(AdapterConfig {
        peft_type,
        declared_r,
        lora_alpha,
        use_rslora: obj.get("use_rslora").and_then(|v| v.as_bool()).unwrap_or(false),
        target_modules,
        base_model_name_or_path: obj
            .get("base_model_name_or_path")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        alpha_pattern,
    })
}

// ---------------------------------------------------------------------------
// safetensors
// ---------------------------------------------------------------------------

/// The element types this reader decodes.
///
/// ⚠️ **`BF16` is not `F16`.** `bf16` is the top 16 bits of an `f32` (8 exponent bits);
/// `f16` has 5. Decoding one as the other produces numbers rather than an error — a
/// `bf16` 1.0 read as `f16` is 32.0 — so the tag is honoured rather than guessed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dtype {
    F32,
    F16,
    Bf16,
}

impl Dtype {
    fn from_tag(tag: &str) -> Option<Dtype> {
        match tag {
            "F32" => Some(Dtype::F32),
            "F16" => Some(Dtype::F16),
            "BF16" => Some(Dtype::Bf16),
            _ => None,
        }
    }

    /// Bytes per element.
    pub fn width(self) -> usize {
        match self {
            Dtype::F32 => 4,
            Dtype::F16 | Dtype::Bf16 => 2,
        }
    }

    fn decode(self, b: &[u8], i: usize) -> f32 {
        match self {
            Dtype::F32 => f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]),
            Dtype::F16 => half::f16::from_bits(u16::from_le_bytes([b[i], b[i + 1]])).to_f32(),
            Dtype::Bf16 => half::bf16::from_bits(u16::from_le_bytes([b[i], b[i + 1]])).to_f32(),
        }
    }
}

/// One row of a safetensors tensor directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TensorEntry {
    pub dtype: Dtype,
    pub shape: Vec<usize>,
    /// ⚠️ **Relative to the end of the header, not to the start of the file.** The
    /// absolute position is [`SafetensorsIndex::data_start`] `+ start`. Getting this
    /// wrong reads plausible-looking garbage: the numbers come out finite and the norms
    /// come out wrong.
    pub start: u64,
    pub end: u64,
}

impl TensorEntry {
    /// Element count implied by the shape.
    pub fn elems(&self) -> u64 {
        self.shape.iter().fold(1u64, |a, d| a.saturating_mul(*d as u64))
    }
}

/// A parsed safetensors tensor directory: what is in the file and where, with not one
/// tensor byte read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafetensorsIndex {
    /// Absolute file offset where tensor payload begins: `8 + header_len`.
    pub data_start: u64,
    pub tensors: BTreeMap<String, TensorEntry>,
    /// The `__metadata__` block, if the file carries one.
    pub metadata: BTreeMap<String, String>,
}

/// The safetensors preamble: 8 bytes of little-endian `u64` header length.
pub const SAFETENSORS_LEN_PREFIX: usize = 8;

/// A header longer than this is refused rather than allocated. Real adapter directories
/// are a few hundred KB of JSON at the outside; the cap exists so a corrupt length field
/// cannot ask for a 16 EiB allocation.
pub const MAX_HEADER_BYTES: u64 = 64 * 1024 * 1024;

/// Parse the tensor directory out of a safetensors stream, reading only the header.
///
/// Format: `u64 LE header_len`, then `header_len` bytes of UTF-8 JSON mapping tensor
/// name → `{dtype, shape, data_offsets: [start, end]}`, then the tensor payload.
pub fn parse_safetensors_index<R: Read>(mut r: R) -> Result<SafetensorsIndex, LoraError> {
    let mut len_buf = [0u8; SAFETENSORS_LEN_PREFIX];
    let mut got = 0usize;
    while got < SAFETENSORS_LEN_PREFIX {
        match r.read(&mut len_buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) => {
                return Err(LoraError::HeaderJson { why: format!("the read failed: {e}") })
            }
        }
    }
    if got < SAFETENSORS_LEN_PREFIX {
        return Err(LoraError::Truncated {
            what: "the 8-byte header length".into(),
            need: SAFETENSORS_LEN_PREFIX as u64,
        });
    }
    let header_len = u64::from_le_bytes(len_buf);
    if header_len > MAX_HEADER_BYTES {
        return Err(LoraError::HeaderJson {
            why: format!(
                "the header claims {header_len} bytes, above the {MAX_HEADER_BYTES}-byte ceiling"
            ),
        });
    }
    let mut json = vec![0u8; header_len as usize];
    r.read_exact(&mut json).map_err(|_| LoraError::Truncated {
        what: format!("a {header_len}-byte header"),
        need: SAFETENSORS_LEN_PREFIX as u64 + header_len,
    })?;
    let text = String::from_utf8(json)
        .map_err(|e| LoraError::HeaderJson { why: format!("the header is not UTF-8: {e}") })?;
    parse_safetensors_header(&text, SAFETENSORS_LEN_PREFIX as u64 + header_len)
}

/// Parse a safetensors header's JSON text. `data_start` is the absolute file offset the
/// payload begins at, which the caller knows and the JSON does not state.
pub fn parse_safetensors_header(
    text: &str,
    data_start: u64,
) -> Result<SafetensorsIndex, LoraError> {
    let v: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| LoraError::HeaderJson { why: e.to_string() })?;
    let obj = v
        .as_object()
        .ok_or_else(|| LoraError::HeaderJson { why: "the header is not a JSON object".into() })?;

    let mut tensors = BTreeMap::new();
    let mut metadata = BTreeMap::new();
    for (name, entry) in obj {
        if name == "__metadata__" {
            if let Some(m) = entry.as_object() {
                for (k, val) in m {
                    if let Some(s) = val.as_str() {
                        metadata.insert(k.clone(), s.to_string());
                    }
                }
            }
            continue;
        }
        let e = entry.as_object().ok_or_else(|| LoraError::BadEntry {
            tensor: name.clone(),
            why: "the entry is not an object".into(),
        })?;
        let tag = e.get("dtype").and_then(|v| v.as_str()).ok_or_else(|| LoraError::BadEntry {
            tensor: name.clone(),
            why: "no `dtype`".into(),
        })?;
        let dtype = Dtype::from_tag(tag).ok_or_else(|| LoraError::UnsupportedDtype {
            tensor: name.clone(),
            dtype: tag.to_string(),
        })?;
        let shape: Vec<usize> = e
            .get("shape")
            .and_then(|v| v.as_array())
            .ok_or_else(|| LoraError::BadEntry {
                tensor: name.clone(),
                why: "no `shape` array".into(),
            })?
            .iter()
            .filter_map(|v| v.as_u64())
            .map(|d| d as usize)
            .collect();
        let offs = e.get("data_offsets").and_then(|v| v.as_array()).ok_or_else(|| {
            LoraError::BadEntry { tensor: name.clone(), why: "no `data_offsets`".into() }
        })?;
        if offs.len() != 2 {
            return Err(LoraError::BadEntry {
                tensor: name.clone(),
                why: format!("`data_offsets` has {} entries, not 2", offs.len()),
            });
        }
        let start = offs[0].as_u64().ok_or_else(|| LoraError::BadEntry {
            tensor: name.clone(),
            why: "`data_offsets[0]` is not a non-negative integer".into(),
        })?;
        let end = offs[1].as_u64().ok_or_else(|| LoraError::BadEntry {
            tensor: name.clone(),
            why: "`data_offsets[1]` is not a non-negative integer".into(),
        })?;
        if end < start {
            return Err(LoraError::BadEntry {
                tensor: name.clone(),
                why: format!("`data_offsets` runs backwards: [{start}, {end}]"),
            });
        }
        let entry = TensorEntry { dtype, shape, start, end };
        let declared = end - start;
        let implied = entry.elems().saturating_mul(dtype.width() as u64);
        if declared != implied {
            return Err(LoraError::BadEntry {
                tensor: name.clone(),
                why: format!(
                    "shape {:?} of {tag} implies {implied} bytes, `data_offsets` spans {declared}",
                    entry.shape
                ),
            });
        }
        tensors.insert(name.clone(), entry);
    }
    Ok(SafetensorsIndex { data_start, tensors, metadata })
}

// ---------------------------------------------------------------------------
// Tensor names → (stack, layer, module)
// ---------------------------------------------------------------------------

/// Which factor a tensor is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Factor {
    A,
    B,
}

/// Split `…q_proj.lora_A.weight` (or `…q_proj.lora_A.default.weight`) into the module
/// path and which factor it is.
///
/// ⚠️ The optional middle segment is the **adapter name**. `save_pretrained` strips it,
/// a raw `state_dict` dump keeps it, and both files are in circulation — so a reader
/// that only accepts one of them silently finds zero adapted modules in half the
/// adapters it is given.
///
/// Returns `None` for anything that is not a 2-D LoRA factor, which includes
/// `lora_embedding_A/B` (an embedding adapter, different shapes) and
/// `lora_magnitude_vector` (DoRA).
fn split_factor(name: &str) -> Option<(&str, Factor)> {
    for (marker, factor) in [(".lora_A", Factor::A), (".lora_B", Factor::B)] {
        let Some(i) = name.find(marker) else { continue };
        let prefix = &name[..i];
        let rest = &name[i + marker.len()..];
        if !rest.ends_with(".weight") || !rest.starts_with('.') {
            continue;
        }
        // Between the marker and `weight` there is either nothing at all, or exactly one
        // segment: the adapter name. Two or more segments is some other tensor whose
        // name happens to contain `.lora_A`, and claiming it would invent a module.
        let Some(mid) = rest[1..].strip_suffix("weight") else { continue };
        if mid.trim_end_matches('.').contains('.') {
            continue;
        }
        return Some((prefix, factor));
    }
    None
}

/// Where in the stack a module sits, as read from its name.
///
/// **The rule**: the first path segment that is entirely ASCII digits is the layer
/// index; the segment before it names the stack. `…model.layers.12.self_attn.q_proj`
/// gives `("layers", 12, "self_attn.q_proj")`. This generalises across the naming
/// conventions in circulation — `layers.N`, `h.N`, `blocks.N`, `encoder.layer.N` — where
/// a hard-coded `"layers."` covers one of them.
///
/// A name with no numeric segment (`…lm_head`) yields `(None, None, whole path)`.
pub fn split_site(prefix: &str) -> (Option<String>, Option<u32>, String) {
    let segs: Vec<&str> = prefix.split('.').collect();
    for (i, s) in segs.iter().enumerate() {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(n) = s.parse::<u32>() {
                let stack = if i > 0 { Some(segs[i - 1].to_string()) } else { None };
                return (stack, Some(n), segs[i + 1..].join("."));
            }
        }
    }
    (None, None, prefix.to_string())
}

// ---------------------------------------------------------------------------
// The small dense-matrix arithmetic
// ---------------------------------------------------------------------------

/// A dense row-major matrix of `f32`. Only ever the two adapter factors and the r×r
/// products of them — never `ΔW`.
#[derive(Clone, Debug, PartialEq)]
pub struct Mat {
    pub rows: usize,
    pub cols: usize,
    pub v: Vec<f32>,
}

impl Mat {
    pub fn new(rows: usize, cols: usize, v: Vec<f32>) -> Mat {
        assert_eq!(v.len(), rows * cols, "Mat data does not match its shape");
        Mat { rows, cols, v }
    }

    #[inline]
    pub fn at(&self, r: usize, c: usize) -> f32 {
        self.v[r * self.cols + c]
    }
}

/// `BᵀB` for a tall `B` (`rows × cols`), returned as a `cols × cols` row-major `f64`
/// buffer. Accumulated in `f64` because `rows` is thousands and `f32` addition of
/// thousands of squares loses digits that then show up in the norm.
fn gram_cols(m: &Mat) -> Vec<f64> {
    let n = m.cols;
    let mut g = vec![0.0f64; n * n];
    for r in 0..m.rows {
        let row = &m.v[r * n..(r + 1) * n];
        for i in 0..n {
            let vi = row[i] as f64;
            if vi == 0.0 {
                continue;
            }
            for j in i..n {
                g[i * n + j] += vi * row[j] as f64;
            }
        }
    }
    for i in 0..n {
        for j in 0..i {
            g[i * n + j] = g[j * n + i];
        }
    }
    g
}

/// `AAᵀ` for a wide `A` (`rows × cols`), returned as a `rows × rows` row-major `f64`
/// buffer.
fn gram_rows(m: &Mat) -> Vec<f64> {
    let n = m.rows;
    let mut g = vec![0.0f64; n * n];
    for i in 0..n {
        let ri = &m.v[i * m.cols..(i + 1) * m.cols];
        for j in i..n {
            let rj = &m.v[j * m.cols..(j + 1) * m.cols];
            let mut s = 0.0f64;
            for k in 0..m.cols {
                s += ri[k] as f64 * rj[k] as f64;
            }
            g[i * n + j] = s;
            g[j * n + i] = s;
        }
    }
    g
}

/// `‖B·A‖_F` with `ΔW` never formed.
///
/// `‖BA‖²_F = trace((BᵀB)(AAᵀ))`, and for symmetric `M`, `N` that trace is
/// `Σᵢⱼ Mᵢⱼ Nᵢⱼ`. `B` is `out × r`, `A` is `r × in`; both Grams are `r × r`.
pub fn frobenius_of_product(b: &Mat, a: &Mat) -> f64 {
    debug_assert_eq!(b.cols, a.rows, "B's columns and A's rows are both r");
    let n = b.cols;
    let btb = gram_cols(b);
    let aat = gram_rows(a);
    let mut acc = 0.0f64;
    for i in 0..n * n {
        acc += btb[i] * aat[i];
    }
    // Rounding can push a value that is mathematically zero a hair below it.
    acc.max(0.0).sqrt()
}

/// Householder QR of a `rows × cols` matrix, returning **only `R`**, as a
/// `min(rows, cols) × cols` row-major `f64` buffer. `Q` is never accumulated: nothing
/// downstream needs it, and forming it would be the `out × out` allocation this whole
/// module exists to avoid.
fn qr_r(m: &Mat) -> (usize, usize, Vec<f64>) {
    let (rows, cols) = (m.rows, m.cols);
    let mut a: Vec<f64> = m.v.iter().map(|x| *x as f64).collect();
    let k = rows.min(cols);
    let mut v = vec![0.0f64; rows];
    for c in 0..k {
        let mut norm = 0.0f64;
        for r in c..rows {
            let x = a[r * cols + c];
            norm += x * x;
        }
        norm = norm.sqrt();
        if norm == 0.0 {
            continue;
        }
        let head = a[c * cols + c];
        let alpha = if head > 0.0 { -norm } else { norm };
        let mut vn2 = 0.0f64;
        for r in c..rows {
            let x = if r == c { a[r * cols + c] - alpha } else { a[r * cols + c] };
            v[r] = x;
            vn2 += x * x;
        }
        if vn2 == 0.0 {
            continue;
        }
        for j in c..cols {
            let mut d = 0.0f64;
            for r in c..rows {
                d += v[r] * a[r * cols + j];
            }
            let f = 2.0 * d / vn2;
            for r in c..rows {
                a[r * cols + j] -= f * v[r];
            }
        }
    }
    let mut r_out = vec![0.0f64; k * cols];
    for i in 0..k {
        for j in i..cols {
            r_out[i * cols + j] = a[i * cols + j];
        }
    }
    (k, cols, r_out)
}

/// Singular values of a small dense `rows × cols` `f64` matrix, descending.
///
/// One-sided Jacobi: rotate pairs of columns until they are mutually orthogonal, then
/// the singular values are the resulting column norms. Chosen over "eigenvalues of
/// `MᵀM`" because forming `MᵀM` squares the condition number, and a rank-deficient
/// update — precisely the interesting case here — is where that costs the most digits.
fn jacobi_singular_values(rows: usize, cols: usize, m: &[f64]) -> Vec<f64> {
    // One-sided Jacobi wants at least as many rows as columns. Singular values are
    // invariant under transposition, so transpose when it does not.
    let (rows, cols, m) = if rows >= cols {
        (rows, cols, m.to_vec())
    } else {
        let mut t = vec![0.0f64; rows * cols];
        for i in 0..rows {
            for j in 0..cols {
                t[j * rows + i] = m[i * cols + j];
            }
        }
        (cols, rows, t)
    };
    // Columns, contiguous, for cheap dot products.
    let mut c: Vec<Vec<f64>> = (0..cols)
        .map(|j| (0..rows).map(|i| m[i * cols + j]).collect::<Vec<f64>>())
        .collect();

    const SWEEPS: usize = 60;
    let eps = 1e-15f64;
    for _ in 0..SWEEPS {
        let mut off = 0.0f64;
        for p in 0..cols.saturating_sub(1) {
            for q in p + 1..cols {
                let mut alpha = 0.0;
                let mut beta = 0.0;
                let mut gamma = 0.0;
                for i in 0..rows {
                    alpha += c[p][i] * c[p][i];
                    beta += c[q][i] * c[q][i];
                    gamma += c[p][i] * c[q][i];
                }
                if gamma == 0.0 || alpha == 0.0 || beta == 0.0 {
                    continue;
                }
                let scale = (alpha * beta).sqrt();
                if gamma.abs() <= eps * scale {
                    continue;
                }
                off += (gamma / scale).abs();
                let zeta = (beta - alpha) / (2.0 * gamma);
                let t = zeta.signum() / (zeta.abs() + (1.0 + zeta * zeta).sqrt());
                let cs = 1.0 / (1.0 + t * t).sqrt();
                let sn = cs * t;
                for i in 0..rows {
                    let (x, y) = (c[p][i], c[q][i]);
                    c[p][i] = cs * x - sn * y;
                    c[q][i] = sn * x + cs * y;
                }
            }
        }
        if off == 0.0 {
            break;
        }
    }
    let mut sv: Vec<f64> =
        c.iter().map(|col| col.iter().map(|x| x * x).sum::<f64>().sqrt()).collect();
    sv.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    sv
}

/// Singular values of `B·A`, descending, without forming `B·A`.
///
/// `B = Q_B R_B` and `Aᵀ = Q_A R_A`, so `BA = Q_B (R_B R_Aᵀ) Q_Aᵀ`. `Q_B` and `Q_A`
/// have orthonormal columns, so they leave the spectrum alone and only the small
/// `R_B R_Aᵀ` — `min(out, r) × min(in, r)` — carries it. The count returned is
/// `min(out, r, in)`, which is the true maximum rank of the product.
pub fn singular_values_of_product(b: &Mat, a: &Mat) -> Vec<f64> {
    debug_assert_eq!(b.cols, a.rows, "B's columns and A's rows are both r");
    let r = b.cols;
    let (kb, _, rb) = qr_r(b); // kb × r
    // Aᵀ is in × r. Build it rather than transposing inside the QR, which would need a
    // second code path for no gain: `in × r` is the same size as `A` itself.
    let at = Mat::new(a.cols, a.rows, {
        let mut t = vec![0.0f32; a.cols * a.rows];
        for i in 0..a.rows {
            for j in 0..a.cols {
                t[j * a.rows + i] = a.at(i, j);
            }
        }
        t
    });
    let (ka, _, ra) = qr_r(&at); // ka × r
    // M = R_B · R_Aᵀ  →  kb × ka
    let mut m = vec![0.0f64; kb * ka];
    for i in 0..kb {
        for j in 0..ka {
            let mut s = 0.0f64;
            for k in 0..r {
                s += rb[i * r + k] * ra[j * r + k];
            }
            m[i * ka + j] = s;
        }
    }
    jacobi_singular_values(kb, ka, &m)
}

/// **Effective rank** in the sense of Roy & Vetterli (2007): `exp(H(p))` where
/// `pᵢ = σᵢ / Σσⱼ` and `H` is Shannon entropy in nats.
///
/// - `r` equal singular values → exactly `r`.
/// - exactly one nonzero singular value → exactly `1`.
/// - an all-zero spectrum → `0`, which is the honest answer for a module the fine-tune
///   did not move at all (and is outside Roy & Vetterli's domain, which assumes a
///   nonzero matrix — stated rather than papered over).
pub fn effective_rank(sv: &[f64]) -> f64 {
    let total: f64 = sv.iter().map(|s| s.max(0.0)).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let mut h = 0.0f64;
    for s in sv {
        let p = s.max(0.0) / total;
        if p > 0.0 {
            h -= p * p.ln();
        }
    }
    h.exp()
}

/// **Stable rank**: `‖M‖²_F / σ²_max = Σσᵢ² / σ²_max`.
///
/// The other scalar people mean by "effective rank". It is dominated by the largest
/// singular value where [`effective_rank`] counts the whole tail, so the two disagree
/// on exactly the spectra worth looking at. Both are reported rather than one being
/// chosen on the reader's behalf.
pub fn stable_rank(sv: &[f64]) -> f64 {
    let max = sv.iter().cloned().fold(0.0f64, f64::max);
    if max <= 0.0 {
        return 0.0;
    }
    sv.iter().map(|s| s * s).sum::<f64>() / (max * max)
}

// ---------------------------------------------------------------------------
// The result types
// ---------------------------------------------------------------------------

/// What one adapted module's update is, measured.
#[derive(Clone, Debug, PartialEq)]
pub struct AdaptedModule {
    /// The full state-dict path with the `.lora_A.weight` tail removed, e.g.
    /// `base_model.model.model.layers.12.self_attn.q_proj`.
    pub name: String,
    /// The stack segment before the layer index (`"layers"`), when the name has one.
    pub stack: Option<String>,
    /// The layer index parsed out of the name, when the name has one.
    pub layer: Option<u32>,
    /// What remains after the layer index — `"self_attn.q_proj"`.
    pub module: String,
    /// The rank, **measured from the tensor shapes** rather than taken from
    /// `adapter_config.json`. ⚠️ PEFT's `rank_pattern` can give a module a rank the
    /// top-level `r` does not state; the shape cannot be wrong about it, so the config
    /// is never consulted for this. [`AdapterSummary::rank_disagrees_with_config`]
    /// reports the discrepancy rather than hiding it.
    pub r: usize,
    /// `A`'s column count.
    pub in_features: usize,
    /// `B`'s row count.
    pub out_features: usize,
    /// The alpha that applied, after [`AdapterConfig::alpha_for`].
    pub alpha: f64,
    /// Whether that alpha came from `alpha_pattern` rather than the global `lora_alpha`.
    pub alpha_from_pattern: bool,
    /// `s` in `ΔW = s·B·A`.
    pub scale: f64,
    /// `‖ΔW‖_F`. **measured** — an exact function of the file.
    pub delta_fro: f64,
    /// The singular values of `ΔW`, descending, `min(out, r, in)` of them. **measured**.
    pub singular_values: Vec<f64>,
    /// [`effective_rank`] of `singular_values`. **derived**.
    pub effective_rank: f64,
    /// [`stable_rank`] of `singular_values`. **derived**.
    pub stable_rank: f64,
}

/// Everything read out of one adapter directory.
#[derive(Clone, Debug, PartialEq)]
pub struct AdapterSummary {
    pub config: AdapterConfig,
    /// Adapted modules, ordered by layer then by name — the order a per-layer readout
    /// wants, and stable across runs because the safetensors directory is a `BTreeMap`.
    pub modules: Vec<AdaptedModule>,
    /// Tensors in the file that are not part of a 2-D LoRA pair — embedding adapters,
    /// biases, anything else. Named rather than dropped, so "the reader saw 384 tensors
    /// and understood 336 of them" is answerable.
    pub skipped: Vec<String>,
    /// Modules whose measured rank differs from `adapter_config.json`'s `r`. Empty for
    /// an ordinary adapter; non-empty means `rank_pattern` was in play (or the config
    /// is wrong), and either way the shapes won.
    pub rank_disagrees_with_config: Vec<String>,
}

impl AdapterSummary {
    /// `‖ΔW‖_F` summed in quadrature per layer — `sqrt(Σ ‖ΔW_m‖²_F)` over the modules of
    /// that layer, which is the Frobenius norm of the layer's whole update taken as one
    /// block. ⚠️ **Not** a sum of norms: norms do not add, and adding them would
    /// overstate every layer by a different amount.
    ///
    /// Modules with no layer index are excluded; they are in [`AdapterSummary::modules`]
    /// with `layer: None`.
    pub fn per_layer_fro(&self) -> BTreeMap<u32, f64> {
        let mut acc: BTreeMap<u32, f64> = BTreeMap::new();
        for m in &self.modules {
            if let Some(l) = m.layer {
                *acc.entry(l).or_insert(0.0) += m.delta_fro * m.delta_fro;
            }
        }
        acc.into_iter().map(|(l, sq)| (l, sq.sqrt())).collect()
    }
}

// ---------------------------------------------------------------------------
// Reading a directory
// ---------------------------------------------------------------------------

/// The two file names PEFT writes.
pub const ADAPTER_CONFIG: &str = "adapter_config.json";
pub const ADAPTER_WEIGHTS: &str = "adapter_model.safetensors";

/// Read a PEFT LoRA adapter directory and measure every adapted module.
///
/// Reads the config, the safetensors header, and then **only the `lora_A`/`lora_B`
/// byte ranges**, one module at a time — peak memory is one module's two factors, not
/// the adapter.
pub fn read_adapter_dir<P: AsRef<Path>>(dir: P) -> Result<AdapterSummary, LoraError> {
    let dir = dir.as_ref();
    let cfg_path = dir.join(ADAPTER_CONFIG);
    let mut text = String::new();
    File::open(&cfg_path)
        .and_then(|mut f| f.read_to_string(&mut text))
        .map_err(|e| LoraError::Io { path: cfg_path.clone(), why: e.to_string() })?;
    let config = parse_adapter_config(&text, &cfg_path)?;

    let w_path = dir.join(ADAPTER_WEIGHTS);
    let mut file = File::open(&w_path)
        .map_err(|e| LoraError::Io { path: w_path.clone(), why: e.to_string() })?;
    read_adapter(&mut file, config, &w_path)
}

/// The directory read, with the weights coming from any seekable stream. Split out so a
/// test can hand it bytes it built, which is the whole verification story for this
/// module until a real adapter is on the machine.
pub fn read_adapter<R: Read + Seek>(
    src: &mut R,
    config: AdapterConfig,
    weights_path: &Path,
) -> Result<AdapterSummary, LoraError> {
    let index = parse_safetensors_index(&mut *src)?;

    // Group the directory into (prefix → A entry, B entry).
    let mut pairs: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
    let mut skipped = Vec::new();
    for name in index.tensors.keys() {
        match split_factor(name) {
            Some((prefix, Factor::A)) => {
                pairs.entry(prefix.to_string()).or_default().0 = Some(name.clone())
            }
            Some((prefix, Factor::B)) => {
                pairs.entry(prefix.to_string()).or_default().1 = Some(name.clone())
            }
            None => {
                if name.contains("lora_magnitude_vector") {
                    return Err(LoraError::UnsupportedMethod {
                        method: format!("DoRA (`{name}` is a magnitude vector)"),
                    });
                }
                skipped.push(name.clone());
            }
        }
    }
    if pairs.is_empty() {
        return Err(LoraError::NoAdaptedModules { path: weights_path.to_path_buf() });
    }

    let mut modules = Vec::with_capacity(pairs.len());
    let mut rank_disagrees_with_config = Vec::new();
    for (prefix, (a_name, b_name)) in pairs {
        let (a_name, b_name) = match (a_name, b_name) {
            (Some(a), Some(b)) => (a, b),
            (Some(_), None) => {
                return Err(LoraError::Unpaired { module: prefix, have: "lora_A" })
            }
            (None, Some(_)) => {
                return Err(LoraError::Unpaired { module: prefix, have: "lora_B" })
            }
            (None, None) => unreachable!("a pair entry exists only because a name landed in it"),
        };
        let ae = &index.tensors[&a_name];
        let be = &index.tensors[&b_name];
        if ae.shape.len() != 2 || be.shape.len() != 2 || ae.shape[0] != be.shape[1] {
            return Err(LoraError::ShapeMismatch {
                module: prefix,
                a: ae.shape.clone(),
                b: be.shape.clone(),
            });
        }
        let r = ae.shape[0];
        let in_features = ae.shape[1];
        let out_features = be.shape[0];

        let a = read_matrix(src, index.data_start, ae, &a_name, r, in_features)?;
        let b = read_matrix(src, index.data_start, be, &b_name, out_features, r)?;

        let (alpha, alpha_from_pattern) = config.alpha_for(&prefix);
        let scale = config.scale(alpha, r);

        let base_fro = frobenius_of_product(&b, &a);
        let sv: Vec<f64> =
            singular_values_of_product(&b, &a).into_iter().map(|s| s * scale.abs()).collect();

        if config.declared_r.map_or(false, |cr| cr != r) {
            rank_disagrees_with_config.push(prefix.clone());
        }

        let (stack, layer, module) = split_site(&prefix);
        modules.push(AdaptedModule {
            name: prefix,
            stack,
            layer,
            module,
            r,
            in_features,
            out_features,
            alpha,
            alpha_from_pattern,
            scale,
            delta_fro: base_fro * scale.abs(),
            effective_rank: effective_rank(&sv),
            stable_rank: stable_rank(&sv),
            singular_values: sv,
        });
    }

    modules.sort_by(|x, y| match (x.layer, y.layer) {
        (Some(a), Some(b)) => a.cmp(&b).then_with(|| x.name.cmp(&y.name)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => x.name.cmp(&y.name),
    });

    Ok(AdapterSummary { config, modules, skipped, rank_disagrees_with_config })
}

/// Read one tensor's bytes and decode them into a row-major `f32` matrix.
fn read_matrix<R: Read + Seek>(
    src: &mut R,
    data_start: u64,
    e: &TensorEntry,
    name: &str,
    rows: usize,
    cols: usize,
) -> Result<Mat, LoraError> {
    let len = (e.end - e.start) as usize;
    src.seek(SeekFrom::Start(data_start + e.start)).map_err(|_| LoraError::Truncated {
        what: format!("tensor `{name}`"),
        need: data_start + e.end,
    })?;
    let mut raw = vec![0u8; len];
    src.read_exact(&mut raw).map_err(|_| LoraError::Truncated {
        what: format!("tensor `{name}`"),
        need: data_start + e.end,
    })?;
    let w = e.dtype.width();
    let mut v = Vec::with_capacity(rows * cols);
    for i in 0..rows * cols {
        v.push(e.dtype.decode(&raw, i * w));
    }
    Ok(Mat::new(rows, cols, v))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // -- fixtures ----------------------------------------------------------

    /// A `rows × cols` matrix from a closure over (row, col).
    fn mat(rows: usize, cols: usize, f: impl Fn(usize, usize) -> f32) -> Mat {
        let mut v = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            for c in 0..cols {
                v.push(f(r, c));
            }
        }
        Mat::new(rows, cols, v)
    }

    /// **The hand-constructed pair whose answer can be stated.**
    ///
    /// `B` is `out × r` with `B[i][i] = d[i]` and zero elsewhere; `A` is `r × in` with
    /// `A[i][i] = 1`. Then `B·A` is `out × in` with `d[i]` on the diagonal and nothing
    /// else, so its singular values are exactly `d` (sorted) and its Frobenius norm is
    /// exactly `‖d‖`. Every spectral claim in this module is checked against this.
    fn diag_pair(out: usize, r: usize, inf: usize, d: &[f32]) -> (Mat, Mat) {
        assert_eq!(d.len(), r);
        let b = mat(out, r, |i, j| if i == j { d[j] } else { 0.0 });
        let a = mat(r, inf, |i, j| if i == j { 1.0 } else { 0.0 });
        (b, a)
    }

    /// A Givens rotation applied to two rows of `B` and two columns of `Aᵀ` leaves the
    /// spectrum of `B·A` alone while making both factors dense — which is what turns
    /// the diagonal fixture into a real test of the QR path.
    fn rotate_rows(m: &Mat, p: usize, q: usize, theta: f32) -> Mat {
        let (c, s) = (theta.cos(), theta.sin());
        let mut out = m.clone();
        for j in 0..m.cols {
            let (x, y) = (m.at(p, j), m.at(q, j));
            out.v[p * m.cols + j] = c * x - s * y;
            out.v[q * m.cols + j] = s * x + c * y;
        }
        out
    }

    fn rotate_cols(m: &Mat, p: usize, q: usize, theta: f32) -> Mat {
        let (c, s) = (theta.cos(), theta.sin());
        let mut out = m.clone();
        for i in 0..m.rows {
            let (x, y) = (m.at(i, p), m.at(i, q));
            out.v[i * m.cols + p] = c * x - s * y;
            out.v[i * m.cols + q] = s * x + c * y;
        }
        out
    }

    /// A deterministic pseudo-random `f32` in `[-1, 1)`. No `rand` dependency, and a
    /// fixed seed so a failure is reproducible rather than a once-a-week mystery.
    fn lcg(seed: &mut u64) -> f32 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    }

    fn dense(rows: usize, cols: usize, seed: &mut u64) -> Mat {
        let mut v = Vec::with_capacity(rows * cols);
        for _ in 0..rows * cols {
            v.push(lcg(seed));
        }
        Mat::new(rows, cols, v)
    }

    /// `B·A`, materialized in full.
    ///
    /// 🚨 **Only a test may do this.** It is exactly the `out × in` product the module
    /// exists to avoid, and it is here for one reason: a shortcut has to be checkable
    /// against the thing it is a shortcut *for*, and the tiny shapes a test uses make
    /// the forbidden version affordable.
    fn naive_product(b: &Mat, a: &Mat) -> Mat {
        let mut v = vec![0.0f32; b.rows * a.cols];
        for i in 0..b.rows {
            for k in 0..b.cols {
                let bik = b.at(i, k);
                for j in 0..a.cols {
                    v[i * a.cols + j] += bik * a.at(k, j);
                }
            }
        }
        Mat::new(b.rows, a.cols, v)
    }

    fn approx(a: f64, b: f64, tol: f64, what: &str) {
        assert!((a - b).abs() <= tol, "{what}: got {a}, expected {b} (tol {tol})");
    }

    // -- the arithmetic ----------------------------------------------------

    #[test]
    fn frobenius_of_a_hand_built_pair_is_the_stated_number() {
        // d = [3, 4, 0] -> B*A has 3 and 4 on the diagonal -> ||.||_F = 5 exactly.
        let (b, a) = diag_pair(8, 3, 6, &[3.0, 4.0, 0.0]);
        approx(frobenius_of_product(&b, &a), 5.0, 1e-9, "||BA||_F of diag(3,4,0)");
    }

    #[test]
    fn singular_values_of_a_hand_built_pair_are_the_stated_spectrum() {
        let (b, a) = diag_pair(8, 3, 6, &[3.0, 4.0, 0.0]);
        let sv = singular_values_of_product(&b, &a);
        assert_eq!(sv.len(), 3, "min(out, r, in) singular values");
        approx(sv[0], 4.0, 1e-9, "sigma_0");
        approx(sv[1], 3.0, 1e-9, "sigma_1");
        approx(sv[2], 0.0, 1e-9, "sigma_2");
    }

    #[test]
    fn a_rotation_makes_both_factors_dense_and_moves_no_singular_value() {
        let (b, a) = diag_pair(8, 3, 6, &[3.0, 4.0, 1.0]);
        let plain = singular_values_of_product(&b, &a);
        // Rotate rows of B (a left orthogonal action) and columns of A (a right one).
        // Both leave the spectrum of B*A alone, and neither leaves a zero in place.
        let b2 = rotate_rows(&rotate_rows(&b, 0, 5, 0.7), 1, 2, -1.1);
        let a2 = rotate_cols(&rotate_cols(&a, 0, 4, 0.3), 2, 3, 1.9);
        assert!(b2.v.iter().filter(|x| **x != 0.0).count() > b.v.iter().filter(|x| **x != 0.0).count());
        let rotated = singular_values_of_product(&b2, &a2);
        for i in 0..plain.len() {
            approx(rotated[i], plain[i], 1e-5, &format!("sigma_{i} under rotation"));
        }
    }

    #[test]
    fn non_orthogonal_factors_need_the_off_diagonal_terms_of_the_trace() {
        // 🚨 The test the first draft of this module did NOT have, and its absence let a
        // deliberately broken `frobenius_of_product` -- one summing only the DIAGONAL of
        // (BᵀB)(AAᵀ) -- pass the whole suite. Every fixture above builds B with
        // orthogonal columns and A with orthogonal rows, and an orthogonal-column
        // rotation leaves BᵀB alone, so both Grams stayed diagonal and the missing terms
        // were all zero.
        //
        //   B = [[1,1],[0,1]]   A = [[1,0],[1,1]]   ->   BA = [[2,1],[1,1]]
        //   ||BA||_F = sqrt(4+1+1+1) = sqrt(7)
        //   BᵀB = AAᵀ = [[1,1],[1,2]]  -- off-diagonal 1, not 0
        //   trace(MN) = 1+1+1+4 = 7 ; the diagonal alone gives 1+4 = 5
        let b = Mat::new(2, 2, vec![1.0, 1.0, 0.0, 1.0]);
        let a = Mat::new(2, 2, vec![1.0, 0.0, 1.0, 1.0]);
        approx(frobenius_of_product(&b, &a), 7.0f64.sqrt(), 1e-9, "||BA||_F of a dense pair");
        // BA is symmetric PSD, so its singular values are its eigenvalues:
        // lambda^2 - 3*lambda + 1 = 0  ->  (3 +- sqrt(5)) / 2.
        let sv = singular_values_of_product(&b, &a);
        approx(sv[0], (3.0 + 5.0f64.sqrt()) / 2.0, 1e-9, "sigma_0");
        approx(sv[1], (3.0 - 5.0f64.sqrt()) / 2.0, 1e-9, "sigma_1");
    }

    #[test]
    fn the_shortcut_agrees_with_the_product_it_is_a_shortcut_for() {
        // Dense random factors of three shapes, including one where r exceeds out --
        // so the QR path's min(out, r, in) reasoning is exercised rather than assumed.
        let mut seed = 0x0117_2026_u64;
        for (out, r, inf) in [(7usize, 3usize, 5usize), (5, 4, 9), (3, 5, 4)] {
            let b = dense(out, r, &mut seed);
            let a = dense(r, inf, &mut seed);
            let w = naive_product(&b, &a);
            let direct = w.v.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
            approx(
                frobenius_of_product(&b, &a),
                direct,
                1e-4,
                &format!("||BA||_F for {out}x{r}x{inf}"),
            );
            let full: Vec<f64> = w.v.iter().map(|x| *x as f64).collect();
            let mut want = jacobi_singular_values(w.rows, w.cols, &full);
            want.truncate(out.min(r).min(inf));
            let got = singular_values_of_product(&b, &a);
            assert_eq!(got.len(), want.len(), "min(out, r, in) singular values");
            for i in 0..got.len() {
                approx(got[i], want[i], 1e-4, &format!("sigma_{i} for {out}x{r}x{inf}"));
            }
        }
    }

    #[test]
    fn the_frobenius_shortcut_and_the_singular_values_agree() {
        // ||M||_F^2 = sum(sigma_i^2). The two are computed by completely different
        // routes -- Gram trace vs QR + Jacobi -- so agreement pins both.
        let (b, a) = diag_pair(9, 4, 7, &[2.0, 5.0, 0.5, 3.0]);
        let b = rotate_rows(&b, 0, 6, 0.4);
        let a = rotate_cols(&a, 1, 5, -0.9);
        let fro = frobenius_of_product(&b, &a);
        let sv = singular_values_of_product(&b, &a);
        let from_sv = sv.iter().map(|s| s * s).sum::<f64>().sqrt();
        approx(fro, from_sv, 1e-5, "||BA||_F two ways");
    }

    #[test]
    fn effective_rank_of_r_equal_values_is_exactly_r() {
        for r in 1..=16usize {
            let sv = vec![2.5f64; r];
            approx(effective_rank(&sv), r as f64, 1e-9, &format!("erank of {r} equal values"));
        }
    }

    #[test]
    fn effective_rank_of_one_nonzero_value_is_one() {
        approx(effective_rank(&[7.0, 0.0, 0.0, 0.0]), 1.0, 1e-12, "erank of a rank-1 spectrum");
    }

    #[test]
    fn effective_rank_of_nothing_is_zero() {
        approx(effective_rank(&[0.0, 0.0]), 0.0, 0.0, "erank of a module that did not move");
        approx(effective_rank(&[]), 0.0, 0.0, "erank of an empty spectrum");
    }

    #[test]
    fn effective_rank_of_a_hand_computed_spectrum() {
        // sigma = {4, 3}, so p = (4/7, 3/7) and exp(H) = prod p^-p, which has a closed
        // form with no exponential in it at all:
        //
        //     exp(H) = (4/7)^(-4/7) * (3/7)^(-3/7) = 7 / (4^(4/7) * 3^(3/7))
        //            = 7 / (2.2081474 * 1.6013297) = 7 / 3.5359844 = 1.97962633
        //
        // ⚠️ Stated this way on purpose: the first version of this test carried a
        // by-hand exp() that was wrong in the fourth decimal, and the failure read as a
        // bug in `effective_rank` rather than in the expectation. A closed form is
        // checkable without trusting the thing under test.
        approx(effective_rank(&[4.0, 3.0]), 1.97962633, 1e-7, "erank of {4, 3}");
    }

    #[test]
    fn effective_rank_is_scale_invariant_and_stable_rank_is_too() {
        let sv = [5.0, 2.0, 1.0, 0.25];
        let scaled: Vec<f64> = sv.iter().map(|s| s * 137.0).collect();
        approx(effective_rank(&scaled), effective_rank(&sv), 1e-9, "erank under scaling");
        approx(stable_rank(&scaled), stable_rank(&sv), 1e-9, "stable rank under scaling");
    }

    #[test]
    fn stable_rank_and_effective_rank_disagree_and_that_is_the_point() {
        // A long thin tail: one big value, many small ones. Stable rank barely leaves 1;
        // entropy rank counts the tail. Both are reported precisely because of this.
        let mut sv = vec![10.0f64];
        sv.extend(std::iter::repeat(0.5).take(15));
        let sr = stable_rank(&sv);
        let er = effective_rank(&sv);
        approx(sr, 1.0 + 15.0 * 0.25 / 100.0, 1e-9, "stable rank of the thin tail");
        assert!(sr < 1.1, "stable rank stays near 1: {sr}");
        assert!(er > 1.5, "entropy rank counts the tail: {er}");
    }

    #[test]
    fn rank_deficiency_survives_the_qr_path() {
        // B's columns are linearly dependent: column 2 is column 0 + column 1. The
        // product is genuinely rank 2 out of r = 3, and the Jacobi path must say so
        // rather than returning three comparable values.
        let b = mat(6, 3, |i, j| match j {
            0 => if i == 0 { 1.0 } else { 0.0 },
            1 => if i == 1 { 1.0 } else { 0.0 },
            _ => if i < 2 { 1.0 } else { 0.0 },
        });
        let a = mat(3, 5, |i, j| if i == j { 1.0 } else { 0.0 });
        let sv = singular_values_of_product(&b, &a);
        assert_eq!(sv.len(), 3);
        assert!(sv[2] < 1e-6, "the dependent direction must vanish, got {}", sv[2]);
        approx(effective_rank(&sv), effective_rank(&sv[..2].to_vec()), 1e-6, "erank ignores the null direction");
    }

    #[test]
    fn a_wide_module_costs_no_out_by_in_product() {
        // out = in = 2048, r = 4. Materializing dW would be 4.2M floats; this is 16k.
        // The answer is still the stated one.
        let (b, a) = diag_pair(2048, 4, 2048, &[1.0, 2.0, 2.0, 4.0]);
        approx(frobenius_of_product(&b, &a), 5.0, 1e-6, "||BA||_F of diag(1,2,2,4)");
        let sv = singular_values_of_product(&b, &a);
        approx(sv[0], 4.0, 1e-6, "sigma_0");
        approx(sv[3], 1.0, 1e-6, "sigma_3");
    }

    // -- adapter_config.json -----------------------------------------------

    fn cfg_json(extra: &str) -> String {
        format!(
            r#"{{"peft_type":"LORA","r":8,"lora_alpha":16,
                "target_modules":["q_proj","v_proj"],
                "base_model_name_or_path":"unsloth/gemma-4-12b-it"{extra}}}"#
        )
    }

    #[test]
    fn a_plain_config_parses_and_the_scale_is_alpha_over_r() {
        let c = parse_adapter_config(&cfg_json(""), Path::new("adapter_config.json")).unwrap();
        assert_eq!(c.declared_r, Some(8));
        assert_eq!(c.lora_alpha, 16.0);
        assert_eq!(c.target_modules, vec!["q_proj", "v_proj"]);
        assert!(!c.use_rslora);
        approx(c.scale(16.0, 8), 2.0, 1e-12, "alpha/r");
    }

    #[test]
    fn rslora_divides_by_sqrt_r_and_the_difference_is_not_an_error() {
        let c = parse_adapter_config(&cfg_json(r#","use_rslora":true"#), Path::new("c")).unwrap();
        assert!(c.use_rslora);
        approx(c.scale(16.0, 16), 4.0, 1e-12, "alpha/sqrt(r)");
        // The plain reading would be 1.0 -- a factor of four, silently.
        let plain = parse_adapter_config(&cfg_json(""), Path::new("c")).unwrap();
        approx(plain.scale(16.0, 16), 1.0, 1e-12, "alpha/r");
    }

    #[test]
    fn dora_is_refused_rather_than_read_as_lora() {
        let e = parse_adapter_config(&cfg_json(r#","use_dora":true"#), Path::new("c")).unwrap_err();
        assert!(
            matches!(&e, LoraError::UnsupportedMethod { method } if method.contains("DoRA")),
            "got {e:?}"
        );
    }

    #[test]
    fn a_non_lora_peft_type_is_refused() {
        let json = r#"{"peft_type":"IA3","lora_alpha":16}"#;
        let e = parse_adapter_config(json, Path::new("c")).unwrap_err();
        assert!(matches!(e, LoraError::UnsupportedMethod { .. }), "got {e:?}");
    }

    #[test]
    fn a_config_with_no_alpha_names_the_file_and_the_key() {
        let e = parse_adapter_config(r#"{"peft_type":"LORA","r":8}"#, Path::new("a/b.json"))
            .unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("b.json") && msg.contains("lora_alpha"), "got: {msg}");
    }

    #[test]
    fn target_modules_may_be_a_bare_regex_string() {
        let json = r#"{"peft_type":"LORA","r":8,"lora_alpha":16,"target_modules":".*proj"}"#;
        let c = parse_adapter_config(json, Path::new("c")).unwrap();
        assert_eq!(c.target_modules, vec![".*proj"]);
    }

    #[test]
    fn alpha_pattern_matches_by_suffix_and_the_longest_key_wins() {
        let extra = r#","alpha_pattern":{"q_proj":32,"layers.3.self_attn.q_proj":64}"#;
        let c = parse_adapter_config(&cfg_json(extra), Path::new("c")).unwrap();
        assert_eq!(c.alpha_for("base_model.model.layers.7.self_attn.q_proj"), (32.0, true));
        assert_eq!(c.alpha_for("base_model.model.layers.3.self_attn.q_proj"), (64.0, true));
        assert_eq!(c.alpha_for("base_model.model.layers.3.self_attn.v_proj"), (16.0, false));
        // A suffix that is not on a segment boundary must not match.
        assert_eq!(c.alpha_for("base_model.model.layers.3.self_attn.xq_proj"), (16.0, false));
    }

    // -- names -------------------------------------------------------------

    #[test]
    fn the_layer_index_is_the_first_all_digit_segment() {
        assert_eq!(
            split_site("base_model.model.model.layers.12.self_attn.q_proj"),
            (Some("layers".into()), Some(12), "self_attn.q_proj".into())
        );
        assert_eq!(
            split_site("transformer.h.5.attn.c_attn"),
            (Some("h".into()), Some(5), "attn.c_attn".into())
        );
        assert_eq!(
            split_site("base_model.model.lm_head"),
            (None, None, "base_model.model.lm_head".into())
        );
    }

    #[test]
    fn both_spellings_of_a_factor_name_are_understood() {
        // `save_pretrained` strips the adapter name; a raw state_dict dump keeps it.
        // Accepting only one silently finds zero modules in half the adapters.
        assert_eq!(split_factor("m.layers.0.q_proj.lora_A.weight"), Some(("m.layers.0.q_proj", Factor::A)));
        assert_eq!(
            split_factor("m.layers.0.q_proj.lora_B.default.weight"),
            Some(("m.layers.0.q_proj", Factor::B))
        );
    }

    #[test]
    fn things_that_are_not_two_dimensional_factors_are_not_claimed() {
        assert_eq!(split_factor("m.embed.lora_embedding_A"), None);
        assert_eq!(split_factor("m.layers.0.q_proj.lora_magnitude_vector"), None);
        assert_eq!(split_factor("m.layers.0.q_proj.weight"), None);
        assert_eq!(split_factor("m.layers.0.q_proj.lora_A.bias"), None);
    }

    // -- safetensors -------------------------------------------------------

    /// Build a safetensors file from (name, dtype, shape, data) in the order given.
    fn safetensors(entries: &[(&str, Dtype, Vec<usize>, Vec<f32>)]) -> Vec<u8> {
        let mut payload: Vec<u8> = Vec::new();
        let mut fields: Vec<String> = Vec::new();
        for (name, dt, shape, data) in entries {
            let start = payload.len();
            for x in data {
                match dt {
                    Dtype::F32 => payload.extend_from_slice(&x.to_le_bytes()),
                    Dtype::F16 => {
                        payload.extend_from_slice(&half::f16::from_f32(*x).to_bits().to_le_bytes())
                    }
                    Dtype::Bf16 => {
                        payload.extend_from_slice(&half::bf16::from_f32(*x).to_bits().to_le_bytes())
                    }
                }
            }
            let tag = match dt {
                Dtype::F32 => "F32",
                Dtype::F16 => "F16",
                Dtype::Bf16 => "BF16",
            };
            fields.push(format!(
                r#""{name}":{{"dtype":"{tag}","shape":{shape:?},"data_offsets":[{start},{}]}}"#,
                payload.len()
            ));
        }
        let header = format!("{{{}}}", fields.join(","));
        let mut out = Vec::new();
        out.extend_from_slice(&(header.len() as u64).to_le_bytes());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&payload);
        out
    }

    fn flat(m: &Mat) -> Vec<f32> {
        m.v.clone()
    }

    /// One adapted module at `layers.<l>.<module>`, from a hand-built pair.
    fn one_module(l: u32, module: &str, b: &Mat, a: &Mat, dt: Dtype) -> Vec<(String, Dtype, Vec<usize>, Vec<f32>)> {
        let p = format!("base_model.model.model.layers.{l}.{module}");
        vec![
            (format!("{p}.lora_A.weight"), dt, vec![a.rows, a.cols], flat(a)),
            (format!("{p}.lora_B.weight"), dt, vec![b.rows, b.cols], flat(b)),
        ]
    }

    fn build(entries: Vec<(String, Dtype, Vec<usize>, Vec<f32>)>) -> Vec<u8> {
        let borrowed: Vec<(&str, Dtype, Vec<usize>, Vec<f32>)> =
            entries.iter().map(|(n, d, s, v)| (n.as_str(), *d, s.clone(), v.clone())).collect();
        safetensors(&borrowed)
    }

    #[test]
    fn the_header_is_read_and_the_payload_is_not() {
        let (b, a) = diag_pair(4, 2, 3, &[3.0, 4.0]);
        let bytes = build(one_module(7, "self_attn.q_proj", &b, &a, Dtype::F32));
        let idx = parse_safetensors_index(Cursor::new(&bytes)).unwrap();
        assert_eq!(idx.tensors.len(), 2);
        let e = &idx.tensors["base_model.model.model.layers.7.self_attn.q_proj.lora_A.weight"];
        assert_eq!(e.shape, vec![2, 3]);
        assert_eq!(e.dtype, Dtype::F32);
        let header_len = u64::from_le_bytes(bytes[..8].try_into().unwrap());
        assert_eq!(idx.data_start, 8 + header_len, "payload begins after the header");
        assert_eq!(e.start, 0);
    }

    #[test]
    fn data_offsets_are_relative_to_the_end_of_the_header() {
        // The trap: reading `start` as absolute would land inside the header JSON and
        // decode text bytes as floats -- finite numbers, wrong norms, no error. The
        // second tensor's offset is what makes this observable.
        let (b, a) = diag_pair(4, 2, 3, &[3.0, 4.0]);
        let bytes = build(one_module(0, "q", &b, &a, Dtype::F32));
        let idx = parse_safetensors_index(Cursor::new(&bytes)).unwrap();
        let be = &idx.tensors["base_model.model.model.layers.0.q.lora_B.weight"];
        assert_eq!(be.start, (2 * 3 * 4) as u64, "B begins right after A's six f32s");
        // The whole file is exactly the header plus the payload, so the last tensor's
        // RELATIVE end plus data_start is the file length. If `start`/`end` were
        // absolute, this would overshoot by the header's length -- and reading at an
        // absolute `start` would decode header JSON bytes as floats: finite numbers,
        // wrong norms, no error anywhere.
        assert_eq!(bytes.len() as u64, idx.data_start + be.end);
        assert!(idx.data_start > be.end, "the header here is longer than the payload");
    }

    #[test]
    fn a_metadata_block_is_kept_and_is_not_mistaken_for_a_tensor() {
        let (b, a) = diag_pair(4, 2, 3, &[3.0, 4.0]);
        let mut bytes = build(one_module(0, "q", &b, &a, Dtype::F32));
        // Splice `__metadata__` into the header rather than rebuilding it.
        let hlen = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
        let header = String::from_utf8(bytes[8..8 + hlen].to_vec()).unwrap();
        let new_header =
            format!(r#"{{"__metadata__":{{"format":"pt"}},{}"#, header.strip_prefix('{').unwrap());
        let payload = bytes.split_off(8 + hlen);
        let mut out = Vec::new();
        out.extend_from_slice(&(new_header.len() as u64).to_le_bytes());
        out.extend_from_slice(new_header.as_bytes());
        out.extend_from_slice(&payload);
        let idx = parse_safetensors_index(Cursor::new(&out)).unwrap();
        assert_eq!(idx.tensors.len(), 2, "__metadata__ is not a tensor");
        assert_eq!(idx.metadata.get("format").map(String::as_str), Some("pt"));
    }

    #[test]
    fn a_shape_that_contradicts_data_offsets_is_refused() {
        let (b, a) = diag_pair(4, 2, 3, &[3.0, 4.0]);
        let bytes = build(one_module(0, "q", &b, &a, Dtype::F32));
        let hlen = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
        let header = String::from_utf8(bytes[8..8 + hlen].to_vec()).unwrap();
        let broken = header.replace(r#""shape":[2, 3]"#, r#""shape":[2, 9]"#);
        let e = parse_safetensors_header(&broken, 8 + hlen as u64).unwrap_err();
        assert!(matches!(e, LoraError::BadEntry { .. }), "got {e:?}");
    }

    #[test]
    fn an_undecodable_dtype_names_the_tensor_and_what_is_decodable() {
        let json = r#"{"t":{"dtype":"I64","shape":[2],"data_offsets":[0,16]}}"#;
        let e = parse_safetensors_header(json, 8).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("`t`") && msg.contains("I64") && msg.contains("BF16"), "got: {msg}");
    }

    #[test]
    fn a_header_length_beyond_the_ceiling_is_refused_not_allocated() {
        let mut bytes = u64::MAX.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        let e = parse_safetensors_index(Cursor::new(&bytes)).unwrap_err();
        assert!(matches!(e, LoraError::HeaderJson { .. }), "got {e:?}");
    }

    #[test]
    fn a_truncated_header_is_refused() {
        let bytes = 4096u64.to_le_bytes().to_vec();
        let e = parse_safetensors_index(Cursor::new(&bytes)).unwrap_err();
        assert!(matches!(e, LoraError::Truncated { .. }), "got {e:?}");
    }

    // -- end to end --------------------------------------------------------

    fn plain_config() -> AdapterConfig {
        parse_adapter_config(&cfg_json(""), Path::new("c")).unwrap()
    }

    #[test]
    fn a_whole_adapter_reads_to_the_stated_numbers() {
        // r = 2, alpha = 16, so scale = 8. B*A has singular values {4, 3}; dW's are
        // {32, 24} and ||dW||_F = 8 * 5 = 40.
        let (b, a) = diag_pair(4, 2, 3, &[3.0, 4.0]);
        let bytes = build(one_module(12, "self_attn.q_proj", &b, &a, Dtype::F32));
        let mut c = Cursor::new(bytes);
        let s = read_adapter(&mut c, plain_config(), Path::new("adapter_model.safetensors")).unwrap();
        assert_eq!(s.modules.len(), 1);
        let m = &s.modules[0];
        assert_eq!(m.layer, Some(12));
        assert_eq!(m.stack.as_deref(), Some("layers"));
        assert_eq!(m.module, "self_attn.q_proj");
        assert_eq!((m.r, m.in_features, m.out_features), (2, 3, 4));
        approx(m.scale, 8.0, 1e-12, "alpha/r with the MEASURED r");
        approx(m.delta_fro, 40.0, 1e-5, "||dW||_F");
        approx(m.singular_values[0], 32.0, 1e-5, "sigma_0 of dW");
        approx(m.singular_values[1], 24.0, 1e-5, "sigma_1 of dW");
        // The spectrum is {32, 24}, a scaling of {4, 3} -- and effective rank is scale
        // invariant, so it is the same 7 / (4^(4/7) * 3^(3/7)) as the unit test above.
        approx(m.effective_rank, 1.97962633, 1e-5, "effective rank");
        approx(m.stable_rank, 1.5625, 1e-5, "stable rank: (32^2 + 24^2) / 32^2");
        assert_eq!(s.rank_disagrees_with_config, vec!["base_model.model.model.layers.12.self_attn.q_proj"]);
    }

    #[test]
    fn f16_and_bf16_decode_to_the_same_answer_as_f32() {
        // Powers of two are exact in all three, so the ONLY thing that can differ is
        // the decode path -- which is the point. A bf16 read as f16 gives 32x.
        let (b, a) = diag_pair(4, 2, 3, &[2.0, 4.0]);
        let mut got = Vec::new();
        for dt in [Dtype::F32, Dtype::F16, Dtype::Bf16] {
            let bytes = build(one_module(0, "q", &b, &a, dt));
            let mut c = Cursor::new(bytes);
            let s = read_adapter(&mut c, plain_config(), Path::new("w")).unwrap();
            got.push(s.modules[0].delta_fro);
        }
        approx(got[0], 8.0 * (20.0f64).sqrt(), 1e-5, "f32");
        approx(got[1], got[0], 1e-5, "f16 matches f32");
        approx(got[2], got[0], 1e-5, "bf16 matches f32");
    }

    #[test]
    fn a_lone_factor_is_refused_and_the_message_says_which_half_is_there() {
        let (_, a) = diag_pair(4, 2, 3, &[3.0, 4.0]);
        let bytes = build(vec![(
            "base_model.model.layers.0.q_proj.lora_A.weight".into(),
            Dtype::F32,
            vec![2, 3],
            flat(&a),
        )]);
        let mut c = Cursor::new(bytes);
        let e = read_adapter(&mut c, plain_config(), Path::new("w")).unwrap_err();
        assert!(matches!(&e, LoraError::Unpaired { have: "lora_A", .. }), "got {e:?}");
        assert!(e.to_string().contains("lora_B"), "the message names the missing half");
    }

    #[test]
    fn factors_that_do_not_compose_are_refused() {
        let b = mat(4, 3, |_, _| 1.0);
        let a = mat(2, 3, |_, _| 1.0); // r = 2 vs 3
        let bytes = build(vec![
            ("m.layers.0.q.lora_A.weight".into(), Dtype::F32, vec![2, 3], flat(&a)),
            ("m.layers.0.q.lora_B.weight".into(), Dtype::F32, vec![4, 3], flat(&b)),
        ]);
        let mut c = Cursor::new(bytes);
        let e = read_adapter(&mut c, plain_config(), Path::new("w")).unwrap_err();
        assert!(matches!(e, LoraError::ShapeMismatch { .. }), "got {e:?}");
    }

    #[test]
    fn a_magnitude_vector_stops_the_read_even_when_the_config_did_not_say_dora() {
        let (b, a) = diag_pair(4, 2, 3, &[3.0, 4.0]);
        let mut entries = one_module(0, "q", &b, &a, Dtype::F32);
        entries.push((
            "base_model.model.model.layers.0.q.lora_magnitude_vector".into(),
            Dtype::F32,
            vec![4],
            vec![1.0; 4],
        ));
        let mut c = Cursor::new(build(entries));
        let e = read_adapter(&mut c, plain_config(), Path::new("w")).unwrap_err();
        assert!(matches!(e, LoraError::UnsupportedMethod { .. }), "got {e:?}");
    }

    #[test]
    fn tensors_that_are_not_factors_are_named_rather_than_dropped() {
        let (b, a) = diag_pair(4, 2, 3, &[3.0, 4.0]);
        let mut entries = one_module(0, "q", &b, &a, Dtype::F32);
        entries.push(("base_model.model.embed.lora_embedding_A".into(), Dtype::F32, vec![4], vec![0.5; 4]));
        let mut c = Cursor::new(build(entries));
        let s = read_adapter(&mut c, plain_config(), Path::new("w")).unwrap();
        assert_eq!(s.modules.len(), 1);
        assert_eq!(s.skipped, vec!["base_model.model.embed.lora_embedding_A"]);
    }

    #[test]
    fn an_adapter_with_no_factors_at_all_is_refused() {
        let bytes = build(vec![("something.weight".into(), Dtype::F32, vec![2], vec![1.0, 2.0])]);
        let mut c = Cursor::new(bytes);
        let e = read_adapter(&mut c, plain_config(), Path::new("w/adapter_model.safetensors"))
            .unwrap_err();
        assert!(matches!(e, LoraError::NoAdaptedModules { .. }), "got {e:?}");
        assert!(e.to_string().contains("adapter_model.safetensors"));
    }

    #[test]
    fn modules_come_back_ordered_by_layer_and_the_per_layer_norm_adds_in_quadrature() {
        // Layer 1 gets two modules with ||dW|| of 40 and 40*sqrt(2)... build it so the
        // arithmetic is checkable: both modules have ||B*A||_F = 5, scale 8, so each
        // ||dW||_F = 40 and the layer's block norm is sqrt(40^2 + 40^2) = 40*sqrt(2).
        let (b, a) = diag_pair(4, 2, 3, &[3.0, 4.0]);
        let mut entries = one_module(1, "self_attn.v_proj", &b, &a, Dtype::F32);
        entries.extend(one_module(1, "self_attn.q_proj", &b, &a, Dtype::F32));
        entries.extend(one_module(0, "self_attn.q_proj", &b, &a, Dtype::F32));
        let mut c = Cursor::new(build(entries));
        let s = read_adapter(&mut c, plain_config(), Path::new("w")).unwrap();
        let layers: Vec<Option<u32>> = s.modules.iter().map(|m| m.layer).collect();
        assert_eq!(layers, vec![Some(0), Some(1), Some(1)]);
        assert_eq!(s.modules[1].module, "self_attn.q_proj", "ties break by name");
        let per = s.per_layer_fro();
        approx(per[&0], 40.0, 1e-4, "layer 0 has one module");
        approx(per[&1], 40.0 * 2.0f64.sqrt(), 1e-4, "layer 1 adds in quadrature, not linearly");
        assert!(
            (per[&1] - 80.0).abs() > 1.0,
            "a linear sum would be 80 and would overstate every multi-module layer"
        );
    }

    #[test]
    fn alpha_pattern_reaches_the_module_and_is_reported() {
        let extra = r#","alpha_pattern":{"self_attn.q_proj":32}"#;
        let cfg = parse_adapter_config(&cfg_json(extra), Path::new("c")).unwrap();
        let (b, a) = diag_pair(4, 2, 3, &[3.0, 4.0]);
        let mut entries = one_module(0, "self_attn.q_proj", &b, &a, Dtype::F32);
        entries.extend(one_module(0, "self_attn.v_proj", &b, &a, Dtype::F32));
        let mut c = Cursor::new(build(entries));
        let s = read_adapter(&mut c, cfg, Path::new("w")).unwrap();
        let q = s.modules.iter().find(|m| m.module.ends_with("q_proj")).unwrap();
        let v = s.modules.iter().find(|m| m.module.ends_with("v_proj")).unwrap();
        assert!(q.alpha_from_pattern && !v.alpha_from_pattern);
        approx(q.delta_fro, 2.0 * v.delta_fro, 1e-4, "alpha 32 moves twice as far as alpha 16");
    }
}
