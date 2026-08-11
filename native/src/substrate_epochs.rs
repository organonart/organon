//! **The epoch ledger** — what the console's backdrop looked like, when it changed, and
//! which of those looks still deserve GPU memory.
//!
//! Console Spike **Tier 4, Leaf B**. The plan's row for this lane
//! (`doc/console_spike_execution_plan.md` §5, Tier 4) asks for *"a crude, honest, logged
//! GPU cap"*, and the **⚡ Sequence amendment** above §5's Tier 3 cuts it down to the shape
//! implemented here:
//!
//! * **substrate-look epochs only** — an epoch is a *look*, not a region, not a widget, not
//!   a patch of arbitrary content;
//! * **one cached texture per epoch, re-rendered on resize only** — the substrate is still
//!   (`substrate_scene.rs` zeroes every clock it can reach), so a rendered epoch is valid
//!   until the pane changes size;
//! * **`background world` collapses history to one live epoch** — [`EpochLedger::collapse_to`];
//! * **the cap is small, honest and logged** — [`MAX_EPOCHS`], and every eviction hands back
//!   an [`Evicted`] carrying the exact stderr line to print;
//! * **an evicted epoch merges into its neighbour** — [`EVICTION_SURVIVOR`] names which of
//!   the pair wins and why.
//!
//! # There is no restyle-everything path, deliberately
//!
//! Nothing in this module can change the look of an epoch that has already been opened.
//! There is no `restyle`, no `set_look`, no `&mut Look` handed out anywhere, and that is not
//! an omission to be filled in later — it is the design. A look applies **forward**; history
//! keeps the look it was written under. The plan states it in one line ("Do not build a
//! restyle-everything path"), and the honest reason is that the alternative is a lie: the
//! console cannot re-derive what a row *would* have looked like under a different rig
//! without re-rendering every band, which is exactly the unbounded GPU cost the cap exists
//! to prevent.
//!
//! The one *approximation* the module does make is the eviction merge, and it is confined to
//! the far end of scrollback, bounded to one epoch at a time, and logged every time it
//! happens. See [`EVICTION_SURVIVOR`].
//!
//! # What this module is not
//!
//! **It owns no GPU objects and imports no `wgpu`.** It decides *which* textures should
//! exist ([`EpochLedger::plan`] → [`SlotAction`]); the Tier 4 integrator executes those
//! decisions against the `Backdrop { texture, view, size, id }` lifecycle already standing in
//! `shell_main.rs` (brief **R1**: create-on-size-change, `register_native_texture` once, then
//! `update_egui_texture_from_wgpu_texture` against the carried `egui::TextureId`). One
//! retained epoch ⇒ one such `Backdrop`, keyed by [`EpochId`].
//!
//! **It owns no scroll geometry.** Boundaries are plain `u64` line numbers in whatever
//! absolute coordinate Tier 4 **Leaf A**'s scroll-anchor module defines. This module stores
//! them and hands them back via [`EpochLedger::boundaries`]; Leaf A consumes that as
//! `&[u64]` and turns offsets into rects. The two leaves are decoupled on purpose and share
//! exactly one type: `u64`.
//!
//! **It owns no look vocabulary.** A [`Look`] is two names — a material and a lighting rig —
//! and the ledger never interprets them, only compares them for equality and prints them.
//! The names come from Tier 2's material set and reach here through
//! `organon console background <name>`, i.e. from **user input**, which is why they are
//! owned `String`s rather than `&'static str`. Epochs store looks **by name and never by
//! `Shared` bytes**: the ledger is bookkeeping, not state. Re-deriving the snapshot from a
//! name is `substrate_scene`'s job, and keeping bytes here would be a second copy of a look
//! that could disagree with the first.
//!
//! ```text
//!   organon console background graphite        ← Tier 2's CLI
//!            │
//!            ▼
//!   ledger.open(Look::new("graphite", "soft-key"), current_line)
//!            │                    │
//!            │                    └──► Option<Evicted> ──► release texture + one stderr line
//!            ▼
//!   ledger.plan(held_ids, pane_resized) ──► [SlotAction] ──► the integrator's wgpu calls
//!            │
//!            └──► ledger.boundaries() ──► &[u64] ──► Leaf A's offset → rect
//! ```

use std::fmt;

// ===========================================================================
// The look — two names, compared by equality, never interpreted
// ===========================================================================

/// A substrate look: which material, under which lighting rig.
///
/// Both are **names**, not values. The ledger compares them (`==` decides whether an
/// [`EpochLedger::open`] is a real change or a no-op) and prints them (the eviction log
/// line). It never resolves them to `Shared` bytes — see the module doc.
///
/// Owned `String`s because the names arrive from `organon console background <name>`, so
/// `&'static str` cannot be assumed. The cost is two small allocations per epoch, bounded by
/// [`MAX_EPOCHS`], plus one clone per [`SlotAction`] that carries a look — deliberate, so a
/// plan can outlive the borrow on the ledger that produced it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Look {
    /// The material name — Tier 2's set (`graphite`, `paper`, `slate`, …).
    pub material: String,
    /// The lighting rig name — Tier 2 ships two.
    pub rig: String,
}

impl Look {
    /// Build a look from anything string-shaped.
    pub fn new(material: impl Into<String>, rig: impl Into<String>) -> Self {
        Self { material: material.into(), rig: rig.into() }
    }
}

impl fmt::Display for Look {
    /// `material/rig` — the spelling the eviction log line uses. Defined once here so the
    /// format string in [`Evicted::log_line`] and this cannot drift apart.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.material, self.rig)
    }
}

// ===========================================================================
// Epoch identity — stable across eviction, which the index is not
// ===========================================================================

/// A stable handle for one epoch, minted once when the epoch opens and never reused.
///
/// **The band index is not a usable key.** Evicting the oldest epoch shifts every remaining
/// index down by one, so an integrator keying its texture map by index would see every
/// eviction as "all of them changed". Ids do not shift. They are also **age-ordered**
/// (`Ord` on the inner counter is the order the epochs opened), which is what makes the
/// release list in [`EpochLedger::plan`] deterministic.
///
/// The counter is a `u64` incremented once per look change. It cannot realistically wrap;
/// if it somehow did, the failure is a duplicate key rather than a panic, and a session that
/// changed its backdrop 1.8 × 10¹⁹ times has other problems.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EpochId(pub u64);

impl fmt::Display for EpochId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// One epoch: a look, and the absolute line at which it **opened**.
///
/// An epoch runs from its own `boundary` up to the next epoch's `boundary` (exclusive), and
/// the newest epoch runs to the end of the scrollback. Boundaries are **strictly**
/// increasing across the ledger, so every epoch spans at least one line — there are no
/// zero-height epochs to produce a texture nobody can see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Epoch {
    /// Stable identity — see [`EpochId`].
    pub id: EpochId,
    /// What it looks like.
    pub look: Look,
    /// The absolute line at which this epoch opened, in Leaf A's coordinate.
    pub boundary: u64,
}

// ===========================================================================
// The cap
// ===========================================================================

