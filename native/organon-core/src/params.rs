//! The two param types the **algorithm** needs — and nothing else from `params.rs`.
//!
//! #626 Tier 3 PR B, resolving #536 T4 **reference #2**: `math.rs` →
//! `crate::params::{FuncName, ParamValues}`, which #536 called *"the one real design
//! decision"* in the whole crate split.
//!
//! ## Why this module is named `params`
//!
//! So that `math.rs`'s existing `use crate::params::{FuncName, ParamValues};` resolves
//! **verbatim** once `math.rs` lives here. A 31k-line file moving between crates is
//! already the riskiest edit in the tier; not also rewriting its imports is worth more
//! than a tidier module name.
//!
//! ## The decision, and the rule that forced it
//!
//! #536 recommended core own these enums behind an optional `host-params` feature that
//! adds nih-plug's `#[derive(Enum)]`, with a caveat about `wasm32` resolution. **That
//! caveat is dead** — #418 is parked, there is no wasm32 target — so the choice is made
//! on native merits, and the merits favour the other option:
//!
//! > **nih-plug must stay out of core *unconditionally*.** With `host-params`,
//! > `cargo tree -p organon-core` would be clean only for the default feature set —
//! > the tier's acceptance test would become a statement about one configuration
//! > rather than about the crate. An optional dependency is still a dependency.
//!
//! So [`FuncName`] is declared **plain** here, and `params.rs` keeps a host-facing
//! mirror carrying the derive. **The orphan rule is what makes this unavoidable rather
//! than merely preferable:** `organic-math-native` cannot
//! `impl nih_plug::Enum for organon_core::FuncName`, because both the trait and the
//! type would be foreign to it. There is no third option where one type serves both
//! sides — either core takes the derive, or the host side owns a mirror.
//!
//! **Core owns the semantic type; the host owns the adapter.** That is the right way
//! round: the duplication lives on the side that has the host concern.
//!
//! ⚠️ The mirror is pinned by a test asserting the two variant lists agree
//! **element-wise, by name, in both directions** — see `params.rs`. A length check
//! would pass a same-length *reordering*, which would silently misindex every saved
//! preset and every automation lane, because the index **is** the wire format.
//!
//! ## What deliberately did NOT move
//!
//! **`GeneratorMode` stays in `params.rs`** — all 27 variants, unduplicated. #536 and
//! #626 both list it as moving with `FuncName`, but reading `math.rs` shows its only
//! non-comment use there is a **test** of `from_u32`/`to_u32` round-tripping — which is
//! `params.rs`'s own `enum_u32_via_index!` machinery being tested from the wrong file.
//! The test moved to sit beside what it tests; the enum never had to move at all.
//!
//! `ipc.rs` likewise stays. #536 T4 reference #3 (`math.rs` → `crate::ipc::Shared`)
//! says it "resolves by co-location", but that reference is also **test-only**, so
//! co-location buys nothing. One relocated test resolved it.

use glam::Vec3;

/// A waveshaper applied to a phasor — a synth oscillator's wave family.
///
/// `Sin`/`Cos` are the smooth defaults; `Tan`/`Log` are exotic curves;
/// `Triangle`/`Square`/`Saw` are the classic synth shapes. All map phase → value and
/// slot into [`crate::math::apply_func`] wherever a function is used (translation
/// deformation + pendulum rotation).
///
/// ⚠️ **Variants are append-only and their ORDER IS THE WIRE FORMAT.** The index is
/// what `to_u32`/`from_u32` write into `Shared` and what presets store, so inserting or
/// reordering a variant silently repoints every saved preset and automation lane at a
/// different waveform. Append at the tail.
///
/// #626 T3: declared plain here (no `#[derive(Enum)]`) so that nih-plug stays out of
/// `organon-core` unconditionally. `params.rs`'s `HostFuncName` is the host-facing
/// mirror; a test there pins the two lists together.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum FuncName {
    Sin,
    Cos,
    Tan,
    Log,
    Triangle,
    Square,
    Saw,
}

impl FuncName {
    /// Every variant, **in declaration order** — which is index order, which is the
    /// wire format. This is the list `params.rs`'s pinning test compares against.
    pub const ALL: [FuncName; 7] = [
        FuncName::Sin,
        FuncName::Cos,
        FuncName::Tan,
        FuncName::Log,
        FuncName::Triangle,
        FuncName::Square,
        FuncName::Saw,
    ];

    /// The stable name for this variant. Matches the `#[name = "…"]` the host mirror
    /// carries, and the pinning test asserts they agree — so this is not a second
    /// source of truth, it is the thing the two are checked against.
    pub fn as_str(self) -> &'static str {
        match self {
            FuncName::Sin => "sin",
            FuncName::Cos => "cos",
            FuncName::Tan => "tan",
            FuncName::Log => "log",
            FuncName::Triangle => "triangle",
            FuncName::Square => "square",
            FuncName::Saw => "saw",
        }
    }

    pub fn to_u32(self) -> u32 {
        match self {
            FuncName::Sin => 0,
            FuncName::Cos => 1,
            FuncName::Tan => 2,
            FuncName::Log => 3,
            FuncName::Triangle => 4,
            FuncName::Square => 5,
            FuncName::Saw => 6,
        }
    }

    /// Unknown values fall back to `Sin` rather than panicking — the visual reads these
    /// out of a shared-memory block a differently-versioned writer may have filled.
    pub fn from_u32(v: u32) -> FuncName {
        match v {
            1 => FuncName::Cos,
            2 => FuncName::Tan,
            3 => FuncName::Log,
            4 => FuncName::Triangle,
            5 => FuncName::Square,
            6 => FuncName::Saw,
            _ => FuncName::Sin,
        }
    }
}

/// The vector-shaped numeric params consumed by [`crate::math::draw_tissue`].
///
/// #626 T3 PR B: moved here from `params.rs` with `FuncName`. It moves *free* — six
/// fields, all `glam::Vec3` or `f32`, with no reference to either param enum — which is
/// why #536 T4 called it out as the easy half of reference #2.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct ParamValues {
    pub loop_count: Vec3,
    pub loop_count_q: f32,
    pub rot_amp: Vec3,
    pub trans_amp: Vec3,
    pub trans_mod: Vec3,
    pub scale_amp: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn func_name_indices_round_trip() {
        for (i, f) in FuncName::ALL.iter().enumerate() {
            assert_eq!(f.to_u32(), i as u32, "{f:?} is not at its declared index");
            assert_eq!(FuncName::from_u32(i as u32), *f);
        }
    }

    #[test]
    fn unknown_func_index_falls_back_to_sin() {
        // The visual may read a Shared written by a differently-versioned plugin.
        assert_eq!(FuncName::from_u32(9999), FuncName::Sin);
        assert_eq!(FuncName::from_u32(7), FuncName::Sin);
    }

    #[test]
    fn func_name_all_is_complete_and_ordered() {
        assert_eq!(FuncName::ALL.len(), 7);
        let names: Vec<_> = FuncName::ALL.iter().map(|f| f.as_str()).collect();
        assert_eq!(names, ["sin", "cos", "tan", "log", "triangle", "square", "saw"]);
    }
}