/// How many epochs the ledger keeps, and therefore how many cached textures the integrator
/// holds at once. **Eight.**
///
/// # The arithmetic, not an adjective
///
/// One epoch costs one pane-sized `Rgba8UnormSrgb` texture — the same allocation
/// `shell_main.rs` already makes for the single live backdrop (brief R1). Four bytes per
/// pixel, no mips, one sample. [`worst_case_bytes`] computes the ceiling; the honest figures:
///
/// | pane | per epoch | × [`MAX_EPOCHS`] |
/// |---|---|---|
/// | 1920 × 1080 | 7.9 MiB | **63.3 MiB** |
/// | 2560 × 1440 | 14.1 MiB | **112.5 MiB** |
/// | 3840 × 2160 | 31.6 MiB | **253.1 MiB** |
///
/// 📌 A quarter of a gigabyte at 4K is **not** "tens of megabytes" — that estimate, made
/// before the multiply, is off by an order of magnitude, and the number is written here
/// rather than described so the next person reads the real one. It is still a fine cost on
/// the machine this spike targets (an RTX 5090: ~0.8 % of VRAM) and a noticeable one on a
/// 4 GB laptop GPU (~6 %). That asymmetry is the argument for a *small* cap.
///
/// # Why eight, specifically
///
/// 1. **Tier 4's beat check is three look changes in a session** (§7). A cap that binds
///    during the demo is a bug the audience sees, so the floor is comfortably above three.
/// 2. **The ceiling is one stateable sentence** — the table above. A cap whose cost you
///    cannot say in one line is not an honest cap.
/// 3. **Eight is small enough that the eviction path actually runs.** This is the real
///    argument. A cap of 200 would never fire in any session anyone plays, so the merge
///    logic would be dead code that has never executed outside a test — which is how a
///    "safety" limit turns out to be broken the first time it matters. Eight fires in a long
///    working session, and the log line makes it visible when it does.
pub const MAX_EPOCHS: usize = 8;

/// The GPU ceiling this cap buys, in bytes: [`MAX_EPOCHS`] pane-sized RGBA8 textures.
///
/// Saturating rather than wrapping, so a nonsense size returns a nonsense-but-monotone
/// number instead of a small one. The tests pin the 4K figure quoted in [`MAX_EPOCHS`]'s
/// table, which is what keeps that table from rotting.
pub const fn worst_case_bytes(pane_w: u32, pane_h: u32) -> u64 {
    (MAX_EPOCHS as u64)
        .saturating_mul(pane_w as u64)
        .saturating_mul(pane_h as u64)
        .saturating_mul(4)
}

// ===========================================================================
// Eviction — the merge, and which half of the pair survives
// ===========================================================================

/// Which epoch of a merged pair keeps its look, when the cap forces the two oldest to become
/// one. A named choice, because it is a **design decision with a visible consequence** and
/// not something to discover later by reading `remove(0)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeSurvivor {
    /// The older epoch survives and extends **forward** over the newer one's rows.
    Older,
    /// The newer epoch survives and extends **backward** over the older one's rows.
    Newer,
}

/// **The newer epoch survives.** This is the constant of the design.
///
/// When the ledger is full and a ninth look opens, the two oldest epochs merge into one. One
/// of the two looks has to be forgotten, and the rows it was written under get repainted in
/// the other. The choice is not symmetric:
///
/// * **Newer survives** (this): the survivor's boundary moves *back* to the evicted epoch's
///   boundary, so the oldest rows in the buffer — the ones furthest from the cursor, the
///   ones the user has to scroll a long way to reach — are repainted in the look that came
///   *next*. The drift is always toward the present.
/// * **Older survives** (the road not taken): the evicted epoch would be the *second*
///   oldest, and the surviving older look would extend forward over it. The repaint lands
///   closer to the live end of the buffer, and every subsequent eviction pushes it closer
///   still, so the oldest look in the session slowly eats the session. It also repaints rows
///   the user is more likely to be looking at.
///
/// Both are lossy — the cap makes that unavoidable — but only one of them concentrates the
/// loss where nobody is looking. `Newer` also has a second property worth naming: after any
/// number of evictions, the surviving looks are always the **most recently chosen** ones,
/// which is the set a user would pick if asked.
///
/// The alternative is not commented out; it is a real argument to a real function
/// ([`EpochLedger::merge_oldest_pair`]), exercised by a control test that demonstrates the
/// behaviour this constant rejects.
pub const EVICTION_SURVIVOR: MergeSurvivor = MergeSurvivor::Newer;

/// Why an epoch left the ledger. It changes what the log line should say, and a cap message
/// printed when the user typed `background world` would be actively misleading on stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvictReason {
    /// The ledger was full and a new epoch opened — [`MAX_EPOCHS`].
    Cap,
    /// [`EpochLedger::collapse_to`] forgot history: `background world`, or a fresh launch.
    Collapse,
}

/// One epoch that left the ledger. **The integrator's to-do item**, and the only way this
/// module ever reports that a texture became garbage.
///
/// Two things follow from receiving one of these, both the integrator's:
///
/// 1. **Release the texture** keyed by [`Evicted::id`] — the `Backdrop`'s `texture`, `view`
///    and the egui `TextureId` registration (`Renderer::free_texture`). The ledger cannot do
///    this; it holds no GPU objects.
/// 2. **Print [`Evicted::log_line`] to stderr**, one line, unconditionally. "Logged" is a
///    third of this lane's one-sentence brief, and a cap that silently drops history is
///    indistinguishable from a bug in the anchor arithmetic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evicted {
    /// The texture key to release.
    pub id: EpochId,
    /// What it looked like — for the log line.
    pub look: Look,
    /// The line it opened at — for the log line.
    pub boundary: u64,
    /// Cap pressure, or a deliberate collapse.
    pub reason: EvictReason,
}

impl Evicted {
    /// The exact stderr line, ready for `eprintln!("{}", ev.log_line())`.
    ///
    /// ```text
    /// [epochs] evicted graphite/soft-key @ line 1200 (cap 8)
    /// [epochs] evicted graphite/soft-key @ line 1200 (collapsed)
    /// ```
    ///
    /// Built here rather than described here, so the integrator cannot drift from the format
    /// and the cap number in the message cannot disagree with [`MAX_EPOCHS`]. Both spellings
    /// are pinned by a test against these literals. `[epochs]` is the tag to grep for.
    ///
    /// The [`EpochId`] is deliberately **not** in the line — it is noise to a human watching
    /// a demo — but it is on the struct if a texture-leak hunt ever wants a noisier one.
    pub fn log_line(&self) -> String {
        match self.reason {
            EvictReason::Cap => format!(
                "[epochs] evicted {} @ line {} (cap {})",
                self.look, self.boundary, MAX_EPOCHS
            ),
            EvictReason::Collapse => {
                format!("[epochs] evicted {} @ line {} (collapsed)", self.look, self.boundary)
            }
        }
    }

    fn from_epoch(e: Epoch, reason: EvictReason) -> Self {
        Self { id: e.id, look: e.look, boundary: e.boundary, reason }
    }
}

// ===========================================================================
// Opening an epoch
// ===========================================================================

/// What [`EpochLedger::open`] did. Every field is meaningful in both outcomes, so the
/// integrator never has to guess which ones to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenOutcome {
    /// `false` when **no epoch was opened**. Nothing was added, nothing was evicted, no
    /// texture work is required. Two causes, and the integrator's response to both is the
    /// same (do nothing), which is why they are one flag:
    ///
    /// 1. **The requested look was already the live one.** The real case: this is what stops
    ///    a repeated `organon console background graphite` from churning the GPU.
    /// 2. **The line coordinate is exhausted** — the live epoch already opened at
    ///    `u64::MAX`, so there is no strictly-later line to open at. Unreachable in
    ///    practice (1.8 × 10¹⁹ lines is ~585 million years of scrollback at 1000 lines a
    ///    second) but handled rather than assumed, because the alternative is a silently
    ///    non-monotone `boundaries()` — the one thing Leaf A must be able to trust.
    pub opened: bool,
    /// The live epoch's id: the new one when `opened`, the unchanged current one otherwise.
    pub id: EpochId,
    /// The live epoch's boundary — **after clamping**, which may be later than the `at_line`
    /// that was requested. See [`EpochLedger::open`].
    pub boundary: u64,
    /// The epoch the cap pushed out, if this open filled the ledger past [`MAX_EPOCHS`].
    /// At most one, because at most one epoch is added per call.
    pub evicted: Option<Evicted>,
}

// ===========================================================================
// Texture-slot decisions, as data
// ===========================================================================

/// One decision about one epoch's texture. Pure data — the integrator turns it into `wgpu`
/// calls; this module makes no `wgpu` calls and holds no `wgpu` types.
///
/// Produced by [`EpochLedger::plan`]. Every action names an [`EpochId`], which is the key
/// the integrator's texture map should use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotAction {
    /// This texture is orphaned — the epoch is gone. Free the texture, the view, and the
    /// egui registration.
    ///
    /// Releases are emitted **first** in the plan; see [`EpochLedger::plan`].
    Release {
        /// The texture key to free.
        id: EpochId,
    },
    /// No texture exists for this epoch yet. Allocate one at the current pane size and
    /// render `look` into it.
    Create {
        /// The texture key to create.
        id: EpochId,
        /// The look to render — cloned, so the plan outlives the borrow on the ledger.
        look: Look,
        /// This is the newest epoch: the one new rows are written under, and the one the
        /// user is looking at. Exactly one action per plan carries `live: true`.
        live: bool,
    },
    /// A texture exists but its contents are stale — the pane changed size, so it is the
    /// wrong resolution. Recreate at the new size and render `look` again.
    ///
    /// **Resize is the only thing that stales an epoch.** The substrate has no running
    /// clocks (`substrate_scene.rs`), and a look never changes in place (see the module
    /// doc), so a rendered epoch is otherwise valid forever.
    Rerender {
        /// The texture key to re-render.
        id: EpochId,
        /// The look to render — cloned, as for [`SlotAction::Create`].
        look: Look,
        /// See [`SlotAction::Create::live`].
        live: bool,
    },
    /// Nothing to do. Emitted rather than omitted so the plan is a **total** description of
    /// every slot: an integrator can rebuild its whole texture map from one plan and be
    /// certain it has not silently dropped one.
    Retain {
        /// The texture key that is already correct.
        id: EpochId,
        /// See [`SlotAction::Create::live`].
        live: bool,
    },
}

impl SlotAction {
    /// The epoch this action is about.
    pub fn id(&self) -> EpochId {
        match *self {
            SlotAction::Release { id }
            | SlotAction::Create { id, .. }
            | SlotAction::Rerender { id, .. }
            | SlotAction::Retain { id, .. } => id,
        }
    }

    /// Whether this action concerns the live (newest) epoch. Never true for
    /// [`SlotAction::Release`] — the live epoch is by construction still in the ledger.
    pub fn is_live(&self) -> bool {
        match *self {
            SlotAction::Release { .. } => false,
            SlotAction::Create { live, .. }
            | SlotAction::Rerender { live, .. }
            | SlotAction::Retain { live, .. } => live,
        }
    }

    /// Whether this action requires the substrate to be rendered — `Create` or `Rerender`.
    /// The integrator's frame budget cares about exactly this distinction.
    pub fn needs_render(&self) -> bool {
        matches!(self, SlotAction::Create { .. } | SlotAction::Rerender { .. })
    }
}

// ===========================================================================
// The ledger
// ===========================================================================

/// The ordered history of substrate looks: oldest first, newest last, never empty, never
/// longer than [`MAX_EPOCHS`].
///
/// # Invariants
///
/// Every one of these holds after **every** public operation, and the test sweep asserts
/// them after every step of a randomized interleaving:
///
/// 1. **Non-empty.** There is always exactly one *live* epoch. A console with a backdrop
///    always looks like something, so an empty ledger would be an unrepresentable state and
///    is therefore not representable — [`EpochLedger::new`] takes a look, and there is no
///    `Default`.
/// 2. **At most [`MAX_EPOCHS`].**
/// 3. **Boundaries strictly increase.** Hence no zero-height epoch, hence no texture that
///    covers no rows, and hence [`EpochLedger::boundaries`] is safe to binary-search without
///    tie-breaking.
/// 4. **No two adjacent epochs share a look** — guaranteed by the no-op rule in
///    [`EpochLedger::open`]. Two neighbours with one look would be one epoch paying for two
///    textures.
/// 5. **Ids strictly increase with age** — they are minted from a monotone counter and
///    eviction only ever removes, never reorders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochLedger {
    /// Oldest first. Never empty. See the invariants above.
    epochs: Vec<Epoch>,
    /// The next [`EpochId`] to mint.
    next_id: u64,
}

impl EpochLedger {
    /// The launch state: one epoch, one look, opening at `at_line`.
    ///
    /// There is no `Default` on purpose. A ledger without a look would have to invent one,
    /// and the look vocabulary belongs to Tier 2's material set — inventing a name here is
    /// exactly how a default drifts out of a table it was copied from.
    pub fn new(look: Look, at_line: u64) -> Self {
        Self { epochs: vec![Epoch { id: EpochId(0), look, boundary: at_line }], next_id: 1 }
    }

    /// Close the current epoch and open the next one at `at_line`.
    ///
    /// # The no-op rule
    ///
    /// If `look` equals the live epoch's look, **nothing happens** and the returned
    /// [`OpenOutcome::opened`] is `false`. A repeated `organon console background graphite`
    /// therefore costs nothing: no zero-height epoch, no second texture for one look, no
    /// churn. This is invariant 4 above.
    ///
    /// # The monotonicity rule
    ///
    /// Boundaries only ever grow. An `at_line` **at or before** the live epoch's boundary is
    /// clamped forward to `boundary + 1` — strictly forward, because clamping to `boundary`
    /// exactly would leave the previous epoch spanning zero lines. At `u64::MAX` the clamp
    /// has nowhere left to go, so the open is **declined** rather than allowed to put two
    /// epochs on one line — [`OpenOutcome::opened`] cause 2. The caller's line is
    /// hostile input in practice (a scrollback that was trimmed, a resize that re-wrapped, a
    /// look change dispatched before the prompt echoed), and a ledger whose boundaries can go
    /// backwards hands Leaf A a non-monotone `&[u64]` that maps two epochs to one row.
    ///
    /// ⚠️ **The degenerate case is real and is accepted, not hidden.** Two look changes with
    /// no output between them produce a one-line epoch holding a full-size texture. The
    /// alternative — replacing the live epoch's look in place when it has no content yet —
    /// was declined because the ledger cannot see content: it would be guessing that nothing
    /// was written, and the guess is wrong the moment a line arrives between the two calls.
    /// The cap reclaims the slot soon enough, and the eviction is logged, so the cost is
    /// bounded and visible rather than bounded and silent.
    ///
    /// # The cap
    ///
    /// If the new epoch takes the ledger past [`MAX_EPOCHS`], the two oldest merge and the
    /// loser comes back in [`OpenOutcome::evicted`] — release its texture and print its
    /// [`Evicted::log_line`]. At most one eviction per call, because at most one epoch is
    /// added per call. Which of the pair survives: [`EVICTION_SURVIVOR`].
    pub fn open(&mut self, look: Look, at_line: u64) -> OpenOutcome {
        let current = self.current();
        if current.look == look {
            return OpenOutcome {
                opened: false,
                id: current.id,
                boundary: current.boundary,
                evicted: None,
            };
        }

        let boundary = at_line.max(current.boundary.saturating_add(1));
        if boundary <= current.boundary {
            // Only reachable when the live boundary is already `u64::MAX`: the saturating
            // add has nowhere to go, and opening here would put two epochs on one line.
            // Decline, and keep `boundaries()` strictly monotone. See `OpenOutcome::opened`.
            return OpenOutcome {
                opened: false,
                id: current.id,
                boundary: current.boundary,
                evicted: None,
            };
        }
        let id = self.mint_id();
        self.epochs.push(Epoch { id, look, boundary });

        let evicted = if self.epochs.len() > MAX_EPOCHS {
            self.merge_oldest_pair(EVICTION_SURVIVOR, EvictReason::Cap)
        } else {
            None
        };

        OpenOutcome { opened: true, id, boundary, evicted }
    }

    /// Forget history: collapse to a single live epoch with `look`, spanning everything the
    /// ledger previously described.
    ///
    /// This is **`background world`** — switching the backdrop source away from the substrate
    /// means none of the cached substrate epochs describe what is on screen any more, so
    /// keeping them would be paying for textures that are now lies. It is also the shape of a
    /// fresh launch ([`EpochLedger::new`] applied to a running ledger).
    ///
    /// Every epoch that goes is returned, **oldest first**, each with
    /// [`EvictReason::Collapse`] so the log line says "collapsed" rather than blaming a cap
    /// that was not involved. Release every returned texture.
    ///
    /// The survivor's boundary is the **oldest** boundary in the ledger — the merge rule of
    /// [`EVICTION_SURVIVOR`] taken to its limit: the newest look wins and extends backward
    /// over all of it.
    ///
    /// # The no-churn case
    ///
    /// If the live epoch's look already equals `look`, that epoch **keeps its id** and its
    /// texture survives; only the older epochs are evicted. Same principle as
    /// [`EpochLedger::open`]'s no-op rule — a look that is already on screen should not cost
    /// a re-render. Otherwise a fresh id is minted and *every* prior epoch is evicted,
    /// including the live one, because its texture shows the wrong look.
    pub fn collapse_to(&mut self, look: Look) -> Vec<Evicted> {
        let base = self.epochs[0].boundary;
        let keep_live = self.current().look == look;

        let mut survivor = if keep_live {
            self.epochs.pop().expect("the ledger is never empty")
        } else {
            Epoch { id: self.mint_id(), look, boundary: base }
        };
        survivor.boundary = base;

        let evicted: Vec<Evicted> = self
            .epochs
            .drain(..)
            .map(|e| Evicted::from_epoch(e, EvictReason::Collapse))
            .collect();
        self.epochs.push(survivor);
        evicted
    }

    /// The open-lines, oldest first — **Leaf A's input**, consumed as `&[u64]`.
    ///
    /// Strictly increasing (invariant 3) and never empty (invariant 1), so a caller may
    /// binary-search it and may index `[0]` without a check.
    ///
    /// Allocates, because the boundaries are fields of `Epoch` rather than a separate array.
    /// A mirrored `Vec<u64>` kept in sync would return `&[u64]` for free — and would be a
    /// second copy of the same state, which is the drift this tree keeps being bitten by. At
    /// [`MAX_EPOCHS`] = 8 the allocation is 64 bytes; call it when the ledger changes or the
    /// pane resizes, not unconditionally every frame.
    pub fn boundaries(&self) -> Vec<u64> {
        self.epochs.iter().map(|e| e.boundary).collect()
    }

    /// How many epochs — equivalently, how many textures the integrator should be holding.
    /// Always `1..=MAX_EPOCHS`.
    ///
    /// Named `epoch_count` rather than `len` because there is no matching `is_empty`: the
    /// ledger is never empty, and a method that provably always returns `false` is dead
    /// weight.
    pub fn epoch_count(&self) -> usize {
        self.epochs.len()
    }

    /// The epoch drawn in band `idx`, oldest first — **index-aligned with
    /// [`EpochLedger::boundaries`]**, so band `i` runs from `boundaries()[i]` to
    /// `boundaries()[i + 1]` (or to the end of the buffer for the last).
    ///
    /// Total: `Some` for every `idx < epoch_count()`, `None` for every `idx` beyond. The
    /// renderer may call it for any index without a bounds check of its own.
    ///
    /// ⚠️ **Band indices are not stable across an eviction** — every index shifts down by
    /// one when the oldest epoch merges away. Only [`EpochId`] is stable; key textures by
    /// that. This accessor exists for the *draw* pass, where the index is the loop variable
    /// and the ledger has not moved since [`EpochLedger::plan`] was called.
    pub fn epoch_for_band(&self, idx: usize) -> Option<&Epoch> {
        self.epochs.get(idx)
    }

    /// The look drawn in band `idx` — [`EpochLedger::epoch_for_band`] with the field already
    /// selected, which is what the draw pass actually wants.
    pub fn look_for_band(&self, idx: usize) -> Option<&Look> {
        self.epochs.get(idx).map(|e| &e.look)
    }

    /// Every epoch, oldest first. For iteration; prefer the accessors above for lookups.
    pub fn epochs(&self) -> &[Epoch] {
        &self.epochs
    }

    /// The live epoch — the newest one, the one new rows are written under. Never `None`
    /// (invariant 1).
    pub fn current(&self) -> &Epoch {
        self.epochs.last().expect("the ledger is never empty")
    }

    /// The live look. See [`EpochLedger::current`].
    pub fn current_look(&self) -> &Look {
        &self.current().look
    }

    /// The live epoch's id. See [`EpochLedger::current`].
    pub fn current_id(&self) -> EpochId {
        self.current().id
    }

    /// Which band owns `line`, or `None` if `line` predates the oldest boundary the ledger
    /// still describes (everything older than that was merged away).
    ///
    /// A convenience, and honestly mostly a *testability* one: it is how the merge semantics
    /// of [`EVICTION_SURVIVOR`] become an assertion ("the rows under the evicted look now
    /// report the survivor's look") rather than a paragraph. The integrator may use it for
    /// logging.
    ///
    /// **Leaf A owns the real mapping.** It takes [`EpochLedger::boundaries`] and turns
    /// offsets into rects; if this and that ever disagree, `boundaries()` is the contract and
    /// this is the bug.
    pub fn band_for_line(&self, line: u64) -> Option<usize> {
        if line < self.epochs[0].boundary {
            return None;
        }
        Some(self.epochs.partition_point(|e| e.boundary <= line) - 1)
    }

    /// **The texture decisions**, as data. Pure: no `wgpu`, no allocation of anything the
    /// GPU can see, no mutation.
    ///
    /// `held` is the set of [`EpochId`]s the integrator currently has textures for — the
    /// integrator holds that truth because it holds the textures, which is why this module
    /// keeps no mirror of it and can never disagree. Duplicates and stale ids in `held` are
    /// tolerated: the result is a pure function of the *set*, and the emitted releases are
    /// deduplicated and id-ascending.
    ///
    /// `pane_resized` is the integrator's existing size-change test — the same condition
    /// `shell_main.rs` already computes as `backdrop.size != (w, h)` (brief R1). It is the
    /// **only** thing that stales a rendered epoch; see [`SlotAction::Rerender`].
    ///
    /// # The order is part of the contract
    ///
    /// 1. **Every [`SlotAction::Release`] first**, id-ascending. Freeing before allocating
    ///    means the transient peak never exceeds the cap — which matters precisely in the
    ///    case the cap exists for, where a create and a release land in the same plan.
    /// 2. **Then the live epoch**, so an integrator spending a per-frame render budget spends
    ///    it on the band the user is looking at. This is the frame where a look change has to
    ///    land "under a second, no flicker" (§7's Tier 2 beat).
    /// 3. **Then the remaining epochs, oldest first.**
    ///
    /// The plan is **total**: exactly one action per ledger epoch plus one per orphaned held
    /// id, so applying it leaves the integrator's key set equal to the ledger's. That
    /// property is asserted directly by the tests.
    pub fn plan(&self, held: &[EpochId], pane_resized: bool) -> Vec<SlotAction> {
        let mut out = Vec::with_capacity(self.epochs.len() + held.len());

        let mut orphans: Vec<EpochId> = held
            .iter()
            .copied()
            .filter(|id| !self.epochs.iter().any(|e| e.id == *id))
            .collect();
        orphans.sort_unstable();
        orphans.dedup();
        out.extend(orphans.into_iter().map(|id| SlotAction::Release { id }));

        let live = self.current_id();
        let action = |e: &Epoch| -> SlotAction {
            let live_flag = e.id == live;
            if !held.contains(&e.id) {
                SlotAction::Create { id: e.id, look: e.look.clone(), live: live_flag }
            } else if pane_resized {
                SlotAction::Rerender { id: e.id, look: e.look.clone(), live: live_flag }
            } else {
                SlotAction::Retain { id: e.id, live: live_flag }
            }
        };

        out.push(action(self.current()));
        for e in self.epochs.iter().filter(|e| e.id != live) {
            out.push(action(e));
        }
        out
    }

    /// Mint the next id. Monotone; never reused.
    fn mint_id(&mut self) -> EpochId {
        let id = EpochId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Merge the two oldest epochs into one, returning the loser.
    ///
    /// `None` when there are fewer than two epochs — nothing to merge, and the ledger's
    /// non-empty invariant means it must never merge itself away.
    ///
    /// `survivor` is a real argument rather than a hard-coded `remove(0)` so that
    /// [`EVICTION_SURVIVOR`]'s rejected alternative is *executable* and its consequence can
    /// be demonstrated by a control test instead of asserted in prose. Production always
    /// passes [`EVICTION_SURVIVOR`].
    fn merge_oldest_pair(
        &mut self,
        survivor: MergeSurvivor,
        reason: EvictReason,
    ) -> Option<Evicted> {
        if self.epochs.len() < 2 {
            return None;
        }
        let gone = match survivor {
            // The newer of the pair wins and extends BACKWARD: it inherits the older
            // epoch's boundary, so the rows written under the evicted look now render
            // under the survivor's.
            MergeSurvivor::Newer => {
                let base = self.epochs[0].boundary;
                let gone = self.epochs.remove(0);
                self.epochs[0].boundary = base;
                gone
            }
            // The older of the pair wins and extends FORWARD: it keeps its own boundary and
            // simply runs on to what is now the next epoch's.
            MergeSurvivor::Older => self.epochs.remove(1),
        };
        Some(Evicted::from_epoch(gone, reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(material: &str) -> Look {
        Look::new(material, "soft-key")
    }

    /// Every invariant from [`EpochLedger`]'s doc, checked mechanically. Called after every
    /// step of the interleaving sweep, and wherever else a whole-ledger check is cheaper
    /// than three assertions.
    fn assert_invariants(l: &EpochLedger) {
        // 1. non-empty, 2. capped
        assert!(l.epoch_count() >= 1, "the ledger must never be empty");
        assert!(
            l.epoch_count() <= MAX_EPOCHS,
            "the ledger held {} epochs, cap is {MAX_EPOCHS}",
            l.epoch_count()
        );

        let b = l.boundaries();
        assert_eq!(b.len(), l.epoch_count(), "boundaries() must be index-aligned with bands");

        for w in b.windows(2) {
            // 3. strictly increasing ⇒ no zero-height epoch
            assert!(w[1] > w[0], "boundaries must strictly increase, saw {w:?}");
        }
        for pair in l.epochs().windows(2) {
            // 4. no two adjacent epochs share a look
            assert_ne!(pair[0].look, pair[1].look, "adjacent epochs share a look");
            // 5. ids increase with age
            assert!(pair[1].id > pair[0].id, "ids must increase with age");
        }

        // epoch_for_band is total over the band range and nowhere else
        for i in 0..l.epoch_count() {
            assert!(l.epoch_for_band(i).is_some(), "band {i} must resolve");
            assert_eq!(l.look_for_band(i), l.epoch_for_band(i).map(|e| &e.look));
            assert_eq!(l.epoch_for_band(i).unwrap().boundary, b[i]);
        }
        assert!(l.epoch_for_band(l.epoch_count()).is_none(), "one past the end must be None");
        assert!(l.epoch_for_band(usize::MAX).is_none());

        // the live epoch is the newest
        assert_eq!(l.current().id, l.epochs()[l.epoch_count() - 1].id);
        assert_eq!(l.current_id(), l.current().id);
        assert_eq!(l.current_look(), &l.current().look);
    }

    // -----------------------------------------------------------------------------
    // (a) the launch state
    // -----------------------------------------------------------------------------

    #[test]
    fn new_opens_exactly_one_epoch_at_the_given_line() {
        let l = EpochLedger::new(look("graphite"), 40);
        assert_eq!(l.epoch_count(), 1);
        assert_eq!(l.boundaries(), vec![40]);
        assert_eq!(l.current_look(), &look("graphite"));
        assert_eq!(l.current_id(), EpochId(0));
        assert_invariants(&l);
    }

    // -----------------------------------------------------------------------------
    // (b) open — the contract, the no-op rule, monotonicity
    // -----------------------------------------------------------------------------

    #[test]
    fn open_closes_the_current_epoch_and_opens_the_next() {
        let mut l = EpochLedger::new(look("graphite"), 0);
        let out = l.open(look("paper"), 100);

        assert!(out.opened);
        assert_eq!(out.boundary, 100);
        assert_eq!(out.evicted, None);
        assert_eq!(l.epoch_count(), 2);
        assert_eq!(l.boundaries(), vec![0, 100]);
        assert_eq!(l.look_for_band(0), Some(&look("graphite")));
        assert_eq!(l.look_for_band(1), Some(&look("paper")));
        assert_eq!(l.current_id(), out.id);
        assert_ne!(out.id, EpochId(0), "a new epoch gets a fresh id");
        assert_invariants(&l);
    }

    #[test]
    fn opening_the_same_look_is_a_no_op() {
        let mut l = EpochLedger::new(look("graphite"), 0);
        let before = l.clone();

        let out = l.open(look("graphite"), 500);
        assert!(!out.opened, "an identical look must not open an epoch");
        assert_eq!(out.id, EpochId(0), "the live epoch is unchanged");
        assert_eq!(out.boundary, 0, "the live boundary is unchanged");
        assert_eq!(out.evicted, None);
        assert_eq!(l, before, "a no-op must leave the ledger byte-identical");

        // …and it stays a no-op however many times it is repeated — the CLI case.
        for _ in 0..50 {
            assert!(!l.open(look("graphite"), 9_999).opened);
        }
        assert_eq!(l, before);
        assert_invariants(&l);
    }

    /// Only the *live* look short-circuits. Returning to a look used earlier in the session
    /// is a real change: history is not restyled, so the old band keeps its own texture and
    /// the new band needs its own.
    #[test]
    fn reopening_an_older_look_is_not_a_no_op() {
        let mut l = EpochLedger::new(look("graphite"), 0);
        l.open(look("paper"), 10);
        let out = l.open(look("graphite"), 20);

        assert!(out.opened);
        assert_eq!(l.epoch_count(), 3);
        assert_ne!(l.epochs()[0].id, l.epochs()[2].id, "same look, two distinct epochs");
        assert_eq!(l.epochs()[0].look, l.epochs()[2].look);
        assert_invariants(&l);
    }

    #[test]
    fn open_clamps_a_backward_line_forward() {
        let mut l = EpochLedger::new(look("graphite"), 1_000);

        // Strictly before the live boundary.
        let a = l.open(look("paper"), 5);
        assert_eq!(a.boundary, 1_001, "clamped to live + 1");

        // Exactly at it — still clamped, because equality would be a zero-height epoch.
        let b = l.open(look("slate"), 1_001);
        assert_eq!(b.boundary, 1_002);

        // Forward is left alone.
        let c = l.open(look("graphite"), 9_000);
        assert_eq!(c.boundary, 9_000);

        assert_eq!(l.boundaries(), vec![1_000, 1_001, 1_002, 9_000]);
        assert_invariants(&l);
    }

    #[test]
    fn boundaries_stay_monotone_under_hostile_lines() {
        let mut l = EpochLedger::new(look("m0"), u64::MAX - 4);
        // Every one of these is at or behind the live boundary, including 0 and a repeat.
        let mut opened = vec![];
        for (i, line) in [0u64, 7, 0, u64::MAX, 3, u64::MAX].iter().enumerate() {
            opened.push(l.open(look(&format!("m{}", i + 1)), *line).opened);
            assert_invariants(&l);
        }
        let b = l.boundaries();
        for w in b.windows(2) {
            assert!(w[1] > w[0], "hostile input produced a non-monotone ledger: {b:?}");
        }
        // Saturation is a ceiling, not a wrap — the opens pile up against u64::MAX and the
        // ledger stays sorted rather than folding back to zero…
        assert_eq!(b, vec![u64::MAX - 4, u64::MAX - 3, u64::MAX - 2, u64::MAX - 1, u64::MAX]);
        // …and once the coordinate is exhausted the ledger DECLINES rather than doubling up
        // on the last line. That is `OpenOutcome::opened` cause 2, exercised.
        assert_eq!(opened, vec![true, true, true, true, false, false]);
    }

    /// The mechanical form of "there is no restyle-everything path": opening a new look
    /// leaves every older epoch's look and boundary exactly as it was.
    #[test]
    fn open_never_restyles_history() {
        let mut l = EpochLedger::new(look("graphite"), 0);
        l.open(look("paper"), 10);
        l.open(look("slate"), 20);
        let history: Vec<Epoch> = l.epochs()[..3].to_vec();

        l.open(look("velvet"), 30);
        assert_eq!(&l.epochs()[..3], &history[..], "an open must not touch history");

        // And there is no way to reach in and change one: `epochs()`, `epoch_for_band` and
        // `look_for_band` all hand out shared references, and nothing on the type takes a
        // band index and a look together. This assertion is the compiler's, not ours —
        // recorded here so the intent survives someone "helpfully" adding a setter.
    }

    // -----------------------------------------------------------------------------
    // (c) the cap, and the merge semantics
    // -----------------------------------------------------------------------------

    /// Fill to exactly the cap: nothing is evicted at `MAX_EPOCHS`, and the very next open
    /// evicts exactly one.
    #[test]
    fn the_cap_binds_at_exactly_max_epochs() {
        let mut l = EpochLedger::new(look("m0"), 0);
        for i in 1..MAX_EPOCHS {
            let out = l.open(look(&format!("m{i}")), (i as u64) * 100);
            assert_eq!(out.evicted, None, "no eviction below the cap (at {} epochs)", i + 1);
        }
        assert_eq!(l.epoch_count(), MAX_EPOCHS, "the ledger fills to exactly the cap");
        assert_invariants(&l);

        let out = l.open(look("over"), 10_000);
        let ev = out.evicted.expect("the cap must evict on the open past MAX_EPOCHS");
        assert_eq!(ev.reason, EvictReason::Cap);
        assert_eq!(ev.look, look("m0"), "the OLDEST epoch is the one evicted");
        assert_eq!(ev.boundary, 0);
        assert_eq!(ev.id, EpochId(0));
        assert_eq!(l.epoch_count(), MAX_EPOCHS, "and the ledger stays at the cap");
        assert_invariants(&l);

        // It keeps binding, one per open, forever.
        for i in 0..20 {
            let out = l.open(look(&format!("x{i}")), 20_000 + i);
            assert!(out.evicted.is_some(), "every open past the cap evicts exactly one");
            assert_eq!(l.epoch_count(), MAX_EPOCHS);
            assert_invariants(&l);
        }
    }

    /// The merge, asserted through the rows: what was written under the evicted look now
    /// renders under the **newer** neighbour's look, and the survivor's span reaches back to
    /// the evicted boundary.
    #[test]
    fn eviction_merges_the_oldest_pair_into_the_newer_look() {
        // Fill to exactly the cap: `new` + (MAX_EPOCHS - 1) opens.
        let mut l = EpochLedger::new(look("m0"), 0);
        for i in 1..MAX_EPOCHS {
            l.open(look(&format!("m{i}")), (i as u64) * 100);
        }
        assert_eq!(l.epoch_count(), MAX_EPOCHS);

        // Rows 0..100 are m0's; rows 100..200 are m1's — before the merge.
        assert_eq!(l.band_for_line(50).and_then(|b| l.look_for_band(b)), Some(&look("m0")));
        assert_eq!(l.band_for_line(150).and_then(|b| l.look_for_band(b)), Some(&look("m1")));

        let ev = l.open(look("last"), 10_000).evicted.expect("must evict");
        assert_eq!(ev.look, look("m0"));

        // THE merge assertion: m0's rows now display m1's look, and the oldest boundary is
        // m0's old one — the survivor inherited the span.
        assert_eq!(l.boundaries()[0], 0, "the survivor inherits the evicted boundary");
        assert_eq!(l.look_for_band(0), Some(&look("m1")), "the NEWER of the pair survives");
        assert_eq!(
            l.band_for_line(50).and_then(|b| l.look_for_band(b)),
            Some(&look("m1")),
            "rows written under the evicted look repaint in the survivor's"
        );
        assert_eq!(l.band_for_line(150).and_then(|b| l.look_for_band(b)), Some(&look("m1")));
        assert_invariants(&l);
    }

    /// The road not taken, executed. `MergeSurvivor::Older` is a real argument, so the
    /// behaviour [`EVICTION_SURVIVOR`] rejects can be demonstrated rather than described:
    /// the repaint lands on the *newer* rows instead of the oldest ones, and the oldest look
    /// in the session starts eating the session.
    #[test]
    fn the_rejected_survivor_repaints_newer_rows_instead() {
        assert_eq!(EVICTION_SURVIVOR, MergeSurvivor::Newer, "the design constant");

        let build = || {
            let mut l = EpochLedger::new(look("m0"), 0);
            l.open(look("m1"), 100);
            l.open(look("m2"), 200);
            l
        };

        let mut newer = build();
        let ev = newer.merge_oldest_pair(MergeSurvivor::Newer, EvictReason::Cap).unwrap();
        assert_eq!(ev.look, look("m0"), "Newer evicts the OLDEST");
        assert_eq!(newer.boundaries(), vec![0, 200]);
        assert_eq!(newer.look_for_band(0), Some(&look("m1")));
        // Rows 150 (m1's own) keep their look; only rows 0..100 changed.
        assert_eq!(newer.band_for_line(150).and_then(|b| newer.look_for_band(b)), Some(&look("m1")));

        let mut older = build();
        let ev = older.merge_oldest_pair(MergeSurvivor::Older, EvictReason::Cap).unwrap();
        assert_eq!(ev.look, look("m1"), "Older evicts the SECOND-oldest instead");
        assert_eq!(older.boundaries(), vec![0, 200]);
        assert_eq!(older.look_for_band(0), Some(&look("m0")), "the stale look survives");
        // …and it has eaten rows 100..200, which are NEWER than the ones Newer touched.
        assert_eq!(older.band_for_line(150).and_then(|b| older.look_for_band(b)), Some(&look("m0")));

        // Same row, two different answers: that difference is the whole decision.
        assert_ne!(
            newer.band_for_line(150).and_then(|b| newer.look_for_band(b)),
            older.band_for_line(150).and_then(|b| older.look_for_band(b))
        );
    }

    #[test]
    fn merging_refuses_to_empty_the_ledger() {
        let mut l = EpochLedger::new(look("only"), 7);
        assert_eq!(l.merge_oldest_pair(MergeSurvivor::Newer, EvictReason::Cap), None);
        assert_eq!(l.merge_oldest_pair(MergeSurvivor::Older, EvictReason::Cap), None);
        assert_eq!(l.epoch_count(), 1);
        assert_invariants(&l);
    }

    // -----------------------------------------------------------------------------
    // (d) collapse
    // -----------------------------------------------------------------------------

    #[test]
    fn collapse_evicts_all_but_one() {
        let mut l = EpochLedger::new(look("m0"), 30);
        for i in 1..5 {
            l.open(look(&format!("m{i}")), 30 + (i as u64) * 100);
        }
        assert_eq!(l.epoch_count(), 5);

        let evicted = l.collapse_to(look("world"));
        assert_eq!(l.epoch_count(), 1, "one live epoch, history forgotten");
        assert_eq!(l.current_look(), &look("world"));
        assert_eq!(l.boundaries(), vec![30], "the survivor covers the whole described span");

        assert_eq!(evicted.len(), 5, "a fresh look evicts every prior epoch, live included");
        let names: Vec<String> = evicted.iter().map(|e| e.look.material.clone()).collect();
        assert_eq!(names, vec!["m0", "m1", "m2", "m3", "m4"], "oldest first");
        assert!(evicted.iter().all(|e| e.reason == EvictReason::Collapse));
        assert_invariants(&l);
    }

    #[test]
    fn collapse_keeps_the_live_epoch_when_the_look_already_matches() {
        let mut l = EpochLedger::new(look("m0"), 30);
        l.open(look("m1"), 100);
        l.open(look("world"), 200);
        let live_id = l.current_id();

        let evicted = l.collapse_to(look("world"));
        assert_eq!(l.epoch_count(), 1);
        assert_eq!(l.current_id(), live_id, "the live texture survives — no churn");
        assert_eq!(l.boundaries(), vec![30], "but its span reaches back over the forgotten");
        assert_eq!(evicted.len(), 2, "only the older epochs go");
        assert!(evicted.iter().all(|e| e.id != live_id));
        assert_invariants(&l);
    }

    #[test]
    fn collapse_of_a_single_epoch_ledger_is_well_behaved() {
        // Same look: nothing goes, nothing changes.
        let mut same = EpochLedger::new(look("world"), 12);
        assert_eq!(same.collapse_to(look("world")), vec![]);
        assert_eq!(same.epoch_count(), 1);
        assert_eq!(same.boundaries(), vec![12]);
        assert_invariants(&same);

        // Different look: the one epoch is evicted and replaced, keeping the span.
        let mut diff = EpochLedger::new(look("graphite"), 12);
        let old_id = diff.current_id();
        let evicted = diff.collapse_to(look("world"));
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].id, old_id);
        assert_eq!(diff.epoch_count(), 1);
        assert_ne!(diff.current_id(), old_id, "a different look means a different texture");
        assert_eq!(diff.boundaries(), vec![12]);
        assert_invariants(&diff);
    }

    /// A collapsed ledger is a launch state: opening from it behaves identically to opening
    /// from `EpochLedger::new`. This is what lets `background world` → `background graphite`
    /// need no special case in the integrator.
    #[test]
    fn a_collapsed_ledger_behaves_like_a_fresh_one() {
        let mut collapsed = EpochLedger::new(look("m0"), 0);
        collapsed.open(look("m1"), 50);
        collapsed.collapse_to(look("world"));
        collapsed.open(look("graphite"), 500);

        assert_eq!(collapsed.epoch_count(), 2);
        assert_eq!(collapsed.boundaries(), vec![0, 500]);
        assert_eq!(collapsed.look_for_band(0), Some(&look("world")));
        assert_eq!(collapsed.look_for_band(1), Some(&look("graphite")));
        assert_invariants(&collapsed);
    }

    // -----------------------------------------------------------------------------
    // (e) band lookup
    // -----------------------------------------------------------------------------

    #[test]
    fn band_for_line_maps_rows_to_the_epoch_that_owns_them() {
        let mut l = EpochLedger::new(look("a"), 100);
        l.open(look("b"), 200);
        l.open(look("c"), 300);

        assert_eq!(l.band_for_line(0), None, "older than anything the ledger describes");
        assert_eq!(l.band_for_line(99), None);
        assert_eq!(l.band_for_line(100), Some(0), "a boundary belongs to the epoch it opens");
        assert_eq!(l.band_for_line(199), Some(0));
        assert_eq!(l.band_for_line(200), Some(1));
        assert_eq!(l.band_for_line(299), Some(1));
        assert_eq!(l.band_for_line(300), Some(2));
        assert_eq!(l.band_for_line(u64::MAX), Some(2), "the live epoch runs to the end");
    }

    // -----------------------------------------------------------------------------
    // (f) the plan
    // -----------------------------------------------------------------------------

    fn ids(actions: &[SlotAction]) -> Vec<EpochId> {
        actions.iter().map(|a| a.id()).collect()
    }

    #[test]
    fn plan_creates_everything_when_nothing_is_held() {
        let mut l = EpochLedger::new(look("a"), 0);
        l.open(look("b"), 10);
        let plan = l.plan(&[], false);

        assert_eq!(plan.len(), 2, "one action per epoch, nothing orphaned");
        assert!(plan.iter().all(|a| matches!(a, SlotAction::Create { .. })));
        assert!(plan.iter().all(|a| a.needs_render()));
        // The live epoch comes first.
        assert_eq!(plan[0].id(), l.current_id());
        assert!(plan[0].is_live());
    }

    #[test]
    fn plan_retains_what_is_held_and_releases_what_is_orphaned() {
        let mut l = EpochLedger::new(look("a"), 0);
        l.open(look("b"), 10);
        let held = [EpochId(0), EpochId(1), EpochId(99)];

        let plan = l.plan(&held, false);
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0], SlotAction::Release { id: EpochId(99) }, "orphan released first");
        assert!(plan[1..].iter().all(|a| matches!(a, SlotAction::Retain { .. })));
        assert!(plan[1..].iter().all(|a| !a.needs_render()), "nothing to render");
    }

    #[test]
    fn plan_rerenders_every_epoch_on_resize() {
        let mut l = EpochLedger::new(look("a"), 0);
        l.open(look("b"), 10);
        l.open(look("c"), 20);
        let held = [EpochId(0), EpochId(1), EpochId(2)];

        let plan = l.plan(&held, true);
        assert_eq!(plan.len(), 3);
        assert!(plan.iter().all(|a| matches!(a, SlotAction::Rerender { .. })));
        assert!(plan.iter().all(|a| a.needs_render()), "resize stales all of them");

        // …and nothing else does. Same ledger, same held set, no resize: no render at all.
        assert!(l.plan(&held, false).iter().all(|a| !a.needs_render()));
    }

    #[test]
    fn plan_releases_before_it_creates() {
        // The case the cap exists for: one create and one release in the same plan.
        let mut l = EpochLedger::new(look("m0"), 0);
        for i in 1..MAX_EPOCHS {
            l.open(look(&format!("m{i}")), (i as u64) * 100);
        }
        let held: Vec<EpochId> = l.epochs().iter().map(|e| e.id).collect();

        let out = l.open(look("new"), 10_000);
        let ev = out.evicted.expect("the cap must bind here");
        let plan = l.plan(&held, false);

        let release_at = plan.iter().position(|a| matches!(a, SlotAction::Release { .. }));
        let create_at = plan.iter().position(|a| matches!(a, SlotAction::Create { .. }));
        assert_eq!(release_at, Some(0), "the release must come first");
        assert!(create_at.unwrap() > release_at.unwrap(), "peak must never exceed the cap");
        assert_eq!(plan[0], SlotAction::Release { id: ev.id }, "and it is the evicted one");
    }

    #[test]
    fn plan_marks_exactly_one_epoch_live_and_it_is_the_newest() {
        let mut l = EpochLedger::new(look("a"), 0);
        l.open(look("b"), 10);
        l.open(look("c"), 20);

        for (held, resized) in [
            (&[][..], false),
            (&[EpochId(0)][..], false),
            (&[EpochId(0), EpochId(1), EpochId(2), EpochId(7)][..], true),
        ] {
            let plan = l.plan(held, resized);
            let live: Vec<EpochId> =
                plan.iter().filter(|a| a.is_live()).map(|a| a.id()).collect();
            assert_eq!(live, vec![l.current_id()], "exactly one live action, the newest");
            assert!(
                plan.iter().all(|a| !matches!(a, SlotAction::Release { .. }) || !a.is_live()),
                "a release is never live"
            );
        }
    }

    #[test]
    fn plan_tolerates_a_malformed_held_set() {
        let l = EpochLedger::new(look("a"), 0);
        // Duplicates, out of order, and ids that never existed.
        let held = [EpochId(9), EpochId(3), EpochId(9), EpochId(3), EpochId(0)];
        let plan = l.plan(&held, false);

        assert_eq!(
            ids(&plan),
            vec![EpochId(3), EpochId(9), EpochId(0)],
            "releases deduplicated and id-ascending, then the live epoch"
        );
        // The result is a function of the SET, not of the order it arrived in.
        let reordered = [EpochId(0), EpochId(9), EpochId(3)];
        assert_eq!(l.plan(&held, false), l.plan(&reordered, false));
    }

    /// Applying a plan to the held set must leave it exactly equal to the ledger's ids. This
    /// is the plan's totality property, and the reason an integrator can trust it as a
    /// complete description rather than a hint.
    fn apply_plan(held: &mut Vec<EpochId>, plan: &[SlotAction]) {
        for a in plan {
            match a {
                SlotAction::Release { id } => held.retain(|h| h != id),
                SlotAction::Create { id, .. } => {
                    if !held.contains(id) {
                        held.push(*id);
                    }
                }
                SlotAction::Rerender { .. } | SlotAction::Retain { .. } => {}
            }
        }
        held.sort_unstable();
    }

    #[test]
    fn applying_the_plan_makes_the_held_set_match_the_ledger() {
        let mut l = EpochLedger::new(look("a"), 0);
        l.open(look("b"), 10);
        l.open(look("c"), 20);

        let mut held = vec![EpochId(0), EpochId(42)];
        let plan = l.plan(&held, false);
        apply_plan(&mut held, &plan);

        let mut want: Vec<EpochId> = l.epochs().iter().map(|e| e.id).collect();
        want.sort_unstable();
        assert_eq!(held, want);
    }

    // -----------------------------------------------------------------------------
    // (g) the log line and the cap arithmetic
    // -----------------------------------------------------------------------------

    #[test]
    fn the_log_line_is_exactly_the_documented_format() {
        let cap = Evicted {
            id: EpochId(3),
            look: Look::new("graphite", "soft-key"),
            boundary: 1_200,
            reason: EvictReason::Cap,
        };
        assert_eq!(cap.log_line(), "[epochs] evicted graphite/soft-key @ line 1200 (cap 8)");

        let collapsed = Evicted { reason: EvictReason::Collapse, ..cap.clone() };
        assert_eq!(
            collapsed.log_line(),
            "[epochs] evicted graphite/soft-key @ line 1200 (collapsed)"
        );

        // The cap in the message is MAX_EPOCHS, not the literal 8 — change the cap and this
        // catches a stale message rather than shipping one.
        assert!(cap.log_line().ends_with(&format!("(cap {MAX_EPOCHS})")));
        assert!(cap.log_line().starts_with("[epochs] evicted "));
    }

    /// The table in [`MAX_EPOCHS`]'s doc, pinned. This is the whole "honest" half of
    /// "crude, honest, logged" — a cap whose cost is written down and checked.
    #[test]
    fn worst_case_bytes_matches_the_documented_table() {
        assert_eq!(MAX_EPOCHS, 8, "the doc table is written for a cap of 8");

        assert_eq!(worst_case_bytes(1920, 1080), 66_355_200);
        assert_eq!(worst_case_bytes(2560, 1440), 117_964_800);
        assert_eq!(worst_case_bytes(3840, 2160), 265_420_800);

        // 253.125 MiB at 4K — a quarter of a gigabyte, NOT "tens of megabytes".
        let mib = worst_case_bytes(3840, 2160) as f64 / (1024.0 * 1024.0);
        assert!((mib - 253.125).abs() < 1e-9, "4K ceiling drifted: {mib} MiB");
        assert!(mib > 100.0, "the doc's order-of-magnitude correction still holds");

        // One epoch is one pane-sized RGBA8 texture, and the cap is a plain multiple of it.
        assert_eq!(worst_case_bytes(1920, 1080), MAX_EPOCHS as u64 * 1920 * 1080 * 4);
        // Nonsense sizes saturate rather than wrap to something small.
        assert_eq!(worst_case_bytes(u32::MAX, u32::MAX), u64::MAX);
    }

    // -----------------------------------------------------------------------------
    // (h) the interleaving sweep
    // -----------------------------------------------------------------------------

    /// A deterministic LCG (Numerical Recipes constants). No `rand` dependency, and the same
    /// sequence on every machine and every run — a sweep that fails only sometimes is a
    /// sweep nobody fixes.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    /// Hostile interleavings of every operation, from several seeds, asserting the full
    /// invariant set after **every** step — plus the two cross-cutting properties that only
    /// show up over a sequence: the held set tracks the ledger through evictions, and the
    /// epoch count never exceeds the cap even transiently between calls.
    #[test]
    fn an_interleaved_sweep_holds_every_invariant() {
        // A small pool, so identical-look opens (the no-op path) actually occur.
        let pool = ["graphite", "paper", "slate", "velvet", "world"];

        for seed in [1u64, 7, 99, 2_718_281_828, u64::MAX / 3] {
            let mut rng = Lcg(seed);
            let mut l = EpochLedger::new(look(pool[0]), rng.below(50));
            let mut held: Vec<EpochId> = vec![l.current_id()];
            let mut evictions = 0usize;
            let mut noops = 0usize;

            for step in 0..400 {
                let name = pool[rng.below(pool.len() as u64) as usize];

                match rng.below(10) {
                    // 1 in 10: collapse — `background world`.
                    0 => {
                        let gone = l.collapse_to(look(name));
                        evictions += gone.len();
                        for e in &gone {
                            assert!(!e.log_line().is_empty());
                            assert_eq!(e.reason, EvictReason::Collapse);
                        }
                        assert_eq!(l.epoch_count(), 1, "collapse leaves exactly one epoch");
                    }
                    // otherwise: open, at a line that is often behind the live boundary.
                    _ => {
                        let at = match rng.below(4) {
                            0 => 0,                              // hostile: the very start
                            1 => l.current().boundary,           // hostile: exactly at it
                            2 => rng.below(100),                 // hostile: probably behind
                            _ => l.current().boundary + 1 + rng.below(500), // normal
                        };
                        let before_look = l.current_look().clone();
                        let before = l.clone();
                        let out = l.open(look(name), at);

                        if before_look == look(name) {
                            assert!(!out.opened, "same look must be a no-op");
                            assert_eq!(l, before, "a no-op must not mutate");
                            noops += 1;
                        } else {
                            assert!(out.opened);
                            assert_eq!(l.current_id(), out.id);
                            assert_eq!(l.current().boundary, out.boundary);
                            assert!(out.boundary > before.current().boundary, "forward only");
                        }
                        if let Some(e) = out.evicted {
                            evictions += 1;
                            assert_eq!(e.reason, EvictReason::Cap);
                            assert_eq!(l.epoch_count(), MAX_EPOCHS);
                        }
                    }
                }

                assert_invariants(&l);

                // The plan is total at every step: apply it and the held set is the ledger.
                let resized = rng.below(8) == 0;
                let plan = l.plan(&held, resized);
                assert_eq!(
                    plan.len(),
                    l.epoch_count() + held.iter().filter(|h| !l.epochs().iter().any(|e| e.id == **h)).count(),
                    "the plan must name every epoch and every orphan, step {step}"
                );
                apply_plan(&mut held, &plan);
                let mut want: Vec<EpochId> = l.epochs().iter().map(|e| e.id).collect();
                want.sort_unstable();
                assert_eq!(held, want, "held set diverged from the ledger at step {step}");

                // band_for_line is total over the described span at every step.
                let b = l.boundaries();
                assert_eq!(l.band_for_line(b[0]), Some(0));
                assert_eq!(l.band_for_line(u64::MAX), Some(l.epoch_count() - 1));
                if b[0] > 0 {
                    assert_eq!(l.band_for_line(b[0] - 1), None);
                }
            }

            // The sweep must actually exercise the paths it claims to, or it is 400 steps of
            // nothing. Both of these fired on every seed when this was written.
            assert!(evictions > 0, "seed {seed} never evicted — the sweep proved nothing");
            assert!(noops > 0, "seed {seed} never hit the no-op path");
        }
    }
}
